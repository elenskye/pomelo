//! 插件库 (v0.4 phase 6): skills 小拾 and Pomelo's other model features use.
//! - Built in (Rust): 书单检索 (books), 影单检索 (films), 联网搜索 (web). Each has 启用 and
//!   「小拾自己决定用」 (offered as a tool on every reply; off = only when a question asks for it with
//!   「/名称」 or the 联网 button), and its own keys (only in the keychain, KeyStore). Defaults: books /
//!   films on and automatic; web on, not automatic (it sends search words that may carry private
//!   content to Tavily). The switches live in picky.db `preferences` (`skills`).
//! - A question that asks for skills (runs.skills, kept for its new versions): they are offered even
//!   when not automatic; 联网 searches the question before the model is asked (chat.rs).
//! - Results are numbered web sources (cite.rs) saved with the run; their 「查了」 entries go along with
//!   later questions (data::WEB is never private).
//! - Elsewhere (`work`): 补全 and 一句话填好 of a book / film, and a 新书影 小拾 proposes, take the
//!   creator / country / 类型 from the lookup when the skill is on.
//! - v0.4.5 阶段 9: 读文件 (files.rs: the question's own files — outline, read, find) and 查知识库 (the
//!   conversation's 知识库, knowledge.rs), on and automatic by default, offered only where there is
//!   something to read (`Ctx`); chat.rs runs them (they need the question). Writing files is not a tool:
//!   the user saves a reply as Word and confirms changes to a file (work.rs).
//! - v0.7 阶段 ④: 「Seeker 资料」 (seeker.rs) reads Seeker and Rater; on and automatic by default, not
//!   offered when both are hidden; chat.rs runs it with the answer's `Access` (cloud, hidden).
//! - v0.7 阶段 ⑤: the three 题库 SKILLs (qbank.rs; 简历题 / 通用题 / 突击题) propose questions for Rater:
//!   they write (a card), so never for 定时任务; not offered when Rater is hidden.
//! - Not a tool, ever: starting / stopping models, 芝麻开门, import / export, backups (act/).

pub mod files;
pub mod qbank;
pub mod seeker;
pub mod toolbox;
pub mod web;
pub mod works;

use super::cite::Sources;
use super::data::{self, Answer};
use super::store::{pref_get, pref_set};
use crate::AppState;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tauri::{AppHandle, Manager, State};
use works::{Which, Work};

/// What goes before the results 联网 searched first (after the question).
pub const SEARCHED_FIRST: &str = "【用户要求联网：已经先搜了这个问题，结果如下，回答里用到时标出编号】";
pub const WEB_TOOL: &str = "pomelo_web_search";
pub const BOOKS: &str = "books";
pub const FILMS: &str = "films";
pub const WEB: &str = "web";
pub const FILES: &str = "files";
pub const KB: &str = "kb";
pub const KB_TOOL: &str = "pomelo_kb";
pub const WORD: &str = "word";
pub const TEXT: &str = "text";
pub const SEEKER: &str = "seeker";
pub const SEEKER_TOOL: &str = seeker::TOOL;
pub const QBANK_RESUME: &str = "qbank_resume";
pub const QBANK_GENERAL: &str = "qbank_general";
pub const QBANK_JOB: &str = "qbank_job";
const PREF: &str = "skills";
/// The contact Wikidata / Open Library get (the user sets it in the 插件库).
const CONTACT: &str = "skills.contact";
/// Titles one call looks up.
const TITLES: usize = 5;

pub struct KeySpec {
    pub account: &'static str,
    pub label: &'static str,
    /// Without it the skill can't work (else it only checks less).
    pub required: bool,
}

/// One node of a skill's flow (the 插件库 draws it): what happens, in order, with the checks and the
/// loops the code really has.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Flow {
    Step { text: &'static str },
    /// `ask`: the check; `yes`: its label and the steps on that side (`join`: the step it goes on at,
    /// "" = it ends there); `no`: the label of the way on down.
    Check { ask: &'static str, yes: &'static str, side: &'static [&'static str], join: &'static str, no: &'static str },
    /// Goes back to the step `back` while `text` says so.
    Again { text: &'static str, back: &'static str },
}

const fn s(text: &'static str) -> Flow {
    Flow::Step { text }
}

const fn check(ask: &'static str, yes: &'static str, side: &'static [&'static str], join: &'static str, no: &'static str) -> Flow {
    Flow::Check { ask, yes, side, join, no }
}

const fn again(text: &'static str, back: &'static str) -> Flow {
    Flow::Again { text, back }
}

/// 题库 SKILLs: a pending card's question (`replace`) or a bank question (`question`) first.
const REWRITE_PENDING: Flow = check("改卡片里还没确认的题？", "是", &["Rater 的模型按你的话重写那道题的答案", "换掉卡片里那一条"], "你勾选、确认", "否");
const CHECKED: &str = "检查模型的回答（素材、改自对得上，去掉重复，条数不超）";
const POLISH_BANK: Flow = check("说了题库里哪道题？", "是", &["按字找那道题（没找到或有几道 → 问你）", "Rater 的模型重写它的答案"], CHECKED, "否");
const QBANK_END: [Flow; 5] = [
    s(CHECKED),
    check("回答用不了？", "是", &["告诉小拾没出成"], "", "否"),
    s("放进确认卡片"),
    s("你勾选、确认"),
    s("写进题库（一步撤销）"),
];

pub struct Skill {
    pub key: &'static str,
    pub name: &'static str,
    pub tool: &'static str,
    /// Its flow (the 插件库 draws it as a chart).
    pub flow: &'static [Flow],
    /// What it can do (and what it sends where), one point each.
    pub features: &'static [&'static str],
    pub keys: &'static [KeySpec],
    pub auto: bool,
    /// It proposes writing files (a card the user confirms): never for 定时任务.
    pub writes: bool,
}

pub const SKILLS: &[Skill] = &[
    Skill {
        key: BOOKS,
        name: "general-book-lookup",
        tool: "pomelo_find_book",
        flow: &[
            s("书名（和作者）"),
            s("Wikidata 按书名找，作者对得上的优先"),
            check("Wikidata 没找到？", "是", &["Open Library 按书名找", "也没有就说查不到，不编"], "作者 · 国家 · 类型", "找到了"),
            s("Open Library 核对作者"),
            check("作者对不上？", "是", &["以 Wikidata 为准，注明不一样"], "作者 · 国家 · 类型", "对得上"),
            s("作者 · 国家 · 类型"),
        ],
        features: &["查书的作者、国家、类型，回答标出处", "书影面板的「补全」「一句话填好」", "小拾提议新书时补上作者、国家、类型", "发出去的只有书名（和作者）"],
        keys: &[],
        auto: true,
        writes: false,
    },
    Skill {
        key: FILMS,
        name: "general-film-lookup",
        tool: "pomelo_find_film",
        flow: &[
            s("名称（和导演）"),
            s("Wikidata 按名称找，导演对得上的优先"),
            check("Wikidata 没找到？", "是", &["TMDb（填了 Token）或 Jikan 按名称找", "也没有就说查不到，不编"], "导演 · 国家 · 类型", "找到了"),
            check("电影、剧集，填了 TMDb Token？", "是", &["TMDb 核对导演"], "导演 · 国家 · 类型", "否"),
            check("是动漫？", "是", &["Jikan 核对"], "导演 · 国家 · 类型", "否"),
            s("不核对，只用 Wikidata"),
            s("导演 · 国家 · 类型"),
        ],
        features: &["查电影、剧集、动漫的导演、国家、类型，回答标出处", "书影面板的「补全」「一句话填好」，小拾提议新影视时补上", "没填 TMDb Token 时电影、剧集只用 Wikidata", "发出去的只有名称（和导演）"],
        keys: &[KeySpec { account: works::TMDB_ACCOUNT, label: "TMDb Read Access Token", required: false }],
        auto: true,
        writes: false,
    },
    Skill {
        key: WEB,
        name: "general-web-search",
        tool: "pomelo_web_search",
        flow: &[
            s("问题"),
            check("用了斜杠命令或「联网」？", "是", &["先拿问题直接搜一次，结果和问题一起给小拾"], "小拾写搜索词", "否"),
            s("小拾写搜索词"),
            s("Tavily 搜索，读搜到的网页摘要"),
            again("不够就换个词再搜（一条回答最多查 4 轮）", "小拾写搜索词"),
            s("回答带来源"),
        ],
        features: &["上网查新的信息，回答带来源，点了用浏览器打开", "用斜杠命令时直接拿问题去搜（可能带上私人内容）", "搜到的存进对话，追问还用得上", "要填 Tavily API Key"],
        keys: &[KeySpec { account: web::ACCOUNT, label: "Tavily API Key", required: true }],
        auto: false,
        writes: false,
    },
    Skill {
        key: FILES,
        name: "general-file-reader",
        tool: files::TOOL,
        flow: &[
            s("这个问题带的文件"),
            check("放得进这次的篇幅？", "是", &["全文随问题发给模型"], "回答标出处", "放不下"),
            s("先给目录和按问题挑出的段落"),
            s("小拾按段号读 / 按字找"),
            again("不够就再读（一条回答最多查 4 轮）", "小拾按段号读 / 按字找"),
            s("回答标出处"),
        ],
        features: &["读附件和项目资料：看目录、按段号读、按字找", "资料太长时先给目录和按问题挑出的段落，再读需要的部分", "只读已经带上的文件，不碰别的文件", "内容只随这个问题发给你选的模型"],
        keys: &[],
        auto: true,
        writes: false,
    },
    Skill {
        key: KB,
        name: "general-knowledge-base",
        tool: KB_TOOL,
        flow: &[
            s("问题"),
            s("选库：对话 → 项目 → 全部"),
            check("用的是云端模型？", "是", &["跳过设成不发给云端的文件夹"], "找段落 / 列文件 / 读整篇", "否"),
            s("找段落 / 列文件 / 读整篇"),
            check("是图片或扫描件？", "是", &["只认文件名；云端模型能看图片"], "回答标出处", "否"),
            again("不够就再查（一条回答最多查 4 轮）", "找段落 / 列文件 / 读整篇"),
            s("回答标出处"),
        ],
        features: &["按问题在知识库里找相关的段落", "列出库里有哪些文件，整篇读一个文件（看目录、按段读、按字找）", "图片和扫描件只认文件名，云端模型能看图片", "用云端模型时不查不发给云端的文件夹"],
        keys: &[],
        auto: true,
        writes: false,
    },
    Skill {
        key: WORD,
        name: "general-word-toolkit",
        tool: toolbox::WORD_TOOL,
        flow: &[
            s("问题"),
            check("要新建一个 Word？", "是", &["名称和内容放进卡片", "你选文件夹、点「保存」才存"], "", "改已有的"),
            s("附件或知识库里的 Word，读成编号的段落"),
            s("小拾提出修改"),
            check("这一段改不了（表格、域，或格式对不上）？", "是", &["不放进卡片，告诉小拾为什么"], "小拾提出修改", "能改"),
            s("放进确认卡片（同一段再改就换掉）"),
            s("你勾选、点「写入文件」"),
            check("文件这之间被改过？", "是", &["不写，要重新读"], "", "没有"),
            s("写入文件（只动改了的字，不留备份）"),
        ],
        features: &["改文字和格式：加粗、斜体、下划线、标题、列表", "加段、删段", "新建一个 Word，你点「保存」才存", "只改附件和知识库里的 Word；不留备份，文件被改过就不写"],
        keys: &[],
        auto: true,
        writes: true,
    },
    Skill {
        key: TEXT,
        name: "general-text-toolkit",
        tool: toolbox::TEXT_TOOL,
        flow: &[
            s("附件或知识库里的 Markdown / 文本，读成编号的段落"),
            s("小拾提出修改"),
            s("放进确认卡片（同一段再改就换掉）"),
            s("你勾选、点「写入文件」"),
            check("文件这之间被改过？", "是", &["不写，要重新读"], "", "没有"),
            s("写入文件（保留原来的换行和编码，不留备份）"),
        ],
        features: &["改段、删段", "保留原来的换行和编码", "只改附件和知识库里的这类文件；不留备份，文件被改过就不写"],
        keys: &[],
        auto: true,
        writes: true,
    },
    Skill {
        key: SEEKER,
        name: "seeker-toolkit",
        tool: seeker::TOOL,
        flow: &[
            s("问到求职资料"),
            check("Seeker / Rater 隐藏了，或云端模型没被允许？", "是", &["不读，告诉小拾为什么"], "", "否"),
            s("挑一种查法：素材、草稿、导出、题库、练习、面试、cv/ 的文件、一份简历的正文"),
            s("只读启用的，按剩下的篇幅截短"),
            again("不够就换一种查法（一条回答最多查 4 轮）", "挑一种查法：素材、草稿、导出、题库、练习、面试、cv/ 的文件、一份简历的正文"),
            s("回答"),
        ],
        features: &["Seeker：素材和基本信息、中英文简历草稿、导出过的 Word 和用在哪个岗位", "cv/ 的简历（列出来、读一份正文）和 material/ 的文件名", "Rater：题库和练习记录、最近几轮卡片练习、面试准备和复盘", "停用的素材和规则、档案、网申不读；云端模型要 Seeker 允许发给云端"],
        keys: &[],
        auto: true,
        writes: false,
    },
    Skill {
        key: QBANK_RESUME,
        name: "rater-qbank-resume",
        tool: qbank::RESUME_TOOL,
        flow: &[
            s("小拾写出经历、题目里的字或卡片编号"),
            REWRITE_PENDING,
            POLISH_BANK,
            s("找经历：没说就用简历草稿里的（找不到 → 问你）"),
            s("读这些经历、经历档案和已有的简历题"),
            s("Rater 的模型照「简历题」工作流出题（一次调用）"),
            QBANK_END[0], QBANK_END[1], QBANK_END[2], QBANK_END[3], QBANK_END[4],
        ],
        features: &["按经历出面试官会追问的题，配口述稿答案", "给已有的简历题重写答案", "读选的经历（没说就是简历草稿里的）和经历档案", "照 Rater 规则里「简历题」的工作流，一步撤销", "用 Rater 选的模型写；云端模型要 Seeker 允许发给云端"],
        keys: &[],
        auto: true,
        writes: true,
    },
    Skill {
        key: QBANK_GENERAL,
        name: "rater-qbank-general",
        tool: qbank::GENERAL_TOOL,
        flow: &[
            s("小拾写出题目里的字或卡片编号"),
            REWRITE_PENDING,
            POLISH_BANK,
            s("读基本信息、已确认事实、经历一览和已有的通用题"),
            s("Rater 的模型照「通用题」工作流出题（一次调用）"),
            QBANK_END[0], QBANK_END[1], QBANK_END[2], QBANK_END[3], QBANK_END[4],
        ],
        features: &["自我介绍、为什么投、优缺点、行为题、职业规划、反问", "配口述稿答案", "照 Rater 规则里「通用题」的工作流，一步撤销", "用 Rater 选的模型写；云端模型要 Seeker 允许发给云端"],
        keys: &[],
        auto: true,
        writes: true,
    },
    Skill {
        key: QBANK_JOB,
        name: "rater-qbank-job",
        tool: qbank::JOB_TOOL,
        flow: &[
            s("小拾写出岗位、题目里的字或卡片编号"),
            REWRITE_PENDING,
            POLISH_BANK,
            check("投石问路里对不上一个岗位？", "是", &["没说、没找到或有几个 → 问你"], "", "对上了"),
            s("读 JD、knowledge/ 里这家公司的资料、已有的简历题和通用题"),
            s("Rater 的模型照「突击题」工作流出题，也改写已有的题（记着改自哪道）"),
            QBANK_END[0], QBANK_END[1], QBANK_END[2], QBANK_END[3], QBANK_END[4],
        ],
        features: &["按投石问路里一个岗位出突击题", "把已有的简历题、通用题按这家公司改写，记着改自哪道，原题不动", "照 Rater 规则里「突击题」的工作流，一步撤销", "用 Rater 选的模型写；云端模型要 Seeker 允许发给云端"],
        keys: &[],
        auto: true,
        writes: true,
    },
];

/// What a question has for the context-bound skills: files to read, a 知识库 to search.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ctx {
    pub files: bool,
    pub kb: bool,
    /// Markdown / text files 文本工具箱 may change.
    pub text: bool,
}

pub fn skill(key: &str) -> Option<&'static Skill> {
    SKILLS.iter().find(|s| s.key == key)
}

fn by_tool(tool: &str) -> Option<&'static Skill> {
    SKILLS.iter().find(|s| s.tool == tool)
}

pub fn is_tool(name: &str) -> bool {
    by_tool(name).is_some()
}

// ---------- switches ----------

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Switch {
    pub enabled: bool,
    pub auto: bool,
}

fn switches(conn: &Connection) -> HashMap<String, Switch> {
    pref_get::<HashMap<String, Switch>>(conn, PREF).ok().flatten().unwrap_or_default()
}

pub fn switch(conn: &Connection, key: &str) -> Switch {
    let d = skill(key).map_or(Switch { enabled: false, auto: false }, |s| Switch { enabled: true, auto: s.auto });
    switches(conn).get(key).copied().unwrap_or(d)
}

pub fn set_switch(conn: &Connection, key: &str, enabled: Option<bool>, auto: Option<bool>) -> Result<(), String> {
    skill(key).ok_or("没有这个技能")?;
    let mut all = switches(conn);
    let mut s = switch(conn, key);
    s.enabled = enabled.unwrap_or(s.enabled);
    s.auto = auto.unwrap_or(s.auto);
    all.insert(key.into(), s);
    pref_set(conn, PREF, &all)
}

/// Its required keys are there (mock mode needs none).
fn ready(p: &super::Picky, s: &Skill) -> bool {
    p.mode == super::Mode::Mock || s.keys.iter().filter(|k| k.required).all(|k| p.keys.has(k.account))
}

/// The skills a reply gets: on, ready, and automatic or asked for (`forced`: skill keys); 读文件 and
/// 查知识库 only where `ctx` has something for them.
pub fn offered(conn: &Connection, p: &super::Picky, forced: &[String], ctx: Ctx) -> Vec<&'static Skill> {
    SKILLS
        .iter()
        .filter(|s| (s.key != FILES || ctx.files) && (s.key != KB || ctx.kb) && (s.key != TEXT || ctx.text))
        .filter(|s| switch(conn, s.key).enabled && ready(p, s) && (switch(conn, s.key).auto || forced.iter().any(|f| f == s.key)))
        .collect()
}

/// Checks what a question asks for: known, on and ready (an error says which isn't).
pub fn checked_forced(conn: &Connection, p: &super::Picky, asked: &[String]) -> Result<Vec<String>, String> {
    let mut out = vec![];
    for k in asked {
        let s = skill(k).ok_or_else(|| format!("没有「{k}」这个技能"))?;
        if !switch(conn, s.key).enabled {
            return Err(format!("「{}」在插件库里关着", s.name));
        }
        if !ready(p, s) {
            return Err(format!("「{}」还没填 {}（小拾 → 插件库）", s.name, s.keys.iter().filter(|k| k.required).map(|k| k.label).collect::<Vec<_>>().join("、")));
        }
        if !out.contains(&s.key.to_string()) {
            out.push(s.key.to_string());
        }
    }
    Ok(out)
}

// ---------- tools and instructions ----------

pub fn tools(skills: &[&Skill]) -> Vec<Value> {
    let s = |d: &str| json!({ "type": "string", "description": d });
    let titles = |d: &str| json!({ "type": "array", "items": { "type": "string" }, "description": d });
    skills
        .iter()
        .map(|sk| {
            let (description, props, required): (&str, Value, Vec<&str>) = match sk.key {
                BOOKS => ("按书名查作者、国家、类型、出版年（Wikidata、Open Library）。补书的信息、核对推荐的书时用，一次可以查几本。", json!({ "titles": titles("书名（中文名或原名），最多 5 个"), "author": s("作者（知道的话，帮着分辨同名的书）") }), vec!["titles"]),
                FILMS => ("按名称查电影、剧集、动漫的导演、国家、类型、年份（Wikidata、TMDb、Jikan）。补影视的信息、核对推荐的片子时用，一次可以查几部。", json!({ "titles": titles("名称（中文名或原名），最多 5 个"), "director": s("导演（知道的话）") }), vec!["titles"]),
                FILES => return files::tool(),
                SEEKER => return seeker::tool(),
                QBANK_RESUME | QBANK_GENERAL | QBANK_JOB => return qbank::tool(sk.tool),
                WORD => return toolbox::tool(true),
                TEXT => return toolbox::tool(false),
                KB => (
                    "查用户的知识库（自己的文件夹和保存的网页）。view=search（默认）：按意思找和问题有关的段落，重排后最多 6 段；问到用户自己的资料、文档里的内容时先用它，可以换个说法再查。view=files：列出库里有哪些文件（keyword 按路径里的字筛）。view=outline：看某个文件的目录（每段的编号和开头）；view=read：按段号读这个文件的全文（from、to）；view=find：在这个文件里按字找（keyword）。要知道有哪些文件、要读完整的一份文档时用 files 再 outline / read。图片没有文字，按意思找不到：files 里标着「图片」，view=look 看一张（file 写它的路径；一次一张，要看几张就调用几次）。",
                    json!({
                        "view": { "type": "string", "enum": ["search", "files", "outline", "read", "find", "look"] },
                        "query": s("search：要找什么，一句话说清"),
                        "keyword": s("files：路径里的字；find：要找的字（空格隔开的几个字都要有）"),
                        "file": s("outline / read / find / look：文件的路径（files 列出的）"),
                        "from": { "type": "integer", "description": "read：从第几段（1 开始）" },
                        "to": { "type": "integer", "description": "read：到第几段（默认同 from）" }
                    }),
                    vec!["view"],
                ),
                _ => ("在网上搜（Tavily）。", json!({ "query": s("搜索词：说清要找什么，不要带用户的私人信息") }), vec!["query"]),
            };
            json!({ "type": "function", "function": { "name": sk.tool, "description": description, "parameters": { "type": "object", "properties": props, "required": required } } })
        })
        .collect()
}

/// The part of the system message about skills (empty when none is offered).
pub fn instructions(skills: &[&Skill], forced: &[String]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut s = String::from("【技能】查到的结果带编号 [n]，回答里用到时标出来；查不到的不要编。");
    for sk in skills {
        s.push_str(match sk.key {
            BOOKS => "\n- 书单检索（pomelo_find_book）：要补书的作者、国家、类型，或者推荐书的时候用：推荐的每一本先核对（真的有、作者对），再写进回答，标出处。",
            FILMS => "\n- 影单检索（pomelo_find_film）：电影、剧集、动漫同上。",
            FILES => "\n- 读文件（pomelo_file）：这个问题带的附件和项目资料；没有全部给你时先看目录，再按段号读需要的部分，用到的标出处。",
            WORD => "\n- Word 工具箱（pomelo_word）：用户要改 Word 文件时，read 读要改的部分，再调用 edit 把改好的整段文字放进确认卡片；要把写好的内容存成 Word 时用 create。只在回答里写改法不会进卡片：要改就一定调用 edit / create，它返回「已放进确认卡片」之后才能说放进去了；也不要说已经改好、已经存好（用户确认后才写）。只是问意见、没让改时不用 edit。",
            TEXT => "\n- 文本工具箱（pomelo_text）：用户要改 Markdown、纯文本文件时，read 之后调用 edit 提议改动，规矩同 Word 工具箱。",
            QBANK_RESUME | QBANK_GENERAL | QBANK_JOB => "",
            SEEKER => "\n- Seeker 资料（pomelo_seeker）：问到用户的简历、求职素材、cv/ 里的简历版本、导出过的简历、面试题库和练习时用，只读；回答里引用原文，不要改写成新的事实。",
            KB => "\n- 查知识库（pomelo_kb）：问到用户自己的文件、文档里的内容时先 search；问有哪些文件时用 files；要读完整的一份时 outline 再 read，用到的标出处；问到图片时用 files 找、look 看；查不到就说没查到。",
            _ => "\n- 联网搜索（pomelo_web_search）：问到最近的事、你不知道或可能过时的事时用；可以换个说法再搜一次。",
        });
    }
    let qbank: Vec<String> = skills.iter().filter_map(|sk| qbank::category(sk.tool).map(|c| format!("{} {c}", sk.tool))).collect();
    if !qbank.is_empty() {
        s.push_str(&format!("\n- 题库（{}）：用户要出面试题、写或改答案时用，出的题进确认卡片，确认后才写进 Rater 题库；不要自己在回答里写一整套题代替它。卡片里还没确认的题要改答案：同一个工具加上 replace（编号）和 request。", qbank.join("、")));
    }
    for f in forced.iter().filter_map(|f| skill(f)) {
        s.push_str(&if f.key == WEB {
            "\n- 这一条用户要求联网：已经用问题先搜了一次（结果在问题后面）；不够的话换个说法再用 pomelo_web_search 搜。".to_string()
        } else {
            format!("\n- 这一条用户要求用「{}」：先调用 {}，再回答。", f.name, f.tool)
        });
    }
    s
}

/// Whether books / films can be checked against the sources this time (data.rs's 推荐 line).
pub fn verifies(skills: &[&Skill]) -> bool {
    skills.iter().any(|s| s.key == BOOKS || s.key == FILMS)
}

// ---------- running a call ----------

fn mock(app: &AppHandle) -> bool {
    app.state::<AppState>().picky.mode == super::Mode::Mock
}

fn key_of(app: &AppHandle, account: &str) -> Option<String> {
    app.state::<AppState>().picky.keys.read(account).ok()
}

pub fn contact(conn: &Connection) -> Option<String> {
    pref_get::<String>(conn, CONTACT).ok().flatten().filter(|c| !c.is_empty())
}

/// An email or a web address (empty clears it).
pub fn set_contact(conn: &Connection, value: &str) -> Result<(), String> {
    let v = value.trim();
    if !v.is_empty() && (v.len() > 200 || !(v.contains('@') || v.starts_with("https://") || v.starts_with("http://")) || v.chars().any(|c| c.is_whitespace() || c.is_control())) {
        return Err("填一个邮箱或网址".into());
    }
    pref_set(conn, CONTACT, &v)
}

fn reach(app: &AppHandle) -> works::Reach {
    let st = app.state::<AppState>();
    works::Reach { tmdb: key_of(app, works::TMDB_ACCOUNT), contact: st.picky.with_db(|c| Ok(contact(c))).ok().flatten() }
}

fn answer(label: String, state: &'static str, text: String) -> Answer {
    Answer { module: Some(data::WEB.into()), label, state, text, records: vec![] }
}

fn clip(s: &str, n: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > n { format!("{}…", flat.chars().take(n).collect::<String>()) } else { flat }
}

/// One work as the model reads it.
pub fn work_line(which: Which, w: &Work) -> String {
    let who = if which == Which::Books { "作者" } else { "导演" };
    let name = if which == Which::Books { format!("《{}》", w.title) } else { w.title.clone() };
    let bits: Vec<String> = [
        w.original.as_ref().map(|o| format!("原名 {o}")),
        w.creator.as_ref().map(|c| format!("{who} {c}")),
        w.country.as_ref().map(|c| format!("国家 {c}")),
        w.kind.as_ref().map(|k| format!("类型 {k}")),
        w.year.as_ref().map(|y| format!("{y} 年")),
    ]
    .into_iter()
    .flatten()
    .collect();
    format!("{name}：{}。核对：{}", bits.join("；"), w.note)
}

/// Runs a skill's call (the model asked for it): numbered sources, text for the model.
pub async fn run(app: &AppHandle, sources: &mut Sources, tool: &str, arguments: &str, budget: i64) -> Answer {
    run_with(mock(app), key_of(app, web::ACCOUNT), reach(app), sources, tool, arguments, budget).await
}

/// `run` with the mode and keys given (tests use the mock).
pub async fn run_with(mock: bool, tavily: Option<String>, reach: works::Reach, sources: &mut Sources, tool: &str, arguments: &str, budget: i64) -> Answer {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let Some(sk) = by_tool(tool).filter(|s| ![FILES, KB, SEEKER, QBANK_RESUME, QBANK_GENERAL, QBANK_JOB].contains(&s.key)) else { return answer("技能 · 没有这个技能".into(), "failed", "没有这个技能。".into()) };
    if sk.key == WEB {
        let q = args["query"].as_str().unwrap_or("").trim().to_string();
        if q.is_empty() {
            return answer("联网搜索 · 没有搜索词".into(), "failed", "没搜：query 不能为空。".into());
        }
        return match web::search(tavily, &q, mock).await {
            Ok(pages) => web_results(&q, &pages, sources, budget),
            Err(e) => answer(format!("联网搜索「{}」· 没搜成", clip(&q, 20)), "failed", format!("没搜成：{e}")),
        };
    }
    let which = if sk.key == BOOKS { Which::Books } else { Which::Films };
    let titles: Vec<String> = match &args["titles"] {
        Value::Array(v) => v.iter().filter_map(Value::as_str).map(String::from).collect(),
        Value::String(s) => vec![s.clone()],
        _ => args["title"].as_str().map(|s| vec![s.to_string()]).unwrap_or_default(),
    };
    let titles: Vec<String> = titles.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).take(TITLES).collect();
    if titles.is_empty() {
        return answer(format!("{} · 没有名称", if which == Which::Books { "书单检索" } else { "影单检索" }), "failed", "没查：titles 不能为空。".into());
    }
    let hint = args[if which == Which::Books { "author" } else { "director" }].as_str().map(String::from);
    // side by side (Wikidata's pacing still holds its own requests apart)
    let tasks: Vec<_> = titles
        .iter()
        .map(|t| {
            let (t, hint, reach) = (t.clone(), hint.clone(), reach.clone());
            tauri::async_runtime::spawn(async move { works::find(which, &t, hint.as_deref(), reach, mock).await })
        })
        .collect();
    let mut found = vec![];
    for (t, task) in titles.iter().zip(tasks) {
        found.push((t.clone(), task.await.unwrap_or_else(|_| Err("查的时候出错了".into()))));
    }
    works_results(which, &found, sources, budget)
}

/// What the model gets for web pages, while they fit `budget`.
pub fn web_results(q: &str, pages: &[web::Page], sources: &mut Sources, budget: i64) -> Answer {
    let budget = budget.min(data::MAX_RESULT);
    let mut lines = vec![if pages.is_empty() { format!("网上没搜到和「{q}」有关的。") } else { format!("网上搜「{q}」找到的（按相关程度排）：") }];
    let mut used = super::context::estimate(&lines[0]);
    for p in pages {
        let site = web::site(&p.url);
        let text = clip(&p.content, 300);
        let n = sources.peek_web(&p.url);
        let line = format!("[{n}] {}（{site}）：{text}", clip(&p.title, 60));
        let cost = super::context::estimate(&line) + 1;
        if used + cost > budget {
            break;
        }
        used += cost;
        sources.web(&p.url, &p.title, &site, &text);
        lines.push(line);
    }
    answer(format!("联网搜索「{}」· {} 条", clip(q, 20), lines.len() - 1), "ok", lines.join("\n"))
}

fn works_results(which: Which, found: &[(String, Result<Option<Work>, String>)], sources: &mut Sources, budget: i64) -> Answer {
    let budget = budget.min(data::MAX_RESULT);
    let mut lines = vec![];
    let mut hits = 0;
    let mut used = 0;
    for (t, r) in found {
        let line = match r {
            Ok(Some(w)) => {
                let n = sources.web(&w.url, &if which == Which::Books { format!("《{}》", w.title) } else { w.title.clone() }, &w.site, &work_line(which, w));
                hits += 1;
                format!("[{n}] {}", work_line(which, w))
            }
            Ok(None) => format!("「{t}」：没查到（{}都没有这一{}）。不要编它的信息。", if which == Which::Books { "Wikidata、Open Library " } else { "Wikidata、TMDb、Jikan " }, if which == Which::Books { "本" } else { "部" }),
            Err(e) => format!("「{t}」：没查成（{e}）。"),
        };
        used += super::context::estimate(&line) + 1;
        if used > budget {
            break;
        }
        lines.push(line);
    }
    let first = clip(&found.first().map(|f| f.0.clone()).unwrap_or_default(), 12);
    let what = if which == Which::Books { "书单检索" } else { "影单检索" };
    let label = if found.len() > 1 { format!("{what}「{first}」等 {} {}", found.len(), if which == Which::Books { "本" } else { "部" }) } else { format!("{what}「{first}」") };
    answer(format!("{label} · 查到 {hits} 个"), if hits > 0 { "ok" } else { "failed" }, lines.join("\n"))
}

// ---------- elsewhere: 补全, 一句话填好, 新书影 ----------

/// A book / film looked up for a panel or a proposal, when its skill is on; None = off or not found.
pub async fn work(app: &AppHandle, module: &str, title: &str, hint: Option<&str>) -> Option<Work> {
    let key = if module == "books" { BOOKS } else { FILMS };
    let st = app.state::<AppState>();
    if !st.picky.with_db(|c| Ok(switch(c, key).enabled)).unwrap_or(false) {
        return None;
    }
    let which = if module == "books" { Which::Books } else { Which::Films };
    works::find(which, title, hint, reach(app), mock(app)).await.ok().flatten()
}

// ---------- the library page ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    pub account: &'static str,
    pub label: &'static str,
    pub required: bool,
    pub saved: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub key: &'static str,
    pub name: &'static str,
    pub command: String,
    pub flow: &'static [Flow],
    pub features: &'static [&'static str],
    pub enabled: bool,
    pub auto: bool,
    pub ready: bool,
    pub keys: Vec<KeyInfo>,
    /// Books / films send the contact to Wikidata / Open Library (one setting for both).
    pub uses_contact: bool,
    pub contact: Option<String>,
}

fn infos(conn: &Connection, p: &super::Picky) -> Vec<SkillInfo> {
    SKILLS
        .iter()
        .map(|s| {
            let sw = switch(conn, s.key);
            SkillInfo {
                key: s.key,
                name: s.name,
                command: format!("/{}", s.name),
                flow: s.flow,
                features: s.features,
                enabled: sw.enabled,
                auto: sw.auto,
                ready: ready(p, s),
                keys: s.keys.iter().map(|k| KeyInfo { account: k.account, label: k.label, required: k.required, saved: p.keys.has(k.account) }).collect(),
                uses_contact: s.key == BOOKS || s.key == FILMS,
                contact: contact(conn),
            }
        })
        .collect()
}

fn key_spec(account: &str) -> Result<&'static KeySpec, String> {
    SKILLS.iter().flat_map(|s| s.keys.iter()).find(|k| k.account == account).ok_or_else(|| "没有这个 Key".to_string())
}

#[tauri::command]
pub fn picky_skills(state: State<AppState>) -> Result<Vec<SkillInfo>, String> {
    state.picky.with_db(|c| Ok(infos(c, &state.picky)))
}

#[tauri::command]
pub fn picky_skill_set(state: State<AppState>, key: String, enabled: Option<bool>, auto: Option<bool>) -> Result<Vec<SkillInfo>, String> {
    state.picky.with_db(|c| {
        set_switch(c, &key, enabled, auto)?;
        Ok(infos(c, &state.picky))
    })
}

#[tauri::command]
pub fn picky_skill_key_save(state: State<AppState>, account: String, key: String) -> Result<Vec<SkillInfo>, String> {
    let spec = key_spec(&account)?;
    state.picky.keys.save(spec.account, &key, &format!("Pomelo 小拾 · {}", spec.label))?;
    state.picky.with_db(|c| Ok(infos(c, &state.picky)))
}

#[tauri::command]
pub fn picky_skill_contact_set(state: State<AppState>, contact: String) -> Result<Vec<SkillInfo>, String> {
    state.picky.with_db(|c| {
        set_contact(c, &contact)?;
        Ok(infos(c, &state.picky))
    })
}

#[tauri::command]
pub fn picky_skill_key_remove(state: State<AppState>, account: String) -> Result<Vec<SkillInfo>, String> {
    let spec = key_spec(&account)?;
    state.picky.keys.remove(spec.account)?;
    state.picky.with_db(|c| Ok(infos(c, &state.picky)))
}

// ---------- mock model ----------

/// The mock model's skill calls: what the question asks for (「/联网搜索」 → the instructions say so),
/// or 「查一下《X》」 / 「X 的导演是谁」.
pub fn mock_calls(question: &str, system: &str) -> Vec<(&'static str, Value)> {
    let title = question.split_once('《').and_then(|(_, r)| r.split_once('》')).map(|(t, _)| t.to_string());
    let q = question.trim().to_string();
    let mut out = vec![];
    let film = ["电影", "导演", "片", "剧", "动漫", "看"].iter().any(|w| q.contains(w));
    let forced_book = system.contains("先调用 pomelo_find_book");
    let forced_film = system.contains("先调用 pomelo_find_film");
    let asked = title.is_some() && ["查一下", "作者", "导演", "是谁", "哪国"].iter().any(|w| q.contains(w));
    if forced_book || forced_film || asked {
        let titles = json!([title.unwrap_or_else(|| q.clone())]);
        if forced_film || (!forced_book && film) {
            out.push(("pomelo_find_film", json!({ "titles": titles })));
        } else {
            out.push(("pomelo_find_book", json!({ "titles": titles })));
        }
    }
    // 「按柚子银行的 JD 出 5 道突击题」「补 3 道通用题」: the 题库 SKILLs (job: the words between 按 and 的)
    if (q.contains('出') || q.contains('补')) && q.contains("道") && q.contains('题') {
        let count: Option<u64> = q.split('道').next().map(|s| s.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect::<String>()).and_then(|n| n.parse().ok());
        let job = q.split_once('按').and_then(|(_, r)| r.split_once('的')).map(|(j, _)| j.trim().to_string()).filter(|j| !j.is_empty());
        let (tool, mut args) = if q.contains("突击题") || q.contains("JD") {
            (qbank::JOB_TOOL, json!({ "job": job.unwrap_or_default() }))
        } else if q.contains("通用题") || q.contains("自我介绍") {
            (qbank::GENERAL_TOOL, json!({}))
        } else {
            (qbank::RESUME_TOOL, json!({}))
        };
        if let Some(n) = count {
            args["count"] = json!(n);
        }
        out.push((tool, args));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::store::tests::memory_db;

    /// Each flow starts and ends with a step, and a branch goes on at / a loop goes back to one of its
    /// own steps.
    #[test]
    fn every_flow_joins_its_own_steps() {
        for skill in SKILLS {
            let steps: Vec<&str> = skill.flow.iter().filter_map(|f| if let Flow::Step { text } = f { Some(*text) } else { None }).collect();
            assert!(matches!(skill.flow.first(), Some(Flow::Step { .. })) && matches!(skill.flow.last(), Some(Flow::Step { .. })), "{}", skill.name);
            for f in skill.flow {
                match f {
                    Flow::Check { join, side, .. } => assert!((join.is_empty() || steps.contains(join)) && !side.is_empty(), "{}: {join}", skill.name),
                    Flow::Again { back, .. } => assert!(steps.contains(back), "{}: {back}", skill.name),
                    Flow::Step { .. } => {}
                }
            }
        }
    }

    /// The flows as JSON for mockIpc.ts (`cargo test skill_flows_json -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn skill_flows_json() {
        for skill in SKILLS {
            println!("FLOW {} {}", skill.key, serde_json::to_string(skill.flow).unwrap());
        }
    }

    #[test]
    fn switches_default_and_change() {
        let c = memory_db();
        assert_eq!(switch(&c, BOOKS), Switch { enabled: true, auto: true });
        assert_eq!(switch(&c, WEB), Switch { enabled: true, auto: false });
        set_switch(&c, WEB, None, Some(true)).unwrap();
        set_switch(&c, BOOKS, Some(false), None).unwrap();
        assert_eq!((switch(&c, WEB), switch(&c, BOOKS).enabled), (Switch { enabled: true, auto: true }, false));
        assert!(set_switch(&c, "vault", Some(true), None).is_err());
        assert_eq!(contact(&c), None);
        set_contact(&c, " me@example.com ").unwrap();
        assert_eq!(contact(&c).as_deref(), Some("me@example.com"));
        assert!(set_contact(&c, "not a contact").is_err());
        set_contact(&c, "").unwrap();
        assert_eq!(contact(&c), None);
    }

    #[test]
    fn results_are_numbered_web_sources() {
        let mut s = Sources::default();
        let pages = vec![web::Page { title: "台风「海燕」登陆".into(), url: "https://news.example.com/a".into(), content: "今天下午……".into() }];
        let a = web_results("台风", &pages, &mut s, 4000);
        assert!(a.text.contains("[1] 台风「海燕」登陆（news.example.com）：今天下午") && a.module.as_deref() == Some(data::WEB), "{}", a.text);
        let again = web_results("台风", &pages, &mut s, 4000);
        assert!(again.text.contains("[1] ")); // the same page keeps its number
        assert_eq!(s.0.len(), 1);
        let w = tauri::async_runtime::block_on(works::find(Which::Books, "三体", None, works::Reach::default(), true)).unwrap();
        let found = vec![("三体".to_string(), Ok(w)), ("没有这本".to_string(), Ok(None))];
        let a = works_results(Which::Books, &found, &mut s, 4000);
        assert!(a.text.contains("[2] 《三体》：原名 The Three-Body Problem；作者 刘慈欣；国家 中国；类型 虚构；2006 年") && a.text.contains("「没有这本」：没查到"), "{}", a.text);
        assert_eq!(a.label, "书单检索「三体」等 2 本 · 查到 1 个");
        assert_eq!(s.0[1].url.as_deref(), Some("https://openlibrary.org/works/OL17267881W"));
    }

    #[test]
    fn tools_instructions_and_the_mock() {
        let all: Vec<&Skill> = SKILLS.iter().collect();
        let defs = tools(&all);
        assert_eq!(defs.len(), 11);
        assert!(defs.iter().any(|t| t["function"]["name"] == files::TOOL) && defs.iter().any(|t| t["function"]["name"] == KB_TOOL));
        // 读文件 / 查知识库 / 文本工具箱 only where the question has something for them; Word 工具箱 always
        // (it can also make a new document)
        let c = crate::picky::store::tests::memory_db();
        let dir = crate::db::tests::temp_dir("skills-ctx");
        let p = crate::picky::Picky::init(&crate::paths::DataPaths::new(dir.to_path_buf()), &dir, crate::picky::Mode::Mock);
        let keys = |ctx| offered(&c, &p, &[], ctx).iter().map(|s| s.key).collect::<Vec<_>>();
        assert_eq!(keys(Ctx::default()), [BOOKS, FILMS, WORD, SEEKER, QBANK_RESUME, QBANK_GENERAL, QBANK_JOB]);
        assert_eq!(keys(Ctx { files: true, kb: true, text: true }), [BOOKS, FILMS, FILES, KB, WORD, TEXT, SEEKER, QBANK_RESUME, QBANK_GENERAL, QBANK_JOB]);
        assert!(SKILLS.iter().filter(|s| s.writes).map(|s| s.key).eq([WORD, TEXT, QBANK_RESUME, QBANK_GENERAL, QBANK_JOB])); // never for 定时任务
        let text = instructions(&all, &[WEB.into(), BOOKS.into()]);
        assert!(text.contains("已经用问题先搜了一次") && text.contains("先调用 pomelo_find_book"));
        assert!(instructions(&[], &[]).is_empty());
        assert!(mock_calls("今天有什么新闻", &text).iter().all(|c| c.0 != "pomelo_web_search")); // searched up front instead
        assert_eq!(mock_calls("查一下《三体》的作者", "")[0], ("pomelo_find_book", json!({ "titles": ["三体"] })));
        assert_eq!(mock_calls("《霸王别姬》的导演是谁", "")[0].0, "pomelo_find_film");
        assert!(mock_calls("你好", "").is_empty());
    }
}
