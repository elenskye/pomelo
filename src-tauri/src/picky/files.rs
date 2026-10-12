//! Files the user adds (from Picky's Attachments / ProjectFiles):
//! - Formats: .txt / .text (UTF-8, or UTF-16 with a BOM), .md / .markdown, .pdf (PDFKit; pages with
//!   text — a scanned PDF is refused). Only a regular file the user chose (no folders, no symlinks),
//!   at most 200 MB, with some text.
//! - Adding copies the bytes into picky-files/ (random name, never overwriting) and extracts the text
//!   once into segments: a PDF page, or a Markdown section, each at most 6000 characters. The same
//!   bytes are stored once (sha256) while that copy is intact. Storing is not undoable (it only adds
//!   a copy; storage management cleans up in phase 7).
//! - Attachments belong to the question they were sent with; questions earlier on the path send
//!   theirs again (chat.rs). The attachment library lists stored copies, except those kept only as
//!   project files.
//! - Project files: per project, never the same bytes twice. Replacing adds a version (messages keep
//!   the version they used, `message_project_files`); removing keeps the copies (they show in the
//!   library). Adding / replacing / removing / turning the index on or off are undoable; the index
//!   itself is derived (index.rs).

use super::store::NOW;
use crate::modules::common::{err, Tx};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MAX_BYTES: u64 = 200 * 1024 * 1024;
const SEGMENT_CHARS: usize = 6000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub id: i64,
    pub name: String,
    /// PDF | Markdown | 文本
    pub format: &'static str,
    pub page_count: Option<i64>,
    pub byte_count: i64,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub ordinal: i64,
    pub page_start: Option<i64>,
    pub page_end: Option<i64>,
    pub heading: Option<String>,
    pub content: String,
}

impl Segment {
    /// 第 3 页 · 第 3–4 页 · the heading · 第 2 段
    pub fn label(&self) -> String {
        match (self.page_start, self.page_end, &self.heading) {
            (Some(a), Some(b), _) if a != b => format!("第 {a}–{b} 页"),
            (Some(a), _, _) => format!("第 {a} 页"),
            (_, _, Some(h)) if !h.is_empty() => h.clone(),
            _ => format!("第 {} 段", self.ordinal + 1),
        }
    }
}

fn format_of(media: &str) -> &'static str {
    match media {
        "application/pdf" => "PDF",
        "text/markdown" => "Markdown",
        _ => "文本",
    }
}

// ---------- preparing ----------

#[derive(Debug, Clone)]
pub struct Prepared {
    pub name: String,
    pub ext: String,
    pub media_type: &'static str,
    pub data: Vec<u8>,
    pub sha256: String,
    pub page_count: Option<i64>,
    pub segments: Vec<Segment>,
}

/// Reads and extracts a file the user chose (slow for a big PDF: call off the main thread).
pub fn prepare(path: &Path) -> Result<Prepared, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).ok_or("请选择一个文件")?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let media_type = match ext.as_str() {
        "txt" | "text" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "pdf" => "application/pdf",
        _ => return Err("只支持文本、Markdown 和 PDF 文件".into()),
    };
    let meta = std::fs::symlink_metadata(path).map_err(|_| "文件不存在或没有读取权限".to_string())?;
    if !meta.file_type().is_file() {
        return Err("请选择普通文件，不能添加文件夹或符号链接".into());
    }
    if meta.len() > MAX_BYTES {
        return Err("文件太大了（超过 200 MB）".into());
    }
    let data = std::fs::read(path).map_err(|e| format!("无法读取文件：{e}"))?;
    if data.is_empty() {
        return Err("文件是空的，没有可以读取的文字".into());
    }
    let (page_count, segments) = if media_type == "application/pdf" {
        let pages = super::pdf::pages(&data)?;
        let segments: Vec<Segment> = pages
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.trim().is_empty())
            .enumerate()
            .map(|(ordinal, (i, t))| Segment { ordinal: ordinal as i64, page_start: Some(i as i64 + 1), page_end: Some(i as i64 + 1), heading: None, content: t.trim().to_string() })
            .collect();
        if segments.is_empty() {
            return Err("PDF 没有可提取的文字，可能是扫描版；请先转换成带文字的 PDF".into());
        }
        (Some(pages.len() as i64), segments)
    } else {
        let text = decode(&data)?;
        if text.trim().is_empty() {
            return Err("文件只有空白，没有可以读取的文字".into());
        }
        (None, split_text(&text, media_type == "text/markdown"))
    };
    Ok(Prepared { name, ext, media_type, sha256: super::pdf::sha256_hex(&data), data, page_count, segments })
}

fn decode(data: &[u8]) -> Result<String, String> {
    let bad = || "无法识别文件文字，请将文本另存为 UTF-8 后再添加".to_string();
    let text = if data.starts_with(&[0xFF, 0xFE]) || data.starts_with(&[0xFE, 0xFF]) {
        let le = data[0] == 0xFF;
        let units: Vec<u16> = data[2..].chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        String::from_utf16(&units).map_err(|_| bad())?
    } else {
        String::from_utf8(data.to_vec()).map_err(|_| bad())?
    };
    if text.contains('\0') {
        return Err(bad());
    }
    Ok(text.replace("\r\n", "\n").replace('\r', "\n").trim_start_matches('\u{feff}').to_string())
}

/// Markdown headings (outside code fences) start sections; each section is cut every 6000
/// characters. Every character lands in exactly one segment.
pub fn split_text(text: &str, markdown: bool) -> Vec<Segment> {
    let mut sections: Vec<(Option<String>, String)> = vec![];
    let (mut heading, mut current, mut fence): (Option<String>, String, Option<char>) = (None, String::new(), None);
    let lines: Vec<&str> = text.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        let marker = if t.starts_with("```") {
            Some('`')
        } else if t.starts_with("~~~") {
            Some('~')
        } else {
            None
        };
        if let Some(m) = marker {
            if fence == Some(m) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(m);
            }
        }
        let hashes = t.chars().take_while(|c| *c == '#').count();
        let is_heading = markdown && fence.is_none() && (1..=6).contains(&hashes) && t[hashes..].starts_with(char::is_whitespace);
        if is_heading {
            if !current.is_empty() {
                sections.push((heading.clone(), std::mem::take(&mut current)));
            }
            heading = Some(t[hashes..].trim().to_string());
        }
        current.push_str(line);
        if i < lines.len() - 1 {
            current.push('\n');
        }
    }
    if !current.is_empty() {
        sections.push((heading, current));
    }
    let mut out = vec![];
    for (h, content) in sections {
        let chars: Vec<char> = content.chars().collect();
        for piece in chars.chunks(SEGMENT_CHARS) {
            out.push(Segment { ordinal: out.len() as i64, page_start: None, page_end: None, heading: h.clone(), content: piece.iter().collect() });
        }
    }
    out
}

// ---------- storing ----------

fn random_name(ext: &str) -> String {
    let mut b = [0u8; 12];
    let _ = getrandom::fill(&mut b);
    format!("{}.{ext}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())
}

/// The folder for copies (created 0700 if missing; never a symlink).
fn folder(dir: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.file_type().is_dir() => Ok(()),
        Ok(_) => Err("小拾的文件夹不是普通文件夹".into()),
        Err(_) => {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).recursive(true).create(dir).map_err(|e| format!("没能建立小拾的文件夹：{e}"))
        }
    }
}

/// Where a stored copy is, checked to be a regular file inside `dir`.
pub fn path_of(conn: &Connection, dir: &Path, id: i64) -> Result<PathBuf, String> {
    let (file, deleted): (String, Option<String>) = conn
        .query_row("SELECT file_name, deleted_at FROM artifacts WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .map_err(err)?
        .ok_or("找不到这个文件")?;
    if deleted.is_some() {
        return Err("文件已删除".into());
    }
    if file.contains('/') || file.contains("..") {
        return Err("文件路径无效".into());
    }
    let p = dir.join(file);
    match std::fs::symlink_metadata(&p) {
        Ok(m) if m.file_type().is_file() => Ok(p),
        _ => Err("文件不见了，可能已被删除".into()),
    }
}

/// Stores a prepared file (reusing an intact copy of the same bytes). Returns its id.
pub fn store(conn: &Connection, dir: &Path, p: &Prepared) -> Result<i64, String> {
    let same: Vec<i64> = {
        let mut st = conn.prepare("SELECT id FROM artifacts WHERE sha256 = ?1 AND deleted_at IS NULL ORDER BY id").map_err(err)?;
        let rows = st.query_map([&p.sha256], |r| r.get(0)).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)?
    };
    for id in same {
        if let Ok(path) = path_of(conn, dir, id) {
            if std::fs::read(&path).is_ok_and(|b| super::pdf::sha256_hex(&b) == p.sha256) {
                return Ok(id);
            }
        }
    }
    folder(dir)?;
    let file_name = random_name(&p.ext);
    let target = dir.join(&file_name);
    let write = || -> std::io::Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&target)?;
        f.write_all(&p.data)?;
        f.sync_all()
    };
    write().map_err(|e| format!("没能保存文件副本：{e}"))?;
    let saved = (|| {
        let tx = Tx::begin(conn)?;
        let chars: i64 = p.segments.iter().map(|s| s.content.chars().count() as i64).sum();
        tx.execute(
            "INSERT INTO artifacts (display_name, file_name, media_type, byte_count, sha256, page_count, text_chars) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![p.name, file_name, p.media_type, p.data.len() as i64, p.sha256, p.page_count, chars],
        )
        .map_err(err)?;
        let id = tx.last_insert_rowid();
        for s in &p.segments {
            tx.execute(
                "INSERT INTO artifact_segments (artifact_id, ordinal, page_start, page_end, heading, content) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, s.ordinal, s.page_start, s.page_end, s.heading, s.content],
            )
            .map_err(err)?;
        }
        tx.commit()?;
        Ok::<i64, String>(id)
    })();
    if saved.is_err() {
        let _ = std::fs::remove_file(&target); // the fresh copy just written, never the user's file
    }
    saved
}

const INFO_COLS: &str = "a.id, a.display_name, a.media_type, a.page_count, a.byte_count, a.deleted_at IS NOT NULL";

fn info_row(r: &rusqlite::Row) -> rusqlite::Result<FileInfo> {
    Ok(FileInfo { id: r.get(0)?, name: r.get(1)?, format: format_of(&r.get::<_, String>(2)?), page_count: r.get(3)?, byte_count: r.get(4)?, deleted: r.get(5)? })
}

pub fn info(conn: &Connection, id: i64) -> Result<Option<FileInfo>, String> {
    conn.query_row(&format!("SELECT {INFO_COLS} FROM artifacts a WHERE a.id = ?1"), [id], info_row).optional().map_err(err)
}

/// Newest first; copies kept only as project files stay out (they belong to their project).
pub fn library(conn: &Connection) -> Result<Vec<FileInfo>, String> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {INFO_COLS} FROM artifacts a WHERE a.deleted_at IS NULL
               AND (EXISTS (SELECT 1 FROM message_attachments m WHERE m.artifact_id = a.id)
                    OR NOT EXISTS (SELECT 1 FROM project_file_versions v WHERE v.artifact_id = a.id))
             ORDER BY a.created_at DESC, a.id DESC"
        ))
        .map_err(err)?;
    let rows = st.query_map([], info_row).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

pub fn segments(conn: &Connection, id: i64) -> Result<Vec<Segment>, String> {
    let mut st = conn.prepare("SELECT ordinal, page_start, page_end, heading, content FROM artifact_segments WHERE artifact_id = ?1 ORDER BY ordinal").map_err(err)?;
    let rows = st.query_map([id], |r| Ok(Segment { ordinal: r.get(0)?, page_start: r.get(1)?, page_end: r.get(2)?, heading: r.get(3)?, content: r.get(4)? })).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// A readable copy (not deleted, with text), or why not.
pub fn usable(conn: &Connection, id: i64) -> Result<FileInfo, String> {
    let f = info(conn, id)?.ok_or("附件不存在了，请重新选择")?;
    if f.deleted {
        return Err(format!("「{}」已删除，请重新选择", f.name));
    }
    Ok(f)
}

// ---------- in messages ----------

/// A file as one question used it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageFile {
    #[serde(flatten)]
    pub file: FileInfo,
    /// Project files: the file and the version used ("资料" / "资料 · 第 2 版").
    pub file_id: Option<i64>,
    pub version: Option<i64>,
    pub label: Option<String>,
}

pub fn attach(conn: &Connection, message_id: i64, ids: &[i64]) -> Result<(), String> {
    let mut seen = vec![];
    for id in ids {
        if seen.contains(id) {
            continue;
        }
        usable(conn, *id)?;
        conn.execute("INSERT INTO message_attachments (message_id, artifact_id, position) VALUES (?1, ?2, ?3)", params![message_id, id, seen.len() as i64]).map_err(err)?;
        seen.push(*id);
    }
    Ok(())
}

/// (file, version, artifact) of each project file a question uses.
pub type Used = (i64, i64, i64);

pub fn record_used(conn: &Connection, message_id: i64, files: &[Used]) -> Result<(), String> {
    for (i, (file, version, artifact)) in files.iter().enumerate() {
        conn.execute(
            "INSERT INTO message_project_files (message_id, position, file_id, version, artifact_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![message_id, i as i64, file, version, artifact],
        )
        .map_err(err)?;
    }
    Ok(())
}

pub fn attachments_of(conn: &Connection, message_id: i64) -> Result<Vec<FileInfo>, String> {
    let mut st = conn.prepare(&format!("SELECT {INFO_COLS} FROM message_attachments m JOIN artifacts a ON a.id = m.artifact_id WHERE m.message_id = ?1 ORDER BY m.position")).map_err(err)?;
    let rows = st.query_map([message_id], info_row).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

pub fn used_of(conn: &Connection, message_id: i64) -> Result<Vec<Used>, String> {
    let mut st = conn.prepare("SELECT file_id, version, artifact_id FROM message_project_files WHERE message_id = ?1 ORDER BY position").map_err(err)?;
    let rows = st.query_map([message_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Attachments, then project files, as the message shows them.
pub fn message_files(conn: &Connection, message_id: i64) -> Result<Vec<MessageFile>, String> {
    let mut out: Vec<MessageFile> = attachments_of(conn, message_id)?.into_iter().map(|file| MessageFile { file, file_id: None, version: None, label: None }).collect();
    for (file_id, version, artifact) in used_of(conn, message_id)? {
        if let Some(file) = info(conn, artifact)? {
            let label = if version > 1 { format!("资料 · 第 {version} 版") } else { "资料".into() };
            out.push(MessageFile { file, file_id: Some(file_id), version: Some(version), label: Some(label) });
        }
    }
    Ok(out)
}

// ---------- project files ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub version: i64,
    pub file: FileInfo,
    pub added_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    pub id: i64,
    pub project_id: i64,
    pub current_version: i64,
    pub indexed: bool,
    /// Oldest first.
    pub versions: Vec<Version>,
}

impl ProjectFile {
    pub fn current(&self) -> Option<&FileInfo> {
        self.versions.iter().find(|v| v.version == self.current_version).map(|v| &v.file)
    }
    pub fn usable(&self) -> bool {
        self.current().is_some_and(|f| !f.deleted)
    }
    pub fn name(&self) -> String {
        self.current().map_or("资料".into(), |f| f.name.clone())
    }
}

/// A project's files (every project's with None), in list order.
pub fn project_files(conn: &Connection, project: Option<i64>) -> Result<Vec<ProjectFile>, String> {
    let mut st = conn
        .prepare("SELECT id, project_id, current_version, indexed FROM project_files WHERE ?1 IS NULL OR project_id = ?1 ORDER BY sort_order, created_at, id")
        .map_err(err)?;
    let mut files: Vec<ProjectFile> = st
        .query_map([project], |r| Ok(ProjectFile { id: r.get(0)?, project_id: r.get(1)?, current_version: r.get(2)?, indexed: r.get(3)?, versions: vec![] }))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut st = conn.prepare("SELECT version, artifact_id, added_at FROM project_file_versions WHERE file_id = ?1 ORDER BY version").map_err(err)?;
    for f in files.iter_mut() {
        let rows: Vec<(i64, i64, String)> = st.query_map([f.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
        for (version, artifact, added_at) in rows {
            if let Some(file) = info(conn, artifact)? {
                f.versions.push(Version { version, file, added_at });
            }
        }
    }
    Ok(files)
}

pub fn project_file(conn: &Connection, id: i64) -> Result<ProjectFile, String> {
    let project: Option<i64> = conn.query_row("SELECT project_id FROM project_files WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(err)?;
    let project = project.ok_or("这份资料已不在项目里了")?;
    project_files(conn, Some(project))?.into_iter().find(|f| f.id == id).ok_or_else(|| "这份资料已不在项目里了".into())
}

/// Refuses bytes already in the project (another file, or one of its versions).
fn reject_duplicate(conn: &Connection, project: i64, artifact: i64, except: Option<i64>) -> Result<(), String> {
    let hit: Option<(i64, i64, i64)> = conn
        .query_row(
            "SELECT f.id, v.version, f.current_version FROM project_file_versions v
             JOIN project_files f ON f.id = v.file_id JOIN artifacts a ON a.id = v.artifact_id
             WHERE f.project_id = ?1 AND f.id IS NOT ?2 AND a.sha256 = (SELECT sha256 FROM artifacts WHERE id = ?3)
             ORDER BY v.version = f.current_version DESC LIMIT 1",
            params![project, except, artifact],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(err)?;
    match hit {
        None => Ok(()),
        Some((id, v, cur)) => {
            let name = project_file(conn, id).map(|f| f.name()).unwrap_or_else(|_| "资料".into());
            Err(if v == cur { format!("项目里已有这份资料：{name}") } else { format!("这是项目资料「{name}」第 {v} 版的文件") })
        }
    }
}

pub fn add_project_file(conn: &Connection, project: i64, artifact: i64) -> Result<i64, String> {
    usable(conn, artifact)?;
    let tx = Tx::begin(conn)?;
    tx.query_row("SELECT id FROM projects WHERE id = ?1", [project], |r| r.get::<_, i64>(0)).optional().map_err(err)?.ok_or("这个项目已经被删除了")?;
    reject_duplicate(&tx, project, artifact, None)?;
    tx.execute("INSERT INTO project_files (project_id, sort_order) VALUES (?1, (SELECT coalesce(max(sort_order), -1) + 1 FROM project_files WHERE project_id = ?1))", [project]).map_err(err)?;
    let id = tx.last_insert_rowid();
    tx.execute("INSERT INTO project_file_versions (file_id, version, artifact_id) VALUES (?1, 1, ?2)", [id, artifact]).map_err(err)?;
    tx.commit()?;
    Ok(id)
}

/// 替换为新版本: the new copy becomes current. Returns whether the file is indexed (rebuild it).
pub fn replace_project_file(conn: &Connection, id: i64, artifact: i64) -> Result<bool, String> {
    usable(conn, artifact)?;
    let tx = Tx::begin(conn)?;
    let f = project_file(&tx, id)?;
    let sha = |a: i64| tx.query_row("SELECT sha256 FROM artifacts WHERE id = ?1", [a], |r| r.get::<_, String>(0)).map_err(err);
    if let Some(cur) = f.current() {
        if sha(cur.id)? == sha(artifact)? {
            return Err("选的文件和现在的版本一样".into());
        }
    }
    reject_duplicate(&tx, f.project_id, artifact, Some(id))?;
    let next = f.versions.iter().map(|v| v.version).max().unwrap_or(0) + 1;
    tx.execute("INSERT INTO project_file_versions (file_id, version, artifact_id) VALUES (?1, ?2, ?3)", params![id, next, artifact]).map_err(err)?;
    tx.execute(&format!("UPDATE project_files SET current_version = ?1, updated_at = {NOW} WHERE id = ?2"), params![next, id]).map_err(err)?;
    super::index::prune(&tx)?;
    tx.commit()?;
    Ok(f.indexed)
}

/// Messages that used it keep their record; the copies stay (and show in the library).
pub fn remove_project_file(conn: &Connection, id: i64) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    if tx.execute("DELETE FROM project_files WHERE id = ?1", [id]).map_err(err)? == 0 {
        return Err("这份资料已不在项目里了".into());
    }
    super::index::prune(&tx)?;
    tx.commit()
}

pub fn set_indexed(conn: &Connection, id: i64, indexed: bool) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    project_file(&tx, id)?;
    tx.execute("UPDATE project_files SET indexed = ?1 WHERE id = ?2 AND indexed <> ?1", params![indexed, id]).map_err(err)?;
    super::index::prune(&tx)?;
    tx.commit()
}

/// The current versions of `ids` (files of `project`, readable) as a question records them.
pub fn current_versions(conn: &Connection, project: Option<i64>, ids: &[i64]) -> Result<Vec<Used>, String> {
    let mut out: Vec<Used> = vec![];
    for id in ids {
        let f = project_file(conn, *id)?;
        if Some(f.project_id) != project {
            return Err(format!("「{}」不是这个项目的资料", f.name()));
        }
        let cur = f.current().filter(|c| !c.deleted).ok_or_else(|| format!("资料「{}」已删除或不能读取，请重新选择", f.name()))?;
        if !out.iter().any(|u| u.2 == cur.id) {
            out.push((f.id, f.current_version, cur.id));
        }
    }
    Ok(out)
}

// ---------- commands ----------

/// Stores a copy of a file the user chose (from the dialog or dropped on the window).
#[tauri::command]
pub async fn picky_import_file(app: tauri::AppHandle, path: String) -> Result<FileInfo, String> {
    use tauri::Manager;
    let p = PathBuf::from(&path);
    let prepared = tauri::async_runtime::spawn_blocking(move || prepare(&p)).await.map_err(|e| e.to_string())??;
    let st = app.state::<crate::AppState>();
    let dir = st.picky.files_dir().to_path_buf();
    st.picky.with_db(|c| {
        let id = store(c, &dir, &prepared)?;
        info(c, id)?.ok_or_else(|| "没能保存文件副本".to_string())
    })
}

#[tauri::command]
pub fn picky_attachment_library(state: tauri::State<crate::AppState>) -> Result<Vec<FileInfo>, String> {
    state.picky.with_db(|c| library(c))
}

/// Opens a stored copy in its default app.
#[tauri::command]
pub fn picky_open_file(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    use tauri::Manager;
    use tauri_plugin_opener::OpenerExt;
    let st = app.state::<crate::AppState>();
    let dir = st.picky.files_dir().to_path_buf();
    let path = st.picky.with_db(|c| path_of(c, &dir, id))?;
    app.opener().open_path(path.display().to_string(), None::<&str>).map_err(|e| format!("打不开这个文件：{e}"))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::picky::library;
    use crate::picky::store::tests::memory_db;

    pub fn temp_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    /// Stores a file with these contents; returns its id.
    pub fn add(c: &Connection, dir: &Path, name: &str, bytes: &[u8]) -> i64 {
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        store(c, &dir.join("files"), &prepare(&temp_file(&src, name, bytes)).unwrap()).unwrap()
    }

    #[test]
    fn markdown_splits_by_heading_outside_fences_and_long_sections() {
        let s = split_text("intro\n# A\none\n```\n# not a heading\n```\n## B\ntwo", true);
        assert_eq!(s.iter().map(|x| x.heading.clone()).collect::<Vec<_>>(), [None, Some("A".into()), Some("B".into())]);
        assert_eq!(s.iter().map(|x| x.content.as_str()).collect::<String>(), "intro\n# A\none\n```\n# not a heading\n```\n## B\ntwo");
        let long = "字".repeat(13_000);
        let s = split_text(&long, false);
        assert_eq!(s.iter().map(|x| x.content.chars().count()).collect::<Vec<_>>(), [6000, 6000, 1000]);
        assert_eq!((s[2].label(), Segment { ordinal: 0, page_start: Some(3), page_end: Some(4), heading: None, content: "".into() }.label()), ("第 3 段".into(), "第 3–4 页".into()));
    }

    #[test]
    fn preparing_checks_format_text_and_encoding() {
        let dir = crate::db::tests::temp_dir("picky-prepare");
        assert!(prepare(&temp_file(&dir, "a.docx", b"x")).unwrap_err().contains("只支持"));
        assert!(prepare(&temp_file(&dir, "a.txt", b"")).unwrap_err().contains("空的"));
        assert!(prepare(&temp_file(&dir, "b.txt", b" \n ")).unwrap_err().contains("空白"));
        assert!(prepare(&temp_file(&dir, "c.txt", &[0xff, 0x00, 0x80])).is_err());
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("你好\r\n世界".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
        assert_eq!(prepare(&temp_file(&dir, "d.txt", &utf16)).unwrap().segments[0].content, "你好\n世界");
        std::os::unix::fs::symlink(dir.join("d.txt"), dir.join("e.txt")).unwrap();
        assert!(prepare(&dir.join("e.txt")).unwrap_err().contains("符号链接"));
        let pdf = prepare(&temp_file(&dir, "f.pdf", &crate::picky::pdf::tests::sample_pdf(&["Page one", "Page two"]))).unwrap();
        assert_eq!((pdf.page_count, pdf.segments.len(), pdf.segments[1].label()), (Some(2), 2, "第 2 页".into()));
    }

    #[test]
    fn same_bytes_are_stored_once_and_the_library_leaves_out_project_only_files() {
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("picky-store");
        let a = add(&c, &dir, "笔记.md", b"# Hi\ntext");
        assert_eq!(add(&c, &dir, "另一个名字.md", b"# Hi\ntext"), a);
        assert_eq!(std::fs::read_dir(dir.join("files")).unwrap().count(), 1);
        assert_eq!(segments(&c, a).unwrap()[0].heading.as_deref(), Some("Hi"));
        // a copy that went missing is not reused
        std::fs::remove_file(path_of(&c, &dir.join("files"), a).unwrap()).unwrap();
        let b = add(&c, &dir, "笔记.md", b"# Hi\ntext");
        assert_ne!(a, b);
        let p = library::create_project(&c, "P", "").unwrap();
        let only = add(&c, &dir, "资料.txt", b"project only");
        add_project_file(&c, p, only).unwrap();
        let ids: Vec<i64> = library(&c).unwrap().iter().map(|f| f.id).collect();
        assert!(ids.contains(&b) && !ids.contains(&only));
    }

    #[test]
    fn project_files_versions_and_duplicates() {
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("picky-project-files");
        let p = library::create_project(&c, "P", "").unwrap();
        let v1 = add(&c, &dir, "提纲.md", b"v1");
        let f = add_project_file(&c, p, v1).unwrap();
        assert!(add_project_file(&c, p, v1).unwrap_err().contains("已有这份资料"));
        let v2 = add(&c, &dir, "提纲.md", b"v2");
        assert!(replace_project_file(&c, f, v1).unwrap_err().contains("一样"));
        assert!(!replace_project_file(&c, f, v2).unwrap());
        let file = project_file(&c, f).unwrap();
        assert_eq!((file.current_version, file.versions.len(), file.current().unwrap().id), (2, 2, v2));
        assert!(add_project_file(&c, p, v1).unwrap_err().contains("第 1 版"));
        assert_eq!(current_versions(&c, Some(p), &[f, f]).unwrap(), [(f, 2, v2)]);
        assert!(current_versions(&c, None, &[f]).is_err());
        set_indexed(&c, f, true).unwrap();
        assert!(project_file(&c, f).unwrap().indexed);
        remove_project_file(&c, f).unwrap();
        assert!(project_files(&c, Some(p)).unwrap().is_empty());
        assert!(library(&c).unwrap().iter().any(|x| x.id == v1)); // copies stay, now in the library
    }
}
