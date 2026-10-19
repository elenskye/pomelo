//! 附件 (v0.4.5): files hung on records — 念念不忘 notes, 一步一步 tasks and goals. Rules:
//! - Adding copies the file into <data>/attachments/ (random name, 0600, never overwriting), at most
//!   200 MB, any kind; images (by extension) show as pictures, the rest open in their default app.
//!   The same bytes are kept once (sha256) while that copy is intact. Storing is not undoable (it
//!   only adds a copy); `stored` is NULL for a file not on this Mac (from a data file).
//! - Links (`attachments`): owner + record id + file, ordered, at most 50 per record. They are
//!   written with the record by its module (one ⌘Z step) and removed in the same step as the
//!   record (`prune` after every delete: cascades included, no triggers so undo replays cleanly).
//! - Cleaning (设置 → 附件): copies no record uses are deleted (file and row); not undoable.
//! - Data files carry each record's attachments as { name, sha256 } only; importing relinks to a
//!   file with the same bytes, else keeps the name as a missing file. Not in backups.
//! - The page shows a stored image through `pomelo-file://localhost/<id>` (lib.rs); nothing else
//!   is served.

use super::common::{err, Tx};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::State;

pub const MAX_BYTES: u64 = 200 * 1024 * 1024;
pub const MAX_PER_RECORD: usize = 50;
const NAME_MAX: usize = 255;
const IMAGES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tif", "tiff", "bmp", "avif"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Owner {
    Note,
    Task,
    Goal,
}

impl Owner {
    pub const ALL: [Owner; 3] = [Owner::Note, Owner::Task, Owner::Goal];
    fn code(self) -> &'static str {
        match self {
            Owner::Note => "note",
            Owner::Task => "task",
            Owner::Goal => "goal",
        }
    }
    fn table(self) -> &'static str {
        match self {
            Owner::Note => "notes",
            Owner::Task => "todo_tasks",
            Owner::Goal => "todo_goals",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub id: i64,
    pub name: String,
    /// image | file
    pub kind: String,
    pub bytes: i64,
    /// Not on this Mac (imported from a data file, or cleaned up since).
    pub missing: bool,
}

/// What a data file keeps of an attachment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachmentRecord {
    pub name: String,
    pub sha256: String,
}

fn kind_of(name: &str) -> &'static str {
    let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if IMAGES.contains(&ext.as_str()) {
        "image"
    } else {
        "file"
    }
}

/// A file name to show: its last path part, no control characters, at most 255 chars.
fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let s: String = base.chars().filter(|c| !c.is_control()).take(NAME_MAX).collect();
    if s.is_empty() || s == "." || s == ".." {
        "附件".into()
    } else {
        s
    }
}

pub fn folder_of(data_root: &Path) -> PathBuf {
    data_root.join("attachments")
}

/// The folder for copies (created 0700 if missing; never a symlink).
fn ensure_folder(dir: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.file_type().is_dir() => Ok(()),
        Ok(_) => Err("附件文件夹不是普通文件夹".into()),
        Err(_) => {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).recursive(true).create(dir).map_err(|e| format!("没能建立附件文件夹：{e}"))
        }
    }
}

fn random_name(ext: &str) -> String {
    let mut b = [0u8; 12];
    let _ = getrandom::fill(&mut b);
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let ext: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).take(10).collect::<String>().to_lowercase();
    if ext.is_empty() {
        hex
    } else {
        format!("{hex}.{ext}")
    }
}

/// Where a file's copy is, checked to be a regular file inside `dir`.
pub fn path_of(conn: &Connection, dir: &Path, id: i64) -> Result<PathBuf, String> {
    let stored: Option<String> = conn
        .query_row("SELECT stored FROM attachment_files WHERE id = ?1", [id], |r| r.get(0))
        .optional()
        .map_err(err)?
        .ok_or("找不到这个附件")?;
    let stored = stored.ok_or("这个附件不在这台 Mac 上")?;
    if stored.contains('/') || stored.contains("..") {
        return Err("附件路径无效".into());
    }
    let p = dir.join(stored);
    match std::fs::symlink_metadata(&p) {
        Ok(m) if m.file_type().is_file() => Ok(p),
        _ => Err("附件文件不见了，可能已被删除".into()),
    }
}

const INFO: &str = "f.id, f.name, f.kind, f.bytes, f.stored IS NULL";

fn info_row(r: &rusqlite::Row) -> rusqlite::Result<FileInfo> {
    Ok(FileInfo { id: r.get(0)?, name: r.get(1)?, kind: r.get(2)?, bytes: r.get(3)?, missing: r.get(4)? })
}

pub fn info(conn: &Connection, id: i64) -> Result<FileInfo, String> {
    conn.query_row(&format!("SELECT {INFO} FROM attachment_files f WHERE f.id = ?1"), [id], info_row).optional().map_err(err)?.ok_or_else(|| "找不到这个附件".into())
}

/// Keeps `data` as an attachment file named `name` (an intact copy of the same bytes is reused).
pub fn store(conn: &Connection, dir: &Path, name: &str, data: &[u8]) -> Result<FileInfo, String> {
    if data.is_empty() {
        return Err("文件是空的".into());
    }
    if data.len() as u64 > MAX_BYTES {
        return Err("文件太大了（超过 200 MB）".into());
    }
    let name = clean_name(name);
    let sha = crate::picky::pdf::sha256_hex(data);
    let same: Vec<i64> = {
        let mut st = conn.prepare("SELECT id FROM attachment_files WHERE sha256 = ?1 ORDER BY stored IS NULL, id").map_err(err)?;
        let rows = st.query_map([&sha], |r| r.get(0)).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)?
    };
    for id in &same {
        if let Ok(p) = path_of(conn, dir, *id) {
            if std::fs::read(&p).is_ok_and(|b| crate::picky::pdf::sha256_hex(&b) == sha) {
                return info(conn, *id);
            }
        }
    }
    ensure_folder(dir)?;
    let ext = Path::new(&name).extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    let stored = random_name(&ext);
    let target = dir.join(&stored);
    let write = || -> std::io::Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&target)?;
        f.write_all(data)?;
        f.sync_all()
    };
    write().map_err(|e| format!("没能保存附件：{e}"))?;
    // a file known by name only (from a data file) gets this copy
    let missing = same.iter().find_map(|id| info(conn, *id).ok().filter(|f| f.missing)).map(|f| f.id);
    let saved = match missing {
        Some(id) => conn.execute("UPDATE attachment_files SET stored = ?1, bytes = ?2 WHERE id = ?3", params![stored, data.len() as i64, id]).map(|_| id),
        None => conn
            .execute("INSERT INTO attachment_files (name, stored, kind, bytes, sha256) VALUES (?1, ?2, ?3, ?4, ?5)", params![name, stored, kind_of(&name), data.len() as i64, sha])
            .map(|_| conn.last_insert_rowid()),
    };
    match saved {
        Ok(id) => info(conn, id),
        Err(e) => {
            let _ = std::fs::remove_file(&target); // the fresh copy just written, never the user's file
            Err(err(e))
        }
    }
}

/// Reads a file the user picked or dropped (a regular file, not a folder or symlink) and keeps it.
pub fn store_path(conn: &Connection, dir: &Path, path: &Path) -> Result<FileInfo, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| "文件不存在或没有读取权限".to_string())?;
    if !meta.file_type().is_file() {
        return Err("只能添加普通文件，不能添加文件夹".into());
    }
    if meta.len() > MAX_BYTES {
        return Err("文件太大了（超过 200 MB）".into());
    }
    let data = std::fs::read(path).map_err(|e| format!("无法读取文件：{e}"))?;
    store(conn, dir, &path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), &data)
}

// ---------- links ----------

/// A record's files, in order.
pub fn of(conn: &Connection, owner: Owner, record: i64) -> Result<Vec<FileInfo>, String> {
    let mut st = conn
        .prepare(&format!("SELECT {INFO} FROM attachments a JOIN attachment_files f ON f.id = a.file_id WHERE a.owner = ?1 AND a.record_id = ?2 ORDER BY a.sort_order, a.id"))
        .map_err(err)?;
    let rows = st.query_map(params![owner.code(), record], info_row).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Every record of `owner` that has files → its files, in order.
pub fn all_of(conn: &Connection, owner: Owner) -> Result<HashMap<i64, Vec<FileInfo>>, String> {
    let mut st = conn
        .prepare(&format!("SELECT a.record_id, {INFO} FROM attachments a JOIN attachment_files f ON f.id = a.file_id WHERE a.owner = ?1 ORDER BY a.record_id, a.sort_order, a.id"))
        .map_err(err)?;
    let mut out: HashMap<i64, Vec<FileInfo>> = HashMap::new();
    let rows = st
        .query_map([owner.code()], |r| Ok((r.get::<_, i64>(0)?, FileInfo { id: r.get(1)?, name: r.get(2)?, kind: r.get(3)?, bytes: r.get(4)?, missing: r.get(5)? })))
        .map_err(err)?;
    for row in rows {
        let (id, f) = row.map_err(err)?;
        out.entry(id).or_default().push(f);
    }
    Ok(out)
}

/// The files a record will have (ids in order): duplicates dropped, each must exist.
pub fn clean_ids(conn: &Connection, ids: &[i64]) -> Result<Vec<i64>, String> {
    let mut seen = std::collections::HashSet::new();
    let out: Vec<i64> = ids.iter().copied().filter(|i| seen.insert(*i)).collect();
    if out.len() > MAX_PER_RECORD {
        return Err(format!("一条最多 {MAX_PER_RECORD} 个附件"));
    }
    for id in &out {
        info(conn, *id)?;
    }
    Ok(out)
}

fn current(conn: &Connection, owner: Owner, record: i64) -> Result<Vec<i64>, String> {
    let mut st = conn.prepare("SELECT file_id FROM attachments WHERE owner = ?1 AND record_id = ?2 ORDER BY sort_order, id").map_err(err)?;
    let rows = st.query_map(params![owner.code(), record], |r| r.get(0)).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// A record's files := `ids` (checked by `clean_ids`); only a real change is written.
pub fn set(conn: &Connection, owner: Owner, record: i64, ids: &[i64]) -> Result<(), String> {
    let ids = clean_ids(conn, ids)?;
    if current(conn, owner, record)? == ids {
        return Ok(());
    }
    let tx = Tx::begin(conn)?;
    tx.execute("DELETE FROM attachments WHERE owner = ?1 AND record_id = ?2", params![owner.code(), record]).map_err(err)?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute("INSERT INTO attachments (owner, record_id, file_id, sort_order) VALUES (?1, ?2, ?3, ?4)", params![owner.code(), record, id, i as i64 + 1])
            .map_err(err)?;
    }
    tx.commit()
}

/// Drops the links of records that are gone (after a delete, cascades included).
pub fn prune(conn: &Connection) -> Result<(), String> {
    for o in Owner::ALL {
        conn.execute(&format!("DELETE FROM attachments WHERE owner = ?1 AND record_id NOT IN (SELECT id FROM {})", o.table()), [o.code()]).map_err(err)?;
    }
    Ok(())
}

// ---------- data files ----------

pub fn records_of(files: &[FileInfo], conn: &Connection) -> Result<Vec<AttachmentRecord>, String> {
    files
        .iter()
        .map(|f| conn.query_row("SELECT sha256 FROM attachment_files WHERE id = ?1", [f.id], |r| r.get(0)).map(|sha256| AttachmentRecord { name: f.name.clone(), sha256 }).map_err(err))
        .collect()
}

pub fn check_records(recs: &[AttachmentRecord]) -> Result<(), String> {
    if recs.len() > MAX_PER_RECORD {
        return Err(format!("一条最多 {MAX_PER_RECORD} 个附件"));
    }
    for r in recs {
        let hex = r.sha256.len() == 64 && r.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
        if !hex {
            return Err(format!("附件「{}」的校验值不对", r.name));
        }
    }
    Ok(())
}

/// The file ids for imported attachments: the same bytes if known, else a missing-file row.
pub fn import_records(conn: &Connection, recs: &[AttachmentRecord]) -> Result<Vec<i64>, String> {
    check_records(recs)?;
    recs.iter()
        .map(|r| {
            let known: Option<i64> = conn
                .query_row("SELECT id FROM attachment_files WHERE sha256 = ?1 ORDER BY stored IS NULL, id LIMIT 1", [&r.sha256], |x| x.get(0))
                .optional()
                .map_err(err)?;
            match known {
                Some(id) => Ok(id),
                None => {
                    let name = clean_name(&r.name);
                    conn.execute("INSERT INTO attachment_files (name, stored, kind, bytes, sha256) VALUES (?1, NULL, ?2, 0, ?3)", params![name, kind_of(&name), r.sha256])
                        .map_err(err)?;
                    Ok(conn.last_insert_rowid())
                }
            }
        })
        .collect()
}

// ---------- space ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub files: i64,
    pub bytes: i64,
    /// Copies no record uses.
    pub unused: i64,
    pub unused_bytes: i64,
    pub dir: String,
}

const USED: &str = "EXISTS (SELECT 1 FROM attachments a WHERE a.file_id = f.id AND (
    (a.owner = 'note' AND a.record_id IN (SELECT id FROM notes)) OR
    (a.owner = 'task' AND a.record_id IN (SELECT id FROM todo_tasks)) OR
    (a.owner = 'goal' AND a.record_id IN (SELECT id FROM todo_goals))))";

pub fn usage(conn: &Connection, dir: &Path) -> Result<Usage, String> {
    let (files, bytes, unused, unused_bytes) = conn
        .query_row(
            &format!("SELECT count(*), coalesce(sum(bytes), 0), coalesce(sum(NOT {USED}), 0), coalesce(sum(CASE WHEN {USED} THEN 0 ELSE bytes END), 0) FROM attachment_files f WHERE stored IS NOT NULL"),
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(err)?;
    Ok(Usage { files, bytes, unused, unused_bytes, dir: dir.display().to_string() })
}

/// Deletes the copies no record uses (and names of files no longer anywhere); returns how many copies.
pub fn clean(conn: &Connection, dir: &Path) -> Result<i64, String> {
    let unused: Vec<(i64, Option<String>)> = {
        let mut st = conn.prepare(&format!("SELECT id, stored FROM attachment_files f WHERE NOT {USED}")).map_err(err)?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)?
    };
    let mut removed = 0;
    for (id, stored) in unused {
        if let Some(s) = stored.filter(|s| !s.contains('/') && !s.contains("..")) {
            let p = dir.join(&s);
            if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_file()) {
                std::fs::remove_file(&p).map_err(|e| format!("没能删除附件文件：{e}"))?;
            }
            removed += 1;
        }
        conn.execute("DELETE FROM attachment_files WHERE id = ?1", [id]).map_err(err)?;
    }
    Ok(removed)
}

// ---------- the page's view of an image ----------

/// (bytes, content type) of a stored image for `pomelo-file://localhost/<id>`.
pub fn image_bytes(conn: &Connection, dir: &Path, id: i64) -> Result<(Vec<u8>, &'static str), String> {
    let f = info(conn, id)?;
    if f.kind != "image" {
        return Err("不是图片".into());
    }
    let p = path_of(conn, dir, id)?;
    let ext = Path::new(&f.name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "tif" | "tiff" => "image/tiff",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    };
    Ok((std::fs::read(p).map_err(err)?, mime))
}

// ---------- commands ----------

impl AppState {
    fn attachments_dir(&self) -> PathBuf {
        folder_of(&self.paths.root)
    }
}

/// Files the user picked or dropped; each one is kept or fails on its own (name: reason).
#[tauri::command]
pub fn attachments_add_paths(state: State<AppState>, paths: Vec<String>) -> Result<Vec<FileInfo>, String> {
    let dir = state.attachments_dir();
    let mut out = vec![];
    let mut failed = vec![];
    state.with_db(|c| {
        for p in &paths {
            match store_path(c, &dir, Path::new(p)) {
                Ok(f) => out.push(f),
                Err(e) => failed.push(format!("{}：{e}", clean_name(p))),
            }
        }
        Ok(())
    })?;
    if out.is_empty() && !failed.is_empty() {
        return Err(failed.join("\n"));
    }
    Ok(out)
}

/// Pasted or dragged-in bytes (raw body); `x-name`: the file name, percent-encoded.
#[tauri::command]
pub fn attachments_add_bytes(state: State<AppState>, request: tauri::ipc::Request<'_>) -> Result<FileInfo, String> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("没有收到文件内容".into());
    };
    let name = request.headers().get("x-name").and_then(|v| v.to_str().ok()).map(percent_decode).unwrap_or_else(|| "粘贴的图片.png".into());
    let dir = state.attachments_dir();
    state.with_db(|c| store(c, &dir, &name, data))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[tauri::command]
pub fn attachments_of(state: State<AppState>, owner: Owner, record: i64) -> Result<Vec<FileInfo>, String> {
    state.with_db(|c| of(c, owner, record))
}

#[tauri::command]
pub fn attachment_open(app: tauri::AppHandle, state: State<AppState>, id: i64) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = state.attachments_dir();
    let path = state.with_db(|c| path_of(c, &dir, id))?;
    app.opener().open_path(path.display().to_string(), None::<&str>).map_err(|e| format!("打不开这个附件：{e}"))
}

#[tauri::command]
pub fn attachments_usage(state: State<AppState>) -> Result<Usage, String> {
    let dir = state.attachments_dir();
    state.with_db(|c| usage(c, &dir))
}

#[tauri::command]
pub fn attachments_clean(state: State<AppState>) -> Result<i64, String> {
    let dir = state.attachments_dir();
    state.with_db(|c| clean(c, &dir))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::tests::{memory_db, temp_dir};

    pub fn note(c: &Connection, text: &str) -> i64 {
        crate::modules::notes::create(c, serde_json::from_value(serde_json::json!({ "content": text })).unwrap()).unwrap().id
    }

    #[test]
    fn store_dedupes_and_keeps_names_and_kinds() {
        let c = memory_db();
        let dir = temp_dir("att-store");
        let a = store(&c, &dir, "/Users/x/截图 1.PNG", b"png-bytes").unwrap();
        assert_eq!((a.name.as_str(), a.kind.as_str(), a.bytes, a.missing), ("截图 1.PNG", "image", 9, false));
        let b = store(&c, &dir, "again.png", b"png-bytes").unwrap();
        assert_eq!(b.id, a.id); // same bytes, one copy
        let f = store(&c, &dir, "发票.pdf", b"%PDF").unwrap();
        assert_eq!(f.kind, "file");
        assert!(store(&c, &dir, "empty.txt", b"").is_err());
        let p = path_of(&c, &dir, a.id).unwrap();
        assert!(p.starts_with(&*dir) && std::fs::read(&p).unwrap() == b"png-bytes");
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name(""), "附件");
        assert_eq!(percent_decode("%E6%88%AA%E5%9B%BE.png"), "截图.png");
        // a folder is refused, a real file is read
        assert!(store_path(&c, &dir, &dir).is_err());
        let src = dir.join("outside.txt");
        std::fs::write(&src, "hello").unwrap();
        assert_eq!(store_path(&c, &dir, &src).unwrap().name, "outside.txt");
        let (bytes, mime) = image_bytes(&c, &dir, a.id).unwrap();
        assert_eq!((bytes.as_slice(), mime), (&b"png-bytes"[..], "image/png"));
        assert!(image_bytes(&c, &dir, f.id).is_err());
    }

    #[test]
    fn links_follow_their_records_and_cleaning_keeps_used_files() {
        let c = memory_db();
        let dir = temp_dir("att-links");
        let a = store(&c, &dir, "a.png", b"aaa").unwrap().id;
        let b = store(&c, &dir, "b.pdf", b"bbb").unwrap().id;
        let n = note(&c, "带图的笔记");
        set(&c, Owner::Note, n, &[b, a, b]).unwrap();
        assert_eq!(of(&c, Owner::Note, n).unwrap().iter().map(|f| f.id).collect::<Vec<_>>(), [b, a]);
        assert!(set(&c, Owner::Note, n, &[999]).is_err());
        assert_eq!(usage(&c, &dir).unwrap().unused, 0);
        // the record goes → its links go (prune), the copies stay until cleaned
        crate::modules::notes::delete(&c, n).unwrap();
        assert!(all_of(&c, Owner::Note).unwrap().is_empty());
        let u = usage(&c, &dir).unwrap();
        assert_eq!((u.files, u.unused, u.unused_bytes), (2, 2, 6));
        let keep = store(&c, &dir, "c.png", b"ccc").unwrap().id;
        let n2 = note(&c, "另一条");
        set(&c, Owner::Note, n2, &[keep]).unwrap();
        assert_eq!(clean(&c, &dir).unwrap(), 2);
        assert!(path_of(&c, &dir, a).is_err() && path_of(&c, &dir, keep).is_ok());
        assert_eq!(std::fs::read_dir(&*dir).unwrap().count(), 1);
    }

    #[test]
    fn imported_names_relink_by_bytes_or_wait_as_missing() {
        let c = memory_db();
        let dir = temp_dir("att-import");
        let a = store(&c, &dir, "a.png", b"aaa").unwrap();
        let recs = records_of(&[a.clone()], &c).unwrap();
        assert_eq!(import_records(&c, &recs).unwrap(), [a.id]);
        let other = AttachmentRecord { name: "别处.jpg".into(), sha256: crate::picky::pdf::sha256_hex(b"elsewhere") };
        let ids = import_records(&c, &[other.clone()]).unwrap();
        let f = info(&c, ids[0]).unwrap();
        assert_eq!((f.name.as_str(), f.kind.as_str(), f.missing), ("别处.jpg", "image", true));
        assert_eq!(import_records(&c, &[other]).unwrap(), ids); // known by name now
        // the same bytes added later fill the missing file
        assert_eq!(store(&c, &dir, "x.jpg", b"elsewhere").unwrap().id, ids[0]);
        assert!(!info(&c, ids[0]).unwrap().missing);
        assert!(check_records(&[AttachmentRecord { name: "x".into(), sha256: "zz".into() }]).is_err());
    }
}
