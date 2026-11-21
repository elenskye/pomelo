//! 见字如面's imports (v1.0 Phase 3). Rules:
//! - From Bear, once: Bear's own `bearcli` (inside Bear.app) is only read (list, tags, attachments save); nothing in
//!   Bear changes. Every note comes over (notes, archive, trash) with its text as written (Bear's marks, `[[links]]`,
//!   fold marks), created / modified times, pinned (any pin), archived, trashed (at its modified time); its pictures
//!   and files become attachments and the links in the text take their stored names (an attachment the text never
//!   names is linked at the end). Bear's tags ignore case: a tag written in another case than Bear's list takes the
//!   list's, level by level, so the tag tree is Bear's. Locked notes can't be read by bearcli: listed, not brought.
//!   Refused when most of Bear's notes are here already (the same title and created second).
//! - The report lists what didn't come over, Bear's tags this text doesn't read as tags, and `[[links]]` that lead
//!   nowhere.
//! - Files: .md / .markdown / .txt, .textbundle (a folder) and .textpack (its zip), picked one by one or as folders:
//!   a folder's sub-folders become tags (the picked folder itself doesn't), written on the second line unless the
//!   text has the tag; pictures and files the text links by a relative path become attachments; created / modified
//!   from the file; a text without a title gets the file's name as its heading. Other files are listed, not brought.
//! - One ⌘Z for the whole import (the attachment copies stay, as attachments.rs says).

use super::{encode_name, insert, links_in, names, title_of};
use crate::modules::attachments::{self, MAX_BYTES, MAX_PER_RECORD};
use crate::modules::common::{err, Tx};
use crate::modules::tags;
use crate::AppState;
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::State;

pub const BEARCLI: &str = "/Applications/Bear.app/Contents/MacOS/bearcli";
const MAX_FILES: usize = 5000;
const MAX_DEPTH: usize = 12;

// ---------- Bear ----------

/// A note as `bearcli list --fields all,content --format json` gives it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BearNote {
    pub id: String,
    pub title: String,
    /// "no" / "yes"
    pub locked: Value,
    /// As Bear lists them: `#a`, `#a/b` (parents included), `#multi word#`.
    pub tags: Vec<String>,
    pub created: String,
    pub modified: String,
    pub pins: Vec<Value>,
    /// notes | archive | trash
    pub location: String,
    /// File names.
    pub attachments: Vec<Value>,
    pub content: Option<String>,
}

impl BearNote {
    fn locked(&self) -> bool {
        self.locked.as_bool() == Some(true) || self.locked.as_str().is_some_and(|s| s.eq_ignore_ascii_case("yes"))
    }
    fn files(&self) -> Vec<String> {
        self.attachments.iter().filter_map(|v| v.as_str().map(String::from).or_else(|| v.get("filename").and_then(Value::as_str).map(String::from))).collect()
    }
    fn shown(&self) -> String {
        if self.title.trim().is_empty() {
            "无标题".into()
        } else {
            self.title.trim().to_string()
        }
    }
}

/// What Bear gives (bearcli; a fake in tests).
pub trait Bear {
    fn notes(&self) -> Result<Vec<BearNote>, String>;
    /// Bear's tag list: one spelling per tag.
    fn tags(&self) -> Result<Vec<String>, String>;
    fn file(&self, note: &str, name: &str) -> Result<Vec<u8>, String>;
}

pub struct BearCli {
    path: PathBuf,
}

impl BearCli {
    pub fn find() -> Result<Self, String> {
        let path = PathBuf::from(BEARCLI);
        if path.is_file() {
            Ok(Self { path })
        } else {
            Err("找不到 Bear（「应用程序」里的 Bear.app）".into())
        }
    }
    fn run(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let out = std::process::Command::new(&self.path).args(args).stdin(std::process::Stdio::null()).output().map_err(|e| format!("没能运行 Bear 的命令行：{e}"))?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(format!("Bear 的命令行出错了：{}", if msg.is_empty() { String::from_utf8_lossy(&out.stdout).trim().to_string() } else { msg }));
        }
        Ok(out.stdout)
    }
    fn json<T: DeserializeOwned>(&self, args: &[&str]) -> Result<T, String> {
        let v: Value = serde_json::from_slice(&self.run(args)?).map_err(|e| format!("读不懂 Bear 的输出：{e}"))?;
        if let Some(e) = v.get("error") {
            return Err(format!("Bear：{}", e.get("message").and_then(Value::as_str).unwrap_or("出错了")));
        }
        serde_json::from_value(v).map_err(|e| format!("读不懂 Bear 的输出：{e}"))
    }
}

impl Bear for BearCli {
    fn notes(&self) -> Result<Vec<BearNote>, String> {
        self.json(&["list", "--location", "all", "--format", "json", "--fields", "all,content"])
    }
    fn tags(&self) -> Result<Vec<String>, String> {
        let rows: Vec<Value> = self.json(&["tags", "list", "--format", "json"])?;
        Ok(rows.iter().filter_map(|r| r.get("tag").and_then(Value::as_str).map(String::from)).collect())
    }
    fn file(&self, note: &str, name: &str) -> Result<Vec<u8>, String> {
        let b = self.run(&["attachments", "save", note, "--filename", name])?;
        if b.is_empty() {
            return Err("Bear 没给出这个文件".into());
        }
        Ok(b)
    }
}

/// Everything read from Bear first (the database isn't held while bearcli runs).
pub struct Loaded {
    notes: Vec<BearNote>,
    tags: Vec<String>,
    files: HashMap<(String, String), Result<Vec<u8>, String>>,
}

impl Loaded {
    pub fn from(bear: &dyn Bear) -> Result<Self, String> {
        let notes = bear.notes()?;
        let tags = bear.tags()?;
        let mut files = HashMap::new();
        for n in notes.iter().filter(|n| !n.locked()) {
            for f in n.files() {
                let got = bear.file(&n.id, &f);
                files.insert((n.id.clone(), f), got);
            }
        }
        Ok(Self { notes, tags, files })
    }
}

impl Bear for Loaded {
    fn notes(&self) -> Result<Vec<BearNote>, String> {
        Ok(self.notes.clone())
    }
    fn tags(&self) -> Result<Vec<String>, String> {
        Ok(self.tags.clone())
    }
    fn file(&self, note: &str, name: &str) -> Result<Vec<u8>, String> {
        self.files.get(&(note.to_string(), name.to_string())).cloned().unwrap_or_else(|| Err("Bear 没给出这个文件".into()))
    }
}

fn bear_tag(t: &str) -> String {
    t.trim().trim_matches('#').trim().to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BearPreview {
    /// In Bear's notes list (not archived, not in the trash).
    pub notes: usize,
    pub archived: usize,
    pub trashed: usize,
    pub locked: usize,
    pub tags: usize,
    pub files: usize,
    /// Already here (the same title and created second).
    pub here: usize,
}

/// Bear's notes already here: the same title (as this module reads it) and created in the same second.
fn already_here(conn: &Connection, notes: &[&BearNote]) -> Result<usize, String> {
    let mut n = 0;
    for b in notes {
        let title = title_of(b.content.as_deref().unwrap_or(""));
        let created = b.created.get(..19).unwrap_or("");
        let hit: bool = conn
            .query_row("SELECT EXISTS (SELECT 1 FROM articles WHERE lower(title) = lower(?1) AND substr(created_at, 1, 19) = ?2)", rusqlite::params![title, created], |r| r.get(0))
            .map_err(err)?;
        n += hit as usize;
    }
    Ok(n)
}

pub fn bear_preview(conn: &Connection, bear: &dyn Bear) -> Result<BearPreview, String> {
    let notes = bear.notes()?;
    let readable: Vec<&BearNote> = notes.iter().filter(|n| !n.locked() && n.content.is_some()).collect();
    let tags: HashSet<String> = notes.iter().filter(|n| n.location != "trash").flat_map(|n| n.tags.iter().map(|t| bear_tag(t).to_lowercase())).filter(|t| !t.is_empty()).collect();
    Ok(BearPreview {
        notes: notes.iter().filter(|n| n.location == "notes").count(),
        archived: notes.iter().filter(|n| n.location == "archive").count(),
        trashed: notes.iter().filter(|n| n.location == "trash").count(),
        locked: notes.iter().filter(|n| n.locked()).count(),
        tags: tags.len(),
        files: notes.iter().map(|n| n.files().len()).sum(),
        here: already_here(conn, &readable)?,
    })
}

/// Bear's tags ignore case and its list has one spelling per tag (a sub-tag's last level is its own); a tag takes,
/// level by level, the spelling of the listed tag at that level.
struct Case {
    listed: HashMap<String, String>,
}

impl Case {
    fn new(listed: &[String]) -> Self {
        Self { listed: listed.iter().map(|t| bear_tag(t)).filter(|t| !t.is_empty()).map(|t| (t.to_lowercase(), t)).collect() }
    }
    fn canonical(&self, tag: &str) -> String {
        let parts: Vec<&str> = tag.split('/').collect();
        (0..parts.len())
            .map(|k| {
                let prefix = parts[..=k].join("/").to_lowercase();
                self.listed.get(&prefix).and_then(|s| s.split('/').nth(k)).filter(|s| s.to_lowercase() == parts[k].to_lowercase()).unwrap_or(parts[k]).to_string()
            })
            .collect::<Vec<_>>()
            .join("/")
    }
    fn apply(&self, text: &str, changed: &mut BTreeMap<(String, String), usize>) -> String {
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for t in tags::tokens(text, true) {
            let c = self.canonical(&t.tag);
            if c != t.tag {
                out.push_str(&text[last..t.hash + 1]);
                out.push_str(&c);
                last = t.hash + 1 + t.tag.len();
                *changed.entry((t.tag.clone(), c)).or_default() += 1;
            }
        }
        out.push_str(&text[last..]);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Left {
    pub title: String,
    pub what: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub imported: usize,
    /// Attachments brought.
    pub files: usize,
    /// Tags among the imported articles (with the levels above them, as the tag tree shows).
    pub tags: usize,
    /// What didn't come over (or came over differently).
    pub left: Vec<Left>,
    /// Changes made on the way (tag spellings).
    pub fixes: Vec<String>,
}

/// Rewrites the targets of `](…)` links outside code: `to(target, decoded)` gives the attachment name to write.
pub(super) fn relink(text: &str, mut to: impl FnMut(&str) -> Option<String>) -> String {
    let masked = tags::mask_code(text);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut at = 0;
    while let Some(i) = masked[at..].find("](").map(|i| at + i) {
        let start = i + 2;
        let (from, to_end, after) = if masked[start..].starts_with('<') {
            match masked[start..].find('>') {
                Some(j) => (start + 1, start + j, start + j + 1),
                None => {
                    at = start;
                    continue;
                }
            }
        } else {
            let j = masked[start..].find(|c: char| c == ')' || c.is_whitespace()).map(|j| start + j).unwrap_or(masked.len());
            (start, j, j)
        };
        let target = &text[from..to_end];
        if !target.is_empty() {
            if let Some(name) = to(&attachments::percent_decode(target)) {
                out.push_str(&text[last..start]);
                out.push_str(&encode_name(&name));
                last = after;
            }
        }
        at = after.max(start);
    }
    out.push_str(&text[last..]);
    out
}

/// A line naming each file the text doesn't (pictures shown, other files as links).
fn tail_links(text: &str, names: &[(String, bool)]) -> String {
    let mut out = text.trim_end().to_string();
    out.push('\n');
    for (name, image) in names {
        let e = encode_name(name);
        out.push('\n');
        out.push_str(&if *image { format!("![]({e})") } else { format!("[{name}]({e})") });
    }
    out
}

/// Keeps a record's files (stored ones; same bytes once), at most MAX_PER_RECORD; the text's links take the names.
struct Files<'a> {
    conn: &'a Connection,
    dir: &'a Path,
    ids: Vec<i64>,
    /// source name → stored name
    names: HashMap<String, (String, bool)>,
}

impl<'a> Files<'a> {
    fn new(conn: &'a Connection, dir: &'a Path) -> Self {
        Self { conn, dir, ids: vec![], names: HashMap::new() }
    }
    fn keep(&mut self, source: &str, name: &str, data: &[u8]) -> Result<String, String> {
        if let Some((n, _)) = self.names.get(source) {
            return Ok(n.clone());
        }
        if self.ids.len() >= MAX_PER_RECORD {
            return Err(format!("一篇最多 {MAX_PER_RECORD} 个附件"));
        }
        let f = attachments::store(self.conn, self.dir, name, data)?;
        if !self.ids.contains(&f.id) {
            self.ids.push(f.id);
        }
        self.names.insert(source.to_string(), (f.name.clone(), f.kind == "image"));
        Ok(f.name)
    }
}

/// Imports every readable Bear note. The caller made the 导入前 copy and owns the undo step.
pub fn import_bear(conn: &Connection, dir: &Path, bear: &dyn Bear) -> Result<Report, String> {
    let notes = bear.notes()?;
    let mut report = Report::default();
    let mut readable: Vec<&BearNote> = Vec::new();
    for n in &notes {
        if n.locked() || n.content.is_none() {
            report.left.push(Left { title: n.shown(), what: "加密的：Bear 的命令行读不出来，没搬（在 Bear 里取消加密再搬，或者把正文复制过来）".into() });
        } else {
            readable.push(n);
        }
    }
    if readable.is_empty() {
        return Err("Bear 里没有能搬的笔记".into());
    }
    let here = already_here(conn, &readable)?;
    if here * 2 > readable.len() {
        return Err(format!("Bear 的 {here} 篇已经在见字如面里了：从 Bear 搬家只做一次"));
    }
    let case = Case::new(&bear.tags()?);
    readable.sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
    let tx = Tx::begin(conn)?;
    let mut changed = BTreeMap::new();
    let mut made: Vec<(i64, String)> = Vec::new();
    for n in readable {
        let title = n.shown();
        let text = n.content.as_deref().unwrap_or("").replace("\r\n", "\n").replace('\r', "\n");
        let text = case.apply(&text, &mut changed);
        let mut files = Files::new(&tx, dir);
        for name in n.files() {
            if let Err(e) = bear.file(&n.id, &name).and_then(|b| files.keep(&name, &name, &b)) {
                report.left.push(Left { title: title.clone(), what: format!("附件「{name}」没搬过来：{e}") });
            }
        }
        let mut used = HashSet::new();
        let mut text = relink(&text, |target| {
            let hit = files.names.get(target).map(|(n, _)| n.clone());
            if hit.is_some() {
                used.insert(target.to_string());
            }
            hit
        });
        let mut unnamed: Vec<(String, bool)> = files.names.iter().filter(|(k, _)| !used.contains(*k)).map(|(_, v)| v.clone()).collect();
        if !unnamed.is_empty() {
            unnamed.sort();
            text = tail_links(&text, &unnamed);
            report.fixes.push(format!("「{title}」：正文里没写到的 {} 个附件接在了最后", unnamed.len()));
        }
        // Bear's tags this text doesn't read as tags (a space inside, …)
        let mut mine: HashSet<String> = HashSet::new();
        for t in tags::tags_in(&text, true) {
            let parts: Vec<&str> = t.split('/').collect();
            for k in 1..=parts.len() {
                mine.insert(parts[..k].join("/").to_lowercase());
            }
        }
        for t in &n.tags {
            let t = bear_tag(t);
            if !t.is_empty() && !mine.contains(&t.to_lowercase()) {
                report.left.push(Left { title: title.clone(), what: format!("Bear 的标签 #{t} 在这里不算标签（正文照原样）") });
            }
        }
        let trashed = (n.location == "trash").then_some(n.modified.as_str());
        let id = insert(&tx, &text, Some(&n.created), Some(&n.modified), !n.pins.is_empty(), n.location == "archive", trashed, &files.ids)?;
        report.imported += 1;
        report.files += files.ids.len();
        if trashed.is_none() {
            made.push((id, title));
        }
    }
    dead_links(&tx, &made, &mut report)?;
    report.tags = tag_count(&tx, &made)?;
    report.fixes.extend(changed.into_iter().map(|((from, to), n)| format!("标签 #{from} 写成了 #{to}（{n} 处：Bear 里它们是同一个标签）")));
    tx.commit()?;
    Ok(report)
}

/// `[[links]]` in the new articles that name no article here.
fn dead_links(conn: &Connection, made: &[(i64, String)], report: &mut Report) -> Result<(), String> {
    let titles: Vec<String> = super::titles(conn)?.into_iter().map(|l| l.title).collect();
    for (id, title) in made {
        let content: String = conn.query_row("SELECT content FROM articles WHERE id = ?1", [id], |r| r.get(0)).map_err(err)?;
        let mut seen = HashSet::new();
        for t in links_in(&content) {
            if seen.insert(t.clone()) && !titles.iter().any(|x| names(&t, x)) {
                report.left.push(Left { title: title.clone(), what: format!("互链 [[{t}]] 找不到同名的一篇") });
            }
        }
    }
    Ok(())
}

fn tag_count(conn: &Connection, made: &[(i64, String)]) -> Result<usize, String> {
    let mut all = HashSet::new();
    for (id, _) in made {
        let mut st = conn.prepare("SELECT tag FROM article_tags WHERE article_id = ?1").map_err(err)?;
        for t in st.query_map([id], |r| r.get::<_, String>(0)).map_err(err)? {
            let t = t.map_err(err)?;
            let parts: Vec<&str> = t.split('/').collect();
            for k in 1..=parts.len() {
                all.insert(parts[..k].join("/"));
            }
        }
    }
    Ok(all.len())
}

// ---------- files ----------

enum Source {
    Text(PathBuf),
    Bundle(PathBuf),
    Pack(PathBuf),
}

struct Item {
    src: Source,
    tag: Option<String>,
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn hidden(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

/// A folder's name as a tag level: spaces become -, # and / go.
fn tag_level(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join("-").replace(['#', '/'], "").trim_matches('-').to_string()
}

/// Walks what was picked: text files, bundles and packs become items; other files are kept aside (pictures the texts
/// link to are used later).
fn collect(path: &Path, tag: Option<String>, depth: usize, items: &mut Vec<Item>, others: &mut Vec<PathBuf>) -> Result<(), String> {
    if items.len() >= MAX_FILES || depth > MAX_DEPTH {
        return Ok(());
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| format!("读不了「{}」", path.display()))?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    let ext = ext_of(path);
    if meta.is_dir() {
        if ext == "textbundle" {
            items.push(Item { src: Source::Bundle(path.to_path_buf()), tag });
            return Ok(());
        }
        let mut kids: Vec<PathBuf> = std::fs::read_dir(path).map_err(|e| format!("读不了「{}」：{e}", path.display()))?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| !hidden(p)).collect();
        kids.sort();
        for k in kids {
            let is_folder = std::fs::symlink_metadata(&k).is_ok_and(|m| m.is_dir()) && ext_of(&k) != "textbundle";
            let child = if is_folder {
                let level = tag_level(&k.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                match (&tag, level.is_empty()) {
                    (_, true) => tag.clone(),
                    (Some(t), false) => Some(format!("{t}/{level}")),
                    (None, false) => Some(level),
                }
            } else {
                tag.clone()
            };
            collect(&k, child, depth + 1, items, others)?;
        }
        return Ok(());
    }
    match ext.as_str() {
        "md" | "markdown" | "txt" | "text" => items.push(Item { src: Source::Text(path.to_path_buf()), tag }),
        "textpack" => items.push(Item { src: Source::Pack(path.to_path_buf()), tag }),
        _ => others.push(path.to_path_buf()),
    }
    Ok(())
}

/// Where a text's linked files are found.
enum Assets {
    Dir(PathBuf),
    /// A pack's entries under the bundle's root (path inside → bytes).
    Zip(HashMap<String, Vec<u8>>),
}

impl Assets {
    /// A relative link → (file name, bytes, its path on disk).
    fn get(&self, target: &str) -> Option<(String, Vec<u8>, Option<PathBuf>)> {
        let t = target.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with('/') || t.contains("://") || t.starts_with("mailto:") || t.starts_with("data:") {
            return None;
        }
        let t = t.split(['?', '#']).next().unwrap_or(t);
        let name = t.rsplit('/').next().unwrap_or(t).to_string();
        match self {
            Assets::Dir(base) => {
                let p = base.join(t);
                let m = std::fs::metadata(&p).ok()?;
                if !m.is_file() || m.len() > MAX_BYTES || ext_of(&p) == "md" {
                    return None;
                }
                let canon = std::fs::canonicalize(&p).ok();
                Some((name, std::fs::read(&p).ok()?, canon))
            }
            Assets::Zip(map) => {
                let key = normalize(t);
                map.get(&key).map(|b| (name, b.clone(), None))
            }
        }
    }
}

/// A relative path with `.` / `..` resolved (inside a zip).
fn normalize(p: &str) -> String {
    let mut out: Vec<&str> = vec![];
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            x => out.push(x),
        }
    }
    out.join("/")
}

fn stamp_of_time(t: std::io::Result<std::time::SystemTime>) -> Option<String> {
    let s = t.ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as i64;
    let secs = s.div_euclid(1000);
    let ms = s.rem_euclid(1000);
    // civil date from days (UTC)
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil(days);
    Some(format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ms:03}Z", rem / 3600, rem % 3600 / 60, rem % 60))
}

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's algorithm).
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

struct Got {
    text: String,
    name: String,
    assets: Assets,
    created: Option<String>,
    modified: Option<String>,
    lossy: bool,
}

fn decode(bytes: Vec<u8>) -> (String, bool) {
    match String::from_utf8(bytes) {
        Ok(s) => (s, false),
        Err(e) => (String::from_utf8_lossy(e.as_bytes()).to_string(), true),
    }
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

fn read_item(src: &Source) -> Result<Got, String> {
    match src {
        Source::Text(p) => {
            let m = std::fs::metadata(p).map_err(|e| format!("读不了：{e}"))?;
            if m.len() > 8 * 1024 * 1024 {
                return Err("文件太大了（超过 8 MB）".into());
            }
            let (text, lossy) = decode(std::fs::read(p).map_err(|e| format!("读不了：{e}"))?);
            Ok(Got { text, name: stem(p), assets: Assets::Dir(p.parent().unwrap_or(Path::new("/")).to_path_buf()), created: stamp_of_time(m.created()), modified: stamp_of_time(m.modified()), lossy })
        }
        Source::Bundle(p) => {
            let file = ["text.md", "text.markdown", "text.txt"].iter().map(|n| p.join(n)).find(|f| f.is_file()).ok_or("TextBundle 里没有 text.md")?;
            let m = std::fs::metadata(&file).map_err(|e| format!("读不了：{e}"))?;
            let (text, lossy) = decode(std::fs::read(&file).map_err(|e| format!("读不了：{e}"))?);
            Ok(Got { text, name: stem(p), assets: Assets::Dir(p.clone()), created: stamp_of_time(m.created()), modified: stamp_of_time(m.modified()), lossy })
        }
        Source::Pack(p) => {
            let m = std::fs::metadata(p).map_err(|e| format!("读不了：{e}"))?;
            if m.len() > 512 * 1024 * 1024 {
                return Err("文件太大了".into());
            }
            let f = std::fs::File::open(p).map_err(|e| format!("读不了：{e}"))?;
            let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("不是 TextPack：{e}"))?;
            let names: Vec<String> = zip.file_names().filter_map(|n| n.ok().map(|c| c.to_string())).collect();
            let text_at = names.iter().filter(|n| ["text.md", "text.markdown", "text.txt"].iter().any(|t| n.as_str() == *t || n.ends_with(&format!("/{t}")))).min_by_key(|n| n.matches('/').count()).ok_or("TextPack 里没有 text.md")?.clone();
            let root = text_at.rsplit_once('/').map(|(r, _)| format!("{r}/")).unwrap_or_default();
            let mut map = HashMap::new();
            let mut text = Vec::new();
            let mut total = 0u64;
            for n in &names {
                if !n.starts_with(&root) || n.ends_with('/') {
                    continue;
                }
                let mut e = zip.by_name(n).map_err(|e| format!("TextPack 读坏了：{e}"))?;
                total += e.size();
                if e.size() > MAX_BYTES || total > 1024 * 1024 * 1024 {
                    continue;
                }
                let mut b = Vec::new();
                e.read_to_end(&mut b).map_err(|e| format!("TextPack 读坏了：{e}"))?;
                if *n == text_at {
                    text = b;
                } else {
                    map.insert(normalize(&n[root.len()..]), b);
                }
            }
            let (text, lossy) = decode(text);
            Ok(Got { text, name: stem(p), assets: Assets::Zip(map), created: stamp_of_time(m.created()), modified: stamp_of_time(m.modified()), lossy })
        }
    }
}

/// `text` with `#tag` on its second line (after the title line), unless it has the tag or one under it.
fn with_tag(text: &str, tag: &str) -> String {
    if tags::tags_in(text, true).iter().any(|t| tags::under(t, tag)) {
        return text.to_string();
    }
    match text.split_once('\n') {
        Some((first, rest)) => format!("{first}\n#{tag}\n{rest}"),
        None => format!("{text}\n#{tag}"),
    }
}

fn import_one(conn: &Connection, dir: &Path, it: &Item, used_paths: &mut HashSet<PathBuf>, report: &mut Report) -> Result<(), String> {
    let r = read_item(&it.src)?;
    let mut text = r.text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    if title_of(&text).is_empty() {
        text = if text.trim().is_empty() { format!("# {}", r.name) } else { format!("# {}\n\n{text}", r.name) };
    }
    if let Some(t) = &it.tag {
        if tags::clean_tag(t).is_ok() {
            text = with_tag(&text, t);
        }
    }
    let title = title_of(&text);
    let mut files = Files::new(conn, dir);
    let mut failed = Vec::new();
    let text = relink(&text, |target| {
        let (name, bytes, path) = r.assets.get(target)?;
        match files.keep(target, &name, &bytes) {
            Ok(n) => {
                if let Some(p) = path {
                    used_paths.insert(p);
                }
                Some(n)
            }
            Err(e) => {
                failed.push(format!("「{name}」：{e}"));
                None
            }
        }
    });
    for f in failed {
        report.left.push(Left { title: title.clone(), what: format!("附件 {f}") });
    }
    if r.lossy {
        report.fixes.push(format!("「{title}」不是 UTF-8，认不出的字成了 �"));
    }
    insert(conn, &text, r.created.as_deref(), r.modified.as_deref(), false, false, None, &files.ids)?;
    report.files += files.ids.len();
    Ok(())
}

/// Imports the picked files and folders.
pub fn import_files(conn: &Connection, dir: &Path, paths: &[PathBuf]) -> Result<Report, String> {
    let mut items = Vec::new();
    let mut others = Vec::new();
    for p in paths {
        collect(p, None, 0, &mut items, &mut others)?;
    }
    if items.is_empty() {
        return Err("没有找到 Markdown、TextBundle 文件".into());
    }
    let mut report = Report::default();
    let mut used = HashSet::new();
    let tx = Tx::begin(conn)?;
    let mut made = Vec::new();
    for it in &items {
        let shown = match &it.src {
            Source::Text(p) | Source::Bundle(p) | Source::Pack(p) => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        };
        let before: i64 = tx.query_row("SELECT coalesce(max(id), 0) FROM articles", [], |r| r.get(0)).map_err(err)?;
        match import_one(&tx, dir, it, &mut used, &mut report) {
            Ok(()) => {
                report.imported += 1;
                let id: i64 = tx.query_row("SELECT max(id) FROM articles", [], |r| r.get(0)).map_err(err)?;
                if id > before {
                    let title: String = tx.query_row("SELECT title FROM articles WHERE id = ?1", [id], |r| r.get(0)).map_err(err)?;
                    made.push((id, title));
                }
            }
            Err(e) => report.left.push(Left { title: shown, what: e }),
        }
    }
    let mut skipped: Vec<String> = others.iter().filter(|p| std::fs::canonicalize(p).map(|c| !used.contains(&c)).unwrap_or(true)).map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).collect();
    if !skipped.is_empty() {
        let n = skipped.len();
        skipped.truncate(12);
        report.left.push(Left { title: format!("{n} 个别的文件"), what: format!("不是 Markdown、也没有哪篇用到，没导入：{}{}", skipped.join("、"), if n > 12 { "……" } else { "" }) });
    }
    dead_links(&tx, &made, &mut report)?;
    report.tags = tag_count(&tx, &made)?;
    tx.commit()?;
    Ok(report)
}

// ---------- commands ----------

/// What Bear has (and how much of it is here already). Reads Bear only.
#[tauri::command]
pub async fn articles_bear_preview(state: State<'_, AppState>) -> Result<BearPreview, String> {
    let loaded = tauri::async_runtime::spawn_blocking(|| -> Result<Vec<BearNote>, String> { BearCli::find()?.notes() }).await.map_err(|e| e.to_string())??;
    let fake = Loaded { notes: loaded, tags: vec![], files: HashMap::new() };
    state.with_db(|c| bear_preview(c, &fake))
}

/// From Bear, once (a 导入前 copy first; one ⌘Z).
#[tauri::command]
pub async fn articles_bear_import(state: State<'_, AppState>) -> Result<Report, String> {
    let loaded = tauri::async_runtime::spawn_blocking(|| -> Result<Loaded, String> { Loaded::from(&BearCli::find()?) }).await.map_err(|e| e.to_string())??;
    let dir = attachments::folder_of(&state.paths.root);
    let backups = state.paths.backups.clone();
    state.with_db(|c| crate::backup::snapshot(c, &backups, "before-import").map(|_| ()))?;
    state.with_undo("从 Bear 搬家", |c| import_bear(c, &dir, &loaded))
}

/// Markdown / TextBundle files and folders (one ⌘Z).
#[tauri::command]
pub fn articles_import_files(state: State<AppState>, paths: Vec<String>) -> Result<Report, String> {
    let dir = attachments::folder_of(&state.paths.root);
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    state.with_undo("导入笔记", |c| import_files(c, &dir, &paths))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::modules::articles::{get, list, Filter};
    use serde_json::json;

    struct Fake {
        notes: Vec<BearNote>,
        tags: Vec<String>,
        files: HashMap<(String, String), Vec<u8>>,
    }
    impl Bear for Fake {
        fn notes(&self) -> Result<Vec<BearNote>, String> {
            Ok(self.notes.clone())
        }
        fn tags(&self) -> Result<Vec<String>, String> {
            Ok(self.tags.clone())
        }
        fn file(&self, note: &str, name: &str) -> Result<Vec<u8>, String> {
            self.files.get(&(note.into(), name.into())).cloned().ok_or_else(|| "没有".into())
        }
    }
    fn note(v: Value) -> BearNote {
        serde_json::from_value(v).unwrap()
    }
    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("pomelo-import-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake() -> Fake {
        Fake {
            notes: vec![
                note(json!({"id": "A", "title": "东京的第三天", "locked": "no", "tags": ["#旅行", "#旅行/日本"], "created": "2026-11-06T01:00:00Z", "modified": "2026-11-07T02:00:00Z", "pins": ["global"], "location": "notes", "attachments": ["Big Fish.jpg"],
                    "content": "# 东京的第三天\n#旅行/日本\n## 上午 <!-- {\"fold\":true} -->\n![](Big%20Fish.jpg)\n见 [[计划]] 和 [[不存在的]]"})),
                note(json!({"id": "B", "title": "计划", "locked": "no", "tags": ["#nus-iss/asd", "#NUS-ISS", "#multi word#"], "created": "2026-11-06T01:00:00Z", "modified": "2026-11-06T01:00:00Z", "pins": [], "location": "archive", "attachments": ["a.pdf"],
                    "content": "# 计划\n#nus-iss/asd 和 #multi word#"})),
                note(json!({"id": "C", "title": "", "locked": "no", "tags": [], "created": "2026-11-01T00:00:00Z", "modified": "2026-11-02T00:00:00Z", "pins": [], "location": "trash", "attachments": [], "content": "# \n"})),
                note(json!({"id": "D", "title": "锁着的", "locked": "yes", "tags": [], "created": "2026-11-01T00:00:00Z", "modified": "2026-11-02T00:00:00Z", "pins": [], "location": "notes", "attachments": []})),
            ],
            tags: vec!["#NUS-ISS".into(), "#nus-iss/asd".into(), "#旅行".into(), "#旅行/日本".into()],
            files: [(("A".into(), "Big Fish.jpg".into()), b"\xff\xd8 jpeg".to_vec()), (("B".into(), "a.pdf".into()), b"%PDF-1.4".to_vec())].into_iter().collect(),
        }
    }

    #[test]
    fn bear_comes_over_whole() {
        let c = memory_db();
        let dir = tmp();
        let bear = fake();
        let p = bear_preview(&c, &bear).unwrap();
        assert_eq!((p.notes, p.archived, p.trashed, p.locked, p.files, p.here), (2, 1, 1, 1, 2, 0));
        let r = import_bear(&c, &dir, &bear).unwrap();
        assert_eq!((r.imported, r.files), (3, 2));
        let all = list(&c, &Filter { view: Some("any".into()), ..Default::default() }).unwrap();
        let tokyo = all.iter().find(|s| s.title == "东京的第三天").unwrap();
        assert!(tokyo.pinned && tokyo.thumb.is_some());
        assert_eq!(tokyo.created_at, "2026-11-06T01:00:00.000Z");
        assert_eq!(tokyo.updated_at, "2026-11-07T02:00:00.000Z");
        let a = get(&c, tokyo.id, None).unwrap();
        assert!(a.content.contains("![](Big%20Fish.jpg)") && a.content.contains("## 上午 <!-- {\"fold\":true} -->"));
        // the same created second: nudged by 1 ms
        let plan = all.iter().find(|s| s.title == "计划").unwrap();
        assert!(plan.archived && plan.created_at == "2026-11-06T01:00:00.001Z");
        // Bear's case: the listed spelling of each level
        let b = get(&c, plan.id, None).unwrap();
        assert!(b.content.contains("#NUS-ISS/asd"), "{}", b.content);
        assert_eq!(b.tags, ["NUS-ISS/asd", "multi"]);
        // a file the text never names is linked at the end
        assert!(b.content.ends_with("[a.pdf](a.pdf)"));
        assert!(all.iter().any(|s| s.trashed));
        let left: Vec<&str> = r.left.iter().map(|l| l.what.as_str()).collect();
        assert!(left.iter().any(|w| w.starts_with("加密的")));
        assert!(left.iter().any(|w| w.contains("#multi word")));
        assert!(left.iter().any(|w| w.contains("[[不存在的]]")) && !left.iter().any(|w| w.contains("[[计划]]")));
        assert!(r.fixes.iter().any(|f| f.contains("#nus-iss/asd 写成了 #NUS-ISS/asd")));
        // once only
        assert!(import_bear(&c, &dir, &bear).unwrap_err().contains("只做一次"));
        assert_eq!(bear_preview(&c, &bear).unwrap().here, 3);
        // the title ignores Bear's fold mark
        assert_eq!(super::super::title_of("## 上午 <!-- {\"fold\":true} -->"), "上午");
    }

    #[test]
    fn markdown_files_and_folders() {
        let c = memory_db();
        let dir = tmp();
        let src = tmp();
        let root = src.join("Bear 导出");
        std::fs::create_dir_all(root.join("读书/小说/东京")).unwrap();
        std::fs::create_dir_all(root.join("散记")).unwrap();
        std::fs::write(root.join("读书/小说/东京.md"), "# 东京\n正文 ![](%E4%B8%9C%E4%BA%AC/pic%201.png) [文件](东京/x.pdf)").unwrap();
        std::fs::write(root.join("读书/小说/东京/pic 1.png"), b"\x89PNG one").unwrap();
        std::fs::write(root.join("读书/小说/东京/x.pdf"), b"%PDF x").unwrap();
        std::fs::write(root.join("读书/没有标题.md"), "\u{feff}第一行\r\n第二行 #读书").unwrap();
        std::fs::write(root.join("散记/.DS_Store"), b"x").unwrap();
        std::fs::write(root.join("散记/清单.txt"), "").unwrap();
        std::fs::write(root.join("散记/图.jpg"), b"\xff\xd8 stray").unwrap();
        // a TextBundle and a TextPack
        let tb = src.join("信.textbundle");
        std::fs::create_dir_all(tb.join("assets")).unwrap();
        std::fs::write(tb.join("text.md"), "# 一封信\n![](assets/a.png)").unwrap();
        std::fs::write(tb.join("assets/a.png"), b"\x89PNG a").unwrap();
        let pack = src.join("包.textpack");
        {
            let f = std::fs::File::create(&pack).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let o = zip::write::SimpleFileOptions::default();
            use std::io::Write;
            z.start_file("包.textbundle/text.md", o).unwrap();
            z.write_all("# 打包的\n![](assets/b.png)".as_bytes()).unwrap();
            z.start_file("包.textbundle/assets/b.png", o).unwrap();
            z.write_all(b"\x89PNG b").unwrap();
            z.finish().unwrap();
        }
        let r = import_files(&c, &dir, &[root.clone(), tb, pack]).unwrap();
        assert_eq!(r.imported, 5, "{r:?}");
        assert_eq!(r.files, 4);
        let all = list(&c, &Filter::default()).unwrap();
        let tokyo = all.iter().find(|s| s.title == "东京").unwrap();
        assert_eq!(tokyo.tags, ["读书/小说"]);
        let t = get(&c, tokyo.id, None).unwrap();
        assert_eq!(t.content, "# 东京\n#读书/小说\n正文 ![](pic%201.png) [文件](x.pdf)");
        assert_eq!(t.attachments.len(), 2);
        // no title: the file's name; the folder's tag is there already
        let first = all.iter().find(|s| s.title == "第一行").unwrap();
        assert_eq!(first.tags, ["读书"]);
        let empty = all.iter().find(|s| s.title == "清单").unwrap();
        assert_eq!(empty.tags, ["散记"]);
        assert!(all.iter().any(|s| s.title == "一封信" && s.thumb.is_some()));
        assert!(all.iter().any(|s| s.title == "打包的" && s.thumb.is_some()));
        assert!(r.left.iter().any(|l| l.what.contains("图.jpg")), "{:?}", r.left);
        assert!(import_files(&c, &dir, &[src.join("没有这个")]).is_err());
    }

    #[test]
    fn relinking_and_dates() {
        assert_eq!(relink("![](a%20b.png) `![](a%20b.png)` [x](<a b.png>) [y](https://a.b)", |t| (t == "a b.png").then(|| "c (1).png".into())), "![](c%20%281%29.png) `![](a%20b.png)` [x](c%20%281%29.png) [y](https://a.b)");
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_406), (2025, 11, 14));
        assert_eq!(tag_level(" 读书 笔记#1 "), "读书-笔记1");
        assert_eq!(with_tag("# 标题\n正文", "读书"), "# 标题\n#读书\n正文");
        assert_eq!(with_tag("# 标题 #读书/小说", "读书"), "# 标题 #读书/小说");
        assert_eq!(normalize("a/./b/../c.png"), "a/c.png");
    }

    /// The real Bear, read only, into a data dir (POMELO_BEAR_INTO: its pomelo.db brought up to date first, a 导入前
    /// copy made; the app must be closed), or an in-memory one: `cargo test real_bear_import -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_bear_import() {
        let bear = Loaded::from(&BearCli::find().unwrap()).unwrap();
        let (mut c, dir) = match std::env::var("POMELO_BEAR_INTO") {
            Ok(root) => {
                let paths = crate::paths::DataPaths::new(PathBuf::from(&root));
                paths.ensure().unwrap();
                let mut c = crate::db::open(&paths.db).unwrap();
                crate::db::migrate(&mut c, crate::db::MIGRATIONS, &paths.backups).unwrap();
                assert_eq!(crate::db::user_version(&c).unwrap(), crate::db::tests::latest_version());
                println!("导入前：{}", crate::backup::snapshot(&c, &paths.backups, "before-import").unwrap().display());
                (c, attachments::folder_of(&paths.root))
            }
            Err(_) => (memory_db(), tmp()),
        };
        println!("{}", serde_json::to_string_pretty(&bear_preview(&c, &bear).unwrap()).unwrap());
        let r = {
            let tx = c.transaction().unwrap();
            let r = import_bear(&tx, &dir, &bear).unwrap();
            tx.commit().unwrap();
            r
        };
        println!("{}", serde_json::to_string_pretty(&r).unwrap());
        // every note's times came over (to the second), its pictures with it
        let mut off = 0;
        for n in bear.notes().unwrap().iter().filter(|n| !n.locked()) {
            let (pics, ok): (i64, bool) = c
                .query_row(
                    "SELECT (SELECT count(*) FROM attachments t WHERE t.owner = 'article' AND t.record_id = a.id), substr(a.updated_at, 1, 19) = substr(?2, 1, 19) FROM articles a WHERE substr(a.created_at, 1, 19) = substr(?1, 1, 19) AND a.content = ?3 OR (substr(a.created_at, 1, 19) = substr(?1, 1, 19) AND a.title = ?4) LIMIT 1",
                    rusqlite::params![n.created, n.modified, n.content.clone().unwrap_or_default(), title_of(n.content.as_deref().unwrap_or(""))],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap_or((-1, false));
            if !ok || pics != n.files().len() as i64 {
                off += 1;
                println!("对不上：{} {pics} {ok}", n.shown());
            }
        }
        println!("时间和图片对不上的：{off}");
    }
}
