//! Files the user adds (from Picky's Attachments / ProjectFiles):
//! - Formats: .txt / .text (UTF-8, or UTF-16 with a BOM), .md / .markdown, .pdf (PDFKit; pages with
//!   text — a scanned PDF is refused); since v0.4.5 Word, PowerPoint, Excel / CSV, images and saved web
//!   pages (office.rs). Only a regular file the user chose (no folders, no symlinks), at most 200 MB,
//!   with some text (images have none).
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
    /// PDF | Markdown | 文本 | Word | PowerPoint | Excel | CSV | 图片 | 网页
    pub format: &'static str,
    /// PDF pages / PowerPoint slides.
    pub page_count: Option<i64>,
    pub byte_count: i64,
    pub deleted: bool,
    /// Tables: data rows.
    pub row_count: Option<i64>,
    /// 网页: its address.
    pub url: Option<String>,
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

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const PPTX: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";

fn format_of(media: &str, url: bool) -> &'static str {
    match media {
        _ if url => "网页",
        "application/pdf" => "PDF",
        "text/markdown" => "Markdown",
        DOCX | "application/msword" => "Word",
        PPTX => "PowerPoint",
        "text/csv" => "CSV",
        m if m.contains("spreadsheet") || m == "application/vnd.ms-excel" => "Excel",
        m if m.starts_with("image/") => "图片",
        _ => "文本",
    }
}

pub fn is_image(media: &str) -> bool {
    media.starts_with("image/")
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
    pub row_count: Option<i64>,
    pub source_url: Option<String>,
}

/// Reads and extracts a file the user chose (slow for a big PDF: call off the main thread).
pub fn prepare(path: &Path) -> Result<Prepared, String> {
    read(path, false)
}

/// `whole_tables`: every row of a big table as text (知识库 files are searched, not sent whole; 查表
/// only exists for attachments).
pub fn read(path: &Path, whole_tables: bool) -> Result<Prepared, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).ok_or("请选择一个文件")?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let media_type = match ext.as_str() {
        "txt" | "text" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "pdf" => "application/pdf",
        "docx" => DOCX,
        "doc" => "application/msword",
        "pptx" => PPTX,
        "xlsx" | "xlsm" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "csv" => "text/csv",
        e => super::office::image_type(e).ok_or("只支持文本、Markdown、PDF、Word、PowerPoint、Excel、CSV 和图片")?,
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
    let mut row_count = None;
    let (page_count, segments) = if is_image(media_type) {
        (None, vec![])
    } else if media_type == DOCX || media_type == "application/msword" {
        let text = super::office::word_text(path)?;
        if text.trim().is_empty() {
            return Err("这个 Word 文件里没有文字".into());
        }
        (None, split_text(text.trim(), false))
    } else if media_type == PPTX {
        let slides = super::office::pptx_slides(&data)?;
        let segments: Vec<Segment> = slides
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.trim().is_empty())
            .enumerate()
            .map(|(ordinal, (i, t))| Segment { ordinal: ordinal as i64, page_start: Some(i as i64 + 1), page_end: Some(i as i64 + 1), heading: None, content: t.clone() })
            .collect();
        if segments.is_empty() {
            return Err("这个 PowerPoint 里没有文字".into());
        }
        (Some(slides.len() as i64), segments)
    } else if super::office::is_table(&ext) {
        let sheets = super::office::read_table(path, &ext)?;
        let (segments, rows) = super::office::table_segments(&sheets, &name, whole_tables);
        row_count = Some(rows);
        (None, segments)
    } else if media_type == "application/pdf" {
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
    Ok(Prepared { name, ext, media_type, sha256: super::pdf::sha256_hex(&data), data, page_count, segments, row_count, source_url: None })
}

/// A pasted image (⌘V): only images; named by the page.
pub fn prepare_image(name: &str, data: Vec<u8>) -> Result<Prepared, String> {
    let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "png".into());
    let media_type = super::office::image_type(&ext).ok_or("只能粘贴图片")?;
    if data.is_empty() || data.len() as u64 > MAX_BYTES {
        return Err("图片是空的或太大了".into());
    }
    Ok(Prepared { name: name.to_string(), ext, media_type, sha256: super::pdf::sha256_hex(&data), data, page_count: None, segments: vec![], row_count: None, source_url: None })
}

/// A saved web page: its article as Markdown is the copy.
pub fn prepare_web(url: &str, title: &str, text: &str) -> Prepared {
    let host = url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url).trim_start_matches("www.");
    let name = if title.trim().is_empty() { host.to_string() } else { title.trim().chars().take(80).collect::<String>() };
    let data = text.as_bytes().to_vec();
    Prepared { name, ext: "md".into(), media_type: "text/markdown", sha256: super::pdf::sha256_hex(&data), data, page_count: None, segments: split_text(text, true), row_count: None, source_url: Some(url.to_string()) }
}

pub fn decode(data: &[u8]) -> Result<String, String> {
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
            "INSERT INTO artifacts (display_name, file_name, media_type, byte_count, sha256, page_count, text_chars, row_count, source_url) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![p.name, file_name, p.media_type, p.data.len() as i64, p.sha256, p.page_count, chars, p.row_count, p.source_url],
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

const INFO_COLS: &str = "a.id, a.display_name, a.media_type, a.page_count, a.byte_count, a.deleted_at IS NOT NULL, a.row_count, a.source_url";

fn info_row(r: &rusqlite::Row) -> rusqlite::Result<FileInfo> {
    let url: Option<String> = r.get(7)?;
    Ok(FileInfo { id: r.get(0)?, name: r.get(1)?, format: format_of(&r.get::<_, String>(2)?, url.is_some()), page_count: r.get(3)?, byte_count: r.get(4)?, deleted: r.get(5)?, row_count: r.get(6)?, url })
}

/// A copy's media type.
pub fn media_of(conn: &Connection, id: i64) -> Result<String, String> {
    conn.query_row("SELECT media_type FROM artifacts WHERE id = ?1", [id], |r| r.get(0)).map_err(err)
}

pub fn info(conn: &Connection, id: i64) -> Result<Option<FileInfo>, String> {
    conn.query_row(&format!("SELECT {INFO_COLS} FROM artifacts a WHERE a.id = ?1"), [id], info_row).optional().map_err(err)
}

/// Newest first; copies kept only as project files or 知识库 web pages stay out (they belong there).
pub fn library(conn: &Connection) -> Result<Vec<FileInfo>, String> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {INFO_COLS} FROM artifacts a WHERE a.deleted_at IS NULL
               AND (EXISTS (SELECT 1 FROM message_attachments m WHERE m.artifact_id = a.id)
                    OR NOT (EXISTS (SELECT 1 FROM project_file_versions v WHERE v.artifact_id = a.id) OR EXISTS (SELECT 1 FROM kb_sources k WHERE k.artifact_id = a.id)))
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
    tx.commit()?;
    Ok(f.indexed)
}

/// Messages that used it keep their record; the copies stay (and show in the library).
pub fn remove_project_file(conn: &Connection, id: i64) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    if tx.execute("DELETE FROM project_files WHERE id = ?1", [id]).map_err(err)? == 0 {
        return Err("这份资料已不在项目里了".into());
    }
    tx.commit()
}

pub fn set_indexed(conn: &Connection, id: i64, indexed: bool) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    let f = project_file(&tx, id)?;
    if indexed && f.current().is_some_and(|v| v.format == "图片") {
        return Err("图片没有文字，不能建立索引".into());
    }
    tx.execute("UPDATE project_files SET indexed = ?1 WHERE id = ?2 AND indexed <> ?1", params![indexed, id]).map_err(err)?;
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

/// Stores a copy of a file the user chose (from the dialog or dropped on the window), remembering where
/// it came from (the file Word 工具箱 / 文本工具箱 change).
#[tauri::command]
pub async fn picky_import_file(app: tauri::AppHandle, path: String) -> Result<FileInfo, String> {
    use tauri::Manager;
    let p = PathBuf::from(&path);
    let prepared = tauri::async_runtime::spawn_blocking(move || prepare(&p)).await.map_err(|e| e.to_string())??;
    let st = app.state::<crate::AppState>();
    let dir = st.picky.files_dir().to_path_buf();
    st.picky.with_db(|c| {
        let id = store(c, &dir, &prepared)?;
        remember_source(c, id, Path::new(&path))?;
        info(c, id)?.ok_or_else(|| "没能保存文件副本".to_string())
    })
}

/// Where a copy came from (the latest file with its content).
pub fn remember_source(conn: &Connection, id: i64, path: &Path) -> Result<(), String> {
    let full = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    conn.execute("UPDATE artifacts SET source_path = ?1 WHERE id = ?2", params![full.display().to_string(), id]).map(|_| ()).map_err(err)
}

/// The file a copy came from, if it was added from disk and is still there.
pub fn source_of(conn: &Connection, id: i64) -> Result<Option<String>, String> {
    let p: Option<String> = conn.query_row("SELECT source_path FROM artifacts WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(err)?.flatten();
    Ok(p.filter(|p| Path::new(p).is_file()))
}

fn save(app: &tauri::AppHandle, prepared: &Prepared) -> Result<FileInfo, String> {
    use tauri::Manager;
    let st = app.state::<crate::AppState>();
    let dir = st.picky.files_dir().to_path_buf();
    st.picky.with_db(|c| {
        let id = store(c, &dir, prepared)?;
        info(c, id)?.ok_or_else(|| "没能保存文件副本".to_string())
    })
}

/// A pasted image (⌘V in the input).
#[tauri::command]
pub fn picky_import_image(app: tauri::AppHandle, name: String, bytes: Vec<u8>) -> Result<FileInfo, String> {
    save(&app, &prepare_image(&name, bytes)?)
}

/// http(s) only; a bare address gets https://.
pub fn clean_url(url: &str) -> Result<String, String> {
    let u = url.trim();
    let u = if u.contains("://") { u.to_string() } else { format!("https://{u}") };
    let host = u.split("://").nth(1).unwrap_or("").split(['/', '?', '#']).next().unwrap_or("");
    if !(u.starts_with("https://") || u.starts_with("http://")) || host.is_empty() || !host.contains('.') || u.chars().any(char::is_whitespace) {
        return Err("网址不对，应为 https://… 这样的网页地址".into());
    }
    Ok(u)
}

const MAX_PAGE: usize = 5 * 1024 * 1024;

async fn fetch_page(url: &str) -> Result<String, String> {
    let c = reqwest::Client::builder().user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Pomelo").timeout(std::time::Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let r = c.get(url).send().await.map_err(|e| if e.is_timeout() { "网页 20 秒都没打开，稍后再试".to_string() } else { "打不开这个网页，检查一下网址和网络".to_string() })?;
    if !r.status().is_success() {
        return Err(format!("网页打不开（{}）", r.status().as_u16()));
    }
    let kind = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
    if !kind.is_empty() && !kind.contains("html") && !kind.contains("text") {
        return Err("这个网址不是网页（可以下载下来当文件添加）".into());
    }
    let bytes = r.bytes().await.map_err(|_| "网页没能读完".to_string())?;
    if bytes.len() > MAX_PAGE {
        return Err("网页太大了".into());
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// A made-up page (dev builds and tests: no network).
pub fn mock_page(url: &str) -> (String, String) {
    let host = url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url).to_string();
    (format!("示例网页（{host}）"), format!("# 示例网页\n\n这是开发版的模拟网页正文，没有联网。\n\n## 第一部分\n\n租房前要看采光、噪音和通勤时间；押一付三，合同要看清楚。\n\n## 第二部分\n\n网址：{url}"))
}

/// A web page by its address: the article's text is saved as a copy (dev builds: a made-up page).
#[tauri::command]
pub async fn picky_import_web(app: tauri::AppHandle, url: String) -> Result<FileInfo, String> {
    use tauri::Manager;
    let url = clean_url(&url)?;
    let (title, text) = if app.state::<crate::AppState>().picky.mode == super::Mode::Mock {
        mock_page(&url)
    } else {
        let html = fetch_page(&url).await?;
        let u = url.clone();
        tauri::async_runtime::spawn_blocking(move || super::office::web_article(&html, &u)).await.map_err(|e| e.to_string())??
    };
    save(&app, &prepare_web(&url, &title, &text))
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

    /// A zip of (name, text) entries, stored (what .pptx / .xlsx are).
    fn zipped(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, text) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(text.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// A real page through the same path (one GET, no user data). Opt-in:
    ///   POMELO_REAL_WEB=1 cargo test real_web_page -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_web_page() {
        if std::env::var("POMELO_REAL_WEB").as_deref() != Ok("1") {
            return;
        }
        let url = clean_url("https://en.wikipedia.org/wiki/Pomelo").unwrap();
        let html = tauri::async_runtime::block_on(fetch_page(&url)).unwrap();
        let (title, text) = super::super::office::web_article(&html, &url).unwrap();
        println!("{title}\n{}…", text.chars().take(400).collect::<String>());
        assert!(title.contains("Pomelo") && text.contains("citrus"));
        let p = prepare_web(&url, &title, &text);
        assert!(p.segments.len() > 1);
    }

    #[test]
    fn word_powerpoint_excel_csv_and_images() {
        let dir = crate::db::tests::temp_dir("picky-office");
        // Word: made by textutil from text
        let txt = temp_file(&dir, "src.txt", "租房笔记\n押一付三，合同看清楚。".as_bytes());
        let docx = dir.join("笔记.docx");
        assert!(std::process::Command::new("/usr/bin/textutil").args(["-convert", "docx", "-output"]).arg(&docx).arg(&txt).status().unwrap().success());
        let p = prepare(&docx).unwrap();
        assert_eq!(format_of(p.media_type, false), "Word");
        assert!(p.segments[0].content.contains("押一付三"));
        // PowerPoint: one segment per slide with text, labelled by slide
        let slide = |t: &str| format!(r#"<p:sld xmlns:a="a" xmlns:p="p"><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:sld>"#);
        let pptx = temp_file(&dir, "汇报.pptx", &zipped(&[("ppt/slides/slide2.xml", &slide("第二页")), ("ppt/slides/slide1.xml", &slide("开场")), ("ppt/slides/slide10.xml", &slide(""))]));
        let p = prepare(&pptx).unwrap();
        assert_eq!((p.page_count, p.segments.len(), p.segments[1].label(), p.segments[1].content.as_str()), (Some(3), 2, "第 2 页".to_string(), "第二页"));
        // Excel: a minimal workbook
        let xlsx = temp_file(&dir, "城市.xlsx", &zipped(&[
            ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#),
            ("_rels/.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#),
            ("xl/workbook.xml", r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="人口" sheetId="1" r:id="rId1"/></sheets></workbook>"#),
            ("xl/_rels/workbook.xml.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#),
            ("xl/worksheets/sheet1.xml", r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>城市</t></is></c><c r="B1" t="inlineStr"><is><t>人口</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>上海</t></is></c><c r="B2"><v>2487</v></c></row></sheetData></worksheet>"#),
        ]));
        let p = prepare(&xlsx).unwrap();
        assert_eq!((format_of(p.media_type, false), p.row_count), ("Excel", Some(1)));
        assert!(p.segments[0].content.contains("1. | 上海 | 2487 |"), "{}", p.segments[0].content);
        // a big CSV keeps a preview; 查表 reads the copy again
        let mut csv = String::from("编号,名称\n");
        for i in 1..=300 {
            csv.push_str(&format!("{i},第{i}项\n"));
        }
        let path = temp_file(&dir, "清单.csv", csv.as_bytes());
        let p = prepare(&path).unwrap();
        assert_eq!((p.row_count, p.segments.len()), (Some(300), 1));
        assert!(p.segments[0].content.contains("前 50 行") && p.segments[0].content.contains("file 填「清单.csv」"));
        let sheets = super::super::office::read_table(&path, "csv").unwrap();
        assert!(super::super::office::table_query(&sheets, &serde_json::json!({ "view": "find", "keyword": "第299项" })).unwrap().contains("299. | 299 | 第299项 |"));
        // images: no text; a big one is shrunk for sending
        let png = dir.join("小.png");
        std::fs::write(&png, super::super::office::tests_png()).unwrap();
        let big = dir.join("大.png");
        assert!(std::process::Command::new("/usr/bin/sips").args(["-z", "40", "3000"]).arg(&png).arg("--out").arg(&big).output().unwrap().status.success());
        let p = prepare(&big).unwrap();
        assert_eq!((format_of(p.media_type, false), p.segments.len()), ("图片", 0));
        let (media, data) = super::super::office::image_for_model(&big, "image/png", &dir.join("tmp")).unwrap();
        assert_eq!(media, "image/png");
        // the PNG header's width (bytes 16–19), after base64
        let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bits: Vec<u8> = data.bytes().take(32).map(|c| table.iter().position(|t| *t == c).unwrap() as u8).collect();
        let bytes: Vec<u8> = bits.chunks(4).flat_map(|q| [q[0] << 2 | q[1] >> 4, q[1] << 4 | q[2] >> 2, q[2] << 6 | q[3]]).collect();
        assert_eq!(u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]), 1568);
        assert!(prepare_image("粘贴的图片.png", std::fs::read(&png).unwrap()).is_ok() && prepare_image("x.txt", b"a".to_vec()).is_err());
        // a web page's text, with its address
        let p = prepare_web("https://example.com/a", "租房笔记", "# 租房\n\n押一付三");
        assert_eq!((p.name.as_str(), p.source_url.as_deref(), format_of(p.media_type, true)), ("租房笔记", Some("https://example.com/a"), "网页"));
        assert_eq!(clean_url("example.com/x").unwrap(), "https://example.com/x");
        assert!(clean_url("ftp://x.com").is_err() && clean_url("not a url").is_err());
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
        assert!(prepare(&temp_file(&dir, "a.zip", b"x")).unwrap_err().contains("只支持"));
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
