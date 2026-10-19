//! 定时任务 (v0.4 phase 7; requirements「小拾 · 定时任务、今日简报、年度总结」):
//! - A schedule: 名称, when (every day / a weekday / a day of the month, at HH:MM local time; a month
//!   without that day uses its last), the model (None = what 小拾 has selected), 要做什么 (the question
//!   it asks), on / off. Ready-made (migration 12, editable and deletable): 每周回顾, 每月回顾.
//! - Due = enabled and its latest time (≤ now) is after both the last time it ran for
//!   (`schedule_state`) and `since` (made / time changed / switched on: never runs for the past).
//!   The monitor checks every 30 s, so Pomelo closed at the time runs it when next open — once, for
//!   the latest time missed. Several due run one after another.
//! - A run: a new conversation (listed under 定时, unseen until opened, titled 「名称 · MM/DD」) asking
//!   `prompt` under the model's rules (cloud rules for a cloud model), read-only (no write tools), no
//!   memories picked; it runs beside replies being written (a local model takes turns). A local model that isn't running
//!   is started (the schedule's, else the last used) and stopped after if this run started it and
//!   nothing else is writing; another local model running → that one is used and the result says so
//!   at the top. Can't get a model → the conversation holds a failed reply saying why.
//! - 思考 (`thinking`, default on): the run asks its model to think (or not); its model must be able to
//!   (`check_model`: a cloud model's 思考能力, a local one's catalog entry; None = the selection, taken as
//!   it is at run time). The conversation it writes keeps the switch.
//! - 立即运行 runs it now without touching its times. One schedule runs at a time.
//! - Edits are undoable (`with_picky_undo`); the bookkeeping (`schedule_state`, sessions.unseen) is
//!   not, and undoable edits never touch it.

use super::chat::{self, Extras, NewSession, Outcome};
use super::store::NOW;
use super::cloud::Think;
use super::{assist, Picky, Route, Selection};
use crate::modules::common::{self, err, require, Tx};
use crate::AppState;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

const NAME_MAX: usize = 30;
const PROMPT_MAX: usize = 2000;
const WEEKDAYS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

/// When a schedule runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// day | week | month | year
    pub every: String,
    /// week: 1 (周一) … 7; month / year: 1 … 31; day: None.
    pub day: Option<i64>,
    /// year: 1 … 12; otherwise None.
    #[serde(default)]
    pub month: Option<i64>,
    /// HH:MM
    pub at: String,
}

impl Rule {
    fn runs_on(&self, date: &str) -> bool {
        match self.every.as_str() {
            "day" => true,
            "week" => common::weekday(date) as i64 == self.day.unwrap_or(1),
            every => {
                let (y, m, d) = (date[..4].parse::<i64>().unwrap_or(1970), date[5..7].parse::<u32>().unwrap_or(1), date[8..10].parse::<i64>().unwrap_or(1));
                (every != "year" || m as i64 == self.month.unwrap_or(1)) && d == self.day.unwrap_or(1).min(common::days_in_month(y, m) as i64)
            }
        }
    }
    /// 「每周一 09:00」
    pub fn text(&self) -> String {
        match (self.every.as_str(), self.day) {
            ("week", Some(d)) => format!("每周{} {}", WEEKDAYS[(d.clamp(1, 7) - 1) as usize], self.at),
            ("month", Some(d)) => format!("每月 {d} 号 {}", self.at),
            ("year", Some(d)) => format!("每年 {} 月 {d} 日 {}", self.month.unwrap_or(1), self.at),
            _ => format!("每天 {}", self.at),
        }
    }
}

/// Days looked back / ahead for a time (a year and a bit).
const SPAN: i64 = 400;

/// The latest time ("YYYY-MM-DD HH:MM") at or before `now` it runs at.
pub fn latest(rule: &Rule, now: &str) -> String {
    let today = &now[..10];
    (0..=SPAN)
        .map(|back| common::add_days(today, -back))
        .filter(|d| rule.runs_on(d))
        .map(|d| format!("{d} {}", rule.at))
        .find(|s| s.as_str() <= now)
        .unwrap_or_else(|| format!("{} {}", common::add_days(today, -SPAN - 1), rule.at))
}

/// The first time after `now` it runs at.
pub fn next(rule: &Rule, now: &str) -> String {
    let today = &now[..10];
    (0..=SPAN)
        .map(|fwd| common::add_days(today, fwd))
        .filter(|d| rule.runs_on(d))
        .map(|d| format!("{d} {}", rule.at))
        .find(|s| s.as_str() > now)
        .unwrap_or_else(|| format!("{} {}", common::add_days(today, SPAN + 1), rule.at))
}

/// Local time now, "YYYY-MM-DD HH:MM".
pub fn now(conn: &Connection) -> Result<String, String> {
    conn.query_row("SELECT strftime('%Y-%m-%d %H:%M', 'now', 'localtime')", [], |r| r.get(0)).map_err(err)
}

// ---------- records ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: i64,
    pub name: String,
    pub prompt: String,
    #[serde(flatten)]
    pub rule: Rule,
    pub model: Option<String>,
    pub thinking: bool,
    pub enabled: bool,
    pub builtin: Option<String>,
    /// The next time it runs ("YYYY-MM-DD HH:MM"; None when off).
    pub next: Option<String>,
    /// The conversation it wrote last, and when.
    pub last_session: Option<i64>,
    pub last_at: Option<String>,
    /// Running now (or waiting for its model).
    pub running: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInput {
    pub name: String,
    pub prompt: String,
    pub every: String,
    #[serde(default)]
    pub day: Option<i64>,
    #[serde(default)]
    pub month: Option<i64>,
    pub at: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "yes")]
    pub thinking: bool,
}

fn yes() -> bool {
    true
}

/// Cleaned, or what's wrong.
pub fn check_input(i: &ScheduleInput) -> Result<(), String> {
    checked(i).map(|_| ())
}

fn checked(i: &ScheduleInput) -> Result<ScheduleInput, String> {
    let name = require(&i.name, "名称", NAME_MAX)?;
    let prompt = require(&i.prompt, "要做什么", PROMPT_MAX)?;
    let (day, month) = match i.every.as_str() {
        "day" => (None, None),
        "week" => (Some(i.day.filter(|d| (1..=7).contains(d)).ok_or("请选星期几")?), None),
        "month" => (Some(i.day.filter(|d| (1..=31).contains(d)).ok_or("请选几号")?), None),
        "year" => (Some(i.day.filter(|d| (1..=31).contains(d)).ok_or("请选几号")?), Some(i.month.filter(|m| (1..=12).contains(m)).ok_or("请选几月")?)),
        _ => return Err("请选什么时候运行".into()),
    };
    let at = {
        let (h, m) = i.at.trim().split_once(':').ok_or("时间要写成 09:00 这样")?;
        match (h.parse::<u32>(), m.parse::<u32>()) {
            (Ok(h), Ok(m)) if h < 24 && m < 60 && m.to_string().len() <= 2 && h.to_string().len() <= 2 => format!("{h:02}:{m:02}"),
            _ => return Err("时间要写成 09:00 这样".into()),
        }
    };
    let model = i.model.as_deref().map(str::trim).filter(|m| !m.is_empty()).map(String::from);
    if model.as_deref().is_some_and(|m| !(m.starts_with("local:") || m.starts_with("cloud:"))) {
        return Err("不认识这个模型".into());
    }
    Ok(ScheduleInput { name, prompt, every: i.every.clone(), day, month, at, model, thinking: i.thinking })
}

const COLS: &str = "s.id, s.name, s.prompt, s.every, s.day, s.at, s.model, s.enabled, s.builtin, \
    (SELECT max(x.id) FROM sessions x WHERE x.schedule_id = s.id), \
    (SELECT x.created_at FROM sessions x WHERE x.schedule_id = s.id ORDER BY x.id DESC LIMIT 1), \
    st.last_slot, strftime('%Y-%m-%d %H:%M', s.since, 'localtime'), s.month, s.thinking";

struct Row {
    s: Schedule,
    last_slot: Option<String>,
    since: String,
}

fn rows(conn: &Connection, filter: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Row>, String> {
    let sql = format!("SELECT {COLS} FROM schedules s LEFT JOIN schedule_state st ON st.schedule_id = s.id {filter} ORDER BY s.sort_order, s.id");
    let mut st = conn.prepare(&sql).map_err(err)?;
    let out = st
        .query_map(args, |r| {
            Ok(Row {
                s: Schedule {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    prompt: r.get(2)?,
                    rule: Rule { every: r.get(3)?, day: r.get(4)?, month: r.get(13)?, at: r.get(5)? },
                    model: r.get(6)?,
                    thinking: r.get(14)?,
                    enabled: r.get(7)?,
                    builtin: r.get(8)?,
                    next: None,
                    last_session: r.get(9)?,
                    last_at: r.get(10)?,
                    running: false,
                },
                last_slot: r.get(11)?,
                since: r.get(12)?,
            })
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(out)
}

pub fn list(conn: &Connection, now: &str) -> Result<Vec<Schedule>, String> {
    let running = current();
    Ok(rows(conn, "", &[])?
        .into_iter()
        .map(|r| {
            let mut s = r.s;
            s.next = s.enabled.then(|| next(&s.rule, now));
            s.running = running == Some(s.id);
            s
        })
        .collect())
}

pub fn get(conn: &Connection, id: i64) -> Result<Schedule, String> {
    rows(conn, "WHERE s.id = ?1", &[&id as &dyn rusqlite::ToSql])?.into_iter().next().map(|r| r.s).ok_or_else(|| "这个定时任务已经被删除了".into())
}

pub fn create(conn: &Connection, input: &ScheduleInput) -> Result<i64, String> {
    let i = checked(input)?;
    conn.execute(
        "INSERT INTO schedules (name, prompt, every, day, month, at, model, thinking, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, (SELECT coalesce(max(sort_order), -1) + 1 FROM schedules))",
        params![i.name, i.prompt, i.every, i.day, i.month, i.at, i.model, i.thinking],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

/// A changed time counts only from now on.
pub fn update(conn: &Connection, id: i64, input: &ScheduleInput) -> Result<(), String> {
    let i = checked(input)?;
    let old = get(conn, id)?;
    let rule = Rule { every: i.every.clone(), day: i.day, month: i.month, at: i.at.clone() };
    let since = if rule != old.rule { NOW } else { "since" };
    conn.execute(
        &format!("UPDATE schedules SET name = ?1, prompt = ?2, every = ?3, day = ?4, month = ?5, at = ?6, model = ?7, thinking = ?9, since = {since}, updated_at = {NOW} WHERE id = ?8"),
        params![i.name, i.prompt, i.every, i.day, i.month, i.at, i.model, id, i.thinking],
    )
    .map_err(err)?;
    Ok(())
}

/// Switched on: times missed while off don't run.
pub fn set_enabled(conn: &Connection, id: i64, on: bool) -> Result<(), String> {
    get(conn, id)?;
    conn.execute(
        &format!("UPDATE schedules SET enabled = ?1, since = CASE WHEN ?1 THEN {NOW} ELSE since END, updated_at = {NOW} WHERE id = ?2 AND enabled <> ?1"),
        params![on, id],
    )
    .map(|_| ())
    .map_err(err)
}

/// Its conversations stay (under 定时).
pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    if conn.execute("DELETE FROM schedules WHERE id = ?1", [id]).map_err(err)? == 0 {
        return Err("这个定时任务已经被删除了".into());
    }
    Ok(())
}

/// The ones to run now, each with the time it runs for, in list order.
pub fn due(conn: &Connection, now: &str) -> Result<Vec<(i64, String)>, String> {
    Ok(rows(conn, "WHERE s.enabled", &[])?
        .into_iter()
        .filter_map(|r| {
            let slot = latest(&r.s.rule, now);
            let start = latest(&r.s.rule, &r.since).max(r.last_slot.unwrap_or_default());
            (slot > start).then_some((r.s.id, slot))
        })
        .collect())
}

/// It ran (or started running) for `slot`.
pub fn mark(conn: &Connection, id: i64, slot: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO schedule_state (schedule_id, last_slot) SELECT id, ?2 FROM schedules WHERE id = ?1 ON CONFLICT (schedule_id) DO UPDATE SET last_slot = excluded.last_slot",
        params![id, slot],
    )
    .map(|_| ())
    .map_err(err)
}

/// 「每周回顾 · 10/19」
fn title(name: &str, now: &str) -> String {
    format!("{name} · {}/{}", now[5..7].trim_start_matches('0'), now[8..10].trim_start_matches('0'))
}

/// Saves the question and an empty reply in a new conversation under 定时.
pub fn begin(conn: &Connection, s: &Schedule, now: &str, model: &str, model_name: &str) -> Result<chat::Begun, String> {
    let tx = Tx::begin(conn)?;
    let b = chat::begin_run_in(&tx, None, &NewSession { thinking_off: !s.thinking, ..Default::default() }, &s.prompt, model, model_name, &Extras::default())?;
    tx.execute(
        "UPDATE sessions SET scheduled = 1, schedule_id = ?1, unseen = 1, title = ?2, title_source = 'user' WHERE id = ?3",
        params![s.id, title(&s.name, now), b.session_id],
    )
    .map_err(err)?;
    tx.commit()?;
    Ok(b)
}

/// A run that couldn't get its model: the conversation says why.
fn failed(conn: &Connection, s: &Schedule, now: &str, model: &str, model_name: &str, error: &str) -> Result<(), String> {
    let b = begin(conn, s, now, model, model_name)?;
    chat::finish(conn, b.run_id, b.reply_id, &Outcome { state: "failed", error: Some(error.into()), ..Default::default() })
}

pub fn seen(conn: &Connection, session: i64) -> Result<(), String> {
    conn.execute("UPDATE sessions SET unseen = 0 WHERE id = ?1 AND unseen", [session]).map(|_| ()).map_err(err)
}

/// Unseen conversations from schedules (the dot on 小拾 in the sidebar).
pub fn unseen(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM sessions WHERE unseen", [], |r| r.get(0)).unwrap_or(0)
}

// ---------- running ----------

/// The schedule running now (one at a time).
static CURRENT: Mutex<Option<i64>> = Mutex::new(None);
/// The monitor's check is going on.
static CHECKING: AtomicBool = AtomicBool::new(false);

fn current() -> Option<i64> {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner())
}

/// A schedule is running (or waiting for its model): a model it started mustn't be stopped by others.
pub fn running() -> bool {
    current().is_some()
}

/// The year a 年度总结 writes about: its time's year (one in January = the year just ended).
pub fn year_of(slot: &str) -> String {
    let y: i64 = slot[..4].parse().unwrap_or(1970);
    if &slot[5..7] == "01" { (y - 1).to_string() } else { y.to_string() }
}

/// Holds the one-at-a-time slot; freed when dropped.
struct Claim;

impl Claim {
    fn take(id: i64) -> Option<Claim> {
        let mut c = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
        if c.is_some() {
            return None;
        }
        *c = Some(id);
        Some(Claim)
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// What a run of `s` sends to: its model (None = the selection). A local one gets started if none
/// is running (returns its key, to stop after); another running local model is used instead, said
/// in the notice.
pub(crate) async fn route(p: &Picky, model: Option<&str>) -> Result<(Route, String, Option<String>), String> {
    let choice = match model {
        Some(m) => Selection::parse(m),
        None => p.selection(),
    };
    match &choice {
        Selection::Cloud(id) => {
            if p.with_db(|c| super::cloud::model(c, id))?.is_none() {
                return Err(if model.is_some() { "这个任务选的云端模型已经被删除了，请在「定时」里换一个。".into() } else { "选的云端模型已经被删除了。".into() });
            }
            Ok((p.route_with(&choice, true)?, String::new(), None))
        }
        Selection::Local => {
            let want = model.and_then(|m| m.strip_prefix("local:"));
            let started = assist::ready_local(p, want).await?;
            let route = p.route_with(&Selection::Local, true)?;
            let used = route.model.strip_prefix("local:").unwrap_or_default();
            let notice = match want {
                Some(w) if w != used => {
                    let name = p.runtime.local_status().models.into_iter().find(|m| m.key == w).map_or(w.to_string(), |m| m.name);
                    format!("（这个任务选的是 {name}，到点时正在运行的是 {}，用了它。）", route.name)
                }
                _ => String::new(),
            };
            Ok((route, notice, started))
        }
    }
}

/// Starts the note (the mock model answers by it).
pub const NOTE_MARK: &str = "【定时任务】";

/// What the model is told besides the question (end of the system message).
pub(crate) fn note(name: &str, rule: &Rule, now: &str) -> String {
    format!("{NOTE_MARK}这是用户设的定时任务「{name}」（{}），{now} 自动运行，用户现在不在：不要反问，直接查数据写好；只能读，不能记录或修改任何东西。", rule.text())
}

/// Runs `id` for `slot` (None = 立即运行, now): writes the conversation (in its own slot, beside
/// whatever else is written; a local model takes turns), then stops a local model it started. 年度总结 (builtin yearly) reads the year month by month (yearly.rs).
async fn run(app: &AppHandle, id: i64, slot: Option<String>) -> Result<(), String> {
    let st = app.state::<AppState>();
    let p = &st.picky;
    let s = p.with_db(|c| get(c, id))?;
    let routed = route(p, s.model.as_deref()).await;
    let at = p.with_db(|c| now(c))?;
    let mut started = None;
    match routed {
        Ok((route, notice, by_us)) => {
            started = by_us;
            let mut b = p.with_db(|c| begin(c, &s, &at, &route.model, &route.name))?;
            b.request.read_only = true;
            b.request.notice = notice;
            if s.builtin.as_deref() == Some(super::yearly::BUILTIN) {
                b.request.yearly = Some(year_of(slot.as_deref().unwrap_or(&at)));
            } else {
                b.request.system = [b.request.system.as_str(), &note(&s.name, &s.rule, &at)].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("\n\n");
            }
            let run_id = b.run_id;
            chat::start(app, b, route, Channel::new(|_| Ok(())));
            while p.runs.holds(run_id) {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        Err(e) => {
            let code = s.model.clone().unwrap_or_else(|| p.selection().code());
            p.with_db(|c| failed(c, &s, &at, &code, &model_name(p, &code), &e))?;
        }
    }
    if let Some(key) = started {
        if !p.runs.using("local") && !super::brief::writing() && p.runtime.local_status().active.is_some_and(|m| m.key == key) {
            p.runtime.stop().await;
        }
    }
    p.changed();
    Ok(())
}

pub(crate) fn model_name(p: &Picky, code: &str) -> String {
    match code.split_once(':') {
        Some(("cloud", id)) => p.with_db(|c| super::cloud::model(c, id)).ok().flatten().map_or(id.to_string(), |m| m.name),
        Some((_, key)) => p.runtime.local_status().models.into_iter().find(|m| m.key == key).map_or(key.to_string(), |m| m.name),
        None => "本地模型".into(),
    }
}

/// The monitor's check (every 30 s): runs what's due, one after another (beside replies being written).
pub async fn check(app: AppHandle) {
    if CHECKING.swap(true, Ordering::SeqCst) {
        return;
    }
    loop {
        let st = app.state::<AppState>();
        if !st.picky.is_open() || current().is_some() {
            break;
        }
        let Some((id, slot)) = st.picky.with_db(|c| due(c, &now(c)?)).ok().and_then(|d| d.into_iter().next()) else { break };
        let Some(claim) = Claim::take(id) else { break };
        let _ = st.picky.with_db(|c| mark(c, id, &slot));
        if let Err(e) = run(&app, id, Some(slot)).await {
            eprintln!("schedule {id} failed: {e}");
        }
        drop(claim);
    }
    CHECKING.store(false, Ordering::SeqCst);
}

// ---------- commands ----------

/// `model` exists and can do what `thinking` asks (None = the selection: taken as it is at run time).
pub(crate) fn check_model(p: &Picky, model: &Option<String>, thinking: bool) -> Result<(), String> {
    let think = match model.as_deref().and_then(|m| m.split_once(':')) {
        Some(("cloud", id)) => p.with_db(|c| super::cloud::model(c, id))?.ok_or("这个云端模型已经被删除了")?.thinking,
        Some(("local", key)) => {
            let models = p.runtime.local_status().models;
            if models.is_empty() {
                return Ok(());
            }
            Think::local(models.iter().find(|m| m.key == key).ok_or("模型清单里没有这个本地模型")?.thinking)
        }
        _ => return Ok(()),
    };
    think_fits(think, thinking)
}

/// 思考 on needs a model that can think; off, one that can do without.
pub fn think_fits(think: Think, thinking: bool) -> Result<(), String> {
    match think.allows(thinking) {
        true => Ok(()),
        false if thinking => Err("这个模型不能思考，换一个能思考的模型，或者关掉「思考」".into()),
        false => Err("这个模型只能思考，换一个能不思考的模型，或者打开「思考」".into()),
    }
}

#[tauri::command]
pub fn picky_schedules(state: State<AppState>) -> Result<Vec<Schedule>, String> {
    state.picky.with_db(|c| list(c, &now(c)?))
}

/// `id` None = a new one. Returns its id.
#[tauri::command]
pub fn picky_schedule_save(state: State<AppState>, id: Option<i64>, input: ScheduleInput) -> Result<i64, String> {
    check_model(&state.picky, &input.model, input.thinking)?;
    match id {
        Some(id) => state.with_picky_undo("修改定时任务", |c| update(c, id, &input).map(|_| id)),
        None => state.with_picky_undo("新建定时任务", |c| create(c, &input)),
    }
}

#[tauri::command]
pub fn picky_schedule_enable(state: State<AppState>, id: i64, enabled: bool) -> Result<(), String> {
    state.with_picky_undo(if enabled { "打开定时任务" } else { "关闭定时任务" }, |c| set_enabled(c, id, enabled))
}

#[tauri::command]
pub fn picky_schedule_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    if current() == Some(id) {
        return Err("这个定时任务正在运行，等它写完再删除。".into());
    }
    state.with_picky_undo("删除定时任务", |c| delete(c, id))
}

/// 立即运行: starts in the background (the model may need starting); the conversation appears under 定时.
#[tauri::command]
pub fn picky_schedule_run(app: AppHandle, id: i64) -> Result<(), String> {
    let st = app.state::<AppState>();
    st.picky.with_db(|c| get(c, id))?;
    let claim = Claim::take(id).ok_or("有一个定时任务正在运行，等它写完再试。")?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run(&app, id, None).await {
            eprintln!("schedule {id} failed: {e}");
        }
        drop(claim);
    });
    Ok(())
}

#[tauri::command]
pub fn picky_seen(state: State<AppState>, session_id: i64) -> Result<(), String> {
    state.picky.with_db(|c| seen(c, session_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::library;
    use crate::picky::store::tests::memory_db;

    fn rule(every: &str, day: Option<i64>, at: &str) -> Rule {
        Rule { every: every.into(), day, month: None, at: at.into() }
    }
    fn input(name: &str, every: &str, day: Option<i64>, at: &str) -> ScheduleInput {
        ScheduleInput { name: name.into(), prompt: "回顾一下".into(), every: every.into(), day, month: None, at: at.into(), model: None, thinking: true }
    }

    #[test]
    fn times_before_and_after_now() {
        // 2026-10-14 is a Wednesday
        let daily = rule("day", None, "10:02");
        assert_eq!(latest(&daily, "2026-10-14 10:01"), "2026-10-13 10:02");
        assert_eq!(latest(&daily, "2026-10-14 10:02"), "2026-10-14 10:02");
        assert_eq!(next(&daily, "2026-10-14 10:02"), "2026-10-15 10:02");
        let monday = rule("week", Some(1), "09:00");
        assert_eq!(latest(&monday, "2026-10-14 08:00"), "2026-10-12 09:00");
        assert_eq!(next(&monday, "2026-10-14 08:00"), "2026-10-19 09:00");
        assert_eq!(latest(&monday, "2026-10-12 08:59"), "2026-10-05 09:00");
        // the 31st in a 30-day month is its last day; February too
        let end = rule("month", Some(31), "21:00");
        assert_eq!(latest(&end, "2026-10-14 12:00"), "2026-09-30 21:00");
        assert_eq!(next(&end, "2027-02-01 00:00"), "2027-02-28 21:00");
        let first = rule("month", Some(1), "09:00");
        assert_eq!(latest(&first, "2027-01-01 08:00"), "2026-12-01 09:00");
        assert_eq!(monday.text(), "每周一 09:00");
        assert_eq!(end.text(), "每月 31 号 21:00");
    }

    #[test]
    fn inputs_are_checked() {
        let ok = checked(&input(" 早报 ", "day", Some(3), "9:5")).unwrap();
        assert_eq!((ok.name.as_str(), ok.day, ok.at.as_str()), ("早报", None, "09:05"));
        assert!(checked(&input("", "day", None, "09:00")).is_err());
        assert!(checked(&input("a", "week", Some(8), "09:00")).is_err());
        assert!(checked(&input("a", "month", None, "09:00")).is_err());
        assert!(checked(&input("a", "day", None, "24:00")).is_err());
        assert!(checked(&input("a", "day", None, "0900")).is_err());
        assert!(checked(&input("a", "year", None, "09:00")).is_err());
        assert!(checked(&ScheduleInput { model: Some("gpt".into()), ..input("a", "day", None, "09:00") }).is_err());
    }

    /// `since` set to a local time (the test's "now" is made up, so it's set directly).
    fn set_since(c: &Connection, id: i64, local: &str) {
        c.execute("UPDATE schedules SET since = strftime('%Y-%m-%dT%H:%M:%fZ', ?1, 'utc') WHERE id = ?2", params![format!("{local}:00"), id]).unwrap();
    }

    #[test]
    fn due_once_for_the_latest_missed_and_never_for_the_past() {
        let c = memory_db();
        // the ready-made two
        let all = list(&c, "2026-10-14 10:00").unwrap();
        assert_eq!(all.iter().map(|s| (s.name.as_str(), s.builtin.as_deref())).collect::<Vec<_>>(), [("每周回顾", Some("weekly")), ("每月回顾", Some("monthly")), ("年度总结", Some("yearly"))]);
        assert_eq!(all[2].next.as_deref(), Some("2026-12-31 21:00"));
        assert_eq!(all[2].rule.text(), "每年 12 月 31 日 21:00");
        assert_eq!(all[0].next.as_deref(), Some("2026-10-19 09:00"));
        for s in &all {
            set_since(&c, s.id, "2026-10-14 10:00");
        }
        // made at 10:00 for 10:02: not before, due at 10:02
        let id = create(&c, &input("两分钟后", "day", None, "10:02")).unwrap();
        set_since(&c, id, "2026-10-14 10:00");
        assert!(due(&c, "2026-10-14 10:01").unwrap().is_empty());
        assert_eq!(due(&c, "2026-10-14 10:02").unwrap(), [(id, "2026-10-14 10:02".to_string())]);
        mark(&c, id, "2026-10-14 10:02").unwrap();
        assert!(due(&c, "2026-10-14 10:30").unwrap().is_empty());
        // closed for three days: once, for the latest
        assert_eq!(due(&c, "2026-10-17 11:00").unwrap(), [(id, "2026-10-17 10:02".to_string())]);
        // the 每周回顾 too, once 周一 09:00 has passed
        let weekly = all[0].id;
        assert_eq!(due(&c, "2026-10-19 09:30").unwrap().iter().map(|d| d.0).collect::<Vec<_>>(), [weekly, id]);
        mark(&c, weekly, "2026-10-19 09:00").unwrap();

        // changing the time only counts from then on; switching on doesn't run what was missed while off
        update(&c, id, &input("两分钟后", "day", None, "09:00")).unwrap();
        set_since(&c, id, "2026-10-17 11:00");
        assert!(due(&c, "2026-10-17 11:05").unwrap().iter().all(|d| d.0 != id));
        assert_eq!(due(&c, "2026-10-18 09:00").unwrap(), [(id, "2026-10-18 09:00".to_string())]);
        set_enabled(&c, id, false).unwrap();
        assert!(due(&c, "2026-10-18 09:00").unwrap().is_empty());
        set_enabled(&c, id, true).unwrap();
        set_since(&c, id, "2026-10-20 12:00");
        assert!(due(&c, "2026-10-20 12:00").unwrap().is_empty());
        // a rename keeps a pending time
        update(&c, id, &ScheduleInput { prompt: "换个说法".into(), ..input("改名了", "day", None, "09:00") }).unwrap();
        assert_eq!(due(&c, "2026-10-21 09:01").unwrap(), [(id, "2026-10-21 09:00".to_string())]);
        delete(&c, id).unwrap();
        assert!(get(&c, id).is_err());
        assert!(delete(&c, id).is_err());
    }

    #[test]
    fn thinking_needs_a_model_that_can_and_goes_with_the_conversation() {
        assert!(think_fits(Think::Switch, true).is_ok() && think_fits(Think::Switch, false).is_ok());
        assert!(think_fits(Think::Always, true).is_ok() && think_fits(Think::Never, false).is_ok());
        assert_eq!(think_fits(Think::Never, true).unwrap_err(), "这个模型不能思考，换一个能思考的模型，或者关掉「思考」");
        assert!(think_fits(Think::Always, false).unwrap_err().contains("只能思考"));
        let c = memory_db();
        // ready-made ones think
        assert!(list(&c, "2026-10-14 10:00").unwrap().iter().all(|s| s.thinking));
        let id = create(&c, &ScheduleInput { thinking: false, ..input("早报", "day", None, "08:00") }).unwrap();
        assert!(!get(&c, id).unwrap().thinking);
        update(&c, id, &input("早报", "day", None, "08:00")).unwrap();
        assert!(get(&c, id).unwrap().thinking);
        update(&c, id, &ScheduleInput { thinking: false, ..input("早报", "day", None, "08:00") }).unwrap();
        let b = begin(&c, &get(&c, id).unwrap(), "2026-10-14 08:00", "local:x", "X").unwrap();
        assert!(!b.request.thinking);
        assert!(!library::library(&c).unwrap().scheduled[0].thinking);
        // a file from before 思考 (format 12) reads as on
        let old: ScheduleInput = serde_json::from_str(r#"{"name":"a","prompt":"b","every":"day","at":"09:00"}"#).unwrap();
        assert!(old.thinking);
    }

    #[test]
    fn runs_write_unseen_conversations_under_scheduled() {
        let c = memory_db();
        let id = create(&c, &input("早报", "day", None, "08:00")).unwrap();
        let s = get(&c, id).unwrap();
        let b = begin(&c, &s, "2026-10-14 08:00", "local:x", "X").unwrap();
        let lib = library::library(&c).unwrap();
        assert_eq!(lib.scheduled.iter().map(|r| (r.id, r.title.as_str(), r.unseen)).collect::<Vec<_>>(), [(b.session_id, "早报 · 10/14", true)]);
        assert!(lib.recent.is_empty());
        assert_eq!(unseen(&c), 1);
        assert_eq!(get(&c, id).unwrap().last_session, Some(b.session_id));
        seen(&c, b.session_id).unwrap();
        assert_eq!(unseen(&c), 0);
        // a failed one says why
        failed(&c, &s, "2026-10-15 08:00", "local:x", "X", "本地模型没能启动：x").unwrap();
        let shown = chat::path(&c, get(&c, id).unwrap().last_session.unwrap()).unwrap();
        assert_eq!((shown[0].content.as_str(), shown[1].state.as_str(), shown[1].error.as_deref()), ("回顾一下", "failed", Some("本地模型没能启动：x")));
        // deleting the schedule keeps its conversations under 定时
        delete(&c, id).unwrap();
        assert_eq!(library::library(&c).unwrap().scheduled.len(), 2);
    }
}
