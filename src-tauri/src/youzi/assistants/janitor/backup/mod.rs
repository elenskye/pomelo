//! DevJanitor 备份 (1.1.0 Phase 1; requirements「DevJanitor › 备份」): a Mac folder → a folder on the external
//! drive, only what changed. Names and metadata only (diff.rs); the bytes are read only to copy them (copy.rs).
//! - Settings (youzi.db preference `assistant.janitor.backup`, not undone): the drive (its volume UUID), the 清单
//!   (`picked`: the top folders of the drive and of ~/Downloads in the backup) and the tasks it makes (`settle`:
//!   each picked folder ~/Downloads has → the drive folder of that name; ids kept). Runs: youzi.db
//!   `backup_runs` (not undone). What each task's Mac folder held at its last backup: janitor.db `backup_seen`.
//! - The watcher (`watch`) looks at the drives every few seconds; when mine appears it compares every task and
//!   keeps the result for the page. Nothing is copied until I pick tasks and press 开始.
//! - 只加不删: a run never deletes on the drive. A changed file's old copy moves to ARCHIVE first (copy.rs);
//!   one newer on the drive is held. A rename or move is done on the drive only when I say so (`apply_moves`).
//! - VAULT (rules::sealed) and the drive's ARCHIVE are never a task's source or target, never listed.
//! - Sandbox (dev, tests, a scratch data dir): the fake Mac's folders and fake drives in `<data>/janitor-volumes`.
//! - 柚子 (Phase 2): each comparison also works out clues (hints.rs); the Agent reads them and proposes cards
//!   (cards.rs); a drive plugged in gets an automatic look (look.rs).
//! - PikPak (Phase 3; 1.1.3): the picked drive folders compared with PikPak, mirrored there when I say so (pikpak.rs,
//!   rclone.rs).

pub mod cards;
pub mod copy;
pub mod diff;
pub mod hints;
pub mod look;
pub mod pikpak;
pub mod rclone;
pub mod volumes;
#[cfg(test)]
mod tests;

use super::{rules, store, Janitor, Mode};
use crate::modules::common::unix_now;
use crate::youzi::store::{pref_get, pref_set};
use crate::AppState;
use diff::{Diff, Skip};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use volumes::{Volume, Volumes};

pub const KEY: &str = "assistant.janitor.backup";
/// The fake drives' folder, beside the fake Mac.
pub const FAKE_VOLUMES: &str = "janitor-volumes";
/// Rows per group the page gets.
const LIST_MAX: usize = 300;
const ERRORS_MAX: usize = 200;
/// Room left on the drive beyond what a run copies.
const MARGIN: u64 = 200 * rules::MB;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub drive: Option<DriveId>,
    pub tasks: Vec<Task>,
    pub next_id: i64,
    pub pikpak: pikpak::PikPak,
    /// Before the 清单 (1.1.1): drive folders kept out. Read once to work `picked` out.
    pub skipped: Vec<String>,
    /// 清单 (1.1.2): the top folders (on the drive or in ~/Downloads) in the backup; the rest of the drive gets no
    /// clues or cards and isn't synced to the cloud. None: old settings, until the drive is in.
    pub picked: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DriveId {
    pub uuid: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Task {
    pub id: i64,
    /// Virtual path of the Mac folder.
    pub source: String,
    /// The drive folder, relative to its root.
    pub target: String,
}

pub fn settings(c: &Connection) -> Settings {
    pref_get::<Settings>(c, KEY).ok().flatten().unwrap_or_default()
}

// ---- state ----

#[derive(Debug, Default)]
pub struct Run {
    pub files_total: AtomicU64,
    pub bytes_total: AtomicU64,
    pub files_done: AtomicU64,
    pub bytes_done: AtomicU64,
    pub stop: AtomicBool,
    pub current: Mutex<String>,
    pub task: Mutex<String>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    uuid: String,
    at: i64,
    /// Same tasks as now? (their JSON)
    tasks: String,
    diffs: HashMap<i64, Result<Diff, String>>,
    /// 柚子's clues (None: they couldn't be worked out).
    hints: Option<hints::Hints>,
    /// Files and bytes in each of the drive's top folders (NFC names).
    tops: HashMap<String, (u64, u64)>,
}

/// What a comparison is of besides the drive: the tasks and the 清单.
fn snap_key(s: &Settings) -> String {
    serde_json::to_string(&(&s.tasks, &s.picked, &s.skipped)).unwrap_or_default()
}

/// The last comparison, when it's of the drive connected now and the settings as they are.
fn fresh_snapshot(j: &Janitor, s: &Settings) -> Option<Snapshot> {
    let v = drive_of(j, s)?;
    let key = snap_key(s);
    j.backup.snapshot.lock().ok().and_then(|g| g.clone()).filter(|sn| sn.uuid == v.uuid && sn.tasks == key)
}

pub struct Backup {
    pub volumes: Volumes,
    snapshot: Mutex<Option<Snapshot>>,
    comparing: AtomicBool,
    running: AtomicBool,
    run: Mutex<Arc<Run>>,
    /// The last run's end (error or how it ended), for the page.
    ended: Mutex<Option<String>>,
    /// Runs ended (this session): the page pops `ended` once per run.
    ends: AtomicU64,
    last_names: Mutex<Vec<String>>,
    /// Pomelo's data dir (rclone's config, the fake cloud).
    pub data: PathBuf,
    /// PikPak's sync (pikpak.rs).
    pub cloud: pikpak::Cloud,
}

impl Backup {
    pub fn new(mode: Mode, data: &Path) -> Self {
        let volumes = match mode {
            Mode::Real => Volumes::new(PathBuf::from("/Volumes"), false),
            Mode::Sandbox => Volumes::new(data.join(FAKE_VOLUMES), true),
        };
        Self { volumes, snapshot: Mutex::new(None), comparing: AtomicBool::new(false), running: AtomicBool::new(false), run: Mutex::new(Arc::default()), ended: Mutex::new(None), ends: AtomicU64::new(0), last_names: Mutex::new(Vec::new()), data: data.to_path_buf(), cloud: pikpak::Cloud::default() }
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// Makes the fake Mac and the fake drive when they're missing or old (sandbox only).
pub(crate) fn ensure_fake(j: &Janitor) -> Result<(), String> {
    if j.mode == Mode::Sandbox {
        super::ensure_sandbox(&j.root)?;
        super::fake::ensure_volumes(&j.backup.volumes.dir)?;
    }
    Ok(())
}

fn drive_of(j: &Janitor, s: &Settings) -> Option<Volume> {
    s.drive.as_ref().and_then(|d| j.backup.volumes.find(&d.uuid))
}

// ---- the 清单 ----

/// Why a Mac folder can't be a task's source (None: it can).
fn source_problem(j: &Janitor, virt: &str) -> Option<String> {
    if !rules::within(virt, &j.home) || virt == j.home {
        return Some("要选个人文件夹里的一个文件夹".into());
    }
    if rules::sealed(virt) {
        return Some("这个地方不能备份".into());
    }
    // the website may be backed up (its folder is protected only from deleting)
    let protected = rules::Protected::new(&j.home, &format!("{}/Documents/GitHub/personal-website", j.home));
    if let Some(why) = protected.of(virt).filter(|w| *w != "个人网站") {
        return Some(format!("在受保护的地方（{why}），不备份"));
    }
    if !j.real_of(virt).is_dir() {
        return Some("电脑上没有这个文件夹了".into());
    }
    None
}

/// ~/Downloads (virtual): the Mac side of every task.
pub fn downloads(j: &Janitor) -> String {
    format!("{}/Downloads", j.home)
}

/// A name a top folder can have in the 清单 (not hidden, VAULT or ARCHIVE).
fn listable(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !rules::sealed_name(name) && name != copy::OLD && !name.contains('/')
}

fn dirs_in(dir: &Path) -> Vec<String> {
    let mut f: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| std::fs::symlink_metadata(e.path()).is_ok_and(|m| m.is_dir()))
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| listable(n))
        .collect();
    f.sort();
    f
}

/// ~/Downloads' top folders (VAULT and hidden ones left out).
pub fn mac_folders(j: &Janitor) -> Vec<String> {
    dirs_in(&j.real_of(&downloads(j)))
}

fn same(a: &str, b: &str) -> bool {
    diff::nfc(a) == diff::nfc(b)
}

/// Is the top folder `top` (of the drive or ~/Downloads) ticked in the 清单?
pub fn is_picked(s: &Settings, top: &str) -> bool {
    match &s.picked {
        Some(p) => p.iter().any(|x| same(x, top)),
        None => !s.skipped.iter().any(|k| same(k, top)),
    }
}

/// The top folder not in the backup that the drive path `rel` is in (ARCHIVE too; None: it's in the backup).
pub fn kept_out_at(s: &Settings, rel: &str) -> Option<String> {
    let top = rel.trim_matches('/').split('/').next().filter(|t| !t.is_empty())?;
    (top == copy::OLD || !is_picked(s, top)).then(|| top.to_string())
}

/// Those of the drive's top folders `tops` not in the backup.
pub fn kept_out(s: &Settings, tops: &[String]) -> Vec<String> {
    tops.iter().filter(|t| !is_picked(s, t)).cloned().collect()
}

/// The settings in line with the 清单 (`drive_tops`: the drive's top folders when it's in; `mac`: ~/Downloads'),
/// and the tasks that went. Old settings get `picked` once the drive is in: what wasn't kept out, and the tasks'
/// folders (a fresh start ticks nothing). Picked names neither side has any more go (the drive in). Each picked
/// folder ~/Downloads has is a task to the drive folder of the same name (an old task of that folder, or with
/// that drive folder, keeps its id); other tasks go.
pub fn settle(j: &Janitor, s: &Settings, drive_tops: Option<&[String]>, mac: &[String]) -> (Settings, Vec<i64>) {
    let mut n = s.clone();
    let dl = downloads(j);
    if n.picked.is_none() {
        if n.tasks.is_empty() && n.skipped.is_empty() {
            n.picked = Some(Vec::new());
        } else if let Some(tops) = drive_tops {
            let mut p: Vec<String> = tops.iter().filter(|t| !n.skipped.iter().any(|k| same(k, t))).cloned().collect();
            p.extend(n.tasks.iter().filter_map(|t| t.source.strip_prefix(&format!("{dl}/")).filter(|r| !r.contains('/')).map(str::to_string)));
            n.picked = Some(p);
            n.skipped.clear();
        } else {
            return (n, Vec::new());
        }
    }
    let p = n.picked.get_or_insert_with(Vec::new);
    if let Some(tops) = drive_tops {
        p.retain(|x| tops.iter().chain(mac).any(|t| same(t, x)));
    }
    let mut seen_names: Vec<String> = Vec::new();
    p.retain(|x| {
        let k = diff::nfc(x);
        let fresh = listable(x) && !seen_names.contains(&k);
        seen_names.push(k);
        fresh
    });
    p.sort();
    let picked = p.clone();
    let mut next = n.next_id.max(n.tasks.iter().map(|t| t.id).max().unwrap_or(0));
    let mut tasks: Vec<Task> = Vec::new();
    for name in mac.iter().filter(|m| picked.iter().any(|x| same(x, m))) {
        let source = format!("{dl}/{name}");
        let was = n.tasks.iter().find(|t| same(&t.source, &source)).or_else(|| n.tasks.iter().find(|t| same(&t.target, name))).filter(|t| !tasks.iter().any(|x| x.id == t.id));
        let id = match was {
            Some(t) => t.id,
            None => {
                next += 1;
                next
            }
        };
        tasks.push(Task { id, source, target: name.clone() });
    }
    let gone: Vec<i64> = n.tasks.iter().filter(|t| !tasks.iter().any(|x| x.id == t.id)).map(|t| t.id).collect();
    n.tasks = tasks;
    n.next_id = next;
    (n, gone)
}

/// The settings in line with the 清单 now (what's on the drive and in ~/Downloads); not stored.
pub fn settled(j: &Janitor, s: &Settings) -> Settings {
    let tops = drive_of(j, s).map(|v| top_folders(&v));
    settle(j, s, tops.as_deref(), &mac_folders(j)).0
}

/// Ticks a top folder in the 清单 (`on`) or takes it out; the settings with it (settled) and the tasks that went.
pub fn set_picked(j: &Janitor, s: &Settings, name: &str, on: bool) -> Result<(Settings, Vec<i64>), String> {
    let v = drive_of(j, s).ok_or("插上移动硬盘才能改清单")?;
    let (tops, mac) = (top_folders(&v), mac_folders(j));
    let (mut n, _) = settle(j, s, Some(&tops), &mac);
    let name = tops.iter().chain(&mac).find(|t| same(t, name)).cloned().ok_or("硬盘上和 Downloads 里都没有这个文件夹")?;
    let p = n.picked.get_or_insert_with(Vec::new);
    p.retain(|x| !same(x, &name));
    if on {
        p.push(name);
    }
    let (n, _) = settle(j, &n, Some(&tops), &mac);
    let gone = s.tasks.iter().filter(|t| !n.tasks.iter().any(|x| x.id == t.id)).map(|t| t.id).collect();
    Ok((n, gone))
}

// ---- comparing ----

/// One task compared (the Mac's listing too, for `backup_seen`).
pub fn compare_task(j: &Janitor, drive: &Volume, t: &Task, stop: &AtomicBool) -> Result<(Diff, diff::Listing), String> {
    if let Some(p) = source_problem(j, &t.source) {
        return Err(p);
    }
    let mac = diff::list(&j.real_of(&t.source), &Skip::default(), stop)?;
    let on_drive = diff::list(&drive.path.join(&t.target), &Skip::default(), stop)?;
    let seen = j.with_db(|c| store::seen(c, t.id))?;
    Ok((diff::compare(&mac, &on_drive, &seen), mac))
}

/// Compares every task with the drive (when it's connected) and keeps the result for the page.
pub fn compare_all(j: &Janitor, s: &Settings) {
    if j.backup.comparing.swap(true, Ordering::SeqCst) {
        return;
    }
    let snap = drive_of(j, s).map(|v| {
        let stop = AtomicBool::new(false);
        let diffs: HashMap<i64, Result<Diff, String>> = s.tasks.iter().map(|t| (t.id, compare_task(j, &v, t, &stop).map(|(d, _)| d))).collect();
        let (hints, tops) = match hints::work_out(j, &v.path, s, &diffs, &stop) {
            Ok((h, t)) => (Some(h), t),
            Err(_) => (None, HashMap::new()),
        };
        Snapshot { uuid: v.uuid.clone(), at: unix_now(), tasks: snap_key(s), diffs, hints, tops }
    });
    if let Ok(mut g) = j.backup.snapshot.lock() {
        *g = snap;
    }
    j.backup.comparing.store(false, Ordering::SeqCst);
}

fn compare_in_background(j: &Janitor, s: Settings) {
    let j = j.clone();
    std::thread::spawn(move || compare_all(&j, &s));
}

// ---- what the page gets ----

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group<T> {
    pub count: usize,
    pub bytes: u64,
    /// The first `LIST_MAX`.
    pub rows: Vec<T>,
}

fn group<T: Clone>(v: &[T], size: impl Fn(&T) -> u64) -> Group<T> {
    Group { count: v.len(), bytes: v.iter().map(&size).sum(), rows: v.iter().take(LIST_MAX).cloned().collect() }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffView {
    pub new: Group<diff::Row>,
    pub changed: Group<diff::Row>,
    pub moved: Group<diff::Move>,
    pub held: Group<diff::Held>,
    pub recent: Group<diff::Row>,
    pub archived: Group<diff::Row>,
    pub same: u64,
    pub same_bytes: u64,
    pub denied: Vec<String>,
}

impl From<&Diff> for DiffView {
    fn from(d: &Diff) -> Self {
        DiffView {
            new: group(&d.new, |r| r.size),
            changed: group(&d.changed, |r| r.size),
            moved: group(&d.moved, |m| m.size),
            held: group(&d.held, |h| h.size),
            recent: group(&d.recent, |r| r.size),
            archived: group(&d.archived, |r| r.size),
            same: d.same,
            same_bytes: d.same_bytes,
            denied: d.denied.iter().take(20).cloned().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: i64,
    pub source: String,
    pub target: String,
    /// When it was last backed up whole.
    pub last: Option<String>,
    pub diff: Option<DiffView>,
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveView {
    pub uuid: String,
    pub name: String,
    pub connected: bool,
    pub fs: Option<String>,
    pub total: u64,
    pub free: u64,
    /// The real path when connected.
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub running: bool,
    pub files_total: u64,
    pub files_done: u64,
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub task: String,
    pub current: String,
    /// How the last run ended (this session).
    pub ended: Option<String>,
    /// Runs ended this session (a new one: pop `ended`).
    pub ends: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTask {
    pub id: i64,
    pub source: String,
    pub target: String,
    pub files: u64,
    pub bytes: u64,
    pub updated: u64,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRow {
    pub id: i64,
    /// drive | pikpak
    pub dest: String,
    pub drive: String,
    pub status: String,
    pub tasks: Vec<RunTask>,
    pub files: u64,
    pub bytes: u64,
    pub errors: Vec<(String, String)>,
    pub note: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub mode: Mode,
    pub home: String,
    pub drive: Option<DriveView>,
    /// External drives connected now (to pick one).
    pub volumes: Vec<Volume>,
    pub tasks: Vec<TaskView>,
    pub comparing: bool,
    pub compared_at: Option<i64>,
    pub run: RunView,
    pub runs: Vec<RunRow>,
    /// 柚子's last look (look.rs).
    pub look: Option<look::LookView>,
    pub pikpak: pikpak::PikPakView,
    /// 清单 (the drive in; empty otherwise).
    pub list: Vec<ListRow>,
}

/// A top folder in the 清单.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRow {
    pub name: String,
    pub on_drive: bool,
    pub on_mac: bool,
    pub picked: bool,
    /// Files and bytes (the drive's copy when it has one), when compared.
    pub files: Option<u64>,
    pub bytes: Option<u64>,
}

/// The drive's top folders (VAULT, 旧版 and hidden ones left out).
pub fn top_folders(v: &Volume) -> Vec<String> {
    dirs_in(&v.path)
}

fn list_rows(j: &Janitor, s: &Settings, v: &Volume, snap: Option<&Snapshot>) -> Vec<ListRow> {
    let (tops, mac) = (top_folders(v), mac_folders(j));
    let mut names: Vec<String> = tops.clone();
    names.extend(mac.iter().filter(|m| !tops.iter().any(|t| same(t, m))).cloned());
    names.sort_by_key(|n| n.to_lowercase());
    let dl = downloads(j);
    names
        .into_iter()
        .map(|name| {
            let (on_drive, on_mac) = (tops.iter().any(|t| same(t, &name)), mac.iter().any(|m| same(m, &name)));
            let size = snap.and_then(|sn| {
                if on_drive {
                    sn.tops.get(&diff::nfc(&name)).copied()
                } else {
                    sn.hints.as_ref()?.folders.iter().find(|f| same(&f.mac, &format!("{dl}/{name}"))).map(|f| (f.files, f.bytes))
                }
            });
            ListRow { picked: is_picked(s, &name), name, on_drive, on_mac, files: size.map(|x| x.0), bytes: size.map(|x| x.1) }
        })
        .collect()
}

pub fn run_view(j: &Janitor) -> RunView {
    let r = j.backup.run.lock().map(|g| g.clone()).unwrap_or_default();
    let text = |m: &Mutex<String>| m.lock().map(|g| g.clone()).unwrap_or_default();
    RunView {
        running: j.backup.running(),
        files_total: r.files_total.load(Ordering::Relaxed),
        files_done: r.files_done.load(Ordering::Relaxed),
        bytes_total: r.bytes_total.load(Ordering::Relaxed),
        bytes_done: r.bytes_done.load(Ordering::Relaxed),
        task: text(&r.task),
        current: text(&r.current),
        ended: j.backup.ended.lock().ok().and_then(|g| g.clone()),
        ends: j.backup.ends.load(Ordering::SeqCst),
    }
}

pub fn overview(j: &Janitor, s: &Settings, runs: Vec<RunRow>) -> Overview {
    let vols = j.backup.volumes.list();
    let mine = s.drive.as_ref().and_then(|d| vols.iter().find(|v| v.uuid == d.uuid).cloned());
    let drive = s.drive.as_ref().map(|d| match &mine {
        Some(v) => DriveView { uuid: v.uuid.clone(), name: v.name.clone(), connected: true, fs: Some(v.fs.clone()), total: v.total, free: v.free, path: Some(v.path.to_string_lossy().into_owned()) },
        None => DriveView { uuid: d.uuid.clone(), name: d.name.clone(), connected: false, fs: None, total: 0, free: 0, path: None },
    });
    let snap = j.backup.snapshot.lock().ok().and_then(|g| g.clone());
    let tasks_json = snap_key(s);
    let fresh = snap.as_ref().filter(|sn| mine.as_ref().is_some_and(|v| v.uuid == sn.uuid) && sn.tasks == tasks_json);
    // a drive just connected, or tasks changed: compare (the page shows 正在对 meanwhile)
    if mine.is_some() && fresh.is_none() && !j.backup.running() {
        compare_in_background(j, s.clone());
    }
    let last_done = |id: i64| runs.iter().find(|r| r.tasks.iter().any(|t| t.id == id && t.done)).and_then(|r| r.finished_at.clone());
    let tasks = s
        .tasks
        .iter()
        .map(|t| {
            let got = fresh.and_then(|sn| sn.diffs.get(&t.id));
            TaskView {
                id: t.id,
                source: t.source.clone(),
                target: t.target.clone(),
                last: last_done(t.id),
                diff: got.and_then(|r| r.as_ref().ok()).map(DiffView::from),
                problem: got.and_then(|r| r.as_ref().err().cloned()).or_else(|| source_problem(j, &t.source)),
            }
        })
        .collect();
    let pikpak = pikpak::view(j, s, &runs);
    Overview {
        mode: j.mode,
        home: j.home.clone(),
        drive,
        volumes: vols,
        tasks,
        comparing: j.backup.comparing.load(Ordering::SeqCst) || (mine.is_some() && fresh.is_none()),
        compared_at: fresh.map(|sn| sn.at),
        run: run_view(j),
        runs,
        look: None,
        pikpak,
        list: mine.as_ref().map(|v| list_rows(j, s, v, fresh)).unwrap_or_default(),
    }
}

// ---- running ----

#[derive(Debug, Default)]
pub struct Outcome {
    pub tasks: Vec<RunTask>,
    pub files: u64,
    pub bytes: u64,
    pub errors: Vec<(String, String)>,
    pub stopped: bool,
    /// Why it couldn't run at all.
    pub failed: Option<String>,
}

fn err_push(errors: &mut Vec<(String, String)>, path: String, why: String) {
    if errors.len() < ERRORS_MAX {
        errors.push((path, why));
    }
}

/// Copies what changed in `tasks` onto `drive` (`day`: the folder in ARCHIVE). Never deletes on the drive.
pub fn run_tasks(j: &Janitor, drive: &Volume, tasks: &[Task], run: &Run, day: &str) -> Outcome {
    let mut out = Outcome::default();
    // the plan first: totals for the progress, and room on the drive
    let mut plan = Vec::new();
    for t in tasks {
        *run.task.lock().unwrap_or_else(|e| e.into_inner()) = format!("对比 {}", t.target);
        match compare_task(j, drive, t, &run.stop) {
            Ok(p) => plan.push((t, p)),
            Err(e) if run.stop.load(Ordering::Relaxed) => {
                out.stopped = true;
                let _ = e;
                return out;
            }
            Err(e) => err_push(&mut out.errors, t.source.clone(), e),
        }
    }
    let bytes: u64 = plan.iter().map(|(_, (d, _))| d.to_copy()).sum();
    let files: u64 = plan.iter().map(|(_, (d, _))| d.files_to_copy()).sum();
    if bytes + MARGIN > drive.free {
        out.failed = Some(format!("移动硬盘的空间不够：这次要复制 {}，只剩 {}", gb(bytes), gb(drive.free)));
        return out;
    }
    run.bytes_total.store(bytes, Ordering::Relaxed);
    run.files_total.store(files, Ordering::Relaxed);
    for (t, (d, mac)) in plan {
        if out.stopped {
            break;
        }
        *run.task.lock().unwrap_or_else(|e| e.into_inner()) = t.target.clone();
        let (src, dst) = (j.real_of(&t.source), drive.path.join(&t.target));
        let mut done = RunTask { id: t.id, source: t.source.clone(), target: t.target.clone(), files: 0, bytes: 0, updated: 0, done: false };
        if let Err(e) = std::fs::create_dir_all(&dst) {
            err_push(&mut out.errors, t.target.clone(), format!("建不了文件夹：{e}"));
            out.tasks.push(done);
            continue;
        }
        for dir in &d.dirs {
            if let Err(e) = std::fs::create_dir_all(dst.join(dir)) {
                err_push(&mut out.errors, format!("{}/{dir}", t.target), format!("建不了文件夹：{e}"));
            }
        }
        for (rel, to) in &d.links {
            if let Err(e) = std::os::unix::fs::symlink(to, dst.join(rel)) {
                err_push(&mut out.errors, format!("{}/{rel}", t.target), format!("替身没建成：{e}"));
            }
        }
        let jobs = d.changed.iter().map(|r| (r, true)).chain(d.new.iter().map(|r| (r, false)));
        for (r, replace) in jobs {
            if !drive.path.exists() {
                out.failed = Some("移动硬盘拔掉了".into());
                out.stopped = true;
                break;
            }
            *run.current.lock().unwrap_or_else(|e| e.into_inner()) = r.path.clone();
            let target_file = dst.join(&r.path);
            if let Some(parent) = target_file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let before = run.bytes_done.load(Ordering::Relaxed);
            let res = copy::copy_file(&src.join(&r.path), &target_file, &run.stop, &run.bytes_done, || if replace { copy::keep_old(&drive.path, &t.target, &r.path, day).map(|_| ()) } else { Ok(()) });
            match res {
                Ok(()) => {
                    run.files_done.fetch_add(1, Ordering::Relaxed);
                    done.files += 1;
                    done.bytes += r.size;
                    done.updated += replace as u64;
                }
                Err(copy::Fail::Stopped) => {
                    run.bytes_done.store(before, Ordering::Relaxed);
                    out.stopped = true;
                    break;
                }
                Err(copy::Fail::Error(e)) => {
                    run.bytes_done.store(before + r.size, Ordering::Relaxed);
                    run.files_done.fetch_add(1, Ordering::Relaxed);
                    err_push(&mut out.errors, format!("{}/{}", t.target, r.path), e);
                }
            }
        }
        if !out.stopped {
            done.done = true;
            let rows = diff::seen_rows(&mac);
            if let Err(e) = j.with_db(|c| store::set_seen(c, t.id, Some(&rows))) {
                err_push(&mut out.errors, t.target.clone(), e);
            }
        }
        out.files += done.files;
        out.bytes += done.bytes;
        out.tasks.push(done);
    }
    copy::flush();
    out
}

fn gb(b: u64) -> String {
    if b >= rules::GB {
        format!("{:.2} GB", b as f64 / rules::GB as f64)
    } else {
        if b >= rules::MB {
            format!("{} MB", b / rules::MB)
        } else {
            format!("{} KB", (b / 1000).max(if b > 0 { 1 } else { 0 }))
        }
    }
}

/// Renames on the drive the moves I picked (by where the Mac has them), as compared just now.
pub fn apply_moves(j: &Janitor, drive: &Volume, t: &Task, picked: &[String]) -> Result<(usize, Vec<(String, String)>), String> {
    let (d, _) = compare_task(j, drive, t, &AtomicBool::new(false))?;
    let root = drive.path.join(&t.target);
    let (mut done, mut failed) = (0, Vec::new());
    for m in d.moved.iter().filter(|m| picked.contains(&m.to)) {
        let (from, to) = (root.join(&m.from), root.join(&m.to));
        let r = (|| {
            if std::fs::symlink_metadata(&to).is_ok() {
                return Err("新位置上已经有东西".to_string());
            }
            std::fs::create_dir_all(to.parent().ok_or("没有上一层")?).map_err(|e| e.to_string())?;
            std::fs::rename(&from, &to).map_err(|e| e.to_string())
        })();
        match r {
            Ok(()) => {
                done += 1;
                prune_empty(&root, from.parent());
            }
            Err(e) => failed.push((m.to.clone(), e)),
        }
    }
    Ok((done, failed))
}

/// Removes folders left empty by a move (a lone .DS_Store counts as empty), up to the task's folder.
fn prune_empty(root: &Path, mut dir: Option<&Path>) {
    while let Some(d) = dir.filter(|d| d.starts_with(root) && *d != root) {
        let names: Vec<String> = std::fs::read_dir(d).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str().map(str::to_string)).collect();
        if names.iter().any(|n| n != ".DS_Store") {
            return;
        }
        let _ = std::fs::remove_file(d.join(".DS_Store"));
        if std::fs::remove_dir(d).is_err() {
            return;
        }
        dir = d.parent();
    }
}

// ---- youzi.db: runs ----

fn now_text(c: &Connection) -> Result<String, String> {
    c.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |r| r.get(0)).map_err(crate::modules::common::err)
}

fn local_day(c: &Connection) -> String {
    c.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0)).unwrap_or_else(|_| "今天".into())
}

pub fn runs(c: &Connection, limit: i64) -> Result<Vec<RunRow>, String> {
    let err = crate::modules::common::err;
    let mut s = c.prepare("SELECT id, drive, status, tasks, files, bytes, errors, note, started_at, finished_at, dest FROM backup_runs ORDER BY id DESC LIMIT ?1").map_err(err)?;
    let rows = s
        .query_map([limit], |r| {
            Ok(RunRow {
                id: r.get(0)?,
                dest: r.get(10)?,
                drive: r.get(1)?,
                status: r.get(2)?,
                tasks: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
                files: r.get::<_, i64>(4)? as u64,
                bytes: r.get::<_, i64>(5)? as u64,
                errors: serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or_default(),
                note: r.get(7)?,
                started_at: r.get(8)?,
                finished_at: r.get(9)?,
            })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// A run left `running` (the App quit mid-run) is said as stopped (`dest`: the kind not running now).
fn settle_stale(c: &Connection, dest: &str) -> Result<(), String> {
    c.execute("UPDATE backup_runs SET status = 'stopped', note = coalesce(note, 'App 关掉时还没做完'), finished_at = coalesce(finished_at, started_at) WHERE status = 'running' AND dest = ?1", [dest]).map(|_| ()).map_err(crate::modules::common::err)
}

fn record_start(c: &Connection, drive: &str, tasks: &[Task]) -> Result<i64, String> {
    let planned: Vec<RunTask> = tasks.iter().map(|t| RunTask { id: t.id, source: t.source.clone(), target: t.target.clone(), files: 0, bytes: 0, updated: 0, done: false }).collect();
    c.execute("INSERT INTO backup_runs (drive, tasks) VALUES (?1, ?2)", params![drive, serde_json::to_string(&planned).map_err(|e| e.to_string())?]).map_err(crate::modules::common::err)?;
    Ok(c.last_insert_rowid())
}

fn record_end(c: &Connection, id: i64, o: &Outcome) -> Result<(), String> {
    let status = if o.failed.is_some() && o.tasks.iter().all(|t| !t.done) { "failed" } else if o.stopped { "stopped" } else { "done" };
    let at = now_text(c)?;
    c.execute(
        "UPDATE backup_runs SET status = ?2, tasks = ?3, files = ?4, bytes = ?5, errors = ?6, note = ?7, finished_at = ?8 WHERE id = ?1",
        params![id, status, serde_json::to_string(&o.tasks).map_err(|e| e.to_string())?, o.files as i64, o.bytes as i64, serde_json::to_string(&o.errors).map_err(|e| e.to_string())?, o.failed, at],
    )
    .map(|_| ())
    .map_err(crate::modules::common::err)
}

/// One line for the page about how a run ended.
fn ended_text(o: &Outcome) -> String {
    let head = match (&o.failed, o.stopped) {
        (Some(f), _) => f.clone(),
        (None, true) => format!("停下了：复制了 {} 个文件（{}）", o.files, gb(o.bytes)),
        (None, false) if o.files == 0 => "备份好了：没有要复制的".into(),
        (None, false) => format!("备份好了：复制了 {} 个文件（{}）", o.files, gb(o.bytes)),
    };
    if o.errors.is_empty() {
        head
    } else {
        format!("{head}；{} 个没成", o.errors.len())
    }
}

// ---- commands ----

fn load(state: &AppState) -> Settings {
    state.youzi.with_db(|c| Ok(settings(c))).unwrap_or_default()
}

fn store_settings(state: &AppState, s: &Settings) -> Result<(), String> {
    state.youzi.with_db(|c| pref_set(c, KEY, s))
}

/// The settings settled with the 清单, stored when that changed them (not while backing up): what the commands
/// and the watcher go by. The tasks that went forget what their folders held.
pub fn load_now(state: &AppState, j: &Janitor) -> Settings {
    let s = load(state);
    if j.backup.running() {
        return s;
    }
    let tops = drive_of(j, &s).map(|v| top_folders(&v));
    let (n, gone) = settle(j, &s, tops.as_deref(), &mac_folders(j));
    if n != s && store_settings(state, &n).is_ok() {
        for id in gone {
            let _ = j.with_db(|c| store::set_seen(c, id, None));
        }
    }
    n
}

fn overview_now(state: &AppState, j: &Janitor) -> Result<Overview, String> {
    ensure_fake(j)?;
    let s = load_now(state, j);
    let runs = state.youzi.with_db(|c| {
        if !j.backup.running() {
            settle_stale(c, "drive")?;
        }
        if !j.backup.cloud.running() {
            settle_stale(c, "pikpak")?;
        }
        runs(c, 30)
    })?;
    let mut o = overview(j, &s, runs);
    let now = look::fingerprint_now(j, &s);
    o.look = state.youzi.with_db(|c| look::view(c, now.as_deref()))?;
    Ok(o)
}

/// The page's poll; PikPak is compared when that's due.
#[tauri::command]
pub async fn janitor_backup_overview(app: tauri::AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>) -> Result<Overview, String> {
    let o = overview_now(&state, &janitor)?;
    pikpak::compare_when_due(&app);
    Ok(o)
}

/// 重新对比: compares again now (waits for it; PikPak after it, in the background), then 柚子 looks if the clues changed.
#[tauri::command]
pub async fn janitor_backup_compare(app: tauri::AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>) -> Result<Overview, String> {
    let (s, j) = (load_now(&state, &janitor), janitor.inner().clone());
    tauri::async_runtime::spawn_blocking(move || compare_all(&j, &s)).await.map_err(|e| e.to_string())?;
    pikpak::compare_later(&app);
    let _ = look::maybe(&app, false).await;
    overview_now(&state, &janitor)
}

/// Picks the drive (a connected one, by its UUID); None forgets it. A drive picked is as good as plugged in:
/// compared, then 柚子 looks.
#[tauri::command]
pub async fn janitor_backup_drive(app: tauri::AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, uuid: Option<String>) -> Result<Overview, String> {
    let mut s = load(&state);
    s.drive = match uuid {
        Some(u) => {
            let v = janitor.backup.volumes.find(&u).ok_or("这块硬盘没插着")?;
            Some(DriveId { uuid: v.uuid, name: v.name })
        }
        None => None,
    };
    store_settings(&state, &s)?;
    if s.drive.is_some() {
        let (j, s) = (janitor.inner().clone(), s.clone());
        tauri::async_runtime::spawn(async move {
            let _ = tauri::async_runtime::spawn_blocking(move || compare_all(&j, &s)).await;
            pikpak::plugged(&app);
            let _ = look::maybe(&app, false).await;
        });
    }
    overview_now(&state, &janitor)
}

/// 清单: ticks a top folder (`on`) or takes it out of the backup.
#[tauri::command]
pub async fn janitor_backup_pick(app: tauri::AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, name: String, on: bool) -> Result<Overview, String> {
    if janitor.backup.running() || janitor.backup.cloud.running() {
        return Err("正在备份，等它做完或先停下".into());
    }
    ensure_fake(&janitor)?;
    let (s, gone) = set_picked(&janitor, &load_now(&state, &janitor), &name, on)?;
    store_settings(&state, &s)?;
    for id in gone {
        janitor.with_db(|c| store::set_seen(c, id, None))?;
    }
    let o = overview_now(&state, &janitor)?;
    pikpak::compare_when_due(&app);
    Ok(o)
}

/// 开始: backs up the picked tasks in the background.
#[tauri::command]
pub async fn janitor_backup_start(app: tauri::AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, ids: Vec<i64>) -> Result<Overview, String> {
    let s = load_now(&state, &janitor);
    let tasks: Vec<Task> = s.tasks.iter().filter(|t| ids.contains(&t.id)).cloned().collect();
    if tasks.is_empty() {
        return Err("先勾要备份的文件夹".into());
    }
    let drive = drive_of(&janitor, &s).ok_or("移动硬盘没插上")?;
    if janitor.backup.running.swap(true, Ordering::SeqCst) {
        return Err("正在备份".into());
    }
    // PikPak gives way while the drive is written
    pikpak::yield_to_backup(&janitor);
    let run = Arc::new(Run::default());
    if let Ok(mut g) = janitor.backup.run.lock() {
        *g = run.clone();
    }
    if let Ok(mut g) = janitor.backup.ended.lock() {
        *g = None;
    }
    let (id, day) = match state.youzi.with_db(|c| Ok((record_start(c, &drive.name, &tasks)?, local_day(c)))) {
        Ok(v) => v,
        Err(e) => {
            janitor.backup.running.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };
    let j = janitor.inner().clone();
    std::thread::spawn(move || {
        let o = run_tasks(&j, &drive, &tasks, &run, &day);
        let st = app.state::<AppState>();
        let _ = st.youzi.with_db(|c| record_end(c, id, &o));
        if let Ok(mut g) = j.backup.ended.lock() {
            *g = Some(ended_text(&o));
        }
        j.backup.ends.fetch_add(1, Ordering::SeqCst);
        j.backup.running.store(false, Ordering::SeqCst);
        compare_all(&j, &load_now(&st, &j));
        pikpak::after_backup(&app);
    });
    overview_now(&state, &janitor)
}

/// Deletes a record from the list (not one still running). Not undone: the runs write these rows themselves.
#[tauri::command]
pub async fn janitor_backup_forget(state: State<'_, AppState>, janitor: State<'_, Janitor>, id: i64) -> Result<Overview, String> {
    let n = state.youzi.with_db(|c| c.execute("DELETE FROM backup_runs WHERE id = ?1 AND status != 'running'", [id]).map_err(crate::modules::common::err))?;
    if n == 0 {
        return Err("这条记录还没做完，或已经删了".into());
    }
    overview_now(&state, &janitor)
}

#[tauri::command]
pub fn janitor_backup_stop(janitor: State<'_, Janitor>) {
    if let Ok(r) = janitor.backup.run.lock() {
        r.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveOutcome {
    pub done: usize,
    pub failed: Vec<(String, String)>,
    pub overview: Overview,
}

/// 在硬盘上跟着改: renames on the drive the moves picked (by their Mac path).
#[tauri::command]
pub async fn janitor_backup_move(app: tauri::AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, id: i64, paths: Vec<String>) -> Result<MoveOutcome, String> {
    if janitor.backup.running() {
        return Err("正在备份，等它做完或先停下".into());
    }
    let s = load_now(&state, &janitor);
    let t = s.tasks.iter().find(|t| t.id == id).ok_or("没有这个文件夹的备份")?.clone();
    let drive = drive_of(&janitor, &s).ok_or("移动硬盘没插上")?;
    let j = janitor.inner().clone();
    let (done, failed) = tauri::async_runtime::spawn_blocking(move || {
        let r = apply_moves(&j, &drive, &t, &paths);
        compare_all(&j, &s);
        r
    })
    .await
    .map_err(|e| e.to_string())??;
    if done > 0 {
        pikpak::compare_later(&app);
    }
    Ok(MoveOutcome { done, failed, overview: overview_now(&state, &janitor)? })
}

/// Watches the drives: when mine appears (or its tasks changed), compares in the background, then 柚子 looks.
pub fn watch(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(3));
        let j = app.state::<Janitor>().inner().clone();
        let names = j.backup.volumes.names();
        let changed = j.backup.last_names.lock().map(|mut g| std::mem::replace(&mut *g, names.clone()) != names).unwrap_or(false);
        if !changed || j.backup.running() {
            continue;
        }
        if j.mode == Mode::Sandbox && ensure_fake(&j).is_err() {
            continue;
        }
        let s = load_now(&app.state::<AppState>(), &j);
        if s.drive.is_none() {
            continue;
        }
        compare_all(&j, &s);
        if drive_of(&j, &s).is_some() {
            pikpak::plugged(&app);
            look::maybe_later(&app);
        }
    });
}
