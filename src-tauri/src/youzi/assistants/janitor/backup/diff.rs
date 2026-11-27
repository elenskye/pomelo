//! A backup task's two folders side by side (requirements「DevJanitor › 备份」): names and metadata only
//! (lstat; never a file's bytes). Keys are relative paths in NFC (Mac OS 扩展 stores names decomposed, APFS as
//! typed); times compare to the second (HFS+ keeps seconds). VAULT and the junk below are never listed.
//! - same: size equal, mtime within 1 s. changed: the Mac's is newer → copied, the drive's goes to ARCHIVE.
//!   held: the drive's is newer, or the name is a folder there → never overwritten.
//! - moved: a Mac-only file with the size and mtime of a drive-only one (same name first) → renamed on the
//!   drive when I say so; never copied again.
//! - only on the drive: kept (只加不删); `recent` when it was on the Mac at the task's last backup.

use super::super::rules;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Our half-copied files on the drive (copy.rs).
pub const PART: &str = ".pomelo-part-";

/// Names never backed up, on either side.
pub const JUNK: [&str; 9] = [".DS_Store", ".localized", ".Trashes", ".Spotlight-V100", ".fseventsd", ".TemporaryItems", ".DocumentRevisions-V100", ".apdisk", ".com.apple.timemachine.donotpresent"];
/// Files still downloading.
const PARTIAL: [&str; 4] = [".crdownload", ".download", ".part", ".partial"];
/// Folders that rebuild (a project's).
const REBUILT: [&str; 11] = ["node_modules", "__pycache__", ".ipynb_checkpoints", ".pytest_cache", ".mypy_cache", ".ruff_cache", ".gradle", ".next", ".turbo", ".parcel-cache", ".astro"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Meta {
    pub size: u64,
    pub mtime: i64,
}

/// What a walk saw. Keys: NFC relative paths.
#[derive(Debug, Default)]
pub struct Listing {
    /// key → (relative path as on disk, metadata)
    pub files: BTreeMap<String, (String, Meta)>,
    pub dirs: BTreeMap<String, String>,
    /// key → (relative path, where it points)
    pub links: BTreeMap<String, (String, String)>,
    /// Folders it couldn't read.
    pub denied: Vec<String>,
}

/// NFC (ASCII as is).
pub fn nfc(s: &str) -> String {
    if s.is_ascii() {
        return s.to_string();
    }
    objc2_foundation::NSString::from_str(s).precomposedStringWithCanonicalMapping().to_string()
}

/// What a walk leaves out: VAULT and the junk; a Mac folder (not `mirror`) also what rebuilds and files still
/// downloading. `mirror`: the drive as PikPak gets it (rclone.rs `filters`: VAULT and the junk only).
#[derive(Debug, Clone, Default)]
pub struct Skip {
    pub mirror: bool,
}

impl Skip {
    /// `dir`: the real folder the entry is in.
    pub fn skips(&self, dir: &Path, name: &str, is_dir: bool) -> bool {
        if rules::sealed_name(name) || JUNK.contains(&name) || name.starts_with(PART) || name.starts_with("._") {
            return true;
        }
        if self.mirror {
            return false;
        }
        if name.starts_with("~$") {
            return true;
        }
        let low = name.to_lowercase();
        if PARTIAL.iter().any(|s| low.ends_with(s)) {
            return true;
        }
        is_dir && (REBUILT.contains(&name) || (name == "target" && dir.join("Cargo.toml").exists()) || ((name == ".venv" || name == "venv") && dir.join(name).join("pyvenv.cfg").exists()))
    }
}

/// Walks `root` (missing: nothing). `stop` ends it early with an error.
pub fn list(root: &Path, skip: &Skip, stop: &AtomicBool) -> Result<Listing, String> {
    let mut out = Listing::default();
    if !root.is_dir() {
        return Ok(out);
    }
    let mut stack = vec![(root.to_path_buf(), String::new())];
    let mut seen = 0usize;
    while let Some((dir, rel)) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => {
                out.denied.push(if rel.is_empty() { ".".into() } else { rel.clone() });
                continue;
            }
        };
        for e in rd.flatten() {
            seen += 1;
            if seen.is_multiple_of(2000) && stop.load(Ordering::Relaxed) {
                return Err("停下了".into());
            }
            let Some(name) = e.file_name().to_str().map(str::to_string) else {
                out.denied.push(format!("{rel}/（名字读不出来）"));
                continue;
            };
            let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
            if skip.skips(&dir, &name, m.is_dir()) {
                continue;
            }
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if m.file_type().is_symlink() {
                let to = std::fs::read_link(e.path()).map(|t| t.to_string_lossy().into_owned()).unwrap_or_default();
                out.links.insert(nfc(&r), (r, to));
            } else if m.is_dir() {
                out.dirs.insert(nfc(&r), r.clone());
                stack.push((e.path(), r));
            } else if m.is_file() {
                out.files.insert(nfc(&r), (r, Meta { size: m.len(), mtime: m.mtime() }));
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// Relative to the task's folder (the Mac's name for Mac files, the drive's for drive-only ones).
    pub path: String,
    pub size: u64,
    pub mtime: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    /// Where it is on the drive now → where the Mac has it.
    pub from: String,
    pub to: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Held {
    pub path: String,
    pub size: u64,
    pub why: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub new: Vec<Row>,
    pub changed: Vec<Row>,
    pub moved: Vec<Move>,
    pub held: Vec<Held>,
    /// Only on the drive and on the Mac at the last backup (deleted or moved off since).
    pub recent: Vec<Row>,
    /// Only on the drive, long since.
    pub archived: Vec<Row>,
    /// Mac folders and symlinks missing on the drive.
    pub dirs: Vec<String>,
    pub links: Vec<(String, String)>,
    pub same: u64,
    pub same_bytes: u64,
    pub denied: Vec<String>,
}

impl Diff {
    /// Bytes a run copies.
    pub fn to_copy(&self) -> u64 {
        self.new.iter().chain(&self.changed).map(|r| r.size).sum()
    }

    pub fn files_to_copy(&self) -> u64 {
        (self.new.len() + self.changed.len()) as u64
    }
}

fn file_name(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// The two sides compared; `seen`: the Mac folder at the task's last backup (NFC key → size, mtime).
pub fn compare(mac: &Listing, drive: &Listing, seen: &HashMap<String, (u64, i64)>) -> Diff {
    let mut d = Diff { denied: mac.denied.iter().map(|p| format!("电脑上 {p}")).chain(drive.denied.iter().map(|p| format!("硬盘上 {p}"))).collect(), ..Default::default() };
    let mut mac_only = Vec::new();
    for (k, (rel, m)) in &mac.files {
        match drive.files.get(k) {
            Some((_, o)) if o.size == m.size && (o.mtime - m.mtime).abs() <= 1 => {
                d.same += 1;
                d.same_bytes += m.size;
            }
            Some((_, o)) if o.mtime > m.mtime + 1 => d.held.push(Held { path: rel.clone(), size: m.size, why: "硬盘上的更新".into() }),
            Some(_) => d.changed.push(Row { path: rel.clone(), size: m.size, mtime: m.mtime }),
            None if drive.dirs.contains_key(k) || drive.links.contains_key(k) => d.held.push(Held { path: rel.clone(), size: m.size, why: "硬盘上同名的不是文件".into() }),
            None => mac_only.push((k, rel, *m)),
        }
    }
    // drive-only files by (size, mtime), to recognize a rename or a move
    let mut by_meta: HashMap<(u64, i64), Vec<&String>> = HashMap::new();
    for (k, (_, o)) in &drive.files {
        if !mac.files.contains_key(k) && o.size > 0 {
            by_meta.entry((o.size, o.mtime)).or_default().push(k);
        }
    }
    let mut used: HashSet<&String> = HashSet::new();
    for (_, rel, m) in mac_only {
        let mut cands: Vec<&String> = [-1i64, 0, 1].iter().filter_map(|dt| by_meta.get(&(m.size, m.mtime + dt))).flatten().copied().filter(|c| !used.contains(*c)).collect();
        let name = nfc(file_name(rel));
        cands.sort_by_key(|c| (file_name(c) != name, (*c).clone()));
        match cands.first() {
            Some(c) => {
                used.insert(*c);
                d.moved.push(Move { from: drive.files[*c].0.clone(), to: rel.clone(), size: m.size });
            }
            None => d.new.push(Row { path: rel.clone(), size: m.size, mtime: m.mtime }),
        }
    }
    for (k, (rel, o)) in &drive.files {
        if mac.files.contains_key(k) || used.contains(k) {
            continue;
        }
        let row = Row { path: rel.clone(), size: o.size, mtime: o.mtime };
        if seen.contains_key(k) {
            d.recent.push(row);
        } else {
            d.archived.push(row);
        }
    }
    // only folders no copied file makes (empty ones): a folder of moves waits for them
    let holds_files = |k: &str| mac.files.range(format!("{k}/")..).next().is_some_and(|(f, _)| f.starts_with(&format!("{k}/")));
    d.dirs = mac.dirs.iter().filter(|(k, _)| !drive.dirs.contains_key(*k) && !holds_files(k)).map(|(_, r)| r.clone()).collect();
    d.links = mac.links.iter().filter(|(k, _)| !drive.links.contains_key(*k) && !drive.files.contains_key(*k) && !drive.dirs.contains_key(*k)).map(|(_, v)| v.clone()).collect();
    d
}

/// The Mac side as `backup_seen` keeps it.
pub fn seen_rows(mac: &Listing) -> Vec<(String, u64, i64)> {
    mac.files.iter().map(|(k, (_, m))| (k.clone(), m.size, m.mtime)).collect()
}
