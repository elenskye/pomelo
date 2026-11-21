//! 系统通知 (v1.0 Phase 6; requirements「总体」「系统通知」): macOS notifications (UserNotifications), four kinds, each
//! its own switch in 设置 → 通知 (pomelo.db setting `notify.kinds`; a kind missing = on). Settings, not undoable.
//! - `todo` 一步一步: undone tasks due today or overdue — the 今天 page's 已过期, and those of 今天 whose 截止 is today
//!   (check-ins and the days inside a 时间段 aren't due).
//! - `jobs` 投石问路: 该跟进 (jobs.rs), longest waiting first.
//! - `days` 来日方长: a reminder day or the day itself (days.rs `today`), and expired 证件 (every day until changed).
//! - `schedule` 定时任务: a run done or failed, right after it (schedule.rs `run`); a stopped one says nothing.
//! The first three: once a day each, at its own time (`notify.times`: kind → HH:MM local; a kind missing there takes
//! `notify.at`, the one time all three shared before 11-16, default 09:00) or the first check after it (Pomelo not
//! running or the Mac asleep then). 定时任务 has no time of its own: it follows each task's. Nothing to say → nothing, and the day still counts as done (`notify.sent`: kind →
//! day). Hidden modules never. The monitor checks every 30 s (youzi::spawn_monitor).
//! - A notification shows up even with Pomelo in front. Clicking it opens what it is about in the main window as
//!   全局搜索 does (event `capture-open`: one record → that record, several → the page); a 定时任务's conversation
//!   opens on the 柚子 page (`capture-chat`), 年度总结 in Atlas.
//! - Posts only from the installed app on the real data dir (or a packaged copy with POMELO_REAL_NOTIFY=1). Dev builds,
//!   tests and scratch data dirs only write what they would post to stderr and never touch UserNotifications (it needs
//!   the .app bundle).

use crate::modules::{days, jobs, notes, todo};
use crate::search::Hit;
use crate::{settings, AppState};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager, State};

const KINDS_KEY: &str = "notify.kinds";
/// The one time all three daily kinds shared (before 11-16): now only what a kind without its own time takes.
const AT_KEY: &str = "notify.at";
const TIMES_KEY: &str = "notify.times";
const SENT_KEY: &str = "notify.sent";
pub const DEFAULT_AT: &str = "09:00";
const REAL_ENV: &str = "POMELO_REAL_NOTIFY";
/// Names listed in one notification; the rest are counted.
const LISTED: usize = 3;
const ERROR_CHARS: usize = 60;
/// Kinds checked once a day, each at its own time (their module ids are the same).
const DAILY: [&str; 3] = ["todo", "jobs", "days"];

static POSTS: OnceLock<bool> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub kind: &'static str,
    pub title: String,
    pub body: String,
    /// What a click opens: `{path, hit}` (as 全局搜索) or `{chat}` (a conversation).
    pub open: Value,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Kinds {
    #[serde(default = "yes")]
    pub todo: bool,
    #[serde(default = "yes")]
    pub jobs: bool,
    #[serde(default = "yes")]
    pub days: bool,
    #[serde(default = "yes")]
    pub schedule: bool,
}

impl Default for Kinds {
    fn default() -> Self {
        Kinds { todo: true, jobs: true, days: true, schedule: true }
    }
}

impl Kinds {
    fn on(&self, kind: &str) -> bool {
        match kind {
            "todo" => self.todo,
            "jobs" => self.jobs,
            "days" => self.days,
            "schedule" => self.schedule,
            _ => false,
        }
    }
}

pub fn kinds(conn: &Connection) -> Kinds {
    settings::get(conn, KINDS_KEY).ok().flatten().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

fn valid_at(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).and_then(|s| check_at(s).ok())
}

/// The time every daily kind without its own takes: the old shared one, else 09:00.
fn base_at(conn: &Connection) -> String {
    valid_at(settings::get(conn, AT_KEY).ok().flatten().as_ref()).unwrap_or_else(|| DEFAULT_AT.into())
}

/// Each daily kind's time (HH:MM).
pub fn times(conn: &Connection) -> BTreeMap<String, String> {
    let own = settings::get(conn, TIMES_KEY).ok().flatten().unwrap_or(Value::Null);
    let base = base_at(conn);
    DAILY.iter().map(|k| (k.to_string(), valid_at(own.get(*k)).unwrap_or_else(|| base.clone()))).collect()
}

fn check_at(s: &str) -> Result<String, String> {
    let ok = s.len() == 5 && s.as_bytes()[2] == b':' && s[..2].parse::<u32>().is_ok_and(|h| h < 24) && s[3..].parse::<u32>().is_ok_and(|m| m < 60) && s.bytes().enumerate().all(|(i, b)| i == 2 || b.is_ascii_digit());
    if ok { Ok(s.into()) } else { Err("时间写成 09:00 这样".into()) }
}

fn sent(conn: &Connection) -> BTreeMap<String, String> {
    settings::get(conn, SENT_KEY).ok().flatten().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

/// The local time, HH:MM.
fn clock(conn: &Connection) -> Result<String, String> {
    conn.query_row("SELECT strftime('%H:%M', 'now', 'localtime')", [], |r| r.get(0)).map_err(|e| e.to_string())
}

fn hit(module: &str, kind: &'static str, id: i64, title: String) -> Value {
    json!(Hit { module: module.into(), kind, id, title, detail: None, parent: None, day: None, amount: None, currency: None, flow: None })
}

fn page(path: &str, one: Option<Value>) -> Value {
    json!({ "path": path, "hit": one })
}

/// 「a、b、c 等 5 件」: the first few, then how many in all.
fn listed(names: &[String], unit: &str) -> String {
    let shown = names.iter().take(LISTED).cloned().collect::<Vec<_>>().join("、");
    if names.len() > LISTED { format!("{shown} 等 {} {unit}", names.len()) } else { shown }
}

fn plain(title: &str) -> String {
    notes::without_icons(title).replace("**", "").trim().to_string()
}

pub fn todo_note(conn: &Connection, today: &str) -> Result<Option<Note>, String> {
    let a = todo::agenda(conn, today)?;
    let due: Vec<&todo::PlacedTask> = a.due.iter().filter(|p| p.task.task_type != todo::TaskType::Checkin && p.task.due_date.as_deref() == Some(today)).collect();
    if due.is_empty() && a.overdue.is_empty() {
        return Ok(None);
    }
    let head = [(due.len(), "今天到期"), (a.overdue.len(), "过期")].iter().filter(|(n, _)| *n > 0).map(|(n, l)| format!("{l} {n} 件")).collect::<Vec<_>>().join(" · ");
    let names: Vec<String> = due.iter().copied().chain(a.overdue.iter()).map(|p| plain(&p.task.title)).collect();
    Ok(Some(Note { kind: "todo", title: "一步一步".into(), body: format!("{head}\n{}", listed(&names, "件")), open: page("/todo", None) }))
}

pub fn jobs_note(conn: &Connection, today: &str) -> Result<Option<Note>, String> {
    let js = jobs::jobs(conn, today)?;
    let mut follow: Vec<&jobs::Job> = js.iter().filter(|j| j.follow_up).collect();
    if follow.is_empty() {
        return Ok(None);
    }
    follow.sort_by_key(|j| std::cmp::Reverse(j.waiting_days));
    let name = |j: &jobs::Job| match &j.position {
        Some(p) => format!("{} · {p}", j.company),
        None => j.company.clone(),
    };
    let names: Vec<String> = follow.iter().map(|j| name(j)).collect();
    let one = (follow.len() == 1).then(|| hit("jobs", "job", follow[0].id, names[0].clone()));
    Ok(Some(Note { kind: "jobs", title: "投石问路".into(), body: format!("{} 份投递该跟进\n{}", follow.len(), listed(&names, "份")), open: page("/jobs", one) }))
}

pub fn days_note(conn: &Connection, today: &str) -> Result<Option<Note>, String> {
    let all = days::list(conn, today)?;
    let due: Vec<&days::Day> = all.iter().filter(|d| d.today || (d.kind == days::Kind::Document && d.days < 0)).collect();
    if due.is_empty() {
        return Ok(None);
    }
    let names: Vec<String> = due.iter().map(|d| format!("{} {}", d.name, days::when(d))).collect();
    let shown = names.iter().take(LISTED).cloned().collect::<Vec<_>>().join(" · ");
    let body = if names.len() > LISTED { format!("{shown}\n还有 {} 个", names.len() - LISTED) } else { shown };
    let one = (due.len() == 1).then(|| hit("days", "day", due[0].id, due[0].name.clone()));
    Ok(Some(Note { kind: "days", title: "来日方长".into(), body, open: page("/days", one) }))
}

fn note_of(conn: &Connection, kind: &str, today: &str) -> Result<Option<Note>, String> {
    match kind {
        "todo" => todo_note(conn, today),
        "jobs" => jobs_note(conn, today),
        "days" => days_note(conn, today),
        _ => Ok(None),
    }
}

/// The once-a-day kinds due at `clock` (HH:MM) on `today` (each from its own time on): each at most once a day,
/// marked as done even with nothing to say.
pub fn daily(conn: &Connection, today: &str, clock: &str) -> Result<Vec<Note>, String> {
    let ks = kinds(conn);
    let ts = times(conn);
    let mut done = sent(conn);
    let todo: Vec<&str> = DAILY.into_iter().filter(|k| ks.on(k) && ts.get(*k).is_some_and(|t| clock >= t.as_str()) && done.get(*k).map(String::as_str) != Some(today)).collect();
    if todo.is_empty() {
        return Ok(vec![]);
    }
    let hidden = crate::youzi::data::hidden(conn);
    let mut out = vec![];
    for k in todo {
        done.insert(k.into(), today.into());
        if hidden.iter().any(|h| h == k) {
            continue;
        }
        if let Some(n) = note_of(conn, k, today)? {
            out.push(n);
        }
    }
    settings::set(conn, SENT_KEY, &json!(done))?;
    Ok(out)
}

/// A 定时任务 run ended: `Ok` written, `Err(why)` not; None = stopped (nothing to say).
pub fn schedule_note(name: &str, session: i64, yearly: bool, outcome: Option<Result<(), String>>) -> Option<Note> {
    let body = match outcome? {
        Ok(()) => format!("「{name}」写好了"),
        Err(e) => {
            let e = e.trim();
            let short: String = e.chars().take(ERROR_CHARS).collect();
            format!("「{name}」没写成：{short}{}", if e.chars().count() > ERROR_CHARS { "…" } else { "" })
        }
    };
    let open = if yearly { page("/atlas", None) } else { json!({ "chat": session }) };
    Some(Note { kind: "schedule", title: "定时任务".into(), body, open })
}

// ---------- posting ----------

/// At launch (main thread): whether this copy posts; if so, the click handler and, with any kind on, the permission
/// (macOS asks once).
pub fn init(app: &AppHandle, data_dir_overridden: bool) {
    let real = (!cfg!(debug_assertions) && !data_dir_overridden) || std::env::var(REAL_ENV).as_deref() == Ok("1");
    let posts = real && mac::bundled();
    let _ = POSTS.set(posts);
    if !posts {
        return;
    }
    let _ = APP.set(app.clone());
    mac::set_delegate();
    if app.state::<AppState>().with_db(|c| Ok(kinds(c))).is_ok_and(|k| k != Kinds { todo: false, jobs: false, days: false, schedule: false }) {
        mac::ask();
    }
}

static APP: OnceLock<AppHandle> = OnceLock::new();

fn posts() -> bool {
    POSTS.get().copied().unwrap_or(false)
}

fn post(n: Note) {
    eprintln!("[notify] {} — {}", n.title, n.body.replace('\n', " / "));
    if posts() {
        mac::post(&n);
    }
}

/// A click: the main window comes forward and opens what the notification is about.
fn clicked(open: &str) {
    let Some(app) = APP.get() else { return };
    let Ok(v) = serde_json::from_str::<Value>(open) else { return };
    crate::tray::show_main(app);
    let _ = match v.get("chat").and_then(Value::as_i64) {
        Some(id) => app.emit_to("main", "capture-chat", id),
        None => app.emit_to("main", "capture-open", v),
    };
}

/// The monitor's check (every 30 s): the once-a-day kinds that are due.
pub fn check(app: &AppHandle) {
    let notes = app.state::<AppState>().with_db(|c| daily(c, &todo::today(c)?, &clock(c)?));
    match notes {
        Ok(ns) => ns.into_iter().for_each(post),
        Err(e) => eprintln!("[notify] {e}"),
    }
}

/// After a 定时任务 run (schedule.rs), unless switched off or 柚子 (Atlas for 年度总结) is hidden.
pub fn schedule_done(app: &AppHandle, name: &str, session: i64, yearly: bool, outcome: Option<Result<(), String>>) {
    let module = if yearly { "atlas" } else { "youzi" };
    let on = app.state::<AppState>().with_db(|c| Ok(kinds(c).schedule && !crate::youzi::data::hidden(c).iter().any(|h| h == module))).unwrap_or(false);
    if let Some(n) = on.then(|| schedule_note(name, session, yearly, outcome)).flatten() {
        post(n);
    }
}

// ---------- commands ----------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifySettings {
    pub kinds: Kinds,
    /// The daily kinds' times (todo, jobs, days → HH:MM).
    pub times: BTreeMap<String, String>,
    /// This copy posts (the installed app); otherwise only the settings are kept.
    pub posts: bool,
    /// macOS lets Pomelo post: None = not asked yet (or this copy doesn't post).
    pub allowed: Option<bool>,
}

#[tauri::command]
pub async fn notify_settings(state: State<'_, AppState>) -> Result<NotifySettings, String> {
    let (kinds, times) = state.with_db(|c| Ok((kinds(c), times(c))))?;
    let allowed = if posts() { tauri::async_runtime::spawn_blocking(mac::allowed).await.ok().flatten() } else { None };
    Ok(NotifySettings { kinds, times, posts: posts(), allowed })
}

#[tauri::command]
pub fn notify_set(state: State<AppState>, kinds: Option<Kinds>, kind: Option<String>, at: Option<String>) -> Result<(), String> {
    state.with_db(|c| {
        if let Some(k) = &kinds {
            settings::set(c, KINDS_KEY, &json!(k))?;
        }
        if let (Some(k), Some(a)) = (&kind, &at) {
            set_time(c, k, a)?;
        }
        Ok(())
    })?;
    if posts() && kinds.is_some() {
        mac::ask();
    }
    Ok(())
}

/// One daily kind's time; the others keep theirs (written out, so a later change of the old shared one can't move them).
pub fn set_time(conn: &Connection, kind: &str, at: &str) -> Result<(), String> {
    if !DAILY.contains(&kind) {
        return Err(format!("没有「{kind}」这种每天的通知"));
    }
    let at = check_at(at.trim())?;
    let mut ts = times(conn);
    ts.insert(kind.into(), at);
    settings::set(conn, TIMES_KEY, &json!(ts))
}

/// 系统设置 → 通知 (where macOS keeps whether Pomelo may post).
#[tauri::command]
pub fn notify_open_system(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url("x-apple.systempreferences:com.apple.Notifications-Settings.extension", None::<&str>).map_err(|e| e.to_string())
}

// ---------- macOS ----------

mod mac {
    use super::Note;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, AllocAnyThread};
    use objc2_foundation::{ns_string, NSBundle, NSDictionary, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
        UNNotificationSettings, UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    const OPEN: &str = "open";

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "PomeloNotifyDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(&self, _center: &UNUserNotificationCenter, _n: &UNNotification, handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>) {
                handler.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List | UNNotificationPresentationOptions::Sound,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(&self, _center: &UNUserNotificationCenter, response: &UNNotificationResponse, handler: &block2::DynBlock<dyn Fn()>) {
                let info = response.notification().request().content().userInfo();
                let open = info.objectForKey(ns_string!("open").as_ref()).and_then(|o| o.downcast::<NSString>().ok()).map(|s| s.to_string());
                if let Some(open) = open {
                    super::clicked(&open);
                }
                handler.call(());
            }
        }
    );

    /// UserNotifications only works from inside the .app (it throws otherwise).
    pub fn bundled() -> bool {
        let b = NSBundle::mainBundle();
        b.bundleIdentifier().is_some() && b.bundlePath().to_string().ends_with(".app")
    }

    fn center() -> Retained<UNUserNotificationCenter> {
        UNUserNotificationCenter::currentNotificationCenter()
    }

    pub fn set_delegate() {
        let d: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(), init] };
        center().setDelegate(Some(ProtocolObject::from_ref(&*d)));
        std::mem::forget(d); // the center holds it weakly; it lives as long as the app
    }

    /// Asks for permission (macOS shows its prompt only the first time).
    pub fn ask() {
        let done = RcBlock::new(|_ok: Bool, _e: *mut NSError| {});
        center().requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &done);
    }

    pub fn allowed() -> Option<bool> {
        let (tx, rx) = mpsc::channel();
        let reply = RcBlock::new(move |s: NonNull<UNNotificationSettings>| {
            let st = unsafe { s.as_ref() }.authorizationStatus();
            let _ = tx.send(if st == UNAuthorizationStatus::NotDetermined { None } else { Some(st != UNAuthorizationStatus::Denied) });
        });
        center().getNotificationSettingsWithCompletionHandler(&reply);
        rx.recv_timeout(Duration::from_secs(3)).ok().flatten()
    }

    pub fn post(n: &Note) {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&n.title));
        content.setBody(&NSString::from_str(&n.body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let open = NSString::from_str(&n.open.to_string());
        let info = NSDictionary::from_slices(&[ns_string!(OPEN)], &[&*open]);
        unsafe { content.setUserInfo(info.cast_unchecked::<AnyObject, AnyObject>()) };
        let id = NSString::from_str(&format!("{}-{}", n.kind, n.open));
        let req = UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        let done = RcBlock::new(|e: *mut NSError| {
            if let Some(e) = unsafe { e.as_ref() } {
                eprintln!("[notify] not posted: {}", e.localizedDescription());
            }
        });
        center().addNotificationRequest_withCompletionHandler(&req, Some(&done));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::modules::todo::{self, TaskPatch};
    use serde_json::json;

    fn task(c: &Connection, title: &str, due: Option<&str>, today: &str) -> i64 {
        let g = todo::create_group(c, "生活").map(|g| g.id).unwrap_or(1);
        let p: TaskPatch = serde_json::from_value(json!({ "groupId": g, "title": title, "dueDate": due })).unwrap();
        todo::create_task_on(c, p, today).unwrap().id
    }

    #[test]
    fn times_of_day() {
        for ok in ["00:00", "09:00", "23:59"] {
            assert_eq!(check_at(ok).unwrap(), ok);
        }
        for bad in ["9:00", "24:00", "09:60", "09-00", "0a:00", "", "09:00 "] {
            assert!(check_at(bad).is_err(), "{bad}");
        }
        let c = memory_db();
        let all = |t: &str| DAILY.iter().map(|k| (k.to_string(), t.to_string())).collect::<BTreeMap<_, _>>();
        assert_eq!(times(&c), all(DEFAULT_AT));
        settings::set(&c, AT_KEY, &json!("bad")).unwrap();
        assert_eq!(times(&c), all(DEFAULT_AT));
        // the old shared time: every kind takes it until it has its own
        settings::set(&c, AT_KEY, &json!("10:30")).unwrap();
        assert_eq!(times(&c), all("10:30"));
        set_time(&c, "jobs", " 08:15").unwrap();
        assert_eq!(times(&c), BTreeMap::from([("days".into(), "10:30".into()), ("jobs".into(), "08:15".into()), ("todo".into(), "10:30".into())]));
        assert!(set_time(&c, "schedule", "08:00").is_err(), "定时任务 follows each task's own time");
        assert!(set_time(&c, "todo", "8:00").is_err());
        assert_eq!(kinds(&c), Kinds::default());
        settings::set(&c, KINDS_KEY, &json!({ "jobs": false })).unwrap();
        assert_eq!(kinds(&c), Kinds { jobs: false, ..Kinds::default() });
    }

    #[test]
    fn todo_counts_due_today_and_overdue_only() {
        let c = memory_db();
        let today = "2026-11-15";
        assert_eq!(todo_note(&c, today).unwrap(), None);
        task(&c, "**交**房租 :pm-star:", Some(today), today);
        task(&c, "写周报", Some("2026-11-13"), today);
        task(&c, "明天的", Some("2026-11-16"), today);
        task(&c, "没日期", None, today);
        let n = todo_note(&c, today).unwrap().unwrap();
        assert_eq!((n.title.as_str(), n.body.as_str()), ("一步一步", "今天到期 1 件 · 过期 1 件\n交房租、写周报"));
        assert_eq!(n.open, json!({ "path": "/todo", "hit": null }));
        for i in 0..3 {
            task(&c, &format!("又一件 {i}"), Some(today), today);
        }
        assert_eq!(todo_note(&c, today).unwrap().unwrap().body, "今天到期 4 件 · 过期 1 件\n交房租、又一件 0、又一件 1 等 5 件");
    }

    #[test]
    fn once_a_day_from_the_time_on_and_never_for_hidden_modules() {
        let c = memory_db();
        let today = "2026-11-15";
        task(&c, "交房租", Some(today), today);
        assert!(daily(&c, today, "08:59").unwrap().is_empty());
        let ns = daily(&c, today, "09:00").unwrap();
        assert_eq!(ns.iter().map(|n| n.kind).collect::<Vec<_>>(), ["todo"]);
        assert!(daily(&c, today, "21:00").unwrap().is_empty(), "once a day");
        // the next day: due again (now overdue); switched off → none
        assert_eq!(daily(&c, "2026-11-16", "10:00").unwrap().len(), 1);
        settings::set(&c, KINDS_KEY, &json!({ "todo": false })).unwrap();
        assert!(daily(&c, "2026-11-17", "10:00").unwrap().is_empty());
        settings::set(&c, KINDS_KEY, &json!({})).unwrap();
        settings::set(&c, crate::youzi::data::HIDDEN_KEY, &json!(["todo"])).unwrap();
        assert!(daily(&c, "2026-11-18", "10:00").unwrap().is_empty());
        settings::set(&c, crate::youzi::data::HIDDEN_KEY, &json!([])).unwrap();
        settings::set(&c, AT_KEY, &json!("20:30")).unwrap();
        assert!(daily(&c, "2026-11-19", "20:29").unwrap().is_empty());
        assert_eq!(daily(&c, "2026-11-19", "20:30").unwrap().len(), 1);
        // each kind at its own time
        let applied = jobs::statuses(&c).unwrap().into_iter().find(|s| s.name == "已投递").unwrap().id;
        let p: jobs::JobPatch = serde_json::from_value(json!({ "company": "柚子科技", "steps": [{ "statusId": applied, "done": true, "doneOn": "2026-10-01" }] })).unwrap();
        jobs::create_job(&c, p, "2026-11-20").unwrap();
        set_time(&c, "todo", "07:00").unwrap();
        set_time(&c, "jobs", "18:00").unwrap();
        assert_eq!(daily(&c, "2026-11-20", "07:30").unwrap().iter().map(|n| n.kind).collect::<Vec<_>>(), ["todo"]);
        assert!(daily(&c, "2026-11-20", "17:59").unwrap().is_empty());
        assert_eq!(daily(&c, "2026-11-20", "18:00").unwrap().iter().map(|n| n.kind).collect::<Vec<_>>(), ["jobs"]);
    }

    #[test]
    fn jobs_and_days() {
        let c = memory_db();
        let today = "2026-11-15";
        assert_eq!(jobs_note(&c, today).unwrap(), None);
        assert_eq!(days_note(&c, today).unwrap(), None);
        let applied = jobs::statuses(&c).unwrap().into_iter().find(|s| s.name == "已投递").unwrap().id;
        for (company, on) in [("柚子科技", "2026-10-20"), ("青柠", "2026-11-10")] {
            let p: jobs::JobPatch = serde_json::from_value(json!({ "company": company, "position": "产品", "steps": [{ "statusId": applied, "done": true, "doneOn": on }] })).unwrap();
            jobs::create_job(&c, p, today).unwrap();
        }
        let n = jobs_note(&c, today).unwrap().unwrap();
        assert_eq!(n.body, "1 份投递该跟进\n柚子科技 · 产品");
        assert_eq!(n.open["hit"]["module"], "jobs");
        let day = |name: &str, kind: &str, date: &str, remind: Vec<i64>| {
            let p: days::DayPatch = serde_json::from_value(json!({ "kind": kind, "name": name, "date": date, "remind": remind })).unwrap();
            days::create(&c, p, today).unwrap();
        };
        day("妈妈生日", "birthday", "--11-20", vec![5]);
        day("护照", "document", "2026-11-12", vec![30]);
        day("周年", "day", "2025-12-01", vec![7]);
        let n = days_note(&c, today).unwrap().unwrap();
        assert_eq!(n.body, "护照 已过期 3 天 · 妈妈生日 还有 5 天");
        assert_eq!(n.open["hit"], Value::Null);
    }

    #[test]
    fn schedule_runs() {
        assert_eq!(schedule_note("每周回顾", 7, false, None), None);
        let n = schedule_note("每周回顾", 7, false, Some(Ok(()))).unwrap();
        assert_eq!((n.body.as_str(), n.open.clone()), ("「每周回顾」写好了", json!({ "chat": 7 })));
        let n = schedule_note("年度总结", 8, true, Some(Err("连不上模型".repeat(20)))).unwrap();
        assert!(n.body.starts_with("「年度总结」没写成：连不上模型") && n.body.ends_with('…'));
        assert_eq!(n.open, json!({ "path": "/atlas", "hit": null }));
    }
}
