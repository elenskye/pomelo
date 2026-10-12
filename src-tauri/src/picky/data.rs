//! 小拾查数据 (v0.3.5 「问我的数据」): read-only lookups the model asks for while answering (tool calls).
//! Rules (requirements.md「小拾懂 Pomelo」, architecture.md「小拾」):
//! - One tool per module (`pomelo_todo` …). Each view runs the module's own read functions, so totals
//!   and derived numbers are the pages' (jobs::stats, finance::assets / totals, todo::agenda …); only
//!   the wording for the model is here. Results are compact Chinese text cut to the room left in the
//!   window ("还有 N 条没列出"); the records shown are kept so the answer can link them.
//! - Never read: 芝麻开门 (no tool exists) and modules hidden in 设置 → 侧栏 (no tool offered; a call
//!   anyway is refused). For a cloud model, modules not allowed to go to the cloud (设置 → 小拾,
//!   `data.cloud`; default 钱来钱往 local only) are refused with the sentence the answer starts with.
//! - Tool definitions carry no user data (no names, categories, amounts).
//! - Nothing here writes Pomelo data.

use super::provider::{Msg, ToolCall};
use super::store::{pref_get, pref_set};
use crate::modules::common::{self, Status as Mark};
use crate::modules::finance::{self, AccountKind, Currency, Flow};
use crate::modules::jobs::{self, Stage};
use crate::modules::{books, drawing, films, kinds, notes, packing, todo};
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
];

fn module(key: &str) -> Option<&'static Module> {
    MODULES.iter().find(|m| m.key == key)
}

// ---------- 设置 → 小拾: 允许发给云端 ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSetting {
    pub module: &'static str,
    pub label: &'static str,
    pub allowed: bool,
}

/// Per module (picky.db preferences); unset ones keep their default.
pub fn cloud_settings(conn: &Connection) -> Result<Vec<CloudSetting>, String> {
    let saved: HashMap<String, bool> = pref_get(conn, CLOUD_KEY).ok().flatten().unwrap_or_default();
    Ok(MODULES.iter().map(|m| CloudSetting { module: m.key, label: m.label, allowed: saved.get(m.key).copied().unwrap_or(m.cloud) }).collect())
}

pub fn set_cloud(conn: &Connection, key: &str, allowed: bool) -> Result<(), String> {
    module(key).ok_or_else(|| format!("没有这个模块：{key}"))?;
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
    /// Modules that may go to cloud models.
    pub cloud_allowed: Vec<String>,
}

pub enum Blocked {
    Hidden,
    Cloud,
}

/// Modules hidden in 设置 → 侧栏 (pomelo.db).
pub fn hidden(pomelo: &Connection) -> Vec<String> {
    crate::settings::get(pomelo, HIDDEN_KEY).ok().flatten().and_then(|v| serde_json::from_value::<Vec<String>>(v).ok()).unwrap_or_default()
}

/// Modules allowed to go to cloud models (picky.db).
pub fn cloud_allowed(picky: &Connection) -> Vec<String> {
    cloud_settings(picky).unwrap_or_default().into_iter().filter(|s| s.allowed).map(|s| s.module.to_string()).collect()
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
    /// An earlier result may go to this model (its module is readable and allowed here).
    pub fn may_send(&self, q: &Query) -> bool {
        q.module.as_deref().and_then(module).is_some_and(|m| self.check(m).is_ok())
    }
    /// Modules that have a tool this time (hidden ones don't; cloud-refused ones do, to say so).
    pub fn offered(&self) -> Vec<&'static Module> {
        MODULES.iter().filter(|m| !self.hidden.iter().any(|h| h == m.key)).collect()
    }
    fn refused(&self) -> Vec<&'static Module> {
        self.offered().into_iter().filter(|m| matches!(self.check(m), Err(Blocked::Cloud))).collect()
    }
}

/// 「钱来钱往不发给云端，这部分没看」
fn cloud_sentence(labels: &[&str]) -> String {
    format!("{}不发给云端，这部分没看", labels.join("、"))
}

const WEEKDAYS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

/// The part of the system message about looking things up.
pub fn instructions(today: &str, access: &Access) -> String {
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
         - 要推荐书、电影或剧时，先用 view taste 看用户的口味，再推荐用户单子里还没有的新作品（用你自己的知识，不联网，说明可能不准）；不要只把用户读过看过的列一遍。\n\
         - 出行清单可能漏了什么，除了查到的，也可以按目的地和天数用常识补充。"
    );
    let refused: Vec<&str> = access.refused().iter().map(|m| m.label).collect();
    if !refused.is_empty() {
        let sentence = cloud_sentence(&refused);
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
        _ => Value::Null,
    }
}

pub fn tools(access: &Access) -> Vec<Value> {
    access.offered().into_iter().map(definition).collect()
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
    pub module: Option<&'static str>,
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
    let Some(m) = MODULES.iter().find(|m| m.tool == name) else {
        return Answer { module: None, label: format!("没有这个查询：{}", clip(name, 40)), state: "failed", text: format!("没有叫 {name} 的工具，只能用给出的这些。"), records: vec![] };
    };
    match access.check(m) {
        Err(Blocked::Hidden) => {
            return Answer { module: Some(m.key), label: format!("{} · 已隐藏，没看", m.label), state: "blocked", text: format!("没有查：{}在侧栏里隐藏了，小拾不读它的数据。", m.label), records: vec![] }
        }
        Err(Blocked::Cloud) => {
            let sentence = cloud_sentence(&[m.label]);
            return Answer {
                module: Some(m.key),
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
        _ => Err("没有这个模块".into()),
    };
    match found {
        Ok(f) => {
            let label = format!("{} · {}", m.label, f.label);
            let (text, records) = f.render(budget.min(MAX_RESULT));
            Answer { module: Some(m.key), label, state: "ok", text, records }
        }
        Err(e) => failed(m, e),
    }
}

fn failed(m: &Module, e: String) -> Answer {
    Answer { module: Some(m.key), label: format!("{} · 没查成", m.label), state: "failed", text: format!("没查成：{e}"), records: vec![] }
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
    match (p.months_left, &p.finish) {
        (Some(0), _) => "全部画完了".into(),
        (Some(m), Some(f)) => format!("还剩 {} 节，最近 {} 个月画完 {} 节（每月约 {:.1} 节），照这个速度还要约 {m} 个月，{}前后画完", p.remaining, drawing::PACE_MONTHS, p.recent, p.rate, month_text(f)),
        _ => format!("还剩 {} 节，最近 {} 个月没有画完的，没法估计什么时候画完", p.remaining, drawing::PACE_MONTHS),
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

// ---------- mock model (dev builds, tests, mockIpc.ts mirrors it) ----------

/// Which lookups the mock model makes for a question (only among `offered` tools).
pub fn mock_calls(question: &str, system: &str, offered: &[&str]) -> Vec<ToolCall> {
    let today = system.split("今天是 ").nth(1).map(|s| s.chars().take(10).collect::<String>()).filter(|d| common::check_date(&Some(d.clone()), "").is_ok());
    let Some(today) = today else { return vec![] };
    let has = |words: &[&str]| words.iter().any(|w| question.contains(w));
    let mut want: Vec<(&str, Value)> = vec![];
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
    if has(&["出门", "行李", "漏带"]) {
        want.push(("pomelo_packing", json!({ "view": "trip" })));
    }
    if has(&["笔记"]) {
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
    let read: Vec<&&str> = results.iter().filter(|r| !r.starts_with("没有查")).collect();
    if read.is_empty() {
        out.push_str("这是开发版的**模拟回答**。");
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
    state.picky.with_db(|c| cloud_settings(c))
}

#[tauri::command]
pub fn picky_data_cloud_set(state: State<AppState>, module: String, allowed: bool) -> Result<(), String> {
    state.picky.with_db(|c| set_cloud(c, &module, allowed))
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
        Query { module: a.module.map(String::from), label: a.label.clone(), state: a.state.into(), result: a.text.clone(), records: a.records.clone() }
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

pub fn queries_of_message(conn: &Connection, message_id: i64) -> Result<Vec<Query>, String> {
    let run: Option<i64> = conn.query_row("SELECT run_id FROM messages WHERE id = ?1", [message_id], |r| r.get(0)).map_err(common::err)?;
    run.map_or(Ok(vec![]), |r| queries_of(conn, r))
}

pub fn queries_of(conn: &Connection, run_id: i64) -> Result<Vec<Query>, String> {
    let mut st = conn.prepare("SELECT module, label, state, result, records FROM run_queries WHERE run_id = ?1 ORDER BY position").map_err(common::err)?;
    let rows = st
        .query_map([run_id], |r| {
            let records: String = r.get(4)?;
            Ok(Query { module: r.get(0)?, label: r.get(1)?, state: r.get(2)?, result: r.get(3)?, records: serde_json::from_str(&records).unwrap_or_default() })
        })
        .map_err(common::err)?;
    rows.collect::<Result<_, _>>().map_err(common::err)
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
        assert_eq!(names(&all()).len(), 8);
        assert!(!names(&all()).iter().any(|n| n.contains("vault")));
        let hidden = Access { hidden: vec!["jobs".into()], ..all() };
        assert!(!names(&hidden).contains(&"pomelo_jobs".to_string()));
        let a = ask(&c, &hidden, "pomelo_jobs", json!({ "view": "stats" }));
        assert_eq!((a.state, a.label.as_str()), ("blocked", "投石问路 · 已隐藏，没看"));

        // a cloud model: 钱来钱往 is offered (to say so) but refused, with the sentence to start with
        let cloud = Access { cloud: true, cloud_allowed: MODULES.iter().filter(|m| m.key != "finance").map(|m| m.key.to_string()).collect(), ..all() };
        assert_eq!(names(&cloud).len(), 8);
        let a = ask(&c, &cloud, "pomelo_finance", json!({ "view": "assets" }));
        assert_eq!((a.state, a.label.as_str()), ("blocked", "钱来钱往 · 不发给云端，没看"));
        assert!(a.text.contains("请在回答开头写明「钱来钱往不发给云端，这部分没看」"));
        assert!(instructions(TODAY, &cloud).contains("「钱来钱往不发给云端，这部分没看」"));
        assert!(instructions(TODAY, &cloud).contains("今天是 2026-10-12（周一）"));
        assert!(!instructions(TODAY, &all()).contains("云端"));
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
        let allowed = |p: &Connection| cloud_settings(p).unwrap().into_iter().filter(|s| s.allowed).map(|s| s.module).collect::<Vec<_>>();
        assert_eq!(allowed(&p), ["todo", "books", "films", "drawing", "packing", "jobs", "notes"]);
        set_cloud(&p, "finance", true).unwrap();
        set_cloud(&p, "notes", false).unwrap();
        assert_eq!(cloud_allowed(&p), ["todo", "books", "films", "drawing", "packing", "finance", "jobs"]);
        assert!(set_cloud(&p, "vault", true).is_err());
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
            for t in tools(&all()) {
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
        for t in tools(&all()) {
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

    #[test]
    fn mock_model_looks_up_and_answers() {
        let system = instructions(TODAY, &all());
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
