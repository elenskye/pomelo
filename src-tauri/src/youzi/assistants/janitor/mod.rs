//! DevJanitor (v0.8 阶段 6～8; requirements「DevJanitor」): 柚子's 电脑管家. Its page is the module
//! `/janitor` (磁盘 · 清理 · 环境 · 设置); the Agent comes in 阶段 8.
//! - Mode: `Real` only for the installed app on the real data dir: the real disk, real deleting. Otherwise
//!   `Sandbox`: the fake Mac in `<data dir>/janitor-sandbox` (fake.rs) — scanning, deleting and the Trash all
//!   happen there, commands aren't run.
//! - The scan (scan.rs) runs in the background when the page opens (if the last one is older than
//!   `FRESH`) or on 重新扫描; the page shows the last result meanwhile. Results live in janitor.db
//!   (store.rs; never backed up, exported or undone).
//! - 一键清理 deletes for good (after the page's confirm); listed items go to the Trash, a protected one only
//!   when the page confirmed twice (`protected_ok`); 清空废纸篓 empties ~/.Trash. Before deleting, Rust looks
//!   again: still there, not protected, not in use now. None of it is undone.
//! - Settings (youzi.db preference `assistant.janitor`): categories kept out of 一键清理, 很久没动 days, the
//!   Agent's model. Not undone. The Agent's rules (agent.rs, youzi.db `janitor_rules`) are their own groups
//!   in 一键清理; adding and deleting one is an undo step.

pub mod agent;
pub mod env;
pub mod fake;
pub mod readonly;
pub mod rules;
pub mod scan;
pub mod store;
pub mod system;
#[cfg(test)]
mod tests;

use crate::youzi::store::{pref_get, pref_set};
use crate::AppState;
use rules::Protected;
use rusqlite::Connection;
use scan::{Config, Progress};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use system::Sys;
use tauri::State;

pub const KEY: &str = "assistant.janitor";
/// A scan this recent isn't redone when the page opens.
pub const FRESH: i64 = 10 * 60;
const SANDBOX: &str = "janitor-sandbox";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Real,
    Sandbox,
}

pub fn mode(data_dir_overridden: bool) -> Mode {
    if cfg!(test) || cfg!(debug_assertions) || data_dir_overridden {
        Mode::Sandbox
    } else {
        Mode::Real
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Categories kept out of 一键清理.
    pub off: Vec<String>,
    /// A project untouched this long has its deps and builds checked by default.
    pub stale_days: u32,
    /// The Agent's model and 思考 (no model: 柚子's selection).
    pub agent: crate::youzi::assistants::Choice,
    /// My names for the built-in categories (id → name; none: the built-in name).
    pub names: std::collections::BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { off: Vec::new(), stale_days: 30, agent: Default::default(), names: Default::default() }
    }
}

impl Settings {
    /// A built-in category's name: mine, or its own.
    pub fn name_of(&self, c: &rules::Category) -> String {
        self.names.get(c.id).cloned().unwrap_or_else(|| c.name.to_string())
    }
}

pub fn settings(conn: &Connection) -> Settings {
    pref_get::<Settings>(conn, KEY).ok().flatten().unwrap_or_default()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanState {
    pub running: bool,
    pub phase: String,
    pub files: u64,
    pub bytes: u64,
    pub started_at: i64,
    pub error: Option<String>,
}

pub struct Inner {
    pub mode: Mode,
    /// The real path of the virtual `/`.
    root: PathBuf,
    home: String,
    db_path: PathBuf,
    db: Mutex<Option<Connection>>,
    state: Mutex<ScanState>,
    progress: Mutex<Arc<Progress>>,
    busy: AtomicBool,
    /// The Agent's commands running, by card.
    pub cmds: agent::Cmds,
}

#[derive(Clone)]
pub struct Janitor(pub Arc<Inner>);

impl std::ops::Deref for Janitor {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Janitor {
    /// `home`: the real home folder; `data`: Pomelo's data dir (janitor.db, the fake Mac).
    pub fn new(mode: Mode, home: &Path, data: &Path) -> Self {
        let (root, home) = match mode {
            Mode::Real => (PathBuf::from("/"), home.to_string_lossy().into_owned()),
            Mode::Sandbox => (data.join(SANDBOX), fake::HOME.to_string()),
        };
        Self(Arc::new(Inner { mode, root, home, db_path: data.join("janitor.db"), db: Mutex::new(None), state: Mutex::default(), progress: Mutex::new(Arc::default()), busy: AtomicBool::new(false), cmds: Mutex::default() }))
    }

    pub fn with_db<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T, String>) -> Result<T, String> {
        let mut g = self.db.lock().map_err(|_| "DevJanitor 的扫描结果打不开，请重启 App".to_string())?;
        if g.is_none() {
            *g = Some(store::open(&self.db_path)?);
        }
        f(g.as_mut().expect("opened"))
    }

    fn sys(&self) -> Sys {
        match self.mode {
            Mode::Real => Sys::Real,
            Mode::Sandbox => Sys::fake(&self.root),
        }
    }

    pub fn config(&self, site: &str, rules: Vec<agent::Rule>) -> Config {
        let skip = if self.mode == Mode::Real && !self.sys().full_disk_access() { scan::TCC.iter().map(|p| format!("{}/{p}", self.home)).collect() } else { Default::default() };
        Config { root: self.root.clone(), home: self.home.clone(), site: site.to_string(), allocated: self.mode == Mode::Real, skip, now: now(), threads: 8, rules }
    }

    pub fn real_of(&self, virt: &str) -> PathBuf {
        let rel = virt.trim_start_matches('/');
        if rel.is_empty() {
            self.root.clone()
        } else {
            self.root.join(rel)
        }
    }

    pub fn status(&self) -> ScanState {
        let mut s = self.state.lock().map(|g| g.clone()).unwrap_or_default();
        if s.running {
            if let Ok(p) = self.progress.lock() {
                s.files = p.files.load(Ordering::Relaxed);
                s.bytes = p.bytes.load(Ordering::Relaxed);
                s.phase = p.phase.lock().map(|g| g.clone()).unwrap_or_default();
            }
        }
        s
    }

    /// The 开发环境 of the last scan.
    pub fn envs(&self) -> Vec<env::Env> {
        self.with_db(|c| store::meta(c, "envs")).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn summary(&self) -> Option<Value> {
        self.with_db(|c| store::meta(c, "summary")).ok().flatten().and_then(|s| serde_json::from_str(&s).ok())
    }

    /// Starts a scan in the background (unless one runs, or — not forced — the last one is fresh).
    pub fn start(&self, site: String, rules: Vec<agent::Rule>, force: bool) -> ScanState {
        // a result from before the 开发环境 (阶段 6) isn't fresh either
        let has_envs = self.with_db(|c| store::meta(c, "envs")).ok().flatten().is_some();
        let fresh = has_envs && self.summary().and_then(|s| s["scannedAt"].as_i64()).is_some_and(|at| now() - at < FRESH);
        {
            let Ok(mut st) = self.state.lock() else { return ScanState::default() };
            if st.running || (fresh && !force) {
                drop(st);
                return self.status();
            }
            *st = ScanState { running: true, phase: "扫描磁盘".into(), started_at: now(), ..Default::default() };
        }
        let progress = Arc::new(Progress::default());
        if let Ok(mut p) = self.progress.lock() {
            *p = progress.clone();
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                if me.mode == Mode::Sandbox {
                    ensure_sandbox(&me.root)?;
                }
                let cfg = me.config(&site, rules);
                let out = scan::scan(&cfg, &me.sys(), &progress)?;
                let envs = serde_json::to_value(&out.envs).map_err(|e| e.to_string())?;
                me.with_db(|c| store::replace(c, &out.summary, &out.nodes, &out.items, &envs))
            })();
            if let Ok(mut st) = me.state.lock() {
                st.running = false;
                st.error = result.err();
            }
        });
        self.status()
    }

    /// Stops a running scan (the old result stays).
    pub fn stop(&self) {
        if let Ok(p) = self.progress.lock() {
            p.stop.store(true, Ordering::Relaxed);
        }
    }

    /// The 一键清理 total the 模块坞 shows: what can be checked, in the categories kept (and the rules still there).
    pub fn cleanable(&self, s: &Settings, rules: &[agent::Rule]) -> u64 {
        self.with_db(|c| store::items(c)).unwrap_or_default().iter().filter(|i| !i.listed && !s.off.contains(&i.kind) && i.extra["inUse"].is_null() && kind_known(&i.kind, rules)).map(|i| i.size).sum()
    }
}

/// A built-in category, or an Agent rule that's still there.
fn kind_known(kind: &str, rules: &[agent::Rule]) -> bool {
    rules::category(kind).is_some() || kind.strip_prefix("rule:").and_then(|n| n.parse::<i64>().ok()).is_some_and(|id| rules.iter().any(|r| r.id == id))
}

/// The fake Mac lives only in a folder named janitor-sandbox.
fn ensure_sandbox(root: &Path) -> Result<(), String> {
    if root.file_name().and_then(|n| n.to_str()) != Some(SANDBOX) {
        return Err("假 Mac 的位置不对".into());
    }
    fake::ensure(root)
}

// ---- what the page gets ----

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: i64,
    pub title: String,
    pub paths: Vec<String>,
    pub size: u64,
    pub newest: i64,
    pub project: Option<String>,
    pub checked: bool,
    pub in_use: Option<String>,
    pub protected: Option<String>,
    pub note: Option<String>,
    pub tag: Option<String>,
    pub dup: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
    pub rebuild: Option<String>,
    pub takes: Option<String>,
    pub network: Option<bool>,
    pub size: u64,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub mode: Mode,
    pub home: String,
    pub scan: ScanState,
    pub summary: Option<Value>,
    /// 一键清理, by category (the ones kept on).
    pub categories: Vec<Group>,
    /// Only listed.
    pub groups: Vec<Group>,
    pub cleanable: u64,
}

const PROJECT: [&str; 4] = [rules::NODE_MODULES, rules::RUST_TARGET, rules::PYTHON, rules::WEB_BUILD];

/// Checked by default: download caches, and a project's deps / builds untouched for `stale_days`.
pub fn default_checked(i: &store::ItemRow, s: &Settings, now: i64) -> bool {
    if !i.extra["inUse"].is_null() || i.extra["website"].as_bool() == Some(true) {
        return false;
    }
    if rules::category(&i.kind).is_some_and(|c| c.download) {
        return true;
    }
    if i.kind.starts_with("rule:") {
        return i.extra["checked"].as_bool() == Some(true);
    }
    PROJECT.contains(&i.kind.as_str()) && i.project_newest.is_some_and(|t| t < now - s.stale_days as i64 * 86400)
}

fn item_of(i: &store::ItemRow, s: &Settings, now: i64) -> Item {
    let text = |k: &str| i.extra[k].as_str().map(str::to_string);
    Item {
        id: i.id,
        title: i.title.clone(),
        paths: i.paths.clone(),
        size: i.size,
        newest: i.newest,
        project: i.project.clone(),
        checked: !i.listed && default_checked(i, s, now),
        in_use: text("inUse"),
        protected: text("protected"),
        note: text("note"),
        tag: text("tag"),
        dup: i.extra["dup"].as_u64(),
    }
}

pub fn overview(j: &Janitor, s: &Settings, agent_rules: &[agent::Rule]) -> Result<Overview, String> {
    let rows = j.with_db(|c| store::items(c))?;
    let t = now();
    let group = |id: String, name: String, rebuild: String, takes: String, network: bool| {
        let items: Vec<Item> = rows.iter().filter(|r| !r.listed && r.kind == id).map(|r| item_of(r, s, t)).collect();
        Group { id, name, rebuild: Some(rebuild), takes: (!takes.is_empty()).then_some(takes), network: Some(network), size: items.iter().map(|i| i.size).sum(), items }
    };
    let categories = rules::CATEGORIES
        .iter()
        .filter(|c| !s.off.iter().any(|o| o == c.id))
        .map(|c| group(c.id.into(), s.name_of(c), c.rebuild.into(), c.takes.into(), c.network))
        .chain(agent_rules.iter().map(|r| group(format!("rule:{}", r.id), r.name.clone(), r.rebuild.clone(), String::new(), false)))
        .filter(|g| !g.items.is_empty())
        .collect::<Vec<_>>();
    let groups = rules::GROUPS
        .iter()
        .map(|(id, name)| {
            let items: Vec<Item> = rows.iter().filter(|r| r.listed && r.kind == *id).map(|r| item_of(r, s, t)).collect();
            Group { id: (*id).into(), name: (*name).into(), rebuild: None, takes: None, network: None, size: items.iter().map(|i| i.size).sum(), items }
        })
        .filter(|g| !g.items.is_empty())
        .collect();
    let cleanable = categories.iter().flat_map(|g| &g.items).filter(|i| i.in_use.is_none()).map(|i| i.size).sum();
    Ok(Overview { mode: j.mode, home: j.home.clone(), scan: j.status(), summary: j.summary(), categories, groups, cleanable })
}

// ---- the ring chart ----

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ring {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub size: u64,
    pub files: u64,
    pub denied: u64,
    pub newest: i64,
    /// What's in it but too small to draw.
    pub rest: u64,
    pub children: Vec<Ring>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RingView {
    pub node: Ring,
    /// From the whole disk down to the parent.
    pub crumbs: Vec<(String, String)>,
}

const RING_DEPTH: usize = 3;
const RING_WIDTH: usize = 14;

fn ring(c: &Connection, n: store::NodeRow, depth: usize) -> Result<Ring, String> {
    let mut children = Vec::new();
    let mut shown = 0;
    if depth > 0 {
        for k in store::children(c, n.id)?.into_iter().take(RING_WIDTH) {
            shown += k.size;
            children.push(ring(c, k, depth - 1)?);
        }
    } else {
        shown = n.size;
    }
    Ok(Ring { rest: n.size.saturating_sub(shown), path: n.path, name: n.name, kind: n.kind, size: n.size, files: n.files, denied: n.denied, newest: n.newest, children })
}

pub fn ring_view(j: &Janitor, path: &str) -> Result<Option<RingView>, String> {
    j.with_db(|c| {
        let Some(n) = store::node_at(c, path)?.or(store::node_at(c, "/")?) else { return Ok(None) };
        let mut crumbs = Vec::new();
        let mut at = n.id;
        while let Some(p) = store::parent_of(c, at)? {
            crumbs.push((p.path.clone(), p.name.clone()));
            at = p.id;
        }
        crumbs.reverse();
        Ok(Some(RingView { node: ring(c, n, RING_DEPTH)?, crumbs }))
    })
}

// ---- cleaning ----

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub freed: u64,
    pub done: usize,
    /// (title, why) of the ones not done.
    pub failed: Vec<(String, String)>,
}

/// Deletes a file or folder; read-only files (Go's module cache) are made writable first.
fn remove(real: &Path) -> Result<(), String> {
    let m = std::fs::symlink_metadata(real).map_err(|_| "已经不在了".to_string())?;
    let r = if m.is_dir() { std::fs::remove_dir_all(real) } else { std::fs::remove_file(real) };
    match r {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && m.is_dir() => {
            let _ = std::process::Command::new("/bin/chmod").arg("-R").arg("u+w").arg(real).status();
            std::fs::remove_dir_all(real).map_err(|e| format!("删不掉：{e}"))
        }
        Err(e) => Err(format!("删不掉：{e}")),
    }
}

fn empty(real: &Path) -> Result<(), String> {
    for e in std::fs::read_dir(real).map_err(|e| format!("打不开：{e}"))?.flatten() {
        remove(&e.path())?;
    }
    Ok(())
}

/// Is it in use now (looked at again right before deleting)?
fn busy_now(i: &store::ItemRow, procs: &[system::Proc], apps: &std::collections::HashSet<String>) -> Option<String> {
    let dirs: Vec<&str> = i.project.iter().map(String::as_str).chain(i.paths.iter().map(String::as_str)).collect();
    if let Some(p) = procs.iter().find(|p| p.cwd.as_deref().is_some_and(|c| dirs.iter().any(|d| rules::within(c, d)))) {
        return Some(format!("{} 在跑", p.name));
    }
    if let Some(app) = i.extra["app"].as_str().filter(|a| !a.is_empty()) {
        if apps.contains(app) {
            return Some("App 开着".into());
        }
    }
    None
}

pub fn clean(j: &Janitor, ids: &[i64], s: &Settings, site: &str) -> Result<Outcome, String> {
    if j.busy.swap(true, Ordering::SeqCst) {
        return Err("正在清理".into());
    }
    let r = (|| {
        let rows = j.with_db(|c| store::items_by_id(c, ids))?;
        let protected = Protected::new(&j.home, site);
        let sys = j.sys();
        let (procs, apps) = sys.running();
        let mut out = Outcome::default();
        let mut gone = Vec::new();
        for i in rows.iter().filter(|i| !i.listed) {
            let why = if s.off.contains(&i.kind) {
                Some("这一类没放进一键清理".to_string())
            } else if let Some(p) = i.paths.iter().find_map(|p| protected.of(p)) {
                Some(format!("受保护（{p}）"))
            } else {
                busy_now(i, &procs, &apps)
            };
            if let Some(why) = why {
                out.failed.push((i.title.clone(), why));
                continue;
            }
            let how = i.extra["how"].as_str().unwrap_or("remove");
            let mut res = Ok(());
            for p in &i.paths {
                let real = j.real_of(p);
                res = match how {
                    "empty" => empty(&real),
                    "conda" => {
                        let program = j.real_of(i.extra["program"].as_str().unwrap_or(""));
                        sys.clean_command(&program, &["clean", "--all", "--yes"], || empty(&real))
                    }
                    _ => remove(&real),
                };
                if res.is_err() {
                    break;
                }
            }
            match res {
                Ok(()) => {
                    out.freed += i.size;
                    out.done += 1;
                    gone.push((i.id, if how == "remove" { i.paths.clone() } else { Vec::new() }, i.size));
                }
                Err(e) => out.failed.push((i.title.clone(), e)),
            }
        }
        j.with_db(|c| store::forget(c, &gone))?;
        Ok(out)
    })();
    j.busy.store(false, Ordering::SeqCst);
    r
}

pub fn trash(j: &Janitor, ids: &[i64], protected_ok: bool) -> Result<Outcome, String> {
    if j.busy.swap(true, Ordering::SeqCst) {
        return Err("正在清理".into());
    }
    let r = (|| {
        let rows = j.with_db(|c| store::items_by_id(c, ids))?;
        let sys = j.sys();
        let home_real = j.real_of(&j.home);
        let mut out = Outcome::default();
        let mut gone = Vec::new();
        for i in rows.iter().filter(|i| i.listed) {
            if i.extra["protected"].is_string() && !protected_ok {
                out.failed.push((i.title.clone(), "受保护，要再确认一次".into()));
                continue;
            }
            let res = i.paths.iter().try_for_each(|p| {
                let real = j.real_of(p);
                if !real.exists() && std::fs::symlink_metadata(&real).is_err() {
                    return Err("已经不在了".to_string());
                }
                sys.trash(&real, &home_real)
            });
            match res {
                Ok(()) => {
                    out.freed += i.size;
                    out.done += 1;
                    gone.push((i.id, i.paths.clone(), i.size));
                }
                Err(e) => out.failed.push((i.title.clone(), e)),
            }
        }
        j.with_db(|c| {
            store::forget(c, &gone)?;
            if out.freed > 0 {
                bump_trash(c, out.freed as i64, out.done as i64)?;
            }
            Ok(())
        })?;
        Ok(out)
    })();
    j.busy.store(false, Ordering::SeqCst);
    r
}

pub(crate) fn bump_trash(c: &Connection, size: i64, count: i64) -> Result<(), String> {
    let Some(mut s) = store::meta(c, "summary")?.and_then(|s| serde_json::from_str::<Value>(&s).ok()) else { return Ok(()) };
    let (old_size, old_count) = (s["trash"]["size"].as_i64().unwrap_or(0), s["trash"]["count"].as_i64().unwrap_or(0));
    s["trash"] = serde_json::json!({ "size": (old_size + size).max(0), "count": (old_count + count).max(0) });
    store::set_meta(c, "summary", &s.to_string())
}

pub fn empty_trash(j: &Janitor) -> Result<Outcome, String> {
    if j.busy.swap(true, Ordering::SeqCst) {
        return Err("正在清理".into());
    }
    let r = (|| {
        let bin = j.real_of(&format!("{}/.Trash", j.home));
        let size = j.summary().and_then(|s| s["trash"]["size"].as_u64()).unwrap_or(0);
        let entries = std::fs::read_dir(&bin).map_err(|_| "打不开废纸篓：给 Pomelo「完全磁盘访问权限」后再试".to_string())?;
        let mut out = Outcome::default();
        for e in entries.flatten() {
            match remove(&e.path()) {
                Ok(()) => out.done += 1,
                Err(why) => out.failed.push((e.file_name().to_string_lossy().into_owned(), why)),
            }
        }
        if out.failed.is_empty() {
            out.freed = size;
        }
        j.with_db(|c| {
            let count = store::meta(c, "summary")?.and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|s| s["trash"]["count"].as_i64()).unwrap_or(0);
            if out.failed.is_empty() {
                bump_trash(c, -(size as i64), -count)?;
                c.execute("UPDATE nodes SET size = 0 WHERE path = ?1", [format!("{}/.Trash", j.home)]).map_err(crate::modules::common::err)?;
            }
            Ok(())
        })?;
        Ok(out)
    })();
    j.busy.store(false, Ordering::SeqCst);
    r
}

// ---- commands ----

/// The website folder (virtual): the one 呦呦漫游 is set to, else the default.
fn site_folder(state: &AppState, j: &Janitor) -> String {
    let default = format!("{}/Documents/GitHub/personal-website", j.home);
    if j.mode == Mode::Sandbox {
        return default;
    }
    state.with_db(|c| crate::settings::get(c, crate::site::FOLDER_KEY)).ok().flatten().and_then(|v| v.as_str().map(str::to_string)).filter(|s| !s.is_empty()).unwrap_or(default)
}

fn load_settings(state: &AppState) -> Settings {
    state.youzi.with_db(|c| Ok(settings(c))).unwrap_or_default()
}

fn load_rules(state: &AppState) -> Vec<agent::Rule> {
    state.youzi.with_db(|c| agent::rules(c)).unwrap_or_default()
}

#[tauri::command]
pub async fn janitor_overview(state: State<'_, AppState>, janitor: State<'_, Janitor>) -> Result<Overview, String> {
    let s = load_settings(&state);
    overview(&janitor, &s, &load_rules(&state))
}

/// Opening the page scans unless the last scan is fresh; 重新扫描 forces one.
#[tauri::command]
pub async fn janitor_scan(state: State<'_, AppState>, janitor: State<'_, Janitor>, force: bool) -> Result<ScanState, String> {
    let site = site_folder(&state, &janitor);
    Ok(janitor.start(site, load_rules(&state), force))
}

#[tauri::command]
pub fn janitor_status(janitor: State<'_, Janitor>) -> ScanState {
    janitor.status()
}

#[tauri::command]
pub fn janitor_stop(janitor: State<'_, Janitor>) {
    janitor.stop()
}

#[tauri::command]
pub async fn janitor_ring(janitor: State<'_, Janitor>, path: Option<String>) -> Result<Option<RingView>, String> {
    ring_view(&janitor, path.as_deref().unwrap_or("/"))
}

#[tauri::command]
pub async fn janitor_clean(state: State<'_, AppState>, janitor: State<'_, Janitor>, ids: Vec<i64>) -> Result<Outcome, String> {
    let (s, site) = (load_settings(&state), site_folder(&state, &janitor));
    let j = janitor.inner().clone();
    tauri::async_runtime::spawn_blocking(move || clean(&j, &ids, &s, &site)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn janitor_trash(janitor: State<'_, Janitor>, ids: Vec<i64>, protected_ok: bool) -> Result<Outcome, String> {
    let j = janitor.inner().clone();
    tauri::async_runtime::spawn_blocking(move || trash(&j, &ids, protected_ok)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn janitor_empty_trash(janitor: State<'_, Janitor>) -> Result<Outcome, String> {
    let j = janitor.inner().clone();
    tauri::async_runtime::spawn_blocking(move || empty_trash(&j)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn janitor_memory() -> Result<system::Memory, String> {
    tauri::async_runtime::spawn_blocking(system::memory).await.map_err(|e| e.to_string())
}

/// Reveals a path the scan found (a node or an item's) in Finder.
#[tauri::command]
pub fn janitor_reveal(janitor: State<'_, Janitor>, path: String) -> Result<(), String> {
    let known = janitor.with_db(|c| Ok(store::node_at(c, &path)?.is_some() || store::items(c)?.iter().any(|i| i.paths.contains(&path) || i.project.as_deref() == Some(path.as_str()))))? || janitor.envs().iter().flat_map(|e| &e.items).any(|i| i.path.as_deref() == Some(path.as_str()));
    if !known {
        return Err("找不到这个位置".into());
    }
    system::reveal(&janitor.real_of(&path))
}

/// 环境页: the 开发环境 of the last scan.
#[tauri::command]
pub async fn janitor_envs(janitor: State<'_, Janitor>) -> Result<Vec<env::Env>, String> {
    Ok(janitor.envs())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    pub categories: Vec<rules::Category>,
    /// The Agent's rules in 一键清理.
    pub rules: Vec<agent::Rule>,
}

fn settings_view(state: &AppState) -> SettingsView {
    SettingsView { settings: load_settings(state), categories: rules::CATEGORIES.to_vec(), rules: load_rules(state) }
}

#[tauri::command]
pub fn janitor_settings(state: State<'_, AppState>) -> SettingsView {
    settings_view(&state)
}

/// The settings page's part (categories, days); the Agent's model stays.
#[tauri::command]
pub fn janitor_settings_set(state: State<'_, AppState>, settings: Settings) -> Result<SettingsView, String> {
    let old = load_settings(&state);
    let names = settings
        .names
        .into_iter()
        .filter_map(|(id, n)| {
            let c = rules::category(&id)?;
            let n: String = n.trim().chars().take(30).collect();
            (!n.is_empty() && n != c.name).then_some((id, n))
        })
        .collect();
    let s = Settings { off: settings.off.into_iter().filter(|o| rules::category(o).is_some()).collect(), stale_days: settings.stale_days.clamp(1, 3650), agent: old.agent, names };
    state.youzi.with_db(|c| pref_set(c, KEY, &s))?;
    Ok(settings_view(&state))
}

/// Renames an Agent rule (one undo step).
#[tauri::command]
pub fn janitor_rule_rename(state: State<'_, AppState>, id: i64, name: String) -> Result<SettingsView, String> {
    state.with_youzi_undo("改一键清理规则的名字", |c| agent::rename_rule(c, id, &name))?;
    Ok(settings_view(&state))
}

/// Deletes an Agent rule (one undo step).
#[tauri::command]
pub fn janitor_rule_delete(state: State<'_, AppState>, id: i64) -> Result<SettingsView, String> {
    state.with_youzi_undo("删掉一键清理的规则", |c| agent::delete_rule(c, id))?;
    Ok(settings_view(&state))
}

// ---- the Agent's page ----

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub session: i64,
    pub choice: crate::youzi::assistants::Choice,
}

/// The Agent's conversation (made when missing) and its model.
#[tauri::command]
pub fn janitor_agent(state: State<'_, AppState>) -> Result<AgentView, String> {
    let s = load_settings(&state);
    let session = state.youzi.with_db(|c| agent::open_chat(c, &s.agent.level()))?;
    Ok(AgentView { session, choice: s.agent })
}

/// 新对话: the old conversation goes.
#[tauri::command]
pub fn janitor_agent_new(state: State<'_, AppState>) -> Result<AgentView, String> {
    let s = load_settings(&state);
    if state.youzi.runs.busy_in(state.youzi.with_db(|c| agent::chat(c))?.unwrap_or(-1)) {
        return Err("还在回答，等它写完或先停止".into());
    }
    let session = state.youzi.with_db(|c| agent::new_chat(c, &s.agent.level()))?;
    Ok(AgentView { session, choice: s.agent })
}

#[tauri::command]
pub fn janitor_agent_set(state: State<'_, AppState>, choice: crate::youzi::assistants::Choice) -> Result<AgentView, String> {
    let mut s = load_settings(&state);
    s.agent = choice;
    state.youzi.with_db(|c| pref_set(c, KEY, &s))?;
    janitor_agent(state)
}

/// What the Agent is told (its SKILL, mine to rewrite, and the situation now).
#[tauri::command]
pub fn janitor_agent_prompt(state: State<'_, AppState>, janitor: State<'_, Janitor>) -> agent::Prompt {
    agent::prompt(&state, &janitor)
}

/// My SKILL for the Agent (None: back to the built-in one). A setting: not undone.
#[tauri::command]
pub fn janitor_agent_skill(state: State<'_, AppState>, janitor: State<'_, Janitor>, skill: Option<String>) -> Result<agent::Prompt, String> {
    state.youzi.with_db(|c| agent::set_skill(c, skill.as_deref()))?;
    Ok(agent::prompt(&state, &janitor))
}

#[tauri::command]
pub fn janitor_card(app: tauri::AppHandle, id: i64) -> Result<agent::Card, String> {
    agent::card_now(&app, id)
}

#[tauri::command]
pub async fn janitor_card_trash(app: tauri::AppHandle, id: i64, picked: Vec<usize>, protected_ok: bool) -> Result<agent::Card, String> {
    tauri::async_runtime::spawn_blocking(move || agent::run_trash(&app, id, &picked, protected_ok)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn janitor_card_run(app: tauri::AppHandle, id: i64, protected_ok: bool) -> Result<agent::Card, String> {
    agent::run_command(&app, id, protected_ok)
}

#[tauri::command]
pub fn janitor_card_stop(app: tauri::AppHandle, id: i64) -> Result<agent::Card, String> {
    agent::stop_command(&app, id)
}

#[tauri::command]
pub fn janitor_card_rule(app: tauri::AppHandle, id: i64) -> Result<agent::Card, String> {
    agent::add_rule(&app, id)
}

#[tauri::command]
pub fn janitor_card_cancel(app: tauri::AppHandle, id: i64) -> Result<agent::Card, String> {
    agent::cancel(&app, id)
}

/// 「可清 12.3 GB」 (模块坞).
pub fn live_text(bytes: u64) -> Option<String> {
    if bytes == 0 {
        return None;
    }
    Some(if bytes >= rules::GB { format!("可清 {:.1} GB", bytes as f64 / rules::GB as f64) } else { format!("可清 {} MB", (bytes / rules::MB).max(1)) })
}

/// System Settings → 隐私与安全性 → 完全磁盘访问权限.
#[tauri::command]
pub fn janitor_open_privacy() -> Result<(), String> {
    std::process::Command::new("/usr/bin/open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles").status().map(|_| ()).map_err(|e| e.to_string())
}
