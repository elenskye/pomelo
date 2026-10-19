//! 小拾查数据 (v0.3.5 「问我的数据」): read-only lookups the model asks for while answering (tool calls).
//! Rules (requirements.md「小拾 · 懂 Pomelo」, architecture.md「小拾」):
//! - One tool per module (`pomelo_todo` …). Each view runs the module's own read functions, so totals
//!   and derived numbers are the pages' (jobs::stats, finance::assets / totals, todo::agenda …); only
//!   the wording for the model is here. Results are compact Chinese text cut to the room left in the
//!   window ("还有 N 条没列出"); the records shown are kept so the answer can link them.
//! - Never read: 芝麻开门 (no tool exists) and modules hidden in 设置 → 侧栏 (no tool offered; a call
//!   anyway is refused). For a cloud model, modules not allowed to go to the cloud (设置 → 小拾,
//!   `data.cloud`; default 钱来钱往 local only) are refused with the sentence the answer starts with.
//! - 五花八门 / 循序渐进: one tool each (`pomelo_lists`, `pomelo_progress`) for every module made from
//!   it; each made module is hidden / allowed for the cloud on its own (key "list/3"), like a module.
//! - Tool definitions carry no user data (no names, categories, amounts).
//! - Nothing here writes Pomelo data.

use super::provider::{Msg, ToolCall};
use super::store::{pref_get, pref_set};
use crate::modules::common::{self, Status as Mark};
use crate::modules::finance::{self, AccountKind, Currency, Flow};
use crate::modules::jobs::{self, Stage};
use crate::modules::custom::{self, Template};
use crate::modules::{books, days, drawing, films, kinds, lists, notes, packing, progress, todo};
use crate::AppState;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use tauri::State;

pub const CLOUD_KEY: &str = "data.cloud";
pub const HIDDEN_KEY: &str = "ui.module_hidden";
/// Rounds of lookups per answer; the next request must answer.
pub const MAX_ROUNDS: usize = 4;
/// At most this much of the window per lookup result.
pub const MAX_RESULT: i64 = 3000;

pub struct Module {
    pub key: &'static str,
    pub label: &'static str,
    pub tool: &'static str,
    cloud: bool,
}

pub const MODULES: &[Module] = &[
    Module { key: "todo", label: "一步一步", tool: "pomelo_todo", cloud: true },
    Module { key: "books", label: "摸鱼读读", tool: "pomelo_books", cloud: true },
    Module { key: "films", label: "好奇看看", tool: "pomelo_films", cloud: true },
    Module { key: "drawing", label: "随意画画", tool: "pomelo_drawing", cloud: true },
    Module { key: "packing", label: "出门走走", tool: "pomelo_packing", cloud: true },
    Module { key: "finance", label: "钱来钱往", tool: "pomelo_finance", cloud: false },
    Module { key: "jobs", label: "投石问路", tool: "pomelo_jobs", cloud: true },
    Module { key: "notes", label: "念念不忘", tool: "pomelo_notes", cloud: true },
    Module { key: "days", label: "来日方长", tool: "pomelo_days", cloud: true },
];

/// The `module` of what a skill (skills/) brought from the web.
pub const WEB: &str = "web";

fn module(key: &str) -> Option<&'static Module> {
    MODULES.iter().find(|m| m.key == key)
}

// ---------- 设置 → 小拾: 允许发给云端 ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSetting {
    pub module: String,
    pub label: String,
    pub allowed: bool,
}

/// Per module (picky.db preferences), then the made ones (allowed unless turned off); unset ones keep
/// their default.
pub fn cloud_settings(conn: &Connection, made: &[Made]) -> Result<Vec<CloudSetting>, String> {
    let saved: HashMap<String, bool> = pref_get(conn, CLOUD_KEY).ok().flatten().unwrap_or_default();
    let fixed = MODULES.iter().map(|m| CloudSetting { module: m.key.into(), label: m.label.into(), allowed: saved.get(m.key).copied().unwrap_or(m.cloud) });
    let made = made.iter().map(|m| CloudSetting { module: m.key.clone(), label: m.name.clone(), allowed: saved.get(&m.key).copied().unwrap_or(true) });
    Ok(fixed.chain(made).collect())
}

pub fn set_cloud(conn: &Connection, key: &str, allowed: bool, made: &[Made]) -> Result<(), String> {
    if module(key).is_none() && !made.iter().any(|m| m.key == key) {
        return Err(format!("没有这个模块：{key}"));
    }
    let mut saved: HashMap<String, bool> = pref_get(conn, CLOUD_KEY).ok().flatten().unwrap_or_default();
    saved.insert(key.to_string(), allowed);
    pref_set(conn, CLOUD_KEY, &saved)
}

// ---------- what this answer may read ----------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Access {
    /// Modules hidden in 设置 → 侧栏 (pomelo.db `ui.module_hidden`).
    pub hidden: Vec<String>,
    /// The answer comes from a cloud model.
    pub cloud: bool,
    /// Modules that may go to cloud models (made ones by key).
    pub cloud_allowed: Vec<String>,
    /// The modules made from 五花八门 / 循序渐进.
    pub made: Vec<Made>,
}

/// A module made from 五花八门 / 循序渐进, as 小拾 sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    pub key: String,
    pub name: String,
    pub template: Template,
}

/// The made modules (pomelo.db).
pub fn made(pomelo: &Connection) -> Vec<Made> {
    custom::all(pomelo).map(|v| v.into_iter().map(|m| Made { key: m.key, name: m.name, template: m.template }).collect()).unwrap_or_default()
}

pub fn custom_keys(pomelo: &Connection) -> Vec<String> {
    made(pomelo).into_iter().map(|m| m.key).collect()
}

pub enum Blocked {
    Hidden,
    Cloud,
}

/// Modules hidden in 设置 → 侧栏 (pomelo.db).
pub fn hidden(pomelo: &Connection) -> Vec<String> {
    crate::settings::get(pomelo, HIDDEN_KEY).ok().flatten().and_then(|v| serde_json::from_value::<Vec<String>>(v).ok()).unwrap_or_default()
}

/// Modules allowed to go to cloud models (picky.db), with the made ones among `customs` (keys).
pub fn cloud_allowed(picky: &Connection, customs: &[String]) -> Vec<String> {
    let made: Vec<Made> = customs.iter().map(|k| Made { key: k.clone(), name: String::new(), template: Template::List }).collect();
    cloud_settings(picky, &made).unwrap_or_default().into_iter().filter(|s| s.allowed).map(|s| s.module).collect()
}

impl Access {
    fn check(&self, m: &Module) -> Result<(), Blocked> {
        if self.hidden.iter().any(|h| h == m.key) {
            Err(Blocked::Hidden)
        } else if self.cloud && !self.cloud_allowed.iter().any(|a| a == m.key) {
            Err(Blocked::Cloud)
        } else {
            Ok(())
        }
    }
    /// A made module (by key): hidden, or not for this cloud model.
    fn check_made(&self, key: &str) -> Result<(), Blocked> {
        if self.hidden.iter().any(|h| h == key) {
            Err(Blocked::Hidden)
        } else if self.cloud && !self.cloud_allowed.iter().any(|a| a == key) {
            Err(Blocked::Cloud)
        } else {
            Ok(())
        }
    }
    /// This answer may read `module` (小拾's own conversations always).
    pub fn allows(&self, module: &str) -> bool {
        module == "picky" || self.check_key(module)
    }
    fn check_key(&self, key: &str) -> bool {
        match (custom::parse_key(key), Template::parse(key)) {
            (Some(_), _) => self.check_made(key).is_ok(),
            // a result about several made modules: all of them must be readable
            (None, Some(t)) => self.made.iter().filter(|m| m.template == t).all(|m| self.check_made(&m.key).is_ok()),
            _ => MODULES.iter().find(|m| m.key == key).is_some_and(|m| self.check(m).is_ok()),
        }
    }
    /// An earlier result may go to this model (its module is readable and allowed here).
    pub fn may_send(&self, q: &Query) -> bool {
        // what a skill found on the web is no one's private data: it always goes along
        q.module.as_deref() == Some(WEB) || q.module.as_deref().is_some_and(|m| self.check_key(m))
    }
    /// Made modules of `t` with a tool this time (hidden ones don't count).
    pub(crate) fn made_offered(&self, t: Template) -> Vec<&Made> {
        self.made.iter().filter(|m| m.template == t && !self.hidden.contains(&m.key)).collect()
    }
    /// Modules that have a tool this time (hidden ones don't; cloud-refused ones do, to say so).
    pub fn offered(&self) -> Vec<&'static Module> {
        MODULES.iter().filter(|m| !self.hidden.iter().any(|h| h == m.key)).collect()
    }
    /// Labels of the modules this cloud model may not get (made ones by name).
    fn refused(&self) -> Vec<String> {
        let fixed = self.offered().into_iter().filter(|m| matches!(self.check(m), Err(Blocked::Cloud))).map(|m| m.label.to_string());
        let made = self.made.iter().filter(|m| matches!(self.check_made(&m.key), Err(Blocked::Cloud))).map(|m| m.name.clone());
        fixed.chain(made).collect()
    }
}

/// 「钱来钱往不发给云端，这部分没看」
fn cloud_sentence(labels: &[&str]) -> String {
    format!("{}不发给云端，这部分没看", labels.join("、"))
}

const WEEKDAYS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

/// The part of the system message about looking things up.
/// `verify`: 书单 / 影单检索 are offered (recommendations get checked).
pub fn instructions(today: &str, access: &Access, verify: bool) -> String {
    let wd = WEEKDAYS[(common::weekday(today) as usize + 6) % 7];
    let monday = jobs::week_start(today);
    let (last_mon, last_sun) = (common::add_days(&monday, -7), common::add_days(&monday, -1));
    let sunday = common::add_days(&monday, 6);
    let month = &today[..7];
    let mut s = format!(
        "【查数据】你可以调用工具查用户在「柚儿园」（Pomelo，用户自己的生活记录 App）里的数据，只能读。今天是 {today}（周{wd}）；本周是 {monday} 至 {sunday}，上周是 {last_mon} 至 {last_sun}，本月从 {month}-01 开始。\n\
         - 问题和这些数据有关时先查再答，可以连查几次；和这些数据无关就直接回答，不要查。\n\
         - 合计、平均、占比、变化、天数、预计完成这类数字以查到的为准，直接引用，不要自己重新加总或推算。\n\
         - 用户没说时间时，自己选合适的时间段，并在回答里写明看的是哪段时间。\n\
         - 查到的内容里没有的，就说没有记录，不要编造。提到具体的记录时写出它的完整名称。\n\
         - 要推荐书、电影或剧时，先用 view taste 看用户的口味，再推荐用户单子里还没有的新作品（{}）；不要只把用户读过看过的列一遍。\n\
         - 出行清单可能漏了什么，除了查到的，也可以按目的地和天数用常识补充。\n\
         - 问题后面附了【我正在看的页面】时，「这条」「这个」「这些」指那里打开或选中的记录：先用它回答，不够再查。\n{}",
        if verify { "推荐的每一部先用书单检索 / 影单检索核对，再写进回答" } else { "用你自己的知识，不联网，说明可能不准" },
        super::cite::INSTRUCTIONS
    );
    let refused = access.refused();
    if !refused.is_empty() {
        let sentence = cloud_sentence(&refused.iter().map(String::as_str).collect::<Vec<_>>());
        s.push_str(&format!("\n- 这些模块的数据不能发给现在用的云端模型：{}。问题涉及它们时不要查，在回答开头写明「{sentence}」，并提醒可以换本地模型再问。", refused.join("、")));
    }
    s
}

// ---------- tool definitions (no user data) ----------

fn tool(name: &str, description: &str, views: &[&str], extra: Value) -> Value {
    let mut props = json!({ "view": { "type": "string", "enum": views } });
    for (k, v) in extra.as_object().into_iter().flatten() {
        props[k] = v.clone();
    }
    json!({ "type": "function", "function": { "name": name, "description": description, "parameters": { "type": "object", "properties": props, "required": ["view"] } } })
}

fn date_param(what: &str) -> Value {
    json!({ "type": "string", "description": format!("{what}，YYYY-MM-DD（也可以写 YYYY-MM 或 YYYY）") })
}

fn definition(m: &Module) -> Value {
    let text = |d: &str| json!({ "type": "string", "description": d });
    match m.key {
        "todo" => tool(
            m.tool,
            "一步一步（待办）：分组 → 目标 → 任务。view：today 今天要做的（含已过期）和今天做完的；done 一段时间里做完的（含每次重复、打卡、进度记录，默认最近 7 天）；stuck 拖着的任务（过期的、加了很久还没做的）和最近没动的目标；open 所有没完成的任务（截止日期、过期天数、加入日期、所在分组和目标）；goals 目标（状态、时间段、进度、最近一次有完成的日子）。",
            &["today", "done", "stuck", "open", "goals"],
            json!({ "from": date_param("开始日期（done 用）"), "to": date_param("结束日期（done 用）"), "keyword": text("open：标题或内容里的字") }),
        ),
        "finance" => tool(
            m.tool,
            "钱来钱往（记账）。金额按币种分开（人民币、新加坡元、港币、美元），只有总资产折成人民币。view：assets 总资产在一段时间里的变化（每次更新余额那天的总数、各账户余额、持仓市值，默认最近半年）；entries 收支明细和按分类的合计（默认本月）；months 每月按币种、分类的收支合计（默认最近 6 个月）；budgets 某月的预算和已用（默认本月）；fixed 固定支出和订阅。",
            &["assets", "entries", "months", "budgets", "fixed"],
            json!({
                "from": date_param("开始日期"),
                "to": date_param("结束日期"),
                "flow": { "type": "string", "enum": ["out", "in"], "description": "entries：out 支出，in 收入，不填是两种都要" },
                "keyword": text("entries：备注或分类里的字"),
            }),
        ),
        "jobs" => tool(
            m.tool,
            "投石问路（求职投递）。等回音满 14 天就该跟进。view：follow_up 现在该跟进的和本周内会到期的；moves 一段时间里的进展（哪天哪份投递走到了哪一步，默认最近 14 天）；list 投递列表（现在的状态、时间线、渠道、备注；可按状态、关键词、投递日期筛选）；stats 统计（投出、本周投了、进行中、拿到、漏斗、最近 12 周每周投了几份、现在的状态分布）。",
            &["follow_up", "moves", "list", "stats"],
            json!({ "status": text("list：状态名称，比如 面试"), "keyword": text("list：公司、岗位、地点或备注里的字"), "from": date_param("moves / list：从（list 按投递日期）"), "to": date_param("moves / list：到") }),
        ),
        "books" | "films" => {
            let (thing, verb) = if m.key == "books" { ("书", "读") } else { ("电影和剧", "看") };
            tool(
                m.tool,
                &format!("{}（{thing}）。view：taste 用户想要推荐时用它：打了高分的（4 星以上）、偏好的类型和国家，和已经在单子里的全部名称（推荐新的、避开这些）；list 列出{thing}（作者 / 导演、国家、类型、状态、星级 1～5、{verb}完的年份、重{verb}、备注；可筛选）；stats 按状态、年份、星级的数量。", m.label),
                &["taste", "list", "stats"],
                json!({
                    "status": { "type": "string", "enum": ["todo", "doing", "done", "unmarked"], "description": format!("todo 想{verb}，doing 在{verb}，done 已{verb}，unmarked 待标记") },
                    "min_rating": { "type": "integer", "description": "最低星级 1～5" },
                    "year": text(&format!("{verb}完的年份（含重{verb}），YYYY")),
                    "keyword": text("名称、作者 / 导演、国家、类型或备注里的字"),
                }),
            )
        }
        "drawing" => tool(
            m.tool,
            "随意画画（绘画课）：课程 → 课节。view：courses 各门课和全部的进度（完成几节、还剩几节、下一节、最近 6 个月的速度、照这个速度预计哪个月画完）；lessons 某门课的课节（course 填课程名里的字；可按状态筛选）。",
            &["courses", "lessons"],
            json!({ "course": text("lessons：课程名里的字"), "status": { "type": "string", "enum": ["todo", "doing", "done"], "description": "todo 未开始，doing 进行中，done 已完成" } }),
        ),
        "packing" => tool(
            m.tool,
            "出门走走（出行清单）。view：trips 每次出行（出发日期、装好几件）；trip 某次出行的清单，和模板、以前的出行里常带但这次没有的（name 填出行名里的字，不填就是最近要出发的那次）；templates 场景模板和里面的物品。",
            &["trips", "trip", "templates"],
            json!({ "name": text("trip：出行名里的字") }),
        ),
        "notes" => tool(
            m.tool,
            "念念不忘（随手记的笔记，正文里写 #标签）。view：search 找和某件事有关的笔记（keyword 必填，可以写几个相关的词、用空格隔开，含任意一个就算，命中多的排前面）；recent 一段时间里的笔记全文和常用的标签（默认最近 30 天）；tags 全部标签和各有几条笔记。",
            &["search", "recent", "tags"],
            json!({ "keyword": text("search：几个相关的词，空格隔开（比如找读书相关的写「读书 书 阅读 摘抄」）"), "tag": text("只看这个标签（含下一级）"), "from": date_param("开始日期"), "to": date_param("结束日期") }),
        ),
        "days" => tool(
            m.tool,
            "来日方长（重要的日子：纪念日和倒数日、亲友的生日、证件到期）。天数、第几周年、几岁都已算好。view：soon 快到的（到了提前提醒的天数的、过期的证件）和接下来 60 天里的；list 全部日子（日期、下一次、还有 / 已经几天、重复、提醒、备注；生日还有礼物点子和送过的礼物；可按类型、关键词筛选）。",
            &["soon", "list"],
            json!({ "kind": { "type": "string", "enum": ["day", "birthday", "document"], "description": "list：day 纪念日和倒数日，birthday 生日，document 证件" }, "keyword": text("list：名称、备注或礼物里的字") }),
        ),
        _ => Value::Null,
    }
}

pub const LISTS_TOOL: &str = "pomelo_lists";
pub const PROGRESS_TOOL: &str = "pomelo_progress";

fn made_definition(t: Template) -> Value {
    let text = |d: &str| json!({ "type": "string", "description": d });
    match t {
        Template::List => tool(
            LISTS_TOOL,
            "五花八门（用户自己建的清单，比如游戏、想去的地方；每个清单有自己的字段）。view：lists 全部清单（名称、字段和类型、条数）；items 某个清单的全部条目和每个字段的值（list 填清单名里的字；keyword 只看名称或文字里含这些字的）。",
            &["lists", "items"],
            json!({ "list": text("items：清单名里的字"), "keyword": text("items：名称或文字里的字") }),
        ),
        Template::Progress => tool(
            PROGRESS_TOOL,
            "循序渐进（用户自己建的进度，分两层，比如课程 → 课节、季 → 集，每个进度叫法不同）。view：overview 全部进度：每一项完成了几节、进行中的、下一节、最近 6 个月的速度和照这个速度哪个月完成；lessons 某一项里的每一节（name 填那一项的名字里的字，可以在前面加上进度的名字；可按状态筛选）。",
            &["overview", "lessons"],
            json!({ "name": text("lessons：那一项的名字里的字"), "status": { "type": "string", "enum": ["todo", "doing", "done"], "description": "todo 未开始，doing 进行中，done 已完成" } }),
        ),
    }
}

/// The modules' views, then searching across them and reading a result (cite.rs).
pub fn tools(access: &Access) -> Vec<Value> {
    let mut out: Vec<Value> = access.offered().into_iter().map(definition).collect();
    for t in [Template::List, Template::Progress] {
        if !access.made_offered(t).is_empty() {
            out.push(made_definition(t));
        }
    }
    if !out.is_empty() {
        out.extend(super::cite::tools(access));
        out.extend(super::act::tools(access));
    }
    out
}

// ---------- running a call ----------

/// A record in a result (opened like a 全局搜索 hit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rec {
    pub module: String,
    pub kind: String,
    pub id: i64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<String>,
}

fn rec(module: &str, kind: &str, id: i64, title: &str) -> Rec {
    Rec { module: module.into(), kind: kind.into(), id, title: title.into(), parent: None, day: None }
}

/// What a call produced. state: ok | blocked | failed.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub module: Option<String>,
    pub label: String,
    pub state: &'static str,
    /// What the model gets.
    pub text: String,
    pub records: Vec<Rec>,
}

struct Row {
    text: String,
    rec: Option<Rec>,
}

fn row(text: String) -> Row {
    Row { text, rec: None }
}

struct Section {
    title: String,
    rows: Vec<Row>,
}

/// A view's result before cutting: the head always goes, rows while they fit.
struct Found {
    label: String,
    head: Vec<String>,
    sections: Vec<Section>,
}

impl Found {
    fn new(label: String, head: Vec<String>) -> Found {
        Found { label, head, sections: vec![] }
    }
    fn section(mut self, title: &str, rows: Vec<Row>) -> Found {
        self.sections.push(Section { title: title.into(), rows });
        self
    }
    fn rows(self, rows: Vec<Row>) -> Found {
        self.section("", rows)
    }

    fn render(self, budget: i64) -> (String, Vec<Rec>) {
        let est = super::context::estimate;
        let mut out: Vec<String> = self.head.clone();
        let mut used: i64 = out.iter().map(|l| est(l) + 1).sum();
        let mut recs = vec![];
        let total: usize = self.sections.iter().map(|s| s.rows.len()).sum();
        let mut shown = 0;
        'all: for s in self.sections {
            if !s.title.is_empty() {
                let t = if s.rows.is_empty() { format!("{}：没有", s.title) } else { format!("{}：", s.title) };
                used += est(&t) + 1;
                out.push(t);
            }
            for r in s.rows {
                let cost = est(&r.text) + 1;
                if used + cost > budget {
                    break 'all;
                }
                used += cost;
                out.push(r.text);
                recs.extend(r.rec);
                shown += 1;
            }
        }
        if shown < total {
            out.push(format!("……还有 {} 条没列出（可以缩短时间段或加条件再查）", total - shown));
        }
        (out.join("\n"), recs)
    }
}

/// Runs one call for the model. `budget`: tokens the result may take.
pub fn run(conn: &Connection, access: &Access, name: &str, arguments: &str, today: &str, budget: i64) -> Answer {
    if name == LISTS_TOOL || name == PROGRESS_TOOL {
        return made_run(conn, access, if name == LISTS_TOOL { Template::List } else { Template::Progress }, arguments, today, budget);
    }
    let Some(m) = MODULES.iter().find(|m| m.tool == name) else {
        return Answer { module: None, label: format!("没有这个查询：{}", clip(name, 40)), state: "failed", text: format!("没有叫 {name} 的工具，只能用给出的这些。"), records: vec![] };
    };
    match access.check(m) {
        Err(Blocked::Hidden) => {
            return Answer { module: Some(m.key.into()), label: format!("{} · 已隐藏，没看", m.label), state: "blocked", text: format!("没有查：{}在侧栏里隐藏了，小拾不读它的数据。", m.label), records: vec![] }
        }
        Err(Blocked::Cloud) => {
            let sentence = cloud_sentence(&[m.label]);
            return Answer {
                module: Some(m.key.into()),
                label: format!("{} · 不发给云端，没看", m.label),
                state: "blocked",
                text: format!("没有查：{}的数据不发给云端模型。请在回答开头写明「{sentence}」，并提醒可以换本地模型再问。", m.label),
                records: vec![],
            };
        }
        Ok(()) => {}
    }
    let args = match Args::parse(arguments) {
        Ok(a) => a,
        Err(e) => return failed(m, e),
    };
    let found = match m.key {
        "todo" => todo_view(conn, &args, today),
        "finance" => finance_view(conn, &args, today),
        "jobs" => jobs_view(conn, &args, today),
        "books" => books_view(conn, &args),
        "films" => films_view(conn, &args),
        "drawing" => drawing_view(conn, &args, today),
        "packing" => packing_view(conn, &args, today),
        "notes" => notes_view(conn, &args, today),
        "days" => days_view(conn, &args, today),
        _ => Err("没有这个模块".into()),
    };
    match found {
        Ok(f) => {
            let label = format!("{} · {}", m.label, f.label);
            let (text, records) = f.render(budget.min(MAX_RESULT));
            Answer { module: Some(m.key.into()), label, state: "ok", text, records }
        }
        Err(e) => failed(m, e),
    }
}

fn failed(m: &Module, e: String) -> Answer {
    Answer { module: Some(m.key.into()), label: format!("{} · 没查成", m.label), state: "failed", text: format!("没查成：{e}"), records: vec![] }
}

struct Args(Value);

impl Args {
    fn parse(s: &str) -> Result<Args, String> {
        if s.trim().is_empty() {
            return Ok(Args(json!({})));
        }
        let v: Value = serde_json::from_str(s).map_err(|_| "参数不是有效的 JSON".to_string())?;
        if v.is_object() {
            Ok(Args(v))
        } else {
            Err("参数要是一个 JSON 对象".into())
        }
    }
    fn str(&self, k: &str) -> Option<String> {
        match &self.0[k] {
            Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }
    fn int(&self, k: &str) -> Option<i64> {
        self.0[k].as_i64().or_else(|| self.str(k).and_then(|s| s.parse().ok()))
    }
    fn view<'a>(&self, views: &[&'a str]) -> Result<&'a str, String> {
        match self.str("view") {
            None => Ok(views[0]),
            Some(v) => views.iter().find(|x| **x == v).copied().ok_or_else(|| format!("view 只能是 {}", views.join(" / "))),
        }
    }
    /// [from, to] as days; YYYY-MM / YYYY widen to the whole month / year.
    fn range(&self, from: String, to: String) -> Result<(String, String), String> {
        let f = self.str("from").map(|s| day_of(&s, false)).transpose()?.unwrap_or(from);
        let t = self.str("to").map(|s| day_of(&s, true)).transpose()?.unwrap_or(to);
        if f > t {
            return Err(format!("开始日期 {f} 晚于结束日期 {t}"));
        }
        Ok((f, t))
    }
}

fn day_of(s: &str, end: bool) -> Result<String, String> {
    let s = s.replace('/', "-");
    if !s.is_ascii() {
        return Err(bad_day(&s)); // the model's text: never slice inside a character
    }
    let d = match s.len() {
        4 => format!("{s}-{}", if end { "12-31" } else { "01-01" }),
        7 => {
            let (y, m) = (s[..4].parse::<i64>().map_err(|_| bad_day(&s))?, s[5..].parse::<u32>().map_err(|_| bad_day(&s))?);
            if !(1..=12).contains(&m) {
                return Err(bad_day(&s));
            }
            format!("{s}-{:02}", if end { common::days_in_month(y, m) } else { 1 })
        }
        _ => s.clone(),
    };
    common::check_date(&Some(d.clone()), "日期").map(|_| d).map_err(|_| bad_day(&s))
}

fn bad_day(s: &str) -> String {
    format!("日期格式不对：{s}（应为 2026-10-01 这样的日期）")
}

// ---------- wording ----------

fn clip(s: &str, n: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > n {
        format!("{}…", flat.chars().take(n).collect::<String>())
    } else {
        flat
    }
}

fn plain(s: &str) -> String {
    s.replace("**", "")
}

fn month_text(m: &str) -> String {
    format!("{} 年 {} 月", &m[..4], m[5..7].trim_start_matches('0'))
}

fn md(d: &str) -> String {
    format!("{} 月 {} 日", d[5..7].trim_start_matches('0'), d[8..10].trim_start_matches('0'))
}

/// 2026 年 9 月 / 2026 年 / 2026 年 9 月 3 日 – 10 月 12 日 …
pub fn span(from: &str, to: &str) -> String {
    let month_end = |d: &str| format!("{}-{:02}", &d[..7], common::days_in_month(d[..4].parse().unwrap_or(2000), d[5..7].parse().unwrap_or(1)));
    if from == to {
        format!("{} 年 {}", &from[..4], md(from))
    } else if &from[..4] == &to[..4] && &from[4..] == "-01-01" && &to[4..] == "-12-31" {
        format!("{} 年", &from[..4])
    } else if &from[..7] == &to[..7] && &from[8..] == "01" && to == month_end(to) {
        month_text(from)
    } else if &from[..4] == &to[..4] {
        format!("{} 年 {} – {}", &from[..4], md(from), md(to))
    } else {
        format!("{} 年 {} – {} 年 {}", &from[..4], md(from), &to[..4], md(to))
    }
}

fn symbol(c: Currency) -> &'static str {
    match c {
        Currency::Cny => "¥",
        Currency::Sgd => "S$",
        Currency::Hkd => "HK$",
        Currency::Usd => "US$",
    }
}

fn currency_name(c: Currency) -> &'static str {
    match c {
        Currency::Cny => "人民币",
        Currency::Sgd => "新加坡元",
        Currency::Hkd => "港币",
        Currency::Usd => "美元",
    }
}

/// ¥1,234 / S$12.50 / -¥3 (cents in).
pub fn money(cents: i64, c: Currency) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let a = cents.unsigned_abs();
    let units = (a / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in units.chars().enumerate() {
        if i > 0 && (units.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let frac = if a % 100 == 0 { String::new() } else { format!(".{:02}", a % 100) };
    format!("{sign}{}{grouped}{frac}", symbol(c))
}

fn signed(cents: i64, c: Currency) -> String {
    if cents > 0 {
        format!("+{}", money(cents, c))
    } else {
        money(cents, c)
    }
}

fn percent(part: i64, whole: i64) -> String {
    if whole == 0 {
        return "—".into();
    }
    let p = part as f64 / whole as f64 * 100.0;
    format!("{}{:.1}%", if p > 0.0 { "+" } else { "" }, p)
}

fn number(x: f64) -> String {
    let s = format!("{x:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn stars(r: Option<i64>) -> String {
    r.map(|n| format!("★{n}")).unwrap_or_default()
}

fn join_bits(bits: Vec<String>) -> String {
    bits.into_iter().filter(|b| !b.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// Local "2026-10-01 14:39" of a UTC timestamp.
fn local_minute(conn: &Connection, ts: &str) -> String {
    conn.query_row("SELECT strftime('%Y-%m-%d %H:%M', ?1, 'localtime')", [ts], |r| r.get::<_, Option<String>>(0)).ok().flatten().unwrap_or_else(|| ts.chars().take(16).collect())
}

fn local_day(conn: &Connection, ts: &str) -> String {
    local_minute(conn, ts).chars().take(10).collect()
}

// ---------- 一步一步 ----------

fn rule_text(rule: &str) -> String {
    let (kind, v) = rule.split_once(':').unwrap_or((rule, ""));
    let list = |s: &str, f: &dyn Fn(&str) -> String| s.split(',').map(f).collect::<Vec<_>>().join("、");
    match kind {
        "daily" => "每天".into(),
        "weekly" => format!("每周{}", list(v, &|d| WEEKDAYS.get(d.parse::<usize>().unwrap_or(1).saturating_sub(1)).unwrap_or(&"?").to_string())),
        "monthly" => format!("每月 {} 号", list(v, &|d| d.to_string())),
        "yearly" => {
            let (md, every) = v.split_once('/').unwrap_or((v, "1"));
            if every == "1" { format!("每年 {md}") } else { format!("每 {every} 年的 {md}") }
        }
        "every" => format!("每 {} {}", &v[..v.len().saturating_sub(1)], if v.ends_with('w') { "周" } else { "天" }),
        "after" => {
            let unit = match v.chars().last() {
                Some('w') => "周",
                Some('m') => "个月",
                Some('y') => "年",
                _ => "天",
            };
            format!("做完后过 {} {unit}", &v[..v.len().saturating_sub(1)])
        }
        _ => rule.into(),
    }
}

fn task_rec(p: &todo::PlacedTask) -> Rec {
    let t = &p.task;
    Rec { parent: Some(t.goal_id.map_or(t.group_id, |g| -g)), day: t.due_date.clone(), ..rec("todo", "task", t.id, &plain(&t.title)) }
}

fn place(p: &todo::PlacedTask) -> String {
    let mut s = p.group_name.clone();
    if let Some(g) = &p.goal_name {
        s.push_str(&format!(" / {g}"));
    }
    if let Some(parent) = &p.parent_title {
        s.push_str(&format!(" / {}", plain(parent)));
    }
    s
}

fn task_line(p: &todo::PlacedTask, today: &str) -> String {
    let t = &p.task;
    let mut bits = vec![];
    match (&t.start_date, &t.due_date) {
        (Some(s), Some(d)) => bits.push(format!("{s} 至 {d}")),
        (None, Some(d)) => bits.push(format!("截止 {d}")),
        _ => {}
    }
    if let Some(d) = t.due_date.as_deref().filter(|d| !t.done && *d < today) {
        bits.push(format!("过期 {} 天", common::day_number(today) - common::day_number(d)));
    }
    match t.task_type {
        todo::TaskType::Repeat => bits.push(format!("重复：{}，已做 {} 次{}", rule_text(t.repeat_rule.as_deref().unwrap_or("")), t.count, t.repeat_total.map(|n| format!(" / 共 {n} 次")).unwrap_or_default())),
        todo::TaskType::Checkin => bits.push(format!("打卡：{}，连续 {} 天", rule_text(t.repeat_rule.as_deref().unwrap_or("")), t.streak)),
        todo::TaskType::Progress => bits.push(format!("进度 {} / {}{}", t.count, t.target.unwrap_or(0), t.unit.as_deref().map(|u| format!(" {u}")).unwrap_or_default())),
        todo::TaskType::Normal => {}
    }
    let note = t.note.as_deref().map(|n| format!("内容：{}", clip(&plain(n), 60))).unwrap_or_default();
    format!("- {}（{}）{}", plain(&t.title), join_bits(vec![join_bits(bits), place(p)]), if note.is_empty() { String::new() } else { format!(" {note}") })
}

/// 「拖着」: no due date, not done, added at least this many days ago; a goal 「没动」 when nothing in it
/// got done for this long.
pub const STUCK_DAYS: i64 = 14;

fn todo_view(conn: &Connection, a: &Args, today: &str) -> Result<Found, String> {
    match a.view(&["today", "done", "stuck", "open", "goals"])? {
        "stuck" => {
            let open = todo::open_tasks(conn, today)?;
            let goals = todo::goals(conn)?;
            let status: HashMap<i64, todo::GoalStatus> = goals.iter().map(|g| (g.id, g.status)).collect();
            let live = |p: &todo::PlacedTask| p.task.goal_id.and_then(|g| status.get(&g)).is_none_or(|s| *s == todo::GoalStatus::Active);
            let mut overdue: Vec<(&todo::PlacedTask, i64)> = open
                .iter()
                .filter(|p| live(p))
                .filter_map(|p| p.task.due_date.as_deref().filter(|d| *d < today).map(|d| (p, common::day_number(today) - common::day_number(d))))
                .collect();
            overdue.sort_by_key(|x| std::cmp::Reverse(x.1));
            let since = common::add_days(today, -STUCK_DAYS);
            let mut old: Vec<(&todo::PlacedTask, String)> = open
                .iter()
                .filter(|p| live(p) && p.task.due_date.is_none() && p.task.task_type == todo::TaskType::Normal)
                .map(|p| (p, local_day(conn, &p.task.created_at)))
                .filter(|(_, d)| d.as_str() <= since.as_str())
                .collect();
            old.sort_by(|x, y| x.1.cmp(&y.1));
            let last = todo::goals_last_done(conn)?;
            let groups: HashMap<i64, String> = todo::groups(conn)?.into_iter().map(|g| (g.id, g.name)).collect();
            let idle: Vec<&todo::Goal> = goals.iter().filter(|g| g.status == todo::GoalStatus::Active && last.get(&g.id).is_none_or(|d| d.as_str() < since.as_str())).collect();
            let head = vec![format!(
                "今天 {today}。过期 = 截止日期已过还没做完；久没动 = 没有截止日期、加了 {STUCK_DAYS} 天以上还没做；没动的目标 = 进行中、最近 {STUCK_DAYS} 天里面没有完成任何东西。暂停或已完成的目标里的任务不算。"
            )];
            Ok(Found::new(format!("拖着的任务和没动的目标（过期 {} 件，久没动 {} 件，目标 {} 个）", overdue.len(), old.len(), idle.len()), head)
                .section("过期的（过期久的在前）", overdue.iter().map(|(p, _)| Row { text: task_line(p, today), rec: Some(task_rec(p)) }).collect())
                .section("久没动的（加得早的在前）", old.iter().map(|(p, d)| Row { text: format!("{} · 加入于 {d}，{} 天了", task_line(p, today), common::day_number(today) - common::day_number(d)), rec: Some(task_rec(p)) }).collect())
                .section(
                    "没动的目标",
                    idle.iter()
                        .map(|g| {
                            let bits = vec![
                                format!("进度 {} / {}", g.done, g.total),
                                last.get(&g.id).map(|d| format!("最近一次有完成：{d}（{} 天前）", common::day_number(today) - common::day_number(d))).unwrap_or_else(|| "还没有完成过".into()),
                                g.end_date.clone().map(|e| format!("结束 {e}")).unwrap_or_default(),
                                groups.get(&g.group_id).cloned().unwrap_or_default(),
                            ];
                            Row { text: format!("- {}（{}）", g.name, join_bits(bits)), rec: Some(Rec { parent: Some(g.group_id), ..rec("todo", "goal", g.id, &g.name) }) }
                        })
                        .collect(),
                ))
        }
        "today" => {
            let ag = todo::agenda(conn, today)?;
            let line = |p: &todo::PlacedTask| Row { text: task_line(p, today), rec: Some(task_rec(p)) };
            Ok(Found::new(format!("今天（{} 件要做，过期 {} 件）", ag.due.len() + ag.overdue.len(), ag.overdue.len()), vec![format!("今天 {today}。暂停或已完成的目标里的任务不算。")])
                .section("已过期", ag.overdue.iter().map(line).collect())
                .section("今天要做", ag.due.iter().map(line).collect())
                .section("今天做完的", ag.done_today.iter().map(line).collect()))
        }
        "done" => {
            let (from, to) = a.range(common::add_days(today, -6), today.to_string())?;
            let items = todo::done_between(conn, &from, &to)?;
            let mut by_group: BTreeMap<String, usize> = BTreeMap::new();
            for i in &items {
                *by_group.entry(i.task.group_name.clone()).or_default() += 1;
            }
            let groups = by_group.iter().map(|(g, n)| format!("{g} {n}")).collect::<Vec<_>>().join("、");
            let head = vec![format!("{}（{from} 至 {to}）做完 {} 件{}。每次重复、打卡、进度记录各算一件。", span(&from, &to), items.len(), if groups.is_empty() { String::new() } else { format!("，按分组：{groups}") })];
            let rows = items
                .iter()
                .map(|i| Row { text: format!("- {} {}{}（{}）", i.day, plain(&i.task.task.title), i.detail.as_deref().map(|d| format!(" · {d}")).unwrap_or_default(), place(&i.task)), rec: Some(task_rec(&i.task)) })
                .collect();
            Ok(Found::new(format!("{}做完的（{} 件）", span(&from, &to), items.len()), head).rows(rows))
        }
        "open" => {
            let kw = a.str("keyword").map(|k| k.to_lowercase());
            let mut open: Vec<todo::PlacedTask> = todo::open_tasks(conn, today)?
                .into_iter()
                .filter(|p| kw.as_ref().is_none_or(|k| p.task.title.to_lowercase().contains(k) || p.task.note.as_deref().is_some_and(|n| n.to_lowercase().contains(k))))
                .collect();
            open.sort_by(|x, y| {
                let key = |p: &todo::PlacedTask| (p.task.due_date.is_none(), p.task.due_date.clone(), p.task.created_at.clone());
                key(x).cmp(&key(y))
            });
            let overdue = open.iter().filter(|p| p.task.due_date.as_deref().is_some_and(|d| d < today)).count();
            let goals: HashMap<i64, todo::GoalStatus> = todo::goals(conn)?.into_iter().map(|g| (g.id, g.status)).collect();
            let rows = open
                .iter()
                .map(|p| {
                    let paused = p.task.goal_id.and_then(|g| goals.get(&g)).is_some_and(|s| *s != todo::GoalStatus::Active);
                    Row { text: format!("{} · 加入于 {}{}", task_line(p, today), local_day(conn, &p.task.created_at), if paused { " · 目标已暂停或完成" } else { "" }), rec: Some(task_rec(p)) }
                })
                .collect();
            Ok(Found::new(format!("没完成的任务（{} 件）", open.len()), vec![format!("今天 {today}。没完成的任务 {} 件，其中过期 {overdue} 件（按截止日期排，没截止日期的在后）。", open.len())]).rows(rows))
        }
        _ => {
            let groups: HashMap<i64, String> = todo::groups(conn)?.into_iter().map(|g| (g.id, g.name)).collect();
            let last = todo::goals_last_done(conn)?;
            let goals = todo::goals(conn)?;
            let status = |s: todo::GoalStatus| match s {
                todo::GoalStatus::Active => "进行中",
                todo::GoalStatus::Paused | todo::GoalStatus::Abandoned => "暂停",
                todo::GoalStatus::Done => "已完成",
            };
            let rows = goals
                .iter()
                .map(|g| {
                    let dates = match (&g.start_date, &g.end_date) {
                        (Some(s), Some(e)) => format!("{s} 至 {e}"),
                        (None, Some(e)) => format!("到 {e}"),
                        (Some(s), None) => format!("从 {s}"),
                        _ => String::new(),
                    };
                    let bits = vec![
                        status(g.status).to_string(),
                        dates,
                        format!("进度 {} / {}", g.done, g.total),
                        last.get(&g.id).map(|d| format!("最近一次有完成：{d}")).unwrap_or_else(|| "还没有完成过".into()),
                        groups.get(&g.group_id).cloned().unwrap_or_default(),
                    ];
                    Row { text: format!("- {}（{}）{}", g.name, join_bits(bits), g.review.as_deref().map(|r| format!(" 复盘：{}", clip(r, 80))).unwrap_or_default()), rec: Some(Rec { parent: Some(g.group_id), ..rec("todo", "goal", g.id, &g.name) }) }
                })
                .collect();
            Ok(Found::new(format!("目标（{} 个）", goals.len()), vec![format!("今天 {today}。进度 = 完成的最底层任务 / 全部最底层任务。")]).rows(rows))
        }
    }
}

// ---------- 钱来钱往 ----------

fn account_kind(k: AccountKind) -> &'static str {
    match k {
        AccountKind::Bank => "银行",
        AccountKind::Wallet => "钱包",
        AccountKind::Broker => "券商",
    }
}

fn flow_text(f: Flow) -> &'static str {
    match f {
        Flow::Out => "支出",
        Flow::In => "收入",
    }
}

fn month_start(d: &str) -> String {
    format!("{}-01", &d[..7])
}

fn finance_view(conn: &Connection, a: &Args, today: &str) -> Result<Found, String> {
    match a.view(&["assets", "entries", "months", "budgets", "fixed"])? {
        "assets" => assets_view(conn, a, today),
        "entries" => entries_view(conn, a, today),
        "months" => months_view(conn, a, today),
        "budgets" => budgets_view(conn, a, today),
        _ => {
            let out_k = kinds::name_map(conn, finance::OUT_KINDS)?;
            let in_k = kinds::name_map(conn, finance::IN_KINDS)?;
            let list = finance::fixed_list(conn)?;
            let rows = list
                .iter()
                .map(|f| {
                    let kind = f.kind_id.and_then(|k| if f.flow == Flow::Out { out_k.get(&k) } else { in_k.get(&k) }).cloned().unwrap_or_else(|| "未分类".into());
                    let rule = match finance::FixedRule::parse(&f.rule) {
                        Ok(finance::FixedRule::Monthly(d)) => format!("每月 {d} 号"),
                        Ok(finance::FixedRule::Yearly(m, d)) => format!("每年 {m} 月 {d} 日"),
                        Err(_) => f.rule.clone(),
                    };
                    let bits = vec![flow_text(f.flow).to_string(), money(f.amount, f.currency), rule, kind, if f.stopped { "已停用".into() } else { format!("下次 {}", f.next_day) }];
                    Row { text: format!("- {}（{}）", f.name, join_bits(bits)), rec: Some(rec("finance", "fixed", f.id, &f.name)) }
                })
                .collect();
            Ok(Found::new(format!("固定支出（{} 项）", list.len()), vec!["到了日子自动记一笔。".into()]).rows(rows))
        }
    }
}

fn assets_view(conn: &Connection, a: &Args, today: &str) -> Result<Found, String> {
    let (from, to) = a.range(common::add_days(today, -182), today.to_string())?;
    let all = finance::assets(conn)?;
    let accounts = finance::accounts(conn)?;
    let inside: Vec<&finance::DayTotal> = all.totals.iter().filter(|t| t.day >= from && t.day <= to).collect();
    let full: Vec<(&str, i64)> = inside.iter().filter_map(|t| t.total_cny.map(|v| (t.day.as_str(), v))).collect();
    let before = all.totals.iter().filter(|t| t.day < from && t.total_cny.is_some()).next_back();
    let mut head = vec![format!("总资产 = 每次更新余额那天各账户最近的余额按汇率折成人民币。{}（{from} 至 {to}）更新过 {} 次。", span(&from, &to), inside.len())];
    if let (Some(first), Some(last)) = (full.first(), full.last()) {
        let hi = full.iter().max_by_key(|x| x.1).unwrap_or(first);
        let lo = full.iter().min_by_key(|x| x.1).unwrap_or(first);
        head.push(format!(
            "最早 {} {}，最近 {} {}，变化 {}（{}）；最高 {} {}，最低 {} {}。",
            first.0,
            money(first.1, Currency::Cny),
            last.0,
            money(last.1, Currency::Cny),
            signed(last.1 - first.1, Currency::Cny),
            percent(last.1 - first.1, first.1),
            hi.0,
            money(hi.1, Currency::Cny),
            lo.0,
            money(lo.1, Currency::Cny)
        ));
    }
    if let Some(b) = before {
        head.push(format!("这段时间之前最后一次：{} {}。", b.day, money(b.total_cny.unwrap_or(0), Currency::Cny)));
    }
    let mut prev = before.and_then(|b| b.total_cny);
    let totals = inside
        .iter()
        .map(|t| {
            row(match t.total_cny {
                Some(v) => {
                    let change = prev.map(|p| format!("（比上次 {}）", signed(v - p, Currency::Cny))).unwrap_or_default();
                    prev = Some(v);
                    format!("- {} {}{change}", t.day, money(v, Currency::Cny))
                }
                None => format!("- {} 缺{}的汇率，没算总数", t.day, t.missing.iter().map(|c| currency_name(*c)).collect::<Vec<_>>().join("、")),
            })
        })
        .collect();
    // a change in CNY at the end day's rate, to rank accounts / holdings in different currencies
    let in_cny = |cents: i64, c: Currency, day: &str| finance::rate_on(&all.rates, c, day).map(|r| (cents as f64 * r).round() as i64);
    let ranked_text = |name: &str, change: i64, c: Currency, cny: Option<i64>| {
        let approx = if c == Currency::Cny { String::new() } else { cny.map(|v| format!("（约 {}）", signed(v, Currency::Cny))).unwrap_or_default() };
        format!("{name} {}{approx}", signed(change, c))
    };
    // each account: its balance as of `from` (or its first one inside) → as of `to`
    let mut accs: Vec<(Option<i64>, String, Row)> = vec![];
    for acc in &accounts {
        let mine: Vec<&finance::Balance> = all.balances.iter().filter(|b| b.account_id == acc.id && b.day <= to).collect();
        let Some(end) = mine.last() else { continue };
        let start = mine.iter().rev().find(|b| b.day <= from).or_else(|| mine.iter().find(|b| b.day >= from)).copied().unwrap_or(end);
        let bits = vec![account_kind(acc.kind).to_string(), currency_name(acc.currency).to_string(), if acc.closed { "已停用".into() } else { String::new() }];
        let (text, change_cny) = if start.day == end.day {
            (format!("- {}（{}）：{} {}，这段时间没更新", acc.name, join_bits(bits), end.day, money(end.amount, acc.currency)), None)
        } else {
            let change = end.amount - start.amount;
            let cny = in_cny(change, acc.currency, &end.day);
            let approx = if acc.currency == Currency::Cny { String::new() } else { cny.map(|v| format!("，约 {}", signed(v, Currency::Cny))).unwrap_or_default() };
            (
                format!(
                    "- {}（{}）：{} {} → {} {}，变化 {}（{}{approx}）",
                    acc.name,
                    join_bits(bits),
                    start.day,
                    money(start.amount, acc.currency),
                    end.day,
                    money(end.amount, acc.currency),
                    signed(change, acc.currency),
                    percent(change, start.amount)
                ),
                cny,
            )
        };
        let summary = change_cny.map(|v| ranked_text(&acc.name, end.amount - start.amount, acc.currency, Some(v))).unwrap_or_default();
        accs.push((change_cny, summary, Row { text, rec: Some(rec("finance", "account", acc.id, &acc.name)) }));
    }
    // holdings: per broker account, the snapshot as of `from` (or the first inside) → as of `to`
    let holdings = finance::holdings(conn)?;
    let mut holds: Vec<(Option<i64>, String, Row)> = vec![];
    for acc in accounts.iter().filter(|x| x.kind == AccountKind::Broker) {
        let mut days: Vec<&str> = holdings.iter().filter(|h| h.account_id == acc.id && h.day.as_str() <= to.as_str()).map(|h| h.day.as_str()).collect();
        days.dedup();
        let Some(&end) = days.last() else { continue };
        let start = days.iter().rev().find(|d| **d <= from.as_str()).or_else(|| days.iter().find(|d| **d >= from.as_str())).copied().unwrap_or(end);
        let at = |day: &str| holdings.iter().filter(|h| h.account_id == acc.id && h.day == day).collect::<Vec<_>>();
        let (old, new) = (at(start), at(end));
        let mut names: Vec<&str> = new.iter().map(|h| h.name.as_str()).collect();
        names.extend(old.iter().map(|h| h.name.as_str()).filter(|n| !new.iter().any(|h| h.name == *n)));
        for n in names {
            let b = old.iter().find(|h| h.name == n);
            let e = new.iter().find(|h| h.name == n);
            let now = e.map(|h| format!("{} × {} = {}", number(h.quantity), number(h.price), money(h.value, acc.currency))).unwrap_or_else(|| "已不持有".into());
            let change = match (b, e) {
                _ if start == end => None,
                (Some(b), Some(e)) => Some(e.value - b.value),
                (None, Some(e)) => Some(e.value),
                (Some(b), None) => Some(-b.value),
                (None, None) => None,
            };
            let detail = match (b, e) {
                _ if start == end => String::new(),
                (Some(b), Some(e)) => format!("，{start} {} → {end} {}，变化 {}（{}）", money(b.value, acc.currency), money(e.value, acc.currency), signed(e.value - b.value, acc.currency), percent(e.value - b.value, b.value)),
                (None, Some(_)) => format!("，{start} 之后新买入"),
                (Some(b), None) => format!("，{start} 时 {}", money(b.value, acc.currency)),
                (None, None) => String::new(),
            };
            let cny = change.and_then(|c| in_cny(c, acc.currency, end));
            let summary = change.map(|c| ranked_text(&format!("{} · {n}", acc.name), c, acc.currency, cny)).unwrap_or_default();
            holds.push((cny, summary, Row { text: format!("- {} · {}：{end} {now}{detail}", acc.name, n), rec: Some(rec("finance", "account", acc.id, &acc.name)) }));
        }
    }
    // biggest changes first
    for list in [&mut accs, &mut holds] {
        list.sort_by_key(|x| std::cmp::Reverse(x.0.map(i64::abs)));
    }
    let top = |list: &[(Option<i64>, String, Row)]| list.iter().filter(|x| x.0.is_some_and(|v| v != 0)).take(3).map(|x| x.1.clone()).collect::<Vec<_>>().join("、");
    let (ta, th) = (top(&accs), top(&holds));
    if !ta.is_empty() || !th.is_empty() {
        head.push(format!(
            "按人民币折算（期末汇率）变化最大的{}{}。",
            if ta.is_empty() { String::new() } else { format!("账户：{ta}") },
            if th.is_empty() { String::new() } else { format!("{}持仓：{th}", if ta.is_empty() { "" } else { "；" }) }
        ));
    }
    Ok(Found::new(format!("总资产 {}（{} 次更新）", span(&from, &to), inside.len()), head)
        .section("每次更新的总资产", totals)
        .section("各账户余额（各自的币种，期初 → 期末，变化大的在前）", accs.into_iter().map(|x| x.2).collect())
        .section("持仓市值（各自的币种，变化大的在前）", holds.into_iter().map(|x| x.2).collect()))
}

fn entries_view(conn: &Connection, a: &Args, today: &str) -> Result<Found, String> {
    let (from, to) = a.range(month_start(today), today.to_string())?;
    let flow = match a.str("flow").as_deref() {
        None => None,
        Some("out" | "支出") => Some(Flow::Out),
        Some("in" | "收入") => Some(Flow::In),
        Some(x) => return Err(format!("flow 只能是 out 或 in，不是 {x}")),
    };
    let kw = a.str("keyword").map(|k| k.to_lowercase());
    let out_k = kinds::name_map(conn, finance::OUT_KINDS)?;
    let in_k = kinds::name_map(conn, finance::IN_KINDS)?;
    let kind_of = |e: &finance::Entry| e.kind_id.and_then(|k| if e.flow == Flow::Out { out_k.get(&k) } else { in_k.get(&k) }).cloned().unwrap_or_else(|| "未分类".into());
    let list: Vec<finance::Entry> = finance::entries_between(conn, &from, &to)?
        .into_iter()
        .filter(|e| flow.is_none_or(|f| e.flow == f))
        .filter(|e| kw.as_ref().is_none_or(|k| kind_of(e).to_lowercase().contains(k) || e.note.as_deref().is_some_and(|n| n.to_lowercase().contains(k))))
        .collect();
    // sums of the parts inside the range, by flow · currency (· 分类)
    let mut sums: BTreeMap<(&str, &str), (Flow, Currency, i64)> = BTreeMap::new();
    let mut by_kind: BTreeMap<(&str, &str, String), i64> = BTreeMap::new();
    let mut periods = false;
    for e in &list {
        let part = finance::share_between(e.amount, &e.day, e.end_day.as_deref(), &from, &to);
        periods |= e.end_day.is_some();
        sums.entry((e.flow.as_str(), e.currency.as_str())).or_insert((e.flow, e.currency, 0)).2 += part;
        *by_kind.entry((e.flow.as_str(), e.currency.as_str(), kind_of(e))).or_default() += part;
    }
    let what = flow.map_or("收支", flow_text);
    let mut head = vec![format!("{}（{from} 至 {to}）{what} {} 笔。", span(&from, &to), list.len())];
    for (f, c, total) in sums.values() {
        let mut kinds: Vec<(&String, i64)> = by_kind.iter().filter(|((kf, kc, _), _)| *kf == f.as_str() && *kc == c.as_str()).map(|((_, _, k), v)| (k, *v)).collect();
        kinds.sort_by(|x, y| y.1.cmp(&x.1));
        let parts = kinds.iter().map(|(k, v)| format!("{k} {}（{}）", money(*v, *c), percent(*v, *total).trim_start_matches('+'))).collect::<Vec<_>>().join("、");
        head.push(format!("{}（{}）合计 {}；按分类：{parts}", flow_text(*f), currency_name(*c), money(*total, *c)));
    }
    if periods {
        head.push("「记小额」是一段时间的合计，按天平摊，合计里只算落在这段时间里的部分。".into());
    }
    let rows = list
        .iter()
        .map(|e| {
            let when = match &e.end_day {
                Some(end) => format!("{} – {}（记小额）", e.day, end),
                None => e.day.clone(),
            };
            let part = finance::share_between(e.amount, &e.day, e.end_day.as_deref(), &from, &to);
            let share = if part != e.amount { format!("，算进这段的 {}", money(part, e.currency)) } else { String::new() };
            let note = e.note.as_deref().map(|n| format!(" · {}", clip(n, 40))).unwrap_or_default();
            let title = e.note.clone().unwrap_or_else(|| kind_of(e));
            Row {
                text: format!("- {when} {} {} {}{share}{note}{}", flow_text(e.flow), kind_of(e), money(e.amount, e.currency), if e.fixed_id.is_some() { " · 固定支出" } else { "" }),
                rec: Some(Rec { day: Some(e.day.clone()), ..rec("finance", "entry", e.id, &title) }),
            }
        })
        .collect();
    Ok(Found::new(format!("{}{what}（{} 笔）", span(&from, &to), list.len()), head).rows(rows))
}

fn months_view(conn: &Connection, a: &Args, today: &str) -> Result<Found, String> {
    let six = month_start(&common::add_days(&month_start(today), -150));
    let (from, to) = a.range(six, today.to_string())?;
    let (m0, m1) = (&from[..7], &to[..7]);
    let out_k = kinds::name_map(conn, finance::OUT_KINDS)?;
    let in_k = kinds::name_map(conn, finance::IN_KINDS)?;
    let totals: Vec<finance::Total> = finance::totals(conn)?.into_iter().filter(|t| t.month.as_str() >= m0 && t.month.as_str() <= m1).collect();
    let mut months: BTreeMap<(&str, &str), Vec<&finance::Total>> = BTreeMap::new();
    for t in &totals {
        months.entry((t.month.as_str(), t.currency.as_str())).or_default().push(t);
    }
    let rows = months
        .iter()
        .rev()
        .map(|((month, _), ts)| {
            let c = ts[0].currency;
            let sum = |f: Flow| ts.iter().filter(|t| t.flow == f).map(|t| t.amount).sum::<i64>();
            let mut outs: Vec<&&finance::Total> = ts.iter().filter(|t| t.flow == Flow::Out).collect();
            outs.sort_by(|x, y| y.amount.cmp(&x.amount));
            let kinds = outs.iter().take(6).map(|t| format!("{} {}", t.kind_id.and_then(|k| out_k.get(&k)).map_or("未分类", |s| s.as_str()), money(t.amount, c))).collect::<Vec<_>>().join("、");
            let ins = ts.iter().filter(|t| t.flow == Flow::In).map(|t| format!("{} {}", t.kind_id.and_then(|k| in_k.get(&k)).map_or("未分类", |s| s.as_str()), money(t.amount, c))).collect::<Vec<_>>().join("、");
            let (o, i) = (sum(Flow::Out), sum(Flow::In));
            row(format!("- {} {}：支出 {}{}，收入 {}{}，结余 {}", month_text(month), currency_name(c), money(o, c), if kinds.is_empty() { String::new() } else { format!("（{kinds}）") }, money(i, c), if ins.is_empty() { String::new() } else { format!("（{ins}）") }, money(i - o, c)))
        })
        .collect();
    Ok(Found::new(format!("每月收支 {} – {}", month_text(m0), month_text(m1)), vec!["按币种分开算，不换算；「记小额」按天平摊到各月。新的月份在前。".into()]).rows(rows))
}

fn budgets_view(conn: &Connection, a: &Args, today: &str) -> Result<Found, String> {
    let month = a.str("from").map(|s| day_of(&s, false)).transpose()?.unwrap_or_else(|| today.to_string())[..7].to_string();
    let names = kinds::name_map(conn, finance::OUT_KINDS)?;
    let spent: HashMap<(Option<i64>, &str), i64> = finance::totals(conn)?
        .iter()
        .filter(|t| t.month == month && t.flow == Flow::Out)
        .map(|t| ((t.kind_id, t.currency.as_str()), t.amount))
        .collect();
    let budgets = finance::budgets(conn)?;
    let rows = budgets
        .iter()
        .map(|b| {
            let used = spent.get(&(Some(b.kind_id), b.currency.as_str())).copied().unwrap_or(0);
            let state = if used > b.amount {
                format!("超出 {}", money(used - b.amount, b.currency))
            } else if used * 10 >= b.amount * 9 {
                format!("快到了，还剩 {}", money(b.amount - used, b.currency))
            } else {
                format!("还剩 {}", money(b.amount - used, b.currency))
            };
            row(format!("- {}（{}）：预算 {}，已用 {}（{}），{state}", names.get(&b.kind_id).map_or("?", |s| s.as_str()), currency_name(b.currency), money(b.amount, b.currency), money(used, b.currency), percent(used, b.amount).trim_start_matches('+')))
        })
        .collect();
    Ok(Found::new(format!("{}预算（{} 项）", month_text(&month), budgets.len()), vec![format!("{}：每个「支出分类 + 币种」的月预算和这个月这个币种已经花的（记小额按天平摊）。", month_text(&month))]).rows(rows))
}

// ---------- 投石问路 ----------

fn job_title(j: &jobs::Job) -> String {
    match &j.position {
        Some(p) => format!("{} · {p}", j.company),
        None => j.company.clone(),
    }
}

fn job_rec(j: &jobs::Job) -> Rec {
    rec("jobs", "job", j.id, &job_title(j))
}

fn jobs_view(conn: &Connection, a: &Args, today: &str) -> Result<Found, String> {
    let view = a.view(&["follow_up", "moves", "list", "stats"])?;
    let sts = jobs::statuses(conn)?;
    let name_of = |id: Option<i64>| id.and_then(|i| sts.iter().find(|s| s.id == i)).map_or("想投".to_string(), |s| s.name.clone());
    let all = jobs::jobs(conn, today)?;
    match view {
        "follow_up" => {
            let sunday = common::add_days(&jobs::week_start(today), 6);
            let mut now: Vec<&jobs::Job> = all.iter().filter(|j| j.follow_up).collect();
            now.sort_by_key(|j| std::cmp::Reverse(j.waiting_days));
            let mut soon: Vec<(&jobs::Job, String)> = all
                .iter()
                .filter(|j| !j.follow_up && j.stage == Stage::Applied)
                .filter_map(|j| j.waiting_days.map(|w| (j, common::add_days(today, jobs::FOLLOW_UP_DAYS - w))))
                .filter(|(_, due)| due.as_str() <= sunday.as_str())
                .collect();
            soon.sort_by(|x, y| x.1.cmp(&y.1));
            let line = |j: &jobs::Job| {
                let since = j.steps.iter().rev().find(|s| s.done).and_then(|s| s.done_on.clone()).unwrap_or_default();
                format!("- {}{}：{} {since}，等了 {} 天{}", job_title(j), j.location.as_deref().map(|l| format!(" · {l}")).unwrap_or_default(), name_of(j.status_id), j.waiting_days.unwrap_or(0), j.followed_on.as_deref().map(|d| format!("（{d} 跟进过）")).unwrap_or_default())
            };
            let head = vec![format!("今天 {today}。在等回音的投递，离那一步（或上次跟进）满 {} 天就该跟进。现在该跟进 {} 份；本周内（到 {sunday}）还会到期 {} 份。", jobs::FOLLOW_UP_DAYS, now.len(), soon.len())];
            Ok(Found::new(format!("该跟进的投递（{} 份{}）", now.len(), if soon.is_empty() { String::new() } else { format!("，本周还有 {} 份", soon.len()) }), head)
                .section("现在该跟进", now.iter().map(|j| Row { text: line(j), rec: Some(job_rec(j)) }).collect())
                .section("本周内会到期", soon.iter().map(|(j, due)| Row { text: format!("{}，{due} 到期", line(j)), rec: Some(job_rec(j)) }).collect()))
        }
        "moves" => {
            let (from, to) = a.range(common::add_days(today, -13), today.to_string())?;
            let mut moves: Vec<(&str, &jobs::Job, String)> = vec![];
            for j in &all {
                for st in j.steps.iter().filter(|x| x.done) {
                    if let Some(d) = st.done_on.as_deref().filter(|d| *d >= from.as_str() && *d <= to.as_str()) {
                        moves.push((d, j, name_of(Some(st.status_id))));
                    }
                }
            }
            moves.sort_by(|x, y| y.0.cmp(x.0).then(y.1.id.cmp(&x.1.id)));
            let mut touched: Vec<i64> = moves.iter().map(|m| m.1.id).collect();
            touched.sort_unstable();
            touched.dedup();
            let new = all.iter().filter(|j| j.applied_on.as_deref().is_some_and(|d| d >= from.as_str() && d <= to.as_str())).count();
            let head = vec![format!("{}（{from} 至 {to}）时间线上勾了 {} 步，涉及 {} 份投递；其中新投出 {new} 份。新的在前。", span(&from, &to), moves.len(), touched.len())];
            let rows = moves.iter().map(|(d, j, st)| Row { text: format!("- {d} {}：{st}（现在：{}）", job_title(j), name_of(j.status_id)), rec: Some(job_rec(j)) }).collect();
            Ok(Found::new(format!("{}的进展（{} 步）", span(&from, &to), moves.len()), head).rows(rows))
        }
        "list" => {
            let channels = kinds::name_map(conn, jobs::CHANNELS)?;
            let status = a.str("status");
            let kw = a.str("keyword").map(|k| k.to_lowercase());
            let dated = a.str("from").is_some() || a.str("to").is_some();
            let (from, to) = a.range("1900-01-01".into(), "2100-12-31".into())?;
            let list: Vec<&jobs::Job> = all
                .iter()
                .filter(|j| status.as_ref().is_none_or(|s| name_of(j.status_id).contains(s.as_str())))
                .filter(|j| {
                    kw.as_ref().is_none_or(|k| {
                        [Some(&j.company), j.position.as_ref(), j.location.as_ref(), j.note.as_ref()].into_iter().flatten().any(|x| x.to_lowercase().contains(k))
                    })
                })
                .filter(|j| !dated || j.applied_on.as_deref().is_some_and(|d| d >= from.as_str() && d <= to.as_str()))
                .collect();
            let rows = list
                .iter()
                .map(|j| {
                    let timeline = j.steps.iter().map(|s| format!("{} {}", name_of(Some(s.status_id)), if s.done { s.done_on.clone().unwrap_or_else(|| "✓".into()) } else { "—".into() })).collect::<Vec<_>>().join(" → ");
                    let bits = vec![
                        j.location.clone().unwrap_or_default(),
                        j.kind_id.and_then(|k| channels.get(&k).cloned()).unwrap_or_default(),
                        format!("现在：{}", name_of(j.status_id)),
                        j.applied_on.as_deref().map(|d| format!("投递 {d}")).unwrap_or_else(|| "还没投".into()),
                        if j.follow_up { format!("该跟进（等了 {} 天）", j.waiting_days.unwrap_or(0)) } else { j.waiting_days.map(|w| format!("等了 {w} 天")).unwrap_or_default() },
                    ];
                    let extra = [
                        (!timeline.is_empty()).then(|| format!("时间线：{timeline}")),
                        j.resume.as_deref().map(|r| format!("简历：{r}")),
                        j.note.as_deref().map(|n| format!("备注：{}", clip(n, 80))),
                    ];
                    Row { text: format!("- {}（{}）{}", job_title(j), join_bits(bits), extra.into_iter().flatten().map(|x| format!(" {x}")).collect::<String>()), rec: Some(job_rec(j)) }
                })
                .collect();
            let mut filters = vec![];
            if let Some(s) = &status {
                filters.push(format!("状态含「{s}」"));
            }
            if let Some(k) = a.str("keyword") {
                filters.push(format!("含「{k}」"));
            }
            if dated {
                filters.push(format!("投递日期 {from} 至 {to}"));
            }
            let f = if filters.is_empty() { String::new() } else { format!("（{}）", filters.join("，")) };
            Ok(Found::new(format!("投递{}（{} 份）", if filters.is_empty() { String::new() } else { format!(" · {}", filters.join("，")) }, list.len()), vec![format!("今天 {today}。投递{f} {} 份，按现在的状态排。", list.len())]).rows(rows))
        }
        _ => {
            let s = jobs::stats_of(&all, &sts, today);
            let head = vec![format!("今天 {today}。投出 {} 份（有投递日期的），本周投了 {} 份，进行中 {} 份，拿到 {} 份。", s.applied, s.this_week, s.ongoing, s.got)];
            let funnel = s.funnel.iter().map(|f| row(format!("- {}：{} / {} 份（勾上的 / 时间线上有这一步的）", f.name, f.done, f.have))).collect();
            let weeks = s.weeks.iter().rev().map(|b| row(format!("- {} 那一周：{} 份", b.from, b.count))).collect();
            let now = s.statuses.iter().map(|x| row(format!("- {}：{} 份", x.name, x.count))).collect();
            Ok(Found::new("统计".into(), head).section("漏斗（只看至少勾了一步的投递）", funnel).section("现在的状态", now).section("最近 12 周每周投了几份（新的在前）", weeks))
        }
    }
}

// ---------- 摸鱼读读 · 好奇看看 ----------

struct Work {
    id: i64,
    title: String,
    by: Option<String>,
    country: Option<String>,
    kind: String,
    status: Mark,
    rating: Option<i64>,
    year: Option<String>,
    note: Option<String>,
    rereads: Vec<common::Reread>,
}

fn status_text(s: Mark, verb: &str) -> String {
    match s {
        Mark::Unmarked => "待标记".into(),
        Mark::Todo => format!("想{verb}"),
        Mark::Doing => format!("在{verb}"),
        Mark::Done => format!("已{verb}"),
    }
}

fn parse_mark(s: &str) -> Option<Mark> {
    Mark::parse(s).or(match s {
        "想读" | "想看" => Some(Mark::Todo),
        "在读" | "在看" => Some(Mark::Doing),
        "已读" | "已看" | "读过" | "看过" => Some(Mark::Done),
        "待标记" => Some(Mark::Unmarked),
        _ => None,
    })
}

fn work_line(w: &Work, verb: &str, by_label: &str) -> String {
    let rereads = w.rereads.iter().map(|r| format!("{}{}", r.year.clone().unwrap_or_else(|| "?".into()), r.rating.map(|n| format!(" ★{n}")).unwrap_or_default())).collect::<Vec<_>>().join("、");
    let bits = vec![
        w.by.clone().map(|b| format!("{by_label} {b}")).unwrap_or_default(),
        w.country.clone().unwrap_or_default(),
        w.kind.clone(),
        status_text(w.status, verb),
        w.year.clone().map(|y| format!("{y} {verb}完")).unwrap_or_default(),
        stars(w.rating),
        if rereads.is_empty() { String::new() } else { format!("重{verb}：{rereads}") },
    ];
    format!("- 《{}》（{}）{}", w.title, join_bits(bits), w.note.as_deref().map(|n| format!(" 备注：{}", clip(n, 80))).unwrap_or_default())
}

/// The best rating a work got (first time or any reread).
fn best(w: &Work) -> Option<i64> {
    w.rereads.iter().filter_map(|r| r.rating).chain(w.rating).max()
}

fn works_view(a: &Args, works: Vec<Work>, module: &str, kind: &str, verb: &str, unit: &str, by_label: &str) -> Result<Found, String> {
    let view = a.view(&["taste", "list", "stats"])?;
    if view == "taste" {
        let mut high: Vec<&Work> = works.iter().filter(|w| w.status == Mark::Done && best(w).is_some_and(|r| r >= 4)).collect();
        high.sort_by(|x, y| best(y).cmp(&best(x)).then_with(|| y.year.cmp(&x.year)));
        let count = |f: &dyn Fn(&Work) -> Option<String>| {
            let mut m: BTreeMap<String, usize> = BTreeMap::new();
            for w in &high {
                if let Some(k) = f(w).filter(|k| !k.is_empty() && k != "未分类") {
                    *m.entry(k).or_default() += 1;
                }
            }
            let mut v: Vec<(String, usize)> = m.into_iter().collect();
            v.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
            v.iter().take(6).map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join("、")
        };
        let done = works.iter().filter(|w| w.status == Mark::Done).count();
        let head = vec![format!(
            "{verb}完的 {done} {unit}里，打了 4 星以上的 {} {unit}（含重{verb}的星级）。高分里的类型：{}；国家：{}。单子里一共 {} {unit}，下面「已经在单子里的」推荐时避开。",
            high.len(),
            if count(&|w| Some(w.kind.clone())).is_empty() { "—".into() } else { count(&|w| Some(w.kind.clone())) },
            if count(&|w| w.country.clone()).is_empty() { "—".into() } else { count(&|w| w.country.clone()) },
            works.len()
        )];
        let titles: Vec<String> = works.iter().map(|w| format!("《{}》", w.title)).collect();
        let listed = titles.chunks(20).map(|c| row(format!("- {}", c.join("、")))).collect();
        return Ok(Found::new(format!("口味（4 星以上 {} {unit}）", high.len()), head)
            .section("4 星以上的（星级高的在前）", high.iter().map(|w| Row { text: work_line(w, verb, by_label), rec: Some(rec(module, kind, w.id, &w.title)) }).collect())
            .section(&format!("已经在单子里的（全部 {} {unit}）", works.len()), listed));
    }
    if view == "stats" {
        let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_year: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_rating: BTreeMap<i64, usize> = BTreeMap::new();
        for w in &works {
            *by_status.entry(status_text(w.status, verb)).or_default() += 1;
            if w.status == Mark::Done {
                for y in w.year.iter().chain(w.rereads.iter().filter_map(|r| r.year.as_ref())) {
                    *by_year.entry(y.clone()).or_default() += 1;
                }
            }
            if let Some(r) = w.rating {
                *by_rating.entry(r).or_default() += 1;
            }
        }
        let rated: Vec<i64> = works.iter().filter_map(|w| w.rating).collect();
        let avg = if rated.is_empty() { "—".into() } else { format!("{:.1}", rated.iter().sum::<i64>() as f64 / rated.len() as f64) };
        let head = vec![format!("一共 {} {unit}；打过星的 {} {unit}，平均 {avg} 星。", works.len(), rated.len())];
        return Ok(Found::new(format!("统计（{} {unit}）", works.len()), head)
            .section("按状态", by_status.iter().map(|(k, v)| row(format!("- {k}：{v} {unit}"))).collect())
            .section(&format!("按{verb}完的年份（含重{verb}，一次算一次）"), by_year.iter().rev().map(|(k, v)| row(format!("- {k}：{v} {unit}"))).collect())
            .section("按星级", by_rating.iter().rev().map(|(k, v)| row(format!("- ★{k}：{v} {unit}"))).collect()));
    }
    let status = a.str("status").map(|s| parse_mark(&s).ok_or_else(|| format!("status 不对：{s}"))).transpose()?;
    let min = a.int("min_rating");
    let year = a.str("year");
    let kw = a.str("keyword").map(|k| k.to_lowercase());
    let mut list: Vec<&Work> = works
        .iter()
        .filter(|w| status.is_none_or(|s| w.status == s))
        .filter(|w| min.is_none_or(|m| w.rating.is_some_and(|r| r >= m) || w.rereads.iter().any(|x| x.rating.is_some_and(|r| r >= m))))
        .filter(|w| year.as_ref().is_none_or(|y| w.year.as_ref() == Some(y) || w.rereads.iter().any(|r| r.year.as_ref() == Some(y))))
        .filter(|w| {
            kw.as_ref().is_none_or(|k| {
                [Some(&w.title), w.by.as_ref(), w.country.as_ref(), Some(&w.kind), w.note.as_ref()].into_iter().flatten().any(|x| x.to_lowercase().contains(k))
            })
        })
        .collect();
    // highest rated, then the latest finished first
    list.sort_by(|x, y| y.rating.cmp(&x.rating).then_with(|| y.year.cmp(&x.year)).then(y.id.cmp(&x.id)));
    let rows = list.iter().map(|w| Row { text: work_line(w, verb, by_label), rec: Some(rec(module, kind, w.id, &w.title)) }).collect();
    let mut filters = vec![];
    if let Some(s) = status {
        filters.push(status_text(s, verb));
    }
    if let Some(m) = min {
        filters.push(format!("{m} 星以上"));
    }
    if let Some(y) = &year {
        filters.push(format!("{y} 年{verb}完"));
    }
    if let Some(k) = a.str("keyword") {
        filters.push(format!("含「{k}」"));
    }
    let f = filters.join("，");
    Ok(Found::new(format!("{}（{} {unit}）", if f.is_empty() { "全部".to_string() } else { f.clone() }, list.len()), vec![format!("{} {} {unit}，星级高的在前。", if f.is_empty() { "全部".to_string() } else { f }, list.len())]).rows(rows))
}

fn books_view(conn: &Connection, a: &Args) -> Result<Found, String> {
    let names = kinds::name_map(conn, "books")?;
    let works = books::list(conn)?
        .into_iter()
        .map(|b| Work { id: b.id, kind: b.kind_id.and_then(|k| names.get(&k).cloned()).unwrap_or_else(|| "未分类".into()), title: b.title, by: b.author, country: b.country, status: b.status, rating: b.rating, year: b.finished_year, note: b.note, rereads: b.rereads })
        .collect();
    works_view(a, works, "books", "book", "读", "本", "作者")
}

fn films_view(conn: &Connection, a: &Args) -> Result<Found, String> {
    let names = kinds::name_map(conn, "films")?;
    let works = films::list(conn)?
        .into_iter()
        .map(|f| Work { id: f.id, kind: f.kind_id.and_then(|k| names.get(&k).cloned()).unwrap_or_else(|| "未分类".into()), title: f.title, by: f.director, country: f.country, status: f.status, rating: f.rating, year: f.finished_year, note: f.note, rereads: f.rereads })
        .collect();
    works_view(a, works, "films", "film", "看", "部", "导演")
}

// ---------- 随意画画 ----------

fn pace_text(p: &drawing::Pace) -> String {
    pace_words(p, "节", "画完")
}

/// `unit`: what a lesson is called; `verb`: 画完 / 完成.
fn pace_words(p: &drawing::Pace, unit: &str, verb: &str) -> String {
    match (p.months_left, &p.finish) {
        (Some(0), _) => format!("全部{verb}了"),
        (Some(m), Some(f)) => format!("还剩 {} {unit}，最近 {} 个月{verb} {} {unit}（每月约 {:.1} {unit}），照这个速度还要约 {m} 个月，{}前后{verb}", p.remaining, drawing::PACE_MONTHS, p.recent, p.rate, month_text(f)),
        _ => format!("还剩 {} {unit}，最近 {} 个月没有{verb}的，没法估计什么时候{verb}", p.remaining, drawing::PACE_MONTHS),
    }
}

fn lesson_status(s: Mark) -> &'static str {
    match s {
        Mark::Done => "已完成",
        Mark::Doing => "进行中",
        _ => "未开始",
    }
}

fn lesson_title(l: &drawing::Lesson) -> String {
    match &l.code {
        Some(c) => format!("{c} {}", l.title),
        None => l.title.clone(),
    }
}

fn drawing_view(conn: &Connection, a: &Args, today: &str) -> Result<Found, String> {
    let courses = drawing::courses(conn)?;
    match a.view(&["courses", "lessons"])? {
        "courses" => {
            let mut rows = vec![];
            let mut everything = vec![];
            for c in &courses {
                let lessons = drawing::lessons(conn, c.id)?;
                let next = lessons.iter().find(|l| l.status == Mark::Doing).or_else(|| lessons.iter().find(|l| l.status != Mark::Done));
                let bits = vec![
                    format!("完成 {} / {} 节", c.done, c.total),
                    if c.doing > 0 { format!("进行中 {} 节", c.doing) } else { String::new() },
                    next.map(|l| format!("下一节：{}", lesson_title(l))).unwrap_or_default(),
                    if c.total > 0 { pace_text(&drawing::pace(&lessons, today)) } else { "还没有课节".into() },
                ];
                rows.push(Row { text: format!("- {}（{}）", c.name, join_bits(bits)), rec: Some(rec("drawing", "course", c.id, &c.name)) });
                everything.extend(lessons);
            }
            let (done, total): (i64, i64) = courses.iter().fold((0, 0), |(d, t), c| (d + c.done, t + c.total));
            let head = vec![
                format!("今天 {today}。速度 = 最近 {} 个月（含本月）画完的节数 ÷ {}；预计 = 还剩的节数 ÷ 速度，向上取整。", drawing::PACE_MONTHS, drawing::PACE_MONTHS),
                format!("全部 {} 门课一共 {total} 节，完成 {done} 节，还剩 {} 节；一起算：{}。", courses.len(), total - done, pace_text(&drawing::pace(&everything, today))),
            ];
            Ok(Found::new(format!("课程进度（{} 门，还剩 {} 节）", courses.len(), total - done), head).rows(rows))
        }
        _ => {
            let want = a.str("course").map(|s| s.to_lowercase());
            let c = courses
                .iter()
                .find(|c| want.as_ref().is_none_or(|w| c.name.to_lowercase().contains(w)))
                .ok_or_else(|| format!("没有课程名含「{}」的课", a.str("course").unwrap_or_default()))?;
            let status = a.str("status").map(|s| parse_mark(&s).ok_or_else(|| format!("status 不对：{s}"))).transpose()?;
            let names = kinds::name_map(conn, "drawing")?;
            let lessons: Vec<drawing::Lesson> = drawing::lessons(conn, c.id)?.into_iter().filter(|l| status.is_none_or(|s| l.status == s)).collect();
            let rows = lessons
                .iter()
                .map(|l| {
                    let bits = vec![l.kind_id.and_then(|k| names.get(&k).cloned()).unwrap_or_default(), lesson_status(l.status).into(), l.finished_month.clone().map(|m| format!("{m} 完成")).unwrap_or_default()];
                    Row {
                        text: format!("- {}（{}）{}", lesson_title(l), join_bits(bits), l.note.as_deref().map(|n| format!(" 备注：{}", clip(n, 60))).unwrap_or_default()),
                        rec: Some(Rec { parent: Some(c.id), ..rec("drawing", "lesson", l.id, &lesson_title(l)) }),
                    }
                })
                .collect();
            Ok(Found::new(format!("{}的课节（{} 节）", c.name, lessons.len()), vec![format!("{}：完成 {} / {} 节，按课程里的顺序。", c.name, c.done, c.total)]).rows(rows))
        }
    }
}

// ---------- 出门走走 ----------

fn items_by_kind(items: &[packing::Item], names: &HashMap<i64, String>) -> Vec<Row> {
    let mut groups: Vec<(String, Vec<String>)> = vec![];
    for i in items {
        let k = i.kind_id.and_then(|k| names.get(&k).cloned()).unwrap_or_else(|| "其他".into());
        let text = format!("{}{}{}", i.name, i.note.as_deref().map(|n| format!("（{}）", clip(n, 30))).unwrap_or_default(), if i.packed == Some(true) { " ✓" } else { "" });
        match groups.iter_mut().find(|(g, _)| *g == k) {
            Some((_, v)) => v.push(text),
            None => groups.push((k, vec![text])),
        }
    }
    groups.into_iter().map(|(k, v)| row(format!("- {k}：{}", v.join("、")))).collect()
}

fn packing_view(conn: &Connection, a: &Args, today: &str) -> Result<Found, String> {
    let names = kinds::name_map(conn, "packing")?;
    let trips = packing::trips(conn)?;
    match a.view(&["trips", "trip", "templates"])? {
        "trips" => {
            let rows = trips
                .iter()
                .map(|t| {
                    let bits = vec![t.depart_on.clone().map(|d| format!("出发 {d}")).unwrap_or_else(|| "没定出发日期".into()), format!("{} 件，已装好 {} 件", t.total, t.packed), t.template_name.clone().map(|n| format!("模板 {n}")).unwrap_or_default()];
                    Row { text: format!("- {}（{}）", t.name, join_bits(bits)), rec: Some(rec("packing", "trip", t.id, &t.name)) }
                })
                .collect();
            Ok(Found::new(format!("出行（{} 次）", trips.len()), vec![format!("今天 {today}。")]).rows(rows))
        }
        "trip" => {
            let t = match a.str("name").map(|s| s.to_lowercase()) {
                Some(w) => trips.iter().find(|t| t.name.to_lowercase().contains(&w)).ok_or_else(|| format!("没有名字含「{}」的出行", a.str("name").unwrap_or_default()))?,
                None => trips
                    .iter()
                    .filter(|t| t.depart_on.as_deref().is_some_and(|d| d >= today))
                    .min_by_key(|t| t.depart_on.clone())
                    .or_else(|| trips.iter().max_by_key(|t| (t.depart_on.clone(), t.id)))
                    .ok_or("还没有出行")?,
            };
            let items = packing::items(conn, packing::List::Trip, t.id)?;
            let when = match t.depart_on.as_deref() {
                Some(d) if d >= today => format!("出发 {d}（还有 {} 天）", common::day_number(d) - common::day_number(today)),
                Some(d) => format!("{d} 出发的（已经过去了）"),
                None => "没定出发日期".into(),
            };
            let head = vec![format!(
                "{}：{when}，{} 件，已装好 {} 件（✓ 是装好的）{}{}。",
                t.name,
                t.total,
                t.packed,
                t.template_name.as_deref().map(|n| format!("；用的模板：{n}")).unwrap_or_default(),
                t.note.as_deref().map(|n| format!("；备注：{}", clip(n, 80))).unwrap_or_default()
            )];
            let mut rows = items_by_kind(&items, &names);
            if let Some(first) = rows.first_mut() {
                first.rec = Some(rec("packing", "trip", t.id, &t.name));
            }
            let missing = packing::maybe_missing(conn, t.id)?
                .into_iter()
                .map(|(name, kind, n, own)| {
                    let k = kind.and_then(|k| names.get(&k).cloned()).unwrap_or_else(|| "其他".into());
                    row(format!("- {name}（{k}，{n} 个清单里有{}）", if own { "，这次用的模板里有" } else { "" }))
                })
                .collect();
            Ok(Found::new(format!("{}的清单（{} 件）", t.name, t.total), head)
                .section("这次的清单", rows)
                .section("模板或以前的出行里有、这次清单里没有的（这次的模板里有的在前，再按出现次数）", missing))
        }
        _ => {
            let templates = packing::templates(conn)?;
            let mut rows = vec![];
            for tp in &templates {
                let items = packing::items(conn, packing::List::Template, tp.id)?;
                let inner = items_by_kind(&items, &names).into_iter().map(|r| r.text.replacen("- ", "", 1)).collect::<Vec<_>>().join("；");
                rows.push(Row { text: format!("- {}（{} 件）：{inner}", tp.name, tp.item_count), rec: Some(rec("packing", "template", tp.id, &tp.name)) });
            }
            Ok(Found::new(format!("场景模板（{} 个）", templates.len()), vec!["新出行的清单可以从模板复制。".into()]).rows(rows))
        }
    }
}

// ---------- 念念不忘 ----------

fn note_rows(conn: &Connection, list: &[notes::Note]) -> Vec<Row> {
    list.iter()
        .map(|n| {
            let body = n.content.trim().replace('\n', "\n  ");
            let body = if body.chars().count() > 600 { format!("{}…（后面略）", body.chars().take(600).collect::<String>()) } else { body };
            Row { text: format!("- {}：{body}", local_minute(conn, &n.created_at)), rec: Some(rec("notes", "note", n.id, &notes::excerpt(&n.content))) }
        })
        .collect()
}

fn notes_view(conn: &Connection, a: &Args, today: &str) -> Result<Found, String> {
    let tag = a.str("tag").map(|t| t.trim_start_matches('#').to_string());
    let tag_text = tag.as_ref().map(|t| format!("#{t} ")).unwrap_or_default();
    match a.view(&["search", "recent", "tags"])? {
        "search" => {
            let q = a.str("keyword").ok_or("search 要填 keyword")?;
            let words: Vec<String> = q.split_whitespace().take(8).map(str::to_lowercase).collect();
            let dated = a.str("from").is_some() || a.str("to").is_some();
            let (from, to) = a.range("1900-01-01".into(), today.to_string())?;
            let f = notes::Filter { tag: tag.clone(), from: dated.then(|| from.clone()), to: dated.then(|| to.clone()), ..Default::default() };
            // any word counts; more words found first, then the newest
            let mut hits: Vec<(usize, notes::Note)> = notes::list(conn, &f)?
                .into_iter()
                .map(|n| {
                    let text = n.content.to_lowercase();
                    (words.iter().filter(|w| text.contains(w.as_str())).count(), n)
                })
                .filter(|(k, _)| *k > 0)
                .collect();
            hits.sort_by(|x, y| y.0.cmp(&x.0).then_with(|| y.1.created_at.cmp(&x.1.created_at)));
            let list: Vec<notes::Note> = hits.into_iter().map(|(_, n)| n).collect();
            let when = if dated { format!("{}，", span(&from, &to)) } else { String::new() };
            let mut head = vec![format!("{when}{tag_text}含「{}」里任意一个词的笔记 {} 条，含的词多的在前，同样多的新的在前。", words.join(" "), list.len())];
            if list.is_empty() {
                head.push("没找到：可以换几个相关的词、或者去掉时间和标签再查。".into());
            }
            Ok(Found::new(format!("「{q}」{tag_text}相关的笔记（{} 条）", list.len()), head).rows(note_rows(conn, &list)))
        }
        "recent" => {
            let (mut from, mut to) = a.range(common::add_days(today, -29), today.to_string())?;
            let mut list = notes::list(conn, &notes::Filter { tag: tag.clone(), from: Some(from.clone()), to: Some(to.clone()), ..Default::default() })?;
            let mut moved = None;
            // nothing in the default 30 days: the latest 30 days that have notes
            if list.is_empty() && a.str("from").is_none() && a.str("to").is_none() {
                if let Some(latest) = notes::list(conn, &notes::Filter { tag: tag.clone(), ..Default::default() })?.iter().map(|n| local_day(conn, &n.created_at)).max() {
                    moved = Some(format!("最近 30 天（{from} 至 {to}）没有笔记，下面是最近有笔记的 30 天。"));
                    (from, to) = (common::add_days(&latest, -29), latest);
                    list = notes::list(conn, &notes::Filter { tag: tag.clone(), from: Some(from.clone()), to: Some(to.clone()), ..Default::default() })?;
                }
            }
            let mut tags: BTreeMap<&str, usize> = BTreeMap::new();
            for n in &list {
                for t in &n.tags {
                    *tags.entry(t.as_str()).or_default() += 1;
                }
            }
            let mut tags: Vec<(&str, usize)> = tags.into_iter().collect();
            tags.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(y.0)));
            let common_tags = tags.iter().take(10).map(|(t, n)| format!("#{t} {n}")).collect::<Vec<_>>().join("、");
            let mut head: Vec<String> = moved.into_iter().collect();
            head.push(format!(
                "{}（{from} 至 {to}）{tag_text}记了 {} 条，新的在前。这段时间用得最多的标签：{}。",
                span(&from, &to),
                list.len(),
                if common_tags.is_empty() { "没有".into() } else { common_tags }
            ));
            if list.is_empty() {
                let latest = notes::overview(conn)?.days.last().map(|d| d.day.clone());
                head.push(match latest {
                    Some(d) => format!("这段时间没有笔记；最近一条记于 {d}，可以把时间段往前放再查。"),
                    None => "还没有笔记。".into(),
                });
            }
            Ok(Found::new(format!("{}{tag_text}的笔记（{} 条）", span(&from, &to), list.len()), head).rows(note_rows(conn, &list)))
        }
        _ => {
            let o = notes::overview(conn)?;
            let mut tags: Vec<&notes::TagCount> = o.tags.iter().collect();
            tags.sort_by(|x, y| y.count.cmp(&x.count).then(x.tag.cmp(&y.tag)));
            let rows = tags.iter().map(|t| row(format!("- #{}：{} 条", t.tag, t.count))).collect();
            Ok(Found::new(format!("标签（{} 个）", tags.len()), vec![format!("一共 {} 条笔记，记了 {} 天；标签的数字含下一级，多的在前。", o.total, o.days.len())]).rows(rows))
        }
    }
}

// ---------- 来日方长 ----------

fn day_when(d: &days::Day) -> String {
    match d.days {
        0 => "就是今天".into(),
        n if n > 0 => format!("还有 {n} 天"),
        n if d.kind == days::Kind::Document => format!("已过期 {} 天", -n),
        n => format!("已经 {} 天", -n),
    }
}

fn remind_text(r: &[i64]) -> String {
    if r.is_empty() {
        return String::new();
    }
    let leads: Vec<String> = r.iter().map(|n| if *n == 0 { "当天".into() } else { format!("提前 {n} 天") }).collect();
    format!("{}提醒", leads.join("、"))
}

/// One day, on one line. 证件: name and expiry only (its note never goes to 小拾).
fn day_bits(d: &days::Day) -> Vec<String> {
    let date = match (d.kind, d.date.strip_prefix("--")) {
        (_, Some(m)) => format!("每年 {}（不知道哪年）", md(&format!("0000-{m}"))),
        (days::Kind::Document, None) => format!("{} 到期", d.date),
        (days::Kind::Birthday, None) => format!("{} 出生", d.date),
        _ => d.date.clone(),
    };
    let repeat = match (d.kind, d.repeat) {
        (days::Kind::Day, days::Repeat::Year) => "每年",
        (days::Kind::Day, days::Repeat::Month) => "每月",
        (days::Kind::Day, days::Repeat::Week) => "每周",
        _ => "",
    };
    let next = if d.next != d.date { format!("下一次 {}", d.next) } else { String::new() };
    let nth = match (d.kind, d.nth) {
        (days::Kind::Birthday, Some(n)) => format!("满 {n} 岁"),
        (_, Some(n)) => format!("第 {n} 周年"),
        _ => String::new(),
    };
    let mut bits = vec![d.kind.label().to_string(), date, repeat.into(), next, day_when(d), nth, remind_text(&d.remind), if d.pinned { "置顶".into() } else { String::new() }];
    if d.kind != days::Kind::Document {
        bits.push(d.note.as_deref().map(|n| format!("备注：{}", clip(n, 200))).unwrap_or_default());
    }
    if d.kind == days::Kind::Birthday {
        bits.push(d.ideas.as_deref().map(|n| format!("礼物点子：{}", clip(n, 200))).unwrap_or_default());
        bits.push(if d.gifts.is_empty() { String::new() } else { format!("送过：{}", d.gifts.iter().take(10).map(|g| format!("{} {}", g.year, clip(&g.gift, 30))).collect::<Vec<_>>().join("、")) });
    }
    bits
}

fn day_row(d: &days::Day) -> Row {
    Row { text: format!("- {}：{}", d.name, join_bits(day_bits(d))), rec: Some(rec("days", "day", d.id, &d.name)) }
}

fn days_view(conn: &Connection, a: &Args, today: &str) -> Result<Found, String> {
    let all = days::list(conn, today)?;
    match a.view(&["soon", "list"])? {
        "soon" => {
            let until = 60;
            let (now, later): (Vec<&days::Day>, Vec<&days::Day>) = all.iter().filter(|d| d.soon || (0..=until).contains(&d.days)).partition(|d| d.soon);
            let head = vec![format!("今天 {today}。「快到的」是已经到了提前提醒天数的（首页也在提醒）和过期的证件。")];
            Ok(Found::new(format!("快到的日子（{} 个）", now.len()), head)
                .section("快到的", now.into_iter().map(day_row).collect())
                .section(&format!("接下来 {until} 天里的其他日子"), later.into_iter().map(day_row).collect()))
        }
        _ => {
            let kind = a.str("kind").map(|k| days::Kind::parse(&k).ok_or("kind 只能是 day / birthday / document")).transpose()?;
            let word = a.str("keyword").map(|w| w.to_lowercase());
            let hit = |d: &days::Day| {
                let Some(w) = &word else { return true };
                let mut hay = vec![d.name.clone()];
                if d.kind != days::Kind::Document {
                    hay.extend(d.note.clone());
                    hay.extend(d.ideas.clone());
                    hay.extend(d.gifts.iter().map(|g| g.gift.clone()));
                }
                hay.join("\n").to_lowercase().contains(w.as_str())
            };
            let list: Vec<&days::Day> = all.iter().filter(|d| kind.is_none_or(|k| d.kind == k) && hit(d)).collect();
            let what = kind.map_or("日子", |k| k.label());
            let head = vec![format!("今天 {today}。{what} {} 个：过期的证件在前，再按还有几天，已经过去的放最后。", list.len())];
            Ok(Found::new(format!("{what}（{} 个）", list.len()), head).rows(list.into_iter().map(day_row).collect()))
        }
    }
}

// ---------- 五花八门 · 循序渐进 ----------

/// One tool over every made module of `t`: hidden ones aren't there, ones this cloud model may not
/// get are named (the answer starts with the sentence).
fn made_run(conn: &Connection, access: &Access, t: Template, arguments: &str, today: &str, budget: i64) -> Answer {
    let label = t.label();
    let mine = access.made_offered(t);
    let refused: Vec<&str> = mine.iter().filter(|m| access.check_made(&m.key).is_err()).map(|m| m.name.as_str()).collect();
    let readable: Vec<&Made> = mine.iter().copied().filter(|m| access.check_made(&m.key).is_ok()).collect();
    let answer = |state: &'static str, l: String, text: String| Answer { module: Some(t.as_str().into()), label: l, state, text, records: vec![] };
    if mine.is_empty() {
        return answer("blocked", format!("{label} · 没有能读的"), format!("没有查：还没有用{label}建的模块，或者都在侧栏里隐藏了。"));
    }
    let sentence = cloud_sentence(&refused);
    if readable.is_empty() {
        return answer("blocked", format!("{label} · 不发给云端，没看"), format!("没有查：这些数据不发给云端模型。请在回答开头写明「{sentence}」，并提醒可以换本地模型再问。"));
    }
    let found = Args::parse(arguments).and_then(|a| match t {
        Template::List => lists_view(conn, &a, &readable),
        Template::Progress => progress_view(conn, &a, &readable, today),
    });
    match found {
        Ok((mut f, key)) => {
            if !refused.is_empty() {
                f.head.insert(0, format!("{}的数据不发给云端模型，没有查。请在回答开头写明「{sentence}」。", refused.join("、")));
            }
            let l = format!("{label} · {}", f.label);
            let (text, records) = f.render(budget.min(MAX_RESULT));
            Answer { module: Some(key.unwrap_or_else(|| t.as_str().into())), label: l, state: "ok", text, records }
        }
        Err(e) => answer("failed", format!("{label} · 没查成"), format!("没查成：{e}")),
    }
}

/// The made module whose name contains `word` (the only one when there's no word).
fn pick_made<'a>(readable: &[&'a Made], word: Option<String>, what: &str) -> Result<&'a Made, String> {
    let names = readable.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join("、");
    match word.map(|w| w.to_lowercase()) {
        Some(w) => readable.iter().find(|m| m.name.to_lowercase().contains(&w) || w.contains(&m.name.to_lowercase())).copied().ok_or_else(|| format!("没有名字含「{w}」的{what}（有：{names}）")),
        None if readable.len() == 1 => Ok(readable[0]),
        None => Err(format!("要说是哪个{what}（有：{names}）")),
    }
}

fn field_text(f: &lists::Field) -> String {
    match f.kind {
        lists::FieldType::Select | lists::FieldType::Multi if !f.options.is_empty() => format!("{}（{}：{}）", f.name, f.kind.label(), f.options.join("、")),
        _ => format!("{}（{}）", f.name, f.kind.label()),
    }
}

fn item_line(it: &lists::Item, fs: &[lists::Field]) -> String {
    let bits: Vec<String> = fs
        .iter()
        .filter_map(|f| it.values.get(&f.id.to_string()).map(|v| {
            let text = lists::value_text(f, v);
            format!("{} {}", f.name, if f.kind == lists::FieldType::LongText { clip(&text, 300) } else { text })
        }))
        .collect();
    if bits.is_empty() { format!("- {}", it.name) } else { format!("- {}：{}", it.name, bits.join(" · ")) }
}

fn lists_view(conn: &Connection, a: &Args, readable: &[&Made]) -> Result<(Found, Option<String>), String> {
    match a.view(&["lists", "items"])? {
        "lists" => {
            let mut rows = vec![];
            for m in readable {
                let id = custom::parse_key(&m.key).map_or(0, |k| k.1);
                let fs = lists::fields(conn, id)?;
                let n = lists::items(conn, id)?.len();
                let fields = if fs.is_empty() { "只有名称".to_string() } else { format!("名称、{}", fs.iter().map(field_text).collect::<Vec<_>>().join("、")) };
                rows.push(row(format!("- {}（{n} 条）：字段 {fields}", m.name)));
            }
            Ok((Found::new(format!("清单（{} 个）", readable.len()), vec!["要看某个清单的条目用 view items。".into()]).rows(rows), None))
        }
        _ => {
            let m = pick_made(readable, a.str("list"), "清单")?;
            let id = custom::parse_key(&m.key).map_or(0, |k| k.1);
            let fs = lists::fields(conn, id)?;
            let word = a.str("keyword").map(|w| w.to_lowercase());
            let all = lists::items(conn, id)?;
            let hit = |it: &lists::Item| {
                word.as_ref().is_none_or(|w| {
                    let mut hay = it.name.to_lowercase();
                    for f in &fs {
                        if let Some(v) = it.values.get(&f.id.to_string()) {
                            hay.push('\n');
                            hay.push_str(&lists::value_text(f, v).to_lowercase());
                        }
                    }
                    hay.contains(w.as_str())
                })
            };
            let list: Vec<&lists::Item> = all.iter().filter(|it| hit(it)).collect();
            let rows = list.iter().map(|it| Row { text: item_line(it, &fs), rec: Some(rec(&m.key, "list_item", it.id, &it.name)) }).collect();
            let head = vec![format!(
                "{}：{} 条{}，按清单里的顺序。字段：{}。没写的字段是没填。",
                m.name,
                list.len(),
                word.as_ref().map(|w| format!("含「{w}」")).unwrap_or_default(),
                if fs.is_empty() { "只有名称".to_string() } else { fs.iter().map(field_text).collect::<Vec<_>>().join("、") }
            )];
            Ok((Found::new(format!("{}（{} 条）", m.name, list.len()), head).rows(rows), Some(m.key.clone())))
        }
    }
}

fn progress_status(s: Mark) -> &'static str {
    match s {
        Mark::Done => "已完成",
        Mark::Doing => "进行中",
        _ => "未开始",
    }
}

fn plesson_title(l: &progress::Lesson) -> String {
    format!("{}{}", l.code.as_deref().map(|c| format!("{c} ")).unwrap_or_default(), l.title)
}

fn progress_view(conn: &Connection, a: &Args, readable: &[&Made], today: &str) -> Result<(Found, Option<String>), String> {
    let words_of = |m: &Made| custom::get(conn, custom::parse_key(&m.key).map_or(0, |k| k.1)).map(|c| (c.course_word, c.lesson_word));
    match a.view(&["overview", "lessons"])? {
        "overview" => {
            let mut sections = vec![];
            for m in readable {
                let (cw, lw) = words_of(m)?;
                let mut rows = vec![];
                for c in progress::courses(conn, custom::parse_key(&m.key).map_or(0, |k| k.1))? {
                    let ls = progress::lessons(conn, c.id)?;
                    let next = progress::next(&ls).map(|l| format!("；下一{lw} {}", plesson_title(l))).unwrap_or_default();
                    let doing = if c.doing > 0 { format!("，进行中 {}", c.doing) } else { String::new() };
                    let pace = pace_words(&progress::pace(&ls, today), &lw, "完成");
                    rows.push(Row { text: format!("- {}：完成 {} / {} {lw}{doing}{next}；{pace}", c.name, c.done, c.total), rec: Some(rec(&m.key, "progress_course", c.id, &c.name)) });
                }
                sections.push((format!("{}（{cw} → {lw}）", m.name), rows));
            }
            let mut f = Found::new(format!("进度（{} 个）", readable.len()), vec![format!("今天 {today}。")]);
            for (title, rows) in sections {
                f = f.section(&title, rows);
            }
            Ok((f, None))
        }
        _ => {
            let w = a.str("name").map(|w| w.to_lowercase()).ok_or("lessons 要填 name")?;
            let status = a.str("status").map(|s| Mark::parse(&s).filter(|s| *s != Mark::Unmarked).ok_or("status 只能是 todo / doing / done")).transpose()?;
            let mut all = vec![];
            for m in readable {
                for c in progress::courses(conn, custom::parse_key(&m.key).map_or(0, |k| k.1))? {
                    all.push((*m, c));
                }
            }
            let (m, c) = all
                .iter()
                .find(|(_, c)| c.name.to_lowercase().contains(&w))
                .or_else(|| all.iter().find(|(m, c)| format!("{} {}", m.name, c.name).to_lowercase().contains(&w) || w.contains(&c.name.to_lowercase())))
                .ok_or_else(|| format!("没有名字含「{w}」的一项（有：{}）", all.iter().map(|(m, c)| format!("{} · {}", m.name, c.name)).collect::<Vec<_>>().join("、")))?;
            let (_, lw) = words_of(m)?;
            let ls = progress::lessons(conn, c.id)?;
            let kind_names = kinds::name_map(conn, &m.key)?;
            let shown: Vec<&progress::Lesson> = ls.iter().filter(|l| status.is_none_or(|s| l.status == s)).collect();
            let rows = shown
                .iter()
                .map(|l| {
                    let done = l.finished_month.as_deref().filter(|_| l.status == Mark::Done).map(|mo| format!("（{mo}）")).unwrap_or_default();
                    let note = l.note.as_deref().map(|n| format!(" · 备注：{}", clip(n, 80))).unwrap_or_default();
                    let kind = l.kind_id.and_then(|k| kind_names.get(&k)).map(|k| format!("（{k}）")).unwrap_or_default();
                    Row { text: format!("- {}{kind}：{}{done}{note}", plesson_title(l), progress_status(l.status)), rec: Some(Rec { parent: Some(c.id), ..rec(&m.key, "progress_lesson", l.id, &plesson_title(l)) }) }
                })
                .collect();
            let head = vec![format!("{} · {}：完成 {} / {} {lw}，按顺序列出{}。", m.name, c.name, c.done, c.total, status.map(|s| format!("{}的", progress_status(s))).unwrap_or_default())];
            Ok((Found::new(format!("{} · {}（{} {lw}）", m.name, c.name, shown.len()), head).rows(rows), Some(m.key.clone())))
        }
    }
}

// ---------- mock model (dev builds, tests, mockIpc.ts mirrors it) ----------

/// Which lookups the mock model makes for a question (only among `offered` tools).
pub fn mock_calls(question: &str, system: &str, offered: &[&str]) -> Vec<ToolCall> {
    let today = system.split("今天是 ").nth(1).map(|s| s.chars().take(10).collect::<String>()).filter(|d| common::check_date(&Some(d.clone()), "").is_ok());
    let Some(today) = today else { return vec![] };
    let has = |words: &[&str]| words.iter().any(|w| question.contains(w));
    // a skill the question asks for, or 「查一下《X》」 (skills/)
    let skilled: Vec<(&str, Value)> = super::skills::mock_calls(question, system).into_iter().filter(|(n, _)| offered.contains(n)).collect();
    if !skilled.is_empty() {
        return skilled.into_iter().enumerate().map(|(i, (name, args))| ToolCall { id: format!("mock{i}"), name: name.into(), arguments: args.to_string() }).collect();
    }
    let mut want: Vec<(&str, Value)> = super::act::mock_calls(question, system, &today);
    if !want.is_empty() {
        // something to write: only that (nothing when its module isn't offered)
        return want
            .into_iter()
            .filter(|(n, _)| offered.contains(n))
            .enumerate()
            .map(|(i, (name, args))| ToolCall { id: format!("mock{i}"), name: name.into(), arguments: args.to_string() })
            .collect();
    }
    if has(&["跟进"]) {
        want.push(("pomelo_jobs", json!({ "view": "follow_up" })));
    } else if has(&["进展"]) {
        want.push(("pomelo_jobs", json!({ "view": "moves" })));
    } else if has(&["漏斗", "投递"]) {
        want.push(("pomelo_jobs", json!({ "view": "stats" })));
    }
    if has(&["资产"]) {
        want.push(("pomelo_finance", json!({ "view": "assets", "from": common::add_days(&today, -182) })));
    } else if has(&["花", "支出", "钱"]) {
        want.push(("pomelo_finance", json!({ "view": "entries", "flow": "out" })));
    }
    if has(&["拖", "没动"]) {
        want.push(("pomelo_todo", json!({ "view": "stuck" })));
    } else if has(&["待办", "做完", "任务"]) {
        want.push(("pomelo_todo", json!({ "view": if question.contains("今天") { "today" } else { "done" } })));
    }
    let works = if has(&["推荐"]) { json!({ "view": "taste" }) } else { json!({ "view": "list", "status": "done" }) };
    if has(&["书"]) {
        want.push(("pomelo_books", works.clone()));
    }
    if has(&["电影", "剧", "片"]) {
        want.push(("pomelo_films", works));
    }
    if has(&["画"]) {
        want.push(("pomelo_drawing", json!({ "view": "courses" })));
    }
    if has(&["生日", "礼物"]) {
        want.push(("pomelo_days", json!({ "view": "list", "kind": "birthday" })));
    } else if has(&["纪念日", "证件", "到期", "日子"]) {
        want.push(("pomelo_days", json!({ "view": "soon" })));
    }
    if has(&["出门", "行李", "漏带"]) {
        want.push(("pomelo_packing", json!({ "view": "trip" })));
    }
    if has(&["清单"]) {
        want.push((LISTS_TOOL, json!({ "view": "lists" })));
    }
    if has(&["表格", "哪几行", "第几行"]) {
        want.push((super::office::TABLE_TOOL, json!({ "view": "rows", "from": 1, "to": 5 })));
    }
    if has(&["进度"]) {
        want.push((PROGRESS_TOOL, json!({ "view": "overview" })));
    }
    if has(&["记过", "关于", "提到过"]) {
        // 「我关于租房记过什么」: search across modules for what follows 关于 (else the question)
        let topic = question.split("关于").nth(1).map(|s| s.split(['记', '的', '？', '?']).next().unwrap_or("").trim().to_string()).filter(|s| !s.is_empty());
        want.push((super::cite::SEARCH, json!({ "query": topic.unwrap_or_else(|| question.trim().to_string()) })));
    } else if has(&["笔记"]) {
        want.push(("pomelo_notes", json!({ "view": "recent" })));
    }
    want.into_iter()
        .filter(|(n, _)| offered.contains(n))
        .enumerate()
        .map(|(i, (name, args))| ToolCall { id: format!("mock{i}"), name: name.into(), arguments: args.to_string() })
        .collect()
}

/// The mock model's answer from this turn's results.
pub fn mock_answer(messages: &[Msg]) -> String {
    let results: Vec<&str> = messages.iter().rev().take_while(|m| m.role == "tool").map(|m| m.content.as_str()).collect::<Vec<_>>().into_iter().rev().collect();
    let mut out = String::new();
    let starts: Vec<String> = results.iter().filter_map(|r| r.split("请在回答开头写明「").nth(1).and_then(|s| s.split('」').next()).map(|s| format!("{s}。"))).collect();
    for s in &starts {
        out.push_str(s);
    }
    if !starts.is_empty() {
        out.push_str("\n\n");
    }
    let carded = results.iter().filter(|r| r.starts_with("已放进确认卡片")).count();
    let failed: Vec<&str> = results.iter().filter_map(|r| r.strip_prefix("没放进卡片：")).collect();
    if carded + failed.len() > 0 {
        if carded > 0 {
            out.push_str(&format!("这是开发版的**模拟回答**：{carded} 条已放进下面的确认卡片，看一下没问题就点「确认」。"));
        }
        for f in failed {
            out.push_str(&format!("\n\n有一条没放进去：{f}"));
        }
        return out;
    }
    let read: Vec<&&str> = results.iter().filter(|r| !r.starts_with("没有查") && !r.starts_with("没有提议")).collect();
    if read.is_empty() {
        out.push_str("这是开发版的**模拟回答**。");
        return out;
    }
    // search results: cite the first two like a real answer would
    let numbers: Vec<String> = read.iter().flat_map(|r| r.lines()).filter_map(|l| l.strip_prefix('[').and_then(|x| x.split_once(']')).map(|(n, _)| n.to_string())).filter(|n| n.chars().all(|c| c.is_ascii_digit())).take(2).collect();
    if !numbers.is_empty() {
        out.push_str(&format!("这是开发版的**模拟回答**：搜到的记录里，最相关的是这几条{}。\n", numbers.iter().map(|n| format!("[{n}]")).collect::<String>()));
        return out;
    }
    out.push_str("这是开发版的**模拟回答**，下面是查到的内容的开头：\n");
    for r in read {
        let lines: Vec<&str> = r.lines().take(4).collect();
        out.push_str(&format!("\n> {}\n", lines.join("\n> ")));
    }
    out
}

// ---------- commands ----------

#[tauri::command]
pub fn picky_data_cloud(state: State<AppState>) -> Result<Vec<CloudSetting>, String> {
    let made = state.with_db(|c| Ok(made(c)))?;
    state.picky.with_db(|c| cloud_settings(c, &made))
}

#[tauri::command]
pub fn picky_data_cloud_set(state: State<AppState>, module: String, allowed: bool) -> Result<(), String> {
    let made = state.with_db(|c| Ok(made(c)))?;
    state.picky.with_db(|c| set_cloud(c, &module, allowed, &made))
}

// ---------- the page a question was asked on (v0.3.5 小窗) ----------
// The page says where it is and which records are open / picked (ids only); Rust fetches the content
// under the same rules: 芝麻开门 (and unknown pages) never; a hidden module, or one this cloud model may
// not get, only says where. Stored as a run_queries row (call_id "page", position -1) with the run.

pub const PAGE_CALL: &str = "page";
const MAX_PICKED: usize = 20;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRef {
    pub kind: String,
    pub id: i64,
}

/// What a page tells the 小窗 (ids only).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageContext {
    /// Route id: a module, "home" or "settings".
    pub module: String,
    /// The view inside it (收支, 统计…).
    #[serde(default)]
    pub page: Option<String>,
    /// Filters as shown (2026 年 9 月 · 餐饮).
    #[serde(default)]
    pub filter: Option<String>,
    /// The record whose editor is open.
    #[serde(default)]
    pub record: Option<PageRef>,
    /// Records picked in 多选.
    #[serde(default)]
    pub selected: Vec<PageRef>,
}

fn short(s: &Option<String>, n: usize) -> Option<String> {
    s.as_deref().map(|x| clip(x, n)).filter(|x| !x.is_empty())
}

/// Which module a record kind lives in.
fn kind_module(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "task" | "goal" | "group" => "todo",
        "book" => "books",
        "film" => "films",
        "course" | "lesson" => "drawing",
        "trip" | "template" => "packing",
        "day" => "days",
        "entry" | "account" | "fixed" => "finance",
        "job" => "jobs",
        "note" => "notes",
        _ => return None,
    })
}

pub(crate) fn kind_label(kind: &str) -> &'static str {
    match kind {
        "task" => "任务",
        "goal" => "目标",
        "group" => "分组",
        "book" => "书",
        "film" => "影视",
        "course" => "课程",
        "lesson" => "课节",
        "trip" => "出行",
        "template" => "模板",
        "entry" => "收支",
        "account" => "账户",
        "fixed" => "固定支出",
        "job" => "投递",
        "note" => "笔记",
        "day" => "日子",
        "list_item" => "条目",
        "progress_course" => "一项进度",
        "progress_lesson" => "一节",
        _ => "记录",
    }
}

/// A made module's record in full (五花八门 条目, 循序渐进 一项 / 一节); None for other kinds.
fn made_record(conn: &Connection, today: &str, r: &PageRef) -> Option<Result<(String, String, Rec), String>> {
    let item = || -> Result<(String, String, Rec), String> {
        let it = lists::item(conn, r.id)?;
        let m = custom::get(conn, it.module_id)?;
        let fs = lists::fields(conn, it.module_id)?;
        let mut lines = vec![format!("清单：{}", m.name), format!("名称：{}", it.name)];
        lines.extend(fs.iter().filter_map(|f| it.values.get(&f.id.to_string()).map(|v| format!("{}：{}", f.name, lists::value_text(f, v)))));
        Ok((it.name.clone(), lines.join("\n"), rec(&m.key, "list_item", it.id, &it.name)))
    };
    let course = || -> Result<(String, String, Rec), String> {
        let c = progress::course(conn, r.id)?;
        let m = custom::get(conn, c.module_id)?;
        let ls = progress::lessons(conn, c.id)?;
        let lw = &m.lesson_word;
        let list = ls.iter().map(|l| format!("{}（{}）", plesson_title(l), progress_status(l.status))).collect::<Vec<_>>().join("、");
        let mut lines = vec![format!("{}：{}", m.name, c.name), format!("进度：完成 {} / {} {lw}，{}", c.done, c.total, pace_words(&progress::pace(&ls, today), lw, "完成"))];
        lines.extend(progress::next(&ls).map(|l| format!("下一{lw}：{}", plesson_title(l))));
        lines.extend(c.note.as_ref().map(|n| format!("备注：{n}")));
        lines.extend((!list.is_empty()).then(|| format!("全部：{list}")));
        Ok((c.name.clone(), lines.join("\n"), rec(&m.key, "progress_course", c.id, &c.name)))
    };
    let lesson = || -> Result<(String, String, Rec), String> {
        let l = progress::lesson(conn, r.id)?;
        let c = progress::course(conn, l.course_id)?;
        let m = custom::get(conn, c.module_id)?;
        let title = plesson_title(&l);
        let mut lines = vec![format!("{} · {}：{title}", m.name, c.name), format!("状态：{}", progress_status(l.status))];
        lines.extend(l.finished_month.as_ref().filter(|_| l.status == Mark::Done).map(|mo| format!("完成时间：{mo}")));
        lines.extend(l.note.as_ref().map(|n| format!("备注：{n}")));
        Ok((title.clone(), lines.join("\n"), Rec { parent: Some(c.id), ..rec(&m.key, "progress_lesson", l.id, &title) }))
    };
    match r.kind.as_str() {
        "list_item" => Some(item()),
        "progress_course" => Some(course()),
        "progress_lesson" => Some(lesson()),
        _ => None,
    }
}

/// One record in full: (title, text, record).
pub(crate) fn record_text(conn: &Connection, today: &str, r: &PageRef) -> Result<(String, String, Rec), String> {
    if let Some(x) = made_record(conn, today, r) {
        return x;
    }
    let module = kind_module(&r.kind).ok_or("不认识这种记录")?;
    let line = |k: &str, v: Option<String>| v.filter(|v| !v.trim().is_empty()).map(|v| format!("{k}：{v}"));
    let (title, lines, mut rc): (String, Vec<Option<String>>, Rec) = match r.kind.as_str() {
        "task" => {
            let t = todo::task_view(conn, r.id)?;
            let group = todo::group(conn, t.group_id)?.name;
            let goal = t.goal_id.and_then(|g| todo::goal(conn, g).ok()).map(|g| g.name);
            let parent = t.parent_id.and_then(|p| todo::task(conn, p).ok()).map(|p| plain(&p.title));
            let kind = match t.task_type {
                todo::TaskType::Normal => "普通".to_string(),
                todo::TaskType::Repeat => format!("重复（{}，已做 {} 次）", rule_text(t.repeat_rule.as_deref().unwrap_or("")), t.count),
                todo::TaskType::Checkin => format!("打卡（{}，连续 {} 天）", rule_text(t.repeat_rule.as_deref().unwrap_or("")), t.streak),
                todo::TaskType::Progress => format!("进度条（{} / {}{}）", t.count, t.target.unwrap_or(0), t.unit.as_deref().map(|u| format!(" {u}")).unwrap_or_default()),
            };
            let subs: Vec<String> = todo::tasks(conn, Some(t.group_id), t.goal_id)?.into_iter().filter(|x| x.parent_id == Some(t.id)).map(|x| format!("{}{}", plain(&x.title), if x.done { "（已完成）" } else { "" })).collect();
            let title = plain(&t.title);
            (
                title.clone(),
                vec![
                    line("任务", Some(title.clone())),
                    line("在", Some([Some(group), goal, parent].into_iter().flatten().collect::<Vec<_>>().join(" / "))),
                    line("类型", Some(kind)),
                    line("时间", match (&t.start_date, &t.due_date) {
                        (Some(s), Some(d)) => Some(format!("{s} 至 {d}")),
                        (None, Some(d)) => Some(format!("截止 {d}")),
                        _ => None,
                    }),
                    line("状态", Some(if t.done { format!("已完成{}", t.done_at.as_deref().map(|d| format!("（{}）", local_day(conn, d))).unwrap_or_default()) } else { "没完成".into() })),
                    line("内容", t.note.as_deref().map(plain)),
                    line("链接", t.link.clone()),
                    line("来自笔记", t.from_note.and_then(|n| notes::get(conn, n).ok()).map(|n| notes::excerpt(&n.content))),
                    line("子任务", (!subs.is_empty()).then(|| subs.join("、"))),
                ],
                Rec { parent: Some(t.goal_id.map_or(t.group_id, |g| -g)), day: t.due_date.clone(), ..rec(module, "task", t.id, &title) },
            )
        }
        "goal" => {
            let g = todo::goal(conn, r.id)?;
            let tasks: Vec<String> = todo::tasks(conn, None, Some(g.id))?.into_iter().filter(|x| x.parent_id.is_none()).map(|x| format!("{}{}", plain(&x.title), if x.done { "（已完成）" } else { "" })).collect();
            (
                g.name.clone(),
                vec![
                    line("目标", Some(g.name.clone())),
                    line("分组", todo::group(conn, g.group_id).ok().map(|x| x.name)),
                    line("状态", Some(match g.status { todo::GoalStatus::Active => "进行中", todo::GoalStatus::Done => "已完成", _ => "暂停" }.to_string())),
                    line("时间", match (&g.start_date, &g.end_date) {
                        (Some(s), Some(e)) => Some(format!("{s} 至 {e}")),
                        (None, Some(e)) => Some(format!("到 {e}")),
                        (Some(s), None) => Some(format!("从 {s}")),
                        _ => None,
                    }),
                    line("进度", Some(format!("{} / {}", g.done, g.total))),
                    line("备注", g.note.clone()),
                    line("复盘", g.review.clone()),
                    line("任务", (!tasks.is_empty()).then(|| tasks.join("、"))),
                ],
                Rec { parent: Some(g.group_id), ..rec(module, "goal", g.id, &g.name) },
            )
        }
        "group" => {
            let g = todo::group(conn, r.id)?;
            (g.name.clone(), vec![line("分组", Some(g.name.clone())), line("没完成的任务", Some(format!("{} 件", g.open)))], rec(module, "group", g.id, &g.name))
        }
        "book" | "film" => {
            let (w, kind) = if r.kind == "book" {
                let names = kinds::name_map(conn, "books")?;
                let b = books::get(conn, r.id)?;
                (Work { id: b.id, kind: b.kind_id.and_then(|k| names.get(&k).cloned()).unwrap_or_else(|| "未分类".into()), title: b.title, by: b.author, country: b.country, status: b.status, rating: b.rating, year: b.finished_year, note: b.note, rereads: b.rereads }, "book")
            } else {
                let names = kinds::name_map(conn, "films")?;
                let f = films::get(conn, r.id)?;
                (Work { id: f.id, kind: f.kind_id.and_then(|k| names.get(&k).cloned()).unwrap_or_else(|| "未分类".into()), title: f.title, by: f.director, country: f.country, status: f.status, rating: f.rating, year: f.finished_year, note: f.note, rereads: f.rereads }, "film")
            };
            let (verb, by) = if kind == "book" { ("读", "作者") } else { ("看", "导演") };
            let text = work_line(&w, verb, by).trim_start_matches("- ").to_string();
            (w.title.clone(), vec![Some(text), line("备注（全文）", w.note.clone())], rec(module, kind, w.id, &w.title))
        }
        "course" => {
            let c = drawing::course(conn, r.id)?;
            let lessons = drawing::lessons(conn, c.id)?;
            let next = lessons.iter().find(|l| l.status == Mark::Doing).or_else(|| lessons.iter().find(|l| l.status != Mark::Done));
            (
                c.name.clone(),
                vec![
                    line("课程", Some(c.name.clone())),
                    line("进度", Some(format!("完成 {} / {} 节，{}", c.done, c.total, pace_text(&drawing::pace(&lessons, today))))),
                    line("下一节", next.map(lesson_title)),
                    line("备注", c.note.clone()),
                ],
                rec(module, "course", c.id, &c.name),
            )
        }
        "lesson" => {
            let l = drawing::lesson(conn, r.id)?;
            let c = drawing::course(conn, l.course_id)?;
            let names = kinds::name_map(conn, "drawing")?;
            let title = lesson_title(&l);
            (
                title.clone(),
                vec![
                    line("课节", Some(title.clone())),
                    line("课程", Some(c.name)),
                    line("类型", l.kind_id.and_then(|k| names.get(&k).cloned())),
                    line("状态", Some(lesson_status(l.status).into())),
                    line("完成时间", l.finished_month.clone()),
                    line("备注", l.note.clone()),
                ],
                Rec { parent: Some(l.course_id), ..rec(module, "lesson", l.id, &title) },
            )
        }
        "trip" | "template" => {
            let names = kinds::name_map(conn, "packing")?;
            if r.kind == "trip" {
                let t = packing::trip(conn, r.id)?;
                let items = items_by_kind(&packing::items(conn, packing::List::Trip, t.id)?, &names).into_iter().map(|x| x.text).collect::<Vec<_>>().join("\n");
                (
                    t.name.clone(),
                    vec![line("出行", Some(t.name.clone())), line("出发", t.depart_on.clone()), line("模板", t.template_name.clone()), line("进度", Some(format!("{} 件，已装好 {} 件（✓ 是装好的）", t.total, t.packed))), line("备注", t.note.clone()), line("清单", Some(format!("\n{items}")))],
                    rec(module, "trip", t.id, &t.name),
                )
            } else {
                let t = packing::template(conn, r.id)?;
                let items = items_by_kind(&packing::items(conn, packing::List::Template, t.id)?, &names).into_iter().map(|x| x.text).collect::<Vec<_>>().join("\n");
                (t.name.clone(), vec![line("模板", Some(t.name.clone())), line("备注", t.note.clone()), line("物品", Some(format!("\n{items}")))], rec(module, "template", t.id, &t.name))
            }
        }
        "entry" => {
            let e = finance::entry(conn, r.id)?;
            let kinds = kinds::name_map(conn, if e.flow == Flow::Out { finance::OUT_KINDS } else { finance::IN_KINDS })?;
            let kind = e.kind_id.and_then(|k| kinds.get(&k).cloned()).unwrap_or_else(|| "未分类".into());
            let title = e.note.clone().unwrap_or_else(|| kind.clone());
            (
                title.clone(),
                vec![
                    line(flow_text(e.flow), Some(money(e.amount, e.currency))),
                    line("日期", Some(match &e.end_day { Some(end) => format!("{} 至 {end}（记小额）", e.day), None => e.day.clone() })),
                    line("分类", Some(kind)),
                    line("备注", e.note.clone()),
                    e.fixed_id.map(|_| "由固定支出自动记的".to_string()),
                ],
                Rec { day: Some(e.day.clone()), ..rec(module, "entry", e.id, &title) },
            )
        }
        "account" => {
            let a = finance::account(conn, r.id)?;
            let recent: Vec<String> = finance::assets(conn)?.balances.into_iter().filter(|b| b.account_id == a.id).rev().take(6).map(|b| format!("{} {}", b.day, money(b.amount, a.currency))).collect();
            (
                a.name.clone(),
                vec![
                    line("账户", Some(a.name.clone())),
                    line("类型", Some(format!("{} · {}{}", account_kind(a.kind), currency_name(a.currency), if a.closed { " · 已停用" } else { "" }))),
                    line("最近的余额", (!recent.is_empty()).then(|| recent.join("、"))),
                    line("备注", a.note.clone()),
                ],
                rec(module, "account", a.id, &a.name),
            )
        }
        "fixed" => {
            let f = finance::fixed(conn, r.id)?;
            (f.name.clone(), vec![line("固定支出", Some(f.name.clone())), line(flow_text(f.flow), Some(money(f.amount, f.currency))), line("周期", Some(f.rule.clone())), line("下次", Some(f.next_day.clone())), line("状态", Some(if f.stopped { "已停用" } else { "在用" }.into())), line("备注", f.note.clone())], rec(module, "fixed", f.id, &f.name))
        }
        "job" => {
            let j = jobs::job(conn, r.id, today)?;
            let sts = jobs::statuses(conn)?;
            let name = |id: i64| sts.iter().find(|s| s.id == id).map_or("?".to_string(), |s| s.name.clone());
            let channels = kinds::name_map(conn, jobs::CHANNELS)?;
            let timeline = j.steps.iter().map(|s| format!("{} {}", name(s.status_id), if s.done { s.done_on.clone().unwrap_or_else(|| "✓".into()) } else { "—".into() })).collect::<Vec<_>>().join(" → ");
            let title = job_title(&j);
            (
                title.clone(),
                vec![
                    line("投递", Some(title.clone())),
                    line("地点", j.location.clone()),
                    line("渠道", j.kind_id.and_then(|k| channels.get(&k).cloned())),
                    line("现在", Some(j.status_id.map(name).unwrap_or_else(|| "想投".into()))),
                    line("投递日期", j.applied_on.clone()),
                    line("跟进", j.waiting_days.map(|w| format!("等了 {w} 天{}{}", if j.follow_up { "，该跟进了（满 14 天）" } else { "" }, j.followed_on.as_deref().map(|d| format!("；{d} 跟进过")).unwrap_or_default()))),
                    line("时间线", (!timeline.is_empty()).then_some(timeline)),
                    line("JD", j.link.clone()),
                    line("简历", j.resume.clone()),
                    line("备注", j.note.clone()),
                ],
                rec(module, "job", j.id, &title),
            )
        }
        "note" => {
            let n = notes::get(conn, r.id)?;
            let title = notes::excerpt(&n.content);
            (title.clone(), vec![line("记于", Some(local_minute(conn, &n.created_at))), line("正文", Some(format!("\n{}", n.content.trim())))], rec(module, "note", n.id, &title))
        }
        "day" => {
            let d = days::get(conn, today, r.id)?;
            (d.name.clone(), vec![line("日子", Some(d.name.clone())), Some(join_bits(day_bits(&d)))], rec(module, "day", d.id, &d.name))
        }
        _ => return Err("不认识这种记录".into()),
    };
    rc.title = title.clone();
    Ok((title, lines.into_iter().flatten().collect::<Vec<_>>().join("\n"), rc))
}

/// What a question sends about its page; None = nothing at all (芝麻开门, 小拾, unknown pages).
pub fn page_query(conn: &Connection, access: &Access, today: &str, ctx: &PageContext) -> Option<Query> {
    if let Some(m) = custom::by_key(conn, &ctx.module) {
        return Some(made_page(conn, access, today, ctx, &m));
    }
    let (m, label) = match ctx.module.as_str() {
        "home" => (None, "首页"),
        "settings" => (None, "设置"),
        key => match module(key) {
            Some(m) => (Some(m), m.label),
            None => return None,
        },
    };
    let place = match short(&ctx.page, 20) {
        Some(p) => format!("{label} · {p}"),
        None => label.to_string(),
    };
    let only_where = |why: &str| Query { module: m.map(|m| m.key.to_string()), label: format!("{place}{why}"), state: "ok".into(), result: format!("页面：{place}"), records: vec![] };
    let Some(m) = m else { return Some(only_where("")) };
    match access.check(m) {
        Err(Blocked::Hidden) => return Some(Query { state: "blocked".into(), ..only_where("（隐藏的模块，只带了在哪一页）") }),
        Err(Blocked::Cloud) => return Some(Query { state: "blocked".into(), ..only_where("（不发给云端，只带了在哪一页）") }),
        Ok(()) => {}
    }
    let mut lines = vec![format!("页面：{place}")];
    let filter = short(&ctx.filter, 60);
    if let Some(f) = &filter {
        lines.push(format!("筛选：{f}"));
    }
    let mut records = vec![];
    let mut title_line = None;
    let mine = |r: &&PageRef| kind_module(&r.kind) == Some(m.key);
    if let Some(r) = ctx.record.as_ref().filter(mine) {
        if let Ok((title, text, rc)) = record_text(conn, today, r) {
            title_line = Some(format!("{}：{title}", kind_label(&r.kind)));
            lines.push(format!("打开的{}：\n{text}", kind_label(&r.kind)));
            records.push(rc);
        }
    }
    let picked: Vec<&PageRef> = ctx.selected.iter().filter(mine).take(MAX_PICKED).collect();
    if !picked.is_empty() {
        let parts: Vec<String> = picked
            .iter()
            .filter_map(|r| record_text(conn, today, r).ok())
            .map(|(_, text, rc)| {
                records.push(rc);
                format!("- {}", text.replace('\n', "；"))
            })
            .collect();
        if title_line.is_none() {
            title_line = Some(format!("{label} · 选中的 {} 条", parts.len()));
        }
        lines.push(format!("选中的 {} 条{}：\n{}", parts.len(), kind_label(&picked[0].kind), parts.join("\n")));
    }
    let label = title_line.unwrap_or_else(|| [Some(place.clone()), filter].into_iter().flatten().collect::<Vec<_>>().join(" · "));
    Some(Query { module: Some(m.key.into()), label, state: "ok".into(), result: lines.join("\n"), records })
}

/// page_query for a made module: its records are this module's 条目 / 一项 / 一节.
fn made_page(conn: &Connection, access: &Access, today: &str, ctx: &PageContext, m: &custom::Custom) -> Query {
    let place = match short(&ctx.page, 20) {
        Some(p) => format!("{} · {p}", m.name),
        None => m.name.clone(),
    };
    let only_where = |why: &str| Query { module: Some(m.key.clone()), label: format!("{place}{why}"), state: "blocked".into(), result: format!("页面：{place}"), records: vec![] };
    match access.check_made(&m.key) {
        Err(Blocked::Hidden) => return only_where("（隐藏的模块，只带了在哪一页）"),
        Err(Blocked::Cloud) => return only_where("（不发给云端，只带了在哪一页）"),
        Ok(()) => {}
    }
    let mut lines = vec![format!("页面：{place}（{}）", m.template.label())];
    let filter = short(&ctx.filter, 60);
    if let Some(f) = &filter {
        lines.push(format!("筛选：{f}"));
    }
    let read = |r: &PageRef| made_record(conn, today, r).and_then(Result::ok).filter(|(_, _, rc)| rc.module == m.key);
    let mut records = vec![];
    let mut title_line = None;
    if let Some((title, text, rc)) = ctx.record.as_ref().and_then(read) {
        title_line = Some(format!("{}：{title}", m.name));
        lines.push(format!("打开的{}：\n{text}", kind_label(&rc.kind)));
        records.push(rc);
    }
    let picked: Vec<(String, String, Rec)> = ctx.selected.iter().take(MAX_PICKED).filter_map(read).collect();
    if !picked.is_empty() {
        title_line.get_or_insert(format!("{} · 选中的 {} 条", m.name, picked.len()));
        lines.push(format!("选中的 {} 条：\n{}", picked.len(), picked.iter().map(|(_, t, _)| format!("- {}", t.replace('\n', "；"))).collect::<Vec<_>>().join("\n")));
        records.extend(picked.into_iter().map(|x| x.2));
    }
    let label = title_line.unwrap_or_else(|| [Some(place.clone()), filter].into_iter().flatten().collect::<Vec<_>>().join(" · "));
    Query { module: Some(m.key.clone()), label, state: "ok".into(), result: lines.join("\n"), records }
}

/// The page part of a question as sent: everything when `full`, else only where it was.
pub fn page_block(q: &Query, full: bool) -> String {
    let text = match (full, &q.module) {
        (true, _) => q.result.clone(),
        (false, Some(_)) => format!("{}（这一页的内容这次不能给你，只说在哪一页；别替用户查这个模块）", q.result.lines().next().unwrap_or("")),
        (false, None) => q.result.lines().next().unwrap_or("").to_string(),
    };
    format!("【我正在看的页面】\n{text}")
}

// ---------- picky.db: what a reply looked up (run_queries) ----------

/// One lookup as stored and shown (「查了：…」, expanded: the result).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub module: Option<String>,
    pub label: String,
    /// ok | blocked | failed | unsupported
    pub state: String,
    pub result: String,
    pub records: Vec<Rec>,
}

impl Query {
    pub fn of(a: &Answer) -> Query {
        Query { module: a.module.clone(), label: a.label.clone(), state: a.state.into(), result: a.text.clone(), records: a.records.clone() }
    }
    /// The service refused tools: the reply went on without looking anything up.
    pub fn unsupported() -> Query {
        Query { module: None, label: "这个模型不支持查数据，这次没看柚儿园的数据".into(), state: "unsupported".into(), result: String::new(), records: vec![] }
    }
}

pub fn save_query(conn: &Connection, run_id: i64, position: usize, round: usize, call: &ToolCall, q: &Query) -> Result<(), String> {
    conn.execute(
        "INSERT INTO run_queries (run_id, position, round, call_id, tool, arguments, module, label, state, result, records) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![run_id, position as i64, round as i64, call.id, call.name, call.arguments, q.module, q.label, q.state, q.result, serde_json::to_string(&q.records).unwrap_or_else(|_| "[]".into())],
    )
    .map(|_| ())
    .map_err(common::err)
}

/// The page a question was asked on, kept with its run (`arguments` = the page's own report).
pub fn save_page(conn: &Connection, run_id: i64, arguments: &str, q: &Query) -> Result<(), String> {
    conn.execute(
        "INSERT INTO run_queries (run_id, position, round, call_id, tool, arguments, module, label, state, result, records) VALUES (?1, -1, 0, ?2, '', ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![run_id, PAGE_CALL, arguments, q.module, q.label, q.state, q.result, serde_json::to_string(&q.records).unwrap_or_else(|_| "[]".into())],
    )
    .map(|_| ())
    .map_err(common::err)
}

/// Copies a run's page to another run (a new version of the same question).
pub fn copy_page(conn: &Connection, from_run: i64, to_run: i64) -> Result<Option<Query>, String> {
    conn.execute(
        "INSERT INTO run_queries (run_id, position, round, call_id, tool, arguments, module, label, state, result, records)
         SELECT ?2, position, round, call_id, tool, arguments, module, label, state, result, records FROM run_queries WHERE run_id = ?1 AND call_id = ?3",
        rusqlite::params![from_run, to_run, PAGE_CALL],
    )
    .map_err(common::err)?;
    page_of(conn, to_run)
}

fn select_queries(conn: &Connection, run_id: i64, which: &str) -> Result<Vec<Query>, String> {
    let mut st = conn.prepare(&format!("SELECT module, label, state, result, records FROM run_queries WHERE run_id = ?1 AND {which} ORDER BY position")).map_err(common::err)?;
    let rows = st
        .query_map(rusqlite::params![run_id, PAGE_CALL], |r| {
            let records: String = r.get(4)?;
            Ok(Query { module: r.get(0)?, label: r.get(1)?, state: r.get(2)?, result: r.get(3)?, records: serde_json::from_str(&records).unwrap_or_default() })
        })
        .map_err(common::err)?;
    rows.collect::<Result<_, _>>().map_err(common::err)
}

/// What a run looked up (shown above the reply), without the page.
pub fn lookups_of(conn: &Connection, run_id: i64) -> Result<Vec<Query>, String> {
    select_queries(conn, run_id, "call_id <> ?2")
}

pub fn page_of(conn: &Connection, run_id: i64) -> Result<Option<Query>, String> {
    Ok(select_queries(conn, run_id, "call_id = ?2")?.pop())
}

pub fn queries_of_message(conn: &Connection, message_id: i64) -> Result<Vec<Query>, String> {
    let run: Option<i64> = conn.query_row("SELECT run_id FROM messages WHERE id = ?1", [message_id], |r| r.get(0)).map_err(common::err)?;
    run.map_or(Ok(vec![]), |r| queries_of(conn, r))
}

/// Everything a run looked at (its page first): what follow-ups get.
pub fn queries_of(conn: &Connection, run_id: i64) -> Result<Vec<Query>, String> {
    select_queries(conn, run_id, "?2 = ?2")
}

// ---------- 今日简报 and 年度总结 (brief.rs, yearly.rs): what they read ----------

/// A part of what a brief / summary reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub title: String,
    pub text: String,
}

/// Modules among `keys` this model may not read: hidden ones quietly, cloud-refused ones named (shown
/// as 没看). Returns (readable, left out).
fn readable(access: &Access, keys: &[&str]) -> (Vec<&'static Module>, Vec<&'static str>) {
    let (mut ok, mut left) = (vec![], vec![]);
    for m in keys.iter().filter_map(|k| module(k)) {
        match access.check(m) {
            Ok(()) => ok.push(m),
            Err(Blocked::Cloud) => left.push(m.label),
            Err(Blocked::Hidden) => {}
        }
    }
    (ok, left)
}

/// 今日简报: 今天的待办、快到期的和过期的; 今天该提醒的日子 (a lead day or the day); 该跟进的投递、在笔试
/// 面试的; 最近两周的笔记 (no part for 日子 / 笔记 when there are none). Each part within `budget` tokens. Also the modules left out (not sent to this
/// cloud model).
pub fn brief_parts(conn: &Connection, access: &Access, today: &str, budget: i64) -> Result<(Vec<Part>, Vec<&'static str>), String> {
    let (ok, left) = readable(access, &["todo", "days", "jobs", "notes"]);
    let ask = |tool: &str, args: Value| run(conn, access, tool, &args.to_string(), today, budget).text;
    let mut parts = vec![];
    for m in ok {
        let text = match m.key {
            "todo" => {
                let soon_until = common::add_days(today, 3);
                let soon: Vec<String> = todo::open_tasks(conn, today)?
                    .iter()
                    .filter(|p| p.task.due_date.as_deref().is_some_and(|d| d > today && d <= soon_until.as_str()))
                    .map(|p| task_line(p, today))
                    .collect();
                let soon = if soon.is_empty() { "接下来 3 天没有快到期的。".into() } else { format!("接下来 3 天到期的：\n{}", soon.join("\n")) };
                format!("{}\n\n{soon}", ask(m.tool, json!({ "view": "today" })))
            }
            "jobs" => {
                let statuses = jobs::statuses(conn)?;
                let active: Vec<String> = jobs::jobs(conn, today)?
                    .iter()
                    .filter(|j| j.stage == Stage::Active)
                    .map(|j| {
                        let status = statuses.iter().find(|s| Some(s.id) == j.status_id).map_or("进行中", |s| s.name.as_str());
                        let since = j.steps.iter().rev().find(|s| s.done).and_then(|s| s.done_on.clone()).map(|d| format!("，{d} 起")).unwrap_or_default();
                        format!("- {}：{status}{since}", job_title(j))
                    })
                    .collect();
                let active = if active.is_empty() { "现在没有在笔试、面试的。".into() } else { format!("正在笔试、面试这一步的：\n{}", active.join("\n")) };
                format!("{}\n\n{active}", ask(m.tool, json!({ "view": "follow_up" })))
            }
            "days" => {
                let due: Vec<String> = days::list(conn, today)?.iter().filter(|d| d.today).map(|d| day_row(d).text).collect();
                if due.is_empty() {
                    continue;
                }
                format!("今天 {today}，到了提前提醒的那天或者就是今天的日子：\n{}", due.join("\n"))
            }
            _ => {
                // none in two weeks: nothing to say about them
                let from = common::add_days(today, -13);
                if notes::list(conn, &notes::Filter { from: Some(from.clone()), to: Some(today.to_string()), ..Default::default() })?.is_empty() {
                    continue;
                }
                ask(m.tool, json!({ "view": "recent", "from": from, "to": today }))
            }
        };
        parts.push(Part { title: m.label.into(), text });
    }
    Ok((parts, left))
}

/// What 年度总结 reads (笔记; 做完的事、目标和打卡; 画完的课; 投递的进展; 出行; 读完看完的书影; never
/// 钱来钱往 or 芝麻开门), and the ones left out this time.
pub fn year_left_out(access: &Access) -> Vec<&'static str> {
    readable(access, &["notes", "todo", "drawing", "jobs", "packing", "books", "films"]).1
}

fn shown(access: &Access, key: &str) -> bool {
    module(key).is_some_and(|m| access.check(m).is_ok())
}

/// Done tasks listed per month (more are only counted).
const DONE_LINES: usize = 30;

fn rows<T>(conn: &Connection, sql: &str, args: impl rusqlite::Params, f: impl FnMut(&rusqlite::Row) -> rusqlite::Result<T>) -> Result<Vec<T>, String> {
    let mut st = conn.prepare(sql).map_err(common::err)?;
    let out = st.query_map(args, f).map_err(common::err)?.collect::<Result<_, _>>().map_err(common::err);
    out
}

/// One month (YYYY-MM) of 年度总结, all of it: notes in full (oldest first), goals done (their end
/// date in it), tasks done, lessons finished in it, steps of applications taken in it. Empty =
/// nothing that month.
pub fn month_part(conn: &Connection, access: &Access, month: &str) -> Result<String, String> {
    let (y, m) = (month[..4].parse::<i64>().map_err(|_| "月份不对")?, month[5..7].parse::<u32>().map_err(|_| "月份不对")?);
    let (from, to) = (format!("{month}-01"), format!("{month}-{:02}", common::days_in_month(y, m)));
    let mut out = vec![];
    if shown(access, "notes") {
        let mut list = notes::list(conn, &notes::Filter { from: Some(from.clone()), to: Some(to.clone()), ..Default::default() })?;
        list.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        if !list.is_empty() {
            let lines: Vec<String> = list.iter().map(|n| format!("- {}：{}", &local_minute(conn, &n.created_at)[5..], n.content.trim().replace('\n', "\n  "))).collect();
            out.push(format!("【笔记 {} 条】\n{}", list.len(), lines.join("\n")));
        }
    }
    if shown(access, "todo") {
        let goals = rows(
            conn,
            "SELECT g.name, gr.name, g.start_date, g.end_date, g.review FROM todo_goals g JOIN todo_groups gr ON gr.id = g.group_id WHERE g.status = 'done' AND g.end_date BETWEEN ?1 AND ?2 ORDER BY g.end_date, g.id",
            [&from, &to],
            |r| {
                let (name, group, start, end, review): (String, String, Option<String>, Option<String>, Option<String>) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
                Ok(format!("- {name}（{group}，{}～{}）{}", start.unwrap_or_default(), end.unwrap_or_default(), review.filter(|x| !x.trim().is_empty()).map(|x| format!("复盘：{}", x.trim())).unwrap_or_default()))
            },
        )?;
        if !goals.is_empty() {
            out.push(format!("【做完的目标】\n{}", goals.join("\n")));
        }
        // as the 做完的 view counts: each repeat, check-in and progress log is one; a task done
        // several times is one line
        let done = todo::done_between(conn, &from, &to)?;
        if !done.is_empty() {
            let mut by_task: Vec<(&todo::DoneItem, usize)> = vec![];
            for d in &done {
                match by_task.iter_mut().find(|(x, _)| x.task.task.id == d.task.task.id) {
                    Some((_, n)) => *n += 1,
                    None => by_task.push((d, 1)),
                }
            }
            let mut lines: Vec<String> = by_task
                .iter()
                .take(DONE_LINES)
                .map(|(d, n)| match n {
                    1 => format!("- {} {}{}（{}）", &d.day[5..], plain(&d.task.task.title), d.detail.as_deref().map(|x| format!(" · {x}")).unwrap_or_default(), place(&d.task)),
                    n => format!("- {} ×{n}（{}）", plain(&d.task.task.title), place(&d.task)),
                })
                .collect();
            if by_task.len() > DONE_LINES {
                lines.push(format!("……还有 {} 项没列出", by_task.len() - DONE_LINES));
            }
            out.push(format!("【做完的事 {} 件】（每次重复、打卡、进度记录各算一件）\n{}", done.len(), lines.join("\n")));
        }
    }
    if shown(access, "drawing") {
        let lessons = rows(
            conn,
            "SELECT c.name, l.code, l.title FROM drawing_lessons l JOIN drawing_courses c ON c.id = l.course_id WHERE l.status = 'done' AND l.finished_month = ?1 ORDER BY c.sort_order, c.id, l.sort_order, l.id",
            [month],
            |r| Ok(format!("- {} · {}{}", r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.map(|c| format!("{c} ")).unwrap_or_default(), r.get::<_, String>(2)?)),
        )?;
        if !lessons.is_empty() {
            out.push(format!("【画完的课节 {} 节】\n{}", lessons.len(), lessons.join("\n")));
        }
    }
    if shown(access, "jobs") {
        let statuses = jobs::statuses(conn)?;
        let mut moves: Vec<String> = vec![];
        for j in jobs::jobs(conn, &to)? {
            for step in j.steps.iter().filter(|s| s.done) {
                if let Some(d) = step.done_on.as_deref().filter(|d| *d >= from.as_str() && *d <= to.as_str()) {
                    let name = statuses.iter().find(|s| s.id == step.status_id).map_or("", |s| s.name.as_str());
                    moves.push(format!("- {d} {}：{name}", job_title(&j)));
                }
            }
        }
        moves.sort();
        if !moves.is_empty() {
            out.push(format!("【求职进展 {} 步】\n{}", moves.len(), moves.join("\n")));
        }
    }
    Ok(out.join("\n\n"))
}

/// Books / films finished in `year` (rereads too; they keep only the year).
fn year_works(conn: &Connection, key: &str, year: &str) -> Result<Vec<String>, String> {
    let (table, rereads, fk, who) = if key == "books" { ("books", "book_rereads", "book_id", "author") } else { ("films", "film_rereads", "film_id", "director") };
    let sql = format!(
        "SELECT x.title, x.{who}, x.country, x.rating, EXISTS (SELECT 1 FROM {rereads} r WHERE r.{fk} = x.id AND r.year = ?1) AND coalesce(x.finished_year, '') <> ?1, x.note
         FROM {table} x WHERE (x.status = 'done' AND x.finished_year = ?1) OR EXISTS (SELECT 1 FROM {rereads} r WHERE r.{fk} = x.id AND r.year = ?1)
         ORDER BY x.id"
    );
    rows(conn, &sql, [year], |r| {
        let (title, by, country, rating, again, note): (String, Option<String>, Option<String>, Option<i64>, bool, Option<String>) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?);
        let bits: Vec<String> = [by, country, rating.map(|n| format!("{n} 星")), again.then(|| "重温".to_string()), note.filter(|n| !n.trim().is_empty()).map(|n| clip(n.trim(), 80))].into_iter().flatten().collect();
        Ok(format!("- 《{title}》{}", if bits.is_empty() { String::new() } else { format!("（{}）", bits.join("，")) }))
    })
}

/// Applications with a step taken in [from, to].
fn moved_jobs(conn: &Connection, from: &str, to: &str) -> Result<Vec<jobs::Job>, String> {
    Ok(jobs::jobs(conn, to)?.into_iter().filter(|j| j.steps.iter().any(|s| s.done && s.done_on.as_deref().is_some_and(|d| d >= from && d <= to))).collect())
}

fn trips_between(conn: &Connection, from: &str, to: &str) -> Result<Vec<(String, String, Option<String>)>, String> {
    rows(conn, "SELECT name, depart_on, note FROM trips WHERE depart_on BETWEEN ?1 AND ?2 ORDER BY depart_on, id", [from, to], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
}

/// The year as a whole for 年度总结: books / films finished in it, each course's lessons finished
/// this year, the year's applications (how far they got), check-ins and trips.
pub fn year_part(conn: &Connection, access: &Access, year: &str) -> Result<String, String> {
    let (from, to) = (format!("{year}-01-01"), format!("{year}-12-31"));
    let mut out = vec![];
    for (key, label) in [("books", "读完的书"), ("films", "看完的电影和剧")] {
        if shown(access, key) {
            let rows = year_works(conn, key, year)?;
            out.push(if rows.is_empty() { format!("【{label}】没有") } else { format!("【{label} {} 部】\n{}", rows.len(), rows.join("\n")) });
        }
    }
    if shown(access, "drawing") {
        let rows = rows(
            conn,
            "SELECT c.name, sum(l.status = 'done' AND substr(l.finished_month, 1, 4) = ?1), sum(l.status = 'done'), count(l.id) FROM drawing_courses c
             JOIN drawing_lessons l ON l.course_id = c.id GROUP BY c.id HAVING sum(l.status = 'done' AND substr(l.finished_month, 1, 4) = ?1) > 0 ORDER BY c.sort_order, c.id",
            [year],
            |r| Ok(format!("- {}：这一年画完 {} 节，一共画完 {} / {} 节", r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)),
        )?;
        out.push(if rows.is_empty() { "【画画】这一年没有画完的课节".into() } else { format!("【画画】\n{}", rows.join("\n")) });
    }
    if shown(access, "jobs") {
        let statuses = jobs::statuses(conn)?;
        let mine = moved_jobs(conn, &from, &to)?;
        if mine.is_empty() {
            out.push("【求职】这一年没有投递的进展".into());
        } else {
            let lines: Vec<String> = mine
                .iter()
                .map(|j| {
                    let now = j.status_id.and_then(|id| statuses.iter().find(|s| s.id == id)).map_or("想投", |s| s.name.as_str());
                    format!("- {}：{}{now}", job_title(j), j.applied_on.as_deref().map(|d| format!("{d} 投出，现在 ")).unwrap_or_default())
                })
                .collect();
            let got = mine.iter().filter(|j| j.stage == Stage::Got).count();
            out.push(format!("【求职】这一年有进展的投递 {} 份，拿到 {got} 份\n{}", mine.len(), lines.join("\n")));
        }
    }
    if shown(access, "todo") {
        let c = todo::checkins_between(conn, &from, &to)?;
        if !c.is_empty() {
            out.push(format!("【打卡】\n{}", c.iter().map(|c| format!("- {}：这一年打卡 {} 天，最长连续打了 {} 次", c.title, c.days, c.longest)).collect::<Vec<_>>().join("\n")));
        }
    }
    if shown(access, "packing") {
        let trips = trips_between(conn, &from, &to)?;
        out.push(if trips.is_empty() {
            "【出行】这一年没有出行".into()
        } else {
            let lines: Vec<String> =
                trips.iter().map(|(name, on, note)| format!("- {} 出发：{name}{}", md(on), note.as_deref().filter(|n| !n.trim().is_empty()).map(|n| format!("（{}）", clip(n, 80))).unwrap_or_default())).collect();
            format!("【出行 {} 次】\n{}", trips.len(), lines.join("\n"))
        });
    }
    Ok(out.join("\n\n"))
}

/// A number 年度总结 states (Rust counts, the model only copies).
#[derive(Debug, Clone, PartialEq)]
pub struct Count {
    pub label: &'static str,
    pub unit: &'static str,
    pub n: i64,
}

/// The year's numbers over [from, to] (a whole year, or up to today for one not over yet).
pub fn year_counts(conn: &Connection, access: &Access, from: &str, to: &str) -> Result<Vec<Count>, String> {
    let c = |label, unit, n| Count { label, unit, n };
    let mut out = vec![];
    if shown(access, "notes") {
        let (n, chars, days): (i64, i64, i64) = conn
            .query_row(
                "SELECT count(*), coalesce(sum(length(content)), 0), count(DISTINCT date(created_at, 'localtime')) FROM notes WHERE date(created_at, 'localtime') BETWEEN ?1 AND ?2",
                [from, to],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(common::err)?;
        out.extend([c("笔记", "条", n), c("笔记的字数", "字", chars), c("写了笔记的日子", "天", days)]);
    }
    if shown(access, "todo") {
        let done = todo::done_between(conn, from, to)?.len() as i64;
        let goals: i64 = conn.query_row("SELECT count(*) FROM todo_goals WHERE status = 'done' AND end_date BETWEEN ?1 AND ?2", [from, to], |r| r.get(0)).map_err(common::err)?;
        let checkins: i64 = todo::checkins_between(conn, from, to)?.iter().map(|c| c.days).sum();
        out.extend([c("做完的事", "件", done), c("做完的目标", "个", goals), c("打卡", "天", checkins)]);
    }
    let year = &from[..4];
    // books / films keep only the year they were finished in
    for (key, label, unit) in [("books", "读完的书", "本"), ("films", "看完的电影和剧", "部")] {
        if shown(access, key) {
            out.push(c(label, unit, year_works(conn, key, year)?.len() as i64));
        }
    }
    if shown(access, "drawing") {
        let n: i64 = conn
            .query_row("SELECT count(*) FROM drawing_lessons WHERE status = 'done' AND finished_month BETWEEN ?1 AND ?2", [&from[..7], &to[..7]], |r| r.get(0))
            .map_err(common::err)?;
        out.push(c("画完的课节", "节", n));
    }
    if shown(access, "jobs") {
        let moved = moved_jobs(conn, from, to)?;
        out.extend([c("有进展的投递", "份", moved.len() as i64), c("拿到的工作", "份", moved.iter().filter(|j| j.stage == Stage::Got).count() as i64)]);
    }
    if shown(access, "packing") {
        out.push(c("出行", "次", trips_between(conn, from, to)?.len() as i64));
    }
    Ok(out)
}

/// 12,345
fn grouped(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// The year's 之最 Rust can tell: the month with most notes, the most used tags, tasks done each
/// month.
pub fn year_extras(conn: &Connection, access: &Access, from: &str, to: &str) -> Result<Vec<String>, String> {
    let mut out = vec![];
    if shown(access, "notes") {
        let busiest = rows(
            conn,
            "SELECT strftime('%Y-%m', created_at, 'localtime') AS m, count(*) AS c FROM notes WHERE date(created_at, 'localtime') BETWEEN ?1 AND ?2 GROUP BY m ORDER BY c DESC, m LIMIT 1",
            [from, to],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )?;
        if let Some((m, n)) = busiest.first() {
            out.push(format!("写笔记最多的月份：{}（{n} 条）", month_label(m)));
        }
        let tags = rows(
            conn,
            "SELECT t.tag, count(DISTINCT t.note_id) AS c FROM note_tags t JOIN notes n ON n.id = t.note_id WHERE date(n.created_at, 'localtime') BETWEEN ?1 AND ?2 GROUP BY t.tag ORDER BY c DESC, t.tag LIMIT 5",
            [from, to],
            |r| Ok(format!("#{}（{} 条）", r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )?;
        if !tags.is_empty() {
            out.push(format!("最常用的标签：{}", tags.join("、")));
        }
    }
    if shown(access, "todo") {
        let mut months = vec![];
        let mut m = from[..7].to_string();
        while m.as_str() <= &to[..7] {
            let (y, mo) = (m[..4].parse::<i64>().unwrap_or(2000), m[5..7].parse::<u32>().unwrap_or(1));
            let end = format!("{m}-{:02}", common::days_in_month(y, mo)).min(to.to_string());
            let n = todo::done_between(conn, &format!("{m}-01").max(from.to_string()), &end)?.len();
            months.push(format!("{} {n} 件", month_label(&m)));
            m = if mo == 12 { format!("{}-01", y + 1) } else { format!("{y}-{:02}", mo + 1) };
        }
        if months.iter().any(|x| !x.ends_with(" 0 件")) {
            out.push(format!("每月做完的事：{}", months.join(" · ")));
        }
    }
    Ok(out)
}

/// 「3 月」
pub fn month_label(month: &str) -> String {
    format!("{} 月", month[5..7].trim_start_matches('0'))
}

/// 【这一年的数字】: the counts that aren't zero, then the 之最.
pub fn numbers_text(counts: &[Count], extras: &[String]) -> String {
    counts.iter().filter(|c| c.n != 0).map(|c| format!("- {}：{} {}", c.label, grouped(c.n), c.unit)).chain(extras.iter().map(|e| format!("- {e}"))).collect::<Vec<_>>().join("\n")
}

/// 【和去年比】 (None: nothing to compare with last year): each number both years, and the difference.
pub fn compare_text(this: &[Count], last: &[Count]) -> Option<String> {
    if last.iter().all(|c| c.n == 0) {
        return None;
    }
    let lines: Vec<String> = this
        .iter()
        .filter_map(|c| {
            let before = last.iter().find(|l| l.label == c.label).map_or(0, |l| l.n);
            if c.n == 0 && before == 0 {
                return None;
            }
            let d = c.n - before;
            let diff = if d > 0 { format!("多 {} {}", grouped(d), c.unit) } else if d < 0 { format!("少 {} {}", grouped(-d), c.unit) } else { "一样多".into() };
            Some(format!("- {}：今年 {} {u}，去年 {} {u}，{diff}", c.label, grouped(c.n), grouped(before), u = c.unit))
        })
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::modules::{finance, jobs};

    pub const TODAY: &str = "2026-10-12"; // a Monday

    fn from<T: serde::de::DeserializeOwned>(v: Value) -> T {
        serde_json::from_value(v).unwrap()
    }

    fn sid(c: &Connection, name: &str) -> i64 {
        jobs::statuses(c).unwrap().into_iter().find(|s| s.name == name).unwrap().id
    }

    fn job(c: &Connection, company: &str, steps: &[(&str, &str)]) -> i64 {
        let steps: Vec<Value> = steps.iter().map(|(s, d)| json!({ "statusId": sid(c, s), "done": true, "doneOn": d })).collect();
        jobs::create_job(c, from(json!({ "company": company, "position": "产品经理", "steps": steps })), TODAY).unwrap().id
    }

    /// Four applications: one to follow up now, one due this week, one not yet, one at 面试.
    pub fn seed_jobs(c: &Connection) -> [i64; 4] {
        [
            job(c, "甲公司", &[("已投递", "2026-09-20")]),
            job(c, "乙公司", &[("已投递", "2026-10-01")]),
            job(c, "丙公司", &[("已投递", "2026-10-10")]),
            job(c, "丁公司", &[("已投递", "2026-09-01"), ("面试", "2026-09-15")]),
        ]
    }

    pub fn seed_finance(c: &Connection) {
        let cmb = finance::create_account(c, from(json!({ "name": "招行", "currency": "CNY" }))).unwrap().id;
        let dbs = finance::create_account(c, from(json!({ "name": "DBS", "currency": "SGD" }))).unwrap().id;
        let bal = |id: i64, amount: i64| -> finance::BalanceInput { from(json!({ "accountId": id, "amount": amount })) };
        let rate = |r: f64| -> finance::RateInput { from(json!({ "currency": "SGD", "rate": r })) };
        finance::save_balances(c, "2026-05-01", &[bal(cmb, 100_000), bal(dbs, 10_000)], &[rate(5.3)]).unwrap();
        finance::save_balances(c, "2026-08-01", &[bal(cmb, 200_000)], &[]).unwrap();
        finance::save_balances(c, "2026-10-01", &[bal(cmb, 300_000), bal(dbs, 20_000)], &[rate(5.4)]).unwrap();
        let food = kinds::resolve(c, finance::OUT_KINDS, Some("餐饮")).unwrap();
        let home = kinds::resolve(c, finance::OUT_KINDS, Some("住房")).unwrap();
        for e in [
            json!({ "flow": "out", "amount": 3500, "currency": "CNY", "day": "2026-09-05", "kindId": food, "note": "午饭" }),
            json!({ "flow": "out", "amount": 120_000, "currency": "CNY", "day": "2026-09-10", "kindId": home, "note": "房租" }),
            json!({ "flow": "out", "amount": 7000, "currency": "CNY", "day": "2026-09-28", "endDay": "2026-10-04", "kindId": food }),
            json!({ "flow": "in", "amount": 50_000, "currency": "SGD", "day": "2026-09-15" }),
        ] {
            finance::create_entry(c, from(e)).unwrap();
        }
    }

    fn all() -> Access {
        Access::default()
    }

    fn ask(c: &Connection, access: &Access, tool: &str, args: Value) -> Answer {
        run(c, access, tool, &args.to_string(), TODAY, 4000)
    }

    #[test]
    fn tools_follow_hidden_modules_and_cloud_rules() {
        let c = memory_db();
        let names = |a: &Access| tools(a).iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names(&all()).len(), 25); // 9 modules + search + read (cite.rs) + 14 writes (act.rs)
        assert!(names(&all()).contains(&super::super::cite::SEARCH.to_string()));
        assert!(!names(&all()).iter().any(|n| n.contains("vault")));
        assert!(names(&Access { hidden: MODULES.iter().map(|m| m.key.to_string()).collect(), ..all() }).is_empty()); // nothing to read: no search either
        let hidden = Access { hidden: vec!["jobs".into()], ..all() };
        assert!(!names(&hidden).contains(&"pomelo_jobs".to_string()));
        let a = ask(&c, &hidden, "pomelo_jobs", json!({ "view": "stats" }));
        assert_eq!((a.state, a.label.as_str()), ("blocked", "投石问路 · 已隐藏，没看"));

        // a cloud model: 钱来钱往 is offered (to say so) but refused, with the sentence to start with
        let cloud = Access { cloud: true, cloud_allowed: MODULES.iter().filter(|m| m.key != "finance").map(|m| m.key.to_string()).collect(), ..all() };
        assert_eq!(names(&cloud).len(), 25); // refused writes are offered too, to say so
        assert!(cloud.allows("notes") && cloud.allows("picky") && !cloud.allows("finance") && !hidden.allows("jobs"));
        let a = ask(&c, &cloud, "pomelo_finance", json!({ "view": "assets" }));
        assert_eq!((a.state, a.label.as_str()), ("blocked", "钱来钱往 · 不发给云端，没看"));
        assert!(a.text.contains("请在回答开头写明「钱来钱往不发给云端，这部分没看」"));
        assert!(instructions(TODAY, &cloud, false).contains("「钱来钱往不发给云端，这部分没看」"));
        assert!(instructions(TODAY, &cloud, false).contains("今天是 2026-10-12（周一）"));
        assert!(!instructions(TODAY, &all(), false).contains("云端"));
        assert_eq!(ask(&c, &cloud, "pomelo_jobs", json!({ "view": "stats" })).state, "ok");
        assert_eq!(ask(&c, &Access { cloud: false, ..cloud.clone() }, "pomelo_finance", json!({ "view": "assets" })).state, "ok");

        assert_eq!(ask(&c, &all(), "pomelo_vault", json!({})).state, "failed");
        let bad = run(&c, &all(), "pomelo_finance", "{not json", TODAY, 4000);
        assert_eq!((bad.state, bad.label.as_str()), ("failed", "钱来钱往 · 没查成"));
        assert!(ask(&c, &all(), "pomelo_finance", json!({ "view": "entries", "from": "2026-13" })).text.contains("日期格式不对"));
        assert!(ask(&c, &all(), "pomelo_jobs", json!({ "view": "nope" })).text.contains("view 只能是"));
    }

    #[test]
    fn cloud_settings_default_to_finance_local_only() {
        let p = crate::picky::store::tests::memory_db();
        let made = [Made { key: "list/3".into(), name: "游戏".into(), template: Template::List }, Made { key: "progress/4".into(), name: "日语".into(), template: Template::Progress }];
        let allowed = |p: &Connection| cloud_settings(p, &made).unwrap().into_iter().filter(|s| s.allowed).map(|s| s.module).collect::<Vec<_>>();
        assert_eq!(allowed(&p), ["todo", "books", "films", "drawing", "packing", "jobs", "notes", "days", "list/3", "progress/4"]);
        set_cloud(&p, "finance", true, &made).unwrap();
        set_cloud(&p, "notes", false, &made).unwrap();
        set_cloud(&p, "list/3", false, &made).unwrap();
        assert_eq!(cloud_allowed(&p, &["list/3".into(), "progress/4".into(), "list/9".into()]), ["todo", "books", "films", "drawing", "packing", "finance", "jobs", "days", "progress/4", "list/9"]);
        assert_eq!(cloud_settings(&p, &made).unwrap().last().unwrap().label, "日语");
        assert!(set_cloud(&p, "vault", true, &made).is_err() && set_cloud(&p, "list/9", true, &made).is_err());
    }

    #[test]
    fn follow_ups_now_and_this_week() {
        let c = memory_db();
        let [a, b, ..] = seed_jobs(&c);
        let r = ask(&c, &all(), "pomelo_jobs", json!({ "view": "follow_up" }));
        assert_eq!(r.label, "投石问路 · 该跟进的投递（1 份，本周还有 1 份）");
        assert!(r.text.contains("现在该跟进：\n- 甲公司 · 产品经理：已投递 2026-09-20，等了 22 天"), "{}", r.text);
        assert!(r.text.contains("本周内会到期：\n- 乙公司 · 产品经理：已投递 2026-10-01，等了 11 天，2026-10-15 到期"), "{}", r.text);
        assert!(!r.text.contains("丙公司") && !r.text.contains("丁公司"));
        assert_eq!(r.records.iter().map(|x| x.id).collect::<Vec<_>>(), [a, b]);
        assert_eq!((r.records[0].module.as_str(), r.records[0].kind.as_str()), ("jobs", "job"));
    }

    #[test]
    fn job_stats_are_the_pages_numbers() {
        let c = memory_db();
        seed_jobs(&c);
        let s = jobs::stats(&c, TODAY).unwrap();
        assert_eq!((s.applied, s.this_week, s.ongoing, s.got), (4, 0, 4, 0));
        let funnel: Vec<(String, i64, i64)> = s.funnel.iter().map(|f| (f.name.clone(), f.done, f.have)).collect();
        assert_eq!(funnel[0], ("已投递".into(), 4, 4));
        assert!(funnel.contains(&("面试".into(), 1, 1)));
        assert_eq!(s.weeks.last().unwrap().from, "2026-10-12");
        assert_eq!(s.weeks.iter().map(|b| b.count).sum::<i64>(), 4);
        assert_eq!(s.days.len(), 30);
        let r = ask(&c, &all(), "pomelo_jobs", json!({ "view": "stats" }));
        assert!(r.text.starts_with("今天 2026-10-12。投出 4 份（有投递日期的），本周投了 0 份，进行中 4 份，拿到 0 份。"), "{}", r.text);
        assert!(r.text.contains("- 已投递：4 / 4 份"));
        let list = ask(&c, &all(), "pomelo_jobs", json!({ "view": "list", "status": "面试" }));
        assert_eq!(list.label, "投石问路 · 投递 · 状态含「面试」（1 份）");
        assert!(list.text.contains("丁公司") && list.text.contains("时间线：已投递 2026-09-01 → 面试 2026-09-15"), "{}", list.text);
    }

    #[test]
    fn assets_over_half_a_year() {
        let c = memory_db();
        seed_finance(&c);
        let r = ask(&c, &all(), "pomelo_finance", json!({ "view": "assets" }));
        assert_eq!(r.label, "钱来钱往 · 总资产 2026 年 4 月 13 日 – 10 月 12 日（3 次更新）");
        // the same totals as the page (finance::assets)
        let totals: Vec<i64> = finance::assets(&c).unwrap().totals.iter().filter_map(|t| t.total_cny).collect();
        assert_eq!(totals, [153_000, 253_000, 408_000]);
        assert!(r.text.contains("最早 2026-05-01 ¥1,530，最近 2026-10-01 ¥4,080，变化 +¥2,550（+166.7%）"), "{}", r.text);
        assert!(r.text.contains("- 招行（银行 · 人民币）：2026-05-01 ¥1,000 → 2026-10-01 ¥3,000，变化 +¥2,000（+200.0%）"), "{}", r.text);
        assert!(r.text.contains("- DBS（银行 · 新加坡元）：2026-05-01 S$100 → 2026-10-01 S$200"), "{}", r.text);
        // a shorter range keeps the balance before it as the start
        let r = ask(&c, &all(), "pomelo_finance", json!({ "view": "assets", "from": "2026-07-01" }));
        assert!(r.text.contains("这段时间之前最后一次：2026-05-01 ¥1,530。"), "{}", r.text);
        assert!(r.text.contains("- 招行（银行 · 人民币）：2026-05-01 ¥1,000 → 2026-10-01 ¥3,000"), "{}", r.text);
    }

    #[test]
    fn entries_of_a_month_spread_periods_like_the_reports() {
        let c = memory_db();
        seed_finance(&c);
        assert_eq!(finance::share_between(7000, "2026-09-28", Some("2026-10-04"), "2026-09-01", "2026-09-30"), 3000);
        assert_eq!(finance::spread(7000, "2026-09-28", "2026-10-04")[0].1, 3000);
        assert_eq!(finance::share_between(7000, "2026-09-28", Some("2026-10-04"), "2026-09-01", "2026-12-31"), 7000);
        assert_eq!(finance::share_between(500, "2026-09-02", None, "2026-09-03", "2026-09-30"), 0);
        let r = ask(&c, &all(), "pomelo_finance", json!({ "view": "entries", "from": "2026-09", "to": "2026-09", "flow": "out" }));
        assert_eq!(r.label, "钱来钱往 · 2026 年 9 月支出（3 笔）");
        let month: i64 = finance::totals(&c).unwrap().iter().filter(|t| t.month == "2026-09" && t.flow == Flow::Out).map(|t| t.amount).sum();
        assert_eq!(month, 126_500);
        assert!(r.text.contains("支出（人民币）合计 ¥1,265；按分类：住房 ¥1,200（94.9%）、餐饮 ¥65（5.1%）"), "{}", r.text);
        assert!(r.text.contains("- 2026-09-28 – 2026-10-04（记小额） 支出 餐饮 ¥70，算进这段的 ¥30"), "{}", r.text);
        assert_eq!(r.records.len(), 3);
        assert_eq!(r.records[0].day.as_deref(), Some("2026-09-28"));
        let both = ask(&c, &all(), "pomelo_finance", json!({ "view": "entries", "from": "2026-09-01", "to": "2026-09-30" }));
        assert!(both.text.contains("收入（新加坡元）合计 S$500"), "{}", both.text);
        let months = ask(&c, &all(), "pomelo_finance", json!({ "view": "months", "from": "2026-09", "to": "2026-10" }));
        assert!(months.text.contains("- 2026 年 9 月 人民币：支出 ¥1,265（住房 ¥1,200、餐饮 ¥65），收入 ¥0，结余 -¥1,265"), "{}", months.text);
    }

    #[test]
    fn results_are_cut_to_the_budget() {
        let f = Found::new("x".into(), vec!["头".into()]).rows((0..100).map(|i| Row { text: format!("- 第 {i} 行有一些文字"), rec: Some(rec("notes", "note", i, "t")) }).collect());
        let (text, recs) = f.render(120);
        assert!(text.starts_with("头\n- 第 0 行"));
        let shown = text.lines().filter(|l| l.starts_with("- ")).count();
        assert!(shown > 0 && shown < 100);
        assert_eq!(recs.len(), shown);
        assert!(text.ends_with(&format!("……还有 {} 条没列出（可以缩短时间段或加条件再查）", 100 - shown)));
        let (all, recs) = Found::new("x".into(), vec![]).section("空的", vec![]).render(1000);
        assert_eq!((all.as_str(), recs.len()), ("空的：没有", 0));
    }

    #[test]
    fn wording_helpers() {
        assert_eq!(span("2026-09-01", "2026-09-30"), "2026 年 9 月");
        assert_eq!(span("2026-01-01", "2026-12-31"), "2026 年");
        assert_eq!(span("2026-09-03", "2026-10-12"), "2026 年 9 月 3 日 – 10 月 12 日");
        assert_eq!(span("2025-12-30", "2026-01-02"), "2025 年 12 月 30 日 – 2026 年 1 月 2 日");
        assert_eq!(span("2026-10-12", "2026-10-12"), "2026 年 10 月 12 日");
        assert_eq!(money(123_456, Currency::Cny), "¥1,234.56");
        assert_eq!(money(100_000, Currency::Sgd), "S$1,000");
        assert_eq!(money(-350, Currency::Usd), "-US$3.50");
        assert_eq!(money(0, Currency::Hkd), "HK$0");
        assert_eq!(day_of("2026-02", true).unwrap(), "2026-02-28");
        assert_eq!(day_of("2026", false).unwrap(), "2026-01-01");
        assert_eq!(day_of("2026/10/01", false).unwrap(), "2026-10-01");
        assert!(day_of("2026-13", false).is_err() && day_of("昨天", false).is_err() && day_of("2026年1", false).is_err());
        assert_eq!(rule_text("weekly:1,3"), "每周一、三");
        assert_eq!(rule_text("after:2w"), "做完后过 2 周");
        assert_eq!(rule_text("yearly:03-01/2"), "每 2 年的 03-01");
    }

    /// Every view of every tool answers on an empty database and on seeded data.
    #[test]
    fn every_view_runs() {
        for seeded in [false, true] {
            let c = memory_db();
            if seeded {
                seed_jobs(&c);
                seed_finance(&c);
                notes::create(&c, from(json!({ "content": "今天读完了 #读书 一本书" }))).unwrap();
            }
            for t in tools(&all()).into_iter().filter(|t| t["function"]["parameters"]["properties"]["view"].is_object()) {
                let name = t["function"]["name"].as_str().unwrap();
                for view in t["function"]["parameters"]["properties"]["view"]["enum"].as_array().unwrap() {
                    let a = ask(&c, &all(), name, json!({ "view": view, "keyword": "读" }));
                    assert!(a.state == "ok" || (a.state == "failed" && a.text.starts_with("没查成：")), "{name} {view}: {a:?}");
                    assert!(!a.label.is_empty() && !a.text.is_empty());
                }
            }
        }
    }

    /// Every view over the real import files (local only, gitignored): labels and sizes only.
    /// Run: cargo test real_views -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_views() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype");
        let mut c = memory_db();
        let scratch = crate::db::tests::temp_dir("real-views");
        for f in ["pomelo-import.json", "todo-import.json", "jobs-import.json", "notes-import.json"] {
            if let Ok(text) = std::fs::read_to_string(dir.join(f)) {
                crate::transfer::apply(&mut c, &text, true, &scratch).unwrap();
            }
        }
        let today = crate::modules::todo::today(&c).unwrap();
        for t in tools(&all()).into_iter().filter(|t| t["function"]["parameters"]["properties"]["view"].is_object()) {
            let name = t["function"]["name"].as_str().unwrap();
            for view in t["function"]["parameters"]["properties"]["view"]["enum"].as_array().unwrap() {
                let a = run(&c, &all(), name, &json!({ "view": view, "keyword": "的" }).to_string(), &today, MAX_RESULT);
                assert!(a.state == "ok" || a.text.starts_with("没查成："), "{name} {view}: {a:?}");
                println!("{name} {view}: {} · {} lines · ~{} tokens · {} records", a.label, a.text.lines().count(), super::super::context::estimate(&a.text), a.records.len());
                assert!(super::super::context::estimate(&a.text) <= MAX_RESULT + 200);
            }
        }
    }

    #[test]
    fn stuck_tasks_and_idle_goals() {
        let c = memory_db();
        let g = todo::create_group(&c, "学习").unwrap().id;
        let goal = todo::create_goal(&c, from(json!({ "groupId": g, "name": "论文" }))).unwrap().id;
        let busy = todo::create_goal(&c, from(json!({ "groupId": g, "name": "健身" }))).unwrap().id;
        todo::create_task_on(&c, from(json!({ "groupId": g, "goalId": goal, "title": "交开题报告", "dueDate": "2026-10-01" })), TODAY).unwrap();
        let old = todo::create_task_on(&c, from(json!({ "groupId": g, "title": "整理书架" })), TODAY).unwrap().id;
        c.execute("UPDATE todo_tasks SET created_at = '2026-09-01T04:00:00Z' WHERE id = ?1", [old]).unwrap();
        todo::create_task_on(&c, from(json!({ "groupId": g, "title": "新加的" })), TODAY).unwrap();
        let ran = todo::create_task_on(&c, from(json!({ "groupId": g, "goalId": busy, "title": "跑步" })), TODAY).unwrap().id;
        todo::set_done_on(&c, ran, true, TODAY).unwrap();
        let r = ask(&c, &all(), "pomelo_todo", json!({ "view": "stuck" }));
        assert_eq!(r.label, "一步一步 · 拖着的任务和没动的目标（过期 1 件，久没动 1 件，目标 1 个）");
        assert!(r.text.contains("过期的（过期久的在前）：\n- 交开题报告（截止 2026-10-01 · 过期 11 天 · 学习 / 论文）"), "{}", r.text);
        assert!(r.text.contains("- 整理书架（学习） · 加入于 2026-09-01，41 天了"), "{}", r.text);
        assert!(r.text.contains("没动的目标：\n- 论文（进度 0 / 1 · 还没有完成过 · 学习）"), "{}", r.text);
        assert!(!r.text.contains("新加的") && !r.text.contains("健身"));
    }

    #[test]
    fn job_moves_in_a_fortnight() {
        let c = memory_db();
        seed_jobs(&c);
        let r = ask(&c, &all(), "pomelo_jobs", json!({ "view": "moves" }));
        assert_eq!(r.label, "投石问路 · 2026 年 9 月 29 日 – 10 月 12 日的进展（2 步）");
        assert!(r.text.contains("勾了 2 步，涉及 2 份投递；其中新投出 2 份"), "{}", r.text);
        assert!(r.text.contains("- 2026-10-10 丙公司 · 产品经理：已投递（现在：已投递）\n- 2026-10-01 乙公司"), "{}", r.text);
    }

    #[test]
    fn taste_for_recommendations() {
        let c = memory_db();
        for (t, by, country, rating, status) in [("沙丘", "赫伯特", "美国", Some(5), "done"), ("三体", "刘慈欣", "中国", Some(4), "done"), ("平庸之作", "某人", "美国", Some(2), "done"), ("想读的", "某人", "日本", None, "todo")] {
            books::create(&c, from(json!({ "title": t, "author": by, "country": country, "rating": rating, "status": status, "finishedYear": if status == "done" { Some("2025") } else { None } }))).unwrap();
        }
        let r = ask(&c, &all(), "pomelo_books", json!({ "view": "taste" }));
        assert_eq!(r.label, "摸鱼读读 · 口味（4 星以上 2 本）");
        assert!(r.text.contains("读完的 3 本里，打了 4 星以上的 2 本（含重读的星级）"), "{}", r.text);
        assert!(r.text.contains("国家：中国 1、美国 1"), "{}", r.text);
        assert!(r.text.contains("4 星以上的（星级高的在前）：\n- 《沙丘》（作者 赫伯特 · 美国 · 未分类 · 已读 · 2025 读完 · ★5）\n- 《三体》"), "{}", r.text);
        assert!(r.text.contains("已经在单子里的（全部 4 本）：\n- 《沙丘》、《三体》、《平庸之作》、《想读的》"), "{}", r.text);
        assert_eq!(r.records.len(), 2);
    }

    #[test]
    fn notes_by_any_word_and_recent_topics() {
        let c = memory_db();
        for (text, at) in [("晨跑五公里 #运动", "2026-10-10T01:00:00Z"), ("读书时想到跑步的节奏 #读书", "2026-10-09T01:00:00Z"), ("读完一本书 #读书", "2026-10-08T01:00:00Z"), ("很久以前 #读书", "2026-01-01T01:00:00Z")] {
            let id = notes::create(&c, from(json!({ "content": text }))).unwrap().id;
            c.execute("UPDATE notes SET created_at = ?1 WHERE id = ?2", (at, id)).unwrap();
        }
        let r = ask(&c, &all(), "pomelo_notes", json!({ "view": "search", "keyword": "跑步 读书" }));
        assert_eq!(r.label, "念念不忘 · 「跑步 读书」相关的笔记（3 条）");
        let order: Vec<&str> = r.text.lines().filter(|l| l.starts_with("- ")).map(|l| l.split('：').nth(1).unwrap_or("")).collect();
        assert!(order[0].starts_with("读书时想到跑步"), "{order:?}"); // both words first
        let r = ask(&c, &all(), "pomelo_notes", json!({ "view": "recent" }));
        assert!(r.label.ends_with("的笔记（3 条）"), "{}", r.label);
        assert!(r.text.contains("这段时间用得最多的标签：#读书 2、#运动 1。"), "{}", r.text);
    }

    #[test]
    fn days_soon_list_brief_and_documents_without_notes() {
        let c = memory_db();
        let add = |v: Value| days::create(&c, from(v), TODAY).unwrap().id;
        let mom = add(json!({ "kind": "birthday", "name": "妈妈", "date": "1965-10-19", "remind": [7], "ideas": "围巾", "gifts": [{ "year": "2025", "gift": "保温杯" }] }));
        let passport = add(json!({ "kind": "document", "name": "护照", "date": "2026-11-11", "remind": [30, 7], "note": "号码 E123" }));
        add(json!({ "kind": "day", "name": "在一起", "date": "2020-12-01", "repeat": "year" }));
        add(json!({ "kind": "day", "name": "搬家", "date": "2026-03-01" }));
        let r = ask(&c, &all(), "pomelo_days", json!({ "view": "soon" }));
        assert_eq!(r.label, "来日方长 · 快到的日子（2 个）");
        assert!(r.text.contains("- 妈妈：生日 · 1965-10-19 出生 · 下一次 2026-10-19 · 还有 7 天 · 满 61 岁 · 提前 7 天提醒 · 礼物点子：围巾 · 送过：2025 保温杯"), "{}", r.text);
        assert!(r.text.contains("- 护照：证件 · 2026-11-11 到期 · 还有 30 天 · 提前 30 天、提前 7 天提醒\n"), "{}", r.text);
        assert!(r.text.contains("其他日子：\n- 在一起：纪念日 · 2020-12-01 · 每年 · 下一次 2026-12-01 · 还有 50 天 · 第 6 周年"), "{}", r.text);
        assert!(!r.text.contains("E123") && !r.text.contains("搬家"));
        assert_eq!(r.records.iter().map(|x| x.id).collect::<Vec<_>>()[..2], [mom, passport]);
        let r = ask(&c, &all(), "pomelo_days", json!({ "view": "list" }));
        assert!(r.text.contains("- 搬家：纪念日 · 2026-03-01 · 已经 225 天"), "{}", r.text);
        assert_eq!(ask(&c, &all(), "pomelo_days", json!({ "view": "list", "kind": "birthday" })).records.len(), 1);
        assert_eq!(ask(&c, &all(), "pomelo_days", json!({ "view": "list", "keyword": "保温杯" })).records[0].id, mom);
        assert!(ask(&c, &all(), "pomelo_days", json!({ "view": "list", "keyword": "E123" })).records.is_empty());
        let (_, text, _) = record_text(&c, TODAY, &PageRef { kind: "day".into(), id: passport }).unwrap();
        assert!(!text.contains("E123"), "{text}");
        // 今日简报: a lead day or the day itself
        let (parts, _) = brief_parts(&c, &all(), TODAY, 1200).unwrap();
        let p = parts.iter().find(|p| p.title == "来日方长").unwrap();
        assert!(p.text.contains("妈妈") && p.text.contains("护照") && !p.text.contains("在一起"), "{}", p.text);
        let (parts, _) = brief_parts(&c, &all(), "2026-10-13", 1200).unwrap();
        assert!(parts.iter().all(|p| p.title != "来日方长"));
    }

    #[test]
    fn made_lists_and_progress() {
        let c = memory_db();
        let (games, [plat, _, rate, _, _]) = lists::tests::seed(&c);
        let zelda = lists::create_item(&c, games, from(json!({ "name": "塞尔达", "values": { plat.to_string(): "Switch", rate.to_string(): 5 } }))).unwrap().id;
        lists::create_item(&c, games, from(json!({ "name": "星露谷" }))).unwrap();
        progress::tests::seed(&c);
        let access = Access { made: made(&c), ..all() };
        let names: Vec<String> = tools(&access).iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect();
        assert!(names.contains(&LISTS_TOOL.to_string()) && names.contains(&PROGRESS_TOOL.to_string()));
        let hidden = Access { hidden: vec!["list/1".into()], ..access.clone() };
        assert!(!tools(&hidden).iter().any(|t| t["function"]["name"] == LISTS_TOOL));

        let r = ask(&c, &access, LISTS_TOOL, json!({ "view": "lists" }));
        assert!(r.text.contains("- 游戏（2 条）：字段 名称、平台（单选：Switch、PC）、标签（多选：解谜、冒险、独立）、评分（评分）"), "{}", r.text);
        let r = ask(&c, &access, LISTS_TOOL, json!({ "view": "items", "keyword": "switch" }));
        assert_eq!((r.label.as_str(), r.module.as_deref()), ("五花八门 · 游戏（1 条）", Some("list/1")));
        assert!(r.text.contains("- 塞尔达：平台 Switch · 评分 ★★★★★"), "{}", r.text);
        assert_eq!((r.records[0].module.as_str(), r.records[0].kind.as_str(), r.records[0].id), ("list/1", "list_item", zelda));

        let r = ask(&c, &access, PROGRESS_TOOL, json!({ "view": "overview" }));
        assert!(r.text.contains("动画（季 → 集）：\n- 第一季：完成 1 / 3 集，进行中 1；下一集 2 相遇；还剩 2 集，最近 6 个月完成 1 集（每月约 0.2 集），照这个速度还要约 12 个月，2027 年 10 月前后完成"), "{}", r.text);
        let r = ask(&c, &access, PROGRESS_TOOL, json!({ "view": "lessons", "name": "动画 第一季", "status": "done" }));
        assert!(r.text.contains("- 1 出发（正篇）：已完成（2026-09）") && !r.text.contains("相遇"), "{}", r.text);
        assert!(ask(&c, &access, PROGRESS_TOOL, json!({ "view": "lessons", "name": "第二季" })).text.contains("有：动画 · 第一季"));

        // a cloud model without 游戏: named, 动画 still read
        let cloud = Access { cloud: true, cloud_allowed: vec!["progress/2".into()], ..access.clone() };
        let r = ask(&c, &cloud, LISTS_TOOL, json!({ "view": "lists" }));
        assert_eq!(r.state, "blocked");
        assert!(r.text.contains("「游戏不发给云端，这部分没看」"));
        assert_eq!(ask(&c, &cloud, PROGRESS_TOOL, json!({ "view": "overview" })).state, "ok");
        assert!(instructions(TODAY, &cloud, false).contains("游戏"));
        assert!(!cloud.allows("list/1") && cloud.allows("progress/2") && !cloud.allows("list"));

        // the page and a record in full
        let ctx = PageContext { module: "list/1".into(), record: Some(PageRef { kind: "list_item".into(), id: zelda }), ..Default::default() };
        let q = page_query(&c, &access, TODAY, &ctx).unwrap();
        assert_eq!(q.label, "游戏：塞尔达");
        assert!(q.result.contains("平台：Switch") && q.result.contains("评分：★★★★★"), "{}", q.result);
        assert_eq!(page_query(&c, &cloud, TODAY, &ctx).unwrap().state, "blocked");
        let (_, text, rc) = record_text(&c, TODAY, &PageRef { kind: "progress_lesson".into(), id: 1 }).unwrap();
        assert!(text.contains("动画 · 第一季：1 出发") && rc.parent.is_some(), "{text}");
    }

    #[test]
    fn recent_notes_fall_back_to_the_latest_month_with_notes() {
        let c = memory_db();
        let id = notes::create(&c, from(json!({ "content": "旧想法 #读书" }))).unwrap().id;
        c.execute("UPDATE notes SET created_at = ?1 WHERE id = ?2", ("2026-02-20T04:00:00Z", id)).unwrap();
        let r = ask(&c, &all(), "pomelo_notes", json!({ "view": "recent" }));
        assert!(r.text.starts_with("最近 30 天（2026-09-13 至 2026-10-12）没有笔记，下面是最近有笔记的 30 天。"), "{}", r.text);
        assert!(r.label.ends_with("的笔记（1 条）") && r.text.contains("旧想法"), "{}", r.text);
        // a range the model chose stays as asked
        let r = ask(&c, &all(), "pomelo_notes", json!({ "view": "recent", "from": "2026-09-01" }));
        assert!(r.text.contains("记了 0 条") && r.text.contains("最近一条记于 2026-02-20"), "{}", r.text);
    }

    #[test]
    fn drawing_pace_and_finish() {
        let lesson = |status: Mark, month: Option<&str>| drawing::Lesson { id: 0, course_id: 1, code: None, kind_id: None, title: "x".into(), status, finished_month: month.map(String::from), note: None, sort_order: 0, created_at: String::new(), updated_at: String::new() };
        let mut ls: Vec<drawing::Lesson> = ["2026-05", "2026-06", "2026-08", "2026-09", "2026-10", "2026-10"].iter().map(|m| lesson(Mark::Done, Some(m))).collect();
        ls.push(lesson(Mark::Done, Some("2026-04"))); // before the six months
        ls.extend((0..4).map(|_| lesson(Mark::Todo, None)));
        let p = drawing::pace(&ls, TODAY);
        assert_eq!((p.recent, p.remaining, p.months_left, p.finish.as_deref()), (6, 4, Some(4), Some("2027-02")));
        assert_eq!(pace_text(&p), "还剩 4 节，最近 6 个月画完 6 节（每月约 1.0 节），照这个速度还要约 4 个月，2027 年 2 月前后画完");
        assert_eq!(drawing::pace(&ls[..7], TODAY).months_left, Some(0));
        assert_eq!(pace_text(&drawing::pace(&ls[6..], TODAY)), "还剩 4 节，最近 6 个月没有画完的，没法估计什么时候画完");
    }

    #[test]
    fn what_a_trip_may_have_forgotten() {
        let c = memory_db();
        let tp = packing::create_template(&c, from(json!({ "name": "出国" }))).unwrap().id;
        for n in ["护照", "充电器"] {
            packing::create_item(&c, packing::List::Template, tp, from(json!({ "name": n }))).unwrap();
        }
        let older = packing::create_trip(&c, from(json!({ "name": "去年大阪", "departOn": "2025-12-01" })), None).unwrap().id;
        packing::create_item(&c, packing::List::Trip, older, from(json!({ "name": "雨伞" }))).unwrap();
        let trip = packing::create_trip(&c, from(json!({ "name": "东京五天", "departOn": "2026-11-01" })), Some(tp)).unwrap().id;
        let passport = packing::items(&c, packing::List::Trip, trip).unwrap().into_iter().find(|i| i.name == "护照").unwrap().id;
        packing::delete_item(&c, packing::List::Trip, passport).unwrap();
        let missing = packing::maybe_missing(&c, trip).unwrap();
        assert_eq!(missing.iter().map(|m| (m.0.as_str(), m.2, m.3)).collect::<Vec<_>>(), [("护照", 1, true), ("雨伞", 1, false)]);
        let r = ask(&c, &all(), "pomelo_packing", json!({ "view": "trip" }));
        assert!(r.text.starts_with("东京五天：出发 2026-11-01（还有 20 天），1 件，已装好 0 件（✓ 是装好的）；用的模板：出国。"), "{}", r.text);
        assert!(r.text.contains("- 护照（其他，1 个清单里有，这次用的模板里有）\n- 雨伞（其他，1 个清单里有）"), "{}", r.text);
    }

    #[test]
    fn biggest_changes_first_in_cny() {
        let c = memory_db();
        seed_finance(&c);
        let r = ask(&c, &all(), "pomelo_finance", json!({ "view": "assets" }));
        assert!(r.text.contains("按人民币折算（期末汇率）变化最大的账户：招行 +¥2,000、DBS +S$100（约 +¥540）。"), "{}", r.text);
        assert!(r.text.contains("- 2026-08-01 ¥2,530（比上次 +¥1,000）\n- 2026-10-01 ¥4,080（比上次 +¥1,550）"), "{}", r.text);
        assert!(r.text.contains("变化 +S$100（+100.0%，约 +¥540）"), "{}", r.text);
    }

    #[test]
    fn earlier_lookups_go_only_where_allowed() {
        let q = |module: &str, state: &str| Query { module: Some(module.into()), label: format!("{module} 标签"), state: state.into(), result: format!("{module} 的数据"), records: vec![] };
        let mut r = crate::picky::context::Request {
            history: vec![Msg::new("user", "问"), Msg::new("assistant", "答")],
            looked: vec![(0, vec![q("finance", "ok"), q("jobs", "ok"), q("notes", "blocked")])],
            ..Default::default()
        };
        let cloud = Access { cloud: true, cloud_allowed: vec!["jobs".into(), "notes".into()], ..all() };
        r.with_lookups(|x| cloud.may_send(x));
        assert_eq!(r.history[0].content, "问\n\n【回答这个问题时查到的柚儿园数据，之后可能有变化】\n### jobs 标签\njobs 的数据");
        assert_eq!(r.history[1].content, "答");
        // hidden now: not sent either
        let mut r2 = crate::picky::context::Request { history: vec![Msg::new("user", "问")], looked: vec![(0, vec![q("jobs", "ok")])], ..Default::default() };
        let hidden = Access { hidden: vec!["jobs".into()], ..all() };
        r2.with_lookups(|x| hidden.may_send(x));
        assert_eq!(r2.history[0].content, "问");
    }

    #[test]
    fn follow_ups_and_exports_carry_lookups() {
        let p = crate::picky::store::tests::memory_db();
        let b = crate::picky::chat::begin_run(&p, None, None, "这周该跟进哪些投递", "local:x", "X").unwrap();
        let c = memory_db();
        seed_jobs(&c);
        let call = ToolCall { id: "c1".into(), name: "pomelo_jobs".into(), arguments: r#"{"view":"follow_up"}"#.into() };
        save_query(&p, b.run_id, 0, 1, &call, &Query::of(&ask(&c, &all(), &call.name, json!({ "view": "follow_up" })))).unwrap();
        crate::picky::chat::finish(&p, b.run_id, b.reply_id, &crate::picky::chat::Outcome { content: "跟进甲公司。".into(), state: "completed", ..Default::default() }).unwrap();
        let mut next = crate::picky::chat::begin_run(&p, Some(b.session_id), None, "甲公司是哪天投的", "local:x", "X").unwrap().request;
        assert_eq!(next.looked.len(), 1);
        next.with_lookups(|x| all().may_send(x));
        assert!(next.history[0].content.starts_with("这周该跟进哪些投递\n\n【回答这个问题时查到的柚儿园数据，之后可能有变化】\n### 投石问路 · 该跟进的投递"), "{}", next.history[0].content);
        assert!(next.history[0].content.contains("甲公司 · 产品经理：已投递 2026-09-20"));
        let md = crate::picky::export::markdown(&p, b.session_id).unwrap();
        assert!(md.contains("查了：投石问路 · 该跟进的投递（1 份，本周还有 1 份）\n\n<details>\n<summary>查到的内容</summary>"), "{md}");
        assert!(md.contains("```text\n今天 2026-10-12。"), "{md}");
        assert!(crate::picky::export::copy_markdown(&p, b.session_id).unwrap().contains("查了：投石问路"));
    }

    fn page(module: &str, page: Option<&str>, record: Option<(&str, i64)>, selected: &[(&str, i64)]) -> PageContext {
        PageContext {
            module: module.into(),
            page: page.map(String::from),
            filter: None,
            record: record.map(|(kind, id)| PageRef { kind: kind.into(), id }),
            selected: selected.iter().map(|(kind, id)| PageRef { kind: (*kind).into(), id: *id }).collect(),
        }
    }

    #[test]
    fn the_page_a_question_is_asked_on() {
        let c = memory_db();
        let [a, ..] = seed_jobs(&c);
        seed_finance(&c);
        let q = page_query(&c, &all(), TODAY, &page("jobs", Some("投递"), Some(("job", a)), &[])).unwrap();
        assert_eq!((q.label.as_str(), q.state.as_str()), ("投递：甲公司 · 产品经理", "ok"));
        assert!(q.result.starts_with("页面：投石问路 · 投递\n打开的投递：\n投递：甲公司 · 产品经理\n"), "{}", q.result);
        assert!(q.result.contains("跟进：等了 22 天，该跟进了（满 14 天）") && q.result.contains("时间线：已投递 2026-09-20"), "{}", q.result);
        assert_eq!(q.records.iter().map(|r| r.id).collect::<Vec<_>>(), [a]);
        // a record from another module is ignored; filters show in the label
        let mut ctx = page("jobs", Some("投递"), Some(("entry", 1)), &[]);
        ctx.filter = Some("状态：面试".into());
        assert_eq!(page_query(&c, &all(), TODAY, &ctx).unwrap().label, "投石问路 · 投递 · 状态：面试");
        // 多选
        let ids: Vec<i64> = finance::entries_between(&c, "2026-09-01", "2026-09-30").unwrap().iter().map(|e| e.id).take(2).collect();
        let picked: Vec<(&str, i64)> = ids.iter().map(|id| ("entry", *id)).collect();
        let q = page_query(&c, &all(), TODAY, &page("finance", Some("收支"), None, &picked)).unwrap();
        assert_eq!(q.label, "钱来钱往 · 选中的 2 条");
        assert!(q.result.contains("选中的 2 条收支：\n- "), "{}", q.result);
        // a cloud model that may not get 钱来钱往: only where; hidden: the same
        let cloud = Access { cloud: true, cloud_allowed: vec!["jobs".into()], ..all() };
        let q = page_query(&c, &cloud, TODAY, &page("finance", Some("收支"), None, &picked)).unwrap();
        assert_eq!((q.state.as_str(), q.result.as_str(), q.records.len()), ("blocked", "页面：钱来钱往 · 收支", 0));
        assert_eq!(q.label, "钱来钱往 · 收支（不发给云端，只带了在哪一页）");
        let hidden = Access { hidden: vec!["jobs".into()], ..all() };
        assert_eq!(page_query(&c, &hidden, TODAY, &page("jobs", None, Some(("job", a)), &[])).unwrap().result, "页面：投石问路");
        // 芝麻开门 and 小拾: nothing; 首页: where
        assert!(page_query(&c, &all(), TODAY, &page("vault", None, Some(("job", a)), &[])).is_none());
        assert!(page_query(&c, &all(), TODAY, &page("picky", None, None, &[])).is_none());
        assert_eq!(page_query(&c, &all(), TODAY, &page("home", None, None, &[])).unwrap().result, "页面：首页");
        // every kind a page may send reads
        let b = books::create(&c, from(json!({ "title": "沙丘", "note": "很长的备注" }))).unwrap().id;
        let n = notes::create(&c, from(json!({ "content": "一条 #想法" }))).unwrap().id;
        let g = todo::create_group(&c, "学习").unwrap().id;
        let t = todo::create_task_on(&c, from(json!({ "groupId": g, "title": "写论文", "note": "第一章" })), TODAY).unwrap().id;
        for (module, kind, id) in [("books", "book", b), ("notes", "note", n), ("todo", "task", t), ("todo", "group", g)] {
            let q = page_query(&c, &all(), TODAY, &page(module, None, Some((kind, id)), &[])).unwrap();
            assert_eq!(q.records.len(), 1, "{kind}: {}", q.result);
        }
    }

    #[test]
    fn a_question_keeps_its_page_and_the_model_gets_what_it_may() {
        let p = crate::picky::store::tests::memory_db();
        let c = memory_db();
        let [a, ..] = seed_jobs(&c);
        let q = page_query(&c, &all(), TODAY, &page("jobs", Some("投递"), Some(("job", a)), &[])).unwrap();
        let extras = crate::picky::chat::Extras { page: Some((q.clone(), "{}".into())), ..Default::default() };
        let b = crate::picky::chat::begin_run_in(&p, None, &Default::default(), "这条投递该怎么跟进", "local:x", "X", &extras).unwrap();
        let mut r = b.request.clone();
        r.with_page(|_| true);
        assert!(r.prompt.starts_with("这条投递该怎么跟进\n\n【我正在看的页面】\n页面：投石问路 · 投递\n打开的投递："), "{}", r.prompt);
        let mut only = b.request.clone();
        only.with_page(|_| false);
        assert_eq!(only.prompt, "这条投递该怎么跟进\n\n【我正在看的页面】\n页面：投石问路 · 投递（这一页的内容这次不能给你，只说在哪一页；别替用户查这个模块）");
        let shown = crate::picky::chat::path(&p, b.session_id).unwrap();
        assert_eq!(shown[0].page.as_ref().map(|x| x.label.as_str()), Some("投递：甲公司 · 产品经理"));
        assert!(shown[1].queries.is_empty()); // the page isn't a lookup
        crate::picky::chat::finish(&p, b.run_id, b.reply_id, &crate::picky::chat::Outcome { content: "发邮件问问。".into(), state: "completed", ..Default::default() }).unwrap();
        // a new version of the reply sends the page again
        let v = crate::picky::chat::begin_version(&p, b.reply_id, None, "local:x", "X", &[], &[]).unwrap();
        assert_eq!(v.request.page.as_ref().map(|x| x.label.as_str()), Some("投递：甲公司 · 产品经理"));
        crate::picky::chat::finish(&p, v.run_id, v.reply_id, &crate::picky::chat::Outcome { content: "再发一封。".into(), state: "completed", ..Default::default() }).unwrap();
        // a follow-up still knows the page (as an earlier lookup)
        let mut next = crate::picky::chat::begin_run(&p, Some(b.session_id), None, "邮件怎么写", "local:x", "X").unwrap().request;
        next.with_lookups(|x| all().may_send(x));
        assert!(next.history[0].content.contains("### 投递：甲公司 · 产品经理"), "{}", next.history[0].content);
        assert!(next.page.is_none());
        let md = crate::picky::export::markdown(&p, b.session_id).unwrap();
        assert!(md.contains("带着：投递：甲公司 · 产品经理"), "{md}");
    }

    #[test]
    fn mock_model_looks_up_and_answers() {
        let system = instructions(TODAY, &all(), false);
        let offered: Vec<&str> = MODULES.iter().map(|m| m.tool).collect();
        let calls = mock_calls("这周该跟进哪些投递", &system, &offered);
        assert_eq!(calls.iter().map(|c| (c.name.as_str(), c.arguments.as_str())).collect::<Vec<_>>(), [("pomelo_jobs", r#"{"view":"follow_up"}"#)]);
        let calls = mock_calls("总资产这半年怎么变的", &system, &offered);
        assert_eq!(calls[0].arguments, r#"{"from":"2026-04-13","view":"assets"}"#);
        assert!(mock_calls("总资产这半年怎么变的", &system, &["pomelo_jobs"]).is_empty());
        assert!(mock_calls("你好", &system, &offered).is_empty());
        let msgs = [Msg::new("user", "钱"), Msg::tool("a", "没有查：钱来钱往的数据不发给云端模型。请在回答开头写明「钱来钱往不发给云端，这部分没看」，并提醒可以换本地模型再问。"), Msg::tool("b", "投出 4 份\n- 已投递")];
        let answer = mock_answer(&msgs);
        assert!(answer.starts_with("钱来钱往不发给云端，这部分没看。\n\n"), "{answer}");
        assert!(answer.contains("> 投出 4 份\n> - 已投递"));
    }

    #[test]
    fn queries_stay_with_the_run_and_go_with_the_conversation() {
        let p = crate::picky::store::tests::memory_db();
        let b = crate::picky::chat::begin_run(&p, None, None, "q", "local:x", "X").unwrap();
        let c = memory_db();
        seed_jobs(&c);
        let call = ToolCall { id: "c1".into(), name: "pomelo_jobs".into(), arguments: r#"{"view":"follow_up"}"#.into() };
        let a = ask(&c, &all(), &call.name, json!({ "view": "follow_up" }));
        save_query(&p, b.run_id, 0, 1, &call, &Query::of(&a)).unwrap();
        save_query(&p, b.run_id, 1, 0, &ToolCall::default(), &Query::unsupported()).unwrap();
        let got = queries_of(&p, b.run_id).unwrap();
        assert_eq!(got[0], Query::of(&a));
        assert_eq!(got[1].state, "unsupported");
        assert_eq!(crate::picky::chat::path(&p, b.session_id).unwrap()[1].queries.len(), 2);
        p.execute("DELETE FROM sessions", []).unwrap();
        assert_eq!(p.query_row("SELECT count(*) FROM run_queries", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
}
