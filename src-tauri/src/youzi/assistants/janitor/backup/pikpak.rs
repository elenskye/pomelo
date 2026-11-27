//! PikPak (1.1.0 Phase 3, 1.1.3; requirements「DevJanitor › 备份」): the drive's folders ticked in the 清单, mirrored
//! into a PikPak folder with rclone (rclone.rs), only when I press 开始同步.
//! - Settings (`Settings::pikpak`, with the backup's; not undone): the account shown (the login itself is in
//!   rclone's encrypted config), the PikPak folder (default: the drive's name), the upload limit.
//! - Comparing (`compare_with`): the drive's picked folders (names and sizes, as rclone sees them) against what
//!   the PikPak folder holds there (`Rclone::list`), the way a sync goes (`cloud_diff`): same path and size →
//!   same; size differs → uploaded, PikPak's old copy into ARCHIVE; a drive file PikPak has elsewhere under the
//!   same name and size → moved there; the rest uploaded; what only PikPak has → into ARCHIVE. In the background
//!   when the drive comes, after a backup run, a move on the drive, a sync, logging in, a new folder, 重新对比,
//!   and when the page finds it due; kept in memory (`CloudSnap`).
//!   Drive names go by what they're called on PikPak (`rclone::cloud_name`); two that would get one name there
//!   (`clash`) keep their folder from syncing until one is renamed.
//! - Syncing (`start`): the folders I ticked; one at a time; a backup run starting stops it and it carries on
//!   after (`resume`); compared again when it ends. More moves into ARCHIVE than compared (plus
//!   `rclone::MAX_DELETE`) stop it. Uploads that came back renamed `name(1)` (a name held by a half-made upload
//!   PikPak hides): their folders are set aside into `ARCHIVE/<day>/<folder> (stuck <time>)`, what can be seen is
//!   moved back, and the sync goes once more.
//! - Each sync that moved something (or failed) is a `backup_runs` row with `dest` 'pikpak' (`tasks`: its folders
//!   by name, id 0; `note`: what went where). The App quitting stops rclone; the next sync carries on.

use super::super::{Janitor, Mode};
use super::diff::{self, Skip};
use super::rclone::{self, Done, Progress, Rclone};
use super::{drive_of, group, is_picked, load, load_now, store_settings, top_folders, Group, RunRow, RunTask, Settings, Volume};
use crate::modules::common::unix_now;
use crate::AppState;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// A file in a compared folder (relative to it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    pub path: String,
    pub size: u64,
}

/// One top folder, the drive against PikPak.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudDiff {
    pub same: u64,
    pub same_bytes: u64,
    /// Not on PikPak: uploaded.
    pub new: Vec<Item>,
    /// Another size on PikPak: uploaded, PikPak's into ARCHIVE.
    pub changed: Vec<Item>,
    /// On PikPak elsewhere (same name and size): moved there. `from`: relative to this folder, or `/<top>/…`.
    pub moved: Vec<diff::Move>,
    /// Only on PikPak: into ARCHIVE.
    pub extra: Vec<Item>,
    /// Drive files that would get another's name on PikPak: the folder doesn't sync until one is renamed.
    pub clash: Vec<Item>,
}

/// The last comparison.
#[derive(Debug, Clone, Default)]
pub struct CloudSnap {
    /// What it's of: the drive, the PikPak folder, the folders picked (`key_of`).
    key: String,
    at: i64,
    /// The PikPak folder isn't there yet.
    missing: bool,
    error: Option<String>,
    /// NFC top folder → how it compares.
    per: BTreeMap<String, CloudDiff>,
}

/// The PikPak side of `Backup` (in memory).
#[derive(Default)]
pub struct Cloud {
    running: AtomicBool,
    run: Mutex<Arc<Progress>>,
    /// The folders the running sync mirrors.
    tops: Mutex<Vec<String>>,
    ended: Mutex<Option<String>>,
    /// Syncs ended this session (the page pops `ended` once each).
    ends: AtomicU64,
    /// Stopped for a backup run: these folders carry on after it.
    resume: Mutex<Option<Vec<String>>>,
    comparing: AtomicBool,
    snap: Mutex<Option<CloudSnap>>,
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

/// HHMMSS, local.
fn local_clock(state: &AppState) -> String {
    state.youzi.with_db(|c| Ok(c.query_row("SELECT strftime('%H%M%S', 'now', 'localtime')", [], |r| r.get::<_, String>(0)).unwrap_or_default())).unwrap_or_default()
}

/// The drive's top folders picked in the 清单.
pub fn picked_on(s: &Settings, drive: &Volume) -> Vec<String> {
    top_folders(drive).into_iter().filter(|t| is_picked(s, t)).collect()
}

fn key_of(s: &Settings, drive: &Volume, tops: &[String]) -> String {
    format!("{}\n{}\n{}", drive.uuid, s.pikpak.folder_for(&drive.name), tops.iter().map(|t| diff::nfc(t)).collect::<Vec<_>>().join("\n"))
}

// ---- comparing ----

fn leaf(k: &str) -> &str {
    k.rsplit('/').next().unwrap_or(k)
}

fn split(k: &str) -> (&str, &str) {
    k.split_once('/').unwrap_or((k, ""))
}

/// The drive (NFC path from its root → size) against PikPak (the same), by top folder (`tops`), as a sync goes.
pub fn cloud_diff(drive: &BTreeMap<String, u64>, cloud: &BTreeMap<String, u64>, tops: &[String]) -> BTreeMap<String, CloudDiff> {
    let mut per: BTreeMap<String, CloudDiff> = tops.iter().map(|t| (diff::nfc(t), CloudDiff::default())).collect();
    let mut only_drive: Vec<(&str, u64)> = Vec::new();
    for (k, &size) in drive {
        let (top, rel) = split(k);
        let Some(d) = per.get_mut(top) else { continue };
        match cloud.get(k) {
            Some(&c) if c == size => {
                d.same += 1;
                d.same_bytes += size;
            }
            Some(_) => d.changed.push(Item { path: rel.into(), size }),
            None => only_drive.push((k, size)),
        }
    }
    // rclone's renames: a PikPak-only file with the name and size of a drive-only one
    let mut left: HashMap<(&str, u64), Vec<&str>> = HashMap::new();
    for (k, &size) in cloud.iter().rev() {
        if !drive.contains_key(k) && per.contains_key(split(k).0) {
            left.entry((leaf(k), size)).or_default().push(k);
        }
    }
    for (k, size) in only_drive {
        let (top, rel) = split(k);
        let d = per.get_mut(top).expect("a top");
        match left.get_mut(&(leaf(k), size)).and_then(|v| v.pop()) {
            Some(from) => {
                let (ft, fr) = split(from);
                d.moved.push(diff::Move { from: if ft == top { fr.into() } else { format!("/{from}") }, to: rel.into(), size });
            }
            None => d.new.push(Item { path: rel.into(), size }),
        }
    }
    let mut rest: Vec<&str> = left.into_values().flatten().collect();
    rest.sort();
    for k in rest {
        let (top, rel) = split(k);
        if let Some(d) = per.get_mut(top) {
            d.extra.push(Item { path: rel.into(), size: cloud[k] });
        }
    }
    per
}

/// Compares the drive's picked folders with PikPak now (None: nothing to compare: not logged in, no drive).
pub fn compare_with(j: &Janitor, s: &Settings, rc: &Rclone) -> Option<CloudSnap> {
    s.pikpak.account.as_ref()?;
    let drive = drive_of(j, s)?;
    let tops = picked_on(s, &drive);
    let mut sn = CloudSnap { key: key_of(s, &drive, &tops), at: unix_now(), ..Default::default() };
    let stop = AtomicBool::new(false);
    // by the names they get on PikPak (the top folder keeps its own: the filters and the diff go by it)
    let mut on_drive: BTreeMap<String, u64> = BTreeMap::new();
    let mut clashes: Vec<(String, Item)> = Vec::new();
    for t in &tops {
        let top = diff::nfc(t);
        let l = match diff::list(&drive.path.join(t), &Skip { mirror: true }, &stop) {
            Ok(l) => l,
            Err(e) => {
                sn.error = Some(format!("硬盘上的 {t} 没读成：{e}"));
                return Some(sn);
            }
        };
        let mut named: HashMap<String, &str> = HashMap::new();
        for (k, (_, m)) in &l.files {
            let up = rclone::cloud_name(k);
            if let Some(other) = named.insert(up.clone(), k) {
                for p in [other, k.as_str()] {
                    if !clashes.iter().any(|(ct, c)| *ct == top && c.path == p) {
                        clashes.push((top.clone(), Item { path: p.to_string(), size: l.files.get(p).map_or(0, |(_, m)| m.size) }));
                    }
                }
            }
            on_drive.insert(format!("{top}/{up}"), m.size);
        }
    }
    match rc.list(&s.pikpak.folder_for(&drive.name), &tops) {
        Ok(cloud) => {
            sn.missing = cloud.is_none();
            sn.per = cloud_diff(&on_drive, &cloud.unwrap_or_default(), &tops);
            for (top, item) in clashes {
                if let Some(d) = sn.per.get_mut(&top) {
                    d.clash.push(item);
                }
            }
        }
        Err(e) => sn.error = Some(e),
    }
    Some(sn)
}

/// Why PikPak can't be compared or synced now (None: it can).
fn blocked(j: &Janitor, s: &Settings) -> Option<&'static str> {
    if s.pikpak.account.is_none() {
        Some("没登录")
    } else if drive_of(j, s).is_none() {
        Some("没插硬盘")
    } else if j.backup.running() {
        Some("正在备份")
    } else if rclone::binary().is_none() {
        Some("没装 rclone")
    } else {
        None
    }
}

fn compare_now(st: &AppState, j: &Janitor) {
    let c = &j.backup.cloud;
    if c.running() || c.comparing.swap(true, Ordering::SeqCst) {
        return;
    }
    let s = load_now(st, j);
    if blocked(j, &s).is_none() {
        let sn = match rclone_of(st, j) {
            Ok(rc) => compare_with(j, &s, &rc),
            Err(e) => drive_of(j, &s).map(|d| CloudSnap { key: key_of(&s, &d, &picked_on(&s, &d)), at: unix_now(), error: Some(e), ..Default::default() }),
        };
        if let (Some(sn), Ok(mut g)) = (sn, c.snap.lock()) {
            *g = Some(sn);
        }
    }
    c.comparing.store(false, Ordering::SeqCst);
}

/// Compares in the background (the drive or PikPak changed).
pub fn compare_later(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || compare_now(&app.state::<AppState>(), app.state::<Janitor>().inner()));
}

/// Compares when there's none of the drive, folder and 清单 as they are now.
pub fn compare_when_due(app: &AppHandle) {
    let j = app.state::<Janitor>();
    let c = &j.backup.cloud;
    if c.running() || c.comparing.load(Ordering::SeqCst) {
        return;
    }
    let s = load(&app.state::<AppState>());
    if blocked(&j, &s).is_some() {
        return;
    }
    let Some(drive) = drive_of(&j, &s) else { return };
    let key = key_of(&s, &drive, &picked_on(&s, &drive));
    if c.snap.lock().ok().and_then(|g| g.as_ref().map(|sn| sn.key != key)).unwrap_or(true) {
        compare_later(app);
    }
}

/// The drive came (or was picked): compare.
pub fn plugged(app: &AppHandle) {
    compare_later(app);
}

/// A backup run starts: a sync stops and carries on after it.
pub fn yield_to_backup(j: &Janitor) {
    if j.backup.cloud.running() {
        let tops = j.backup.cloud.tops.lock().map(|g| g.clone()).unwrap_or_default();
        if let Ok(mut g) = j.backup.cloud.resume.lock() {
            *g = Some(tops);
        }
        stop_now(j);
    }
}

/// A backup run ended: the sync it stopped carries on; otherwise compare (the drive changed).
pub fn after_backup(app: &AppHandle) {
    let resume = app.state::<Janitor>().backup.cloud.resume.lock().ok().and_then(|mut g| g.take());
    match resume {
        Some(tops) if start(app, &tops).is_ok() => {}
        _ => compare_later(app),
    }
}

fn stop_now(j: &Janitor) {
    if let Ok(r) = j.backup.cloud.run.lock() {
        r.kill();
    }
}

// ---- syncing ----

/// What a sync of `tops` may move into ARCHIVE: what was compared, and `rclone::MAX_DELETE` more.
fn max_delete(j: &Janitor, key: &str, tops: &[String]) -> u64 {
    let compared = j.backup.cloud.snap.lock().ok().and_then(|g| g.as_ref().filter(|sn| sn.key == key).map(|sn| tops.iter().filter_map(|t| sn.per.get(&diff::nfc(t))).map(|d| d.extra.len() as u64).sum::<u64>())).unwrap_or(0);
    compared + rclone::MAX_DELETE as u64
}

/// 开始同步: mirrors the picked folders among `tops` in the background, then compares again.
pub fn start(app: &AppHandle, tops: &[String]) -> Result<(), String> {
    let j = app.state::<Janitor>().inner().clone();
    let st = app.state::<AppState>();
    let s = load_now(&st, &j);
    if let Some(why) = blocked(&j, &s) {
        return Err(why.into());
    }
    let drive = drive_of(&j, &s).ok_or("没插硬盘")?;
    let picked = picked_on(&s, &drive);
    let tops: Vec<String> = picked.iter().filter(|p| tops.iter().any(|t| diff::nfc(t) == diff::nfc(p))).cloned().collect();
    if tops.is_empty() {
        return Err("先勾要同步的文件夹".into());
    }
    if j.backup.cloud.comparing.load(Ordering::SeqCst) {
        return Err("正在和 PikPak 对比，等它对完".into());
    }
    let key = key_of(&s, &drive, &picked);
    let clashing: Vec<String> = j.backup.cloud.snap.lock().ok().and_then(|g| g.as_ref().filter(|sn| sn.key == key).map(|sn| tops.iter().filter(|t| sn.per.get(&diff::nfc(t)).is_some_and(|d| !d.clash.is_empty())).cloned().collect())).unwrap_or_default();
    if !clashing.is_empty() {
        return Err(format!("{} 里有名字传到 PikPak 上会撞名，先改名再同步", clashing.join("、")));
    }
    if j.backup.cloud.running.swap(true, Ordering::SeqCst) {
        return Err("正在同步".into());
    }
    let max = max_delete(&j, &key, &tops);
    let app = app.clone();
    std::thread::spawn(move || {
        let st = app.state::<AppState>();
        once(&st, &j, &s, &drive, &tops, max);
        j.backup.cloud.running.store(false, Ordering::SeqCst);
        // stopped for a backup run: it compares (or carries on) when that ends
        if j.backup.cloud.resume.lock().map(|g| g.is_none()).unwrap_or(true) {
            compare_now(&st, &j);
        }
    });
    Ok(())
}

/// What a sync moved (over its passes).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Tally {
    pub files: u64,
    pub bytes: u64,
    pub deletes: u64,
    pub renames: u64,
}

impl Tally {
    pub fn of(p: &Progress) -> Self {
        Tally { files: p.files.load(Ordering::Relaxed), bytes: p.bytes.load(Ordering::Relaxed), deletes: p.deletes.load(Ordering::Relaxed), renames: p.renames.load(Ordering::Relaxed) }
    }

    fn add(self, o: Tally) -> Self {
        Tally { files: self.files + o.files, bytes: self.bytes + o.bytes, deletes: self.deletes + o.deletes, renames: self.renames + o.renames }
    }
}

/// The cloud folders (as named there) holding names a half-made upload took.
pub fn stuck_folders(d: &Done) -> Vec<String> {
    let mut f: Vec<String> = d.stuck.iter().filter_map(|o| rclone::cloud_name(&diff::nfc(o)).rsplit_once('/').map(|(p, _)| p.to_string())).collect();
    f.sort();
    f.dedup();
    f
}

/// Where a stuck folder goes in the cloud folder's ARCHIVE.
pub fn aside_for(rel: &str, day: &str, clock: &str) -> String {
    format!("{}/{day}/{rel} (stuck {clock})", super::copy::OLD)
}

#[allow(clippy::too_many_arguments)]
/// Syncs `tops`; when uploads came back renamed, sets their folders aside and syncs once more. What moved over
/// the passes, how the last one ended, the folders set aside.
fn sync_mending(rc: &Rclone, run: &Mutex<Arc<Progress>>, source: &std::path::Path, folder: &str, tops: &[String], limit: u32, max: u64, day: &str, clock: &str) -> (Tally, Done, Vec<String>) {
    let p = run.lock().map(|g| g.clone()).unwrap_or_default();
    let d = rc.sync(source, folder, tops, limit, max, day, &p);
    let tally = Tally::of(&p);
    let stuck = stuck_folders(&d);
    if stuck.is_empty() || d.stopped || d.fatal.is_some() || d.too_many_deletes {
        return (tally, d, Vec::new());
    }
    let (mut fixed, mut failed) = (Vec::new(), Vec::new());
    for f in stuck {
        if p.stop.load(Ordering::SeqCst) {
            break;
        }
        match rc.set_aside(folder, &f, &aside_for(&f, day, clock)) {
            Ok(()) => fixed.push(f),
            Err(e) => failed.push((f, format!("卡住的名字没收拾成：{e}"))),
        }
    }
    if fixed.is_empty() || p.stop.load(Ordering::SeqCst) {
        let mut d = d;
        d.errors.extend(failed);
        return (tally, d, fixed);
    }
    let p2 = Arc::new(Progress::new());
    if let Ok(mut g) = run.lock() {
        *g = p2.clone();
    }
    let mut d2 = rc.sync(source, folder, tops, limit, max, day, &p2);
    d2.errors.extend(failed);
    (tally.add(Tally::of(&p2)), d2, fixed)
}

/// One sync, recorded.
fn once(st: &AppState, j: &Janitor, s: &Settings, drive: &Volume, tops: &[String], max: u64) {
    let c = &j.backup.cloud;
    if let Ok(mut g) = c.run.lock() {
        *g = Arc::new(Progress::new());
    }
    if let Ok(mut g) = c.tops.lock() {
        *g = tops.to_vec();
    }
    if let Ok(mut g) = c.ended.lock() {
        *g = None;
    }
    let end = |text: String| {
        if c.resume.lock().map(|g| g.is_none()).unwrap_or(true) {
            if let Ok(mut g) = c.ended.lock() {
                *g = Some(text);
            }
            c.ends.fetch_add(1, Ordering::SeqCst);
        }
    };
    let rc = match rclone_of(st, j) {
        Ok(r) => r,
        Err(e) => return end(e),
    };
    let id = st.youzi.with_db(|c| record_start(c, &drive.name, tops)).ok();
    let (day, clock) = (local_day(st), local_clock(st));
    let (t, d, fixed) = sync_mending(&rc, &c.run, &drive.path, &s.pikpak.folder_for(&drive.name), tops, s.pikpak.limit, max, &day, &clock);
    if let Some(id) = id {
        let _ = st.youzi.with_db(|c| record_end(c, id, tops, &t, &d, fixed.len()));
    }
    if !d.stopped {
        if let Ok(q) = rc.about() {
            if let Ok(mut g) = c.quota.lock() {
                *g = Some(q);
            }
        }
    }
    end(ended_text(&t, &d, fixed.len()));
}

/// The App quits: rclone goes too (the next sync carries on).
pub fn on_exit(app: &AppHandle) {
    stop_now(app.state::<Janitor>().inner());
}

// ---- runs (youzi.db backup_runs, dest 'pikpak') ----

fn run_tasks(tops: &[String], done: bool) -> String {
    let t: Vec<RunTask> = tops.iter().map(|t| RunTask { id: 0, source: String::new(), target: t.clone(), files: 0, bytes: 0, updated: 0, done }).collect();
    serde_json::to_string(&t).unwrap_or_else(|_| "[]".into())
}

pub(super) fn record_start(c: &Connection, drive: &str, tops: &[String]) -> Result<i64, String> {
    c.execute("INSERT INTO backup_runs (dest, drive, tasks) VALUES ('pikpak', ?1, ?2)", params![drive, run_tasks(tops, false)]).map_err(crate::modules::common::err)?;
    Ok(c.last_insert_rowid())
}

/// The row as it ended; a sync that changed nothing and went fine leaves none.
pub(super) fn record_end(c: &Connection, id: i64, tops: &[String], t: &Tally, d: &Done, fixed: usize) -> Result<(), String> {
    let Tally { files, bytes, deletes, renames } = *t;
    if d.ok && files == 0 && deletes == 0 && renames == 0 && d.errors.is_empty() && fixed == 0 {
        c.execute("DELETE FROM backup_runs WHERE id = ?1", [id]).map_err(crate::modules::common::err)?;
        return Ok(());
    }
    let status = if d.ok { "done" } else if d.stopped { "stopped" } else { "failed" };
    let mut note: Vec<String> = Vec::new();
    if fixed > 0 {
        note.push(format!("{fixed} 个文件夹里有名字被没传完的旧上传占着，整个挪进 ARCHIVE、能看见的挪回来后再传了一遍"));
    }
    if renames > 0 {
        note.push(format!("在 PikPak 上挪了 {renames} 个"));
    }
    if deletes > 0 {
        note.push(format!("挪进 PikPak 的 ARCHIVE {deletes} 个"));
    }
    if d.too_many_deletes {
        note.push("要挪进 ARCHIVE 的比对出来的多，没接着同步".into());
    }
    if let Some(f) = &d.fatal {
        note.push(f.clone());
    }
    let at: String = c.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |r| r.get(0)).map_err(crate::modules::common::err)?;
    c.execute(
        "UPDATE backup_runs SET status = ?2, tasks = ?3, files = ?4, bytes = ?5, errors = ?6, note = ?7, finished_at = ?8 WHERE id = ?1",
        params![id, status, run_tasks(tops, d.ok), files as i64, bytes as i64, serde_json::to_string(&d.errors).map_err(|e| e.to_string())?, (!note.is_empty()).then(|| note.join("；")), at],
    )
    .map(|_| ())
    .map_err(crate::modules::common::err)
}

/// One line for the page about how a sync ended.
pub(super) fn ended_text(t: &Tally, d: &Done, fixed: usize) -> String {
    let Tally { files, bytes, deletes, renames } = *t;
    let moved = if files > 0 { format!("传了 {files} 个（{}）", super::gb(bytes)) } else { String::new() };
    let head = if d.too_many_deletes {
        "要挪进 ARCHIVE 的比对出来的多，先停下了：点「重新对比」再看看".into()
    } else if let Some(f) = &d.fatal {
        f.clone()
    } else if d.stopped {
        if moved.is_empty() { "停下了".into() } else { format!("停下了：{moved}") }
    } else if d.ok && files == 0 && deletes == 0 && renames == 0 {
        "和硬盘上一样".into()
    } else if d.ok {
        let renamed = if renames > 0 { format!("在 PikPak 上挪了 {renames} 个") } else { String::new() };
        let archived = if deletes > 0 { format!("挪进 ARCHIVE {deletes} 个") } else { String::new() };
        let mended = if fixed > 0 { format!("收拾了 {fixed} 个卡住的文件夹") } else { String::new() };
        format!("同步好了：{}", [moved, renamed, archived, mended].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("，"))
    } else {
        format!("没传完{}", if moved.is_empty() { String::new() } else { format!("：{moved}") })
    };
    if d.errors.is_empty() { head } else { format!("{head}；{} 个没成", d.errors.len()) }
}

// ---- what the page gets ----

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudDiffView {
    pub same: u64,
    pub same_bytes: u64,
    pub new: Group<Item>,
    pub changed: Group<Item>,
    pub moved: Group<diff::Move>,
    pub extra: Group<Item>,
    pub clash: Group<Item>,
}

/// A picked drive folder on the PikPak side.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFolder {
    pub name: String,
    /// When a sync of it last went through.
    pub last: Option<String>,
    /// How it compares (None: not yet).
    pub diff: Option<CloudDiffView>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PikPakView {
    /// rclone is there.
    pub installed: bool,
    pub account: Option<String>,
    pub folder: String,
    pub limit: u32,
    pub running: bool,
    /// The running sync's folders.
    pub syncing: Vec<String>,
    /// rclone still comparing before it uploads.
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
    /// Syncs ended this session (a new one: pop `ended`).
    pub ends: u64,
    /// Why none can run now.
    pub waiting: Option<String>,
    pub used: Option<u64>,
    pub total: Option<u64>,
    /// Comparing with PikPak now.
    pub comparing: bool,
    pub compared_at: Option<i64>,
    /// The PikPak folder isn't there yet (a sync makes it).
    pub missing: bool,
    /// The comparison didn't go through.
    pub error: Option<String>,
    /// The picked drive folders (the drive in).
    pub folders: Vec<CloudFolder>,
}

pub fn view(j: &Janitor, s: &Settings, runs: &[RunRow]) -> PikPakView {
    let c = &j.backup.cloud;
    let p = c.run.lock().map(|g| g.clone()).unwrap_or_default();
    let running = c.running();
    let quota = c.quota.lock().ok().and_then(|g| *g);
    let n = |a: &AtomicU64| a.load(Ordering::Relaxed);
    let drive = drive_of(j, s);
    let name = s.drive.as_ref().map(|d| d.name.clone()).unwrap_or_default();
    let tops = drive.as_ref().map(|d| picked_on(s, d)).unwrap_or_default();
    let snap = drive.as_ref().and_then(|d| {
        let key = key_of(s, d, &tops);
        c.snap.lock().ok().and_then(|g| g.clone()).filter(|sn| sn.key == key)
    });
    let last = |t: &str| runs.iter().filter(|r| r.dest == "pikpak").find(|r| r.tasks.iter().any(|x| x.done && diff::nfc(&x.target) == diff::nfc(t))).and_then(|r| r.finished_at.clone());
    let folders = tops
        .iter()
        .map(|t| CloudFolder {
            name: t.clone(),
            last: last(t),
            diff: snap.as_ref().filter(|sn| sn.error.is_none()).and_then(|sn| sn.per.get(&diff::nfc(t))).map(|d| CloudDiffView {
                same: d.same,
                same_bytes: d.same_bytes,
                new: group(&d.new, |r| r.size),
                changed: group(&d.changed, |r| r.size),
                moved: group(&d.moved, |m| m.size),
                extra: group(&d.extra, |r| r.size),
                clash: group(&d.clash, |r| r.size),
            }),
        })
        .collect();
    PikPakView {
        installed: rclone::binary().is_some(),
        account: s.pikpak.account.clone(),
        folder: s.pikpak.folder_for(&name),
        limit: s.pikpak.limit,
        running,
        syncing: if running { c.tops.lock().map(|g| g.clone()).unwrap_or_default() } else { vec![] },
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
        ends: c.ends.load(Ordering::SeqCst),
        waiting: if running { None } else { blocked(j, s).map(str::to_string) },
        used: quota.map(|q| q.0),
        total: quota.map(|q| q.1),
        comparing: c.comparing.load(Ordering::SeqCst),
        compared_at: snap.as_ref().map(|sn| sn.at),
        missing: snap.as_ref().is_some_and(|sn| sn.missing),
        error: snap.as_ref().and_then(|sn| sn.error.clone()),
        folders,
    }
}

/// The page's lines about PikPak for the Agent (look.rs).
pub fn situation(j: &Janitor, s: &Settings) -> String {
    if s.pikpak.account.is_none() {
        return "PikPak 还没登录。\n".into();
    }
    let v = view(j, s, &[]);
    let now = if v.running {
        format!("正在同步 {}（{} / {} 个，{} / {}）", v.syncing.join("、"), v.files, v.total_files, super::gb(v.bytes), super::gb(v.total_bytes))
    } else if v.comparing {
        "正在和 PikPak 对比".into()
    } else {
        let todo: Vec<String> = v.folders.iter().filter_map(|f| f.diff.as_ref().map(|d| (f, d))).filter(|(_, d)| d.new.count + d.changed.count + d.moved.count + d.extra.count > 0).map(|(f, d)| format!("{} 要传 {} 个、在 PikPak 上挪 {} 个、挪进 ARCHIVE {} 个", f.name, d.new.count + d.changed.count, d.moved.count, d.extra.count)).collect();
        v.error.clone().or(v.waiting.clone()).unwrap_or_else(|| if todo.is_empty() { "勾了的都和硬盘上一样".into() } else { todo.join("；") })
    };
    format!("PikPak：硬盘上清单里勾了的文件夹，用户点「开始同步」才同步到 PikPak 的「{}」（和硬盘一样；PikPak 上多的、被换掉的旧的挪进那里的 ARCHIVE，不删）；现在：{now}。\n", v.folder)
}

// ---- commands ----

fn overview(state: &AppState, janitor: &Janitor) -> Result<super::Overview, String> {
    super::overview_now(state, janitor)
}

/// The PikPak folder as kept (the drive's name: empty).
fn folder_setting(s: &Settings, folder: &str) -> Result<String, String> {
    let drive = s.drive.as_ref().map(|d| d.name.clone()).unwrap_or_default();
    let folder = rclone::folder_of(folder)?;
    Ok(if folder == drive { String::new() } else { folder })
}

/// 登录: logs in (rclone checks it at once), remembers the account and the folder, compares.
#[tauri::command]
pub async fn janitor_pikpak_login(app: AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, account: String, password: String, folder: String) -> Result<super::Overview, String> {
    let account = account.trim().to_string();
    if account.is_empty() || password.is_empty() {
        return Err("要写 PikPak 的账号和密码".into());
    }
    if janitor.backup.cloud.running() {
        return Err("正在同步，先停下".into());
    }
    let folder = folder_setting(&load(&state), &folder)?;
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
    s.pikpak.folder = folder;
    store_settings(&state, &s)?;
    compare_later(&app);
    overview(&state, &janitor)
}

/// 退出登录: rclone's config goes, and its password with it (PikPak's files stay).
#[tauri::command]
pub async fn janitor_pikpak_logout(state: State<'_, AppState>, janitor: State<'_, Janitor>) -> Result<super::Overview, String> {
    if janitor.backup.cloud.running() {
        return Err("正在同步，先停下".into());
    }
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
    if let Ok(mut g) = janitor.backup.cloud.snap.lock() {
        *g = None;
    }
    overview(&state, &janitor)
}

/// The PikPak folder and the upload limit.
#[tauri::command]
pub async fn janitor_pikpak_set(app: AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, folder: String, limit: u32) -> Result<super::Overview, String> {
    if janitor.backup.cloud.running() {
        return Err("正在同步，先停下再改".into());
    }
    let mut s = load(&state);
    s.pikpak.folder = folder_setting(&s, &folder)?;
    s.pikpak.limit = limit.min(1000);
    store_settings(&state, &s)?;
    compare_when_due(&app);
    overview(&state, &janitor)
}

/// 开始同步: the folders I ticked (their names).
#[tauri::command]
pub async fn janitor_pikpak_start(app: AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, tops: Vec<String>) -> Result<super::Overview, String> {
    start(&app, &tops)?;
    overview(&state, &janitor)
}

/// 停下: stops the sync (it doesn't carry on by itself).
#[tauri::command]
pub fn janitor_pikpak_stop(janitor: State<'_, Janitor>) {
    if let Ok(mut g) = janitor.backup.cloud.resume.lock() {
        *g = None;
    }
    stop_now(&janitor);
}

#[cfg(test)]
pub(super) fn sync_for_test(j: &Janitor, s: &Settings, tops: &[&str], max: u64) -> (Tally, Done) {
    let drive = drive_of(j, s).expect("the fake drive");
    let p = Arc::new(Progress::new());
    let rc = Rclone::new(j, None).expect("rclone");
    let tops: Vec<String> = tops.iter().map(|t| t.to_string()).collect();
    let d = rc.sync(&drive.path, &s.pikpak.folder_for(&drive.name), &tops, s.pikpak.limit, max, "2026-11-25", &p);
    (Tally::of(&p), d)
}

#[cfg(test)]
pub(super) fn compare_for_test(j: &Janitor, s: &Settings) -> CloudSnap {
    compare_with(j, s, &Rclone::new(j, None).expect("rclone")).expect("logged in, the drive in")
}

#[cfg(test)]
impl CloudSnap {
    pub fn per(&self) -> &BTreeMap<String, CloudDiff> {
        &self.per
    }
    pub fn missing(&self) -> bool {
        self.missing
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}
