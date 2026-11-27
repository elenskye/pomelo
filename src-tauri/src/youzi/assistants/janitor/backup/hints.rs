//! 柚子's clues (1.1.0 Phase 2; requirements「DevJanitor › 备份」): what names and metadata say beyond one
//! task's comparison, worked out with every comparison (`compare_all`) and kept beside it. The Agent reads
//! them (the system message, `agent::BACKUP`) and judges; nothing here acts.
//! - Twins: files with the same name (NFC), size and mtime (±1 s); empty files never count. No checksums.
//! - Mac folders: the subfolders of ~/Downloads and of each task folder's parent, and the task folders. Each
//!   votes for the drive folders holding its files' twins at the same relative path; one holding half its
//!   files (or bytes) is where the drive has it. A task (picked in the 清单) whose drive folder isn't there yet
//!   while another holds its files → rename that one to the task's name; a folder not picked is only said.
//! - Renames: a task's moves taking every file of one drive folder to one new folder → rename the folder.
//! - What only the drive has is archived (1.1.3): kept, nothing said; duplicates on the drive aren't the backup's
//!   business (DevJanitor finds the Mac's own).
//! - The drive's top folders not picked in the 清单, and ARCHIVE, aren't looked at; the Agent is only told they're out.
//! Suggestions are said as `可以：…` lines (`Suggest::line`), which the mock model reads back (`Suggest::parse`).

use super::super::{rules, Janitor};
use super::diff::{self, Diff, Listing, Skip};
use super::{copy, Settings};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::AtomicBool;

/// What a line says before its suggestion.
pub const CAN: &str = "可以：";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suggest {
    /// Rename or move on the drive (relative to its root).
    Rename { from: String, to: String },
}

pub fn tilde(home: &str, p: &str) -> String {
    match p.strip_prefix(home) {
        Some(r) if r.is_empty() || r.starts_with('/') => format!("~{r}"),
        _ => p.to_string(),
    }
}

impl Suggest {
    pub fn line(&self) -> String {
        let Suggest::Rename { from, to } = self;
        format!("在硬盘上改名 {from} → {to}")
    }

    /// A `line` back (the Mac folder keeps its ~).
    pub fn parse(line: &str) -> Option<Suggest> {
        let (from, to) = line.strip_prefix("在硬盘上改名 ")?.split_once(" → ")?;
        Some(Suggest::Rename { from: from.trim().to_string(), to: to.trim().to_string() })
    }

    /// Every suggestion said in `text` (its `可以：` lines).
    pub fn all_in(text: &str) -> Vec<Suggest> {
        text.lines().filter_map(|l| l.split_once(CAN)).filter_map(|(_, s)| Suggest::parse(s.trim())).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub drive: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderHint {
    /// The Mac folder (virtual).
    pub mac: String,
    pub files: u64,
    pub bytes: u64,
    /// The task backing it up.
    pub task: Option<i64>,
    /// Drive folders holding its files at the same relative path, most first.
    pub found: Vec<Found>,
    /// Where the drive has it (half its files or bytes).
    pub best: Option<String>,
    pub note: Option<String>,
    pub suggest: Option<Suggest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameHint {
    pub task: i64,
    pub from: String,
    pub to: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hints {
    pub folders: Vec<FolderHint>,
    pub renames: Vec<RenameHint>,
    /// The drive's top folders not picked in the 清单.
    pub skipped: Vec<String>,
}

fn name_of(k: &str) -> &str {
    k.rsplit('/').next().unwrap_or(k)
}

fn under<'a, V>(m: &'a BTreeMap<String, V>, dir: &str) -> impl Iterator<Item = (&'a String, &'a V)> {
    let p = format!("{dir}/");
    m.range(p.clone()..).take_while(move |(k, _)| k.starts_with(&p))
}

type Index<'a> = HashMap<(&'a str, u64), Vec<(&'a str, i64)>>;

/// Files by (name, size) → (key, mtime).
fn index(files: &BTreeMap<String, (String, diff::Meta)>) -> Index<'_> {
    let mut ix: Index = HashMap::new();
    for (k, (_, m)) in files {
        if m.size > 0 {
            ix.entry((name_of(k), m.size)).or_default().push((k.as_str(), m.mtime));
        }
    }
    ix
}

fn twins<'a>(ix: &'a Index<'a>, key: &'a str, size: u64, mtime: i64) -> impl Iterator<Item = &'a str> + 'a {
    ix.get(&(name_of(key), size)).into_iter().flatten().filter(move |(k, t)| *k != key && (t - mtime).abs() <= 1).map(|(k, _)| *k)
}

/// The Mac folders to look at: subfolders of ~/Downloads and of the task folders' parents, the task folders;
/// not inside a task folder, not holding one, none a task couldn't have.
pub fn candidates(j: &Janitor, s: &Settings) -> Vec<String> {
    let mut roots: Vec<String> = vec![format!("{}/Downloads", j.home)];
    for t in &s.tasks {
        if let Some((p, _)) = t.source.rsplit_once('/') {
            if p != j.home && !roots.iter().any(|r| r == p) {
                roots.push(p.to_string());
            }
        }
    }
    let mut out: Vec<String> = s.tasks.iter().map(|t| t.source.clone()).collect();
    for r in &roots {
        for e in std::fs::read_dir(j.real_of(r)).into_iter().flatten().flatten() {
            let Some(name) = e.file_name().to_str().map(str::to_string) else { continue };
            if name.starts_with('.') || rules::sealed_name(&name) || !std::fs::symlink_metadata(e.path()).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            out.push(format!("{r}/{name}"));
        }
    }
    out.sort();
    out.dedup();
    out.retain(|m| {
        let task = s.tasks.iter().any(|t| t.source == *m);
        task || (!s.tasks.iter().any(|t| rules::within(m, &t.source) || rules::within(&t.source, m)) && super::source_problem(j, m).is_none())
    });
    out
}

/// Lists the candidates and the drive (ARCHIVE and the folders not picked left out), then works the clues out; with
/// the files and bytes in each of the drive's top folders.
pub fn work_out(j: &Janitor, drive: &std::path::Path, s: &Settings, diffs: &HashMap<i64, Result<Diff, String>>, stop: &AtomicBool) -> Result<(Hints, HashMap<String, (u64, u64)>), String> {
    let mut on_drive = diff::list(drive, &Skip::default(), stop)?;
    drop_old(&mut on_drive);
    let mut tops: HashMap<String, (u64, u64)> = HashMap::new();
    for (k, (_, x)) in &on_drive.files {
        if let Some((top, _)) = k.split_once('/') {
            let e = tops.entry(top.to_string()).or_default();
            (e.0, e.1) = (e.0 + 1, e.1 + x.size);
        }
    }
    let names: Vec<String> = on_drive.dirs.keys().filter(|k| !k.contains('/')).cloned().collect();
    let out = super::kept_out(s, &names);
    drop_skipped(&mut on_drive, &out);
    let mut mac = Vec::new();
    for m in candidates(j, s) {
        if let Ok(l) = diff::list(&j.real_of(&m), &Skip::default(), stop) {
            mac.push((m, l));
        }
    }
    let mut h = from_listings(s, &mac, &on_drive, diffs);
    h.skipped = out;
    Ok((h, tops))
}

/// The drive's ARCHIVE isn't backup content.
pub fn drop_old(l: &mut Listing) {
    let old = |k: &String| k == copy::OLD || k.starts_with(&format!("{}/", copy::OLD));
    l.files.retain(|k, _| !old(k));
    l.dirs.retain(|k, _| !old(k));
    l.links.retain(|k, _| !old(k));
}

/// The folders not picked in the 清单 aren't backup content either.
pub fn drop_skipped(l: &mut Listing, skipped: &[String]) {
    let out = |k: &String| skipped.iter().any(|s| rules::within(&diff::nfc(k), &diff::nfc(s)));
    l.files.retain(|k, _| !out(k));
    l.dirs.retain(|k, _| !out(k));
    l.links.retain(|k, _| !out(k));
}

/// The clues from the listings (`mac`: virtual folder → its files; `drive`: without the folders not picked).
pub fn from_listings(s: &Settings, mac: &[(String, Listing)], drive: &Listing, diffs: &HashMap<i64, Result<Diff, String>>) -> Hints {
    let ix = index(&drive.files);
    let targets: Vec<(i64, String)> = s.tasks.iter().map(|t| (t.id, diff::nfc(&t.target))).collect();
    let overlaps = |d: &str, except: Option<i64>| targets.iter().filter(|(i, _)| Some(*i) != except).any(|(_, t)| rules::within(t, d) || rules::within(d, t));
    let mut h = Hints::default();
    // Mac folders and where the drive has them
    for (m, l) in mac {
        let files = l.files.len() as u64;
        let bytes: u64 = l.files.values().map(|(_, x)| x.size).sum();
        if files == 0 {
            continue;
        }
        let task = s.tasks.iter().find(|t| t.source == *m);
        let mut votes: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for (k, (_, x)) in &l.files {
            let mut cps: Vec<&str> = Vec::new();
            for t in twins(&ix, k, x.size, x.mtime) {
                if let Some(cp) = t.strip_suffix(k.as_str()).and_then(|p| p.strip_suffix('/')).filter(|p| !p.is_empty()) {
                    if !cps.contains(&cp) {
                        cps.push(cp);
                    }
                }
            }
            for cp in cps {
                let e = votes.entry(cp.to_string()).or_default();
                e.0 += 1;
                e.1 += x.size;
            }
        }
        let mut found: Vec<Found> = votes.iter().map(|(d, (f, b))| Found { drive: d.clone(), files: *f, bytes: *b }).collect();
        found.sort_by(|a, b| b.files.cmp(&a.files).then(b.bytes.cmp(&a.bytes)).then(a.drive.cmp(&b.drive)));
        found.truncate(3);
        let best = found.first().filter(|f| f.files * 2 >= files || f.bytes * 2 >= bytes).map(|f| f.drive.clone());
        let (mut note, mut suggest) = (None, None);
        match (task, &best) {
            (Some(t), Some(b)) if *b != diff::nfc(&t.target) && !votes.contains_key(&diff::nfc(&t.target)) => {
                let tt = diff::nfc(&t.target);
                if drive.dirs.contains_key(&tt) || drive.files.contains_key(&tt) || drive.links.contains_key(&tt) {
                    note = Some(format!("硬盘上的 {} 里一个都没有，它们在 {b}", t.target));
                } else if overlaps(b, Some(t.id)) {
                    note = Some(format!("硬盘上还没有 {}，它们在别的文件夹的备份里（{b}）", t.target));
                } else {
                    note = Some(format!("硬盘上还没有 {}，它们在 {b}", t.target));
                    suggest = Some(Suggest::Rename { from: b.clone(), to: tt });
                }
            }
            (Some(_), _) => {}
            (None, Some(b)) if *b != diff::nfc(name_of(m)) => note = Some(format!("硬盘上在 {b}")),
            (None, _) => {}
        }
        h.folders.push(FolderHint { mac: m.clone(), files, bytes, task: task.map(|t| t.id), found, best, note, suggest });
    }
    // a whole folder renamed inside a task
    for t in &s.tasks {
        let Some(Ok(d)) = diffs.get(&t.id) else { continue };
        let mut groups: BTreeMap<(String, String), (u64, u64)> = BTreeMap::new();
        for mv in &d.moved {
            let (f, to) = (diff::nfc(&mv.from), diff::nfc(&mv.to));
            let (fc, tc): (Vec<&str>, Vec<&str>) = (f.split('/').collect(), to.split('/').collect());
            let common = fc.iter().rev().zip(tc.iter().rev()).take_while(|(a, b)| a == b).count();
            if common == 0 || common >= fc.len() || common >= tc.len() {
                continue;
            }
            let e = groups.entry((fc[..fc.len() - common].join("/"), tc[..tc.len() - common].join("/"))).or_default();
            e.0 += 1;
            e.1 += mv.size;
        }
        let root = diff::nfc(&t.target);
        for ((fd, td), (n, b)) in groups {
            let (from, to) = (format!("{root}/{fd}"), format!("{root}/{td}"));
            let whole = under(&drive.files, &from).count() as u64 == n;
            let taken = drive.dirs.contains_key(&to) || drive.files.contains_key(&to) || drive.links.contains_key(&to);
            if whole && !taken && !rules::within(&to, &from) {
                h.renames.push(RenameHint { task: t.id, from, to, files: n, bytes: b });
            }
        }
    }
    h
}

impl Hints {
    /// What 柚子 could suggest, in order.
    pub fn suggestions(&self) -> Vec<Suggest> {
        let mut out: Vec<Suggest> = self.folders.iter().filter_map(|f| f.suggest.clone()).collect();
        out.extend(self.renames.iter().map(|r| Suggest::Rename { from: r.from.clone(), to: r.to.clone() }));
        out
    }

    /// The clues as the Agent reads them (empty: nothing to say).
    pub fn text(&self, home: &str, gb: impl Fn(u64) -> String) -> String {
        let mut out = String::new();
        let can = |s: &Option<Suggest>| s.as_ref().map(|s| format!("；{CAN}{}", s.line())).unwrap_or_default();
        if !self.folders.is_empty() {
            out.push_str("- Downloads 里的文件夹（清单里勾了的备份到硬盘上同名的文件夹；硬盘上哪里有一样的文件：同样的名字、大小、修改时间，在同样的相对位置）：\n");
            for f in &self.folders {
                let ticked = if f.task.is_some() { "勾了" } else { "清单里没勾（不备份）" };
                let found = if f.found.is_empty() { "硬盘上哪儿都没有一样的".to_string() } else { format!("硬盘上 {}", f.found.iter().map(|x| format!("{} 里对得上 {} 个（{}）", x.drive, x.files, gb(x.bytes))).collect::<Vec<_>>().join("，")) };
                let note = f.note.as_ref().map(|n| format!("；{n}")).unwrap_or_default();
                out.push_str(&format!("  - {}（{} 个文件，{}）：{ticked}；{found}{note}{}\n", tilde(home, &f.mac), f.files, gb(f.bytes), can(&f.suggest)));
            }
        }
        if !self.renames.is_empty() {
            out.push_str("- 勾了的文件夹里整个子文件夹改了名（硬盘上那个文件夹的文件全都挪到了电脑上的新名字下）：\n");
            for r in &self.renames {
                out.push_str(&format!("  - 硬盘上的 {} 在电脑上叫 {}（{} 个，{}）{}\n", r.from, r.to, r.files, gb(r.bytes), can(&Some(Suggest::Rename { from: r.from.clone(), to: r.to.clone() }))));
            }
        }
        if !self.skipped.is_empty() {
            out.push_str(&format!("- 硬盘上清单里没勾的文件夹（不备份：不对比、不进 PikPak，什么卡片都不提，留着）：{}\n", self.skipped.join("、")));
        }
        out
    }

    /// What a look is about: the suggestions and what's held (a new one means look again).
    pub fn fingerprint(&self, held: &[(i64, usize)]) -> String {
        let mut parts: Vec<String> = self.suggestions().iter().map(|s| s.line()).collect();
        parts.extend(held.iter().filter(|(_, n)| *n > 0).map(|(t, n)| format!("held #{t} {n}")));
        if parts.is_empty() {
            return String::new();
        }
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        parts.hash(&mut h);
        format!("{:016x}", h.finish())
    }
}
