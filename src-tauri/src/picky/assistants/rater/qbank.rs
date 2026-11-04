//! 三个题库 SKILL (v0.7 阶段 ⑤): 简历题 / 通用题 / 突击题. Each has an editable workflow (Seeker
//! rules of that 用途; none yet: the built-in `default_workflow`), reads what it
//! needs, and only proposes: new questions (a list to tick on Rater's page, or the reply's card in a
//! 柚子 conversation) or one new answer (a diff). Writing is the user's click (rater.rs `put`, one undo
//! step). The page's 「让柚子补题」 / 「润色」 and 柚子's tools (skills/mod.rs) all call `generate`.
//! - Sources: 简历题: the chosen Chinese materials (none chosen: those in the Chinese draft, else every
//!   enabled 项目 / 工作 one) with their 经历档案; 通用题: 基本信息, 已确认事实 and the experiences (each
//!   material with its points, no 档案: a self-introduction or a STAR answer needs them); 突击题: the job,
//!   its JD (备注), the passages of its company's knowledge/ most like the JD, the experiences (to prove
//!   a requirement) and the bank's 简历题 / 通用题 (to adapt one: `from`, the original stays).
//! - The model reads the working principles (seeker/model.rs `PRINCIPLES`), 题库维护 and this SKILL's
//!   workflow, the answer skeleton (`SKELETON`), the sources, the questions of that scope already in the bank (no repeats), and answers JSON; Rust
//!   checks every id and the count. One call, no tools, no loops.
//! - The model is Rater's own (`rater.options` in picky.db preferences: model, thinking; not undone),
//!   not the conversation's. A cloud model only when Seeker's 「允许发给云端」 is on.
use super::*;
use crate::picky::assistants::seeker::{self, catalog::Record, company, output::JobSnapshot, JobRef, Workspace};
use crate::picky::assistants::Choice;
use crate::picky::cloud::Level;
use crate::picky::store::{pref_get, pref_set};
use serde_json::Value;
use tauri::{AppHandle, Manager};

const OPTIONS: &str = "rater.options";
/// At most this many new questions a request.
pub const MAX: usize = 12;

/// The answer's shape, for every SKILL.
pub const SKELETON: &str = "答案是用户自己的口述稿：第一人称、短句、大白话，能在 90 秒（约 400 字）内说完。骨架：一句话结论 → 我做了什么（动作 + 技术或方法）→ 结果（数字和范围）→ 局限或如果重来。行为题用 情境 → 任务 → 行动 → 结果。假设题（「如果让你……」）答「我会怎么做」，不写成做过的事，也不写结果数字。";

/// A SKILL's workflow before the user writes their own (Rater 规则 page shows it): its points.
pub fn default_workflow(category: &str) -> &'static [&'static str] {
    match category {
        "简历题" => &[
            "面试官照着简历问，要有问必答：按每段经历各出几道。",
            "题型：「【项目】里你具体负责什么？哪些是队友做的？」「为什么用【技术】而不是【另一种】？」「【指标】是怎么算的、在什么数据上？」「最难的一步是什么，怎么解决的？」「如果重做，你会改哪里？」",
            "简历上的每个技术名词都出题：它是什么、你在哪里用过、用到什么程度。",
            "只用素材和经历档案里有的事实，本人做的和队友做的分开。",
            "缺的地方写「（待补：缺什么）」，不编数字和细节。",
            "资料里没写的做法（指标怎么算、数据怎么来、为什么这么选）也写「（待补）」，不按常理补全。",
        ],
        "通用题" => &[
            "题型：自我介绍（1 分钟、3 分钟两版）、为什么投这个岗位 / 这家公司、优点和缺点、一次失败 / 冲突 / 带队的经历、职业规划、到岗时间和期望薪资、最后的反问。",
            "事实只用基本信息、已确认事实和经历一览里有的。",
            "到岗时间、薪资这类照已确认事实写，没有就写「（待补）」，不替用户定。",
        ],
        _ => &[
            "每个岗位第一次出 8～10 道：「JD 里的【要求】，你有什么经历能证明？」「你怎么看这家公司的【业务 / 产品】？」「如果让你做【这个岗位的日常工作】，第一个月做什么？」",
            "再把已有的简历题、通用题按这家公司改写几道，写明改自哪道，原题不动。",
            "公司资料是招聘方的情况，不是用户的经历：只用来判断岗位看重什么、回答怎么贴近。",
            "用户的事实只从已有的题、素材里来。",
        ],
    }
}

/// 准备面试's framework (Rater 规则 · 面试, 「速查卡」): what the prep writes. The JSON shape stays in
/// the program (interview.rs `prep_prompt`).
pub const PREP: &[&str] = &[
    "questions：从题库里挑最可能被问到的题，按面试的顺序（自我介绍打头，再简历题，再这个岗位的突击题），8～15 道，只写列出的题目编号。",
    "follows：补 5～10 个可能的追问：从简历里的名词（它是什么、你在哪用过、用到什么程度）、数字（怎么算的、在什么数据上）、本人和队友的分工、JD 的岗位基础概念里找，about 写追的是哪道题或简历上的哪个词，category 写简历题 / 通用题 / 突击题；参考答案是用户的口述稿，事实只用资料里写明的，缺的写「（待补：缺什么）」。",
    "sheet：一页速查卡（Markdown，A4 一页印得下，约 800～1200 字）：# 公司 · 岗位；## 岗位要点（JD 的 3～5 条，各配我能拿出的证据）；## 自我介绍（要点）；## 经历速记（每段一行：本人职责｜关键数据｜已知边界）；## 高频题（题目 + 答案要点，复用题库答案）；## 可能的追问（问题 → 应对要点）；## 反问（2～3 个）。",
    "速查卡和追问里不写题目编号、素材编号（写题目原文、经历名称）。",
    "事实只用资料里写明的，不编数字和细节，资料没写的做法和过程写「（待补）」；明确本人职责、项目数据和已知边界。",
    "公司资料是招聘方的情况，不是用户的经历。",
    "准备稿不改题库和简历。",
];
/// 记录复盘's framework (规则 · 复盘, 「复盘记录」): how the conversation fills the table (record.rs).
pub const RECORD: &[&str] = &[
    "按面试里问的顺序一题一题记：问题、用户当时怎么答的、面试官当场的反应（追问、点头、打断……）。",
    "「我答的」尽量用用户的原话，可以理顺句子，不加用户没说的内容。",
    "一次只问一两件事；用户说得笼统时追问细节：答到哪一步、面试官接着问了什么。",
    "形式（如「视频 · 技术面」）和结果（待定 / 通过 / 没过）要问到；结果还不知道就写待定。",
    "面试官的整体反馈是发生的事实，用户自己的判断是个人看法，分开记。",
    "用户想不起来时，可以查速查卡和以前的复盘帮着回忆，但表里只写用户确认过的。",
    "都记齐了，把整张表念一遍给用户确认。",
];
/// 总结复盘's framework (规则 · 复盘, 「复盘卡」): what the summary writes (interview.rs `summary_prompt`).
pub const SUMMARY: &[&str] = &[
    "summary：一两句总评，发生的事实和个人判断分开说；几场一起总结时，说说前后几轮反复出现的问题和同一个短板。",
    "hits：准备到了的问题，asked 写问的序号，text 写准备里对应的题、追问或速查卡的哪部分。",
    "misses：没准备到、或答得不够的，asked 写序号，text 写缺在哪（哪段经历讲不清、缺什么事实），不替用户编答案。",
    "questions：值得加进题库的题（多半是遗漏的），category 写简历题 / 通用题 / 突击题，answer 按资料和这次的回答整理成口述稿：每个做法、数字、分工都要能在资料或这次的回答里找到原话，找不到的写「（待补：缺什么）」，不按常理补全过程；题库里已有的不要重复。",
    "rules：只有明显该长期遵守的教训才提议成规则的一个要点（最多 2 条，没有就空数组），use 写 复盘 / 面试 / 卡片练习 / 题库维护 / 简历题 / 通用题 / 突击题，title 是规则的标题（下面已有的规则标题能用就用同一个），text 是一句话的要点，why 写依据这次的什么。",
    "个案经验留在复盘里；一次录取或拒绝不说明简历好坏。",
];

/// A rule the program brings until the user writes their own (规则 page: a dashed card, 「改成我的」):
/// the SKILLs' workflows (any rule of the group stands in for it: `whole`) and the frameworks of
/// 准备面试, 记录复盘 and 总结复盘 (a rule of the same title in the group stands in for it; titles a user's
/// imported rules don't have: 「速查卡」「复盘记录」「复盘卡」).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Builtin {
    pub category: String,
    pub title: String,
    pub points: Vec<String>,
    pub whole: bool,
}

pub fn builtins() -> Vec<Builtin> {
    let one = |category: &str, title: &str, points: &[&str], whole: bool| Builtin { category: category.into(), title: title.into(), points: points.iter().map(|p| p.to_string()).collect(), whole };
    let mut out: Vec<Builtin> = CATEGORIES.iter().map(|c| one(c, "工作流", default_workflow(c), true)).collect();
    out.push(one(INTERVIEW, "速查卡", PREP, false));
    out.push(one(REVIEW, "复盘记录", RECORD, false));
    out.push(one(REVIEW, "复盘卡", SUMMARY, false));
    out
}

impl Builtin {
    /// As a rule (its points, ids `p1`…), for the prompt.
    pub fn record(&self) -> Record {
        Record { kind: "rule".into(), category: self.category.clone(), title: self.title.clone(), points: self.points.iter().enumerate().map(|(i, t)| seeker::Point { id: format!("p{}", i + 1), text: t.clone() }).collect(), lang: "cn".into(), ..Default::default() }
    }
}

/// A group's rules as sent: the user's (enabled), then the built-in ones nothing stands in for.
pub fn group_rules(w: &Workspace, group: &str) -> Vec<Record> {
    let mine: Vec<Record> = w.catalog.records.iter().filter(|r| r.enabled && r.for_lang("cn") && r.kind == "rule" && r.category == group).cloned().collect();
    let extra: Vec<Record> = builtins().into_iter().filter(|b| b.category == group && if b.whole { mine.is_empty() } else { !mine.iter().any(|r| r.title.trim() == b.title) }).map(|b| b.record()).collect();
    mine.into_iter().chain(extra).collect()
}

/// `groups` as Markdown (`# 组`, `## 标题`, points numbered; a SKILL's own group is 「工作流：类别」),
/// then `# 答案` (the answer skeleton) when `answers`.
pub fn rules_of(w: &Workspace, groups: &[&str], answers: bool) -> String {
    let lists: Vec<(String, Vec<Record>)> = groups.iter().map(|g| (if CATEGORIES.contains(g) { format!("工作流：{g}") } else { seeker::catalog::group_label(&w.catalog, g) }, group_rules(w, g))).collect();
    let refs: Vec<(String, Vec<&Record>)> = lists.iter().map(|(h, rs)| (h.clone(), rs.iter().collect())).collect();
    let mut out = seeker::catalog::rules_markdown(&refs);
    if answers {
        out.push(format!("# 答案\n{SKELETON}"));
    }
    out.join("\n\n")
}

/// Rater's AI functions (设置 page): each may have its own model and 思考, else the default.
pub const FEATURES: [(&str, &str); 8] = [("suggest", "补题"), ("polish", "润色答案"), ("pick", "卡片练习 · 按 JD 挑卡"), ("follow", "卡片练习 · 追问"), ("comment", "卡片练习 · 点评"), ("prep", "面试 · 准备"), ("record", "复盘 · 记录"), ("summary", "复盘 · 总结")];

/// The default model and 思考, and the functions that have their own (`features`, by key).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Options {
    pub model: Option<String>,
    pub thinking: bool,
    pub effort: Option<String>,
    pub features: BTreeMap<String, Choice>,
}

impl Options {
    /// The model and 思考 a function uses.
    pub fn of(&self, feature: &str) -> Choice {
        self.features.get(feature).cloned().unwrap_or(Choice { model: self.model.clone(), thinking: self.thinking, effort: self.effort.clone() })
    }
}

pub fn options(c: &Connection) -> Result<Options, String> {
    Ok(pref_get::<Options>(c, OPTIONS)?.unwrap_or_default())
}

/// What one request is about.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ask {
    /// 简历题 / 通用题 / 突击题 (new questions), or 润色 (a new answer for `question`).
    pub kind: String,
    /// 简历题: the materials to ask about (none: the draft's, else all 项目 / 工作).
    pub materials: Vec<String>,
    /// 突击题: the 投石问路 job.
    pub job: Option<i64>,
    /// How many new questions (0: the SKILL's default).
    pub count: usize,
    /// 润色: the question.
    pub question: Option<String>,
    /// The user's words for this time.
    pub request: String,
    /// 润色 a question still in a 柚子 card (not in the bank yet, or its answer not saved yet): it
    /// stands in for the bank's (skills/qbank.rs `replace`).
    #[serde(skip)]
    pub pending: Option<Question>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub kind: String,
    pub model: String,
    pub note: String,
    pub advice: String,
    /// New questions (revision 0), checked; their material / job / from point at real records.
    pub questions: Vec<Question>,
    /// 润色: the question as it is and with the new answer.
    pub before: Option<Question>,
    pub after: Option<Question>,
}

pub const POLISH: &str = "润色";

/// A 投石问路 job as a question (or a round) keeps it: 「公司 · 职位」.
pub fn job_link(j: &JobSnapshot) -> JobLink {
    JobLink { id: j.id, created_at: j.created_at.clone(), label: j.position.as_deref().map_or(j.company.clone(), |p| format!("{} · {p}", j.company)) }
}

/// The Chinese materials 简历题 may ask about (enabled).
pub fn materials(w: &Workspace) -> Vec<&Record> {
    w.catalog.records.iter().filter(|r| r.kind == "material" && r.enabled && r.lang == "cn").collect()
}

/// 简历题's scope: the chosen ones, else those in the Chinese draft, else every 项目 / 工作 one.
fn chosen<'a>(w: &'a Workspace, ids: &[String]) -> Result<Vec<&'a Record>, String> {
    let all = materials(w);
    if !ids.is_empty() {
        return ids.iter().map(|id| all.iter().find(|r| &r.id == id).copied().ok_or_else(|| "选的素材已删除或停用了".to_string())).collect();
    }
    let in_draft: Vec<&Record> = all.iter().filter(|r| w.cn.draft.as_ref().is_some_and(|d| d.sections.iter().flat_map(|s| &s.entries).any(|e| e.source.as_ref().is_some_and(|s| s.record_id == r.id)))).copied().filter(|r| r.category != "教育").collect();
    if !in_draft.is_empty() {
        return Ok(in_draft);
    }
    Ok(all.into_iter().filter(|r| r.category == "项目" || r.category == "工作").collect())
}

/// The rule groups a SKILL (题库 · 简历题 / 通用题 / 突击题) reads.
pub fn reads(category: &str) -> [&str; 2] {
    [QBANK, category]
}

/// The rules this SKILL reads (`reads`): 题库维护 and its workflow (its rules, else the
/// default), as Markdown (`# 组`, `## 标题`, points numbered), then `# 答案`.
pub fn rules(w: &Workspace, category: &str) -> String {
    rules_of(w, &reads(category), true)
}

/// 基本信息 and 已确认事实 (not 档案): what 通用题 take a person's facts from.
pub(super) fn profile_and_facts(w: &Workspace) -> Vec<String> {
    let on = |r: &&Record| r.enabled && r.for_lang("cn");
    let mut out = vec![];
    if let Some(p) = w.catalog.records.iter().filter(on).find(|r| r.kind == "profile" && r.lang == "cn") {
        out.push(format!("基本信息：\n{}", p.fields.iter().filter(|f| !f.value.trim().is_empty()).map(|f| format!("- {}：{}", f.label, f.value.trim())).collect::<Vec<_>>().join("\n")));
    }
    let facts: Vec<String> = w.catalog.records.iter().filter(on).filter(|r| r.kind == "fact" && !r.is_dossier()).map(|r| format!("- [{}] {}：{}", r.category, r.title, r.text)).collect();
    if !facts.is_empty() {
        out.push(format!("已确认事实：\n{}", facts.join("\n")));
    }
    out
}

/// The experiences as the resume has them (no 档案): what 通用题 and 突击题 take their facts from.
pub(super) fn experiences(w: &Workspace) -> Option<String> {
    let list: Vec<String> = materials(w)
        .into_iter()
        .map(|r| {
            let mut lines = vec![format!("【{}】{}", r.category, [r.title.as_str(), &r.role, &r.time].iter().filter(|x| !x.trim().is_empty()).copied().collect::<Vec<_>>().join("｜"))];
            if !r.text.trim().is_empty() {
                lines.push(format!("  {}", r.text.trim()));
            }
            lines.extend(r.choosable().iter().map(|p| format!("  - {}", p.text.replace("**", ""))));
            lines.join("\n")
        })
        .collect();
    (!list.is_empty()).then(|| format!("经历（简历素材，用户的事实只从这里和已确认事实来）：\n{}", list.join("\n")))
}

pub(super) fn material_block(w: &Workspace, r: &Record) -> String {
    let mut lines = vec![format!("素材 {}【{}】{}", r.id, r.category, [r.title.as_str(), &r.role, &r.time, &r.location].iter().filter(|x| !x.trim().is_empty()).copied().collect::<Vec<_>>().join("｜"))];
    if !r.text.trim().is_empty() {
        lines.push(format!("概览：{}", r.text.trim()));
    }
    if !r.stack.trim().is_empty() {
        lines.push(format!("技术栈：{}", r.stack.trim()));
    }
    lines.extend(r.points.iter().map(|p| format!("- {}", p.text.replace("**", ""))));
    for d in w.catalog.records.iter().filter(|d| d.enabled && d.is_dossier() && (d.material.as_deref() == Some(&r.id) || d.material_en.as_deref() == Some(&r.id))) {
        lines.push(format!("经历档案《{}》：\n{}", d.title, d.text));
    }
    lines.join("\n")
}

pub(super) fn question_line(q: &Question) -> String {
    let answer: String = q.answer.chars().take(300).collect();
    format!("题目 {}【{}】{}\n答：{}", q.id, q.category, q.question, if answer.trim().is_empty() { "（还没写）".into() } else { answer })
}

/// The system message and the request for one ask (no model call). `job`, `employer`: 突击题's job
/// and what its company's knowledge/ says.
pub fn prompt(w: &Workspace, bank: &Bank, ask: &Ask, job: Option<&JobSnapshot>, employer: &str) -> Result<(String, String), String> {
    let polish = ask.kind == POLISH;
    let target = match (&ask.question, polish) {
        (Some(id), true) => Some(bank.questions.iter().find(|q| &q.id == id).ok_or("这道题已经删除了")?),
        (None, true) => return Err("要润色哪道题？".into()),
        _ => None,
    };
    let category = target.map_or(ask.kind.as_str(), |q| q.category.as_str());
    if !CATEGORIES.contains(&category) {
        return Err("未知的题目类别".into());
    }
    let count = how_many(ask, category, bank, job);
    // what it reads
    let mut sources = vec![];
    let job = if category == "突击题" { job } else { None };
    let in_scope = |q: &&Question| q.category == category && (category != "突击题" || job.is_some_and(|j| q.job.as_ref().is_some_and(|x| x.id == j.id && x.created_at == j.created_at)));
    match category {
        "简历题" => {
            let mats = match target.and_then(|q| q.material.as_ref()) {
                Some(m) => materials(w).into_iter().filter(|r| &r.id == m).collect(),
                None if polish => vec![],
                None => chosen(w, &ask.materials)?,
            };
            if mats.is_empty() && !polish {
                return Err("还没有可以出题的中文素材".into());
            }
            sources.extend(mats.iter().map(|r| material_block(w, r)));
        }
        "通用题" => {
            sources.extend(profile_and_facts(w));
            sources.extend(experiences(w));
        }
        _ => {
            match job {
                Some(job) => {
                    let jd = job.note.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or("（这个岗位的备注里没有 JD）");
                    sources.push(format!("岗位：{}{}{}\nJD：\n{jd}", job.company, job.position.as_deref().map(|p| format!(" · {p}")).unwrap_or_default(), job.location.as_deref().map(|l| format!("（{l}）")).unwrap_or_default()));
                }
                None if !polish => return Err("突击题要先选一个岗位".into()),
                None => {}
            }
            sources.extend(experiences(w));
            let base: Vec<String> = bank.questions.iter().filter(|q| q.category != "突击题").map(question_line).collect();
            if !base.is_empty() && !polish {
                sources.push(format!("已有的简历题和通用题（可以按这家公司改写，写明改自哪道）：\n{}", base.join("\n\n")));
            }
        }
    }
    let have: Vec<String> = bank.questions.iter().filter(in_scope).filter(|q| target.is_none_or(|t| t.id != q.id)).map(|q| format!("- {}", q.question)).collect();
    let rules = rules(w, category);
    let shape = if polish {
        "仅输出 JSON：{\"advice\":\"改了什么（中文，一两句）\",\"answer\":\"新的答案全文\"}".to_string()
    } else {
        let extra = match category {
            "简历题" => "，\"material\":\"它问的素材编号\"",
            "突击题" => "，\"from\":\"改写自哪道题的编号（不是改写就不写）\"",
            _ => "",
        };
        format!("仅输出 JSON：{{\"advice\":\"这次出了什么、还缺什么（中文，一两句）\",\"questions\":[{{\"question\":\"问题\",\"answer\":\"口述稿\"{extra},\"tags\":[\"标签（可不写）\"]}}]}}")
    };
    let task = if polish {
        format!("把下面这道{category}的答案改写一版：更像能直接说出口的口述稿，结构照答案骨架，事实只用资料里有的，不编；不改问题。{shape}")
    } else {
        format!("按工作流出满 {count} 道新的{category}（本次要求只是补充，不减少题数），每道配一份答案。不要和已有的题重复；事实只用资料里有的，缺的写「（待补：缺什么）」，不编；资料里没写的做法和过程不按常理补全；地点、意愿、薪资、到岗时间这类个人情况只照已确认事实写，岗位的地点、要求不等于用户的情况。{shape}")
    };
    let system = format!("你是用户的中文面试准备助手，说明用中文。\n\n工作原则：\n{}\n\n任务：{task}\n\n规则：\n{rules}{employer}", seeker::model::principles(&["你只提议，用户确认后才写进题库。"]));
    let mut user = vec![format!("用户本次要求：{}", if ask.request.trim().is_empty() { "（没有）" } else { ask.request.trim() })];
    if let Some(t) = target {
        user.push(format!("要润色的题：\n{}", question_line(t)));
    }
    if !sources.is_empty() {
        user.push(format!("资料：\n{}", sources.join("\n\n")));
    }
    if !have.is_empty() && !polish {
        user.push(format!("这一类已经有的题（不要重复）：\n{}", have.join("\n")));
    }
    Ok((system, user.join("\n\n")))
}

/// How many new questions: as asked (at most `MAX`), else the SKILL's default: a job's first 突击题 8,
/// the rest 5. The model is held to it too.
fn how_many(ask: &Ask, category: &str, bank: &Bank, job: Option<&JobSnapshot>) -> usize {
    if ask.count != 0 {
        return ask.count.min(MAX);
    }
    let first = category == "突击题" && !bank.questions.iter().any(|q| q.category == "突击题" && job.is_some_and(|j| q.job.as_ref().is_some_and(|x| x.id == j.id)));
    if first { 8 } else { 5 }
}

/// The JSON in a reply, its end mended when the model left off a closing bracket or two (a small
/// local model does).
pub fn answer_json(text: &str) -> Option<Value> {
    if let Some(start) = text.find('{') {
        let body = text[start..].trim_end().trim_end_matches("```").trim_end();
        for tail in ["", "}", "]}", "}]}", "\"}]}"] {
            if let Ok(v) = serde_json::from_str::<Value>(&format!("{body}{tail}")) {
                return Some(v);
            }
        }
    }
    crate::picky::assist::json_in(text)
}

/// An id the model wrote: exactly one of `ids`, else the longest of them inside what it wrote
/// (「改写自 q2」), else none.
fn known(raw: Option<String>, ids: &[&str]) -> Option<String> {
    let raw = raw?;
    if ids.contains(&raw.as_str()) {
        return Some(raw);
    }
    ids.iter().filter(|id| raw.contains(**id)).max_by_key(|id| id.len()).map(|id| id.to_string())
}

/// The model's JSON checked: new questions (ids made here) or the new answer. A link it got wrong
/// (素材, 改自) is dropped, said in the advice; the question stays.
pub fn parse(json: &Value, w: &Workspace, bank: &Bank, ask: &Ask, job: Option<&JobSnapshot>) -> Result<Proposal, String> {
    let mut out = Proposal { kind: ask.kind.clone(), advice: json["advice"].as_str().unwrap_or("").trim().into(), ..Default::default() };
    if ask.kind == POLISH {
        let before = bank.questions.iter().find(|q| Some(&q.id) == ask.question.as_ref()).ok_or("这道题已经删除了")?.clone();
        let answer = json["answer"].as_str().map(str::trim).filter(|a| !a.is_empty()).ok_or("模型没有给出新的答案")?;
        out.after = Some(Question { answer: answer.into(), ..before.clone() });
        out.before = Some(before);
        return Ok(out);
    }
    let items = json["questions"].as_array().ok_or("模型没有给出题目")?;
    let mats = materials(w);
    let link = job.map(job_link);
    let mut seen: Vec<String> = bank.questions.iter().map(|q| q.question.trim().to_string()).collect();
    let material_ids: Vec<&str> = mats.iter().map(|r| r.id.as_str()).collect();
    let from_ids: Vec<&str> = bank.questions.iter().filter(|q| q.category != "突击题").map(|q| q.id.as_str()).collect();
    let mut dropped = 0;
    // as many as asked for (none said: the SKILL's default, at most MAX)
    let cap = how_many(ask, &ask.kind, bank, job);
    for item in items.iter() {
        if out.questions.len() >= cap {
            break;
        }
        let question = item["question"].as_str().map(str::trim).unwrap_or("");
        if question.is_empty() || seen.iter().any(|s| s == question) {
            continue; // empty, or the same as one there (or just proposed)
        }
        seen.push(question.into());
        let text = |k: &str| item[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
        let material = match (ask.kind.as_str(), text("material")) {
            ("简历题", raw) => {
                let found = known(raw.clone(), &material_ids);
                dropped += usize::from(raw.is_some() && found.is_none() && ask.materials.len() != 1);
                found.or_else(|| (ask.materials.len() == 1).then(|| ask.materials[0].clone()))
            }
            _ => None,
        };
        let from = match (ask.kind.as_str(), text("from")) {
            ("突击题", raw) => {
                let found = known(raw.clone(), &from_ids);
                dropped += usize::from(raw.is_some() && found.is_none());
                found
            }
            _ => None,
        };
        let tags: Vec<String> = item["tags"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::trim).filter(|t| !t.is_empty() && t.chars().count() <= 20).map(String::from).collect()).unwrap_or_default();
        let q = Question { id: seeker::workflow::id()?, category: ask.kind.clone(), question: question.into(), answer: text("answer").unwrap_or_default(), tags, material, job: if ask.kind == "突击题" { link.clone() } else { None }, from, revision: 0 };
        validate(&q)?;
        out.questions.push(q);
    }
    if out.questions.is_empty() {
        return Err("模型没有给出新的题（可能都和已有的重复了）".into());
    }
    if dropped > 0 {
        out.advice = format!("{}（有 {dropped} 道题挂的素材或「改自」对不上，已去掉这个链接）", out.advice).trim().to_string();
    }
    Ok(out)
}

/// What the mock model answers (the same shape a real one returns).
fn mock(w: &Workspace, bank: &Bank, ask: &Ask) -> Value {
    if ask.kind == POLISH {
        let q = bank.questions.iter().find(|q| Some(&q.id) == ask.question.as_ref());
        return serde_json::json!({ "advice": "（模拟）按骨架理了一遍。", "answer": format!("（模拟润色）一句话结论：{}", q.map_or("", |q| q.answer.as_str())) });
    }
    let n = if ask.count == 0 { 3 } else { ask.count.min(MAX) };
    let questions: Vec<Value> = match ask.kind.as_str() {
        "简历题" => chosen(w, &ask.materials).unwrap_or_default().iter().cycle().take(n).enumerate().map(|(i, r)| serde_json::json!({ "question": format!("（模拟）{}里你具体负责什么？（{}）", r.title, i + 1), "answer": format!("我负责{}里的一部分（待补：具体做了什么）。", r.title), "material": r.id })).collect(),
        "通用题" => (1..=n).map(|i| serde_json::json!({ "question": format!("（模拟）通用题 {i}：为什么投这个岗位？"), "answer": "（模拟）因为……", "tags": ["必背"] })).collect(),
        _ => (1..=n).map(|i| serde_json::json!({ "question": format!("（模拟）突击题 {i}：JD 里的要求你怎么证明？"), "answer": "（模拟）我在……", "from": bank.questions.iter().find(|q| q.category != "突击题").map(|q| q.id.clone()) })).collect(),
    };
    serde_json::json!({ "advice": "（模拟模型）出了几道示例题。", "questions": questions })
}

/// The job and what its company's knowledge/ says (突击题, and polishing one that has a job).
pub(super) async fn job_context(app: &AppHandle, job: Option<i64>, cloud: bool) -> Result<(Option<JobSnapshot>, String, String), String> {
    let Some(id) = job else { return Ok((None, String::new(), String::new())) };
    let state = app.state::<AppState>();
    let job = seeker::output::job(&state, id)?;
    let query: String = job.note.as_deref().unwrap_or(&job.company).chars().take(600).collect();
    let lookup = company::lookup(app, Some(&JobRef { id: job.id, created_at: job.created_at.clone() }), &query, cloud).await;
    let (mut employer, mut found) = (String::new(), lookup.note.clone().unwrap_or_default());
    if !lookup.passages.is_empty() {
        let name = lookup.company.clone().unwrap_or_default();
        employer = format!("\n\n公司资料（{name}，来自招聘资料，是招聘方的情况，不是用户的经历）：\n{}", lookup.passages.iter().enumerate().map(|(i, p)| format!("[{}] {}\n{}", i + 1, p.label, p.content)).collect::<Vec<_>>().join("\n\n"));
        found = format!("参考了{name}的 {} 段公司资料", lookup.passages.len());
    }
    Ok((Some(job), employer, found))
}

/// The model (and 思考) a Rater function uses (`feature`, `FEATURES`), checked: a cloud one only
/// when Seeker allows it (Rater follows it).
pub(crate) fn chosen_model(w: &Workspace, opts: &Options, feature: &str) -> Result<(String, Level), String> {
    let pick = opts.of(feature);
    let level = pick.level();
    let model = pick.model.ok_or("请先在 Rater 的设置里选模型")?;
    if !model.starts_with("local:") && !model.starts_with("cloud:") {
        return Err("请在 Rater 的设置里重新选择模型".into());
    }
    if model.starts_with("cloud:") && !w.catalog.settings.cloud_allowed {
        return Err("Seeker 资料（Rater 也照它）还没允许发给云端，请在 Seeker 的设置里打开，或选本地模型".into());
    }
    Ok((model, level))
}

/// What one call to Rater's model gave: its JSON (None: the mock model; the caller makes the answer
/// up), the model's name and a notice (another local model was running…).
pub(crate) struct Reply {
    pub json: Option<Value>,
    pub model: String,
    pub note: String,
}

/// One call to Rater's model (`chosen_model`), no tools, no loops. `max`: the reply's length before
/// room to think.
pub(crate) async fn call(state: &AppState, model: &str, system: &str, user: &str, max: i64, level: &Level) -> Result<Reply, String> {
    use crate::picky::{context, provider::{self, Event, Msg, Target}, schedule};
    let (route, note, _) = schedule::route(&state.picky, Some(model)).await?;
    let output = route.with_thinking(max, level.thinking());
    if context::estimate(system) + context::estimate(user) + output + 512 > route.window {
        return Err("资料超出所选模型的上下文，请少选几段素材，或换更大上下文的模型".into());
    }
    if matches!(&route.target, Target::Mock { .. }) {
        return Ok(Reply { json: None, model: route.name, note });
    }
    let mut answer = String::new();
    let mut finish = String::new();
    let stream = provider::stream(route.target.clone(), vec![Msg::new("system", system), Msg::new("user", user)], provider::Options { max_tokens: Some(output), thinking: Some(level.thinking()), effort: level.effort(), one_off: true, ..Default::default() }, |event| match event {
        Event::Content(text) => answer.push_str(&text),
        Event::Finished(reason) => finish = reason,
        _ => (),
    });
    tokio::time::timeout(std::time::Duration::from_secs(300), stream).await.map_err(|_| "模型等待超时，请重试或换模型".to_string())??;
    if finish == "length" {
        return Err("模型回复被截断，请少要一些或换模型".into());
    }
    let json = answer_json(&answer).ok_or("模型没有返回可用的结果，请重试")?;
    Ok(Reply { json: Some(json), model: route.name, note })
}

/// One SKILL run: reads, asks Rater's model once, checks the answer. Nothing is written.
pub async fn generate(app: &AppHandle, ask: Ask) -> Result<Proposal, String> {
    let state = app.state::<AppState>();
    let (w, mut bank, opts) = state.picky.with_db(|c| Ok((seeker::read(c)?, read(c)?, options(c)?)))?;
    if let Some(q) = &ask.pending {
        bank.questions.retain(|x| x.id != q.id);
        bank.questions.push(q.clone());
    }
    let (model, level) = chosen_model(&w, &opts, if ask.kind == POLISH { "polish" } else { "suggest" })?;
    let cloud = model.starts_with("cloud:");
    if ask.kind == "突击题" && ask.job.is_none() {
        return Err("突击题要先选一个岗位".into());
    }
    // a 突击题 being polished reads its job too (gone: without it)
    let job_id = match ask.kind.as_str() {
        "突击题" => ask.job,
        POLISH => bank.questions.iter().find(|q| Some(&q.id) == ask.question.as_ref() && q.category == "突击题").and_then(|q| q.job.as_ref()).map(|j| j.id),
        _ => None,
    };
    let (job, employer, found) = match job_context(app, job_id, cloud).await {
        Ok(x) => x,
        Err(_) if ask.kind == POLISH => (None, String::new(), String::new()),
        Err(e) => return Err(e),
    };
    let (system, user) = prompt(&w, &bank, &ask, job.as_ref(), &employer)?;
    let reply = call(&state, &model, &system, &user, if ask.kind == POLISH { 2048 } else { 6144 }, &level).await?;
    let json = reply.json.unwrap_or_else(|| mock(&w, &bank, &ask));
    let mut proposal = parse(&json, &w, &bank, &ask, job.as_ref())?;
    proposal.model = reply.model;
    proposal.note = [reply.note, found].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    Ok(proposal)
}

#[tauri::command]
pub async fn picky_rater_suggest(app: AppHandle, ask: Ask) -> Result<Proposal, String> {
    generate(&app, ask).await
}

/// Writes the ticked new questions; one undo step.
pub fn add(c: &Connection, questions: &[Question]) -> Result<(), String> {
    let tx = crate::modules::common::Tx::begin(c)?;
    for q in questions {
        if q.revision != 0 {
            return Err("只能加新题".into());
        }
        put(&tx, q)?;
    }
    tx.commit()
}

#[tauri::command]
pub async fn picky_rater_add(state: tauri::State<'_, AppState>, questions: Vec<Question>) -> Result<Bank, String> {
    if questions.is_empty() {
        return Err("至少勾上一道".into());
    }
    state.with_picky_undo(&format!("补题（{} 道）", questions.len()), |c| {
        add(c, &questions)?;
        read(c)
    })
}

/// The rules the program brings (规则 page: shown until the user writes their own).
#[tauri::command]
pub fn picky_rater_defaults() -> Vec<Builtin> {
    builtins()
}

#[tauri::command]
pub async fn picky_rater_options(state: tauri::State<'_, AppState>) -> Result<Options, String> {
    state.picky.with_db(|c| options(c))
}

/// Like the other assistants' settings: not undone. A function of its own has a model.
#[tauri::command]
pub async fn picky_rater_options_set(state: tauri::State<'_, AppState>, options: Options) -> Result<Options, String> {
    if options.features.iter().any(|(k, v)| !FEATURES.iter().any(|(f, _)| f == k) || v.model.is_none()) {
        return Err("设置里有不认识的功能，请刷新".into());
    }
    state.picky.with_db(|c| {
        pref_set(c, OPTIONS, &options)?;
        Ok(options.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::assistants::seeker::{catalog, Point};
    use crate::picky::store::tests::memory_db;
    use serde_json::json;

    fn record(id: &str, kind: &str, category: &str, title: &str, text: &str) -> Record {
        Record { id: id.into(), kind: kind.into(), category: category.into(), title: title.into(), text: text.into(), lang: "cn".into(), ..Default::default() }
    }

    /// A made-up Seeker and bank: two materials (one with its 档案), facts, rules, two questions.
    fn seeded(c: &Connection) {
        let mut notes = record("m-notes", "material", "项目", "柚子笔记", "读书笔记整理工具");
        notes.role = "独立开发".into();
        notes.points = vec![Point { id: "a".into(), text: "**检索改进**：按意思搜索".into() }];
        let mut profile = record("p", "profile", "基本信息", "基本信息", "");
        profile.fields = vec![seeker::HeaderField { id: "f1".into(), label: "姓名".into(), value: "林小柚".into(), visible: true }];
        let mut dossier = record("d", "fact", "经历档案", "柚子笔记 经历档案", "本人负责全部代码，向量用的是本地模型。");
        dossier.material = Some("m-notes".into());
        let mut shared = record("r1", "rule", "共用准则", "定位", "只做指定的事。");
        shared.lang = String::new();
        for r in [notes, record("m-work", "material", "工作", "示例证券", "研究部实习"), profile, dossier, record("f", "fact", "个人事实", "到岗时间", "2027 年 4 月后"), shared, record("r2", "rule", "题库维护", "答案要求", "能口述。"), record("r3", "rule", "通用题", "自己写的", "只出自我介绍。"), record("x", "rule", "素材润色", "别的", "不该读到")] {
            catalog::put(c, &r).unwrap();
        }
        put(c, &Question { id: "q1".into(), category: "简历题".into(), question: "柚子笔记里你负责什么？".into(), answer: "全部代码。".into(), material: Some("m-notes".into()), ..Default::default() }).unwrap();
        put(c, &Question { id: "q2".into(), category: "通用题".into(), question: "为什么投这个岗位？".into(), answer: "因为……".into(), ..Default::default() }).unwrap();
    }

    fn bank_job() -> JobSnapshot {
        JobSnapshot { id: 4, company: "柚子银行".into(), position: Some("数据分析师".into()), location: Some("上海".into()), link: None, note: Some("负责信贷数据分析，要求 SQL。".into()), created_at: "2026-10-01T00:00:00.000Z".into() }
    }

    fn ask(kind: &str) -> Ask {
        Ask { kind: kind.into(), ..Default::default() }
    }

    #[test]
    fn each_skill_reads_its_workflow_and_what_it_needs() {
        let c = memory_db();
        seeded(&c);
        let (w, bank) = (seeker::read(&c).unwrap(), read(&c).unwrap());
        // 简历题: the default workflow, the material with its 档案, what's there already
        let (system, user) = prompt(&w, &bank, &Ask { materials: vec!["m-notes".into()], count: 2, ..ask("简历题") }, None, "").unwrap();
        for part in ["工作原则：\n1. 说明通俗、精简", "4. 你只提议，用户确认后才写进题库。", "# 题库维护\n\n## 答案要求\n1. 能口述。", "# 工作流：简历题\n\n## 工作流\n1. 面试官照着简历问", "按工作流出满 2 道新的简历题", "# 答案\n答案是用户自己的口述稿"] {
            assert!(system.contains(part), "{part}\n{system}");
        }
        assert!(!system.contains("不该读到") && !system.contains("只做指定的事"), "{system}");
        for part in ["素材 m-notes【项目】柚子笔记｜独立开发", "- 检索改进：按意思搜索", "经历档案《柚子笔记 经历档案》", "这一类已经有的题（不要重复）：\n- 柚子笔记里你负责什么？"] {
            assert!(user.contains(part), "{part}\n{user}");
        }
        assert!(!user.contains("示例证券"));
        // nothing chosen and no draft: every 项目 / 工作
        assert!(prompt(&w, &bank, &ask("简历题"), None, "").unwrap().1.contains("示例证券"));
        // 通用题: the user's own workflow replaces the default; 基本信息, facts, the experiences
        let (system, user) = prompt(&w, &bank, &ask("通用题"), None, "").unwrap();
        assert!(system.contains("# 工作流：通用题\n\n## 自己写的\n1. 只出自我介绍。") && !system.contains("1. 题型：自我介绍"), "{system}");
        for part in ["基本信息：\n- 姓名：林小柚", "已确认事实：\n- [个人事实] 到岗时间：2027 年 4 月后", "经历（简历素材，", "【项目】柚子笔记｜独立开发\n  读书笔记整理工具\n  - 检索改进：按意思搜索"] {
            assert!(user.contains(part), "{part}\n{user}");
        }
        assert!(!user.contains("本人负责全部代码"));
        // 突击题: needs a job; the JD and the other questions to adapt; 8 the first time
        assert!(prompt(&w, &bank, &ask("突击题"), None, "").is_err());
        let (system, user) = prompt(&w, &bank, &ask("突击题"), Some(&bank_job()), "\n公司资料（柚子银行…）").unwrap();
        assert!(system.contains("出满 8 道新的突击题") && system.contains("公司资料（柚子银行…）") && system.contains("\"from\""), "{system}");
        assert!(user.contains("岗位：柚子银行 · 数据分析师（上海）\nJD：\n负责信贷数据分析") && user.contains("题目 q2【通用题】为什么投这个岗位？") && user.contains("【工作】示例证券") && !user.contains("本人负责全部代码"), "{user}");
        // 润色: the question, no new ones
        let (system, user) = prompt(&w, &bank, &Ask { question: Some("q1".into()), ..ask(POLISH) }, None, "").unwrap();
        assert!(system.contains("把下面这道简历题的答案改写一版") && user.contains("要润色的题：\n题目 q1【简历题】") && user.contains("经历档案《"), "{user}");
        assert!(prompt(&w, &bank, &Ask { question: Some("gone".into()), ..ask(POLISH) }, None, "").is_err());
    }

    #[test]
    fn the_answer_is_checked_before_anything_is_shown() {
        let c = memory_db();
        seeded(&c);
        let (w, bank) = (seeker::read(&c).unwrap(), read(&c).unwrap());
        let resume = Ask { materials: vec!["m-notes".into()], ..ask("简历题") };
        let ok = json!({ "advice": "两道", "questions": [
            { "question": "为什么用 SQLite？", "answer": "因为轻。", "material": "m-notes", "tags": ["必背", ""] },
            { "question": "柚子笔记里你负责什么？", "answer": "重复的" },
            { "question": "检索怎么做的？", "answer": "按意思。" },
            { "question": " ", "answer": "空的" }
        ] });
        let p = parse(&ok, &w, &bank, &resume, None).unwrap();
        assert_eq!(p.questions.iter().map(|q| (q.question.as_str(), q.material.as_deref(), q.tags.clone(), q.revision)).collect::<Vec<_>>(), [("为什么用 SQLite？", Some("m-notes"), vec!["必背".to_string()], 0), ("检索怎么做的？", Some("m-notes"), vec![], 0)]);
        // a link the model got wrong: dropped (one material chosen: that one), said in the advice
        let p = parse(&json!({ "questions": [{ "question": "x", "material": "没有" }] }), &w, &bank, &resume, None).unwrap();
        assert_eq!(p.questions[0].material.as_deref(), Some("m-notes"));
        let p = parse(&json!({ "questions": [{ "question": "x", "material": "没有" }] }), &w, &bank, &ask("简历题"), None).unwrap();
        assert!(p.questions[0].material.is_none() && p.advice.contains("有 1 道题挂的素材"), "{}", p.advice);
        // the end mended
        assert_eq!(answer_json("好的：{\"questions\":[{\"question\":\"x\"}]").unwrap()["questions"][0]["question"], "x");
        assert_eq!(answer_json("```json\n{\"answer\":\"y\"}\n```").unwrap()["answer"], "y");
        assert!(parse(&json!({ "questions": [] }), &w, &bank, &resume, None).is_err());
        // 突击题: the job goes on each; from must be a 简历题 / 通用题 that is there
        let job = Ask { job: Some(4), ..ask("突击题") };
        let p = parse(&json!({ "questions": [{ "question": "为什么投柚子银行？", "answer": "……", "from": "q2" }] }), &w, &bank, &job, Some(&bank_job())).unwrap();
        assert_eq!((p.questions[0].from.as_deref(), p.questions[0].job.as_ref().map(|j| j.label.as_str())), (Some("q2"), Some("柚子银行 · 数据分析师")));
        let p = parse(&json!({ "questions": [{ "question": "y", "from": "nope" }, { "question": "z", "from": "改写自 q2" }] }), &w, &bank, &job, Some(&bank_job())).unwrap();
        assert_eq!(p.questions.iter().map(|q| q.from.as_deref()).collect::<Vec<_>>(), [None, Some("q2")]);
        // 润色: the same question with the new answer (its revision, so a save checks it)
        let p = parse(&json!({ "advice": "顺了", "answer": "新答案" }), &w, &bank, &Ask { question: Some("q1".into()), ..ask(POLISH) }, None).unwrap();
        let after = p.after.unwrap();
        assert_eq!((p.before.unwrap().answer, after.answer.as_str(), after.revision), ("全部代码。".to_string(), "新答案", 1));
        // the mock model's answers pass the same checks
        for a in [resume.clone(), ask("通用题"), Ask { question: Some("q2".into()), ..ask(POLISH) }] {
            assert!(parse(&mock(&w, &bank, &a), &w, &bank, &a, None).is_ok(), "{a:?}");
        }
        assert!(parse(&mock(&w, &bank, &job), &w, &bank, &job, Some(&bank_job())).is_ok());
    }

    #[test]
    fn ticked_questions_go_in_with_one_undo_step_and_from_a_card_too() {
        let c = memory_db();
        seeded(&c);
        let (w, bank) = (seeker::read(&c).unwrap(), read(&c).unwrap());
        let p = parse(&mock(&w, &bank, &ask("通用题")), &w, &bank, &ask("通用题"), None).unwrap();
        let mut h = crate::undo::History::default();
        crate::undo::tests::step(&c, &mut h, "补题（3 道）", |c| add(c, &p.questions));
        assert_eq!(read(&c).unwrap().questions.len(), 5);
        assert!(add(&c, &p.questions).is_err()); // not twice (revision 0 is new only)
        crate::undo::undo(&c, &mut h).unwrap();
        assert_eq!(read(&c).unwrap().questions.len(), 2);

        // in a conversation: the card carries them; confirming writes the ticked ones (Rater's own undo step)
        let dir = crate::db::tests::temp_dir("qbank-card");
        let st = crate::AppState::init(crate::paths::DataPaths::new(dir.to_path_buf()), &dir, crate::picky::Mode::Mock);
        st.picky.with_db(|c| {
            seeded(c);
            c.execute_batch("INSERT INTO sessions (title) VALUES ('t'); INSERT INTO runs (session_id, model, model_name, state) VALUES (1, 'local:x', 'X', 'completed');").map_err(crate::modules::common::err)
        }).unwrap();
        let tool = crate::picky::skills::qbank::GENERAL_TOOL;
        let field = |label: &str, after: &str| crate::picky::act::Field { label: label.into(), before: None, after: after.into(), long: false };
        st.picky.with_db(|c| {
            for q in &p.questions {
                crate::picky::act::card::add_rater(c, 1, tool, &q.question, &[field("类别", "通用题")], q)?;
            }
            let after = Question { answer: "改过的".into(), ..read(c)?.questions[1].clone() };
            crate::picky::act::card::add_rater(c, 1, tool, &after.question, &[], &after)
        }).unwrap();
        let card = st.picky.with_db(|c| crate::picky::act::card_of(c, 1)).unwrap().unwrap();
        assert_eq!(card.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["新通用题", "新通用题", "新通用题", "改答案"]);
        let picks: Vec<_> = [0, 2, 3].map(|position| crate::picky::act::card::Pick { position, option: None }).into();
        let card = crate::picky::act::card::confirm(&st, 1, &picks).unwrap();
        assert_eq!(card.items.iter().map(|i| (i.checked, i.written.as_ref().map(|w| (w.kind.as_str(), w.gone)))).collect::<Vec<_>>(), [(true, Some(("rater", false))), (false, None), (true, Some(("rater", false))), (true, Some(("rater", false)))]);
        let bank = st.picky.with_db(|c| read(c)).unwrap();
        assert_eq!((bank.questions.len(), bank.questions[1].answer.as_str()), (4, "改过的"));
        assert_eq!(crate::undo::step_back(&st, false).unwrap().label.as_deref(), Some("柚子：3 道题"));
        let card = st.picky.with_db(|c| crate::picky::act::card_of(c, 1)).unwrap().unwrap();
        assert_eq!(card.items.iter().filter_map(|i| i.written.as_ref()).map(|w| w.gone).collect::<Vec<_>>(), [true, true, false]); // the edited one is still there
        assert_eq!(st.picky.with_db(|c| read(c)).unwrap().questions[1].answer, "因为……");
    }

    /// Richer made-up material for the real-model runs (still fictional).
    fn seeded_rich(c: &Connection) {
        seeded(c);
        let mut notes = catalog::load(c).unwrap().records.into_iter().find(|r| r.id == "m-notes").unwrap();
        notes.time = "2026.03 - 至今".into();
        notes.stack = "Rust、SQLite、本地向量模型".into();
        notes.points = vec![
            Point { id: "a".into(), text: "**检索改进**：用本地向量模型做按意思搜索，在 120 条测试问题上前 5 条命中率从 54% 提到 81%".into() },
            Point { id: "b".into(), text: "**数据整理**：把零散笔记按书合并成卷，去重后条数减少 30%".into() },
            Point { id: "c".into(), text: "**离线优先**：全部数据存在本机 SQLite，单文件备份".into() },
        ];
        catalog::put(c, &notes).unwrap();
        let mut orange = record("m-orange", "material", "项目", "橙子识别", "四类水果图像分类");
        (orange.role, orange.time, orange.stack) = ("项目组长".into(), "2025.12 - 2026.01".into(), "PyTorch、MobileNetV2".into());
        orange.points = vec![Point { id: "a".into(), text: "**模型迭代**：带 3 人小组两轮调参，测试集准确率 0.91".into() }, Point { id: "b".into(), text: "**类别均衡**：样本 20:75 失衡，加权采样后验证集 Macro-F1 从 0.88 到 0.92".into() }];
        catalog::put(c, &orange).unwrap();
        let mut d = record("d2", "fact", "经历档案", "橙子识别 经历档案", "本人负责第二版和最终版的调参与类别均衡；数据清洗是队友做的；测试集只有 60 张图，结果波动大。");
        d.material = Some("m-orange".into());
        catalog::put(c, &d).unwrap();
        catalog::put(c, &record("f2", "fact", "个人事实", "语言成绩", "CET-4 580，没考 CET-6")).unwrap();
    }

    /// Opt-in, a real model: POMELO_REAL_LLM=1 [POMELO_MODEL=qwen3.5-9b] (the running local server), or
    /// POMELO_CLOUD_KEY_FILE=… [POMELO_CLOUD_SERVICE=…] [POMELO_CLOUD_MODEL=…] (chat.rs `cloud_target`).
    /// [POMELO_THINK=1] [POMELO_QBANK_CASE=简历题|通用题|突击题|润色] [POMELO_QBANK_OUT=<report.md>]
    ///   cargo test --manifest-path src-tauri/Cargo.toml real_qbank -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_qbank() {
        use crate::picky::provider::{self, Event, Msg, Target};
        let target = match crate::picky::chat::tests::cloud_target() {
            Some((t, _)) => t,
            None if std::env::var("POMELO_REAL_LLM").as_deref() == Ok("1") => Target::Local { model: std::env::var("POMELO_MODEL").unwrap_or_else(|_| "qwen3.5-9b".into()) },
            None => return eprintln!("needs POMELO_REAL_LLM=1 or POMELO_CLOUD_KEY_FILE"),
        };
        let name = match &target { Target::Cloud { model, .. } => model.api_model.clone(), Target::Local { model } => model.clone(), _ => "mock".into() };
        let think = std::env::var("POMELO_THINK").as_deref() == Ok("1");
        let c = memory_db();
        seeded_rich(&c);
        let (w, bank) = (seeker::read(&c).unwrap(), read(&c).unwrap());
        let only = std::env::var("POMELO_QBANK_CASE").ok();
        let cases = [
            ("简历题（柚子笔记、橙子识别，4 道）", Ask { materials: vec!["m-notes".into(), "m-orange".into()], count: 4, ..ask("简历题") }, None),
            ("通用题（3 道）", Ask { count: 3, request: "要有 1 分钟自我介绍".into(), ..ask("通用题") }, None),
            ("突击题（柚子银行 · 数据分析师，默认条数）", ask("突击题"), Some(JobSnapshot { note: Some("岗位职责：1. 负责信贷业务的数据分析和报表；2. 用 SQL、Python 做数据清洗和建模；3. 和业务部门沟通需求。任职要求：统计、计算机相关专业；熟悉 SQL；有机器学习项目经历优先。".into()), ..bank_job() })),
            ("润色（柚子笔记里你负责什么？）", Ask { question: Some("q1".into()), ..ask(POLISH) }, None),
        ];
        let mut report = vec![format!("# Rater 题库 SKILL 真测：{name}{}\n", if think { "（思考）" } else { "" })];
        for (label, a, job) in cases {
            if only.as_deref().is_some_and(|o| !label.starts_with(o)) {
                continue;
            }
            let (system, user) = prompt(&w, &bank, &a, job.as_ref(), "").unwrap();
            // room to think, as the app gives it (Route::with_thinking): a model that always thinks too
            let room = match &target {
                Target::Cloud { model, .. } if model.thinking.thinks(think) => crate::picky::context::think_room(model.context_window),
                _ if think => crate::picky::context::think_room(16384),
                _ => 0,
            };
            let start = std::time::Instant::now();
            let (mut answer, mut finish) = (String::new(), String::new());
            let call = provider::stream(target.clone(), vec![Msg::new("system", &system), Msg::new("user", &user)], provider::Options { max_tokens: Some(if a.kind == POLISH { 2048 } else { 6144 } + room), thinking: Some(think), one_off: true, ..Default::default() }, |e| match e {
                Event::Content(t) => answer.push_str(&t),
                Event::Finished(r) => finish = r,
                _ => (),
            });
            let res = tauri::async_runtime::block_on(async { tokio::time::timeout(std::time::Duration::from_secs(600), call).await });
            let secs = start.elapsed().as_secs();
            report.push(format!("## {label}\n\n用时 {secs} 秒；结束原因 {finish}"));
            if let Err(e) = res.map_err(|_| "超时".to_string()).and_then(|r| r) {
                report.push(format!("- **失败**：{e}\n"));
                continue;
            }
            let parsed = answer_json(&answer).ok_or_else(|| "回答里没有 JSON".to_string()).and_then(|j| parse(&j, &w, &bank, &a, job.as_ref()));
            match parsed {
                Err(e) => report.push(format!("- **没通过检查**：{e}\n\n原文：\n\n```\n{answer}\n```\n")),
                Ok(p) => {
                    // numbers in the answers that the material doesn't have (maybe made up)
                    let source = format!("{system}\n{user}");
                    let odd = |text: &str| -> Vec<String> {
                        let mut out: Vec<String> = text.split(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '%')).map(|s| s.trim_matches('.')).filter(|s| s.chars().any(|ch| ch.is_ascii_digit()) && s.len() > 1 && !source.contains(*s)).map(String::from).collect();
                        out.dedup();
                        out
                    };
                    report.push(format!("- 说明：{}", p.advice));
                    if let (Some(b), Some(after)) = (&p.before, &p.after) {
                        report.push(format!("- 原答案（约 {} 秒）：{}\n- 新答案（约 {} 秒）：{}{}", seconds(&b.answer), b.answer, seconds(&after.answer), after.answer, match odd(&after.answer).as_slice() { [] => String::new(), x => format!("\n  - ⚠️ 资料里没有的数字：{}", x.join("、")) }));
                    }
                    if a.kind != POLISH {
                        report.push(format!("- 出了 {} 道（要的 {}）", p.questions.len(), if a.count == 0 { "默认".to_string() } else { a.count.to_string() }));
                    }
                    for q in &p.questions {
                        let s = seconds(&q.answer);
                        let extra = [q.material.as_ref().map(|m| format!("素材 {m}")), q.from.as_ref().map(|f| format!("改自 {f}")), (!q.tags.is_empty()).then(|| format!("标签 {}", q.tags.join("、")))].into_iter().flatten().collect::<Vec<_>>().join("；");
                        report.push(format!("\n### {}\n\n约 {s} 秒{}{}\n\n{}{}", q.question, if s > LIMIT { "（超过 90 秒）" } else { "" }, if extra.is_empty() { String::new() } else { format!("；{extra}") }, q.answer, match odd(&q.answer).as_slice() { [] => String::new(), x => format!("\n\n⚠️ 资料里没有的数字：{}", x.join("、")) }));
                    }
                    report.push(String::new());
                }
            }
        }
        let text = report.join("\n");
        eprintln!("{text}");
        if let Ok(out) = std::env::var("POMELO_QBANK_OUT") {
            std::fs::write(out, text).unwrap();
        }
    }
}
