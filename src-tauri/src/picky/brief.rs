//! 今日简报 (v0.4 phase 8; requirements「小拾 · 定时任务、今日简报、年度总结」):
//! - A card at the top of 首页: a few sentences 小拾 writes the first time 首页 is shown each day
//!   (the page asks for it), kept for the day; 「重写」 writes it again. On / off and its model are set
//!   in 管理 → 定时 (preferences `brief.enabled`, default on; `brief.model`, None = the selection;
//!   `brief.thinking`, default on: the model must be able to, as for 定时任务).
//! - Only: 今天的待办、快到期的（3 天内）和过期的；今天该提醒的日子（提前提醒的那几天和当天）；该跟进
//!   的投递、在笔试面试的；最近两周的笔记里常在想什么. Rust reads them first (data.rs `brief_parts`, each part capped) and sends them in one request
//!   (thinking as set, with room for it). It writes nothing but the brief.
//! - Model as 定时任务 (schedule.rs `route`): a stopped local model is started and stopped after (only
//!   when it started it and nothing else is using it); another running local model is used; cloud
//!   rules (modules not sent to cloud are left out and named under the card; hidden ones quietly).
//! - One per local day in picky.db `briefs`; a rewrite keeps the old text until the new one is done,
//!   a failure keeps it too and says why. Not undoable, not exported. Doesn't take the reply slot
//!   (a local server queues requests).

use super::data::{self, Part};
use super::provider::{self, Event, Msg, Options};
use super::store::{pref_get, pref_set, NOW};
use super::{schedule, Picky};
use crate::modules::common::err;
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

/// Starts the brief's system message (the mock model answers by it).
pub const MARK: &str = "【今日简报】";
const ENABLED: &str = "brief.enabled";
const MODEL: &str = "brief.model";
const THINKING: &str = "brief.thinking";
/// Tokens each part (待办, 日子, 投递, 笔记) may take.
const PART_BUDGET: i64 = 1200;
const MAX_TOKENS: i64 = 800;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Brief {
    pub enabled: bool,
    /// local:<key> | cloud:<id>; None = the selection.
    pub model: Option<String>,
    pub thinking: bool,
    pub day: String,
    /// The text (the last good one while rewriting or after a failure).
    pub content: String,
    /// none | writing | done | failed
    pub state: String,
    pub error: Option<String>,
    pub model_name: Option<String>,
    /// Modules not read (not sent to this cloud model).
    pub left_out: Vec<String>,
    /// While writing: starting (the local model) | writing.
    pub phase: Option<&'static str>,
}

static WRITING: AtomicBool = AtomicBool::new(false);
static PHASE: Mutex<Option<&'static str>> = Mutex::new(None);

/// A brief is being written (a local model it started mustn't be stopped by others).
pub fn writing() -> bool {
    WRITING.load(Ordering::SeqCst)
}

fn set_phase(p: Option<&'static str>) {
    *PHASE.lock().unwrap_or_else(|e| e.into_inner()) = p;
}

/// (on, model, thinking)
pub fn settings(conn: &Connection) -> (bool, Option<String>, bool) {
    (
        pref_get::<bool>(conn, ENABLED).ok().flatten().unwrap_or(true),
        pref_get::<Option<String>>(conn, MODEL).ok().flatten().flatten(),
        pref_get::<bool>(conn, THINKING).ok().flatten().unwrap_or(true),
    )
}

pub fn set_settings(conn: &Connection, enabled: bool, model: Option<&str>, thinking: bool) -> Result<(), String> {
    pref_set(conn, ENABLED, &enabled)?;
    pref_set(conn, MODEL, &model)?;
    pref_set(conn, THINKING, &thinking)
}

/// Today's brief as the card shows it. A row left `writing` by a quit app reads as failed.
pub fn get(conn: &Connection, day: &str) -> Result<Brief, String> {
    let (enabled, model, thinking) = settings(conn);
    let row: Option<(String, String, Option<String>, Option<String>, Option<String>)> = conn
        .query_row("SELECT content, state, error, model_name, left_out FROM briefs WHERE day = ?1", [day], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .optional()
        .map_err(err)?;
    let mut b = Brief { enabled, model, thinking, day: day.into(), content: String::new(), state: "none".into(), error: None, model_name: None, left_out: vec![], phase: None };
    if let Some((content, state, error, model_name, left_out)) = row {
        let stale = state == "writing" && !writing();
        b = Brief {
            content,
            state: if stale { "failed".into() } else { state },
            error: if stale { Some("上次没写完。".into()) } else { error },
            model_name,
            left_out: left_out.and_then(|l| serde_json::from_str(&l).ok()).unwrap_or_default(),
            ..b
        };
    }
    if b.state == "writing" {
        b.phase = *PHASE.lock().unwrap_or_else(|e| e.into_inner());
    }
    Ok(b)
}

/// Whether to write now: none yet, or a rewrite (`force`), or one a quit app left unfinished.
fn needs(b: &Brief, force: bool) -> bool {
    b.enabled && !writing() && (force || b.state == "none" || (b.state == "failed" && b.error.as_deref() == Some("上次没写完。")))
}

fn mark_writing(conn: &Connection, day: &str) -> Result<(), String> {
    conn.execute(
        &format!("INSERT INTO briefs (day, state) VALUES (?1, 'writing') ON CONFLICT (day) DO UPDATE SET state = 'writing', error = NULL, updated_at = {NOW}"),
        [day],
    )
    .map(|_| ())
    .map_err(err)
}

fn save_done(conn: &Connection, day: &str, content: &str, model: &str, model_name: &str, left_out: &[&str]) -> Result<(), String> {
    conn.execute(
        &format!("UPDATE briefs SET content = ?2, state = 'done', error = NULL, model = ?3, model_name = ?4, left_out = ?5, updated_at = {NOW} WHERE day = ?1"),
        params![day, content, model, model_name, serde_json::to_string(left_out).map_err(err)?],
    )
    .map(|_| ())
    .map_err(err)
}

fn save_failed(conn: &Connection, day: &str, error: &str) -> Result<(), String> {
    conn.execute(&format!("UPDATE briefs SET state = 'failed', error = ?2, updated_at = {NOW} WHERE day = ?1"), params![day, error]).map(|_| ()).map_err(err)
}

const WEEKDAYS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

/// What each part is for in the brief.
fn topic(title: &str) -> &'static str {
    match title {
        "一步一步" => "今天要做的、快到期的和过期的待办",
        "投石问路" => "该跟进的投递、正在笔试面试的",
        "来日方长" => "快到的日子（还有几天，或者就是今天）",
        _ => "最近两周的笔记里常在想什么（一两句，像朋友一样提一下）",
    }
}

/// The request: what to write (only the parts it got), then what was read.
pub fn prompt(parts: &[Part], today: &str, instructions: &str) -> Vec<Msg> {
    let wd = WEEKDAYS[(crate::modules::common::weekday(today) as usize).clamp(1, 7) - 1];
    let topics: Vec<String> = parts.iter().map(|p| format!("{}（看【{}】）", topic(&p.title), p.title)).collect();
    let system = format!(
        "{MARK}你是小拾，用户自己的生活记录 App「柚儿园」里的助手，在帮用户写今天的简报。今天是 {today}（周{wd}）。\n\
         - 根据下面读到的数据写 3～6 句话，可以分两三小段；称呼用户「你」；不要标题、不要开场白和客套话。\n\
         - 只写这{}件事：{}。没给的就一个字都不要提（比如没给笔记，就不要说笔记）。\n\
         - 提到具体的事照抄它的名称；不要编，也不要建议用户去记录什么。\n\
         - 用户的指令只管语气：{}",
        ["零", "一", "两", "三", "四"][topics.len().min(4)],
        if topics.is_empty() { "（没有可读的数据，就写一句今天没什么要提醒的）".to_string() } else { topics.join("；") },
        if instructions.trim().is_empty() { "没有。" } else { instructions.trim() }
    );
    let read: Vec<String> = parts.iter().map(|p| format!("【{}】\n{}", p.title, p.text)).collect();
    let user = if read.is_empty() { "（这次什么数据都不能读。）".to_string() } else { read.join("\n\n") };
    vec![Msg::new("system", &system), Msg::new("user", &format!("{user}\n\n请写今天的简报。"))]
}

/// How long one whole answer may take (a busy cloud service may only send keep-alives for ~15 min).
const ANSWER_LIMIT: std::time::Duration = std::time::Duration::from_secs(300);

/// One answer, whole, within `ANSWER_LIMIT` (`max_tokens` already has room for thinking).
pub(crate) async fn ask(target: &provider::Target, messages: Vec<Msg>, max_tokens: i64, thinking: bool) -> Result<(String, Option<(i64, i64)>), String> {
    let (mut out, mut usage) = (String::new(), None);
    let streamed = provider::stream(target.clone(), messages, Options { max_tokens: Some(max_tokens), thinking: Some(thinking), ..Default::default() }, |e| match e {
        Event::Content(t) => out.push_str(&t),
        Event::Usage { prompt, completion } => usage = Some((prompt, completion)),
        _ => {}
    });
    match tokio::time::timeout(ANSWER_LIMIT, streamed).await {
        Ok(r) => r?,
        Err(_) => return Err("模型 5 分钟都没写完（云端可能正忙），请稍后再试。".into()),
    }
    Ok((out.trim().to_string(), usage))
}

/// Writes `day`'s brief (the row is already `writing`).
async fn write(app: &AppHandle, day: &str) -> Result<(), String> {
    let st = app.state::<AppState>();
    let p: &Picky = &st.picky;
    let (_, model, thinking) = p.with_db(|c| Ok(settings(c)))?;
    let local = match &model {
        Some(m) => m.starts_with("local:"),
        None => p.selection() == super::Selection::Local,
    };
    if local && p.runtime.ready_model().is_none() {
        set_phase(Some("starting"));
    }
    let (route, _, started) = schedule::route(p, model.as_deref()).await?;
    set_phase(Some("writing"));
    let result = async {
        let (access, today) = super::chat::access_for(app, &route).ok_or("读不到柚儿园的数据。")?;
        let (parts, left_out) = st.with_db(|c| data::brief_parts(c, &access, &today, PART_BUDGET))?;
        let instructions = p.with_db(|c| super::instructions::global(c)).unwrap_or_default();
        let max = if thinking { MAX_TOKENS + super::context::think_room(route.window) } else { MAX_TOKENS };
        let (text, _) = ask(&route.target, prompt(&parts, &today, &instructions), max, thinking).await?;
        if text.is_empty() {
            return Err("模型没有写出内容。".to_string());
        }
        p.with_db(|c| save_done(c, day, &text, &route.model, &route.name, &left_out))
    }
    .await;
    if let Some(key) = started {
        if !p.runs.using("local") && !schedule::running() && p.runtime.local_status().active.is_some_and(|m| m.key == key) {
            p.runtime.stop().await;
        }
    }
    result
}

/// Starts writing today's brief when needed (`force`: 重写 / 重试). Returns at once.
fn start(app: &AppHandle, force: bool) -> Result<Brief, String> {
    let st = app.state::<AppState>();
    let day = st.with_db(|c| crate::modules::todo::today(c))?;
    let b = st.picky.with_db(|c| get(c, &day))?;
    if force && !b.enabled {
        return Err("今日简报关着（小拾 → 管理 → 定时）。".into());
    }
    if !needs(&b, force) || WRITING.swap(true, Ordering::SeqCst) {
        return Ok(b);
    }
    if let Err(e) = st.picky.with_db(|c| mark_writing(c, &day)) {
        WRITING.store(false, Ordering::SeqCst);
        return Err(e);
    }
    let app2 = app.clone();
    let day2 = day.clone();
    tauri::async_runtime::spawn(async move {
        let r = write(&app2, &day2).await;
        let p = &app2.state::<AppState>().picky;
        if let Err(e) = r {
            let _ = p.with_db(|c| save_failed(c, &day2, &e));
        }
        set_phase(None);
        WRITING.store(false, Ordering::SeqCst);
        p.changed();
    });
    st.picky.with_db(|c| get(c, &day))
}

#[tauri::command]
pub fn picky_brief(state: State<AppState>) -> Result<Brief, String> {
    let day = state.with_db(|c| crate::modules::todo::today(c))?;
    state.picky.with_db(|c| get(c, &day))
}

/// 首页 asks on showing (`force` false: only when there's none today); 重写 / 重试 force it.
#[tauri::command]
pub fn picky_brief_write(app: AppHandle, force: bool) -> Result<Brief, String> {
    start(&app, force)
}

/// 管理 → 定时: on / off, the model (None = the selection) and 思考.
#[tauri::command]
pub fn picky_brief_set(state: State<AppState>, enabled: bool, model: Option<String>, thinking: Option<bool>) -> Result<Brief, String> {
    let model = model.filter(|m| !m.trim().is_empty());
    let thinking = thinking.unwrap_or(true);
    schedule::check_model(&state.picky, &model, thinking)?;
    let day = state.with_db(|c| crate::modules::todo::today(c))?;
    state.picky.with_db(|c| {
        set_settings(c, enabled, model.as_deref(), thinking)?;
        get(c, &day)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::store::tests::memory_db;

    #[test]
    fn one_a_day_rewrites_keep_the_old_text_and_settings_default_on() {
        let c = memory_db();
        assert_eq!(settings(&c), (true, None, true));
        let b = get(&c, "2026-10-15").unwrap();
        assert_eq!((b.state.as_str(), b.content.as_str()), ("none", ""));
        assert!(needs(&b, false));
        mark_writing(&c, "2026-10-15").unwrap();
        // not being written in this process: a quit app left it, so it's written again
        let b = get(&c, "2026-10-15").unwrap();
        assert_eq!((b.state.as_str(), b.error.as_deref()), ("failed", Some("上次没写完。")));
        assert!(needs(&b, false));
        save_done(&c, "2026-10-15", "今天有两件事。", "local:x", "X", &["投石问路"]).unwrap();
        let b = get(&c, "2026-10-15").unwrap();
        assert_eq!((b.state.as_str(), b.left_out.clone()), ("done", vec!["投石问路".to_string()]));
        assert!(!needs(&b, false) && needs(&b, true));
        // a rewrite that fails keeps the text
        mark_writing(&c, "2026-10-15").unwrap();
        save_failed(&c, "2026-10-15", "本地模型没能启动").unwrap();
        let b = get(&c, "2026-10-15").unwrap();
        assert_eq!((b.state.as_str(), b.content.as_str(), b.error.as_deref()), ("failed", "今天有两件事。", Some("本地模型没能启动")));
        assert!(!needs(&b, false));
        // another day starts empty; off = never written
        assert_eq!(get(&c, "2026-10-16").unwrap().state, "none");
        set_settings(&c, false, Some("cloud:deepseek-flash"), false).unwrap();
        let b = get(&c, "2026-10-16").unwrap();
        assert_eq!((b.enabled, b.model.as_deref(), b.thinking), (false, Some("cloud:deepseek-flash"), false));
        assert!(!needs(&b, true));
    }

    #[test]
    fn the_brief_reads_three_parts_under_the_models_rules() {
        let pomelo = crate::db::tests::memory_db();
        data::tests::seed_jobs(&pomelo);
        let today = data::tests::TODAY;
        // no notes in two weeks: no part for them
        let (parts, _) = data::brief_parts(&pomelo, &data::Access::default(), today, PART_BUDGET).unwrap();
        assert_eq!(parts.len(), 2);
        pomelo.execute("INSERT INTO notes (content, created_at) VALUES ('又在想要不要换城市 #生活', '2026-10-10T02:00:00.000Z')", []).unwrap();
        let (parts, left) = data::brief_parts(&pomelo, &data::Access::default(), today, PART_BUDGET).unwrap();
        assert_eq!(parts.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["一步一步", "投石问路", "念念不忘"]);
        assert!(left.is_empty());
        assert!(parts[2].text.contains("又在想要不要换城市"));
        // cloud without 投石问路; 念念不忘 hidden: quietly left out
        let cloud = data::Access { cloud: true, cloud_allowed: vec!["todo".into(), "notes".into()], hidden: vec!["notes".into()], ..Default::default() };
        let (parts, left) = data::brief_parts(&pomelo, &cloud, today, PART_BUDGET).unwrap();
        assert_eq!((parts.len(), left), (1, vec!["来日方长", "投石问路"]));
        let msgs = prompt(&parts, today, "叫我小柚");
        assert!(msgs[0].content.starts_with(MARK) && msgs[0].content.contains("叫我小柚"));
        assert!(msgs[1].content.contains("【一步一步】"));
        // the mock model answers it
        let (text, _) = tauri::async_runtime::block_on(ask(&provider::Target::Mock { thinking: false }, msgs, MAX_TOKENS, true)).unwrap();
        assert!(text.contains("模拟简报"));
    }

    /// The real brief on the local model (ask first), on the import files, today = 2026-10-12 or
    /// POMELO_TODAY: POMELO_REAL_LLM=1 [POMELO_MODEL=qwen3.5-9b]. POMELO_SHOW=1 prints what it read.
    /// Cloud models are the user's to test (in the installed app).
    #[test]
    #[ignore]
    fn real_brief() {
        if std::env::var("POMELO_REAL_LLM").as_deref() != Ok("1") {
            return;
        }
        let (target, access) = (provider::Target::Local { model: std::env::var("POMELO_MODEL").unwrap_or_else(|_| "qwen3.5-9b".into()) }, data::Access::default());
        let pomelo = crate::picky::chat::tests::real_pomelo();
        let today = std::env::var("POMELO_TODAY").unwrap_or_else(|_| data::tests::TODAY.into());
        let (parts, left) = data::brief_parts(&pomelo, &access, &today, PART_BUDGET).unwrap();
        if std::env::var("POMELO_SHOW").is_ok() {
            for p in &parts {
                println!("【{}】\n{}\n", p.title, p.text);
            }
        }
        let t = std::time::Instant::now();
        let r = tauri::async_runtime::block_on(ask(&target, prompt(&parts, &today, ""), MAX_TOKENS + 8192, true));
        println!("==== 今日简报 ({:.1}s, left out {left:?})\n{r:?}", t.elapsed().as_secs_f64());
        if let Ok((text, _)) = r {
            println!("\n{text}");
        }
    }
}
