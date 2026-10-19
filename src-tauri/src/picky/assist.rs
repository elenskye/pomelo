//! 小拾 helps in the edit panels (v0.3.5 阶段 4 「润色和补全」): one model call each, thinking off (fast),
//! nothing kept in 小拾's conversations. Results only go into the panel or a list to pick from — the
//! user saves / adds (undoable as usual); nothing here writes Pomelo data.
//! - 润色: a note rewritten to this time's request (任务内容、目标复盘、投递备注、书影 / 课节 / 出行 /
//!   收支的备注; never 念念不忘).
//! - 补全: 书影 (作者 / 导演、国家、类型 from the model's own knowledge), JD (公司、岗位、地点), a task's
//!   子任务, what a trip may have forgotten (names + 类别).
//! - Same model as 小拾; a stopped local model is started first (the last one picked, else the first in
//!   the list). A module a cloud model may not get (钱来钱往 by default) is refused.

use super::data::{self, Access};
use super::provider::{self, Event, Msg, Options};
use super::store::{pref_get, pref_set};
use super::{Mode, Picky, Route, Selection};
use crate::modules::{common, kinds, packing, todo};
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// The local model picked last (started when a help is asked with none running).
pub const LAST_LOCAL: &str = "model.last_local";
/// Every assist system prompt starts with this (the mock model answers by it).
pub const MARK: &str = "【小拾帮忙】";
const START_WAIT: Duration = Duration::from_secs(180);

// ---------- the model ----------

/// Where a help goes: the selected model, started if it is a stopped local one.
async fn route(p: &Picky) -> Result<Route, String> {
    if p.selection() == Selection::Local {
        ready_local(p, None).await?;
    }
    p.target()
}

/// A local model ready for work: the running one (or the one starting), else `want` (if listed),
/// the last used or the first listed, started. Returns the key it started (None: one was there).
pub async fn ready_local(p: &Picky, want: Option<&str>) -> Result<Option<String>, String> {
    if p.runtime.ready_model().is_some() {
        return Ok(None);
    }
    let status = p.runtime.local_status();
    let mut started = None;
    if !status.working {
        let listed = |k: &String| status.models.iter().any(|m| &m.key == k);
        let last: Option<String> = p.with_db(|c| pref_get(c, LAST_LOCAL)).ok().flatten();
        let key = want.map(String::from).filter(listed).or(last.filter(listed)).or_else(|| status.models.first().map(|m| m.key.clone())).ok_or("没有可用的本地模型")?;
        p.runtime.activate(&key).await;
        p.changed();
        started = Some(key);
    }
    let since = std::time::Instant::now();
    while p.runtime.ready_model().is_none() {
        if since.elapsed() > START_WAIT {
            return Err("本地模型还没准备好，等它就绪再试。".into());
        }
        if let Some(issue) = p.runtime.local_status().issue {
            return Err(format!("本地模型没能启动：{}", issue.title));
        }
        tokio::time::sleep(Duration::from_millis(if p.mode == Mode::Mock { 50 } else { 1000 })).await;
        p.runtime.refresh(false).await;
    }
    Ok(started)
}

/// A module's data may go to this model (a cloud model only gets the allowed ones).
fn check(p: &Picky, route: &Route, module: &str) -> Result<(), String> {
    let m = data::MODULES.iter().find(|m| m.key == module).ok_or_else(|| format!("没有这个模块：{module}"))?;
    if !route.model.starts_with("cloud:") {
        return Ok(());
    }
    let allowed = p.with_db(|c| Ok(data::cloud_allowed(c, &[])))?;
    let access = Access { cloud: true, cloud_allowed: allowed, ..Default::default() };
    if access.may_send(&data::Query { module: Some(m.key.into()), label: String::new(), state: "ok".into(), result: String::new(), records: vec![] }) {
        Ok(())
    } else {
        Err(format!("选了云端模型：{}不发给云端。换成本地模型再用。", m.label))
    }
}

/// One answer, whole; thinking when the model can (with room for it).
async fn ask(route: &Route, system: &str, user: &str, max_tokens: i64) -> Result<String, String> {
    let mut out = String::new();
    let messages = vec![Msg::new("system", &format!("{MARK}{system}")), Msg::new("user", user)];
    provider::stream(route.target.clone(), messages, Options { max_tokens: Some(route.with_thinking(max_tokens, true)), thinking: Some(true), ..Default::default() }, |e| {
        if let Event::Content(t) = e {
            out.push_str(&t);
        }
    })
    .await?;
    Ok(out.trim().to_string())
}

/// The first JSON value (object or array) in a reply, fences and chatter around it ignored.
pub fn json_in(text: &str) -> Option<Value> {
    let starts: Vec<usize> = text.char_indices().filter(|(_, c)| *c == '{' || *c == '[').map(|(i, _)| i).collect();
    for s in starts {
        let mut de = serde_json::Deserializer::from_str(&text[s..]).into_iter::<Value>();
        if let Some(Ok(v)) = de.next() {
            return Some(v);
        }
    }
    None
}

pub(crate) fn field(v: &Value, k: &str, max: usize) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(|s| s.trim().chars().take(max).collect::<String>()).filter(|s| !s.is_empty() && s != "无" && s != "未知")
}

/// A reply that should be just text: no fences, quotes or a leading 「改好的：」.
pub fn clean_text(reply: &str) -> String {
    let mut t = reply.trim();
    if let Some(inner) = t.strip_prefix("```").and_then(|r| r.split_once('\n')).map(|(_, rest)| rest.trim_end().trim_end_matches("```")) {
        t = inner.trim();
    }
    for prefix in ["改好的：", "润色后：", "修改后："] {
        t = t.strip_prefix(prefix).unwrap_or(t).trim();
    }
    for (a, b) in [("「", "」"), ("“", "”"), ("\"", "\"")] {
        if t.starts_with(a) && t.ends_with(b) && t.chars().count() > 2 && !t[a.len()..t.len() - b.len()].contains(b) {
            t = &t[a.len()..t.len() - b.len()];
        }
    }
    t.to_string()
}

// ---------- 润色 ----------

pub const POLISH: &str = "你是文字润色助手。按用户这次的要求改写原文，保持原意和原来的语言，保留原文的换行和列表；只输出改好的全文，不要解释，不要加引号或标题。";

async fn polish(app: &AppHandle, module: &str, text: &str, request: &str) -> Result<String, String> {
    if module == "notes" {
        return Err("念念不忘的笔记不润色".into());
    }
    let text = text.trim();
    if text.is_empty() {
        return Err("没有可以润色的文字".into());
    }
    if text.chars().count() > 5000 {
        return Err("文字太长了（最多 5000 字）".into());
    }
    let p = &app.state::<AppState>().picky;
    let route = route(p).await?;
    check(p, &route, module)?;
    let request = match request.trim() {
        "" => "改得更通顺、自然，意思不变",
        r => r,
    };
    let user = format!("这次的要求：{request}\n\n原文：\n{text}");
    let out = clean_text(&ask(&route, POLISH, &user, (super::context::estimate(text) * 2 + 256).clamp(256, 8192)).await?);
    if out.is_empty() {
        return Err("模型没有给出改好的文字，再试一次吧".into());
    }
    Ok(out)
}

// ---------- 补全 ----------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkFill {
    /// 作者 / 导演.
    pub creator: Option<String>,
    pub country: Option<String>,
    pub kind_id: Option<i64>,
}

pub fn work_prompt(module: &str, kinds: &[String]) -> String {
    let (thing, who) = if module == "books" { ("书", "作者") } else { ("电影或剧", "导演") };
    format!(
        "用户给出一部{thing}的名称，请用你自己的知识补全信息，不确定的留空字符串，不要编造。只输出一个 JSON 对象：{{\"creator\": \"{who}（多人用「 / 」隔开，用常见的中文译名）\", \"country\": \"国家（中文，如 美国）\", \"kind\": \"从这些类型里选一个：{}；都不合适就留空\"}}",
        kinds.join("、")
    )
}

pub fn parse_work(reply: &str, kinds: &[(i64, String)]) -> Result<WorkFill, String> {
    let v = json_in(reply).filter(Value::is_object).ok_or("模型的回答看不懂，再试一次吧")?;
    let kind = field(&v, "kind", 20);
    Ok(WorkFill {
        creator: field(&v, "creator", 100),
        country: field(&v, "country", 30),
        kind_id: kind.and_then(|k| kinds.iter().find(|(_, n)| *n == k).map(|(id, _)| *id)),
    })
}

/// 补全: 书单 / 影单检索 first (v0.4 phase 6, when on); the model only fills what it didn't find
/// (and is not even started when it found everything).
async fn fill_work(app: &AppHandle, module: &str, title: &str) -> Result<WorkFill, String> {
    if module != "books" && module != "films" {
        return Err("只能补全书和影视".into());
    }
    let title = title.trim();
    if title.is_empty() {
        return Err("先填名称".into());
    }
    let names: Vec<(i64, String)> = app.state::<AppState>().with_db(|c| Ok(kinds::list(c, module)?.into_iter().map(|k| (k.id, k.name)).collect()))?;
    let looked = super::skills::work(app, module, title, None).await.map(|w| WorkFill { kind_id: super::skills::works::kind_id(&names, w.kind.as_deref()), creator: w.creator, country: w.country });
    if let Some(f) = looked.as_ref().filter(|f| f.creator.is_some() && f.country.is_some() && f.kind_id.is_some()) {
        return Ok(f.clone());
    }
    let p = &app.state::<AppState>().picky;
    let by_model = async {
        let route = route(p).await?;
        check(p, &route, module)?;
        let system = work_prompt(module, &names.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>());
        parse_work(&ask(&route, &system, &format!("名称：{}", common::clean(Some(title.chars().take(200).collect())).unwrap_or_default()), 512).await?, &names)
    }
    .await;
    match (looked, by_model) {
        // what the lookup found wins
        (Some(l), Ok(m)) => Ok(WorkFill { creator: l.creator.or(m.creator), country: l.country.or(m.country), kind_id: l.kind_id.or(m.kind_id) }),
        (Some(l), Err(_)) => Ok(l),
        (None, m) => m,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobFill {
    pub company: Option<String>,
    pub position: Option<String>,
    pub location: Option<String>,
}

pub const JOB: &str = "从用户贴的招聘信息（JD）里找出公司名称、岗位名称、工作地点。只输出一个 JSON 对象：{\"company\": \"公司\", \"position\": \"岗位\", \"location\": \"城市（几个城市用「 / 」隔开）\"}；找不到的写空字符串，不要编造。";

pub fn parse_job(reply: &str) -> Result<JobFill, String> {
    let v = json_in(reply).filter(Value::is_object).ok_or("模型的回答看不懂，再试一次吧")?;
    Ok(JobFill { company: field(&v, "company", 100), position: field(&v, "position", 100), location: field(&v, "location", 100) })
}

async fn fill_job(app: &AppHandle, jd: &str) -> Result<JobFill, String> {
    let jd = jd.trim();
    if jd.is_empty() {
        return Err("先贴进 JD".into());
    }
    let p = &app.state::<AppState>().picky;
    let route = route(p).await?;
    check(p, &route, "jobs")?;
    let jd: String = jd.chars().take(8000).collect();
    parse_job(&ask(&route, JOB, &jd, 512).await?)
}

pub const SPLIT: &str = "把用户的任务拆成 3～8 个具体、一次能做完的子任务，按先后顺序，避开已经有的子任务。只输出一个 JSON 数组，每项是一个子任务标题（20 字以内，不要编号），比如 [\"查资料\", \"写提纲\"]。";

pub const NOTE_SPLIT: &str = "用户的一条笔记里写了几件要做的事。把它们拆成待办，每件一个，按笔记里的顺序；笔记里没提到的不要加，已经转成待办的不要再列。只输出一个 JSON 数组，每项是一个待办标题（20 字以内，动词开头，不要编号），比如 [\"给房东打电话\", \"交水电费\"]。";

/// A reply's list: its JSON array, else one item per line (small models sometimes write `[一项]` lines).
fn list_in(reply: &str) -> Vec<Value> {
    if let Some(Value::Array(a)) = json_in(reply) {
        return a;
    }
    reply
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.starts_with("```"))
        .map(|l| l.trim_matches(|c: char| "[]\"「」“”,，*•-– ".contains(c)).to_string())
        .filter(|l| !l.is_empty())
        .map(Value::String)
        .collect()
}

pub fn parse_list(reply: &str, max: usize, have: &[String]) -> Result<Vec<String>, String> {
    let items = list_in(reply);
    let mut out: Vec<String> = vec![];
    for it in &items {
        let s = it.as_str().map(String::from).or_else(|| it.get("title").and_then(Value::as_str).map(String::from)).unwrap_or_default();
        let s: String = s.trim().trim_start_matches(|c: char| c.is_ascii_digit() || ".、) ".contains(c)).chars().take(60).collect();
        if !s.is_empty() && !out.contains(&s) && !have.iter().any(|h| h.trim() == s) {
            out.push(s);
        }
    }
    out.truncate(max);
    if out.is_empty() {
        return Err("没拆出新的子任务".into());
    }
    Ok(out)
}

async fn split_task(app: &AppHandle, task_id: i64) -> Result<Vec<String>, String> {
    let p = &app.state::<AppState>().picky;
    let route = route(p).await?;
    check(p, &route, "todo")?;
    let (user, have) = app.state::<AppState>().with_db(|c| {
        let t = todo::task(c, task_id)?;
        if t.task_type != todo::TaskType::Normal {
            return Err("只有普通任务能拆子任务".into());
        }
        let place = todo::tasks(c, Some(t.group_id), t.goal_id)?;
        let have: Vec<String> = place.iter().filter(|x| x.parent_id == Some(t.id)).map(|x| x.title.replace("**", "")).collect();
        let goal = t.goal_id.and_then(|g| todo::goal(c, g).ok()).map(|g| g.name);
        let mut user = format!("任务：{}", t.title.replace("**", ""));
        if let Some(n) = &t.note {
            user.push_str(&format!("\n内容：{}", n.replace("**", "")));
        }
        if let Some(g) = goal {
            user.push_str(&format!("\n属于目标：{g}"));
        }
        if let Some(d) = &t.due_date {
            user.push_str(&format!("\n截止：{d}"));
        }
        if !have.is_empty() {
            user.push_str(&format!("\n已经有的子任务：{}", have.join("、")));
        }
        Ok((user, have))
    })?;
    parse_list(&ask(&route, SPLIT, &user, 1024).await?, 8, &have)
}

/// 念念不忘 让小拾拆成待办: the things to do a note mentions (those already made into tasks left out).
async fn split_note(app: &AppHandle, note_id: i64) -> Result<Vec<String>, String> {
    let p = &app.state::<AppState>().picky;
    let route = route(p).await?;
    check(p, &route, "notes")?;
    let (user, have) = app.state::<AppState>().with_db(|c| {
        let n = crate::modules::notes::get(c, note_id)?;
        let have: Vec<String> = n.tasks.iter().map(|t| t.title.replace("**", "")).collect();
        let mut user = format!("笔记：\n{}", n.content.trim());
        if !have.is_empty() {
            user.push_str(&format!("\n\n已经转成的待办：{}", have.join("、")));
        }
        Ok((user, have))
    })?;
    parse_list(&ask(&route, NOTE_SPLIT, &user, 1024).await?, 12, &have)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemIdea {
    pub name: String,
    pub kind_id: Option<i64>,
}

pub fn trip_prompt(kinds: &[String]) -> String {
    format!(
        "用户要出门。按目的地、天数和季节，结合他以前常带的东西，建议这次清单里可能漏的东西（最多 12 件，最重要的在前），不要重复清单里已有的。只输出一个 JSON 数组，每项 {{\"name\": \"物品名（10 字以内）\", \"kind\": \"从这些类别里选：{}\"}}。",
        kinds.join("、")
    )
}

pub fn parse_items(reply: &str, kinds: &[(i64, String)], have: &[String]) -> Result<Vec<ItemIdea>, String> {
    let v = json_in(reply).ok_or("模型的回答看不懂，再试一次吧")?;
    let items = v.as_array().ok_or("模型的回答看不懂，再试一次吧")?;
    let norm = |s: &str| s.trim().to_lowercase();
    let mut out: Vec<ItemIdea> = vec![];
    for it in items {
        let (name, kind) = match it {
            Value::String(s) => (s.clone(), None),
            v => (v.get("name").and_then(Value::as_str).unwrap_or("").to_string(), v.get("kind").and_then(Value::as_str).map(String::from)),
        };
        let name: String = name.trim().chars().take(40).collect();
        if name.is_empty() || have.iter().any(|h| norm(h) == norm(&name)) || out.iter().any(|o| norm(&o.name) == norm(&name)) {
            continue;
        }
        out.push(ItemIdea { name, kind_id: kind.and_then(|k| kinds.iter().find(|(_, n)| *n == k).map(|(id, _)| *id)) });
    }
    out.truncate(12);
    if out.is_empty() {
        return Err("没想到这次还缺什么".into());
    }
    Ok(out)
}

async fn trip_missing(app: &AppHandle, trip_id: i64) -> Result<Vec<ItemIdea>, String> {
    let p = &app.state::<AppState>().picky;
    let route = route(p).await?;
    check(p, &route, "packing")?;
    let st = app.state::<AppState>();
    let (user, names, have) = st.with_db(|c| {
        let today = todo::today(c)?;
        let t = packing::trip(c, trip_id)?;
        let names: Vec<(i64, String)> = kinds::list(c, "packing")?.into_iter().map(|k| (k.id, k.name)).collect();
        let items = packing::items(c, packing::List::Trip, trip_id)?;
        let have: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
        let usual: Vec<String> = packing::maybe_missing(c, trip_id)?.into_iter().take(30).map(|(n, _, k, _)| format!("{n}（{k} 次）")).collect();
        let mut user = format!("出行：{}", t.name);
        if let Some(d) = &t.depart_on {
            user.push_str(&format!("\n出发：{d}（今天 {today}）"));
        }
        if let Some(n) = &t.note {
            user.push_str(&format!("\n备注：{n}"));
        }
        user.push_str(&format!("\n这次的清单：{}", if have.is_empty() { "（还是空的）".to_string() } else { have.join("、") }));
        if !usual.is_empty() {
            user.push_str(&format!("\n模板和以前的出行里有、这次没有的：{}", usual.join("、")));
        }
        Ok((user, names, have))
    })?;
    let system = trip_prompt(&names.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>());
    parse_items(&ask(&route, &system, &user, 1024).await?, &names, &have)
}

// ---------- 一句话填好 ----------
// A new-record panel's top line: the model splits one sentence into fields; Rust checks every value
// (dates real, 分类 / 渠道 / 状态 / 模板 / 分组 / 目标 matched by name, 币种 known, amounts in cents)
// and the panel fills what came back. One thing per sentence (several are v0.5).

pub const QUICK: &str = "用户用一句话说要新建的一条记录，你把它拆成各个字段。只输出一个 JSON 对象，只写这句话里说到的、或能确定的字段，其他的不要写，不要编造。日期写成 YYYY-MM-DD，按下面的日历算。";

const WEEKDAYS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

/// Today and the weeks around it written as the words people use (这周五 / 下周五 / 上周三…), so a
/// small model copies the date instead of counting.
pub fn calendar(today: &str) -> String {
    let monday = crate::modules::jobs::week_start(today);
    let week = |word: &str, start: &str| (0..7).map(|i| format!("{word}{} {}", WEEKDAYS[i as usize], common::add_days(start, i))).collect::<Vec<_>>().join("、");
    format!(
        "今天是 {today}（周{}），昨天 {}，明天 {}，后天 {}。{}。{}。{}。",
        WEEKDAYS[(common::weekday(today) as usize + 6) % 7],
        common::add_days(today, -1),
        common::add_days(today, 1),
        common::add_days(today, 2),
        week("这周", &monday),
        week("下周", &common::add_days(&monday, 7)),
        week("上周", &common::add_days(&monday, -7))
    )
}

/// The one date a sentence names, worked out from `today`: 今天 / 明天 / 后天 / 大后天 / 昨天 / 前天,
/// (这|本|下|下下|上)(个)?(周|星期|礼拜)X, X月X日 / X月X号, X号. None when there is none or more than one
/// (then the model's dates stand). Small models get 「下周五」 wrong; this doesn't.
pub fn date_in(sentence: &str, today: &str) -> Option<String> {
    let c: Vec<char> = sentence.chars().collect();
    let at = |i: usize, w: &str| w.chars().enumerate().all(|(k, x)| c.get(i + k) == Some(&x));
    let digits = |i: usize| -> (Option<u32>, usize) {
        let n: String = c[i..].iter().take_while(|x| x.is_ascii_digit()).take(2).collect();
        (n.parse().ok(), n.len())
    };
    let monday = crate::modules::jobs::week_start(today);
    let (y, m) = (&today[..4], &today[5..7]);
    let real = |d: String| common::check_date(&Some(d.clone()), "").is_ok().then_some(d);
    let mut found: Vec<String> = vec![];
    let mut i = 0;
    'scan: while i < c.len() {
        for (w, n) in [("大后天", 3), ("后天", 2), ("明天", 1), ("今天", 0), ("昨天", -1), ("前天", -2)] {
            if at(i, w) {
                found.push(common::add_days(today, n));
                i += w.chars().count();
                continue 'scan;
            }
        }
        let (shift, mut j) = [("下下", 14), ("这", 0), ("本", 0), ("下", 7), ("上", -7)].iter().find(|(p, _)| at(i, p)).map_or((0, i), |(p, n)| (*n, i + p.chars().count()));
        if at(j, "个") {
            j += 1;
        }
        if let Some(word) = ["星期", "礼拜", "周"].iter().find(|w| at(j, w)) {
            let k = j + word.chars().count();
            if let Some(wd) = c.get(k).and_then(|x| "一二三四五六日天".find(*x)).map(|b| (b / 3).min(6) as i64) {
                found.push(common::add_days(&monday, shift + wd));
                i = k + 1;
                continue;
            }
        }
        if let (Some(a), la) = digits(i) {
            let k = i + la;
            if at(k, "月") {
                if let (Some(b), lb) = digits(k + 1) {
                    if let Some(d) = real(format!("{y}-{a:02}-{b:02}")) {
                        found.push(d);
                    }
                    i = k + 1 + lb + usize::from(at(k + 1 + lb, "日") || at(k + 1 + lb, "号"));
                    continue;
                }
            } else if at(k, "号") || at(k, "日") {
                if let Some(d) = real(format!("{y}-{m}-{a:02}")) {
                    found.push(d);
                }
                i = k + 1;
                continue;
            }
            i = k;
            continue;
        }
        i += 1;
    }
    (found.len() == 1).then(|| found.remove(0))
}

/// The date fields take the sentence's own date when Rust can read one.
pub fn fix_dates(module: &str, f: &mut QuickFill, sentence: &str, today: &str) {
    let Some(d) = date_in(sentence, today) else { return };
    match module {
        "finance" => f.day = Some(d),
        "todo" if f.start_date.is_none() => f.due_date = Some(d),
        "jobs" if f.status_id.is_some() => f.day = Some(d),
        "packing" => f.depart_on = Some(d),
        _ => {}
    }
}

/// What a sentence filled in (only the fields of its module; ids checked against the database).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickFill {
    // 收支
    pub flow: Option<String>,
    /// Cents.
    pub amount: Option<i64>,
    pub currency: Option<String>,
    pub day: Option<String>,
    pub kind_id: Option<i64>,
    pub note: Option<String>,
    // 任务
    pub title: Option<String>,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub group_id: Option<i64>,
    pub goal_id: Option<i64>,
    // 投递
    pub company: Option<String>,
    pub position: Option<String>,
    pub location: Option<String>,
    pub status_id: Option<i64>,
    // 书影 (title, kind_id above)
    pub creator: Option<String>,
    pub country: Option<String>,
    /// unmarked | todo | doing | done
    pub status: Option<String>,
    pub rating: Option<i64>,
    pub finished_year: Option<String>,
    // 新出行
    pub name: Option<String>,
    pub depart_on: Option<String>,
    pub template_id: Option<i64>,
}

/// By name: exact, else the one name that contains it (or it contains).
pub(crate) fn pick(name: Option<String>, list: &[(i64, String)]) -> Option<i64> {
    let n = name?.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    if let Some((id, _)) = list.iter().find(|(_, x)| x.to_lowercase() == n) {
        return Some(*id);
    }
    let near: Vec<i64> = list.iter().filter(|(_, x)| { let x = x.to_lowercase(); x.contains(&n) || n.contains(&x) }).map(|(id, _)| *id).collect();
    (near.len() == 1).then(|| near[0])
}

pub(crate) fn date(v: &Value, k: &str) -> Option<String> {
    field(v, k, 10).filter(|d| common::check_date(&Some(d.clone()), "").is_ok())
}

fn currency_code(s: &str) -> Option<&'static str> {
    let s = s.trim().to_uppercase();
    Some(match s.as_str() {
        "CNY" | "RMB" | "人民币" | "元" | "块" | "¥" => "CNY",
        "SGD" | "新币" | "新加坡元" | "S$" | "新元" => "SGD",
        "HKD" | "港币" | "港元" | "HK$" => "HKD",
        "USD" | "美元" | "美金" | "刀" | "US$" | "$" => "USD",
        _ => return None,
    })
}

/// Whether a sentence names a 分组 / 目标: two characters in a row of its name (emoji, spaces left out).
fn mentions(sentence: &str, name: &str) -> bool {
    let core: Vec<char> = name.chars().filter(|c| c.is_alphanumeric()).collect();
    match core.len() {
        0 => false,
        1 => sentence.contains(core[0]),
        _ => core.windows(2).any(|w| sentence.contains(&w.iter().collect::<String>())),
    }
}

fn names(list: impl IntoIterator<Item = (i64, String)>) -> Vec<(i64, String)> {
    list.into_iter().collect()
}

fn joined(list: &[(i64, String)]) -> String {
    if list.is_empty() { "（没有）".into() } else { list.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join("、") }
}

/// The module's fields for the prompt, and how to read the answer back (with the sentence).
pub(crate) fn quick_fields(c: &rusqlite::Connection, module: &str, today: &str) -> Result<(String, Box<dyn Fn(&Value, &str) -> QuickFill + Send + Sync>), String> {
    let text = |v: &Value, k: &str, n: usize| field(v, k, n);
    match module {
        "finance" => {
            let outs = names(kinds::list(c, crate::modules::finance::OUT_KINDS)?.into_iter().map(|k| (k.id, k.name)));
            let ins = names(kinds::list(c, crate::modules::finance::IN_KINDS)?.into_iter().map(|k| (k.id, k.name)));
            // no currency said: the one used most lately
            let usual: Option<String> = c
                .query_row("SELECT currency FROM fin_entries WHERE day >= date(?1, '-60 days') GROUP BY currency ORDER BY count(*) DESC LIMIT 1", [today], |r| r.get(0))
                .ok();
            let fields = format!(
                "字段：{{\"flow\": \"out（支出）或 in（收入）\", \"amount\": 金额数字（单位元）, \"currency\": \"CNY / SGD / HKD / USD（人民币、新币、港币、美元；没说就不写）\", \"day\": \"日期（没说就是今天）\", \"kind\": \"支出分类之一：{}；或收入分类之一：{}\", \"note\": \"备注（吃了什么、买了什么、在哪，简短）\"}}",
                joined(&outs),
                joined(&ins)
            );
            let today = today.to_string();
            Ok((
                fields,
                Box::new(move |v, _| {
                    let kind = text(v, "kind", 20);
                    let out_kind = pick(kind.clone(), &outs);
                    let in_kind = pick(kind, &ins);
                    let flow = match text(v, "flow", 10).as_deref() {
                        Some("in" | "收入") => "in",
                        Some("out" | "支出") => "out",
                        _ if in_kind.is_some() && out_kind.is_none() => "in",
                        _ => "out",
                    };
                    let amount = v.get("amount").and_then(|a| a.as_f64().or_else(|| a.as_str().and_then(|s| s.trim().trim_start_matches(['¥', '$']).parse().ok())));
                    QuickFill {
                        flow: Some(flow.into()),
                        amount: amount.filter(|a| a.is_finite() && *a > 0.0 && *a < 1e10).map(|a| (a * 100.0).round() as i64),
                        currency: text(v, "currency", 10).and_then(|s| currency_code(&s).map(String::from)).or(usual.clone()),
                        day: date(v, "day").or(Some(today.clone())),
                        kind_id: if flow == "in" { in_kind } else { out_kind },
                        note: text(v, "note", 200),
                        ..Default::default()
                    }
                }),
            ))
        }
        "todo" => {
            let groups = names(todo::groups(c)?.into_iter().map(|g| (g.id, g.name)));
            let goals: Vec<(i64, String, i64)> = todo::goals(c)?.into_iter().filter(|g| g.status == todo::GoalStatus::Active).map(|g| (g.id, g.name, g.group_id)).collect();
            let goal_names: Vec<(i64, String)> = goals.iter().map(|(id, n, _)| (*id, n.clone())).collect();
            let fields = format!(
                "字段：{{\"title\": \"任务标题（去掉时间词，简短）\", \"start_date\": \"开始日期（说了从哪天到哪天才写）\", \"due_date\": \"截止日期或要做的那天（只说了一天就写这里）\", \"group\": \"分组之一：{}（说到了才写）\", \"goal\": \"进行中的目标之一：{}（说到了才写）\"}}",
                joined(&groups),
                goals.iter().map(|(_, n, g)| format!("{n}（{}）", groups.iter().find(|x| x.0 == *g).map_or("", |x| x.1.as_str()))).collect::<Vec<_>>().join("、")
            );
            Ok((
                fields,
                Box::new(move |v, said| {
                    // a 分组 / 目标 only when the sentence names it (else the panel keeps where it was opened)
                    let named = |id: Option<i64>, list: &[(i64, String)]| id.filter(|id| list.iter().any(|(x, n)| x == id && mentions(said, n)));
                    let goal_id = named(pick(text(v, "goal", 60), &goal_names), &goal_names);
                    let group_id = goal_id.and_then(|g| goals.iter().find(|x| x.0 == g).map(|x| x.2)).or_else(|| named(pick(text(v, "group", 60), &groups), &groups));
                    let (start, due) = match (date(v, "start_date"), date(v, "due_date")) {
                        (Some(s), None) => (None, Some(s)), // one day said: that's the day it's due
                        (s, d) => (s.filter(|s| d.as_ref().is_some_and(|d| s < d)), d), // a start only before an end
                    };
                    QuickFill {
                        title: text(v, "title", 200),
                        start_date: start,
                        due_date: due,
                        group_id,
                        goal_id,
                        ..Default::default()
                    }
                }),
            ))
        }
        "jobs" => {
            let channels = names(kinds::list(c, crate::modules::jobs::CHANNELS)?.into_iter().map(|k| (k.id, k.name)));
            let statuses = names(crate::modules::jobs::statuses(c)?.into_iter().filter(|s| s.stage != crate::modules::jobs::Stage::Wish).map(|s| (s.id, s.name)));
            let fields = format!(
                "字段：{{\"company\": \"公司\", \"position\": \"岗位\", \"location\": \"城市\", \"channel\": \"渠道之一：{}（说了才写）\", \"status\": \"走到了哪一步，状态之一：{}（说投了就是第一个；只是想投就不写）\", \"day\": \"那一步的日期（没说就是今天）\"}}",
                joined(&channels),
                joined(&statuses)
            );
            let today = today.to_string();
            Ok((
                fields,
                Box::new(move |v, _| {
                    let status_id = pick(text(v, "status", 20), &statuses);
                    QuickFill {
                        company: text(v, "company", 100),
                        position: text(v, "position", 100),
                        location: text(v, "location", 50),
                        kind_id: pick(text(v, "channel", 20), &channels),
                        status_id,
                        day: status_id.map(|_| date(v, "day").unwrap_or_else(|| today.clone())),
                        ..Default::default()
                    }
                }),
            ))
        }
        "books" | "films" => {
            let kinds_list = names(kinds::list(c, module)?.into_iter().map(|k| (k.id, k.name)));
            let (verb, who) = if module == "books" { ("读", "作者") } else { ("看", "导演") };
            let fields = format!(
                "字段：{{\"title\": \"名称（不要书名号）\", \"creator\": \"{who}（你确定才写，常见中文译名）\", \"country\": \"国家（你确定才写，中文）\", \"kind\": \"类型之一：{}\", \"status\": \"todo（想{verb}）/ doing（在{verb}）/ done（{verb}完了）\", \"rating\": 星级 1～5 的整数（说了几星、几分才写）, \"year\": \"{verb}完的年份（{verb}完了没说哪年就是今年）\"}}",
                joined(&kinds_list)
            );
            let year = today[..4].to_string();
            Ok((
                fields,
                Box::new(move |v, _| {
                    let status = text(v, "status", 10).and_then(|s| match s.as_str() {
                        "todo" | "想读" | "想看" => Some("todo"),
                        "doing" | "在读" | "在看" => Some("doing"),
                        "done" | "已读" | "已看" | "读完" | "看完" => Some("done"),
                        _ => None,
                    });
                    let finished = field(v, "year", 4).filter(|y| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()) && ("1900".."2101").contains(&y.as_str()));
                    QuickFill {
                        title: text(v, "title", 200).map(|t| t.trim_matches(|c| c == '《' || c == '》').to_string()),
                        creator: text(v, "creator", 100),
                        country: text(v, "country", 30),
                        kind_id: pick(text(v, "kind", 20), &kinds_list),
                        status: status.map(String::from),
                        rating: v.get("rating").and_then(|r| r.as_i64().or_else(|| r.as_f64().map(|f| f.round() as i64))).filter(|r| (1..=5).contains(r)),
                        finished_year: if matches!(status, Some("todo" | "doing")) { None } else { finished.or_else(|| (status == Some("done")).then(|| year.clone())) },
                        ..Default::default()
                    }
                }),
            ))
        }
        "packing" => {
            let templates = names(packing::templates(c)?.into_iter().map(|t| (t.id, t.name)));
            let fields = format!(
                "字段：{{\"name\": \"出行名称（目的地加天数，比如 东京五天）\", \"depart_on\": \"出发日期\", \"template\": \"最合适的场景模板之一：{}（都不合适就不写）\"}}",
                joined(&templates)
            );
            Ok((
                fields,
                Box::new(move |v, _| QuickFill { name: text(v, "name", 100), depart_on: date(v, "depart_on"), template_id: pick(text(v, "template", 60), &templates), ..Default::default() }),
            ))
        }
        _ => Err(format!("这里不能一句话填好：{module}")),
    }
}

async fn quick_fill(app: &AppHandle, module: &str, sentence: &str) -> Result<QuickFill, String> {
    let sentence = sentence.trim();
    if sentence.is_empty() {
        return Err("先写一句话".into());
    }
    let p = &app.state::<AppState>().picky;
    let route = route(p).await?;
    check(p, &route, module)?;
    let (system, read, today) = app.state::<AppState>().with_db(|c| {
        let today = todo::today(c)?;
        let (fields, read) = quick_fields(c, module, &today)?;
        Ok((format!("{QUICK}\n{}\n{fields}", calendar(&today)), read, today))
    })?;
    let reply = ask(&route, &system, &sentence.chars().take(300).collect::<String>(), 512).await?;
    let v = json_in(&reply).filter(Value::is_object).ok_or("没看懂这句话，换个说法试试")?;
    let mut f = read(&v, sentence);
    fix_dates(module, &mut f, sentence, &today);
    // 书影: the creator / country / 类型 from 书单 / 影单检索 (when on and found)
    if let (true, Some(title)) = (module == "books" || module == "films", f.title.clone()) {
        if let Some(w) = super::skills::work(app, module, &title, f.creator.as_deref()).await {
            let names: Vec<(i64, String)> = app.state::<AppState>().with_db(|c| Ok(kinds::list(c, module)?.into_iter().map(|k| (k.id, k.name)).collect()))?;
            f.creator = w.creator.or(f.creator);
            f.country = w.country.or(f.country);
            f.kind_id = super::skills::works::kind_id(&names, w.kind.as_deref()).or(f.kind_id);
        }
    }
    Ok(f)
}

#[tauri::command]
pub async fn picky_quick_fill(app: AppHandle, module: String, sentence: String) -> Result<QuickFill, String> {
    quick_fill(&app, &module, &sentence).await
}

// ---------- mock model (dev builds, tests; mockIpc.ts mirrors it) ----------

/// What the mock model answers to an assist prompt.
pub fn mock_reply(system: &str, user: &str) -> String {
    let first_kind = |s: &str| s.split("里选：").nth(1).or_else(|| s.split("里选一个：").nth(1)).and_then(|r| r.split(['、', '；', '"']).next()).unwrap_or("").to_string();
    if system.contains("拆成各个字段") {
        return mock_quick(system, user);
    }
    if system.contains("润色") {
        let text = user.split("原文：\n").nth(1).unwrap_or(user);
        format!("{}（模拟润色）", text.replace('很', "非常"))
    } else if system.contains("补全信息") {
        format!("{{\"creator\": \"模拟作者\", \"country\": \"美国\", \"kind\": \"{}\"}}", first_kind(system))
    } else if system.contains("招聘信息") {
        "```json\n{\"company\": \"模拟公司\", \"position\": \"产品经理\", \"location\": \"上海\"}\n```".into()
    } else if system.contains("子任务") {
        "[\"列出要做的步骤\", \"先做第一步\", \"检查一遍\"]".into()
    } else if system.contains("拆成待办") {
        // one per line of the note (its list items or sentences), up to 5
        let note = user.split("笔记：\n").nth(1).unwrap_or(user).split("\n\n已经转成的待办").next().unwrap_or("");
        let items: Vec<String> = note.split(['\n', '；', '。']).map(|l| l.trim().trim_start_matches(|c: char| c == '-' || c.is_ascii_digit() || c == '.').trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with('#')).take(5).collect();
        serde_json::to_string(&items).unwrap_or_default()
    } else if system.contains("出门") {
        let k = first_kind(system);
        format!("[{{\"name\": \"充电宝\", \"kind\": \"{k}\"}}, {{\"name\": \"雨伞\", \"kind\": \"{k}\"}}, {{\"name\": \"常用药\", \"kind\": \"{k}\"}}]")
    } else {
        "（模拟）".into()
    }
}

/// The mock model's 一句话填好: a few plain rules (a number is the amount, 下周X a date…).
fn mock_quick(system: &str, sentence: &str) -> String {
    let after = |key: &str| system.split(key).nth(1).map(|r| r.split(['、', '；', '（', '"']).next().unwrap_or("").to_string()).unwrap_or_default();
    let next_week = |day: &str| system.split(&format!("下周{day} ")).nth(1).map(|r| r.chars().take(10).collect::<String>());
    let date = ["一", "二", "三", "四", "五", "六", "日"].iter().find(|d| sentence.contains(&format!("下周{d}"))).and_then(|d| next_week(d));
    let number: String = sentence.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let mut v = serde_json::Map::new();
    let mut put = |k: &str, x: Value| {
        v.insert(k.into(), x);
    };
    if system.contains("支出分类") {
        put("flow", "out".into());
        if let Ok(n) = number.parse::<f64>() {
            put("amount", n.into());
        }
        for (word, code) in [("新币", "SGD"), ("港币", "HKD"), ("美元", "USD"), ("人民币", "CNY")] {
            if sentence.contains(word) {
                put("currency", code.into());
            }
        }
        put("kind", after("支出分类之一：").into());
        put("note", sentence.split(|c: char| c.is_ascii_digit() || c.is_whitespace()).next().unwrap_or("").into());
    } else if system.contains("截止日期") {
        put("title", sentence.trim_start_matches(|c: char| "下周一二三四五六日前".contains(c)).trim().into());
        if let Some(d) = date {
            put("due_date", d.into());
        }
        put("group", after("分组之一：").into());
    } else if system.contains("渠道之一") {
        let rest = sentence.split("投了").nth(1).unwrap_or(sentence);
        let mut parts = rest.split(['，', ',']);
        let head = parts.next().unwrap_or("");
        let (company, position) = head.split_once('的').unwrap_or((head, ""));
        put("company", company.trim().into());
        put("position", position.trim().into());
        put("location", parts.next().unwrap_or("").trim().into());
        put("channel", parts.next().unwrap_or("").trim().into());
        put("status", after("状态之一：").into());
    } else if system.contains("出发日期") {
        put("name", sentence.trim_start_matches(|c: char| "下周一二三四五六日去".contains(c)).trim().into());
        if let Some(d) = date {
            put("depart_on", d.into());
        }
        put("template", after("模板之一：").into());
    } else {
        let title = sentence.split(['，', ',']).next().unwrap_or(sentence).trim_start_matches("看完了").trim_start_matches("读完了").trim();
        put("title", title.into());
        if sentence.contains('完') {
            put("status", "done".into());
        }
        for (word, n) in [("五星", 5), ("四星", 4), ("三星", 3), ("两星", 2), ("一星", 1)] {
            if sentence.contains(word) {
                put("rating", n.into());
            }
        }
    }
    Value::Object(v).to_string()
}

// ---------- commands ----------

#[tauri::command]
pub async fn picky_polish(app: AppHandle, module: String, text: String, request: String) -> Result<String, String> {
    polish(&app, &module, &text, &request).await
}

#[tauri::command]
pub async fn picky_fill_work(app: AppHandle, module: String, title: String) -> Result<WorkFill, String> {
    fill_work(&app, &module, &title).await
}

#[tauri::command]
pub async fn picky_fill_job(app: AppHandle, jd: String) -> Result<JobFill, String> {
    fill_job(&app, &jd).await
}

#[tauri::command]
pub async fn picky_split_task(app: AppHandle, task_id: i64) -> Result<Vec<String>, String> {
    split_task(&app, task_id).await
}

#[tauri::command]
pub async fn picky_split_note(app: AppHandle, note_id: i64) -> Result<Vec<String>, String> {
    split_note(&app, note_id).await
}

#[tauri::command]
pub async fn picky_trip_missing(app: AppHandle, trip_id: i64) -> Result<Vec<ItemIdea>, String> {
    trip_missing(&app, trip_id).await
}

/// Remembers the local model picked (select() calls it), for starting it when a help needs one.
pub fn remember_local(conn: &rusqlite::Connection, key: &str) {
    let _ = pref_set(conn, LAST_LOCAL, key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;

    #[test]
    fn replies_are_read_through_fences_and_chatter() {
        assert_eq!(json_in("好的：\n```json\n{\"a\": 1}\n```\n希望有用"), Some(serde_json::json!({ "a": 1 })));
        assert_eq!(json_in("[\"x\", \"y\"] 以上"), Some(serde_json::json!(["x", "y"])));
        assert_eq!(json_in("{不是 json} 然后 {\"b\": 2}"), Some(serde_json::json!({ "b": 2 })));
        assert_eq!(json_in("没有"), None);
        assert_eq!(clean_text("```\n改好的文字\n```"), "改好的文字");
        assert_eq!(clean_text("改好的：「今天很好。」"), "今天很好。");
        assert_eq!(clean_text("“他说「好」”"), "他说「好」");
        assert_eq!(clean_text("「一」和「二」"), "「一」和「二」"); // not one quoted block
    }

    #[test]
    fn fills_keep_only_what_makes_sense() {
        let kinds = vec![(1, "虚构".to_string()), (2, "非虚构".to_string())];
        let w = parse_work("{\"creator\": \"弗兰克·赫伯特\", \"country\": \"美国\", \"kind\": \"虚构\"}", &kinds).unwrap();
        assert_eq!(w, WorkFill { creator: Some("弗兰克·赫伯特".into()), country: Some("美国".into()), kind_id: Some(1) });
        let w = parse_work("{\"creator\": \"\", \"country\": \"未知\", \"kind\": \"科幻\"}", &kinds).unwrap();
        assert_eq!(w, WorkFill::default());
        assert!(parse_work("我不知道", &kinds).is_err());
        let j = parse_job("{\"company\": \"字节跳动\", \"position\": \"产品经理\", \"location\": \"\"}").unwrap();
        assert_eq!((j.company.as_deref(), j.position.as_deref(), j.location), (Some("字节跳动"), Some("产品经理"), None));
        let l = parse_list("[\"1. 查资料\", \"写提纲\", \"写提纲\", \"已经有的\", {\"title\": \"交稿\"}]", 8, &["已经有的".into()]).unwrap();
        assert_eq!(l, ["查资料", "写提纲", "交稿"]);
        // what Qwen3.5 9B wrote once: one bracketed item per line
        assert_eq!(parse_list("[确定选题]\n[收集资料]\n\n- 写初稿", 8, &[]).unwrap(), ["确定选题", "收集资料", "写初稿"]);
        assert!(parse_list("[]", 8, &[]).is_err());
        let items = parse_items("[{\"name\": \"护照\", \"kind\": \"重要物品\"}, {\"name\": \"牙刷\", \"kind\": \"护理\"}, \"雨伞\", {\"name\": \"雨伞\"}]", &[(5, "重要物品".into())], &["牙刷".into()]).unwrap();
        assert_eq!(items, [ItemIdea { name: "护照".into(), kind_id: Some(5) }, ItemIdea { name: "雨伞".into(), kind_id: None }]);
    }

    #[test]
    fn the_mock_model_answers_every_help() {
        let ask = |system: &str, user: &str| mock_reply(&format!("{MARK}{system}"), user);
        assert_eq!(clean_text(&ask(POLISH, "这次的要求：通顺\n\n原文：\n很好")), "非常好（模拟润色）");
        let kinds = vec![(1, "虚构".to_string())];
        assert_eq!(parse_work(&ask(&work_prompt("books", &["虚构".into()]), "名称：沙丘"), &kinds).unwrap().kind_id, Some(1));
        assert_eq!(parse_job(&ask(JOB, "招聘")).unwrap().company.as_deref(), Some("模拟公司"));
        assert_eq!(parse_list(&ask(SPLIT, "任务：写论文"), 8, &[]).unwrap().len(), 3);
        let todos = parse_list(&ask(NOTE_SPLIT, "笔记：\n周末要做的 #生活\n- 交水电费\n- 给房东打电话\n\n已经转成的待办：交水电费"), 12, &["交水电费".into()]).unwrap();
        assert_eq!(todos, ["周末要做的 #生活", "给房东打电话"]);
        let items = parse_items(&ask(&trip_prompt(&["随身".into()]), "出行：东京"), &[(9, "随身".into())], &[]).unwrap();
        assert_eq!((items.len(), items[0].kind_id), (3, Some(9)));
    }

    /// Every help on the real local model (8080), thinking off. Opt-in:
    ///   POMELO_REAL_LLM=1 [POMELO_MODEL=qwen3.5-9b] cargo test real_assist -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_assist() {
        if std::env::var("POMELO_REAL_LLM").as_deref() != Ok("1") {
            return;
        }
        let model = std::env::var("POMELO_MODEL").unwrap_or_else(|_| "qwen3.5-9b".into());
        let route = Route { target: crate::picky::provider::Target::Local { model }, model: "local:x".into(), name: "X".into(), window: 32768, think: crate::picky::cloud::Think::Switch };
        let run = |label: &str, system: &str, user: &str| {
            let t = std::time::Instant::now();
            let out = tauri::async_runtime::block_on(ask(&route, system, user, 1024)).unwrap();
            println!("\n===== {label} ({:.1}s)\n{out}", t.elapsed().as_secs_f64());
            out
        };
        let text = "今天开会讨论了很多东西，但是其实没有什么结论，大家都在说自己的想法，下次要早点准备一下资料，不然又浪费时间";
        println!("--> {}", clean_text(&run("润色：更简洁", POLISH, &format!("这次的要求：更简洁\n\n原文：\n{text}"))));
        println!("--> {}", clean_text(&run("润色：默认", POLISH, &format!("这次的要求：改得更通顺、自然，意思不变\n\n原文：\n{text}"))));
        let books: Vec<(i64, String)> = ["虚构", "非虚构", "漫画", "其他"].iter().enumerate().map(|(i, n)| (i as i64, n.to_string())).collect();
        let names: Vec<String> = books.iter().map(|b| b.1.clone()).collect();
        println!("--> {:?}", parse_work(&run("书：沙丘", &work_prompt("books", &names), "名称：沙丘"), &books));
        println!("--> {:?}", parse_work(&run("书：人类简史", &work_prompt("books", &names), "名称：人类简史"), &books));
        let films: Vec<(i64, String)> = ["电影", "剧集", "动漫"].iter().enumerate().map(|(i, n)| (i as i64, n.to_string())).collect();
        println!("--> {:?}", parse_work(&run("影：星际穿越", &work_prompt("films", &films.iter().map(|b| b.1.clone()).collect::<Vec<_>>()), "名称：星际穿越"), &films));
        let jd = "【字节跳动】产品经理-抖音电商（北京/上海）\n岗位职责：\n1. 负责抖音电商商家端产品规划\n2. 分析用户需求，推动产品迭代\n任职要求：本科及以上，2027 届毕业生";
        println!("--> {:?}", parse_job(&run("JD", JOB, jd)));
        println!("--> {:?}", parse_list(&run("拆子任务", SPLIT, "任务：写毕业论文开题报告\n内容：下周五前交给导师，要有研究背景、方法和时间安排\n截止：2026-10-23"), 8, &[]));
        let packing: Vec<(i64, String)> = ["重要物品", "随身", "护理", "兴趣"].iter().enumerate().map(|(i, n)| (i as i64, n.to_string())).collect();
        println!(
            "--> {:?}",
            parse_items(
                &run("出门漏带", &trip_prompt(&packing.iter().map(|b| b.1.clone()).collect::<Vec<_>>()), "出行：东京五天\n出发：2026-11-01（今天 2026-10-12）\n这次的清单：身份证、手机、充电器、牙刷、换洗衣服\n模板和以前的出行里有、这次没有的：护照（2 次）、转换插头（2 次）、画本（1 次）"),
                &packing,
                &["身份证".into(), "手机".into(), "充电器".into(), "牙刷".into(), "换洗衣服".into()]
            )
        );
    }

    #[test]
    fn a_stopped_local_model_starts_and_cloud_rules_hold() {
        use crate::paths::DataPaths;
        let dir = crate::db::tests::temp_dir("picky-assist");
        let p = Picky::init(&DataPaths::new(dir.to_path_buf()), &dir, Mode::Mock);
        p.with_db(|c| Ok(remember_local(c, "ministral14b"))).unwrap();
        assert!(p.runtime.ready_model().is_none());
        let r = tauri::async_runtime::block_on(route(&p)).unwrap();
        assert_eq!(r.model, "local:ministral14b"); // the one picked last
        assert!(check(&p, &r, "finance").is_ok()); // local: everything
        let cloud = Route { model: "cloud:deepseek-flash".into(), ..r };
        assert_eq!(check(&p, &cloud, "finance").unwrap_err(), "选了云端模型：钱来钱往不发给云端。换成本地模型再用。");
        assert!(check(&p, &cloud, "jobs").is_ok());
        assert!(check(&p, &cloud, "vault").is_err());
    }

    const TODAY: &str = "2026-10-12";

    /// One sentence through the mock model, read back like the command does.
    fn quick(c: &rusqlite::Connection, module: &str, sentence: &str) -> QuickFill {
        let (fields, read) = quick_fields(c, module, TODAY).unwrap();
        let system = format!("{MARK}{QUICK}\n{}\n{fields}", calendar(TODAY));
        let mut f = read(&json_in(&mock_reply(&system, sentence)).unwrap(), sentence);
        fix_dates(module, &mut f, sentence, TODAY);
        f
    }

    #[test]
    fn date_words_are_read_from_today() {
        let d = |s: &str| date_in(s, TODAY);
        assert_eq!(d("下周五前交开题报告").as_deref(), Some("2026-10-23"));
        assert_eq!(d("这周五交").as_deref(), Some("2026-10-16"));
        assert_eq!(d("周日去爬山").as_deref(), Some("2026-10-18"));
        assert_eq!(d("上周三面试了").as_deref(), Some("2026-10-07"));
        assert_eq!(d("下个星期一开会").as_deref(), Some("2026-10-19"));
        assert_eq!(d("下下周二").as_deref(), Some("2026-10-27"));
        assert_eq!(d("大后天出发").as_deref(), Some("2026-10-15"));
        assert_eq!(d("昨天打车 28").as_deref(), Some("2026-10-11"));
        assert_eq!(d("11月3日去东京").as_deref(), Some("2026-11-03"));
        assert_eq!(d("20号交房租").as_deref(), Some("2026-10-20"));
        assert_eq!(d("午饭 35 新币"), None); // a number isn't a date
        assert_eq!(d("从明天到下周五"), None); // two dates: the model decides
        assert_eq!(d("2月30号"), None);
    }

    #[test]
    fn the_calendar_names_the_days() {
        let cal = calendar(TODAY);
        assert!(cal.starts_with("今天是 2026-10-12（周一），昨天 2026-10-11，明天 2026-10-13，后天 2026-10-14。这周一 2026-10-12、"), "{cal}");
        assert!(cal.contains("下周一 2026-10-19、下周二 2026-10-20、下周三 2026-10-21、下周四 2026-10-22、下周五 2026-10-23、下周六 2026-10-24、下周日 2026-10-25。"), "{cal}");
        assert!(cal.contains("上周三 2026-10-07"), "{cal}");
    }

    #[test]
    fn one_sentence_fills_each_new_panel() {
        let c = memory_db();
        let id = |module: &str, name: &str| kinds::list(&c, module).unwrap().into_iter().find(|k| k.name == name).unwrap().id;
        // 收支
        let f = quick(&c, "finance", "午饭 35 新币");
        assert_eq!((f.flow.as_deref(), f.amount, f.currency.as_deref(), f.day.as_deref(), f.kind_id, f.note.as_deref()), (Some("out"), Some(3500), Some("SGD"), Some(TODAY), Some(id("fin_out", "餐饮")), Some("午饭")));
        // 任务: 下周五 from the calendar; a 分组 the sentence doesn't name stays out
        let g = todo::create_group(&c, "🖥️ 学业").unwrap().id;
        let f = quick(&c, "todo", "下周五前交开题报告");
        assert_eq!((f.title.as_deref(), f.due_date.as_deref(), f.group_id), (Some("交开题报告"), Some("2026-10-23"), None));
        let f = quick(&c, "todo", "下周五前交学业报告");
        assert_eq!(f.group_id, Some(g));
        // 投递: 渠道 and 状态 by name, the step today
        let f = quick(&c, "jobs", "投了某公司的产品经理，北京，内推");
        let applied = crate::modules::jobs::statuses(&c).unwrap().into_iter().find(|s| s.name == "已投递").unwrap().id;
        assert_eq!((f.company.as_deref(), f.position.as_deref(), f.location.as_deref()), (Some("某公司"), Some("产品经理"), Some("北京")));
        assert_eq!((f.kind_id, f.status_id, f.day.as_deref()), (Some(id("job_channel", "内推")), Some(applied), Some(TODAY)));
        // 书影: done without a year → this year
        let f = quick(&c, "films", "看完了沙丘，四星");
        assert_eq!((f.title.as_deref(), f.status.as_deref(), f.rating, f.finished_year.as_deref()), (Some("沙丘"), Some("done"), Some(4), Some("2026")));
        // 新出行: the template by name
        let tp = packing::create_template(&c, serde_json::from_value(serde_json::json!({ "name": "出国" })).unwrap()).unwrap().id;
        let f = quick(&c, "packing", "下周六去东京五天");
        assert_eq!((f.name.as_deref(), f.depart_on.as_deref(), f.template_id), (Some("东京五天"), Some("2026-10-24"), Some(tp)));
        assert!(quick_fields(&c, "notes", TODAY).is_err());
    }

    #[test]
    fn filled_values_are_checked() {
        let c = memory_db();
        let (_, read) = quick_fields(&c, "finance", TODAY).unwrap();
        let f = read(&serde_json::json!({ "amount": "12.5", "currency": "港币", "day": "2026-02-30", "kind": "不存在", "flow": "支出" }), "");
        assert_eq!((f.amount, f.currency.as_deref(), f.day.as_deref(), f.kind_id), (Some(1250), Some("HKD"), Some(TODAY), None));
        assert_eq!(read(&serde_json::json!({ "amount": -3 }), "").amount, None);
        assert_eq!(read(&serde_json::json!({ "kind": "津贴" }), "").flow.as_deref(), Some("in")); // an income 分类 means income
        let (_, read) = quick_fields(&c, "books", TODAY).unwrap();
        let f = read(&serde_json::json!({ "title": "《沙丘》", "rating": 9, "year": "公元前", "status": "想读" }), "");
        assert_eq!((f.title.as_deref(), f.rating, f.finished_year, f.status.as_deref()), (Some("沙丘"), None, None, Some("todo")));
        assert_eq!(read(&serde_json::json!({ "status": "todo", "year": "2026" }), "").finished_year, None); // a year only once finished
        let (_, read) = quick_fields(&c, "todo", TODAY).unwrap();
        let f = read(&serde_json::json!({ "title": "x", "start_date": "2026-11-01", "due_date": "2026-10-20" }), "");
        assert_eq!((f.start_date, f.due_date.as_deref()), (None, Some("2026-10-20"))); // a start after the end is dropped
        let f = read(&serde_json::json!({ "title": "x", "start_date": "2026-10-13" }), "");
        assert_eq!((f.start_date, f.due_date.as_deref()), (None, Some("2026-10-13"))); // one day: the day it's due
        assert_eq!(read(&serde_json::json!({ "start_date": "2026-10-13", "due_date": "2026-10-13" }), "").start_date, None);
        let g = todo::create_group(&c, "学业").unwrap().id;
        let o = todo::create_goal(&c, serde_json::from_value(serde_json::json!({ "groupId": g, "name": "毕业论文" })).unwrap()).unwrap().id;
        let (_, read) = quick_fields(&c, "todo", TODAY).unwrap();
        let guess = serde_json::json!({ "title": "x", "group": "学业", "goal": "毕业论文" });
        assert_eq!((read(&guess, "交开题报告").group_id, read(&guess, "交开题报告").goal_id), (None, None)); // guessed: not filled
        assert_eq!((read(&guess, "论文开题报告").group_id, read(&guess, "论文开题报告").goal_id), (Some(g), Some(o)));
        assert!(mentions("个人网站写首页", "📌 个人网站上线") && !mentions("背单词", "学日语"));
        assert_eq!(pick(Some("餐".into()), &[(1, "餐饮".into()), (2, "交通".into())]), Some(1));
        assert_eq!(pick(Some("其".into()), &[(1, "其他".into()), (2, "其它".into())]), None); // ambiguous
    }

    /// 一句话填好 on the real local model (8080). Opt-in like real_assist:
    ///   POMELO_REAL_LLM=1 cargo test real_quick -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_quick() {
        if std::env::var("POMELO_REAL_LLM").as_deref() != Ok("1") {
            return;
        }
        let model = std::env::var("POMELO_MODEL").unwrap_or_else(|_| "qwen3.5-9b".into());
        let route = Route { target: crate::picky::provider::Target::Local { model }, model: "local:x".into(), name: "X".into(), window: 32768, think: crate::picky::cloud::Think::Switch };
        let c = memory_db();
        let g = todo::create_group(&c, "学业").unwrap().id;
        todo::create_group(&c, "个人项目").unwrap();
        todo::create_goal(&c, serde_json::from_value(serde_json::json!({ "groupId": g, "name": "毕业论文" })).unwrap()).unwrap();
        for n in ["国内游 3～7 天", "出国", "周末短途"] {
            packing::create_template(&c, serde_json::from_value(serde_json::json!({ "name": n })).unwrap()).unwrap();
        }
        let cases = [
            ("finance", "午饭 35 新币"),
            ("finance", "昨天打车回家花了 28 块"),
            ("finance", "这个月奖学金到账 2000 新币"),
            ("todo", "下周五前交开题报告"),
            ("todo", "明天给导师发邮件约时间"),
            ("todo", "毕业论文这周四前改完第二章"),
            ("jobs", "投了字节跳动的产品经理，北京，内推"),
            ("jobs", "腾讯后台开发上周三面试了"),
            ("films", "看完了沙丘，四星"),
            ("books", "想读人类简史"),
            ("packing", "下周六去东京五天"),
        ];
        for (module, sentence) in cases {
            let (fields, read) = quick_fields(&c, module, TODAY).unwrap();
            let system = format!("{QUICK}\n{}\n{fields}", calendar(TODAY));
            let t = std::time::Instant::now();
            let reply = tauri::async_runtime::block_on(ask(&route, &system, sentence, 512)).unwrap();
            let got = json_in(&reply).filter(Value::is_object).map(|v| {
                let mut f = read(&v, sentence);
                fix_dates(module, &mut f, sentence, TODAY);
                f
            });
            println!("\n== {module}: {sentence} ({:.1}s)\n{}\n--> {got:?}", t.elapsed().as_secs_f64(), reply.trim());
        }
    }

    #[test]
    fn picked_subtasks_and_items_are_added_in_one_go() {
        let c = memory_db();
        let today = todo::today(&c).unwrap();
        let g = todo::create_group(&c, "学习").unwrap().id;
        let parent = todo::create_task_on(&c, serde_json::from_value(serde_json::json!({ "groupId": g, "title": "写论文" })).unwrap(), &today).unwrap().id;
        let made = todo::create_subtasks(&c, parent, &["查资料".into(), "写提纲".into()], &today).unwrap();
        assert!(made.iter().all(|t| t.parent_id == Some(parent)));
        let kids: Vec<String> = todo::tasks(&c, Some(g), None).unwrap().into_iter().filter(|t| t.parent_id == Some(parent)).map(|t| t.title).collect();
        assert_eq!(kids, ["查资料", "写提纲"]);
        assert!(todo::create_subtasks(&c, parent, &[], &today).is_err());
        // a bad one stops the whole step
        assert!(todo::create_subtasks(&c, parent, &["好的".into(), "  ".into()], &today).is_err());
        assert_eq!(todo::tasks(&c, Some(g), None).unwrap().len(), 3);

        let trip = packing::create_trip(&c, serde_json::from_value(serde_json::json!({ "name": "东京" })).unwrap(), None).unwrap().id;
        let items: Vec<packing::ItemPatch> = ["充电宝", "雨伞"].iter().map(|n| serde_json::from_value(serde_json::json!({ "name": n })).unwrap()).collect();
        assert_eq!(packing::create_items(&c, packing::List::Trip, trip, items).unwrap().len(), 2);
        assert_eq!(packing::items(&c, packing::List::Trip, trip).unwrap().iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["充电宝", "雨伞"]);
    }
}
