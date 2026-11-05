//! The program's own scan (requirements「DevJanitor」, the coarse layer): walks the disk once in parallel,
//! then works out the folder tree to draw, the 一键清理 items and the listed groups. Nothing is read but
//! names, sizes and times — except the files that may be duplicates (size, then the first and last 64 KB,
//! then all of it).
//! - Sizes: real = allocated blocks (a sparse Docker.raw counts what it takes), the fake Mac = length (its
//!   files are sparse). A file with several hard links counts once, where it's met first; an item frees
//!   only its files whose every link is inside it (pnpm's node_modules link into the store, Conda's envs
//!   into `pkgs`: deleting those frees little, and the size says so).
//! - Never crosses into another volume, never follows symlinks, never looks for single files inside a
//!   bundle (`.app`, a Photos library…), never looks for projects inside an item (node_modules within
//!   node_modules). Places that need Full Disk Access are skipped (and counted) when it isn't given.
//! - Project activity: the newest file of the project outside its rebuildable dirs and `.git`.

use super::rules::{self, How, Needs, Protected};
use super::store::{ItemRow, NodeRow};
use super::system::{Proc, Sys};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::hash::Hasher;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

/// Folders and files at least this big are kept for the ring chart.
pub const KEEP: u64 = 20 * rules::MB;
const NONE: u32 = u32::MAX;

pub struct Config {
    /// The real path of the virtual `/`.
    pub root: PathBuf,
    pub home: String,
    pub site: String,
    /// Count allocated blocks (the real disk) or lengths (the fake Mac).
    pub allocated: bool,
    /// Virtual paths not to enter (Full Disk Access not given): counted as 没权限.
    pub skip: HashSet<String>,
    pub now: i64,
    pub threads: usize,
    /// The Agent's rules (agent.rs): folders matching one are its 一键清理 items.
    pub rules: Vec<super::agent::Rule>,
}

impl Config {
    pub fn real_of(&self, virt: &str) -> PathBuf {
        let rel = virt.trim_start_matches('/');
        if rel.is_empty() {
            self.root.clone()
        } else {
            self.root.join(rel)
        }
    }
}

/// Places that need Full Disk Access (under home).
pub const TCC: [&str; 16] = [
    "Library/Mail",
    "Library/Messages",
    "Library/Safari",
    "Library/Containers",
    "Library/Group Containers",
    "Library/Cookies",
    "Library/HomeKit",
    "Library/Suggestions",
    "Library/Calendars",
    "Library/Reminders",
    "Library/Metadata/CoreSpotlight",
    "Library/Application Support/AddressBook",
    "Library/Application Support/CallHistoryDB",
    "Library/Application Support/com.apple.TCC",
    "Library/Biome",
    "Library/Daemon Containers",
];

#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    pub phase: Mutex<String>,
    pub stop: AtomicBool,
}

impl Progress {
    fn phase(&self, p: &str) {
        if let Ok(mut g) = self.phase.lock() {
            *g = p.to_string();
        }
    }
}

// ---- the walk ----

#[derive(Clone)]
struct Dir {
    parent: u32,
    name: Box<str>,
    own: u64,
    files: u32,
    newest: i64,
    markers: u16,
    denied: bool,
    /// Bytes of own files with more than one link (counted here because met first).
    linked: u64,
    seed: u32,
}

#[derive(Clone)]
struct FileRec {
    dir: u32,
    name: Box<str>,
    size: u64,
    mtime: i64,
    ino: (u64, u64),
    /// Outside ~/Library, not in a bundle, an item or the Trash.
    personal: bool,
    downloads: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum SeedKind {
    /// An Agent rule's folder (the rule's id).
    Rule(i64),
    /// A project's dir (node_modules…): the category.
    Project(&'static str),
    Place(&'static str, &'static str, How),
    AppCache(&'static str),
    UnknownCache,
    Conda(String),
    DeviceSupport,
    Archive,
    /// One of Mail's places (rules::MAIL_PLACES): title, what deleting means.
    Mail(&'static str, &'static str),
}

#[derive(Debug, Clone)]
struct Seed {
    dir: u32,
    kind: SeedKind,
}

#[derive(Clone)]
struct Job {
    dir: u32,
    real: PathBuf,
    virt: String,
    dev: u64,
    seed: u32,
    bundle: bool,
    personal: bool,
    downloads: bool,
}

struct Links {
    /// (dev, ino) → (links, seed, seen in that seed, size, met elsewhere too)
    inside: HashMap<(u64, u64), (u64, u32, u64, u64, bool)>,
    seen: HashSet<(u64, u64)>,
}

struct Walk<'a> {
    cfg: &'a Config,
    progress: &'a Progress,
    dirs: Mutex<Vec<Dir>>,
    files: Mutex<Vec<FileRec>>,
    seeds: Mutex<Vec<Seed>>,
    apps: Mutex<Vec<String>>,
    leftover_dirs: Mutex<Vec<(u32, String)>>,
    links: Mutex<Links>,
    queue: Mutex<Vec<Job>>,
    cv: Condvar,
    pending: AtomicUsize,
    conda_pkgs: Vec<String>,
    watched: Vec<String>,
    places: Vec<(String, &'static str, &'static str, How)>,
    mail: Vec<(String, &'static str, &'static str)>,
    device_support: Vec<String>,
    caches: String,
    archives: String,
    trash: String,
    library: String,
    downloads: String,
    app_dirs: Vec<String>,
}

fn join(virt: &str, name: &str) -> String {
    if virt == "/" {
        format!("/{name}")
    } else {
        format!("{virt}/{name}")
    }
}

fn mtime(m: &std::fs::Metadata) -> i64 {
    m.mtime()
}

impl<'a> Walk<'a> {
    fn new(cfg: &'a Config, progress: &'a Progress) -> Self {
        let home = &cfg.home;
        let at = |p: &str| format!("{home}/{p}");
        let conda_pkgs = rules::CONDA_ROOTS.iter().map(|r| if let Some(rest) = r.strip_prefix("~/") { format!("{}/pkgs", at(rest)) } else { format!("{r}/pkgs") }).collect();
        let watched = ["Library/Application Support", "Library/Containers", "Library/Group Containers", "Library/Saved Application State", "Library/HTTPStorages", "Library/WebKit", "Library/Caches", "Library/Preferences", "Library/Logs"].iter().map(|p| at(p)).collect();
        Self {
            cfg,
            progress,
            dirs: Mutex::new(Vec::new()),
            files: Mutex::new(Vec::new()),
            seeds: Mutex::new(Vec::new()),
            apps: Mutex::new(Vec::new()),
            leftover_dirs: Mutex::new(Vec::new()),
            links: Mutex::new(Links { inside: HashMap::new(), seen: HashSet::new() }),
            queue: Mutex::new(Vec::new()),
            cv: Condvar::new(),
            pending: AtomicUsize::new(0),
            conda_pkgs,
            watched,
            places: rules::HOME_PLACES.iter().map(|(p, c, t, h)| (at(p), *c, *t, *h)).collect(),
            mail: rules::MAIL_PLACES.iter().map(|(p, t, n)| (at(p), *t, *n)).collect(),
            device_support: rules::DEVICE_SUPPORT.iter().map(|p| at(p)).collect(),
            caches: at(rules::CACHES_DIR),
            archives: at(rules::ARCHIVES_DIR),
            trash: at(".Trash"),
            library: at("Library"),
            downloads: at("Downloads"),
            app_dirs: vec!["/Applications".into(), at("Applications")],
        }
    }

    fn push(&self, jobs: Vec<Job>) {
        if jobs.is_empty() {
            return;
        }
        self.pending.fetch_add(jobs.len(), Ordering::SeqCst);
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.extend(jobs);
        drop(q);
        self.cv.notify_all();
    }

    fn worker(&self) {
        loop {
            let job = {
                let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if let Some(j) = q.pop() {
                        break Some(j);
                    }
                    if self.pending.load(Ordering::SeqCst) == 0 {
                        break None;
                    }
                    q = self.cv.wait(q).unwrap_or_else(|e| e.into_inner());
                }
            };
            let Some(job) = job else {
                self.cv.notify_all();
                return;
            };
            if !self.progress.stop.load(Ordering::Relaxed) {
                self.visit(job);
            }
            if self.pending.fetch_sub(1, Ordering::SeqCst) == 1 {
                self.cv.notify_all();
            }
        }
    }

    fn seed_for(&self, parent_virt: &str, name: &str, virt: &str, parent_markers: u16, real: &Path) -> Option<SeedKind> {
        // .vercel/output: the dot folder is the project's own
        if let Some(r) = self.cfg.rules.iter().find(|r| super::agent::matches(&r.pattern, virt)) {
            return Some(SeedKind::Rule(r.id));
        }
        let (projects, parent_in) = (rules::in_projects(&self.cfg.home, virt), rules::in_projects(&self.cfg.home, parent_virt));
        for (dir, cat, needs) in &rules::PROJECT_DIRS {
            if *dir != name || !(if matches!(needs, Needs::Under(_)) { parent_in } else { projects }) {
                continue;
            }
            let ok = match needs {
                Needs::Parent(m) => rules::markers_have(parent_markers, m),
                Needs::Inside(f) => real.join(f).exists(),
                Needs::Under(p) => parent_virt.rsplit('/').next() == Some(*p),
                Needs::Any => true,
            };
            if ok {
                return Some(SeedKind::Project(cat));
            }
        }
        if let Some((_, c, t, h)) = self.places.iter().find(|(p, ..)| p == virt) {
            return Some(SeedKind::Place(c, t, *h));
        }
        if let Some((_, t, n)) = self.mail.iter().find(|(p, ..)| p == virt) {
            return Some(SeedKind::Mail(t, n));
        }
        if self.conda_pkgs.iter().any(|p| p == virt) {
            return Some(SeedKind::Conda(parent_virt.to_string()));
        }
        if parent_virt == self.caches {
            return Some(rules::app_cache(name).map_or(SeedKind::UnknownCache, SeedKind::AppCache));
        }
        if self.device_support.iter().any(|d| d == parent_virt) {
            return Some(SeedKind::DeviceSupport);
        }
        if name.ends_with(".xcarchive") && rules::within(virt, &self.archives) {
            return Some(SeedKind::Archive);
        }
        None
    }

    fn visit(&self, job: Job) {
        let entries = match std::fs::read_dir(&job.real) {
            Ok(r) => r,
            Err(_) => {
                let mut d = self.dirs.lock().unwrap_or_else(|e| e.into_inner());
                d[job.dir as usize].denied = true;
                return;
            }
        };
        let mut list: Vec<(String, std::fs::Metadata)> = Vec::new();
        for e in entries.flatten() {
            if let Ok(m) = e.metadata() {
                list.push((e.file_name().to_string_lossy().into_owned(), m));
            }
        }
        let markers = list.iter().fold(0u16, |b, (n, _)| b | rules::marker_bit(n));
        let (mut own, mut files, mut newest, mut linked) = (0u64, 0u32, 0i64, 0u64);
        let mut recs: Vec<FileRec> = Vec::new();
        let mut kids: Vec<(String, std::fs::Metadata)> = Vec::new();
        let watched_here = self.watched.iter().any(|w| *w == job.virt);
        let in_trash = rules::within(&job.virt, &self.trash);
        for (name, m) in list {
            if m.is_dir() {
                kids.push((name, m));
                continue;
            }
            let size = if self.cfg.allocated { m.blocks() * 512 } else { m.len() };
            let ino = (m.dev(), m.ino());
            let mut counted = true;
            if m.nlink() > 1 && m.is_file() {
                let mut l = self.links.lock().unwrap_or_else(|e| e.into_inner());
                counted = l.seen.insert(ino);
                if job.seed != NONE {
                    let e = l.inside.entry(ino).or_insert((m.nlink(), job.seed, 0, size, false));
                    if e.1 == job.seed {
                        e.2 += 1;
                    } else {
                        e.4 = true;
                    }
                }
            }
            if counted {
                own += size;
                if m.nlink() > 1 {
                    linked += size;
                }
            }
            files += 1;
            newest = newest.max(mtime(&m));
            if !m.is_file() {
                continue;
            }
            let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
            let installer = job.downloads && size >= rules::INSTALLER_FILE && rules::INSTALLER_EXT.contains(&ext.as_str());
            if !job.bundle && (size >= rules::DUP_FILE || installer || watched_here) {
                recs.push(FileRec { dir: job.dir, name: name.into(), size, mtime: mtime(&m), ino, personal: job.personal && job.seed == NONE && !in_trash, downloads: job.downloads });
            }
        }
        self.progress.files.fetch_add(files as u64, Ordering::Relaxed);
        self.progress.bytes.fetch_add(own, Ordering::Relaxed);
        if !recs.is_empty() {
            self.files.lock().unwrap_or_else(|e| e.into_inner()).extend(recs);
        }
        // 0: nothing to note, 1: an installed app, 2: a Library entry that may be a removed app's
        let mut prepared: Vec<(Dir, Job, Option<SeedKind>, bool, u8)> = Vec::new();
        for (name, m) in kids {
            if m.dev() != job.dev {
                continue;
            }
            let virt = join(&job.virt, &name);
            let real = job.real.join(&name);
            let bundle = job.bundle || rules::is_bundle(&name);
            let seed = if job.seed == NONE && !job.bundle { self.seed_for(&job.virt, &name, &virt, markers, &real) } else { None };
            let skipped = self.cfg.skip.contains(&virt);
            let note = if name.ends_with(".app") && !job.bundle && self.app_dirs.iter().any(|a| rules::within(&job.virt, a)) {
                1
            } else if watched_here && job.seed == NONE {
                2
            } else {
                0
            };
            // personal files: not ~/Library, the Trash or a dot folder of home (tools' own: ~/.nvm, ~/.ollama…)
            let personal = job.personal && !rules::within(&virt, &self.library) && !rules::within(&virt, &self.trash) && !(job.virt == self.cfg.home && name.starts_with('.'));
            let downloads = job.downloads || virt == self.downloads;
            let dir = Dir { parent: job.dir, name: name.as_str().into(), own: 0, files: 0, newest: 0, markers: 0, denied: skipped, linked: 0, seed: NONE };
            let child = Job { dir: NONE, real, virt, dev: job.dev, seed: job.seed, bundle, personal, downloads };
            prepared.push((dir, child, seed, skipped, note));
        }
        let mut jobs = Vec::new();
        let (mut apps, mut lefts) = (Vec::new(), Vec::new());
        {
            let mut dirs = self.dirs.lock().unwrap_or_else(|e| e.into_inner());
            let me = &mut dirs[job.dir as usize];
            me.own = own;
            me.files = files;
            me.newest = me.newest.max(newest);
            me.markers = markers;
            me.linked = linked;
            let mut seeds = prepared.iter().any(|p| p.2.is_some()).then(|| self.seeds.lock().unwrap_or_else(|e| e.into_inner()));
            for (mut dir, mut child, seed, skipped, note) in prepared {
                let idx = dirs.len() as u32;
                child.dir = idx;
                if let (Some(kind), Some(s)) = (seed, seeds.as_mut()) {
                    let si = s.len() as u32;
                    s.push(Seed { dir: idx, kind });
                    dir.seed = si;
                    child.seed = si;
                }
                dirs.push(dir);
                match note {
                    1 => apps.push(child.virt.clone()),
                    2 => lefts.push((idx, child.virt.clone())),
                    _ => {}
                }
                if !skipped {
                    jobs.push(child);
                }
            }
        }
        if !apps.is_empty() {
            self.apps.lock().unwrap_or_else(|e| e.into_inner()).extend(apps);
        }
        if !lefts.is_empty() {
            self.leftover_dirs.lock().unwrap_or_else(|e| e.into_inner()).extend(lefts);
        }
        self.push(jobs);
    }
}

// ---- after the walk ----

pub struct Outcome {
    pub summary: Value,
    pub nodes: Vec<NodeRow>,
    pub items: Vec<ItemRow>,
    pub envs: Vec<super::env::Env>,
}

/// The whole walked tree, by virtual path (for the 开发环境: sizes of any folder, its biggest children).
pub struct Tree<'a> {
    dirs: &'a [Dir],
    total: &'a [u64],
    newest: &'a [i64],
    kids: Vec<Vec<u32>>,
    roots: Vec<(u32, String)>,
}

impl Tree<'_> {
    pub fn find(&self, virt: &str) -> Option<usize> {
        let (r, root) = self.roots.iter().filter(|(_, v)| rules::within(virt, v)).max_by_key(|(_, v)| v.len())?;
        let mut at = *r as usize;
        let rest = virt[root.len()..].trim_start_matches('/');
        if rest.is_empty() {
            return Some(at);
        }
        for seg in rest.split('/') {
            at = *self.kids[at].iter().find(|k| &*self.dirs[**k as usize].name == seg)? as usize;
        }
        Some(at)
    }

    /// A folder's size (0: not a folder the walk saw).
    pub fn size(&self, virt: &str) -> u64 {
        self.find(virt).map_or(0, |i| self.total[i])
    }

    pub fn newest(&self, virt: &str) -> i64 {
        self.find(virt).map_or(0, |i| self.newest[i])
    }

    /// The walk couldn't (or wasn't allowed to) read it, or the folder holding it.
    pub fn denied(&self, virt: &str) -> bool {
        let mut at = virt;
        loop {
            if let Some(i) = self.find(at) {
                return self.dirs[i].denied;
            }
            match at.rsplit_once('/') {
                Some((up, _)) if !up.is_empty() => at = up,
                _ => return false,
            }
        }
    }

    /// The folders in it, biggest first.
    pub fn list(&self, virt: &str) -> Vec<(String, u64)> {
        let Some(i) = self.find(virt) else { return Vec::new() };
        let mut v: Vec<(String, u64)> = self.kids[i].iter().map(|k| (self.dirs[*k as usize].name.to_string(), self.total[*k as usize])).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }
}

/// The scanned places: (virtual path, label).
pub fn roots(home: &str) -> Vec<(String, String)> {
    vec![
        ("/Applications".into(), "App".into()),
        (home.to_string(), "个人文件夹".into()),
        ("/Users/Shared".into(), "共享".into()),
        ("/Library".into(), "资源库".into()),
        ("/opt".into(), "/opt".into()),
        ("/usr/local".into(), "/usr/local".into()),
    ]
}

fn hash_part(path: &Path, size: u64) -> Option<u64> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    let n = f.read(&mut buf).ok()?;
    h.write(&buf[..n]);
    if size > 128 * 1024 {
        use std::io::Seek;
        f.seek(std::io::SeekFrom::Start(size - 64 * 1024)).ok()?;
        let n = f.read(&mut buf).ok()?;
        h.write(&buf[..n]);
    }
    Some(h.finish())
}

fn hash_all(path: &Path, stop: &AtomicBool) -> Option<u64> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            return Some(h.finish());
        }
        h.write(&buf[..n]);
    }
}

/// A bundle-id-like name behind a Library entry (`com.foo.bar.plist`, `group.com.foo`, `ABCDE12345.com.foo`).
pub fn bundle_like(name: &str) -> Option<String> {
    let mut n = name;
    for suffix in [".plist", ".savedState", ".binarycookies"] {
        n = n.strip_suffix(suffix).unwrap_or(n);
    }
    n = n.strip_prefix("group.").unwrap_or(n);
    if n.len() > 11 && n.as_bytes()[10] == b'.' && n[..10].bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) {
        n = &n[11..];
    }
    let parts: Vec<&str> = n.split('.').collect();
    let ok = parts.len() >= 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    (ok && !n.to_ascii_lowercase().starts_with("com.apple.")).then(|| n.to_ascii_lowercase())
}

/// An installed app owns it: the same id, one inside the other, or the same vendor (com.docker.install
/// belongs with com.docker.docker).
fn belongs(id: &str, installed: &HashSet<String>) -> bool {
    let vendor = |s: &str| s.splitn(3, '.').take(2).collect::<Vec<_>>().join(".");
    let v = vendor(id);
    installed.iter().any(|i| id == i || id.starts_with(&format!("{i}.")) || i.starts_with(&format!("{id}.")) || vendor(i) == v)
}

/// A version-ish sort key for 设备支持 folders ("17.2 (21C62)").
fn version_key(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).take(4).filter_map(|s| s.parse().ok()).collect()
}

/// Which tool keeps a package cache busy.
fn cache_tool(title: &str) -> &'static [&'static str] {
    match title {
        t if t.starts_with("npm") => &["npm"],
        t if t.starts_with("pnpm") => &["pnpm"],
        t if t.starts_with("cargo") => &["cargo"],
        t if t.starts_with("pip") => &["pip"],
        t if t.starts_with("Homebrew") => &["brew"],
        t if t.starts_with("Yarn") => &["yarn"],
        t if t.starts_with("Gradle") => &["gradle"],
        t if t.starts_with("Maven") => &["mvn"],
        t if t.starts_with("Go") => &["go"],
        _ => &[],
    }
}

pub fn scan(cfg: &Config, sys: &Sys, progress: &Progress) -> Result<Outcome, String> {
    let started = std::time::Instant::now();
    progress.phase("扫描磁盘");
    let walk = Walk::new(cfg, progress);
    let mut root_dirs: Vec<(u32, String, String)> = Vec::new();
    {
        let mut dirs = walk.dirs.lock().unwrap_or_else(|e| e.into_inner());
        let mut jobs = Vec::new();
        for (virt, label) in roots(&cfg.home) {
            // /Users/Shared and the rest only when they're there; home is under /Users, Shared too
            let real = cfg.real_of(&virt);
            let Ok(m) = std::fs::symlink_metadata(&real) else { continue };
            if !m.is_dir() {
                continue;
            }
            let idx = dirs.len() as u32;
            dirs.push(Dir { parent: NONE, name: label.as_str().into(), own: 0, files: 0, newest: 0, markers: 0, denied: false, linked: 0, seed: NONE });
            let personal = virt == cfg.home;
            jobs.push(Job { dir: idx, real, virt: virt.clone(), dev: m.dev(), seed: NONE, bundle: false, personal, downloads: false });
            root_dirs.push((idx, virt, label));
        }
        drop(dirs);
        walk.push(jobs);
    }
    std::thread::scope(|s| {
        for _ in 0..cfg.threads.max(1) {
            s.spawn(|| walk.worker());
        }
    });
    if progress.stop.load(Ordering::Relaxed) {
        return Err("扫描停下了".into());
    }
    progress.phase("整理结果");
    let dirs = walk.dirs.into_inner().unwrap_or_else(|e| e.into_inner());
    let files = walk.files.into_inner().unwrap_or_else(|e| e.into_inner());
    let seeds = walk.seeds.into_inner().unwrap_or_else(|e| e.into_inner());
    let apps = walk.apps.into_inner().unwrap_or_else(|e| e.into_inner());
    let leftover_dirs = walk.leftover_dirs.into_inner().unwrap_or_else(|e| e.into_inner());
    let links = walk.links.into_inner().unwrap_or_else(|e| e.into_inner());
    let n = dirs.len();

    // totals, bottom-up (a child always comes after its parent)
    let mut total: Vec<u64> = dirs.iter().map(|d| d.own).collect();
    let mut count: Vec<u64> = dirs.iter().map(|d| d.files as u64).collect();
    let mut newest: Vec<i64> = dirs.iter().map(|d| d.newest).collect();
    let mut linked: Vec<u64> = dirs.iter().map(|d| d.linked).collect();
    let mut denied: Vec<u64> = dirs.iter().map(|d| d.denied as u64).collect();
    let mut active: Vec<i64> = dirs.iter().map(|d| d.newest).collect();
    for i in (0..n).rev() {
        let p = dirs[i].parent;
        if p == NONE {
            continue;
        }
        let p = p as usize;
        total[p] += total[i];
        count[p] += count[i];
        newest[p] = newest[p].max(newest[i]);
        linked[p] += linked[i];
        denied[p] += denied[i];
        if dirs[i].seed == NONE && &*dirs[i].name != ".git" {
            active[p] = active[p].max(active[i]);
        }
    }
    let mut kids: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (i, d) in dirs.iter().enumerate() {
        if d.parent != NONE {
            kids[d.parent as usize].push(i as u32);
        }
    }
    let tree = Tree { dirs: &dirs, total: &total, newest: &newest, kids, roots: root_dirs.iter().map(|(i, v, _)| (*i, v.clone())).collect() };
    let mut paths: Vec<Option<String>> = vec![None; n];
    let path_of = |i: usize, paths: &mut Vec<Option<String>>| -> String {
        let mut chain: Vec<usize> = Vec::new();
        let mut at = i;
        loop {
            if let Some(p) = &paths[at] {
                let mut s = p.clone();
                for c in chain.iter().rev() {
                    s = join(&s, &dirs[*c].name);
                }
                return s;
            }
            if dirs[at].parent == NONE {
                let root = root_dirs.iter().find(|(r, ..)| *r as usize == at).map(|(_, v, _)| v.clone()).unwrap_or_default();
                paths[at] = Some(root.clone());
                let mut s = root;
                for c in chain.iter().rev() {
                    s = join(&s, &dirs[*c].name);
                }
                return s;
            }
            chain.push(at);
            at = dirs[at].parent as usize;
        }
    };
    let protected = Protected::new(&cfg.home, &cfg.site);
    let (procs, running_apps) = sys.running();

    // what each seed frees
    let mut freed_links: HashMap<u32, u64> = HashMap::new();
    for (links_n, seed, seen, size, elsewhere) in links.inside.values() {
        if !elsewhere && seen == links_n {
            *freed_links.entry(*seed).or_default() += size;
        }
    }
    let freeable = |si: u32| -> u64 {
        let d = seeds[si as usize].dir as usize;
        total[d].saturating_sub(linked[d]) + freed_links.get(&si).copied().unwrap_or(0)
    };

    let mut items: Vec<ItemRow> = Vec::new();
    let mut next_id = 1i64;
    let mut push = |items: &mut Vec<ItemRow>, listed: bool, kind: &str, title: String, ps: Vec<String>, size: u64, newest_at: i64, project: Option<String>, project_newest: Option<i64>, extra: Value| {
        // a 一键清理 item that frees nothing (pnpm's node_modules: links into the store) isn't worth a row
        if !listed && size < 5 * rules::MB {
            return;
        }
        items.push(ItemRow { id: next_id, listed, kind: kind.into(), title, paths: ps, size, newest: newest_at, project, project_newest, extra });
        next_id += 1;
    };
    let in_use_by = |dir: &str, tools: &[&str], procs: &[Proc]| -> Option<String> {
        procs.iter().find(|p| p.cwd.as_deref().is_some_and(|c| rules::within(c, dir)) || tools.iter().any(|t| p.name.eq_ignore_ascii_case(t) || p.name.starts_with(t))).map(|p| format!("{} 在跑", p.name))
    };

    // projects: one item per dir; __pycache__ grouped by project
    let mut pycache: HashMap<usize, (Vec<String>, u64, i64)> = HashMap::new();
    let mut device_support: HashMap<usize, Vec<(u32, String)>> = HashMap::new();
    let mut unknown_caches: Vec<(u32, String)> = Vec::new();
    for (si, seed) in seeds.iter().enumerate() {
        let si = si as u32;
        let d = seed.dir as usize;
        let virt = path_of(d, &mut paths);
        if protected.of(&virt).is_some() && !matches!(seed.kind, SeedKind::Archive | SeedKind::UnknownCache) {
            continue; // 一键清理 never enters a protected place
        }
        let size = freeable(si);
        match &seed.kind {
            SeedKind::Project(cat) if &*dirs[d].name == "__pycache__" => {
                let mut up = dirs[d].parent as usize;
                for _ in 0..8 {
                    if rules::project_root(dirs[up].markers) || dirs[up].parent == NONE {
                        break;
                    }
                    up = dirs[up].parent as usize;
                }
                let _ = cat;
                let e = pycache.entry(up).or_insert((Vec::new(), 0, 0));
                e.0.push(virt);
                e.1 += size;
                e.2 = e.2.max(newest[d]);
            }
            SeedKind::Project(cat) => {
                // the project is the repo holding it (a folder with .git, a few levels up), else its parent
                let parent = dirs[d].parent as usize;
                let (mut root, mut up) = (parent, parent);
                for _ in 0..4 {
                    if dirs[up].markers & rules::marker_bit(".git") != 0 {
                        root = up;
                        break;
                    }
                    if dirs[up].parent == NONE {
                        break;
                    }
                    up = dirs[up].parent as usize;
                }
                let project = path_of(root, &mut paths);
                let name = project.rsplit('/').next().unwrap_or(&project).to_string();
                let rel = virt.strip_prefix(&format!("{project}/")).unwrap_or(&dirs[d].name).to_string();
                let mut extra = json!({ "how": "remove" });
                if path_of(parent, &mut paths) == protected.site() && *cat == rules::NODE_MODULES {
                    extra["note"] = json!("呦呦漫游本地检查前要在网站文件夹跑一次 npm ci");
                    extra["website"] = json!(true);
                }
                if let Some(u) = in_use_by(&project, &[], &procs) {
                    extra["inUse"] = json!(u);
                }
                push(&mut items, false, cat, format!("{name} · {rel}"), vec![virt], size, newest[d], Some(project), Some(active[root]), extra);
            }
            SeedKind::Place(cat, title, how) => {
                let mut extra = json!({ "how": if *how == How::Empty { "empty" } else { "remove" } });
                if let Some(u) = in_use_by(&virt, cache_tool(title), &procs) {
                    extra["inUse"] = json!(u);
                }
                push(&mut items, false, cat, title.to_string(), vec![virt], size, newest[d], None, None, extra);
            }
            SeedKind::AppCache(app) => {
                let mut extra = json!({ "how": "remove", "app": app });
                if !app.is_empty() && running_apps.contains(*app) {
                    extra["inUse"] = json!("App 开着");
                }
                push(&mut items, false, rules::APP_CACHE, rules::app_cache_title(&dirs[d].name), vec![virt], size, newest[d], None, None, extra);
            }
            SeedKind::Conda(root) => {
                let mut extra = json!({ "how": "conda", "program": format!("{root}/bin/conda") });
                if let Some(u) = in_use_by(&virt, &["conda"], &procs) {
                    extra["inUse"] = json!(u);
                }
                push(&mut items, false, rules::CONDA, format!("Conda 的包缓存（{}）", root.rsplit('/').nth(1).unwrap_or("conda")), vec![virt], size, newest[d], None, None, extra);
            }
            SeedKind::Rule(id) => {
                let Some(r) = cfg.rules.iter().find(|r| r.id == *id) else { continue };
                let title = if r.pattern.contains('*') { format!("{} · {}", r.name, dirs[d].name) } else { r.name.clone() };
                let mut extra = json!({ "how": "remove", "checked": r.checked });
                if let Some(u) = in_use_by(&virt, &[], &procs) {
                    extra["inUse"] = json!(u);
                }
                push(&mut items, false, &format!("rule:{id}"), title, vec![virt], size, newest[d], None, None, extra);
            }
            SeedKind::DeviceSupport => device_support.entry(dirs[d].parent as usize).or_default().push((si, virt)),
            SeedKind::UnknownCache => unknown_caches.push((si, virt)),
            SeedKind::Mail(title, note) => {
                if size < rules::MB {
                    continue;
                }
                let mut extra = json!({ "note": note });
                if running_apps.contains("com.apple.mail") {
                    extra["inUse"] = json!("Mail 开着");
                }
                push(&mut items, true, rules::MAIL, title.to_string(), vec![virt], size, newest[d], None, None, extra);
            }
            SeedKind::Archive => {
                let extra = json!({ "protected": protected.of(&virt) });
                push(&mut items, true, rules::ARCHIVES, dirs[d].name.to_string(), vec![virt], size, newest[d], None, None, extra);
            }
        }
    }
    let mut seen_titles: HashMap<String, usize> = HashMap::new();
    for i in items.iter().filter(|i| i.kind == rules::APP_CACHE) {
        *seen_titles.entry(i.title.clone()).or_default() += 1;
    }
    for i in items.iter_mut().filter(|i| i.kind == rules::APP_CACHE && seen_titles.get(&i.title).is_some_and(|n| *n > 1)) {
        let folder = i.paths[0].rsplit('/').next().unwrap_or("").to_string();
        i.title = format!("{}（{folder}）", i.title);
    }
    for (root, (ps, size, at)) in pycache {
        let project = path_of(root, &mut paths);
        let name = project.rsplit('/').next().unwrap_or(&project).to_string();
        let mut extra = json!({ "how": "remove", "tag": format!("{} 个", ps.len()) });
        if let Some(u) = in_use_by(&project, &[], &procs) {
            extra["inUse"] = json!(u);
        }
        let act = active[root];
        push(&mut items, false, rules::PYTHON, format!("{name} · __pycache__"), ps, size, at, Some(project), Some(act), extra);
    }
    for (platform, mut versions) in device_support {
        versions.sort_by_key(|(si, _)| version_key(&dirs[seeds[*si as usize].dir as usize].name));
        versions.pop(); // keep the newest
        let what = dirs[platform].name.replace(" DeviceSupport", "");
        for (si, virt) in versions {
            let d = seeds[si as usize].dir as usize;
            push(&mut items, false, rules::XCODE, format!("{what} 设备支持 {}", dirs[d].name), vec![virt], freeable(si), newest[d], None, None, json!({ "how": "remove" }));
        }
    }

    // leftovers of removed apps: Library entries named like a bundle id no installed app owns
    let mut installed: HashSet<String> = running_apps.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut app_paths: Vec<PathBuf> = apps.iter().map(|v| cfg.real_of(v)).collect();
    if matches!(sys, Sys::Real) {
        for dir in ["/System/Applications", "/System/Applications/Utilities", "/System/Library/CoreServices"] {
            if let Ok(r) = std::fs::read_dir(dir) {
                app_paths.extend(r.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "app")));
            }
        }
    }
    for p in &app_paths {
        if let Some(id) = super::system::bundle_id(p) {
            installed.insert(id.to_ascii_lowercase());
        }
    }
    let mut leftovers: HashMap<String, (Vec<String>, u64, i64)> = HashMap::new();
    let mut leftover_paths: HashSet<String> = HashSet::new();
    let cleaned: HashSet<String> = items.iter().filter(|i| !i.listed).flat_map(|i| i.paths.iter().cloned()).collect();
    for (d, virt) in &leftover_dirs {
        let name = virt.rsplit('/').next().unwrap_or(virt);
        if cleaned.contains(virt) {
            continue;
        }
        if let Some(id) = bundle_like(name) {
            if !belongs(&id, &installed) && protected.of(virt).is_none() {
                let e = leftovers.entry(id).or_insert((Vec::new(), 0, 0));
                e.0.push(virt.clone());
                e.1 += total[*d as usize];
                e.2 = e.2.max(newest[*d as usize]);
                leftover_paths.insert(virt.clone());
            }
        }
    }
    for f in &files {
        let parent = path_of(f.dir as usize, &mut paths);
        if !walk_watched(&cfg.home, &parent) {
            continue;
        }
        if let Some(id) = bundle_like(&f.name) {
            if !belongs(&id, &installed) {
                let e = leftovers.entry(id).or_insert((Vec::new(), 0, 0));
                let virt = join(&parent, &f.name);
                e.0.push(virt.clone());
                e.1 += f.size;
                e.2 = e.2.max(f.mtime);
                leftover_paths.insert(virt);
            }
        }
    }
    let mut lv: Vec<(String, (Vec<String>, u64, i64))> = leftovers.into_iter().filter(|(_, (_, size, _))| *size >= rules::MB).collect();
    lv.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then(a.0.cmp(&b.0)));
    for (id, (ps, size, at)) in lv {
        let tag = format!("{} 处", ps.len());
        push(&mut items, true, rules::LEFTOVERS, id, ps, size, at, None, None, json!({ "tag": tag }));
    }
    for (si, virt) in unknown_caches {
        let size = freeable(si);
        let d = seeds[si as usize].dir as usize;
        if size < rules::MB || leftover_paths.contains(&virt) || rules::apple_cache(&dirs[d].name) {
            continue;
        }
        push(&mut items, true, rules::UNKNOWN_CACHE, dirs[d].name.to_string(), vec![virt], size, newest[d], None, None, json!({}));
    }

    // big and long-unopened files; installers in Downloads
    let personal: Vec<&FileRec> = files.iter().filter(|f| f.personal && f.size >= rules::OLD_FILE).collect();
    let real_paths: Vec<PathBuf> = personal.iter().map(|f| cfg.real_of(&join(&path_of(f.dir as usize, &mut paths), &f.name))).collect();
    let opened = sys.last_opened(&real_paths);
    let old_before = cfg.now - rules::OLD_DAYS * 86400;
    for (f, real) in personal.iter().zip(&real_paths) {
        let last = opened.get(real).copied().map_or(f.mtime, |o| o.max(f.mtime));
        let old = last < old_before;
        if f.size < rules::BIG_FILE && !old {
            continue;
        }
        let virt = join(&path_of(f.dir as usize, &mut paths), &f.name);
        let mut extra = json!({ "opened": last });
        if old {
            extra["tag"] = json!(format!("很久没打开 · {}", day_of(last)));
        }
        if let Some(p) = protected.of(&virt) {
            extra["protected"] = json!(p);
        }
        push(&mut items, true, rules::BIG, f.name.to_string(), vec![virt], f.size, f.mtime, None, None, extra);
    }
    for f in files.iter().filter(|f| f.downloads && f.size >= rules::INSTALLER_FILE) {
        let ext = f.name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if !rules::INSTALLER_EXT.contains(&ext.as_str()) {
            continue;
        }
        let virt = join(&path_of(f.dir as usize, &mut paths), &f.name);
        push(&mut items, true, rules::INSTALLERS, f.name.to_string(), vec![virt], f.size, f.mtime, None, None, json!({}));
    }

    // duplicates: same size → same first and last 64 KB → same content
    progress.phase("找重复的文件");
    let mut by_size: HashMap<u64, Vec<&FileRec>> = HashMap::new();
    for f in files.iter().filter(|f| f.personal && f.size >= rules::DUP_FILE) {
        by_size.entry(f.size).or_default().push(f);
    }
    let mut groups: Vec<Vec<(&FileRec, String)>> = Vec::new();
    let mut sizes: Vec<u64> = by_size.iter().filter(|(_, v)| v.len() > 1).map(|(s, _)| *s).collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    for size in sizes {
        if progress.stop.load(Ordering::Relaxed) {
            return Err("扫描停下了".into());
        }
        let mut seen_ino = HashSet::new();
        let cands: Vec<(&FileRec, String)> = by_size[&size].iter().filter(|f| seen_ino.insert(f.ino)).map(|f| (*f, join(&path_of(f.dir as usize, &mut paths), &f.name))).collect();
        if cands.len() < 2 {
            continue;
        }
        let mut parts: HashMap<u64, Vec<(&FileRec, String)>> = HashMap::new();
        for (f, v) in cands {
            if let Some(h) = hash_part(&cfg.real_of(&v), size) {
                parts.entry(h).or_default().push((f, v));
            }
        }
        for (_, same) in parts.into_iter().filter(|(_, v)| v.len() > 1) {
            let mut full: HashMap<u64, Vec<(&FileRec, String)>> = HashMap::new();
            for (f, v) in same {
                if let Some(h) = hash_all(&cfg.real_of(&v), &progress.stop) {
                    full.entry(h).or_default().push((f, v));
                }
            }
            groups.extend(full.into_values().filter(|v| v.len() > 1));
        }
        if groups.len() >= 200 {
            break;
        }
    }
    for (g, mut group) in groups.into_iter().enumerate() {
        group.sort_by(|a, b| a.1.cmp(&b.1));
        let copies = group.len();
        for (f, virt) in group {
            let mut extra = json!({ "dup": g + 1, "tag": format!("{copies} 份一样") });
            if let Some(p) = protected.of(&virt) {
                extra["protected"] = json!(p);
            }
            push(&mut items, true, rules::DUPLICATES, f.name.to_string(), vec![virt], f.size, f.mtime, None, None, extra);
        }
    }

    // the 开发环境 (env.rs), from the tree and the items found
    progress.phase("认开发环境");
    let envs = super::env::detect(&super::env::Ctx { cfg, sys, tree: &tree, items: &items, protected: &protected });

    // the tree to draw: the whole disk, the scanned places, 系统和其他
    progress.phase("整理结果");
    let scanned: u64 = root_dirs.iter().map(|(i, ..)| total[*i as usize]).sum();
    let disk = sys.disk(scanned);
    let system = disk.map_or(0, |(t, f)| t.saturating_sub(f).saturating_sub(scanned));
    let mut nodes: Vec<NodeRow> = Vec::new();
    nodes.push(NodeRow { id: 1, parent: None, name: "整块磁盘".into(), path: "/".into(), kind: "disk".into(), size: scanned + system, files: root_dirs.iter().map(|(i, ..)| count[*i as usize]).sum(), newest: 0, denied: root_dirs.iter().map(|(i, ..)| denied[*i as usize]).sum() });
    let home_idx = root_dirs.iter().find(|(_, v, _)| *v == cfg.home).map(|(i, ..)| *i as usize);
    let mut id_of: Vec<i64> = vec![0; n];
    let mut next_node = 2i64;
    for i in 0..n {
        // the scanned places too, unless nearly empty (home always)
        let keep = if dirs[i].parent == NONE { total[i] >= KEEP || home_idx == Some(i) } else { total[i] >= KEEP && id_of[dirs[i].parent as usize] != 0 };
        if !keep {
            continue;
        }
        let path = path_of(i, &mut paths);
        paths[i] = Some(path.clone());
        let parent = if dirs[i].parent == NONE { 1 } else { id_of[dirs[i].parent as usize] };
        id_of[i] = next_node;
        nodes.push(NodeRow { id: next_node, parent: Some(parent), name: dirs[i].name.to_string(), path, kind: "dir".into(), size: total[i], files: count[i], newest: newest[i], denied: denied[i] });
        next_node += 1;
    }
    for f in files.iter().filter(|f| f.size >= KEEP && id_of[f.dir as usize] != 0) {
        let parent_path = paths[f.dir as usize].clone().unwrap_or_default();
        nodes.push(NodeRow { id: next_node, parent: Some(id_of[f.dir as usize]), name: f.name.to_string(), path: join(&parent_path, &f.name), kind: "file".into(), size: f.size, files: 1, newest: f.mtime, denied: 0 });
        next_node += 1;
    }
    nodes.push(NodeRow { id: next_node, parent: Some(1), name: "系统和其他".into(), path: "/(系统和其他)".into(), kind: "system".into(), size: system, files: 0, newest: 0, denied: 0 });

    let child_named = |parent: Option<usize>, name: &str| -> Option<usize> { parent.and_then(|p| (0..n).find(|i| dirs[*i].parent as usize == p && &*dirs[*i].name == name)) };
    let trash_idx = child_named(home_idx, ".Trash");
    let trash_count = trash_idx.map_or(0, |t| dirs.iter().filter(|d| d.parent as usize == t).count() as u64 + dirs[t].files as u64);
    let docker_size = child_named(child_named(child_named(home_idx, "Library"), "Containers"), "com.docker.docker").map(|i| total[i]);
    let docker_parts = sys.docker();
    let summary = json!({
        "scannedAt": cfg.now,
        "took": started.elapsed().as_millis() as u64,
        "disk": disk.map(|(t, f)| json!({ "total": t, "free": f, "scanned": scanned, "system": system })),
        "trash": { "size": trash_idx.map_or(0, |t| total[t]), "count": trash_count },
        "docker": if docker_size.is_some() || docker_parts.is_some() { json!({ "size": docker_size.unwrap_or(0), "parts": docker_parts }) } else { Value::Null },
        "denied": nodes[0].denied,
        "fullDiskAccess": sys.full_disk_access(),
        "files": nodes[0].files,
    });
    Ok(Outcome { summary, nodes, items, envs })
}

fn walk_watched(home: &str, parent: &str) -> bool {
    ["Library/Preferences", "Library/Saved Application State", "Library/Application Support", "Library/Containers", "Library/Group Containers", "Library/HTTPStorages", "Library/WebKit", "Library/Caches", "Library/Logs"].iter().any(|w| parent == format!("{home}/{w}"))
}

/// Unix seconds → "2024-03-01" (UTC is close enough for 很久没打开).
pub fn day_of(t: i64) -> String {
    crate::modules::common::from_day_number(t.div_euclid(86400))
}
