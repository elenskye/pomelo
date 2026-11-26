//! PikPak (1.1.0 Phase 3; requirements「DevJanitor › 备份」): the whole external drive (VAULT and the system's
//! junk left out) mirrored to a PikPak folder with rclone (rclone.rs), in the background while the drive is in.
//! - Settings (`Settings::pikpak`, with the backup's; not undone): the account shown (the login itself is in
//!   rclone's encrypted config), the PikPak folder (default: the drive's name), the upload limit.
//! - When: the drive appears (the watcher), a backup run ends, a backup card changed the drive, I log in, change a
//!   setting or press 接着传. Not while a backup run copies (that stops it; it starts again after), nor after I
//!   pressed 停下 until the drive comes back or I press 接着传. One sync at a time; a change meanwhile means once
//!   more after it.
//! - A sync that would delete more than `rclone::MAX_DELETE` stops; the next one waits for 照样同步 (`force`).
//! - Each sync that moved something (or failed) is a `backup_runs` row with `dest` 'pikpak' (`note`: what went to
//!   PikPak's trash, why it stopped). The App quitting stops rclone; the next sync carries on.

use super::super::{Janitor, Mode};
use super::rclone::{self, Done, Progress, Rclone};
use super::{drive_of, load, store_settings, Settings};
use crate::AppState;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PikPak {
    /// The account logged in (None: not logged in).
    pub account: Option<String>,
    /// The PikPak folder the drive goes to (empty: the drive's name).
    pub folder: String,
    /// Upload limit in MB/s (0: none).
    pub limit: u32,
}

impl PikPak {
    pub fn folder_for(&self, drive: &str) -> String {
        if self.folder.trim().is_empty() { drive.to_string() } else { self.folder.clone() }
    }
}

/// The PikPak side of `Backup` (in memory).
#[derive(Default)]
pub struct Cloud {
    running: AtomicBool,
    run: Mutex<Arc<Progress>>,
    ended: Mutex<Option<String>>,
    /// 停下 pressed: no sync until the drive comes back or 接着传.
    paused: AtomicBool,
    /// Something changed during a sync: once more after it.
    again: AtomicBool,
    /// The last sync stopped at too many deletions: waiting for 照样同步.
    held: AtomicBool,
    /// Stopped for a backup run: starts again after it.
    yielded: AtomicBool,
    quota: Mutex<Option<(u64, u64)>>,
}

impl Cloud {
    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// The config's password: the keychain's (sandbox: rclone.rs's own).
fn rclone_of(state: &AppState, j: &Janitor) -> Result<Rclone, String> {
    let pass = if j.mode == Mode::Real { state.youzi.keys.read(rclone::KEY).ok() } else { None };
    Rclone::new(j, pass)
}

fn local_day(state: &AppState) -> String {
    state.youzi.with_db(|c| Ok(c.query_row("SELECT date('now', 'localtime')", [], |r| r.get::<_, String>(0)).unwrap_or_default())).unwrap_or_default()
}

// ---- runs (youzi.db backup_runs, dest 'pikpak') ----

pub(super) fn record_start(c: &Connection, drive: &str) -> Result<i64, String> {
    c.execute("INSERT INTO backup_runs (dest, drive) VALUES ('pikpak', ?1)", [drive]).map_err(crate::modules::common::err)?;
    Ok(c.last_insert_rowid())
}

/// The row as it ended; a sync that changed nothing and went fine leaves none.
pub(super) fn record_end(c: &Connection, id: i64, p: &Progress, d: &Done) -> Result<(), String> {
    let (files, bytes, deletes, renames) = (p.files.load(Ordering::Relaxed), p.bytes.load(Ordering::Relaxed), p.deletes.load(Ordering::Relaxed), p.renames.load(Ordering::Relaxed));
    if d.ok && files == 0 && deletes == 0 && renames == 0 && d.errors.is_empty() {
        c.execute("DELETE FROM backup_runs WHERE id = ?1", [id]).map_err(crate::modules::common::err)?;
        return Ok(());
    }
    let status = if d.ok { "done" } else if d.stopped { "stopped" } else { "failed" };
    let mut note: Vec<String> = Vec::new();
    if renames > 0 {
        note.push(format!("在 PikPak 上挪了 {renames} 个"));
    }
    if deletes > 0 {
        note.push(format!("挪进 PikPak 回收站 {deletes} 个"));
    }
    if d.too_many_deletes {
        note.push(format!("要挪进回收站的超过 {} 个，没接着同步", rclone::MAX_DELETE));
    }
    if let Some(f) = &d.fatal {
        note.push(f.clone());
    }
    let at: String = c.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |r| r.get(0)).map_err(crate::modules::common::err)?;
    c.execute(
        "UPDATE backup_runs SET status = ?2, files = ?3, bytes = ?4, errors = ?5, note = ?6, finished_at = ?7 WHERE id = ?1",
        params![id, status, files as i64, bytes as i64, serde_json::to_string(&d.errors).map_err(|e| e.to_string())?, (!note.is_empty()).then(|| note.join("；")), at],
    )
    .map(|_| ())
    .map_err(crate::modules::common::err)
}

/// One line for the page about how a sync ended.
pub(super) fn ended_text(p: &Progress, d: &Done) -> String {
    let (files, bytes, deletes, renames) = (p.files.load(Ordering::Relaxed), p.bytes.load(Ordering::Relaxed), p.deletes.load(Ordering::Relaxed), p.renames.load(Ordering::Relaxed));
    let moved = if files > 0 { format!("传了 {files} 个（{}）", super::gb(bytes)) } else { String::new() };
    let renamed = if renames > 0 { format!("在 PikPak 上挪了 {renames} 个") } else { String::new() };
    let head = if d.too_many_deletes {
        format!("要挪进 PikPak 回收站的超过 {} 个，先停下了", rclone::MAX_DELETE)
    } else if let Some(f) = &d.fatal {
        f.clone()
    } else if d.stopped {
        if moved.is_empty() { "停下了".into() } else { format!("停下了：{moved}") }
    } else if d.ok && files == 0 && deletes == 0 && renames == 0 {
        "和硬盘上一样".into()
    } else if d.ok {
        [moved, renamed, if deletes > 0 { format!("挪进回收站 {deletes} 个") } else { String::new() }].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("，")
    } else {
        format!("没传完{}", if moved.is_empty() { String::new() } else { format!("：{moved}") })
    };
    if d.errors.is_empty() { head } else { format!("{head}；{} 个没成", d.errors.len()) }
}

// ---- when ----

/// Starts a sync when it may (or asks for one more after the running one).
pub fn kick(app: &AppHandle) {
    let j = app.state::<Janitor>().inner().clone();
    if j.backup.cloud.running() {
        j.backup.cloud.again.store(true, Ordering::SeqCst);
        return;
    }
    start(app, false);
}

/// The drive came back: 停下 no longer holds.
pub fn plugged(app: &AppHandle) {
    app.state::<Janitor>().backup.cloud.paused.store(false, Ordering::SeqCst);
    kick(app);
}

/// A backup run starts: the sync gives way and starts again after.
pub fn yield_to_backup(j: &Janitor) {
    if j.backup.cloud.running() {
        j.backup.cloud.yielded.store(true, Ordering::SeqCst);
        stop_now(j);
    }
}

/// A backup run ended: carry on.
pub fn after_backup(app: &AppHandle) {
    app.state::<Janitor>().backup.cloud.yielded.store(false, Ordering::SeqCst);
    kick(app);
}

fn stop_now(j: &Janitor) {
    if let Ok(r) = j.backup.cloud.run.lock() {
        r.kill();
    }
}

/// Why a sync can't start now (None: it can).
fn blocked(j: &Janitor, s: &Settings) -> Option<&'static str> {
    let c = &j.backup.cloud;
    if s.pikpak.account.is_none() {
        Some("没登录")
    } else if drive_of(j, s).is_none() {
        Some("没插硬盘")
    } else if j.backup.running() || c.yielded.load(Ordering::SeqCst) {
        Some("正在备份")
    } else if c.paused.load(Ordering::SeqCst) {
        Some("停下了")
    } else if c.held.load(Ordering::SeqCst) {
        Some("等你确认")
    } else if rclone::binary().is_none() {
        Some("没装 rclone")
    } else {
        None
    }
}

/// Syncs in the background, again while something changed meanwhile. `force`: past the deletion limit.
fn start(app: &AppHandle, force: bool) {
    let j = app.state::<Janitor>().inner().clone();
    if blocked(&j, &load(&app.state::<AppState>())).is_some() || j.backup.cloud.running.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut force = force;
        loop {
            let st = app.state::<AppState>();
            let s = load(&st);
            if blocked(&j, &s).is_some() {
                break;
            }
            j.backup.cloud.again.store(false, Ordering::SeqCst);
            once(&st, &j, &s, force);
            force = false;
            if !j.backup.cloud.again.load(Ordering::SeqCst) {
                break;
            }
        }
        j.backup.cloud.running.store(false, Ordering::SeqCst);
        // a backup run that started meanwhile hands back when it ends
    });
}

/// One sync, recorded.
fn once(st: &AppState, j: &Janitor, s: &Settings, force: bool) {
    let Some(drive) = drive_of(j, s) else { return };
    let p = Arc::new(Progress::new());
    if let Ok(mut g) = j.backup.cloud.run.lock() {
        *g = p.clone();
    }
    if let Ok(mut g) = j.backup.cloud.ended.lock() {
        *g = None;
    }
    let rc = match rclone_of(st, j) {
        Ok(r) => r,
        Err(e) => {
            if let Ok(mut g) = j.backup.cloud.ended.lock() {
                *g = Some(e);
            }
            return;
        }
    };
    let id = st.youzi.with_db(|c| record_start(c, &drive.name)).ok();
    let d = rc.sync(&drive.path, &s.pikpak.folder_for(&drive.name), &s.skipped, s.pikpak.limit, force, &local_day(st), &p);
    if let Some(id) = id {
        let _ = st.youzi.with_db(|c| record_end(c, id, &p, &d));
    }
    j.backup.cloud.held.store(d.too_many_deletes, Ordering::SeqCst);
    if !d.stopped {
        if let Ok(q) = rc.about() {
            if let Ok(mut g) = j.backup.cloud.quota.lock() {
                *g = Some(q);
            }
        }
    }
    if let Ok(mut g) = j.backup.cloud.ended.lock() {
        *g = Some(ended_text(&p, &d));
    }
}

/// The App quits: rclone goes too (the next sync carries on).
pub fn on_exit(app: &AppHandle) {
    stop_now(app.state::<Janitor>().inner());
}

// ---- what the page gets ----

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PikPakView {
    /// rclone is there.
    pub installed: bool,
    pub account: Option<String>,
    pub folder: String,
    pub limit: u32,
    pub running: bool,
    /// Comparing the two sides before uploading.
    pub checking: bool,
    pub bytes: u64,
    pub total_bytes: u64,
    pub files: u64,
    pub total_files: u64,
    pub deletes: u64,
    pub speed: u64,
    pub eta: u64,
    pub current: Vec<String>,
    /// How the last sync ended (this session).
    pub ended: Option<String>,
    /// Why none runs now.
    pub waiting: Option<String>,
    /// Too many deletions: 照样同步 lets it go on.
    pub held: bool,
    pub used: Option<u64>,
    pub total: Option<u64>,
}

pub fn view(j: &Janitor, s: &Settings) -> PikPakView {
    let c = &j.backup.cloud;
    let p = c.run.lock().map(|g| g.clone()).unwrap_or_default();
    let running = c.running();
    let quota = c.quota.lock().ok().and_then(|g| *g);
    let n = |a: &std::sync::atomic::AtomicU64| a.load(Ordering::Relaxed);
    let drive = s.drive.as_ref().map(|d| d.name.clone()).unwrap_or_default();
    PikPakView {
        installed: rclone::binary().is_some(),
        account: s.pikpak.account.clone(),
        folder: s.pikpak.folder_for(&drive),
        limit: s.pikpak.limit,
        running,
        checking: running && p.checking.load(Ordering::Relaxed),
        bytes: n(&p.bytes),
        total_bytes: n(&p.total_bytes),
        files: n(&p.files),
        total_files: n(&p.total_files),
        deletes: n(&p.deletes),
        speed: n(&p.speed),
        eta: n(&p.eta),
        current: if running { p.current.lock().map(|g| g.clone()).unwrap_or_default() } else { vec![] },
        ended: c.ended.lock().ok().and_then(|g| g.clone()),
        waiting: if running { None } else { blocked(j, s).map(str::to_string) },
        held: c.held.load(Ordering::SeqCst),
        used: quota.map(|q| q.0),
        total: quota.map(|q| q.1),
    }
}

/// The page's line about PikPak for the Agent (look.rs).
pub fn situation(j: &Janitor, s: &Settings) -> String {
    if s.pikpak.account.is_none() {
        return "PikPak 还没登录。\n".into();
    }
    let v = view(j, s);
    let now = if v.running {
        format!("正在同步（{} / {} 个，{} / {}）", v.files, v.total_files, super::gb(v.bytes), super::gb(v.total_bytes))
    } else {
        v.ended.clone().or(v.waiting.clone()).unwrap_or_else(|| "还没同步过".into())
    };
    format!("PikPak：整块硬盘（VAULT 除外）同步到 PikPak 的「{}」，程序自己传，硬盘上删了的进 PikPak 回收站；现在：{now}。\n", v.folder)
}

// ---- commands ----

fn overview(state: &AppState, janitor: &Janitor) -> Result<super::Overview, String> {
    super::overview_now(state, janitor)
}

/// 登录: logs in (rclone checks it at once), remembers the account, syncs.
#[tauri::command]
pub async fn janitor_pikpak_login(app: AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, account: String, password: String) -> Result<super::Overview, String> {
    let account = account.trim().to_string();
    if account.is_empty() || password.is_empty() {
        return Err("要写 PikPak 的账号和密码".into());
    }
    stop_now(&janitor);
    let j = janitor.inner().clone();
    // the config's password: the keychain's, or a new one kept there first (sandbox: rclone.rs's own)
    let pass = if j.mode == Mode::Real {
        match state.youzi.keys.read(rclone::KEY) {
            Ok(p) => Some(p),
            Err(_) => {
                let p = Rclone::fresh_password();
                state.youzi.keys.save(rclone::KEY, &p)?;
                Some(p)
            }
        }
    } else {
        None
    };
    let rc = Rclone::new(&j, pass)?;
    let acc = account.clone();
    let quota = tauri::async_runtime::spawn_blocking(move || rc.login(&acc, &password)).await.map_err(|e| e.to_string())??;
    if let Ok(mut g) = janitor.backup.cloud.quota.lock() {
        *g = Some(quota);
    }
    let mut s = load(&state);
    s.pikpak.account = Some(account);
    store_settings(&state, &s)?;
    janitor.backup.cloud.held.store(false, Ordering::SeqCst);
    kick(&app);
    overview(&state, &janitor)
}

/// 退出登录: the sync stops, rclone's config goes, and its password with it.
#[tauri::command]
pub async fn janitor_pikpak_logout(state: State<'_, AppState>, janitor: State<'_, Janitor>) -> Result<super::Overview, String> {
    stop_now(&janitor);
    if let Ok(rc) = rclone_of(&state, &janitor) {
        rc.logout();
    }
    if janitor.mode == Mode::Real {
        let _ = state.youzi.keys.remove(rclone::KEY);
    }
    let mut s = load(&state);
    s.pikpak.account = None;
    store_settings(&state, &s)?;
    if let Ok(mut g) = janitor.backup.cloud.quota.lock() {
        *g = None;
    }
    overview(&state, &janitor)
}

/// The PikPak folder and the upload limit (a running sync starts again with them).
#[tauri::command]
pub async fn janitor_pikpak_set(app: AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, folder: String, limit: u32) -> Result<super::Overview, String> {
    let mut s = load(&state);
    let drive = s.drive.as_ref().map(|d| d.name.clone()).unwrap_or_default();
    let folder = rclone::folder_of(&folder)?;
    s.pikpak.folder = if folder == drive { String::new() } else { folder };
    s.pikpak.limit = limit.min(1000);
    store_settings(&state, &s)?;
    if janitor.backup.cloud.running() {
        // the sync's loop goes round again, with these
        janitor.backup.cloud.again.store(true, Ordering::SeqCst);
        stop_now(&janitor);
    }
    kick(&app);
    overview(&state, &janitor)
}

/// 停下: stops the sync; none starts until the drive comes back or 接着传.
#[tauri::command]
pub fn janitor_pikpak_stop(janitor: State<'_, Janitor>) {
    janitor.backup.cloud.paused.store(true, Ordering::SeqCst);
    janitor.backup.cloud.again.store(false, Ordering::SeqCst);
    stop_now(&janitor);
}

/// 接着传 (`force`: 照样同步, past the deletion limit once).
#[tauri::command]
pub async fn janitor_pikpak_go(app: AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, force: bool) -> Result<super::Overview, String> {
    janitor.backup.cloud.paused.store(false, Ordering::SeqCst);
    if force {
        janitor.backup.cloud.held.store(false, Ordering::SeqCst);
    }
    start(&app, force);
    overview(&state, &janitor)
}

#[cfg(test)]
pub(super) fn sync_for_test(j: &Janitor, s: &Settings, force: bool) -> (Arc<Progress>, Done) {
    let drive = drive_of(j, s).expect("the fake drive");
    let p = Arc::new(Progress::new());
    let rc = Rclone::new(j, None).expect("rclone");
    let d = rc.sync(&drive.path, &s.pikpak.folder_for(&drive.name), &s.skipped, s.pikpak.limit, force, "2026-11-25", &p);
    (p, d)
}
