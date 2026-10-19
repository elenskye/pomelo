//! Work (v0.4.5 阶段 9), inside conversations (the user's choice: no page of its own).
//! - 保存为 Word: any finished reply can be saved as a .docx (word.rs) — its Markdown as Word headings,
//!   lists and tables, and the sources it cites listed under 「出处」 at the end. Into a folder the user
//!   picks (remembered: preference `work.folder`, default ~/Documents), named after the reply's first
//!   heading (else the conversation) and the date; an existing file is never overwritten (「 (2)」…).
//! - 改文件 (improve-10: the plugin library's Word 工具箱 / 文本工具箱, skills/toolbox.rs, used when 小拾
//!   decides): a file (Markdown, plain text, Word) read in place (never copied; no backup kept — the
//!   user's choice) as numbered paragraphs (a table, a field, an empty paragraph 不能改); each change the
//!   model proposes is checked (a Word change must keep its formatting, word::check) into the reply's
//!   card (`file_edits`, one file per reply, the file's sha256 when first proposed): each change shows
//!   its paragraph before and after, ticked. 写入文件 writes the ticked ones — refused when the file
//!   changed since — and nothing else in the file moves. Markdown / text keep their line endings and
//!   encoding (UTF-8 with or without BOM, UTF-16). A card can instead hold a new Word document (Word
//!   工具箱 create: its name and Markdown), saved where the user picks when they confirm.
//! - Older questions picked their file (message_edit_files); a new version of one still has it.
//! - Writing a file isn't undoable (it is outside Pomelo's data); proposing, writing and cancelling
//!   aren't either (like sending).

use super::store::{pref_get, pref_set, NOW};
use super::{cite, word};
use crate::modules::common::err;
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

pub const EXTS: &[&str] = &["md", "markdown", "txt", "text", "docx"];
const MAX_TEXT: u64 = 5 * 1024 * 1024;
const MAX_WORD: u64 = 50 * 1024 * 1024;
const MAX_ITEMS: usize = 5000;
const FOLDER: &str = "work.folder";

// ---------- the file to change ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub path: String,
    pub name: String,
}

/// A file a question may change: a regular file of a kind 改文件 handles.
pub fn target(path: &str) -> Result<Target, String> {
    let p = Path::new(path);
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !EXTS.contains(&ext.as_str()) {
        return Err("只能改 Markdown、纯文本和 Word（.docx）文件".into());
    }
    let meta = std::fs::metadata(p).map_err(|_| "找不到这个文件".to_string())?;
    if !meta.is_file() {
        return Err("请选择一个文件".into());
    }
    let max = if ext == "docx" { MAX_WORD } else { MAX_TEXT };
    if meta.len() > max {
        return Err("文件太大了".into());
    }
    Ok(Target { path: path.to_string(), name: p.file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string()) })
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Enc {
    Utf8 { bom: bool },
    Utf16 { le: bool },
}

#[derive(Debug, Clone)]
enum Raw {
    /// The text with \n line ends, whether it had \r\n, how it was encoded, and its paragraphs (byte
    /// ranges of the blocks between blank lines).
    Text { body: String, crlf: bool, enc: Enc, blocks: Vec<(usize, usize)> },
    Word { bytes: Vec<u8>, docx: word::Docx },
}

/// A file read for changing: its paragraphs (text, why it can't be changed) and what it was.
#[derive(Debug, Clone)]
pub struct Doc {
    /// markdown | text | word
    pub kind: &'static str,
    pub items: Vec<(String, Option<&'static str>)>,
    pub sha256: String,
    raw: Raw,
}

fn decode(bytes: &[u8]) -> Result<(String, Enc), String> {
    let bad = || "这个文件不是 UTF-8 或 UTF-16 文本，改不了".to_string();
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let le = bytes[0] == 0xFF;
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        return Ok((String::from_utf16(&units).map_err(|_| bad())?, Enc::Utf16 { le }));
    }
    let bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    let text = std::str::from_utf8(if bom { &bytes[3..] } else { bytes }).map_err(|_| bad())?;
    if text.contains('\0') {
        return Err(bad());
    }
    Ok((text.to_string(), Enc::Utf8 { bom }))
}

fn encode(body: &str, crlf: bool, enc: Enc) -> Vec<u8> {
    let text = if crlf { body.replace('\n', "\r\n") } else { body.to_string() };
    match enc {
        Enc::Utf8 { bom } => {
            let mut out = if bom { vec![0xEF, 0xBB, 0xBF] } else { vec![] };
            out.extend_from_slice(text.as_bytes());
            out
        }
        Enc::Utf16 { le } => {
            let mut out = if le { vec![0xFF, 0xFE] } else { vec![0xFE, 0xFF] };
            for u in text.encode_utf16() {
                out.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
            }
            out
        }
    }
}

/// Paragraphs of a text: blocks of lines between blank lines (a fenced code block is one block).
fn blocks(body: &str) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let (mut start, mut end): (Option<usize>, usize) = (None, 0);
    let mut fence: Option<&str> = None;
    let mut pos = 0;
    for line in body.split_inclusive('\n') {
        let content = line.trim_end_matches('\n');
        let t = content.trim();
        let at = pos;
        pos += line.len();
        if fence.is_none() && t.is_empty() {
            if let Some(s) = start.take() {
                out.push((s, end));
            }
            continue;
        }
        for f in ["```", "~~~"] {
            if t.starts_with(f) {
                fence = if fence == Some(f) { None } else if fence.is_none() { Some(f) } else { fence };
            }
        }
        start.get_or_insert(at);
        end = at + content.len();
    }
    if let Some(s) = start {
        out.push((s, end));
    }
    out
}

pub fn read(path: &Path) -> Result<Doc, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|_| "读不了这个文件，可能已经移走或删除了".to_string())?;
    let sha256 = super::pdf::sha256_hex(&bytes);
    let doc = if ext == "docx" {
        let docx = word::read_docx(&bytes)?;
        Doc { kind: "word", items: docx.items.iter().map(|i| (i.text.clone(), i.fixed)).collect(), sha256, raw: Raw::Word { bytes, docx } }
    } else {
        let (text, enc) = decode(&bytes)?;
        let crlf = text.contains("\r\n");
        let body = text.replace("\r\n", "\n");
        let blocks = blocks(&body);
        let items = blocks.iter().map(|(a, b)| (body[*a..*b].to_string(), None)).collect();
        Doc { kind: if ext.starts_with("md") || ext == "markdown" { "markdown" } else { "text" }, items, sha256, raw: Raw::Text { body, crlf, enc, blocks } }
    };
    if doc.items.is_empty() {
        return Err("文件里没有文字".into());
    }
    if doc.items.len() > MAX_ITEMS {
        return Err(format!("文件太长了（超过 {MAX_ITEMS} 段）"));
    }
    Ok(doc)
}

/// The file's bytes with `changes` ((paragraph index, new text)) made.
pub fn apply(doc: &Doc, changes: &[(usize, String)]) -> Result<Vec<u8>, String> {
    match &doc.raw {
        Raw::Word { bytes, .. } => word::edit_docx(bytes, changes),
        Raw::Text { body, crlf, enc, blocks } => {
            let mut sorted: Vec<&(usize, String)> = changes.iter().collect();
            sorted.sort_by(|a, b| b.0.cmp(&a.0));
            let mut out = body.clone();
            for (i, new) in sorted {
                let (a, b) = *blocks.get(*i).ok_or("没有这一段")?;
                let new = new.trim_end_matches('\n');
                if new.trim().is_empty() {
                    // a removed paragraph takes the blank lines after it (before it, when last)
                    let (a, b) = match (blocks.get(i + 1), i.checked_sub(1).map(|p| blocks[p])) {
                        (Some(next), _) => (a, next.0),
                        (None, Some(prev)) => (prev.1, b),
                        (None, None) => (a, b),
                    };
                    out.replace_range(a..b, "");
                } else {
                    out.replace_range(a..b, new);
                }
            }
            Ok(encode(&out, *crlf, *enc))
        }
    }
}

/// Whether item `i` is a Word table (a line per row, its cells' text can change).
pub fn is_table(doc: &Doc, i: usize) -> bool {
    matches!(&doc.raw, Raw::Word { docx, .. } if docx.items.get(i).is_some_and(|x| x.is_table()))
}

/// Whether paragraph `i` can become `new` (Word: with its formatting kept).
pub fn check(doc: &Doc, i: usize, new: &str) -> Result<(), String> {
    match &doc.raw {
        Raw::Word { docx, .. } => word::check(docx, i, new),
        Raw::Text { .. } => doc.items.get(i).map(|_| ()).ok_or_else(|| "没有这一段".into()),
    }
}

// ---------- picky.db ----------

pub fn record_target(conn: &Connection, message_id: i64, t: &Target) -> Result<(), String> {
    conn.execute("INSERT OR REPLACE INTO message_edit_files (message_id, path, name) VALUES (?1, ?2, ?3)", params![message_id, t.path, t.name]).map(|_| ()).map_err(err)
}

pub fn target_of(conn: &Connection, message_id: i64) -> Result<Option<Target>, String> {
    conn.query_row("SELECT path, name FROM message_edit_files WHERE message_id = ?1", [message_id], |r| Ok(Target { path: r.get(0)?, name: r.get(1)? })).optional().map_err(err)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditItem {
    pub position: i64,
    /// 1-based.
    pub paragraph: i64,
    pub before: String,
    pub after: String,
    pub checked: bool,
}

/// A reply's proposed changes to a file.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEdit {
    pub run_id: i64,
    pub path: String,
    pub name: String,
    /// markdown | text | word
    pub kind: String,
    pub total: i64,
    /// pending | written | cancelled
    pub state: String,
    pub error: Option<String>,
    pub items: Vec<EditItem>,
}

/// A card's kind for a new Word document.
pub const NEW_WORD: &str = "new-word";

/// One proposed change to `t` (paragraph `i`, 0-based) into the reply's card: the card gets the file's
/// state when first proposed; a reply changes one file; the same paragraph again replaces its change.
pub fn propose_change(conn: &Connection, run_id: i64, t: &Target, doc: &Doc, i: usize, before: &str, after: &str) -> Result<(), String> {
    match edit_of(conn, run_id)? {
        Some(e) if e.kind == NEW_WORD => return Err("这条回答已经提议新建一个 Word 了：一条回答只做一件".into()),
        Some(e) if e.path != t.path => return Err(format!("一条回答只改一个文件（已经在改《{}》）", e.name)),
        Some(_) => {
            let sha: String = conn.query_row("SELECT sha256 FROM file_edits WHERE run_id = ?1", [run_id], |r| r.get(0)).map_err(err)?;
            if sha != doc.sha256 {
                return Err(format!("《{}》在这之间被改过了：重新读一遍再改", t.name));
            }
        }
        None => {
            conn.execute("INSERT INTO file_edits (run_id, path, name, kind, sha256, total) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![run_id, t.path, t.name, doc.kind, doc.sha256, doc.items.len() as i64]).map_err(err)?;
        }
    }
    let n = i as i64 + 1;
    if conn.execute("UPDATE file_edit_items SET before = ?3, after = ?4 WHERE run_id = ?1 AND paragraph = ?2", params![run_id, n, before, after]).map_err(err)? == 0 {
        conn.execute(
            "INSERT INTO file_edit_items (run_id, position, paragraph, before, after) VALUES (?1, (SELECT coalesce(max(position) + 1, 0) FROM file_edit_items WHERE run_id = ?1), ?2, ?3, ?4)",
            params![run_id, n, before, after],
        )
        .map_err(err)?;
    }
    Ok(())
}

/// A new Word document into the reply's card (its name and Markdown; a second one replaces it). Returns
/// the folder it would go to (the last one used, else 文稿).
pub fn propose_new_word(conn: &Connection, run_id: i64, name: &str, content: &str) -> Result<String, String> {
    if let Some(e) = edit_of(conn, run_id)? {
        if e.kind != NEW_WORD {
            return Err(format!("这条回答已经在改《{}》了：一条回答只做一件", e.name));
        }
        conn.execute("DELETE FROM file_edits WHERE run_id = ?1", [run_id]).map_err(err)?;
    }
    let folder = default_folder(conn);
    conn.execute("INSERT INTO file_edits (run_id, path, name, kind, sha256, total) VALUES (?1, ?2, ?3, ?4, '', 0)", params![run_id, folder, format!("{name}.docx"), NEW_WORD]).map_err(err)?;
    conn.execute("INSERT INTO file_edit_items (run_id, position, paragraph, before, after) VALUES (?1, 0, 0, '', ?2)", params![run_id, content]).map_err(err)?;
    Ok(std::env::var("HOME").ok().and_then(|h| folder.strip_prefix(&h).map(|r| format!("~{r}"))).unwrap_or(folder))
}

/// Saves a proposed new Word document in `folder` as `name` (never over a file); the card then says where.
pub fn create_word(conn: &Connection, run_id: i64, folder: &str, name: &str) -> Result<FileEdit, String> {
    let e = edit_of(conn, run_id)?.filter(|e| e.kind == NEW_WORD).ok_or("找不到这份文档")?;
    if e.state != "pending" {
        return Err("这份文档已经处理过了".into());
    }
    let content = e.items.first().map(|i| i.after.clone()).unwrap_or_default();
    let name = if name.trim().is_empty() { e.name.trim_end_matches(".docx").to_string() } else { name.to_string() };
    let path = new_docx(conn, run_id, &content, folder, &name)?;
    let file = Path::new(&path).file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().to_string());
    conn.execute(&format!("UPDATE file_edits SET path = ?2, name = ?3, state = 'written', decided_at = {NOW} WHERE run_id = ?1"), params![run_id, path, file]).map_err(err)?;
    edit_of(conn, run_id)?.ok_or_else(|| "找不到这份文档".into())
}

pub fn edit_of(conn: &Connection, run_id: i64) -> Result<Option<FileEdit>, String> {
    let Some(mut e) = conn
        .query_row("SELECT path, name, kind, total, state, error FROM file_edits WHERE run_id = ?1", [run_id], |r| {
            Ok(FileEdit { run_id, path: r.get(0)?, name: r.get(1)?, kind: r.get(2)?, total: r.get(3)?, state: r.get(4)?, error: r.get(5)?, items: vec![] })
        })
        .optional()
        .map_err(err)?
    else {
        return Ok(None);
    };
    let mut st = conn.prepare("SELECT position, paragraph, before, after, checked FROM file_edit_items WHERE run_id = ?1 ORDER BY paragraph, position").map_err(err)?;
    let rows = st.query_map([run_id], |r| Ok(EditItem { position: r.get(0)?, paragraph: r.get(1)?, before: r.get(2)?, after: r.get(3)?, checked: r.get(4)? })).map_err(err)?;
    e.items = rows.collect::<Result<_, _>>().map_err(err)?;
    Ok(Some(e))
}

/// Writes the ticked changes into the file (atomically: a new file in the same folder, then renamed
/// over it, with the old one's permissions). Refused when the file changed since it was read.
pub fn write(conn: &Connection, run_id: i64, positions: &[i64]) -> Result<FileEdit, String> {
    let e = edit_of(conn, run_id)?.filter(|e| e.kind != NEW_WORD).ok_or("找不到这些改动")?;
    if e.state != "pending" {
        return Err("这些改动已经处理过了".into());
    }
    if positions.is_empty() {
        return Err("至少勾上一段".into());
    }
    let path = PathBuf::from(&e.path);
    let doc = read(&path)?;
    let sha: String = conn.query_row("SELECT sha256 FROM file_edits WHERE run_id = ?1", [run_id], |r| r.get(0)).map_err(err)?;
    if doc.sha256 != sha {
        return Err(format!("「{}」在这之间被改过了，没写进去：让小拾重新改一次", e.name));
    }
    let picked: Vec<(usize, String)> = e.items.iter().filter(|i| positions.contains(&i.position)).map(|i| (i.paragraph as usize - 1, i.after.clone())).collect();
    let bytes = apply(&doc, &picked)?;
    // the card names a 知识库 file 「文件夹/路径」: the temporary file goes by the real name
    let file = path.file_name().map_or_else(|| "file".into(), |n| n.to_string_lossy().to_string());
    let tmp = path.with_file_name(format!(".{file}.pomelo-{}", std::process::id()));
    let written = (|| -> std::io::Result<()> {
        std::fs::write(&tmp, &bytes)?;
        if let Ok(m) = std::fs::metadata(&path) {
            std::fs::set_permissions(&tmp, m.permissions())?;
        }
        std::fs::rename(&tmp, &path)
    })();
    if let Err(err_) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("没能写进「{}」：{err_}", e.name));
    }
    conn.execute(&format!("UPDATE file_edits SET state = 'written', decided_at = {NOW} WHERE run_id = ?1"), [run_id]).map_err(err)?;
    conn.execute("UPDATE file_edit_items SET checked = 0 WHERE run_id = ?1", [run_id]).map_err(err)?;
    for p in positions {
        conn.execute("UPDATE file_edit_items SET checked = 1 WHERE run_id = ?1 AND position = ?2", [run_id, *p]).map_err(err)?;
    }
    edit_of(conn, run_id)?.ok_or_else(|| "找不到这些改动".into())
}

pub fn cancel(conn: &Connection, run_id: i64) -> Result<FileEdit, String> {
    if conn.execute(&format!("UPDATE file_edits SET state = 'cancelled', decided_at = {NOW} WHERE run_id = ?1 AND state = 'pending'"), [run_id]).map_err(err)? == 0 {
        return Err("这些改动已经处理过了".into());
    }
    edit_of(conn, run_id)?.ok_or_else(|| "找不到这些改动".into())
}

// ---------- 保存为 Word ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordDefaults {
    pub folder: String,
    pub name: String,
}

fn default_folder(conn: &Connection) -> String {
    pref_get::<String>(conn, FOLDER).ok().flatten().filter(|f| Path::new(f).is_dir()).unwrap_or_else(|| std::env::var("HOME").map(|h| format!("{h}/Documents")).unwrap_or_default())
}

/// A name a file can have: no slashes, colons or control characters; at most 80 characters.
pub fn clean_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c }).collect();
    let n = n.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = n.trim_matches('.').trim();
    let n: String = n.chars().take(80).collect();
    if n.is_empty() { "小拾的报告".into() } else { n }
}

/// A finished reply: (its run, content, its conversation's title).
fn reply(conn: &Connection, message_id: i64) -> Result<(i64, String, String), String> {
    conn.query_row(
        "SELECT m.run_id, m.content, s.title FROM messages m JOIN sessions s ON s.id = m.session_id WHERE m.id = ?1 AND m.role = 'assistant'",
        [message_id],
        |r| Ok((r.get::<_, Option<i64>>(0)?.unwrap_or(0), r.get(1)?, r.get(2)?)),
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "找不到这条回答".into())
}

pub fn word_defaults(conn: &Connection, message_id: i64, today: &str) -> Result<WordDefaults, String> {
    let (_, content, title) = reply(conn, message_id)?;
    let heading = content.lines().map(str::trim).find(|l| l.starts_with('#')).map(|l| l.trim_start_matches('#').trim().to_string()).filter(|h| !h.is_empty());
    Ok(WordDefaults { folder: default_folder(conn), name: clean_name(&format!("{} {today}", heading.unwrap_or(title))) })
}

/// The reply as a .docx in `folder` (never over an existing file); the path written.
pub fn save_word(conn: &Connection, message_id: i64, folder: &str, name: &str) -> Result<String, String> {
    let (run, content, _) = reply(conn, message_id)?;
    if content.trim().is_empty() {
        return Err("这条回答是空的".into());
    }
    new_docx(conn, run, &content, folder, name)
}

/// Markdown as a new .docx in `folder` (「 (2)」… rather than over a file), the sources of `run` it
/// cites listed at its end; the folder is remembered. The path written.
fn new_docx(conn: &Connection, run: i64, content: &str, folder: &str, name: &str) -> Result<String, String> {
    let dir = Path::new(folder);
    if !dir.is_dir() {
        return Err("找不到这个文件夹".into());
    }
    let sources = cite::of_run(conn, run)?;
    let mut cited = cite::cited(content, &sources);
    cited.sort();
    let lines: Vec<String> = cited
        .iter()
        .filter_map(|n| sources.iter().find(|s| s.n == *n))
        .map(|s| {
            let what = [s.detail.clone().unwrap_or_default(), s.title.replace('\n', " ")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("：");
            match &s.url {
                Some(u) => format!("[{}] {what}（{u}）", s.n),
                None => format!("[{}] {what}", s.n),
            }
        })
        .collect();
    let bytes = word::markdown_to_docx(content, &lines)?;
    let base = clean_name(name.trim().trim_end_matches(".docx"));
    let mut k = 1;
    loop {
        let file = if k == 1 { format!("{base}.docx") } else { format!("{base} ({k}).docx") };
        let path = dir.join(&file);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(&bytes).map_err(|e| format!("没能保存：{e}"))?;
                pref_set(conn, FOLDER, &folder.to_string())?;
                return Ok(path.display().to_string());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && k < 100 => k += 1,
            Err(e) => return Err(format!("没能保存：{e}")),
        }
    }
}

// ---------- commands ----------

#[tauri::command]
pub fn picky_file_edit_confirm(state: State<AppState>, run_id: i64, positions: Vec<i64>) -> Result<FileEdit, String> {
    if state.picky.runs.holds(run_id) {
        return Err("回答还没写完".into());
    }
    state.picky.with_db(|c| write(c, run_id, &positions))
}

/// Saves the new Word document a reply proposed (Word 工具箱 create).
#[tauri::command]
pub fn picky_file_create_confirm(state: State<AppState>, run_id: i64, folder: String, name: String) -> Result<FileEdit, String> {
    if state.picky.runs.holds(run_id) {
        return Err("回答还没写完".into());
    }
    state.picky.with_db(|c| create_word(c, run_id, &folder, &name))
}

#[tauri::command]
pub fn picky_file_edit_cancel(state: State<AppState>, run_id: i64) -> Result<FileEdit, String> {
    state.picky.with_db(|c| cancel(c, run_id))
}

#[tauri::command]
pub fn picky_word_defaults(app: AppHandle, message_id: i64) -> Result<WordDefaults, String> {
    let st = app.state::<AppState>();
    let today = st.with_db(|c| crate::modules::todo::today(c)).unwrap_or_default();
    st.picky.with_db(|c| word_defaults(c, message_id, &today))
}

#[tauri::command]
pub fn picky_save_word(state: State<AppState>, message_id: i64, folder: String, name: String) -> Result<String, String> {
    state.picky.with_db(|c| save_word(c, message_id, &folder, &name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::chat::{begin_run, finish, Outcome};
    #[allow(unused_imports)]
    use crate::picky::provider;
    use crate::picky::store::tests::memory_db;

    #[test]
    fn text_files_keep_their_line_ends_and_encoding() {
        let dir = crate::db::tests::temp_dir("work-text");
        let p = dir.join("notes.md");
        std::fs::write(&p, "# 标题\r\n\r\n第一段，\r\n还是第一段。\r\n\r\n```\r\ncode\r\n\r\nmore\r\n```\r\n\r\n最后一段。\r\n").unwrap();
        let doc = read(&p).unwrap();
        assert_eq!(doc.kind, "markdown");
        assert_eq!(doc.items.iter().map(|i| i.0.as_str()).collect::<Vec<_>>(), ["# 标题", "第一段，\n还是第一段。", "```\ncode\n\nmore\n```", "最后一段。"]);
        let out = apply(&doc, &[(1, "改好的第一段。".into()), (3, String::new())]).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "# 标题\r\n\r\n改好的第一段。\r\n\r\n```\r\ncode\r\n\r\nmore\r\n```\r\n");
        // UTF-16 stays UTF-16
        let q = dir.join("u16.txt");
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("甲\n\n乙".encode_utf16().flat_map(|u| u.to_le_bytes()));
        std::fs::write(&q, &bytes).unwrap();
        let doc = read(&q).unwrap();
        let out = apply(&doc, &[(1, "丙".into())]).unwrap();
        assert_eq!(decode(&out).unwrap(), ("甲\n\n丙".to_string(), Enc::Utf16 { le: true }));
        assert!(target(dir.join("x.pdf").to_str().unwrap()).is_err());
        assert_eq!(target(p.to_str().unwrap()).unwrap().name, "notes.md");
    }

    #[test]
    fn a_question_keeps_its_file_for_new_versions() {
        use crate::picky::chat::{self, Extras, NewSession};
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("work-target");
        let p = dir.join("简历.md");
        std::fs::write(&p, "# 简历\n\n三年经验。").unwrap();
        let t = target(p.to_str().unwrap()).unwrap();
        // an older question that picked its file
        let b = chat::begin_run_in(&c, None, &NewSession::default(), "改正式一点", "local:x", "X", &Extras::default()).unwrap();
        record_target(&c, b.user_id, &t).unwrap();
        finish(&c, b.run_id, b.reply_id, &Outcome { content: "改了 1 段".into(), state: "completed", ..Default::default() }).unwrap();
        assert_eq!(chat::path(&c, b.session_id).unwrap()[0].edit_file.as_ref(), Some(&t));
        let again = chat::begin_version(&c, b.reply_id, None, "local:x", "X", &[], &[]).unwrap();
        assert_eq!(again.request.edit.as_ref(), Some(&t));
        finish(&c, again.run_id, again.reply_id, &Outcome { content: "x".into(), state: "completed", ..Default::default() }).unwrap();
        let edited = chat::begin_version(&c, b.user_id, Some("只改错别字"), "local:x", "X", &[], &[]).unwrap();
        assert_eq!(edited.request.edit.as_ref(), Some(&t));
        assert_eq!(target_of(&c, edited.user_id).unwrap().as_ref(), Some(&t));
    }

    #[test]
    fn a_reply_saves_as_word_with_its_sources() {
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("work-word");
        let b = begin_run(&c, None, None, "写报告", "local:x", "X").unwrap();
        finish(&c, b.run_id, b.reply_id, &Outcome { content: "# 租房报告\n\n押金要退 [1]。".into(), state: "completed", ..Default::default() }).unwrap();
        let src = cite::Source { n: 1, kind: "passage".into(), module: None, record_kind: None, id: None, parent: None, day: None, artifact_id: None, segment: Some(0), title: "合同.md · 押金".into(), detail: Some("知识库 · 生活".into()), content: "押金".into(), url: None, kb_source: Some(1), kb_path: Some("合同.md".into()) };
        cite::save(&c, b.run_id, &src).unwrap();
        let d = word_defaults(&c, b.reply_id, "2026-10-18").unwrap();
        assert_eq!(d.name, "租房报告 2026-10-18");
        let folder = dir.display().to_string();
        let first = save_word(&c, b.reply_id, &folder, &d.name).unwrap();
        let second = save_word(&c, b.reply_id, &folder, &d.name).unwrap();
        assert!(first.ends_with("租房报告 2026-10-18.docx") && second.ends_with("租房报告 2026-10-18 (2).docx"));
        let text = word::tests::textutil_text(&dir, &std::fs::read(&first).unwrap());
        assert!(text.contains("押金要退 [1]。") && text.contains("出处") && text.contains("[1] 知识库 · 生活：合同.md · 押金"), "{text}");
        assert_eq!(default_folder(&c), folder);
        assert_eq!(clean_name(" a/b: c? "), "a b c");
    }
}
