//! DevJanitor 备份 (1.1.0 Phase 1; requirements「DevJanitor › 备份」): a Mac folder → a folder on the external
//! drive, only what changed. Names and metadata only (diff.rs); the bytes are read only to copy them (copy.rs).
//! - Settings (youzi.db preference `assistant.janitor.backup`, not undone): the drive (its volume UUID) and the
//!   tasks (source: a virtual path under home; target: relative to the drive's root). Runs: youzi.db
//!   `backup_runs` (not undone). What each task's Mac folder held at its last backup: janitor.db `backup_seen`.
//! - The watcher (`watch`) looks at the drives every few seconds; when mine appears it compares every task and
//!   keeps the result for the page. Nothing is copied until I pick tasks and press 开始.
//! - 只加不删: a run never deletes on the drive. A changed file's old copy moves to 旧版 first (copy.rs);
//!   one newer on the drive is held. A rename or move is done on the drive only when I say so (`apply_moves`).
//! - VAULT (rules::sealed) and the drive's 旧版 are never a task's source or target, never listed.
//! - Sandbox (dev, tests, a scratch data dir): the fake Mac's folders and fake drives in `<data>/janitor-volumes`.
//! - 柚子 (Phase 2): each comparison also works out clues (hints.rs); the Agent reads them and proposes cards
//!   (cards.rs); a drive plugged in gets an automatic look (look.rs).
//! - PikPak (Phase 3): the whole drive mirrored by rclone in the background while it's in (pikpak.rs, rclone.rs).

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
    /// Name patterns left out (`*` any characters).
    pub excludes: Vec<String>,
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
}

/// The last comparison, when it's of the drive connected now and the tasks as they are.
fn fresh_snapshot(j: &Janitor, s: &Settings) -> Option<Snapshot> {
    let v = drive_of(j, s)?;
    let tasks = serde_json::to_string(&s.tasks).unwrap_or_default();
    j.backup.snapshot.lock().ok().and_then(|g| g.clone()).filter(|sn| sn.uuid == v.uuid && sn.tasks == tasks)
}

pub struct Backup {
    pub volumes: Volumes,
    snapshot: Mutex<Option<Snapshot>>,
    comparing: AtomicBool,
    running: AtomicBool,
    run: Mutex<Arc<Run>>,
    /// The last run's end (error or how it ended), for the page.
    ended: Mutex<Option<String>>,
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
        Self { volumes, snapshot: Mutex::new(None), comparing: AtomicBool::new(false), running: AtomicBool::new(false), run: Mutex::new(Arc::default()), ended: Mutex::new(None), last_names: Mutex::new(Vec::new()), data: data.to_path_buf(), cloud: pikpak::Cloud::default() }
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

// ---- checking a task ----

/// A real path (from a folder dialog) or a virtual one → virtual (the sandbox: a path inside the fake Mac's
/// folder loses that prefix; anything else is taken as virtual already).
fn virt_of_real(j: &Janitor, p: &str) -> String {
    let p = p.trim().trim_end_matches('/');
    let root = j.root.to_string_lossy().into_owned();
    match p.strip_prefix(&root) {
        Some(r) if j.mode == Mode::Sandbox && r.starts_with('/') => r.to_string(),
        _ => p.to_string(),
    }
}

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

/// A drive folder (relative, or a real path inside the drive) → relative, checked.
fn target_of(drive: Option<&Volume>, t: &str) -> Result<String, String> {
    let t = t.trim();
    let rel = match drive {
        Some(v) if t.starts_with('/') => {
            let root = v.path.to_string_lossy().into_owned();
            t.strip_prefix(&root).filter(|r| r.is_empty() || r.starts_with('/')).ok_or("要选移动硬盘里的文件夹")?.to_string()
        }
        _ if t.starts_with('/') => return Err("移动硬盘没插上：写硬盘里的文件夹名".into()),
        _ => t.to_string(),
    };
    let rel = rel.trim_matches('/').to_string();
    if rel.is_empty() {
        return Err("要选移动硬盘里的一个文件夹，不能是整块硬盘".into());
    }
    for part in rel.split('/') {
        if part.is_empty() || part == ".." || part.starts_with('.') {
            return Err("文件夹的名字不对".into());
        }
        if rules::sealed_name(part) {
            return Err("这个地方不能备份".into());
        }
    }
    if rel.split('/').next() == Some(copy::OLD) {
        return Err(format!("「{}」是放旧版的地方", copy::OLD));
    }
    Ok(rel)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub id: Option<i64>,
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub excludes: Vec<String>,
}

/// Adds or changes a task (checked); the settings with it.
pub fn save_task(j: &Janitor, mut s: Settings, t: TaskInput) -> Result<Settings, String> {
    let source = virt_of_real(j, &t.source);
    if let Some(p) = source_problem(j, &source) {
        return Err(p);
    }
    let drive = drive_of(j, &s);
    let target = target_of(drive.as_ref(), &t.target)?;
    if s.tasks.iter().filter(|o| Some(o.id) != t.id).any(|o| rules::within(&o.target, &target) || rules::within(&target, &o.target)) {
        return Err("别的任务已经用了这个文件夹（或它的上一层、下一层）".into());
    }
    let excludes: Vec<String> = t.excludes.iter().map(|e| e.trim().chars().take(60).collect::<String>()).filter(|e| !e.is_empty()).take(30).collect();
    match t.id {
        Some(id) => {
            let task = s.tasks.iter_mut().find(|o| o.id == id).ok_or("没有这个任务")?;
            *task = Task { id, source, target, excludes };
        }
        None => {
            s.next_id = s.next_id.max(s.tasks.iter().map(|o| o.id).max().unwrap_or(0)) + 1;
            s.tasks.push(Task { id: s.next_id, source, target, excludes });
        }
    }
    Ok(s)
}

// ---- comparing ----

/// One task compared (the Mac's listing too, for `backup_seen`).
pub fn compare_task(j: &Janitor, drive: &Volume, t: &Task, stop: &AtomicBool) -> Result<(Diff, diff::Listing), String> {
    if let Some(p) = source_problem(j, &t.source) {
        return Err(p);
    }
    let skip = Skip { extra: t.excludes.clone() };
    let mac = diff::list(&j.real_of(&t.source), &skip, stop)?;
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
        let hints = hints::work_out(j, &v.path, s, &diffs, &stop).ok();
        Snapshot { uuid: v.uuid.clone(), at: unix_now(), tasks: serde_json::to_string(&s.tasks).unwrap_or_default(), diffs, hints }
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
    pub excludes: Vec<String>,
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
    /// The real path (for the folder dialog) when connected.
    pub path: Option<String>,
    /// Its top-level folders (VAULT, 旧版 and hidden ones left out).
    pub folders: Vec<String>,
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
}

fn top_folders(v: &Volume) -> Vec<String> {
    let mut f: Vec<String> = std::fs::read_dir(&v.path)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| std::fs::symlink_metadata(e.path()).is_ok_and(|m| m.is_dir()))
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| !n.starts_with('.') && !rules::sealed_name(n) && n != copy::OLD)
        .collect();
    f.sort();
    f
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
    }
}

pub fn overview(j: &Janitor, s: &Settings, runs: Vec<RunRow>) -> Overview {
    let vols = j.backup.volumes.list();
    let mine = s.drive.as_ref().and_then(|d| vols.iter().find(|v| v.uuid == d.uuid).cloned());
    let drive = s.drive.as_ref().map(|d| match &mine {
        Some(v) => DriveView { uuid: v.uuid.clone(), name: v.name.clone(), connected: true, fs: Some(v.fs.clone()), total: v.total, free: v.free, path: Some(v.path.to_string_lossy().into_owned()), folders: top_folders(v) },
        None => DriveView { uuid: d.uuid.clone(), name: d.name.clone(), connected: false, fs: None, total: 0, free: 0, path: None, folders: Vec::new() },
    });
    let snap = j.backup.snapshot.lock().ok().and_then(|g| g.clone());
    let tasks_json = serde_json::to_string(&s.tasks).unwrap_or_default();
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
                excludes: t.excludes.clone(),
                last: last_done(t.id),
                diff: got.and_then(|r| r.as_ref().ok()).map(DiffView::from),
                problem: got.and_then(|r| r.as_ref().err().cloned()).or_else(|| source_problem(j, &t.source)),
            }
        })
        .collect();
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
        pikpak: pikpak::view(j, s),
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

/// Copies what changed in `tasks` onto `drive` (`day`: 旧版's folder). Never deletes on the drive.
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

fn overview_now(state: &AppState, j: &Janitor) -> Result<Overview, String> {
    ensure_fake(j)?;
    let s = load(state);
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

#[tauri::command]
pub async fn janitor_backup_overview(state: State<'_, AppState>, janitor: State<'_, Janitor>) -> Result<Overview, String> {
    overview_now(&state, &janitor)
}

/// 重新对比: compares again now (waits for it), then 柚子 looks if the clues changed.
#[tauri::command]
pub async fn janitor_backup_compare(app: tauri::AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>) -> Result<Overview, String> {
    let (s, j) = (load(&state), janitor.inner().clone());
    tauri::async_runtime::spawn_blocking(move || compare_all(&j, &s)).await.map_err(|e| e.to_string())?;
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

#[tauri::command]
pub async fn janitor_backup_task_save(state: State<'_, AppState>, janitor: State<'_, Janitor>, task: TaskInput) -> Result<Overview, String> {
    if janitor.backup.running() {
        return Err("正在备份，等它做完或先停下".into());
    }
    ensure_fake(&janitor)?;
    let s = save_task(&janitor, load(&state), task)?;
    store_settings(&state, &s)?;
    overview_now(&state, &janitor)
}

#[tauri::command]
pub async fn janitor_backup_task_delete(state: State<'_, AppState>, janitor: State<'_, Janitor>, id: i64) -> Result<Overview, String> {
    if janitor.backup.running() {
        return Err("正在备份，等它做完或先停下".into());
    }
    let mut s = load(&state);
    s.tasks.retain(|t| t.id != id);
    store_settings(&state, &s)?;
    janitor.with_db(|c| store::set_seen(c, id, None))?;
    overview_now(&state, &janitor)
}

/// 开始: backs up the picked tasks in the background.
#[tauri::command]
pub async fn janitor_backup_start(app: tauri::AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, ids: Vec<i64>) -> Result<Overview, String> {
    let s = load(&state);
    let tasks: Vec<Task> = s.tasks.iter().filter(|t| ids.contains(&t.id)).cloned().collect();
    if tasks.is_empty() {
        return Err("先选要备份的任务".into());
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
        j.backup.running.store(false, Ordering::SeqCst);
        compare_all(&j, &load(&st));
        pikpak::after_backup(&app);
    });
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
pub async fn janitor_backup_move(state: State<'_, AppState>, janitor: State<'_, Janitor>, id: i64, paths: Vec<String>) -> Result<MoveOutcome, String> {
    if janitor.backup.running() {
        return Err("正在备份，等它做完或先停下".into());
    }
    let s = load(&state);
    let t = s.tasks.iter().find(|t| t.id == id).ok_or("没有这个任务")?.clone();
    let drive = drive_of(&janitor, &s).ok_or("移动硬盘没插上")?;
    let j = janitor.inner().clone();
    let (done, failed) = tauri::async_runtime::spawn_blocking(move || {
        let r = apply_moves(&j, &drive, &t, &paths);
        compare_all(&j, &s);
        r
    })
    .await
    .map_err(|e| e.to_string())??;
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
        let s = load(&app.state::<AppState>());
        if s.drive.is_none() || (j.mode == Mode::Sandbox && ensure_fake(&j).is_err()) {
            continue;
        }
        compare_all(&j, &s);
        if drive_of(&j, &s).is_some() {
            pikpak::plugged(&app);
            look::maybe_later(&app);
        }
    });
}
