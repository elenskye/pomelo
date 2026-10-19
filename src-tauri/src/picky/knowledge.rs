//! 知识库 (v0.4.5 阶段 7): libraries of folders and saved web pages that a conversation searches
//! while its 知识库 switch is on (the composer's button, kept per conversation like 思考).
//! - picky.db (undoable, `with_picky_undo`): libraries (name, order); their sources — a folder (its
//!   absolute path: files are read in place, never copied) or a saved web page (its article, a copy
//!   like 添加网页) — each with 「发给云端」 (default on); the libraries a conversation or a project
//!   chose; the conversation's switch.
//! - What a question searches (`scope`): the conversation's own libraries, else its project's, else
//!   all. A new conversation in a project that chose libraries starts with the switch on (the page).
//!   With a cloud model, sources not allowed there are left out (`Scope::skipped`, shown in the menu).
//! - search.db (derived, never undone): every readable file of each folder (`kb_docs`: path, size and
//!   mtime last seen, what its passages were made from) and its passages with int8 vectors
//!   (`kb_chunks`). `scan` (opening 小拾, every 10 minutes, after adding) compares size and mtime:
//!   new and changed files become pending, gone ones go. Building a pending file reads it again
//!   (files::read: the formats attachments take, whole tables), cuts it like project files
//!   (index::pieces) and embeds what changed — passages whose text is the same keep their vectors;
//!   the same bytes only mark it current. Images are listed but never built (no text: `PENDING`
//!   leaves them out); a cloud model looks at one with `view=look` (`look`: shrunk like an attached
//!   image, sent in a user message after the tool results — chat.rs).
//! - When (the user's choice): pending files are built while 8081 runs anyway (after a scan), or
//!   when a question searches them (`prepare`, which starts 8081); adding a source and 重新建立 start
//!   it too. One build at a time: a running one takes new requests (`Inner::want`), and a question
//!   meanwhile uses what is ready.
//! - Folders: hidden files and folders, Office lock files (~$), symlinks, packages (.app…) and
//!   node_modules are skipped; at most 2000 files, 200 MB each. A folder that can't be found keeps
//!   its passages (shown as missing); a web page whose copy was deleted loses them.
//! - Retrieval: like project files (index.rs): a pool of 20 passages ≥ 0.30 by similarity, reordered
//!   by the rerank model, at most 6 (without it: ≥ 0.35, the best 6). Each passage carries its
//!   source (`KbRef`) for its citation, which opens the file (or the page) it came from.
//! - 查知识库 (`kb_tool`) also lists the files of the libraries searched (`files_view`: as last
//!   scanned, by a word of their path) and reads one whole, fresh from its folder (`browse`: outline /
//!   read / find, shared with 读文件), what it reads numbered as 知识库 passages.

use super::catalog::ServiceModel;
use super::embed::{self, EmbedError, Input};
use super::files::{self, Segment};
use super::index::{self, Passage, Piece};
use super::runtime::{Runtime, ServiceState, Svc};
use crate::modules::common::{err, Tx};
use crate::search::vectors;
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

const NAME_CHARS: usize = 30;
pub const MAX_FILES: usize = 2000;
const READABLE: &[&str] = &["txt", "text", "md", "markdown", "pdf", "docx", "doc", "pptx", "xlsx", "xlsm", "xls", "ods", "csv"];
/// Listed (and looked at by a cloud model), never built.
const IMAGES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "heic", "heif"];

/// A folder's image (by its extension).
pub fn is_image(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, x)| IMAGES.contains(&x.to_lowercase().as_str()))
}
const PACKAGES: &[&str] = &["app", "bundle", "framework", "photoslibrary", "musiclibrary", "tvlibrary", "pkg", "xcodeproj", "xcworkspace"];
/// The monitor scans the folders this often (seconds).
pub const SCAN_EVERY: u64 = 600;

/// Where a passage came from (citations open its file or page).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KbRef {
    pub source: i64,
    /// The file, relative to the folder; "" for a web page.
    pub path: String,
    /// The library's name when it was found.
    pub library: String,
}

// ---------- picky.db: libraries and sources ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: i64,
    pub library_id: i64,
    /// folder | web
    pub kind: String,
    /// A folder's last part, or the page's title.
    pub name: String,
    pub path: Option<String>,
    /// A web page: its address and its copy (deleted by storage management: no passages).
    pub url: Option<String>,
    pub artifact_id: Option<i64>,
    pub deleted: bool,
    pub cloud: bool,
}

fn folder_name(p: &str) -> String {
    Path::new(p).file_name().map_or_else(|| p.to_string(), |n| n.to_string_lossy().to_string())
}

const SOURCE_COLS: &str = "s.id, s.library_id, s.kind, s.path, s.artifact_id, s.cloud, a.display_name, a.source_url, a.deleted_at IS NOT NULL";

fn source_row(r: &rusqlite::Row) -> rusqlite::Result<Source> {
    let path: Option<String> = r.get(3)?;
    let title: Option<String> = r.get(6)?;
    let name = match &path {
        Some(p) => folder_name(p),
        None => title.unwrap_or_else(|| "网页".into()),
    };
    Ok(Source { id: r.get(0)?, library_id: r.get(1)?, kind: r.get(2)?, name, path, url: r.get(7)?, artifact_id: r.get(4)?, deleted: r.get::<_, Option<bool>>(8)?.unwrap_or(false), cloud: r.get(5)? })
}

pub fn libraries(conn: &Connection) -> Result<Vec<Library>, String> {
    let mut st = conn.prepare("SELECT id, name FROM kb_libraries ORDER BY sort_order, id").map_err(err)?;
    let rows = st.query_map([], |r| Ok(Library { id: r.get(0)?, name: r.get(1)? })).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Every source (or one library's), in list order.
pub fn sources(conn: &Connection, library: Option<i64>) -> Result<Vec<Source>, String> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {SOURCE_COLS} FROM kb_sources s JOIN kb_libraries l ON l.id = s.library_id LEFT JOIN artifacts a ON a.id = s.artifact_id
             WHERE ?1 IS NULL OR s.library_id = ?1 ORDER BY l.sort_order, l.id, s.sort_order, s.id"
        ))
        .map_err(err)?;
    let rows = st.query_map([library], source_row).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

pub fn source(conn: &Connection, id: i64) -> Result<Source, String> {
    conn.query_row(&format!("SELECT {SOURCE_COLS} FROM kb_sources s LEFT JOIN artifacts a ON a.id = s.artifact_id WHERE s.id = ?1"), [id], source_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| "这个文件夹或网页已经不在知识库里了".into())
}

fn library_name(conn: &Connection, name: &str, except: Option<i64>) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("请填写库的名称".into());
    }
    if n.chars().count() > NAME_CHARS {
        return Err(format!("库的名称最多 {NAME_CHARS} 个字"));
    }
    let taken: Option<i64> = conn.query_row("SELECT id FROM kb_libraries WHERE name = ?1 AND id IS NOT ?2", params![n, except], |r| r.get(0)).optional().map_err(err)?;
    if taken.is_some() {
        return Err(format!("已经有叫「{n}」的库了"));
    }
    Ok(n.to_string())
}

fn library_exists(conn: &Connection, id: i64) -> Result<(), String> {
    conn.query_row("SELECT id FROM kb_libraries WHERE id = ?1", [id], |r| r.get::<_, i64>(0)).optional().map_err(err)?.map(|_| ()).ok_or_else(|| "这个库已经被删除了".into())
}

/// New libraries go last.
pub fn create_library(conn: &Connection, name: &str) -> Result<i64, String> {
    let name = library_name(conn, name, None)?;
    conn.execute("INSERT INTO kb_libraries (name, sort_order) VALUES (?1, (SELECT coalesce(max(sort_order), -1) + 1 FROM kb_libraries))", [name]).map_err(err)?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_library(conn: &Connection, id: i64, name: &str) -> Result<(), String> {
    library_exists(conn, id)?;
    let name = library_name(conn, name, Some(id))?;
    conn.execute("UPDATE kb_libraries SET name = ?1 WHERE id = ?2 AND name <> ?1", params![name, id]).map(|_| ()).map_err(err)
}

/// Its sources and the choices of it go too; the folders themselves are never touched.
pub fn delete_library(conn: &Connection, id: i64) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    if tx.execute("DELETE FROM kb_libraries WHERE id = ?1", [id]).map_err(err)? == 0 {
        return Err("这个库已经被删除了".into());
    }
    tx.commit()
}

/// `ids`: every library, in their new order.
pub fn reorder_libraries(conn: &Connection, ids: &[i64]) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    let have: HashSet<i64> = libraries(&tx)?.into_iter().map(|l| l.id).collect();
    if have != ids.iter().copied().collect::<HashSet<_>>() || have.len() != ids.len() {
        return Err("库变了，请重新打开再拖".into());
    }
    for (i, id) in ids.iter().enumerate() {
        tx.execute("UPDATE kb_libraries SET sort_order = ?1 WHERE id = ?2 AND sort_order <> ?1", params![i as i64, id]).map_err(err)?;
    }
    tx.commit()
}

/// `data`: Pomelo's own data folder (never in a library). Folders of one library don't overlap.
pub fn add_folder(conn: &Connection, library: i64, path: &Path, data: &Path) -> Result<i64, String> {
    let real = std::fs::canonicalize(path).map_err(|_| "找不到这个文件夹".to_string())?;
    if !real.is_dir() {
        return Err("请选择一个文件夹".into());
    }
    if real.parent().is_none() {
        return Err("不能把整个磁盘放进知识库".into());
    }
    let data = std::fs::canonicalize(data).unwrap_or_else(|_| data.to_path_buf());
    if real.starts_with(&data) || data.starts_with(&real) {
        return Err("这个文件夹里有 Pomelo 自己的数据，不能放进知识库".into());
    }
    let tx = Tx::begin(conn)?;
    library_exists(&tx, library)?;
    for s in sources(&tx, Some(library))? {
        let Some(p) = &s.path else { continue };
        let other = Path::new(p);
        if other == real {
            return Err("这个库里已经有这个文件夹了".into());
        }
        if real.starts_with(other) || other.starts_with(&real) {
            return Err(format!("和库里的「{}」重叠了：一个在另一个里面", s.name));
        }
    }
    tx.execute(
        "INSERT INTO kb_sources (library_id, kind, path, sort_order) VALUES (?1, 'folder', ?2, (SELECT coalesce(max(sort_order), -1) + 1 FROM kb_sources WHERE library_id = ?1))",
        params![library, real.to_string_lossy().to_string()],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(id)
}

/// A saved web page (picky_import_web stored it).
pub fn add_web(conn: &Connection, library: i64, artifact: i64) -> Result<i64, String> {
    let f = files::usable(conn, artifact)?;
    if f.url.is_none() {
        return Err("只能添加网页".into());
    }
    let tx = Tx::begin(conn)?;
    library_exists(&tx, library)?;
    let dup: Option<i64> = tx.query_row("SELECT id FROM kb_sources WHERE library_id = ?1 AND artifact_id = ?2", [library, artifact], |r| r.get(0)).optional().map_err(err)?;
    if dup.is_some() {
        return Err("这个库里已经有这个网页了".into());
    }
    tx.execute(
        "INSERT INTO kb_sources (library_id, kind, artifact_id, sort_order) VALUES (?1, 'web', ?2, (SELECT coalesce(max(sort_order), -1) + 1 FROM kb_sources WHERE library_id = ?1))",
        [library, artifact],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(id)
}

pub fn remove_source(conn: &Connection, id: i64) -> Result<(), String> {
    if conn.execute("DELETE FROM kb_sources WHERE id = ?1", [id]).map_err(err)? == 0 {
        return Err("这个文件夹或网页已经不在知识库里了".into());
    }
    Ok(())
}

pub fn set_cloud(conn: &Connection, id: i64, on: bool) -> Result<(), String> {
    source(conn, id)?;
    conn.execute("UPDATE kb_sources SET cloud = ?1 WHERE id = ?2 AND cloud <> ?1", params![on, id]).map(|_| ()).map_err(err)
}

// ---------- picky.db: choices ----------

#[derive(Debug, Clone, Copy)]
pub enum Owner {
    Session(i64),
    Project(i64),
}

impl Owner {
    fn parts(self) -> (&'static str, &'static str, i64) {
        match self {
            Owner::Session(id) => ("session_libraries", "session_id", id),
            Owner::Project(id) => ("project_libraries", "project_id", id),
        }
    }
}

/// The libraries a conversation or project chose, in library order.
pub fn chosen(conn: &Connection, owner: Owner) -> Result<Vec<i64>, String> {
    let (table, col, id) = owner.parts();
    let mut st = conn.prepare(&format!("SELECT c.library_id FROM {table} c JOIN kb_libraries l ON l.id = c.library_id WHERE c.{col} = ?1 ORDER BY l.sort_order, l.id")).map_err(err)?;
    let rows = st.query_map([id], |r| r.get(0)).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Only the difference is written (so an unchanged choice records nothing).
pub fn set_chosen(conn: &Connection, owner: Owner, ids: &[i64]) -> Result<(), String> {
    let (table, col, id) = owner.parts();
    let tx = Tx::begin(conn)?;
    let have: HashSet<i64> = chosen(&tx, owner)?.into_iter().collect();
    let want: HashSet<i64> = ids.iter().copied().collect();
    for l in want.difference(&have) {
        library_exists(&tx, *l)?;
        tx.execute(&format!("INSERT INTO {table} ({col}, library_id) VALUES (?1, ?2)"), [id, *l]).map_err(err)?;
    }
    for l in have.difference(&want) {
        tx.execute(&format!("DELETE FROM {table} WHERE {col} = ?1 AND library_id = ?2"), [id, *l]).map_err(err)?;
    }
    tx.commit()
}

/// The composer's 知识库 switch of a conversation.
pub fn set_knowledge(conn: &Connection, session: i64, on: bool) -> Result<(), String> {
    conn.query_row("SELECT id FROM sessions WHERE id = ?1", [session], |r| r.get::<_, i64>(0)).optional().map_err(err)?.ok_or("找不到这个对话，可能已经被删除了")?;
    conn.execute("UPDATE sessions SET knowledge = ?1 WHERE id = ?2 AND knowledge <> ?1", params![on, session]).map(|_| ()).map_err(err)
}

/// A new conversation's switch and libraries (kept on the page until its first question).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub on: bool,
    pub libraries: Vec<i64>,
}

/// Saves a draft's choice with its new conversation.
pub fn save_draft(conn: &Connection, session: i64, d: &Draft) -> Result<(), String> {
    if d.on {
        set_knowledge(conn, session, true)?;
    }
    let known: HashSet<i64> = libraries(conn)?.into_iter().map(|l| l.id).collect();
    let ids: Vec<i64> = d.libraries.iter().copied().filter(|l| known.contains(l)).collect();
    set_chosen(conn, Owner::Session(session), &ids)
}

// ---------- what a question searches ----------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scope {
    /// The libraries searched.
    pub libraries: Vec<String>,
    /// Their sources a question may use, each with its library's name.
    pub sources: Vec<(Source, String)>,
    /// Sources left out: not sent to a cloud model.
    pub skipped: Vec<String>,
}

impl Scope {
    pub fn ids(&self) -> HashSet<i64> {
        self.sources.iter().map(|s| s.0.id).collect()
    }
}

/// None when the switch is off or there is no library. `session` None: a draft (`draft`, in `project`).
pub fn scope(conn: &Connection, session: Option<i64>, project: Option<i64>, draft: Option<&Draft>, cloud: bool) -> Result<Option<Scope>, String> {
    scope_with(conn, session, project, draft, cloud, false)
}

/// What 查知识库 (the skill) may search for a conversation: its libraries whether its switch is on or not.
pub fn tool_scope(conn: &Connection, session: i64, cloud: bool) -> Result<Option<Scope>, String> {
    Ok(scope_with(conn, Some(session), None, None, cloud, true)?.filter(|s| !s.sources.is_empty()))
}

fn scope_with(conn: &Connection, session: Option<i64>, project: Option<i64>, draft: Option<&Draft>, cloud: bool, always: bool) -> Result<Option<Scope>, String> {
    let (on, own, project) = match session {
        Some(s) => {
            let (on, project): (bool, Option<i64>) =
                conn.query_row("SELECT knowledge, project_id FROM sessions WHERE id = ?1", [s], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(err)?.ok_or("找不到这个对话，可能已经被删除了")?;
            (on, chosen(conn, Owner::Session(s))?, project)
        }
        None => (draft.is_some_and(|d| d.on), draft.map(|d| d.libraries.clone()).unwrap_or_default(), project),
    };
    if !on && !always {
        return Ok(None);
    }
    let all = libraries(conn)?;
    let mut ids: Vec<i64> = own.into_iter().filter(|id| all.iter().any(|l| l.id == *id)).collect();
    if ids.is_empty() {
        if let Some(p) = project {
            ids = chosen(conn, Owner::Project(p))?;
        }
    }
    let picked: Vec<&Library> = all.iter().filter(|l| ids.is_empty() || ids.contains(&l.id)).collect();
    if picked.is_empty() {
        return Ok(None);
    }
    let mut out = Scope { libraries: picked.iter().map(|l| l.name.clone()).collect(), ..Default::default() };
    for s in sources(conn, None)? {
        let Some(l) = picked.iter().find(|l| l.id == s.library_id) else { continue };
        if cloud && !s.cloud {
            out.skipped.push(s.name.clone());
        } else if !s.deleted {
            out.sources.push((s, l.name.clone()));
        }
    }
    Ok(Some(out))
}

// ---------- search.db: what the folders hold ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    pub id: i64,
    pub source_id: i64,
    /// Relative to the folder; "" for a web page.
    pub path: String,
    pub size: i64,
    pub mtime: i64,
    /// What its passages were made from.
    pub sha256: Option<String>,
    pub model: Option<String>,
    /// Its passages match the size and mtime last seen.
    pub fresh: bool,
    /// The file as last seen couldn't be read.
    pub error: Option<String>,
}

const DOC_COLS: &str = "id, source_id, path, size, mtime, sha256, model, fresh, error";

fn doc_row(r: &rusqlite::Row) -> rusqlite::Result<Doc> {
    Ok(Doc { id: r.get(0)?, source_id: r.get(1)?, path: r.get(2)?, size: r.get(3)?, mtime: r.get(4)?, sha256: r.get(5)?, model: r.get(6)?, fresh: r.get(7)?, error: r.get(8)? })
}

/// Needs (re)building for `?1` (the embedding model): changed since, or never made with it.
/// What still needs building (images never do).
const PENDING: &str = "(NOT (lower(path) GLOB '*.png' OR lower(path) GLOB '*.jpg' OR lower(path) GLOB '*.jpeg' OR lower(path) GLOB '*.gif' OR lower(path) GLOB '*.webp' OR lower(path) GLOB '*.heic' OR lower(path) GLOB '*.heif') AND (fresh = 0 OR (error IS NULL AND (model IS NULL OR model <> ?1))))";

fn id_list(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

pub fn docs(search: &Connection, source: i64) -> Result<Vec<Doc>, String> {
    let mut st = search.prepare(&format!("SELECT {DOC_COLS} FROM kb_docs WHERE source_id = ?1 ORDER BY path")).map_err(err)?;
    let rows = st.query_map([source], doc_row).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

pub fn next_pending(search: &Connection, sources: &[i64], model: &str) -> Result<Option<Doc>, String> {
    if sources.is_empty() {
        return Ok(None);
    }
    search
        .query_row(&format!("SELECT {DOC_COLS} FROM kb_docs WHERE source_id IN ({}) AND {PENDING} ORDER BY source_id, path LIMIT 1", id_list(sources)), [model], doc_row)
        .optional()
        .map_err(err)
}

/// A readable file seen in a folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub path: String,
    pub size: i64,
    pub mtime: i64,
}

fn skipped_dir(name: &str) -> bool {
    name == "node_modules" || name.rsplit_once('.').is_some_and(|(_, ext)| PACKAGES.contains(&ext.to_lowercase().as_str()))
}

/// The folder's readable files (subfolders too, by level), and whether there were more than MAX_FILES.
pub fn walk(folder: &Path) -> Result<(Vec<Seen>, bool), String> {
    if !std::fs::metadata(folder).is_ok_and(|m| m.is_dir()) {
        return Err("找不到这个文件夹".into());
    }
    let mut out = vec![];
    let mut queue: VecDeque<PathBuf> = VecDeque::from([PathBuf::new()]);
    while let Some(rel) = queue.pop_front() {
        let Ok(rd) = std::fs::read_dir(folder.join(&rel)) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Some(name) = e.file_name().to_str().map(String::from) else { continue };
            if name.starts_with('.') || name.starts_with("~$") {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue }; // a symlink is neither a file nor a folder here
            let r = rel.join(&name);
            if ft.is_dir() {
                if !skipped_dir(&name) {
                    queue.push_back(r);
                }
                continue;
            }
            let ext = name.rsplit_once('.').map(|(_, x)| x.to_lowercase()).unwrap_or_default();
            if !ft.is_file() || !(READABLE.contains(&ext.as_str()) || IMAGES.contains(&ext.as_str())) {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.len() == 0 || meta.len() > files::MAX_BYTES {
                continue;
            }
            let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as i64);
            out.push(Seen { path: r.to_string_lossy().to_string(), size: meta.len() as i64, mtime });
            if out.len() >= MAX_FILES {
                return Ok((out, true));
            }
        }
    }
    Ok((out, false))
}

/// What a scan found in one source: new files are added, changed ones become pending, gone ones go.
/// Returns whether anything changed.
pub fn apply_scan(search: &Connection, source: i64, seen: &[Seen]) -> Result<bool, String> {
    let tx = Tx::begin(search)?;
    let have: HashMap<String, (i64, i64, i64)> = docs(&tx, source)?.into_iter().map(|d| (d.path, (d.id, d.size, d.mtime))).collect();
    let mut changed = false;
    for s in seen {
        match have.get(&s.path) {
            None => {
                tx.execute("INSERT INTO kb_docs (source_id, path, size, mtime) VALUES (?1, ?2, ?3, ?4)", params![source, s.path, s.size, s.mtime]).map_err(err)?;
                changed = true;
            }
            Some((id, size, mtime)) if (*size, *mtime) != (s.size, s.mtime) => {
                tx.execute("UPDATE kb_docs SET size = ?1, mtime = ?2, fresh = 0 WHERE id = ?3", params![s.size, s.mtime, id]).map_err(err)?;
                changed = true;
            }
            Some(_) => {}
        }
    }
    let now: HashSet<&str> = seen.iter().map(|s| s.path.as_str()).collect();
    for (path, (id, _, _)) in &have {
        if !now.contains(path.as_str()) {
            tx.execute("DELETE FROM kb_docs WHERE id = ?1", [id]).map_err(err)?;
            changed = true;
        }
    }
    tx.commit()?;
    Ok(changed)
}

/// A web page is one doc; its size stands for its copy.
pub fn ensure_web(search: &Connection, source: i64, size: i64) -> Result<(), String> {
    apply_scan(search, source, &[Seen { path: String::new(), size, mtime: 0 }]).map(|_| ())
}

/// Drops what gone sources (and deleted web pages) left behind.
pub fn keep_sources(search: &Connection, keep: &HashSet<i64>) -> Result<(), String> {
    let have: Vec<i64> = {
        let mut st = search.prepare("SELECT DISTINCT source_id FROM kb_docs").map_err(err)?;
        let rows = st.query_map([], |r| r.get(0)).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)?
    };
    for s in have.into_iter().filter(|s| !keep.contains(s)) {
        search.execute("DELETE FROM kb_docs WHERE source_id = ?1", [s]).map_err(err)?;
    }
    Ok(())
}

/// 重新建立: every file is read and embedded again.
pub fn force(search: &Connection, source: i64) -> Result<(), String> {
    let tx = Tx::begin(search)?;
    tx.execute("DELETE FROM kb_chunks WHERE doc_id IN (SELECT id FROM kb_docs WHERE source_id = ?1)", [source]).map_err(err)?;
    tx.execute("UPDATE kb_docs SET fresh = 0, sha256 = NULL, model = NULL, error = NULL WHERE source_id = ?1", [source]).map_err(err)?;
    tx.commit()
}

/// A passage made for a doc.
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    pub segment: i64,
    pub label: String,
    pub content: String,
    /// int8 (vectors::to_blob).
    pub vector: Vec<u8>,
}

/// Passage vectors a doc has for `model`, by (label, text).
pub fn old_vectors(search: &Connection, doc: i64) -> Result<HashMap<(String, String), Vec<u8>>, String> {
    let mut st = search.prepare("SELECT label, content, vector FROM kb_chunks WHERE doc_id = ?1").map_err(err)?;
    let rows = st.query_map([doc], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?))).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Its new passages; current only if the file is still as it was when read (a scan may have seen it
/// change meanwhile).
pub fn save(search: &Connection, doc: &Doc, sha: &str, model: &str, made: &[Made]) -> Result<(), String> {
    let tx = Tx::begin(search)?;
    tx.execute("DELETE FROM kb_chunks WHERE doc_id = ?1", [doc.id]).map_err(err)?;
    {
        let mut st = tx.prepare("INSERT INTO kb_chunks (doc_id, ordinal, segment, label, content, vector) VALUES (?1, ?2, ?3, ?4, ?5, ?6)").map_err(err)?;
        for (i, m) in made.iter().enumerate() {
            st.execute(params![doc.id, i as i64, m.segment, m.label, m.content, m.vector]).map_err(err)?;
        }
    }
    tx.execute("UPDATE kb_docs SET sha256 = ?1, model = ?2, error = NULL, fresh = (size = ?3 AND mtime = ?4) WHERE id = ?5", params![sha, model, doc.size, doc.mtime, doc.id]).map_err(err)?;
    tx.commit()
}

/// The file couldn't be read: no passages until it changes.
pub fn fail(search: &Connection, doc: &Doc, error: &str) -> Result<(), String> {
    let tx = Tx::begin(search)?;
    tx.execute("DELETE FROM kb_chunks WHERE doc_id = ?1", [doc.id]).map_err(err)?;
    tx.execute("UPDATE kb_docs SET sha256 = NULL, model = NULL, error = ?1, fresh = (size = ?2 AND mtime = ?3) WHERE id = ?4", params![error, doc.size, doc.mtime, doc.id]).map_err(err)?;
    tx.commit()
}

/// Touched but the same bytes: its passages stay.
pub fn unchanged(search: &Connection, doc: &Doc) -> Result<(), String> {
    search.execute("UPDATE kb_docs SET fresh = (size = ?1 AND mtime = ?2) WHERE id = ?3", params![doc.size, doc.mtime, doc.id]).map(|_| ()).map_err(err)
}

// ---------- building ----------

/// What a doc reads as: its name, sha256 and text.
pub type Read = (String, String, Vec<Segment>);

/// A folder's file (or a web page's copy) as text. Err: it can't be read (kept as its error).
pub fn read_doc(picky: &Connection, src: &Source, doc: &Doc) -> Result<Read, String> {
    if src.kind == "web" {
        let id = src.artifact_id.ok_or("网页的副本不见了")?;
        files::usable(picky, id)?;
        let sha: String = picky.query_row("SELECT sha256 FROM artifacts WHERE id = ?1", [id], |r| r.get(0)).map_err(err)?;
        return Ok((src.name.clone(), sha, files::segments(picky, id)?));
    }
    let full = file_path(src, &doc.path)?;
    let p = files::read(&full, true)?;
    Ok((p.name, p.sha256, p.segments))
}

/// A file of a folder source, refusing anything that would leave the folder.
pub fn file_path(src: &Source, rel: &str) -> Result<PathBuf, String> {
    let root = src.path.as_deref().ok_or("这不是文件夹")?;
    let r = Path::new(rel);
    if rel.is_empty() || r.is_absolute() || r.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err("文件路径无效".into());
    }
    Ok(Path::new(root).join(r))
}

/// The passages of a doc's text, reusing `old` vectors (by label and text); Ok(Err) = the doc can't be
/// indexed (too long, a passage the service won't take); Err = the service failed.
pub async fn embed_doc(rt: &Runtime, model: &ServiceModel, name: &str, segs: &[Segment], old: &HashMap<(String, String), Vec<u8>>) -> Result<Result<Vec<Made>, String>, String> {
    let labels: HashMap<i64, String> = segs.iter().map(|s| (s.ordinal, s.label())).collect();
    let label = |p: &Piece| labels.get(&p.segment).cloned().unwrap_or_default();
    let mut list = index::pieces(segs);
    let too_long = || format!("太长了（超过 {} 段），没有建立索引", index::MAX_CHUNKS);
    if list.is_empty() {
        return Ok(Err("没有可以建立索引的文字".into()));
    }
    if list.len() > index::MAX_CHUNKS {
        return Ok(Err(too_long()));
    }
    let mut made: Vec<Made> = vec![];
    let mut start = 0;
    while start < list.len() {
        let reuse = |p: &Piece| old.get(&(label(p), p.content.clone())).cloned();
        if let Some(v) = reuse(&list[start]) {
            let p = &list[start];
            made.push(Made { segment: p.segment, label: label(p), content: p.content.clone(), vector: v });
            start += 1;
            continue;
        }
        let mut end = start;
        while end < list.len() && end - start < 16 && reuse(&list[end]).is_none() {
            end += 1;
        }
        let texts: Vec<String> = list[start..end].iter().map(|p| format!("{name} · {}\n{}", label(p), p.content)).collect();
        match embed::embed(rt, model, &texts, Input::Document).await {
            Ok(vs) => {
                for (p, v) in list[start..end].iter().zip(vs) {
                    made.push(Made { segment: p.segment, label: label(p), content: p.content.clone(), vector: vectors::to_blob(&vectors::quantize(&v)) });
                }
                start = end;
            }
            Err(EmbedError::TooLong) => {
                let halves: Vec<Piece> = list[start..end].iter().flat_map(|p| index::halve(&p.content).into_iter().map(move |c| Piece { segment: p.segment, content: c })).collect();
                if halves.len() <= end - start {
                    return Ok(Err("有一段文字没法建立索引".into()));
                }
                list.splice(start..end, halves);
                if list.len() > index::MAX_CHUNKS {
                    return Ok(Err(too_long()));
                }
            }
            Err(EmbedError::Failed(m)) => return Err(m),
        }
    }
    Ok(Ok(made))
}

/// Builds one pending doc. Err: the embedding service failed (the build stops).
async fn build_doc(app: &AppHandle, model: &ServiceModel, doc: &Doc, src: &Source) -> Result<(), String> {
    let st = app.state::<AppState>();
    let read = if src.kind == "web" {
        st.picky.with_db(|c| Ok(read_doc(c, src, doc)))?
    } else {
        let (s, d) = (src.clone(), doc.clone());
        tauri::async_runtime::spawn_blocking(move || {
            let full = file_path(&s, &d.path)?;
            files::read(&full, true).map(|p| (p.name, p.sha256, p.segments))
        })
        .await
        .map_err(|e| e.to_string())?
    };
    let (name, sha, segs) = match read {
        Ok(r) => r,
        Err(e) => return st.search.with_db(|c| fail(c, doc, &e)),
    };
    if doc.error.is_none() && doc.sha256.as_deref() == Some(sha.as_str()) && doc.model.as_deref() == Some(model.key.as_str()) {
        return st.search.with_db(|c| unchanged(c, doc));
    }
    let old = if doc.model.as_deref() == Some(model.key.as_str()) { st.search.with_db(|c| old_vectors(c, doc.id))? } else { HashMap::new() };
    match embed_doc(&st.picky.runtime, model, &name, &segs, &old).await? {
        Ok(made) => st.search.with_db(|c| save(c, doc, &sha, &model.key, &made)),
        Err(e) => st.search.with_db(|c| fail(c, doc, &e)),
    }
}

/// Builds the pending docs of `sources` until none is left. `start`: may start 8081; otherwise only
/// while it runs anyway. Err: the embedding service failed.
async fn drain(app: &AppHandle, sources: &HashSet<i64>, start: bool) -> Result<(), String> {
    let st = app.state::<AppState>();
    let mut model: Option<ServiceModel> = None;
    loop {
        let srcs: Vec<Source> = st.picky.with_db(|c| sources_of(c, sources))?;
        let ids: Vec<i64> = srcs.iter().map(|s| s.id).collect();
        let key = embed::model(&st.picky);
        if key.is_empty() {
            return Err("模型清单里没有向量模型".into());
        }
        let Some(doc) = st.search.with_db(|c| next_pending(c, &ids, &key))? else { return Ok(()) };
        let m = match &model {
            Some(m) if m.key == key => m.clone(),
            _ if start => embed::prepare(app).await?,
            _ if st.picky.runtime.service_status(Svc::Embedding).state == ServiceState::Running => st.picky.runtime.service_model(Svc::Embedding).ok_or("模型清单里没有向量模型")?,
            _ => return Ok(()), // waits until a question needs it
        };
        model = Some(m.clone());
        let Some(src) = srcs.iter().find(|s| s.id == doc.source_id) else { return Ok(()) };
        st.picky.kb.lock().current = Some(src.id);
        build_doc(app, &m, &doc, src).await?;
    }
}

fn sources_of(picky: &Connection, ids: &HashSet<i64>) -> Result<Vec<Source>, String> {
    Ok(sources(picky, None)?.into_iter().filter(|s| ids.contains(&s.id) && !s.deleted).collect())
}

// ---------- jobs ----------

#[derive(Default)]
struct Inner {
    /// Sources a build should go through, and whether it may start 8081.
    want: HashSet<i64>,
    start: bool,
    /// The source being built now.
    current: Option<i64>,
    /// From the last scan: folders that can't be found, and those with more than MAX_FILES files.
    missing: HashSet<i64>,
    truncated: HashSet<i64>,
    /// The last build stopped on this (the embedding service).
    error: Option<String>,
}

#[derive(Default)]
pub struct Jobs {
    inner: Mutex<Inner>,
    build: tokio::sync::Mutex<()>,
    scanning: AtomicBool,
}

impl Jobs {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn finished(&self, done: &HashSet<i64>, r: &Result<(), String>) {
        let mut g = self.lock();
        g.current = None;
        g.want.retain(|s| !done.contains(s));
        if g.want.is_empty() {
            g.start = false;
        }
        g.error = r.clone().err();
    }
}

/// Asks for `sources` to be built in the background.
pub fn request(app: &AppHandle, sources: &[i64], start: bool) {
    {
        let st = app.state::<AppState>();
        let mut g = st.picky.kb.lock();
        g.want.extend(sources);
        g.start |= start;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run(&app).await });
}

async fn run(app: &AppHandle) {
    let jobs = &app.state::<AppState>().picky.kb;
    loop {
        let Ok(guard) = jobs.build.try_lock() else { return }; // the running build takes the new wants
        let (want, start) = {
            let g = jobs.lock();
            (g.want.clone(), g.start)
        };
        if want.is_empty() {
            return;
        }
        let r = drain(app, &want, start).await;
        jobs.finished(&want, &r);
        drop(guard);
        if jobs.lock().want.is_empty() {
            return;
        }
    }
}

/// Before a question: builds what the scope still needs (starting 8081). While another build runs,
/// the question uses what is ready (and the build takes these sources too).
pub async fn prepare(app: &AppHandle, scope: &Scope) -> Result<(), String> {
    let ids = scope.ids();
    let jobs = &app.state::<AppState>().picky.kb;
    let Ok(guard) = jobs.build.try_lock() else {
        request(app, &ids.into_iter().collect::<Vec<_>>(), true);
        return Ok(());
    };
    let r = drain(app, &ids, true).await;
    jobs.finished(&ids, &r);
    drop(guard);
    if !jobs.lock().want.is_empty() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { run(&app).await });
    }
    r
}

/// Compares every folder with what search.db saw, then builds the changes if 8081 runs anyway.
pub async fn scan(app: &AppHandle) {
    let st = app.state::<AppState>();
    if st.picky.kb.scanning.swap(true, Ordering::SeqCst) {
        return;
    }
    let app2 = app.clone();
    let ids = tauri::async_runtime::spawn_blocking(move || scan_now(&app2)).await.ok().and_then(Result::ok);
    st.picky.kb.scanning.store(false, Ordering::SeqCst);
    if let Some(ids) = ids {
        if st.picky.runtime.service_status(Svc::Embedding).state == ServiceState::Running {
            request(app, &ids, false);
        }
    }
}

fn scan_now(app: &AppHandle) -> Result<Vec<i64>, String> {
    let st = app.state::<AppState>();
    let srcs = st.picky.with_db(|c| sources(c, None))?;
    let keep: HashSet<i64> = srcs.iter().filter(|s| !s.deleted).map(|s| s.id).collect();
    st.search.with_db(|c| keep_sources(c, &keep))?;
    let (mut missing, mut truncated) = (HashSet::new(), HashSet::new());
    for s in srcs.iter().filter(|s| !s.deleted) {
        match (&s.path, s.artifact_id) {
            (Some(p), _) => match walk(Path::new(p)) {
                Ok((seen, cut)) => {
                    st.search.with_db(|c| apply_scan(c, s.id, &seen))?;
                    if cut {
                        truncated.insert(s.id);
                    }
                }
                Err(_) => {
                    missing.insert(s.id);
                }
            },
            (None, Some(a)) => {
                let size: i64 = st.picky.with_db(|c| c.query_row("SELECT byte_count FROM artifacts WHERE id = ?1", [a], |r| r.get(0)).map_err(err))?;
                st.search.with_db(|c| ensure_web(c, s.id, size))?;
            }
            _ => {}
        }
    }
    let mut g = st.picky.kb.lock();
    (g.missing, g.truncated) = (missing, truncated);
    Ok(keep.into_iter().collect())
}

/// Scans, then builds one source (it was just added or asked to be rebuilt), starting 8081.
fn scan_and_build(app: &AppHandle, source: i64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        scan(&app).await;
        request(&app, &[source], true);
    });
}

// ---------- retrieval ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub source: i64,
    pub path: String,
    pub segment: i64,
    pub label: String,
    pub content: String,
    pub score: f32,
}

/// The `keep` passages of `sources` (made with `model`) most like `question`, scoring ≥ `min`.
pub fn similar(search: &Connection, sources: &[i64], model: &str, question: &[f32], keep: usize, min: f32) -> Result<Vec<Hit>, String> {
    if sources.is_empty() || keep == 0 {
        return Ok(vec![]);
    }
    let q = vectors::quantize(question);
    let mut st = search
        .prepare(&format!("SELECT d.source_id, d.path, k.segment, k.label, k.content, k.vector FROM kb_chunks k JOIN kb_docs d ON d.id = k.doc_id WHERE d.model = ?1 AND d.source_id IN ({}) ORDER BY d.id, k.ordinal", id_list(sources)))
        .map_err(err)?;
    let mut rows = st.query([model]).map_err(err)?;
    let mut hits: Vec<Hit> = vec![];
    while let Some(r) = rows.next().map_err(err)? {
        let blob: Vec<u8> = r.get(5).map_err(err)?;
        let Some(v) = vectors::from_blob(&blob).filter(|v| v.values.len() == q.values.len()) else { continue };
        let score = vectors::dot(&q.values, &v.values) as f32 * q.scale * v.scale;
        if score < min {
            continue;
        }
        hits.push(Hit { source: r.get(0).map_err(err)?, path: r.get(1).map_err(err)?, segment: r.get(2).map_err(err)?, label: r.get(3).map_err(err)?, content: r.get(4).map_err(err)?, score });
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    hits.truncate(keep);
    Ok(hits)
}

/// A hit as a passage of the question (labelled 「文件名 · 第 3 页」).
fn passage(h: &Hit, scope: &Scope) -> Passage {
    let (src, library) = scope.sources.iter().find(|s| s.0.id == h.source).map_or((None, String::new()), |(s, l)| (Some(s), l.clone()));
    let name = if h.path.is_empty() { src.map_or_else(|| "网页".into(), |s| s.name.clone()) } else { folder_name(&h.path) };
    let label = if h.label.is_empty() { name } else { format!("{name} · {}", h.label) };
    Passage {
        file_id: 0,
        artifact_id: src.and_then(|s| s.artifact_id).unwrap_or(0),
        segment_ordinal: h.segment,
        label,
        content: h.content.clone(),
        kb: Some(KbRef { source: h.source, path: h.path.clone(), library }),
    }
}

/// Passages for a question; an error means 8081 couldn't be used.
pub async fn retrieve(app: &AppHandle, question: &str, scope: &Scope) -> Result<Vec<Passage>, String> {
    if scope.sources.is_empty() {
        return Ok(vec![]);
    }
    prepare(app, scope).await?;
    let st = app.state::<AppState>();
    let model = embed::prepare(app).await?;
    let q = embed::embed(&st.picky.runtime, &model, &[question.to_string()], Input::PassageQuery).await.map_err(String::from)?.pop().ok_or("向量服务没有返回问题的向量。")?;
    let ids: Vec<i64> = scope.ids().into_iter().collect();
    let pool: Vec<Passage> = st.search.with_db(|c| similar(c, &ids, &model.key, &q, index::POOL, index::POOL_MIN))?.iter().map(|h| passage(h, scope)).collect();
    // v0.4: the rerank model picks from a wider pool; without it, similarity alone
    match super::rerank::pick(app, question, &pool, |p| format!("{}\n{}", p.label, p.content), index::LIMIT).await {
        Some(picked) => Ok(picked),
        None => Ok(st.search.with_db(|c| similar(c, &ids, &model.key, &q, index::LIMIT, index::MIN_SCORE))?.iter().map(|h| passage(h, scope)).collect()),
    }
}

/// 查知识库 (pomelo_kb): passages for the model's own query, numbered as sources.
pub async fn search_tool(app: &AppHandle, scope: &Scope, sources: &mut super::cite::Sources, arguments: &str, budget: i64) -> super::data::Answer {
    let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
    let q = args["query"].as_str().unwrap_or("").trim().to_string();
    let answer = |label: String, state: &'static str, text: String| super::data::Answer { module: None, label, state, text, records: vec![] };
    if q.is_empty() {
        return answer("查知识库 · 没有要找的".into(), "failed", "没查：query 不能为空。".into());
    }
    let short: String = q.chars().take(20).collect();
    let found = match retrieve(app, &q, scope).await {
        Ok(p) => p,
        Err(e) => return answer(format!("查知识库「{short}」· 没查成"), "failed", format!("没查成：{e}")),
    };
    let mut lines = vec![if found.is_empty() { format!("知识库（{}）里没找到和「{q}」有关的。", scope.libraries.join("、")) } else { format!("知识库（{}）里和「{q}」有关的：", scope.libraries.join("、")) }];
    let mut used = super::context::estimate(&lines[0]);
    for p in &found {
        let n = sources.knowledge(p);
        let line = format!("【[{n}] {}】\n{}", p.label, p.content);
        let cost = super::context::estimate(&line);
        if used + cost > budget {
            break;
        }
        used += cost;
        lines.push(line);
    }
    answer(format!("查知识库「{short}」· {} 段", lines.len() - 1), "ok", lines.join("\n\n"))
}

/// 查知识库 (`pomelo_kb`) by its `view`: search (default), files, or one file read whole (outline /
/// read / find).
pub async fn kb_tool(app: &AppHandle, scope: &Scope, sources: &mut super::cite::Sources, arguments: &str, budget: i64) -> super::data::Answer {
    let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
    match args["view"].as_str().unwrap_or("search") {
        "files" => app.state::<AppState>().search.with_db(|c| files_view(c, scope, &args, budget)).unwrap_or_else(|e| browse_failed("文件", &e)),
        "outline" | "read" | "find" => browse(app, scope, sources, &args, budget).await,
        "look" => browse_failed("看图", "只有云端模型能看图片（现在用的是本地模型，告诉用户换云端模型再问）"),
        _ => search_tool(app, scope, sources, arguments, budget).await,
    }
}

fn browse_failed(what: &str, why: &str) -> super::data::Answer {
    super::data::Answer { module: None, label: format!("知识库 · {what} · 没查成"), state: "failed", text: format!("没查成：{why}"), records: vec![] }
}

/// A file of the scope as 查知识库 lists it: 「文件夹名/相对路径」, a web page by its title.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub source: Source,
    pub library: String,
    pub path: String,
    pub shown: String,
    pub size: i64,
    /// Last modified (local date).
    pub date: String,
    pub error: Option<String>,
}

/// Every file the scope's sources hold (as last scanned), in order.
pub fn listed(search: &Connection, scope: &Scope) -> Result<Vec<Listed>, String> {
    let mut out = vec![];
    let mut st = search.prepare("SELECT path, size, date(mtime / 1000000000, 'unixepoch', 'localtime'), error FROM kb_docs WHERE source_id = ?1 ORDER BY path").map_err(err)?;
    for (src, library) in &scope.sources {
        let rows = st.query_map([src.id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?))).map_err(err)?;
        for row in rows {
            let (path, size, date, error) = row.map_err(err)?;
            let shown = if src.kind == "web" { src.name.clone() } else { format!("{}/{path}", src.name) };
            out.push(Listed { source: src.clone(), library: library.clone(), path, shown, size, date: date.unwrap_or_default(), error });
        }
    }
    Ok(out)
}

fn size_text(b: i64) -> String {
    if b >= 1 << 20 { format!("{:.1} MB", b as f64 / 1048576.0) } else { format!("{} KB", (b + 1023) / 1024) }
}

/// `view=files`: what the libraries hold, those with every word of `keyword` in their path.
pub fn files_view(search: &Connection, scope: &Scope, args: &serde_json::Value, budget: i64) -> Result<super::data::Answer, String> {
    let words: Vec<String> = args["keyword"].as_str().unwrap_or("").split_whitespace().map(str::to_lowercase).collect();
    let all = listed(search, scope)?;
    let hits: Vec<&Listed> = all.iter().filter(|l| words.iter().all(|w| l.shown.to_lowercase().contains(w.as_str()))).collect();
    let libs = scope.libraries.join("、");
    let head = if words.is_empty() { format!("知识库（{libs}）里一共 {} 个文件（路径用在 view=outline / read / find 的 file 里）：", all.len()) } else { format!("知识库（{libs}）里路径含「{}」的文件 {} 个：", words.join(" "), hits.len()) };
    let mut lines = vec![head];
    let mut used = super::context::estimate(&lines[0]);
    for (i, l) in hits.iter().enumerate() {
        let kind = if l.source.kind == "web" {
            "网页".to_string()
        } else if is_image(&l.path) {
            "图片，用 look 看".to_string()
        } else {
            Path::new(&l.path).extension().map_or("文件".into(), |e| e.to_string_lossy().to_uppercase())
        };
        let broken = l.error.as_deref().map(|e| format!(" · 读不了：{e}")).unwrap_or_default();
        let line = format!("- {}（{kind} · {} · {}{broken}）", l.shown, size_text(l.size), l.date);
        if used + super::context::estimate(&line) > budget {
            lines.push(format!("……还有 {} 个没列出，可以用 keyword 缩小", hits.len() - i));
            break;
        }
        used += super::context::estimate(&line);
        lines.push(line);
    }
    let label = if words.is_empty() { format!("知识库 · 文件（{} 个）", all.len()) } else { format!("知识库 · 找文件「{}」· {} 个", words.join(" "), hits.len()) };
    Ok(super::data::Answer { module: None, label, state: "ok", text: lines.join("\n"), records: vec![] })
}

/// The listed file `file` names: its listed path, its path in the folder, or its file name.
pub fn pick<'a>(all: &'a [Listed], file: &str) -> Result<&'a Listed, String> {
    let want = file.trim().trim_start_matches('/');
    if want.is_empty() {
        return Err("file 写文件的路径（view=files 列出的）".into());
    }
    if let Some(l) = all.iter().find(|l| l.shown == want || l.path == want) {
        return Ok(l);
    }
    let name = |l: &Listed| l.shown.rsplit('/').next().unwrap_or(&l.shown).to_string();
    let found: Vec<&Listed> = all.iter().filter(|l| name(l) == want).collect();
    let found = if found.is_empty() { all.iter().filter(|l| l.shown.contains(want)).collect() } else { found };
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("知识库里没有「{want}」（用 view=files 看有哪些）")),
        many => Err(format!("有 {} 个文件对得上，file 写完整的路径：{}", many.len(), many.iter().take(8).map(|l| l.shown.as_str()).collect::<Vec<_>>().join("、"))),
    }
}

/// The file's text, fresh: a folder's file from disk (whole tables), a web page from its copy.
pub fn whole(picky: &Connection, l: &Listed) -> Result<Read, String> {
    if let Some(e) = &l.error {
        return Err(format!("这个文件读不了：{e}"));
    }
    if l.source.kind == "web" {
        let id = l.source.artifact_id.ok_or("网页的副本不见了")?;
        files::usable(picky, id)?;
        return Ok((l.source.name.clone(), String::new(), files::segments(picky, id)?));
    }
    let p = files::read(&file_path(&l.source, &l.path)?, true)?;
    Ok((p.name, p.sha256, p.segments))
}

/// `view=outline` / `read` / `find` over a file read whole; what is read is numbered as 知识库 passages
/// (its citation opens the file).
pub fn whole_view(l: &Listed, read: Read, sources: &mut super::cite::Sources, args: &serde_json::Value, budget: i64) -> super::data::Answer {
    let (name, _, segs) = read;
    if segs.is_empty() {
        return browse_failed(&format!("《{name}》"), "没有可以读的文字");
    }
    let budget = budget.min(super::data::MAX_RESULT * 2);
    let kb = KbRef { source: l.source.id, path: l.path.clone(), library: l.library.clone() };
    let artifact = if l.source.kind == "web" { l.source.artifact_id.unwrap_or(0) } else { 0 };
    let mut cite = |s: &Segment, label: &str, content: &str| sources.knowledge(&Passage { file_id: 0, artifact_id: artifact, segment_ordinal: s.ordinal, label: label.into(), content: content.into(), kb: Some(kb.clone()) });
    use super::skills::files as views;
    match args["view"].as_str().unwrap_or("outline") {
        "read" => views::read_view("知识库", &name, &segs, args, budget, &mut cite),
        "find" => views::find_view("知识库", &[(name.as_str(), segs.as_slice())], args, budget, &mut |_, s, label, snippet| cite(s, label, snippet)),
        _ => {
            let mut text = views::outline(&name, &segs, 40);
            if super::context::estimate(&text) > budget {
                text = format!("{}\n……（目录太长，后面没给）", text.chars().take((budget * 2) as usize).collect::<String>());
            }
            super::data::Answer { module: None, label: format!("知识库 · 《{name}》的目录"), state: "ok", text, records: vec![] }
        }
    }
}

/// One file of the scope read whole (a folder's file is read off the main thread).
async fn browse(app: &AppHandle, scope: &Scope, sources: &mut super::cite::Sources, args: &serde_json::Value, budget: i64) -> super::data::Answer {
    let st = app.state::<AppState>();
    let file = args["file"].as_str().unwrap_or("").to_string();
    let l = match st.search.with_db(|c| listed(c, scope)).and_then(|all| pick(&all, &file).cloned()) {
        Ok(l) => l,
        Err(e) => return browse_failed(&format!("「{}」", file.chars().take(30).collect::<String>()), &e),
    };
    if l.source.kind == "folder" && is_image(&l.path) {
        return browse_failed(&format!("《{}》", l.shown), "这是图片，没有文字：用 view=look 看");
    }
    let read = if l.source.kind == "web" {
        st.picky.with_db(|c| Ok(whole(c, &l)))
    } else {
        let one = l.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let p = files::read(&file_path(&one.source, &one.path)?, true)?;
            Ok::<Read, String>((p.name, p.sha256, p.segments))
        })
        .await
        .map_err(|e| e.to_string())
    };
    match read.and_then(|r| r) {
        Ok(r) => whole_view(&l, r, sources, args, budget),
        Err(e) => browse_failed(&format!("《{}》", l.shown), &e),
    }
}

/// `view=look`: one image of the scope, shrunk for the model (sent after the tool results, chat.rs),
/// numbered as a 知识库 source (its citation opens the image).
pub async fn look(app: &AppHandle, scope: &Scope, sources: &mut super::cite::Sources, args: &serde_json::Value) -> (super::data::Answer, Option<super::provider::Image>) {
    let st = app.state::<AppState>();
    let file = args["file"].as_str().unwrap_or("").to_string();
    let l = match st.search.with_db(|c| listed(c, scope)).and_then(|all| pick(&all, &file).cloned()) {
        Ok(l) => l,
        Err(e) => return (browse_failed(&format!("看图「{}」", file.chars().take(30).collect::<String>()), &e), None),
    };
    if l.source.kind != "folder" || !is_image(&l.path) {
        return (browse_failed(&format!("《{}》", l.shown), "这不是图片：文字用 outline / read 读"), None);
    }
    let one = l.clone();
    let tmp = std::env::temp_dir().join("pomelo-picky-images");
    let shrunk = tauri::async_runtime::spawn_blocking(move || {
        let path = file_path(&one.source, &one.path)?;
        let ext = one.path.rsplit_once('.').map(|(_, x)| x.to_lowercase()).unwrap_or_default();
        let media = super::office::image_type(&ext).ok_or("不认识这种图片")?;
        if !path.is_file() {
            return Err("找不到这张图片，可能已经移走或删除了".to_string());
        }
        super::office::image_for_model(&path, media, &tmp)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    match shrunk {
        Ok((media_type, data)) => {
            let kb = KbRef { source: l.source.id, path: l.path.clone(), library: l.library.clone() };
            let n = sources.knowledge(&Passage { file_id: 0, artifact_id: 0, segment_ordinal: 0, label: format!("{} · 图片", l.shown), content: String::new(), kb: Some(kb) });
            let text = format!("[{n}] 图片《{}》（知识库 · {}）附在下面那条消息里：看了再回答，用到时标 [{n}]。", l.shown, l.library);
            (super::data::Answer { module: None, label: format!("知识库 · 看图《{}》", l.shown), state: "ok", text, records: vec![] }, Some(super::provider::Image { media_type, data }))
        }
        Err(e) => (browse_failed(&format!("《{}》", l.shown), &e), None),
    }
}

/// What the composer's menu says about the scope.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KbPlan {
    pub libraries: Vec<String>,
    /// Files with passages (for the current embedding model) and files still to (re)build.
    pub ready: i64,
    pub pending: i64,
    /// Sources not sent to a cloud model.
    pub skipped: Vec<String>,
}

pub fn plan(search: &Connection, scope: &Scope, model: &str) -> Result<KbPlan, String> {
    let ids: Vec<i64> = scope.ids().into_iter().collect();
    let (ready, pending) = if ids.is_empty() {
        (0, 0)
    } else {
        search
            .query_row(
                &format!("SELECT coalesce(sum(model = ?1 AND error IS NULL), 0), coalesce(sum({PENDING}), 0) FROM kb_docs WHERE source_id IN ({})", id_list(&ids)),
                [model],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(err)?
    };
    Ok(KbPlan { libraries: scope.libraries.clone(), ready, pending, skipped: scope.skipped.clone() })
}

// ---------- the manager ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failed {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    #[serde(flatten)]
    pub source: Source,
    /// Files it holds (images apart); with passages; still to (re)build; that can't be read.
    pub files: i64,
    pub images: i64,
    pub ready: i64,
    pub pending: i64,
    pub failed: Vec<Failed>,
    pub building: bool,
    /// The folder can't be found / it has more than MAX_FILES files (the last scan).
    pub missing: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    pub id: i64,
    pub name: String,
    pub sources: Vec<SourceView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub libraries: Vec<LibraryView>,
    /// The last build stopped on this (the embedding service).
    pub error: Option<String>,
}

pub fn source_view(search: &Connection, s: Source, model: &str) -> Result<SourceView, String> {
    let (images, list): (Vec<Doc>, Vec<Doc>) = docs(search, s.id)?.into_iter().partition(|d| is_image(&d.path));
    let ready = list.iter().filter(|d| d.error.is_none() && d.model.as_deref() == Some(model)).count() as i64;
    let pending = list.iter().filter(|d| !d.fresh || (d.error.is_none() && d.model.as_deref() != Some(model))).count() as i64;
    let failed = list.iter().filter_map(|d| d.error.clone().map(|error| Failed { path: d.path.clone(), error })).collect();
    Ok(SourceView { files: list.len() as i64, images: images.len() as i64, ready, pending, failed, building: false, missing: false, truncated: false, source: s })
}

fn overview(app: &AppHandle) -> Result<Overview, String> {
    let st = app.state::<AppState>();
    let model = embed::model(&st.picky);
    let (libs, srcs) = st.picky.with_db(|c| Ok((libraries(c)?, sources(c, None)?)))?;
    let g = st.picky.kb.lock();
    let (current, missing, truncated, error) = (g.current, g.missing.clone(), g.truncated.clone(), g.error.clone());
    drop(g);
    let mut out = vec![];
    for l in libs {
        let mut views = vec![];
        for s in srcs.iter().filter(|s| s.library_id == l.id) {
            let mut v = st.search.with_db(|c| source_view(c, s.clone(), &model))?;
            v.building = current == Some(s.id);
            v.missing = missing.contains(&s.id) || s.deleted;
            v.truncated = truncated.contains(&s.id);
            views.push(v);
        }
        out.push(LibraryView { id: l.id, name: l.name, sources: views });
    }
    Ok(Overview { libraries: out, error })
}

// ---------- commands ----------

#[tauri::command]
pub fn picky_kb_overview(app: AppHandle) -> Result<Overview, String> {
    overview(&app)
}

#[tauri::command]
pub fn picky_kb_library_create(state: State<AppState>, name: String) -> Result<i64, String> {
    state.with_picky_undo("新建知识库", |c| create_library(c, &name))
}

#[tauri::command]
pub fn picky_kb_library_rename(state: State<AppState>, id: i64, name: String) -> Result<(), String> {
    state.with_picky_undo("知识库改名", |c| rename_library(c, id, &name))
}

#[tauri::command]
pub fn picky_kb_library_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_picky_undo("删除知识库", |c| delete_library(c, id))
}

#[tauri::command]
pub fn picky_kb_libraries_reorder(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    state.with_picky_undo("调整知识库的顺序", |c| reorder_libraries(c, &ids))
}

#[tauri::command]
pub fn picky_kb_folder_add(app: AppHandle, library_id: i64, path: String) -> Result<i64, String> {
    let st = app.state::<AppState>();
    let data = st.paths.root.clone();
    let id = st.with_picky_undo("添加文件夹", |c| add_folder(c, library_id, Path::new(&path), &data))?;
    scan_and_build(&app, id);
    Ok(id)
}

/// After picky_import_web saved the page.
#[tauri::command]
pub fn picky_kb_web_add(app: AppHandle, library_id: i64, artifact_id: i64) -> Result<i64, String> {
    let id = app.state::<AppState>().with_picky_undo("添加网页", |c| add_web(c, library_id, artifact_id))?;
    scan_and_build(&app, id);
    Ok(id)
}

#[tauri::command]
pub fn picky_kb_source_remove(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_picky_undo("从知识库移除", |c| remove_source(c, id))
}

#[tauri::command]
pub fn picky_kb_source_cloud(state: State<AppState>, id: i64, on: bool) -> Result<(), String> {
    state.with_picky_undo(if on { "知识库发给云端" } else { "知识库不发给云端" }, |c| set_cloud(c, id, on))
}

/// 重新建立 (starts 8081).
#[tauri::command]
pub fn picky_kb_source_rebuild(app: AppHandle, id: i64) -> Result<(), String> {
    let st = app.state::<AppState>();
    st.picky.with_db(|c| source(c, id))?;
    st.search.with_db(|c| force(c, id))?;
    scan_and_build(&app, id);
    Ok(())
}

/// Opening 小拾 (and the monitor every 10 minutes).
#[tauri::command]
pub fn picky_kb_scan(app: AppHandle) {
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move { scan(&app2).await });
}

/// Opens a folder's file (or the folder: `path` empty), or a web page in the browser.
#[tauri::command]
pub fn picky_kb_open(app: AppHandle, source_id: i64, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let src = app.state::<AppState>().picky.with_db(|c| source(c, source_id))?;
    if let Some(url) = src.url.as_deref().filter(|_| src.kind == "web") {
        return app.opener().open_url(url, None::<&str>).map_err(|e| format!("打不开这个网页：{e}"));
    }
    let target = if path.is_empty() { PathBuf::from(src.path.clone().unwrap_or_default()) } else { file_path(&src, &path)? };
    if !target.exists() {
        return Err(if path.is_empty() { "找不到这个文件夹".into() } else { "找不到这个文件，可能已经移走或删除了".into() });
    }
    app.opener().open_path(target.display().to_string(), None::<&str>).map_err(|e| format!("打不开：{e}"))
}

#[tauri::command]
pub fn picky_conversation_knowledge(state: State<AppState>, session_id: i64, on: bool) -> Result<(), String> {
    state.with_picky_undo(if on { "本会话查知识库" } else { "本会话不查知识库" }, |c| set_knowledge(c, session_id, on))
}

/// `library_ids` empty: follow the project (or all).
#[tauri::command]
pub fn picky_conversation_libraries(state: State<AppState>, session_id: i64, library_ids: Vec<i64>) -> Result<(), String> {
    state.with_picky_undo("选择知识库", |c| {
        set_knowledge(c, session_id, true)?;
        set_chosen(c, Owner::Session(session_id), &library_ids)
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::picky::files::tests::add;
    use crate::picky::runtime::LlmPaths;
    use crate::picky::store::tests::memory_db;
    use crate::picky::{library, Mode};

    fn mock_rt() -> (Runtime, ServiceModel) {
        let rt = Runtime::new(Mode::Mock, LlmPaths::new(Path::new("/nonexistent-home")));
        let m = rt.service_model(Svc::Embedding).unwrap();
        (rt, m)
    }

    /// Builds every pending doc of `source` the way `drain` does; (passages embedded, passages whose
    /// vector was kept).
    pub async fn build_source(picky: &Connection, search: &Connection, source: i64, rt: &Runtime, model: &ServiceModel) -> (usize, usize) {
        let src = super::source(picky, source).unwrap();
        let (mut embedded, mut kept) = (0, 0);
        while let Some(doc) = next_pending(search, &[source], &model.key).unwrap() {
            match read_doc(picky, &src, &doc) {
                Err(e) => fail(search, &doc, &e).unwrap(),
                Ok((name, sha, segs)) => {
                    if doc.error.is_none() && doc.sha256.as_deref() == Some(sha.as_str()) && doc.model.as_deref() == Some(model.key.as_str()) {
                        unchanged(search, &doc).unwrap();
                        continue;
                    }
                    let old = if doc.model.as_deref() == Some(model.key.as_str()) { old_vectors(search, doc.id).unwrap() } else { HashMap::new() };
                    match embed_doc(rt, model, &name, &segs, &old).await.unwrap() {
                        Ok(made) => {
                            let reused = made.iter().filter(|m| old.contains_key(&(m.label.clone(), m.content.clone()))).count();
                            (embedded, kept) = (embedded + made.len() - reused, kept + reused);
                            save(search, &doc, &sha, &model.key, &made).unwrap()
                        }
                        Err(e) => fail(search, &doc, &e).unwrap(),
                    }
                }
            }
        }
        (embedded, kept)
    }

    /// With the mock model.
    pub fn build_all(picky: &Connection, search: &Connection, source: i64) {
        let (rt, model) = mock_rt();
        tauri::async_runtime::block_on(build_source(picky, search, source, &rt, &model));
    }

    fn scan_one(picky: &Connection, search: &Connection, source: i64) -> bool {
        let s = super::source(picky, source).unwrap();
        let (seen, _) = walk(Path::new(s.path.as_deref().unwrap())).unwrap();
        apply_scan(search, source, &seen).unwrap()
    }

    #[test]
    fn libraries_sources_and_choices() {
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("kb-sources");
        let (a, b) = (dir.join("工作"), dir.join("工作/子文件夹"));
        std::fs::create_dir_all(&b).unwrap();
        let data = dir.join("data");
        std::fs::create_dir_all(&data).unwrap();
        assert!(create_library(&c, " ").is_err());
        let work = create_library(&c, "工作").unwrap();
        assert!(create_library(&c, "工作").unwrap_err().contains("已经有"));
        let life = create_library(&c, "生活").unwrap();
        assert_eq!(libraries(&c).unwrap().iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["工作", "生活"]);
        reorder_libraries(&c, &[life, work]).unwrap();
        assert_eq!(libraries(&c).unwrap()[0].id, life);
        assert!(reorder_libraries(&c, &[life]).is_err());
        rename_library(&c, life, "日常").unwrap();

        let f = add_folder(&c, work, &a, &data).unwrap();
        assert!(add_folder(&c, work, &a, &data).unwrap_err().contains("已经有"));
        assert!(add_folder(&c, work, &b, &data).unwrap_err().contains("重叠"));
        assert!(add_folder(&c, life, &b, &data).is_ok()); // another library may
        assert!(add_folder(&c, work, &data, &data).unwrap_err().contains("Pomelo 自己的数据"));
        assert!(add_folder(&c, work, &dir.join("没有"), &data).is_err());
        let s = source(&c, f).unwrap();
        assert_eq!((s.kind.as_str(), s.name.as_str(), s.cloud), ("folder", "工作", true));
        set_cloud(&c, f, false).unwrap();
        // a web page: only a saved page
        let note = add(&c, &dir, "笔记.md", b"# x\ny");
        assert!(add_web(&c, work, note).unwrap_err().contains("只能添加网页"));
        let page = files::store(&c, &dir.join("files"), &files::prepare_web("https://example.com/a", "租房指南", "# 租房\n押一付三")).unwrap();
        let w = add_web(&c, work, page).unwrap();
        assert!(add_web(&c, work, page).is_err());
        assert_eq!(source(&c, w).unwrap().name, "租房指南");
        // the attachment library leaves a library's pages out
        assert!(!files::library(&c).unwrap().iter().any(|x| x.id == page));

        // what a question searches: own > project > all; cloud leaves out what isn't allowed
        let p = library::create_project(&c, "P", "").unwrap();
        let sess = crate::picky::chat::begin_run(&c, None, Some(p), "问题", "local:x", "X").unwrap().session_id;
        assert_eq!(scope(&c, Some(sess), None, None, false).unwrap(), None); // switch off
        set_knowledge(&c, sess, true).unwrap();
        let all = scope(&c, Some(sess), None, None, false).unwrap().unwrap();
        assert_eq!(all.libraries, ["日常", "工作"]);
        set_chosen(&c, Owner::Project(p), &[work]).unwrap();
        let proj = scope(&c, Some(sess), None, None, true).unwrap().unwrap();
        assert_eq!((proj.libraries.clone(), proj.sources.len(), proj.skipped.clone()), (vec!["工作".to_string()], 1, vec!["工作".to_string()]));
        set_chosen(&c, Owner::Session(sess), &[life]).unwrap();
        assert_eq!(scope(&c, Some(sess), None, None, false).unwrap().unwrap().libraries, ["日常"]);
        let d = Draft { on: true, libraries: vec![] };
        assert_eq!(scope(&c, None, Some(p), Some(&d), false).unwrap().unwrap().libraries, ["工作"]);
        // deleting a library takes its sources and choices
        delete_library(&c, life).unwrap();
        assert!(chosen(&c, Owner::Session(sess)).unwrap().is_empty());
        assert_eq!(scope(&c, Some(sess), None, None, false).unwrap().unwrap().libraries, ["工作"]);
        remove_source(&c, w).unwrap();
        assert_eq!(sources(&c, None).unwrap().len(), 1);
    }

    #[test]
    fn scans_find_new_changed_and_gone_files() {
        let dir = crate::db::tests::temp_dir("kb-walk");
        let root = dir.join("库");
        std::fs::create_dir_all(root.join("子/更深")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("x.app")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        for (p, t) in [("a.md", "A"), ("子/b.txt", "B"), ("子/更深/c.csv", "x,y\n1,2"), (".隐藏.md", "h"), ("~$临时.docx", "t"), ("图.png", "i"), ("空.txt", ""), (".git/d.md", "g"), ("x.app/e.md", "e"), ("node_modules/f.md", "f")] {
            std::fs::write(root.join(p), t).unwrap();
        }
        std::os::unix::fs::symlink(root.join("a.md"), root.join("链接.md")).unwrap();
        let (seen, cut) = walk(&root).unwrap();
        assert_eq!(seen.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), ["a.md", "图.png", "子/b.txt", "子/更深/c.csv"]);
        assert!(!cut);
        assert!(walk(&dir.join("没有")).is_err());

        let s = crate::search::store::tests::memory_db();
        assert!(apply_scan(&s, 1, &seen).unwrap());
        assert!(!apply_scan(&s, 1, &seen).unwrap());
        assert_eq!(next_pending(&s, &[1], "m").unwrap().unwrap().path, "a.md");
        // an image is listed, never built
        assert!(is_image("图.png") && is_image("x/Y.JPG") && !is_image("a.md"));
        apply_scan(&s, 2, &seen[1..2]).unwrap();
        assert!(next_pending(&s, &[2], "m").unwrap().is_none());
        let src2 = Source { id: 2, library_id: 1, kind: "folder".into(), name: "图库".into(), path: Some(root.display().to_string()), url: None, artifact_id: None, deleted: false, cloud: true };
        let v = source_view(&s, src2, "m").unwrap();
        assert_eq!((v.files, v.images, v.pending), (0, 1, 0));
        let mut later = seen.clone();
        later[0].mtime += 1;
        later.remove(2);
        assert!(apply_scan(&s, 1, &later).unwrap());
        assert_eq!(docs(&s, 1).unwrap().iter().map(|d| d.path.as_str()).collect::<Vec<_>>(), ["a.md", "图.png", "子/更深/c.csv"]);
        keep_sources(&s, &HashSet::new()).unwrap();
        assert!(docs(&s, 1).unwrap().is_empty());
        // paths never leave the folder
        let src = Source { id: 1, library_id: 1, kind: "folder".into(), name: "库".into(), path: Some(root.display().to_string()), url: None, artifact_id: None, deleted: false, cloud: true };
        assert!(file_path(&src, "../x.md").is_err() && file_path(&src, "/etc/hosts").is_err() && file_path(&src, "").is_err());
        assert_eq!(file_path(&src, "子/b.txt").unwrap(), root.join("子/b.txt"));
    }

    /// 查知识库 lists what the libraries hold and reads one file whole, citing what it read.
    #[test]
    fn browsing_and_reading_a_whole_file() {
        let c = memory_db();
        let s = crate::search::store::tests::memory_db();
        let dir = crate::db::tests::temp_dir("kb-browse");
        let root = dir.join("租房");
        std::fs::create_dir_all(root.join("合同")).unwrap();
        let body: String = (1..=5).map(|i| format!("# 第{i}条\n{}\n\n", if i == 4 { "押金在退房后七天内退还。" } else { "其他约定。" })).collect();
        std::fs::write(root.join("合同/租房合同.md"), body).unwrap();
        std::fs::write(root.join("清单.txt"), "钥匙两把").unwrap();
        let data = dir.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let lib = create_library(&c, "生活").unwrap();
        let src = add_folder(&c, lib, &root, &data).unwrap();
        scan_one(&c, &s, src);
        let scope = Scope { libraries: vec!["生活".into()], sources: vec![(source(&c, src).unwrap(), "生活".into())], skipped: vec![] };
        // the files
        let a = files_view(&s, &scope, &serde_json::json!({ "view": "files" }), 4000).unwrap();
        assert!(a.text.contains("一共 2 个文件") && a.text.contains("- 租房/合同/租房合同.md（MD · 1 KB") && a.text.contains("- 租房/清单.txt（TXT"), "{}", a.text);
        let a = files_view(&s, &scope, &serde_json::json!({ "view": "files", "keyword": "合同" }), 4000).unwrap();
        assert!(a.label.contains("找文件「合同」· 1 个") && !a.text.contains("清单"), "{}", a.text);
        // a file by its path or its name
        let all = listed(&s, &scope).unwrap();
        assert_eq!(pick(&all, "租房合同.md").unwrap().path, "合同/租房合同.md");
        assert!(pick(&all, "没有.md").unwrap_err().contains("view=files"));
        let l = pick(&all, "租房/合同/租房合同.md").unwrap();
        // outline, read (cited as 知识库 passages: the citation opens the file), find
        let mut sources = crate::picky::cite::Sources::default();
        let o = whole_view(l, whole(&c, l).unwrap(), &mut sources, &serde_json::json!({ "view": "outline" }), 4000);
        assert!(o.text.contains("第 4 段 · 第4条：押金在退房后七天内退还"), "{}", o.text);
        let r = whole_view(l, whole(&c, l).unwrap(), &mut sources, &serde_json::json!({ "view": "read", "from": 4 }), 4000);
        assert!(r.text.starts_with("【[1] 租房合同.md · 第4条】") && r.label == "知识库 · 读《租房合同.md》第 4 段", "{r:?}");
        assert_eq!((sources.0[0].kb_source, sources.0[0].kb_path.as_deref()), (Some(src), Some("合同/租房合同.md")));
        let f = whole_view(l, whole(&c, l).unwrap(), &mut sources, &serde_json::json!({ "view": "find", "keyword": "押金" }), 4000);
        assert!(f.text.contains("找到 1 段") && f.label.contains("在《租房合同.md》里找「押金」"), "{f:?}");
    }

    #[test]
    fn building_keeps_unchanged_passages_and_retrieval_ranks() {
        let c = memory_db();
        let s = crate::search::store::tests::memory_db();
        let dir = crate::db::tests::temp_dir("kb-build");
        let root = dir.join("资料");
        std::fs::create_dir_all(&root).unwrap();
        let data = dir.join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(root.join("租房.md"), "# 押金\n租房合同的押金怎么退。\n\n# 周末\n周末去公园散步。").unwrap();
        std::fs::write(root.join("坏.pdf"), b"not a pdf").unwrap();
        let lib = create_library(&c, "生活").unwrap();
        let f = add_folder(&c, lib, &root, &data).unwrap();
        scan_one(&c, &s, f);
        build_all(&c, &s, f);
        let list = docs(&s, f).unwrap();
        let model = mock_rt().1.key;
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|d| d.path == "坏.pdf" && d.error.is_some() && d.fresh));
        let good = list.iter().find(|d| d.path == "租房.md").unwrap().clone();
        assert!(good.fresh && good.model.as_deref() == Some(model.as_str()));
        assert!(next_pending(&s, &[f], &model).unwrap().is_none());
        let v = source_view(&s, source(&c, f).unwrap(), &model).unwrap();
        assert_eq!((v.files, v.ready, v.pending, v.failed.len()), (2, 1, 0, 1));

        // retrieval: the passage about deposits ranks first, labelled with its file and section
        let sc = Scope { libraries: vec!["生活".into()], sources: vec![(source(&c, f).unwrap(), "生活".into())], skipped: vec![] };
        let hits = similar(&s, &[f], &model, &embed::mock_vector("押金怎么退"), 6, 0.1).unwrap();
        assert_eq!(hits[0].label, "押金");
        let p = passage(&hits[0], &sc);
        assert_eq!((p.label.as_str(), p.kb.as_ref().unwrap().path.as_str(), p.kb.as_ref().unwrap().library.as_str()), ("租房.md · 押金", "租房.md", "生活"));
        assert!(similar(&s, &[f], "another-model", &embed::mock_vector("押金"), 6, 0.1).unwrap().is_empty());

        // touched without new bytes: only marked current; an edit keeps the unchanged passage's vector
        let first: Vec<u8> = s.query_row("SELECT vector FROM kb_chunks WHERE doc_id = ?1 AND label = '周末'", [good.id], |r| r.get(0)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(root.join("租房.md"), "# 押金\n租房合同的押金怎么退。\n\n# 周末\n周末去公园散步。").unwrap();
        assert!(scan_one(&c, &s, f));
        assert_eq!(next_pending(&s, &[f], &model).unwrap().unwrap().path, "租房.md");
        build_all(&c, &s, f);
        assert!(next_pending(&s, &[f], &model).unwrap().is_none());
        std::fs::write(root.join("租房.md"), "# 押金\n押金要在退房时结清。\n\n# 周末\n周末去公园散步。").unwrap();
        scan_one(&c, &s, f);
        build_all(&c, &s, f);
        let again: Vec<u8> = s.query_row("SELECT vector FROM kb_chunks WHERE doc_id = ?1 AND label = '周末'", [good.id], |r| r.get(0)).unwrap();
        assert_eq!(first, again);
        assert!(similar(&s, &[f], &model, &embed::mock_vector("退房时结清"), 1, 0.1).unwrap()[0].content.contains("退房"));

        // 重新建立 drops the passages until built again
        force(&s, f).unwrap();
        assert!(similar(&s, &[f], &model, &embed::mock_vector("押金"), 6, 0.0).unwrap().is_empty());
        build_all(&c, &s, f);
        assert!(!similar(&s, &[f], &model, &embed::mock_vector("押金"), 6, 0.1).unwrap().is_empty());

        // a web page is one doc read from its copy
        let page = files::store(&c, &dir.join("files"), &files::prepare_web("https://example.com/b", "租房指南", "# 合同\n合同要写清押金。")).unwrap();
        let w = add_web(&c, lib, page).unwrap();
        ensure_web(&s, w, 10).unwrap();
        build_all(&c, &s, w);
        let hits = similar(&s, &[w], &model, &embed::mock_vector("合同押金"), 6, 0.1).unwrap();
        assert_eq!((hits[0].path.as_str(), hits[0].label.as_str()), ("", "合同"));
        let sc = Scope { libraries: vec!["生活".into()], sources: vec![(source(&c, w).unwrap(), "生活".into())], skipped: vec![] };
        let p = passage(&hits[0], &sc);
        assert_eq!((p.label.as_str(), p.artifact_id), ("租房指南 · 合同", page));
        let kp = plan(&s, &sc, &model).unwrap();
        assert_eq!((kp.ready, kp.pending), (1, 0));
    }

    /// Against the real 8081 / 8082 services (both stopped before; started here and stopped at the end), on
    /// a made-up folder in a temporary directory: build, ask, change a file, ask again.
    ///   POMELO_REAL_LLM=1 cargo test real_knowledge -- --ignored --nocapture   (POMELO_ASK='问题1|问题2')
    #[test]
    #[ignore]
    fn real_knowledge() {
        if std::env::var("POMELO_REAL_LLM").as_deref() != Ok("1") {
            return;
        }
        use crate::picky::rerank;
        use crate::picky::runtime::LlmPaths;
        let home = std::env::var("HOME").unwrap();
        let rt = Runtime::new(Mode::Real, LlmPaths::new(Path::new(&home)));
        let dir = crate::db::tests::temp_dir("real-kb");
        let root = dir.join("我的资料");
        for d in ["租房", "工作", "读书", "旅行", "English"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let write = |p: &str, t: &str| std::fs::write(root.join(p), t).unwrap();
        write("租房/看房记录.md", "# 看房记录\n\n## 10 月 5 日 · 朝阳区两居\n\n朝南，采光很好，离地铁 600 米，通勤 35 分钟。月租 6800，押一付三。中介说可以谈到 6500。\n\n## 10 月 12 日 · 海淀一居\n\n朝北，有点暗，但小区安静，楼下有菜市场。月租 5200，押二付一。房东自己住在隔壁楼。\n\n## 结论\n\n倾向朝阳那套，周末再去看一次晚上的噪音。");
        let txt = dir.join("合同.txt");
        std::fs::write(&txt, "租房合同要点\n押金在退房验收后 7 个工作日内无息退还，扣除欠费和损坏赔偿。\n房屋自然损耗和家电故障由房东负责维修，人为损坏由租客承担。\n租期一年，提前退租需提前 30 天书面通知，并支付一个月租金作为违约金。\n水电燃气在入住当天抄表，物业费由房东承担。").unwrap();
        assert!(std::process::Command::new("/usr/bin/textutil").args(["-convert", "docx", "-output"]).arg(root.join("租房/合同要点.docx")).arg(&txt).status().unwrap().success());
        write("工作/周报-第41周.md", "# 第 41 周周报\n\n## 完成\n\n- Pomelo 知识库的方案定了：库里放文件夹和网页，文件夹原地读，只重算变了的文件。\n- 后台每 10 分钟扫一遍修改时间。\n\n## 下周\n\n- 做助手的框架和翻译。\n- 准备季度汇报的幻灯片。");
        write("工作/面试准备.txt", "面试准备清单\n1. 用 STAR 法准备三个项目故事：情境、任务、行动、结果。\n2. 想好为什么离开上一家公司，说成长而不是抱怨。\n3. 反问环节问团队的工作方式和三个月内的期望。\n4. 带两份打印的简历，提前 15 分钟到。");
        write("读书/读书笔记.md", "# 读书笔记\n\n## 《百年孤独》\n\n布恩迪亚家族七代人在马孔多的兴衰，魔幻现实主义。最喜欢的是开头冰块那一段。\n\n## 《三体》\n\n黑暗森林法则：宇宙就是一座黑暗森林，每个文明都是带枪的猎人。");
        let mut csv = String::from("日期,地点,项目,金额\n");
        for i in 1..=260 {
            let (place, item) = match i {
                137 => ("伏见稻荷大社", "千本鸟居，门票免费，早上七点去人少"),
                201 => ("岚山", "竹林小径和小火车，小火车票 880 日元"),
                _ => ("便利店", "饮料和饭团"),
            };
            csv.push_str(&format!("2026-11-{:02},{place},{item},{}\n", 1 + i % 7, 300 + i));
        }
        write("旅行/京都消费.csv", &csv);
        write("English/deploy.md", "# Deployment checklist\n\nBefore deploying to production: run the migrations on a copy of the database, check the health endpoint, and keep the previous build for a quick rollback.");
        std::fs::write(root.join("English/leave-policy.pdf"), crate::picky::pdf::tests::sample_pdf(&["Annual leave: every employee gets 15 days per year.", "Unused days can be carried over until March 31."])).unwrap();
        std::fs::write(root.join("扫描件.pdf"), b"%PDF-1.4 not really a pdf").unwrap();
        std::fs::write(root.join("照片.png"), b"png").unwrap();

        let c = memory_db();
        let s = crate::search::store::tests::memory_db();
        let lib = create_library(&c, "我的资料").unwrap();
        let src = add_folder(&c, lib, &root, &dir.join("data")).unwrap();
        let scope = Scope { libraries: vec!["我的资料".into()], sources: vec![(super::source(&c, src).unwrap(), "我的资料".into())], skipped: vec![] };
        let mut problems: Vec<String> = vec![];
        tauri::async_runtime::block_on(async {
            let t = std::time::Instant::now();
            let (seen, _) = walk(&root).unwrap();
            apply_scan(&s, src, &seen).unwrap();
            println!("scan: {} files in {:.2} s", seen.len(), t.elapsed().as_secs_f32());
            let model = rt.ensure(Svc::Embedding).await.unwrap();
            let rerank_model = rt.ensure(Svc::Rerank).await.unwrap();
            let t = std::time::Instant::now();
            let (embedded, _) = build_source(&c, &s, src, &rt, &model).await;
            println!("build: {embedded} passages in {:.1} s ({})", t.elapsed().as_secs_f32(), model.key);
            let v = source_view(&s, super::source(&c, src).unwrap(), &model.key).unwrap();
            println!("files {} · ready {} · pending {} · failed {:?}", v.files, v.ready, v.pending, v.failed.iter().map(|f| format!("{}：{}", f.path, f.error)).collect::<Vec<_>>());
            if (v.files, v.ready, v.failed.len()) != (9, 8, 1) {
                problems.push(format!("expected 9 files, 8 ready, 1 failed; got {} / {} / {}", v.files, v.ready, v.failed.len()));
            }
            // a question: the pool by similarity, reordered by the rerank model (what `retrieve` does)
            let ask = |q: String| {
                let (rt, model, rerank_model, s, scope) = (&rt, &model, &rerank_model, &s, &scope);
                async move {
                    let t = std::time::Instant::now();
                    let qv = embed::embed(rt, model, &[q.clone()], Input::PassageQuery).await.map_err(String::from).unwrap().remove(0);
                    let ids: Vec<i64> = scope.ids().into_iter().collect();
                    let pool: Vec<Passage> = similar(s, &ids, &model.key, &qv, index::POOL, index::POOL_MIN).unwrap().iter().map(|h| passage(h, scope)).collect();
                    let texts: Vec<String> = pool.iter().map(|p| format!("{}\n{}", p.label, p.content)).collect();
                    let scores = rerank::scores(rt, rerank_model, &q, &texts).await.unwrap();
                    let picked = rerank::order(&pool, &scores, index::LIMIT);
                    println!("\n==== {q}  ({:.1} s, pool {})", t.elapsed().as_secs_f32(), pool.len());
                    for p in &picked {
                        let i = pool.iter().position(|x| x == p).unwrap();
                        println!("  {:.3}  {}  {}", scores[i], p.label, p.content.replace('\n', " ").chars().take(50).collect::<String>());
                    }
                    picked
                }
            };
            let expect = [
                ("押金什么时候能退", "合同要点.docx"),
                ("租的房子家电坏了谁来修", "合同要点.docx"),
                ("我看过的房子里哪套采光好", "看房记录.md"),
                ("知识库这周做到哪了", "周报-第41周.md"),
                ("面试前要准备什么", "面试准备.txt"),
                ("京都哪天去的伏见稻荷", "京都消费.csv"),
                ("how many days of annual leave do I get", "leave-policy.pdf"),
                ("黑暗森林是什么意思", "读书笔记.md"),
            ];
            let asks: Vec<(String, Option<&str>)> = match std::env::var("POMELO_ASK") {
                Ok(a) => a.split('|').map(|q| (q.to_string(), None)).collect(),
                Err(_) => expect.iter().map(|(q, f)| (q.to_string(), Some(*f))).collect(),
            };
            for (q, want) in asks {
                let picked = ask(q.clone()).await;
                if let Some(f) = want {
                    if !picked.first().is_some_and(|p| p.label.starts_with(f)) {
                        problems.push(format!("「{q}」: expected {f} first"));
                    }
                }
            }
            // an edit: only the changed passages are embedded again, and the new text is found
            std::thread::sleep(std::time::Duration::from_millis(20));
            write("租房/看房记录.md", "# 看房记录\n\n## 10 月 5 日 · 朝阳区两居\n\n朝南，采光很好，离地铁 600 米，通勤 35 分钟。月租 6800，押一付三。中介说可以谈到 6500。\n\n## 10 月 12 日 · 海淀一居\n\n朝北，有点暗，但小区安静，楼下有菜市场。月租 5200，押二付一。房东自己住在隔壁楼。\n\n## 结论\n\n10 月 19 日签了朝阳那套，月租谈到 6500，11 月 1 日搬家。");
            let (seen, _) = walk(&root).unwrap();
            apply_scan(&s, src, &seen).unwrap();
            let pending = next_pending(&s, &[src], &model.key).unwrap().map(|d| d.path);
            let (embedded, kept) = build_source(&c, &s, src, &rt, &model).await;
            println!("\nafter an edit: pending {pending:?}, embedded {embedded}, kept {kept}");
            if pending.as_deref() != Some("租房/看房记录.md") || embedded == 0 {
                problems.push("the edited file was not rebuilt".into());
            }
            let picked = ask("最后租了哪套房子，什么时候搬家".into()).await;
            if !picked.first().is_some_and(|p| p.content.contains("11 月 1 日")) {
                problems.push("the edited passage was not found first".into());
            }
            // touched, same bytes: nothing to embed
            let bytes = std::fs::read(root.join("工作/面试准备.txt")).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            std::fs::write(root.join("工作/面试准备.txt"), &bytes).unwrap();
            let (seen, _) = walk(&root).unwrap();
            apply_scan(&s, src, &seen).unwrap();
            let (embedded, _) = build_source(&c, &s, src, &rt, &model).await;
            println!("touched without a change: embedded {embedded}");
            if embedded != 0 {
                problems.push("an unchanged file was embedded again".into());
            }
            rt.stop_ours_on_exit();
            let t = std::time::Instant::now();
            for svc in [Svc::Embedding, Svc::Rerank] {
                loop {
                    rt.service_refresh(svc, true).await;
                    if rt.service_status(svc).state == ServiceState::Stopped || t.elapsed() > std::time::Duration::from_secs(20) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
                println!("{} stopped: {}", svc.label(), rt.service_status(svc).state == ServiceState::Stopped);
            }
        });
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
