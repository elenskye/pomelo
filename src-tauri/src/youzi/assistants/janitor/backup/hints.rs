//! 柚子's clues (1.1.0 Phase 2; requirements「DevJanitor › 备份」): what names and metadata say beyond one
//! task's comparison, worked out with every comparison (`compare_all`) and kept beside it. The Agent reads
//! them (the system message, `agent::BACKUP`) and judges; nothing here acts.
//! - Twins: files with the same name (NFC), size and mtime (±1 s); empty files never count. No checksums.
//! - Mac folders: the subfolders of ~/Downloads and of each task folder's parent, and the task folders. Each
//!   votes for the drive folders holding its files' twins at the same relative path; one holding half its
//!   files (or bytes) is where the drive has it. Not in a task → 新任务 there (or under its own name when the
//!   drive has none of it); its task's drive folder has none of it but another has → 改任务.
//! - Renames: a task's moves taking every file of one drive folder to one new folder → rename the folder.
//! - Dups: the biggest drive folders (≥ 1 MB) whose every file has a twin outside them (旧版 left out); the
//!   copy no task uses (and no suggested task) is the one to trash.
//! - Elsewhere: a task's drive-only files the Mac has in another folder (an old name, not an archive).
//! - Archives: top drive folders nothing on the Mac and no task matches.
//! - The folders I keep out of the backup (`Settings::skipped`) aren't looked at; the Agent is only told they're out.
//! Suggestions are said as `可以：…` lines (`Suggest::line`), which the mock model reads back (`Suggest::parse`).

use super::super::{rules, Janitor};
use super::diff::{self, Diff, Listing, Skip};
use super::{copy, Settings};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::AtomicBool;

const MIN_DUP: u64 = rules::MB;
/// What a line says before its suggestion.
pub const CAN: &str = "可以：";
const SHOWN: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suggest {
    /// A task: new (`id` None) or changed. `source` virtual (or with ~), `target` relative to the drive.
    Task { id: Option<i64>, source: String, target: String },
    /// Rename or move on the drive (relative to its root).
    Rename { from: String, to: String },
    /// To the drive's Trash (relative to its root).
    Trash { path: String },
}

pub fn tilde(home: &str, p: &str) -> String {
    match p.strip_prefix(home) {
        Some(r) if r.is_empty() || r.starts_with('/') => format!("~{r}"),
        _ => p.to_string(),
    }
}

impl Suggest {
    pub fn line(&self, home: &str) -> String {
        match self {
            Suggest::Task { id: None, source, target } => format!("新任务 {} → {target}", tilde(home, source)),
            Suggest::Task { id: Some(i), source, target } => format!("改任务 #{i} {} → {target}", tilde(home, source)),
            Suggest::Rename { from, to } => format!("在硬盘上改名 {from} → {to}"),
            Suggest::Trash { path } => format!("移到硬盘的废纸篓 {path}"),
        }
    }

    /// A `line` back (the Mac folder keeps its ~).
    pub fn parse(line: &str) -> Option<Suggest> {
        let pair = |s: &str| s.split_once(" → ").map(|(a, b)| (a.trim().to_string(), b.trim().to_string()));
        if let Some(r) = line.strip_prefix("新任务 ") {
            let (source, target) = pair(r)?;
            return Some(Suggest::Task { id: None, source, target });
        }
        if let Some(r) = line.strip_prefix("改任务 #") {
            let (id, r) = r.split_once(' ')?;
            let (source, target) = pair(r)?;
            return Some(Suggest::Task { id: Some(id.parse().ok()?), source, target });
        }
        if let Some(r) = line.strip_prefix("在硬盘上改名 ") {
            let (from, to) = pair(r)?;
            return Some(Suggest::Rename { from, to });
        }
        line.strip_prefix("移到硬盘的废纸篓 ").map(|p| Suggest::Trash { path: p.trim().to_string() })
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DupHint {
    pub path: String,
    pub files: u64,
    pub bytes: u64,
    /// Where its twins mostly are.
    pub twin: Option<String>,
    /// A task's drive folder is it, holds it or is in it.
    pub used: Option<i64>,
    pub suggest: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElsewhereHint {
    pub task: i64,
    /// The Mac folder the twins are in (virtual).
    pub mac: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spot {
    pub path: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hints {
    pub folders: Vec<FolderHint>,
    pub renames: Vec<RenameHint>,
    pub dups: Vec<DupHint>,
    pub elsewhere: Vec<ElsewhereHint>,
    pub archives: Vec<Spot>,
    pub skipped: Vec<String>,
}

fn name_of(k: &str) -> &str {
    k.rsplit('/').next().unwrap_or(k)
}

fn dir_of(k: &str) -> &str {
    k.rsplit_once('/').map_or("", |(d, _)| d)
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

/// Lists the candidates and the drive (旧版 left out), then works the clues out.
pub fn work_out(j: &Janitor, drive: &std::path::Path, s: &Settings, diffs: &HashMap<i64, Result<Diff, String>>, stop: &AtomicBool) -> Result<Hints, String> {
    let mut on_drive = diff::list(drive, &Skip::default(), stop)?;
    drop_old(&mut on_drive);
    drop_skipped(&mut on_drive, &s.skipped);
    let mut mac = Vec::new();
    for m in candidates(j, s) {
        let skip = Skip { extra: s.tasks.iter().find(|t| t.source == m).map(|t| t.excludes.clone()).unwrap_or_default() };
        if let Ok(l) = diff::list(&j.real_of(&m), &skip, stop) {
            mac.push((m, l));
        }
    }
    let gone: Vec<i64> = s.tasks.iter().filter(|t| !j.real_of(&t.source).is_dir()).map(|t| t.id).collect();
    Ok(from_listings(s, &mac, &on_drive, diffs, &gone))
}

/// The drive's 旧版 isn't backup content.
pub fn drop_old(l: &mut Listing) {
    let old = |k: &String| k == copy::OLD || k.starts_with(&format!("{}/", copy::OLD));
    l.files.retain(|k, _| !old(k));
    l.dirs.retain(|k, _| !old(k));
    l.links.retain(|k, _| !old(k));
}

/// The folders kept out of the backup aren't backup content either.
pub fn drop_skipped(l: &mut Listing, skipped: &[String]) {
    let out = |k: &String| skipped.iter().any(|s| rules::within(&diff::nfc(k), &diff::nfc(s)));
    l.files.retain(|k, _| !out(k));
    l.dirs.retain(|k, _| !out(k));
    l.links.retain(|k, _| !out(k));
}

/// The clues from the listings (`mac`: virtual folder → its files; `gone`: tasks whose Mac folder is missing).
pub fn from_listings(s: &Settings, mac: &[(String, Listing)], drive: &Listing, diffs: &HashMap<i64, Result<Diff, String>>, gone: &[i64]) -> Hints {
    let ix = index(&drive.files);
    let targets: Vec<(i64, String)> = s.tasks.iter().map(|t| (t.id, diff::nfc(&t.target))).collect();
    let overlaps = |d: &str, except: Option<i64>| targets.iter().filter(|(i, _)| Some(*i) != except).any(|(_, t)| rules::within(t, d) || rules::within(d, t));
    let used_by = |d: &str| targets.iter().find(|(_, t)| rules::within(t, d) || rules::within(d, t)).map(|(i, _)| *i);
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
                if overlaps(b, Some(t.id)) {
                    note = Some(format!("任务的硬盘文件夹 {} 里一个都没有，它们在别的任务的文件夹里", t.target));
                } else {
                    note = Some(format!("任务的硬盘文件夹 {} 里一个都没有", t.target));
                    suggest = Some(Suggest::Task { id: Some(t.id), source: m.clone(), target: b.clone() });
                }
            }
            (Some(_), _) => {}
            (None, Some(b)) => match s.tasks.iter().find(|t| diff::nfc(&t.target) == *b) {
                Some(t) if gone.contains(&t.id) => {
                    note = Some(format!("任务 #{} 电脑上的文件夹不在了，像是改了名", t.id));
                    suggest = Some(Suggest::Task { id: Some(t.id), source: m.clone(), target: b.clone() });
                }
                Some(t) => note = Some(format!("硬盘上这个文件夹是任务 #{} 的", t.id)),
                None if overlaps(b, None) => note = Some("在别的任务的硬盘文件夹里（或包着它）".into()),
                None => suggest = Some(Suggest::Task { id: None, source: m.clone(), target: b.clone() }),
            },
            (None, None) => {
                let name = diff::nfc(name_of(m));
                if drive.dirs.contains_key(&name) {
                    note = Some(format!("硬盘上同名的 {name} 里对不上"));
                } else if overlaps(&name, None) {
                    note = Some("同名的文件夹是别的任务的".into());
                } else {
                    suggest = Some(Suggest::Task { id: None, source: m.clone(), target: name });
                }
            }
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
    // duplicates on the drive: folders whose every file has a twin outside them
    let mut agg: HashMap<&str, (usize, u64, u64)> = HashMap::new(); // folder → (max cut, files, bytes)
    for (k, (_, x)) in &drive.files {
        if x.size == 0 {
            continue;
        }
        let dc: Vec<&str> = dir_of(k).split('/').filter(|c| !c.is_empty()).collect();
        let cut = twins(&ix, k, x.size, x.mtime)
            .map(|t| {
                let tc: Vec<&str> = dir_of(t).split('/').filter(|c| !c.is_empty()).collect();
                dc.iter().zip(&tc).take_while(|(a, b)| a == b).count()
            })
            .min()
            .unwrap_or(usize::MAX);
        let mut end = 0;
        for c in &dc {
            end += c.len() + if end == 0 { 0 } else { 1 };
            let e = agg.entry(&k[..end]).or_insert((0, 0, 0));
            e.0 = e.0.max(cut);
            e.1 += 1;
            e.2 += x.size;
        }
    }
    let depth = |d: &str| d.split('/').count();
    let qualifies = |d: &str| agg.get(d).is_some_and(|(cut, _, _)| *cut < depth(d));
    let suggested: Vec<String> = h.folders.iter().filter_map(|f| match &f.suggest { Some(Suggest::Task { target, .. }) => Some(target.clone()), _ => None }).collect();
    let mut dups: Vec<DupHint> = Vec::new();
    for (d, (_, files, bytes)) in &agg {
        if !qualifies(d) || *bytes < MIN_DUP || d.rsplit_once('/').is_some_and(|(p, _)| qualifies(p)) {
            continue;
        }
        let mut votes: BTreeMap<&str, u64> = BTreeMap::new();
        for (k, (_, x)) in under(&drive.files, d) {
            let rel = &k[d.len() + 1..];
            for t in twins(&ix, k, x.size, x.mtime).filter(|t| !rules::within(t, d)) {
                if let Some(cp) = t.strip_suffix(rel).and_then(|p| p.strip_suffix('/')).filter(|p| !p.is_empty()) {
                    *votes.entry(cp).or_default() += 1;
                }
            }
        }
        let twin = votes.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map(|(cp, _)| cp.to_string());
        let used = used_by(d);
        let near_suggested = suggested.iter().any(|t| rules::within(t, d) || rules::within(d, t));
        let suggest = used.is_none()
            && !near_suggested
            && twin.as_deref().is_some_and(|tw| {
                let twin_kept = used_by(tw).is_some() || suggested.iter().any(|t| rules::within(t, tw) || rules::within(tw, t));
                twin_kept || !qualifies(tw) || **d > *tw
            });
        dups.push(DupHint { path: d.to_string(), files: *files as u64, bytes: *bytes, twin, used, suggest });
    }
    dups.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.path.cmp(&b.path)));
    dups.truncate(SHOWN);
    h.dups = dups;
    // a task's drive-only files the Mac has elsewhere
    let mac_files: BTreeMap<String, (String, diff::Meta)> = mac.iter().flat_map(|(m, l)| l.files.iter().map(move |(k, v)| (format!("{m}/{k}"), v.clone()))).collect();
    let mac_ix = index(&mac_files);
    for t in &s.tasks {
        let Some(Ok(d)) = diffs.get(&t.id) else { continue };
        let mut by: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for r in d.archived.iter().chain(&d.recent) {
            let k = diff::nfc(&r.path);
            let hit = mac_ix.get(&(name_of(&k), r.size)).into_iter().flatten().find(|(p, mt)| (mt - r.mtime).abs() <= 1 && !rules::within(p, &t.source));
            if let Some((p, _)) = hit {
                let e = by.entry(dir_of(p).to_string()).or_default();
                e.0 += 1;
                e.1 += r.size;
            }
        }
        h.elsewhere.extend(by.into_iter().map(|(mac, (files, bytes))| ElsewhereHint { task: t.id, mac, files, bytes }));
    }
    h.elsewhere.truncate(SHOWN);
    // top drive folders nothing matches
    let mut archives: Vec<Spot> = drive
        .dirs
        .keys()
        .filter(|k| !k.contains('/'))
        .filter(|k| used_by(k).is_none())
        .filter(|k| !h.folders.iter().any(|f| f.found.iter().any(|x| rules::within(&x.drive, k))))
        .filter(|k| !h.dups.iter().any(|x| rules::within(&x.path, k)))
        .map(|k| {
            let (files, bytes) = under(&drive.files, k).fold((0, 0), |(n, b), (_, (_, x))| (n + 1, b + x.size));
            Spot { path: k.clone(), files, bytes }
        })
        .filter(|s| s.files > 0)
        .collect();
    archives.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.path.cmp(&b.path)));
    archives.truncate(SHOWN);
    h.archives = archives;
    h.skipped = s.skipped.clone();
    h
}

impl Hints {
    /// What 柚子 could suggest, in order.
    pub fn suggestions(&self) -> Vec<Suggest> {
        let mut out: Vec<Suggest> = self.folders.iter().filter_map(|f| f.suggest.clone()).collect();
        out.extend(self.renames.iter().map(|r| Suggest::Rename { from: r.from.clone(), to: r.to.clone() }));
        out.extend(self.dups.iter().filter(|d| d.suggest).map(|d| Suggest::Trash { path: d.path.clone() }));
        out
    }

    /// The clues as the Agent reads them (empty: nothing to say).
    pub fn text(&self, home: &str, gb: impl Fn(u64) -> String) -> String {
        let mut out = String::new();
        let can = |s: &Option<Suggest>| s.as_ref().map(|s| format!("；{CAN}{}", s.line(home))).unwrap_or_default();
        if !self.folders.is_empty() {
            out.push_str("- 电脑上的文件夹（硬盘上哪里有一样的文件：同样的名字、大小、修改时间，在同样的相对位置）：\n");
            for f in &self.folders {
                let task = f.task.map_or("没有任务".to_string(), |i| format!("任务 #{i}"));
                let found = if f.found.is_empty() { "硬盘上哪儿都没有一样的".to_string() } else { format!("硬盘上 {}", f.found.iter().map(|x| format!("{} 里对得上 {} 个（{}）", x.drive, x.files, gb(x.bytes))).collect::<Vec<_>>().join("，")) };
                let note = f.note.as_ref().map(|n| format!("；{n}")).unwrap_or_default();
                out.push_str(&format!("  - {}（{} 个文件，{}）：{task}；{found}{note}{}\n", tilde(home, &f.mac), f.files, gb(f.bytes), can(&f.suggest)));
            }
        }
        if !self.renames.is_empty() {
            out.push_str("- 任务里整个文件夹改了名（硬盘上那个文件夹的文件全都挪到了电脑上的新名字下）：\n");
            for r in &self.renames {
                out.push_str(&format!("  - 任务 #{}：硬盘上的 {} 在电脑上叫 {}（{} 个，{}）{}\n", r.task, r.from, r.to, r.files, gb(r.bytes), can(&Some(Suggest::Rename { from: r.from.clone(), to: r.to.clone() }))));
            }
        }
        if !self.dups.is_empty() {
            out.push_str("- 硬盘里的重复（文件夹里每个文件在别处都有一样的）：\n");
            for d in &self.dups {
                let twin = d.twin.as_ref().map(|t| format!("，大多在 {t} 里")).unwrap_or_default();
                let used = d.used.map(|i| format!("；和任务 #{i} 的硬盘文件夹有关，别动")).unwrap_or_default();
                let sug = if d.suggest { Some(Suggest::Trash { path: d.path.clone() }) } else { None };
                out.push_str(&format!("  - {}（{} 个，{}）{twin}{used}{}\n", d.path, d.files, gb(d.bytes), can(&sug)));
            }
        }
        if !self.elsewhere.is_empty() {
            out.push_str("- 只在硬盘上、但电脑上别的文件夹里有一样的（改了名或换了地方留下的旧名字，不是归档）：\n");
            for e in &self.elsewhere {
                out.push_str(&format!("  - 任务 #{} 的 {} 个（{}）在电脑上的 {} 里\n", e.task, e.files, gb(e.bytes), tilde(home, &e.mac)));
            }
        }
        if !self.archives.is_empty() {
            out.push_str(&format!("- 只在硬盘上、电脑上和任务里都没有的文件夹（像是归档，留着）：{}\n", self.archives.iter().map(|a| format!("{}（{} 个，{}）", a.path, a.files, gb(a.bytes))).collect::<Vec<_>>().join("、")));
        }
        if !self.skipped.is_empty() {
            out.push_str(&format!("- 用户定了不备份的硬盘文件夹（不对比、不进 PikPak，什么卡片都不提，留着）：{}\n", self.skipped.join("、")));
        }
        out
    }

    /// What a look is about: the suggestions and what's held (a new one means look again).
    pub fn fingerprint(&self, home: &str, held: &[(i64, usize)]) -> String {
        let mut parts: Vec<String> = self.suggestions().iter().map(|s| s.line(home)).collect();
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
