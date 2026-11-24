//! A write tool's arguments → what to propose (`Op`s). Read like 「一句话填好」 (assist.rs): names
//! matched to existing 分类 / 分组 / 状态 / 模板, dates worked out by Rust. An edit names its record
//! with words (`fit`): one fits → it; several → up to `MAX` for the user to pick (newest / best
//! first); none → an error telling the model to ask.

use super::ops::{Op, Rewrite};
use super::*;
use crate::modules::common::{add_days, err, norm};
use crate::modules::{articles, books, drawing, films, finance, jobs, kinds, notes, packing, todo};
use crate::youzi::assist::{self, QuickFill};
use rusqlite::Connection;
use serde_json::{json, Value};

/// Candidates listed for the user to pick from.
pub const MAX: usize = 5;

/// What one call proposes: one op, or several candidates; `extra`: lines the record's own don't
/// show (an edited trip's new and packed items), per op; `note`: told to the model.
#[derive(Debug, Default)]
pub struct Plan {
    pub ops: Vec<(Op, Vec<Field>)>,
    /// Candidates beyond the listed ones.
    pub more: usize,
    pub note: Option<String>,
}

fn one(op: Op) -> Result<Plan, String> {
    Ok(Plan { ops: vec![(op, vec![])], ..Default::default() })
}

fn some(ops: Vec<Op>) -> Plan {
    let more = ops.len().saturating_sub(MAX);
    Plan { ops: ops.into_iter().take(MAX).map(|o| (o, vec![])).collect(), more, note: None }
}

fn text(a: &Value, k: &str, max: usize) -> Option<String> {
    assist::field(a, k, max)
}

/// Yuan (a number or 「¥35」) → cents.
fn cents(a: &Value, k: &str) -> Option<i64> {
    let v = a.get(k)?;
    let y = v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().trim_start_matches(['¥', '$']).replace(',', "").parse().ok()))?;
    (y.is_finite() && y.abs() < 1e10).then(|| (y * 100.0).round() as i64)
}

fn list(a: &Value, k: &str) -> Vec<String> {
    match a.get(k) {
        Some(Value::Array(v)) => v.iter().filter_map(Value::as_str).map(|s| s.trim().chars().take(100).collect::<String>()).filter(|s| !s.is_empty()).collect(),
        Some(Value::String(s)) => s.split(['、', '，', ',']).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
        _ => vec![],
    }
}

/// The dates the model gave for `keys`; when the question names one date and the model gave exactly
/// one of them, that one is Rust's reading of it (small models get 「下周五」 wrong) — unless the model
/// gave today: in 「午饭 35，昨天打车 20」 the 午饭 is today's (tried on the real model).
fn dates(a: &Value, keys: &[&str], question: &str, today: &str) -> Vec<Option<String>> {
    let mut v: Vec<Option<String>> = keys.iter().map(|k| assist::date(a, k)).collect();
    let given: Vec<usize> = keys.iter().enumerate().filter(|(_, k)| a.get(**k).is_some_and(Value::is_string)).map(|(i, _)| i).collect();
    if let (Some(d), [i]) = (assist::date_in(question, today), given.as_slice()) {
        if v[*i].as_deref() != Some(today) {
            v[*i] = Some(d);
        }
    }
    v
}

fn quick(c: &Connection, module: &str, a: &Value, question: &str, today: &str) -> Result<QuickFill, String> {
    let (_, read) = assist::quick_fields(c, module, today, false)?;
    Ok(read(a, question))
}

/// The items `query` means: whole-name matches if any; else those containing every word; else those
/// sharing most of its character pairs (best first). Otherwise in the given order.
pub fn fit<T>(items: Vec<T>, name: impl Fn(&T) -> String, query: &str) -> Vec<T> {
    let q = norm(query);
    if q.is_empty() {
        return vec![];
    }
    let named: Vec<(T, String)> = items.into_iter().map(|t| { let n = norm(&name(&t)); (t, n) }).collect();
    if named.iter().any(|(_, n)| *n == q) {
        return named.into_iter().filter(|(_, n)| *n == q).map(|(t, _)| t).collect();
    }
    let words: Vec<String> = query.split(|c: char| c.is_whitespace() || "，,、;；".contains(c)).map(norm).filter(|w| !w.is_empty()).collect();
    let all = |n: &str| words.iter().all(|w| n.contains(w.as_str()));
    if named.iter().any(|(_, n)| all(n)) {
        return named.into_iter().filter(|(_, n)| all(n)).map(|(t, _)| t).collect();
    }
    let chars: Vec<char> = q.chars().collect();
    let pairs: Vec<String> = chars.windows(2).map(|w| w.iter().collect()).collect();
    if pairs.is_empty() {
        return vec![];
    }
    let mut scored: Vec<(f32, usize, T)> = named
        .into_iter()
        .enumerate()
        .map(|(i, (t, n))| (pairs.iter().filter(|p| n.contains(p.as_str())).count() as f32 / pairs.len() as f32, i, t))
        .filter(|(s, _, _)| *s >= 0.6)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, t)| t).collect()
}

pub fn plan(c: &Connection, tool: &str, a: &Value, question: &str, today: &str) -> Result<Plan, String> {
    match tool {
        ADD_ENTRY => add_entry(c, a, question, today),
        EDIT_ENTRY => edit_entry(c, a, question, today),
        SET_BALANCE => set_balance(c, a, question, today),
        ADD_TASK => add_task(c, a, question, today),
        EDIT_TASK => edit_task(c, a, question, today),
        ADD_JOB => add_job(c, a, question, today),
        EDIT_JOB => edit_job(c, a, question, today),
        ADD_WORK => add_work(c, a, question, today),
        EDIT_WORK => edit_work(c, a),
        FINISH_LESSON => finish_lesson(c, a),
        ADD_TRIP => add_trip(c, a, question, today),
        EDIT_TRIP => edit_trip(c, a, today),
        ADD_NOTE => add_note(a),
        EDIT_NOTE => edit_note(c, a),
        ADD_ARTICLE => add_article(a),
        EDIT_ARTICLE => edit_article(c, a),
        _ => Err("没有这个工具".into()),
    }
}

// ---------- 钱来钱往 ----------

fn add_entry(c: &Connection, a: &Value, question: &str, today: &str) -> Result<Plan, String> {
    let mut f = quick(c, "finance", a, question, today)?;
    if let Some(d) = dates(a, &["day"], question, today).remove(0) {
        f.day = Some(d);
    }
    let amount = f.amount.ok_or("没说金额")?;
    // none said or used lately: like the 收支 panel, the first open account's, else CNY
    let currency = match f.currency {
        Some(c) => c,
        None => finance::accounts(c)?.into_iter().find(|x| !x.closed).map_or("CNY".into(), |x| serde_json::to_value(x.currency).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or("CNY".into())),
    };
    let mut patch = json!({ "flow": f.flow.unwrap_or_else(|| "out".into()), "amount": amount, "currency": currency, "day": f.day.unwrap_or_else(|| today.to_string()) });
    if let Some(k) = f.kind_id {
        patch["kindId"] = json!(k);
    }
    if let Some(n) = f.note {
        patch["note"] = json!(n);
    }
    one(Op::AddEntry { patch })
}

fn edit_entry(c: &Connection, a: &Value, question: &str, today: &str) -> Result<Plan, String> {
    let d = dates(a, &["on", "day"], question, today);
    let (on, day) = (d[0].clone(), d[1].clone());
    let (find, was, amount, kind) = (text(a, "find", 50), cents(a, "was"), cents(a, "amount").filter(|n| *n > 0), text(a, "kind", 20));
    if amount.is_none() && kind.is_none() && day.is_none() {
        return Err("没说要改成什么（金额、分类或日期）".into());
    }
    if find.is_none() && was.is_none() && on.is_none() {
        return Err("没说是哪一笔：请问用户（哪天的、备注是什么、多少钱）".into());
    }
    let outs: Vec<(i64, String)> = kinds::list(c, finance::OUT_KINDS)?.into_iter().map(|k| (k.id, k.name)).collect();
    let ins: Vec<(i64, String)> = kinds::list(c, finance::IN_KINDS)?.into_iter().map(|k| (k.id, k.name)).collect();
    if let Some(k) = &kind {
        if assist::pick(Some(k.clone()), &outs).is_none() && assist::pick(Some(k.clone()), &ins).is_none() {
            return Err(format!("没有叫「{k}」的分类"));
        }
    }
    let (from, to) = match &on {
        Some(d) => (d.clone(), d.clone()),
        None => (add_days(today, -30), today.to_string()),
    };
    let mut st = c.prepare("SELECT id FROM fin_entries WHERE day BETWEEN ?1 AND ?2 ORDER BY day DESC, id DESC").map_err(err)?;
    let ids: Vec<i64> = st.query_map([&from, &to], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut found: Vec<finance::Entry> = ids.into_iter().map(|id| finance::entry(c, id)).collect::<Result<_, _>>()?;
    if let Some(w) = was {
        found.retain(|e| e.amount == w);
    }
    let label = |e: &finance::Entry| {
        let names = if e.flow == finance::Flow::In { &ins } else { &outs };
        format!("{} {}", e.note.clone().unwrap_or_default(), e.kind_id.and_then(|k| names.iter().find(|x| x.0 == k)).map_or("", |x| x.1.as_str()))
    };
    if let Some(f) = &find {
        found = fit(found, label, f);
    }
    if found.is_empty() {
        let what = [on.map(|d| d.replace('-', "/")), find, was.map(|w| format!("{} 元", w as f64 / 100.0))].into_iter().flatten().collect::<Vec<_>>().join("、");
        return Err(format!("没找到「{what}」这笔收支（只看最近 31 天，说了哪天就看那天）：请问用户是哪一笔"));
    }
    let ops = found
        .iter()
        .map(|e| {
            let mut patch = json!({});
            if let Some(n) = amount {
                patch["amount"] = json!(n);
            }
            if let Some(k) = &kind {
                let names = if e.flow == finance::Flow::In { &ins } else { &outs };
                if let Some(id) = assist::pick(Some(k.clone()), names) {
                    patch["kindId"] = json!(id);
                }
            }
            if let Some(d) = &day {
                patch["day"] = json!(d);
            }
            Op::EditEntry { id: e.id, patch }
        })
        .collect();
    Ok(some(ops))
}

fn set_balance(c: &Connection, a: &Value, question: &str, today: &str) -> Result<Plan, String> {
    let name = text(a, "account", 50).ok_or("没说是哪个账户")?;
    let amount = cents(a, "amount").ok_or("没说余额是多少")?;
    let day = dates(a, &["day"], question, today).remove(0).unwrap_or_else(|| today.to_string());
    let open: Vec<finance::Account> = finance::accounts(c)?.into_iter().filter(|x| !x.closed).collect();
    let all = joined(open.iter().map(|x| x.name.clone()).collect());
    let found = fit(open, |x| x.name.clone(), &name);
    if found.is_empty() {
        return Err(format!("没有叫「{name}」的账户（有：{all}）：请问用户是哪个"));
    }
    Ok(some(found.into_iter().map(|x| Op::SetBalance { account: x.id, day: day.clone(), amount }).collect()))
}

// ---------- 一步一步 ----------

/// Undone tasks, recently touched first: (id, title).
fn open_tasks(c: &Connection) -> Result<Vec<(i64, String)>, String> {
    let mut st = c.prepare("SELECT id, title FROM todo_tasks WHERE done = 0 ORDER BY updated_at DESC, id DESC").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// [start, due] like the panel: one day said is the day it's due; a start only before an end.
fn span(d: Vec<Option<String>>) -> (Option<String>, Option<String>) {
    match (d[0].clone(), d[1].clone()) {
        (Some(s), None) => (None, Some(s)),
        (s, d) => (s.filter(|s| d.as_ref().is_some_and(|d| s < d)), d),
    }
}

fn add_task(c: &Connection, a: &Value, question: &str, today: &str) -> Result<Plan, String> {
    let f = quick(c, "todo", a, question, today)?;
    let title = f.title.clone().ok_or("没说任务是什么")?;
    let (start, due) = span(dates(a, &["start_date", "due_date"], question, today));
    let mut base = json!({ "title": title });
    if let Some(d) = start {
        base["startDate"] = json!(d);
    }
    if let Some(d) = due {
        base["dueDate"] = json!(d);
    }
    if let Some(n) = text(a, "note", 2000) {
        base["note"] = json!(n);
    }
    if let Some(p) = text(a, "parent", 100) {
        let parents = fit(open_tasks(c)?, |t| t.1.clone(), &p);
        if parents.is_empty() {
            return Err(format!("没找到叫「{p}」的任务（只找没完成的）：请问用户要加在哪个任务下面"));
        }
        return Ok(some(parents.into_iter().map(|(id, _)| { let mut patch = base.clone(); patch["parentId"] = json!(id); Op::AddTask { patch } }).collect()));
    }
    // the 分组 / 目标 the model picked from the list (it knows where similar tasks are; the card
    // shows it), else the first 分组 — not 「一句话填好」's mention rule, which has an open panel to fall back on
    let goals: Vec<todo::Goal> = todo::goals(c)?.into_iter().filter(|g| g.status == todo::GoalStatus::Active).collect();
    let goal = assist::pick(text(a, "goal", 60), &goals.iter().map(|g| (g.id, g.name.clone())).collect::<Vec<_>>()).or(f.goal_id);
    let groups: Vec<(i64, String)> = todo::groups(c)?.into_iter().map(|g| (g.id, g.name)).collect();
    let group = match goal.and_then(|id| goals.iter().find(|g| g.id == id)).map(|g| g.group_id) {
        Some(g) => g,
        None => assist::pick(text(a, "group", 60), &groups).or(f.group_id).or(groups.first().map(|g| g.0)).ok_or("还没有分组，先在一步一步里建一个")?,
    };
    base["groupId"] = json!(group);
    if let Some(g) = goal {
        base["goalId"] = json!(g);
    }
    one(Op::AddTask { patch: base })
}

fn edit_task(c: &Connection, a: &Value, question: &str, today: &str) -> Result<Plan, String> {
    let name = text(a, "task", 100).ok_or("没说是哪个任务")?;
    let done = a.get("done").and_then(Value::as_bool).unwrap_or(false);
    let amount = a.get("amount").and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64))).filter(|n| (1..=100_000).contains(n));
    let (start, due) = span(dates(a, &["start_date", "due_date"], question, today));
    let mut patch = json!({});
    if let Some(d) = start {
        patch["startDate"] = json!(d);
    }
    if let Some(d) = due {
        patch["dueDate"] = json!(d);
    }
    if let Some(g) = text(a, "goal", 60) {
        let goals: Vec<todo::Goal> = todo::goals(c)?.into_iter().filter(|x| x.status == todo::GoalStatus::Active).collect();
        let id = assist::pick(Some(g.clone()), &goals.iter().map(|x| (x.id, x.name.clone())).collect::<Vec<_>>()).ok_or(format!("没有叫「{g}」的进行中的目标"))?;
        patch["groupId"] = json!(goals.iter().find(|x| x.id == id).map(|x| x.group_id));
        patch["goalId"] = json!(id);
    } else if let Some(g) = text(a, "group", 60) {
        let groups: Vec<(i64, String)> = todo::groups(c)?.into_iter().map(|x| (x.id, x.name)).collect();
        let id = assist::pick(Some(g.clone()), &groups).ok_or(format!("没有叫「{g}」的分组（有：{}）", joined(groups.iter().map(|x| x.1.clone()).collect())))?;
        patch["groupId"] = json!(id);
        patch["goalId"] = Value::Null;
    }
    let edits = patch.as_object().is_some_and(|o| !o.is_empty());
    if !done && amount.is_none() && !edits {
        return Err("没说要做什么（完成、打卡、记进度、改日期或分组）".into());
    }
    let found = fit(open_tasks(c)?, |t| t.1.clone(), &name);
    if found.is_empty() {
        return Err(format!("没找到叫「{name}」的任务（只找没完成的）：请问用户是哪一个"));
    }
    Ok(some(found.into_iter().map(|(id, _)| Op::EditTask { id, patch: edits.then(|| patch.clone()), done: done && amount.is_none(), log: amount }).collect()))
}

// ---------- 投石问路 ----------

fn add_job(c: &Connection, a: &Value, question: &str, today: &str) -> Result<Plan, String> {
    let mut f = quick(c, "jobs", a, question, today)?;
    // a step that names no status: 「投了」 is the first one, anything else is asked again (not dropped);
    // none given but the question says 投了: the first one too
    let said = text(a, "status", 20).or_else(|| ["投了", "投递了", "已投", "刚投"].iter().any(|w| question.contains(w)).then(|| "投了".to_string()));
    if let (None, Some(s)) = (f.status_id, said) {
        let sts: Vec<jobs::Status> = jobs::statuses(c)?.into_iter().filter(|x| x.stage != jobs::Stage::Wish).collect();
        f.status_id = Some(match sts.iter().find(|x| s.contains('投') && x.stage == jobs::Stage::Applied) {
            Some(x) => x.id,
            None => return Err(format!("没有「{s}」这个状态（有：{}）", joined(sts.iter().map(|x| x.name.clone()).collect()))),
        });
    }
    let mut patch = json!({ "company": f.company.clone().unwrap_or_default() });
    for (k, v) in [("position", &f.position), ("location", &f.location)] {
        if let Some(v) = v {
            patch[k] = json!(v);
        }
    }
    if let Some(k) = f.kind_id {
        patch["kindId"] = json!(k);
    }
    if let Some(n) = text(a, "note", 5000) {
        patch["note"] = json!(n);
    }
    if let Some(s) = f.status_id {
        let day = dates(a, &["day"], question, today).remove(0).or(f.day).unwrap_or_else(|| today.to_string());
        let mut steps: Vec<jobs::Step> = jobs::statuses(c)?
            .into_iter()
            .filter(|x| !matches!(x.stage, jobs::Stage::Wish | jobs::Stage::Closed))
            .map(|x| jobs::Step { status_id: x.id, done: false, done_on: None })
            .collect();
        match steps.iter_mut().find(|x| x.status_id == s) {
            Some(x) => (x.done, x.done_on) = (true, Some(day)),
            None => steps.push(jobs::Step { status_id: s, done: true, done_on: Some(day) }),
        }
        patch["steps"] = serde_json::to_value(steps).map_err(err)?;
    }
    one(Op::AddJob { patch })
}

fn edit_job(c: &Connection, a: &Value, question: &str, today: &str) -> Result<Plan, String> {
    let name = text(a, "job", 100).ok_or("没说是哪个投递")?;
    let (status, note) = (text(a, "status", 20), text(a, "note", 2000));
    if status.is_none() && note.is_none() {
        return Err("没说要改什么（走到了哪一步、加一句备注）".into());
    }
    let sts: Vec<(i64, String)> = jobs::statuses(c)?.into_iter().filter(|x| x.stage != jobs::Stage::Wish).map(|x| (x.id, x.name)).collect();
    let step = match &status {
        Some(s) => Some(assist::pick(Some(s.clone()), &sts).ok_or(format!("没有「{s}」这个状态（有：{}）", joined(sts.iter().map(|x| x.1.clone()).collect())))?),
        None => None,
    };
    let day = dates(a, &["day"], question, today).remove(0).unwrap_or_else(|| today.to_string());
    let found = fit(jobs::jobs(c, today)?, |j| format!("{} {}", j.company, j.position.clone().unwrap_or_default()), &name);
    if found.is_empty() {
        return Err(format!("没找到「{name}」的投递：请问用户是哪一个"));
    }
    let ops = found
        .into_iter()
        .map(|j| {
            let mut patch = json!({});
            if let Some(s) = step {
                let mut steps = j.steps.clone();
                match steps.iter_mut().find(|x| x.status_id == s) {
                    Some(x) => (x.done, x.done_on) = (true, Some(day.clone())),
                    None => steps.push(jobs::Step { status_id: s, done: true, done_on: Some(day.clone()) }),
                }
                patch["steps"] = serde_json::to_value(steps).unwrap_or_default();
            }
            if let Some(n) = &note {
                patch["note"] = json!(match j.note.as_deref().map(str::trim).filter(|o| !o.is_empty()) {
                    Some(old) => format!("{old}\n{n}"),
                    None => n.clone(),
                });
            }
            Op::EditJob { id: j.id, patch }
        })
        .collect();
    Ok(some(ops))
}

// ---------- 摸鱼读读 · 好奇看看 ----------

fn work_module(a: &Value) -> Result<&'static str, String> {
    match text(a, "kind", 10).as_deref() {
        Some("book" | "书") => Ok("books"),
        Some("film" | "影视" | "电影" | "剧") => Ok("films"),
        _ => Err("没说是书（book）还是影视（film）".into()),
    }
}

fn works(c: &Connection, module: &str) -> Result<Vec<(i64, String)>, String> {
    Ok(if module == "books" {
        books::list(c)?.into_iter().map(|b| (b.id, b.title)).collect()
    } else {
        films::list(c)?.into_iter().map(|f| (f.id, f.title)).collect()
    })
}

fn add_work(c: &Connection, a: &Value, question: &str, today: &str) -> Result<Plan, String> {
    let module = work_module(a)?;
    // the tool's `kind` is book / film, its `type` the 类型 (the reader's `kind`)
    let mut read = a.clone();
    read["kind"] = a.get("type").cloned().unwrap_or(Value::Null);
    let f = quick(c, module, &read, question, today)?;
    let title = f.title.clone().ok_or("没说名称")?;
    if works(c, module)?.iter().any(|(_, t)| norm(t) == norm(&title)) {
        return Err(format!("「{title}」已经在{}里了：要改状态、星级就用 {EDIT_WORK}", if module == "books" { "摸鱼读读" } else { "好奇看看" }));
    }
    let mut patch = json!({ "title": title });
    let creator = if module == "books" { "author" } else { "director" };
    for (k, v) in [(creator, &f.creator), ("country", &f.country), ("status", &f.status), ("finishedYear", &f.finished_year)] {
        if let Some(v) = v {
            patch[k] = json!(v);
        }
    }
    if let Some(k) = f.kind_id {
        patch["kindId"] = json!(k);
    }
    if let Some(r) = f.rating {
        patch["rating"] = json!(r);
    }
    one(Op::AddWork { module: module.into(), patch })
}

fn edit_work(c: &Connection, a: &Value) -> Result<Plan, String> {
    let module = work_module(a)?;
    let title = text(a, "title", 200).ok_or("没说是哪一本 / 哪一部")?;
    let mut patch = json!({});
    let status = text(a, "status", 10).and_then(|s| match s.as_str() {
        "todo" | "想读" | "想看" => Some("todo"),
        "doing" | "在读" | "在看" => Some("doing"),
        "done" | "读完" | "看完" | "已读" | "已看" => Some("done"),
        _ => None,
    });
    if let Some(s) = status {
        patch["status"] = json!(s);
    }
    if let Some(r) = a.get("rating").and_then(|r| r.as_i64().or_else(|| r.as_f64().map(|f| f.round() as i64))).filter(|r| (1..=5).contains(r)) {
        patch["rating"] = json!(r);
    }
    if let Some(y) = text(a, "year", 4).filter(|y| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit())) {
        patch["finishedYear"] = json!(y);
    }
    if patch.as_object().is_some_and(|o| o.is_empty()) {
        return Err("没说要改什么（状态、星级、读完看完的年份）".into());
    }
    let found = fit(works(c, module)?, |w| w.1.clone(), title.trim_matches(|c| c == '《' || c == '》'));
    if found.is_empty() {
        return Err(format!("{}里没有「{title}」：要新加就用 {ADD_WORK}，不确定就问用户", if module == "books" { "摸鱼读读" } else { "好奇看看" }));
    }
    Ok(some(found.into_iter().map(|(id, _)| Op::EditWork { module: module.into(), id, patch: patch.clone() }).collect()))
}

// ---------- 随意画画 ----------

fn finish_lesson(c: &Connection, a: &Value) -> Result<Plan, String> {
    let (course, lesson) = (text(a, "course", 100), text(a, "lesson", 100));
    if course.is_none() && lesson.is_none() {
        return Err("没说是哪门课、哪一节".into());
    }
    let all = drawing::courses(c)?;
    let names = joined(all.iter().map(|x| x.name.clone()).collect());
    let courses = match &course {
        Some(q) => fit(all, |x| x.name.clone(), q),
        None => all,
    };
    if courses.is_empty() {
        return Err(format!("没有叫「{}」的课程（有：{names}）", course.unwrap_or_default()));
    }
    let mut ops = vec![];
    for co in &courses {
        let open: Vec<drawing::Lesson> = drawing::lessons(c, co.id)?.into_iter().filter(|l| l.status != crate::modules::common::Status::Done).collect();
        let picked: Vec<drawing::Lesson> = match &lesson {
            Some(q) => {
                // 「第 3 课」「3」 → the lesson numbered 3, else by its title
                let num = norm(q).trim_start_matches('第').trim_end_matches(['课', '节']).to_string();
                let by_code: Vec<drawing::Lesson> = open.iter().filter(|l| l.code.as_deref().is_some_and(|x| norm(x) == num)).cloned().collect();
                if by_code.is_empty() { fit(open, |l| format!("{} {}", l.code.clone().unwrap_or_default(), l.title), q) } else { by_code }
            }
            // a lesson not named: the next one not done yet
            None => open.into_iter().take(1).collect(),
        };
        ops.extend(picked.into_iter().map(|l| Op::EditLesson { id: l.id, patch: json!({ "status": "done" }) }));
    }
    if ops.is_empty() {
        return Err(format!("没找到没画完的「{}」：请问用户是哪一节", [course, lesson].into_iter().flatten().collect::<Vec<_>>().join(" ")));
    }
    Ok(some(ops))
}

// ---------- 出门走走 ----------

fn add_trip(c: &Connection, a: &Value, question: &str, today: &str) -> Result<Plan, String> {
    let f = quick(c, "packing", a, question, today)?;
    let mut patch = json!({ "name": f.name.clone().ok_or("没说出行的名称")? });
    if let Some(d) = dates(a, &["depart_on"], question, today).remove(0).or(f.depart_on) {
        patch["departOn"] = json!(d);
    }
    one(Op::AddTrip { patch, template: f.template_id })
}

fn edit_trip(c: &Connection, a: &Value, today: &str) -> Result<Plan, String> {
    let (add, pack) = (list(a, "add"), list(a, "pack"));
    if add.is_empty() && pack.is_empty() {
        return Err("没说要加上或装好哪些东西".into());
    }
    let trips = packing::trips(c)?;
    // not named: the trips still ahead (else the newest)
    let found = match text(a, "trip", 100) {
        Some(q) => fit(trips, |t| t.name.clone(), &q),
        None => {
            let ahead: Vec<packing::Trip> = trips.iter().filter(|t| t.depart_on.as_deref().is_none_or(|d| d >= today)).cloned().collect();
            if ahead.is_empty() { trips.into_iter().take(1).collect() } else { ahead }
        }
    };
    if found.is_empty() {
        return Err("没找到这次出行：请问用户是哪一次".into());
    }
    let mut plan = Plan::default();
    let mut missing: Vec<String> = vec![];
    for t in found {
        let items = packing::items(c, packing::List::Trip, t.id)?;
        let mut new: Vec<String> = vec![];
        for n in &add {
            if !items.iter().any(|i| norm(&i.name) == norm(n)) && !new.iter().any(|x| norm(x) == norm(n)) {
                new.push(n.clone());
            }
        }
        let mut packed: Vec<(i64, String)> = vec![];
        for n in &pack {
            let open: Vec<&packing::Item> = items.iter().filter(|i| i.packed != Some(true) && !packed.iter().any(|p| p.0 == i.id)).collect();
            match fit(open, |i| i.name.clone(), n).first() {
                Some(i) => packed.push((i.id, i.name.clone())),
                None if !missing.contains(n) => missing.push(n.clone()),
                None => {}
            }
        }
        if new.is_empty() && packed.is_empty() {
            continue;
        }
        let mut extra = vec![];
        if !new.is_empty() {
            extra.push(Field { label: "加上".into(), before: None, after: new.join("、"), long: false });
        }
        if !packed.is_empty() {
            extra.push(Field { label: "装好".into(), before: None, after: packed.iter().map(|p| p.1.as_str()).collect::<Vec<_>>().join("、"), long: false });
        }
        plan.ops.push((Op::EditTrip { id: t.id, add: new, pack: packed.into_iter().map(|p| p.0).collect() }, extra));
    }
    if !missing.is_empty() {
        plan.note = Some(format!("清单里没找到（或已经装好了）：{}", missing.join("、")));
    }
    if plan.ops.is_empty() {
        return Err(plan.note.unwrap_or_else(|| "要加的都已经在清单里了".into()));
    }
    plan.more = plan.ops.len().saturating_sub(MAX);
    plan.ops.truncate(MAX);
    Ok(plan)
}

// ---------- 念念不忘 ----------

fn add_note(a: &Value) -> Result<Plan, String> {
    let content = text(a, "content", notes::MAX_CHARS).ok_or("没说笔记写什么")?;
    one(Op::AddNote { patch: json!({ "content": content }) })
}

fn edit_note(c: &Connection, a: &Value) -> Result<Plan, String> {
    let find = text(a, "find", 100).ok_or("没说是哪条笔记")?;
    let content = text(a, "content", notes::MAX_CHARS);
    let new = a.get("new").and_then(Value::as_str).unwrap_or("");
    // 「押一付三改成押一付二」 given as find + new: the words found are the part replaced
    let old = a.get("old").and_then(Value::as_str).filter(|s| !s.is_empty()).or_else(|| (content.is_none() && a.get("new").is_some()).then_some(find.as_str()));
    if content.is_none() && old.is_none() {
        return Err("没说要改成什么（改好后的全文 content，或者 old 换成 new）".into());
    }
    let mut found = notes::list(c, &notes::Filter { q: Some(find.clone()), ..Default::default() })?;
    if found.is_empty() {
        let mut recent = notes::list(c, &notes::Filter::default())?;
        recent.truncate(300);
        found = fit(recent, |n| n.content.clone(), &find);
    }
    if let Some(o) = old {
        found.retain(|n| n.content.contains(o));
    }
    let ops: Vec<Op> = found
        .into_iter()
        .filter_map(|n| {
            let next = match (&content, old) {
                (Some(t), _) => t.clone(),
                (None, Some(o)) => n.content.replacen(o, new, 1),
                _ => return None,
            };
            (next != n.content).then(|| Op::EditNote { id: n.id, patch: json!({ "content": next }) })
        })
        .collect();
    if ops.is_empty() {
        return Err(format!("没找到要改的笔记（含「{find}」{}）：请问用户是哪一条", old.map(|o| format!("、写着「{o}」")).unwrap_or_default()));
    }
    Ok(some(ops))
}

// ---------- 见字如面 ----------

/// Characters of the unchanged text shown around a change.
const AROUND: usize = 60;

fn add_article(a: &Value) -> Result<Plan, String> {
    let content = text(a, "content", articles::MAX_CHARS).ok_or("没说文章写什么")?;
    one(Op::AddArticle { patch: json!({ "content": content }) })
}

/// The part of a long text a change touches, with a little unchanged text around it (to the line's
/// start and end, at most AROUND characters each way): (before, after), cut ends marked 「…」.
pub fn changed_part(old: &str, new: &str) -> (String, String) {
    let (a, b): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let mut p = 0;
    while p < a.len() && p < b.len() && a[p] == b[p] {
        p += 1;
    }
    let mut s = 0;
    while s < a.len() - p && s < b.len() - p && a[a.len() - 1 - s] == b[b.len() - 1 - s] {
        s += 1;
    }
    let mut start = p;
    while start > 0 && a[start - 1] != '\n' && p - start < AROUND {
        start -= 1;
    }
    let mut end = a.len() - s;
    while end < a.len() && a[end] != '\n' && end - (a.len() - s) < AROUND {
        end += 1;
    }
    let tail = a.len() - end;
    let cut = |v: &[char], from: usize, to: usize| format!("{}{}{}", if from > 0 { "…" } else { "" }, v[from..to].iter().collect::<String>(), if to < v.len() { "…" } else { "" });
    (cut(&a, start, end), cut(&b, start, b.len() - tail))
}

/// 润色 (old → new), 续写 (append, after a passage or at the end) or the whole new text of an article
/// 柚子 may read (by its 编号, else words of its title or text); the card shows the part that changes.
fn edit_article(c: &Connection, a: &Value) -> Result<Plan, String> {
    let id = a.get("id").and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().trim_start_matches("编号").trim().parse().ok())));
    let find = text(a, "find", 100);
    let given = |k: &str| a.get(k).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(String::from);
    let edit = Rewrite {
        content: text(a, "content", articles::MAX_CHARS),
        old: given("old"),
        new: a.get("new").and_then(Value::as_str).unwrap_or("").to_string(),
        append: given("append").map(|s| s.trim().to_string()),
        after: given("after"),
    };
    if edit.content.is_none() && edit.old.is_none() && edit.append.is_none() {
        return Err("没说要怎么改（润色：old 换成 new；续写：append，可以加 after；或者改好后的全文 content）".into());
    }
    if id.is_none() && find.is_none() {
        return Err("没说是哪一篇（编号 id，或标题里的字 find）".into());
    }
    // only the ones 柚子 may read: never a locked one or the trash
    let all: Vec<(i64, String, String)> = articles::readable(c)?;
    let found: Vec<(i64, String, String)> = match (id, &find) {
        (Some(id), _) => all.into_iter().filter(|x| x.0 == id).collect(),
        (None, Some(q)) => {
            let by_title = fit(all.clone(), |x| x.1.clone(), q);
            if by_title.is_empty() {
                let words: Vec<String> = q.split_whitespace().map(str::to_lowercase).collect();
                all.into_iter().filter(|x| { let t = format!("{}\n{}", x.1, x.2).to_lowercase(); words.iter().all(|w| t.contains(w.as_str())) }).collect()
            } else {
                by_title
            }
        }
        (None, None) => vec![],
    };
    if found.is_empty() {
        return Err(match id {
            Some(id) => format!("没有编号 {id} 的文章（加密的、废纸篓里的不能改，也可能已经删了）：请问用户是哪一篇"),
            None => format!("没找到标题或正文含「{}」的文章：请问用户是哪一篇", find.unwrap_or_default()),
        });
    }
    let mut ops: Vec<(Op, Vec<Field>)> = vec![];
    for (id, _, text) in &found {
        let Some(next) = edit.apply(text).filter(|n| n != text) else { continue };
        let (before, after) = changed_part(text, &next);
        ops.push((Op::EditArticle { id: *id, edit: edit.clone() }, vec![Field { label: "内容".into(), before: Some(before), after, long: true }]));
    }
    if ops.is_empty() {
        let clip = |s: &Option<String>| s.as_deref().map(|x| x.chars().take(30).collect::<String>());
        let what = [clip(&edit.old).map(|o| format!("写着「{o}」")), clip(&edit.after).map(|x| format!("有「{x}」这段"))].into_iter().flatten().collect::<Vec<_>>().join("、");
        return Err(if what.is_empty() { "改了和原来一样，不用改".into() } else { format!("找到的文章里没有{what}的地方：先用 pomelo_articles 的 read 读原文，照原文抄") });
    }
    let more = ops.len().saturating_sub(MAX);
    ops.truncate(MAX);
    Ok(Plan { ops, more, note: None })
}
