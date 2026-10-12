//! 小拾's space (设置 → 小拾 → 空间管理, from Picky's StorageMaintenance):
//! - What it takes: picky.db (with its -wal) and the copies in picky-files/ (backups are in the
//!   backup list).
//! - Every stored copy with its size and what uses it (questions, project files). Deleting one
//!   (confirmed on the page) removes the file and marks it 「文件已删除」: messages keep its name, a
//!   project file using it can't be read any more, its index goes. Files in picky-files/ that no
//!   record knows can be removed too. Neither is undoable.
//! - Conversations without any message, untouched for 30 days, can be cleaned (undoable).

use crate::modules::common::{err, Tx};
use crate::AppState;
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;
use tauri::State;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRow {
    #[serde(flatten)]
    pub file: super::files::FileInfo,
    /// Size on disk now (0 when the copy is gone).
    pub bytes: u64,
    pub messages: i64,
    pub project_files: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stray {
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub db_bytes: u64,
    pub files_bytes: u64,
    /// Biggest first; deleted ones left out.
    pub files: Vec<FileRow>,
    pub strays: Vec<Stray>,
    pub empty_sessions: i64,
}

fn size(p: &Path) -> u64 {
    std::fs::symlink_metadata(p).ok().filter(|m| m.file_type().is_file()).map_or(0, |m| m.len())
}

const EMPTY: &str = "NOT EXISTS (SELECT 1 FROM messages m WHERE m.session_id = sessions.id) AND updated_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-30 days')";

pub fn usage(conn: &Connection, db: &Path, dir: &Path) -> Result<Usage, String> {
    let mut st = conn
        .prepare(
            "SELECT a.id, a.file_name,
                    (SELECT count(*) FROM message_attachments m WHERE m.artifact_id = a.id) + (SELECT count(*) FROM message_project_files m WHERE m.artifact_id = a.id),
                    (SELECT count(*) FROM project_file_versions v WHERE v.artifact_id = a.id)
             FROM artifacts a",
        )
        .map_err(err)?;
    let all: Vec<(i64, String, i64, i64)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut files = vec![];
    for (id, file_name, messages, project_files) in &all {
        let Some(info) = super::files::info(conn, *id)? else { continue };
        if info.deleted {
            continue;
        }
        files.push(FileRow { bytes: size(&dir.join(file_name)), file: info, messages: *messages, project_files: *project_files });
    }
    files.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.file.id.cmp(&b.file.id)));
    let known: Vec<&String> = all.iter().map(|x| &x.1).collect();
    let mut strays = vec![];
    let mut files_bytes = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let b = size(&e.path());
            files_bytes += b;
            if b > 0 && !name.starts_with('.') && !known.contains(&&name) {
                strays.push(Stray { name, bytes: b });
            }
        }
    }
    strays.sort_by(|a, b| a.name.cmp(&b.name));
    let db_bytes = size(db) + size(&db.with_extension("db-wal"));
    let empty_sessions = conn.query_row(&format!("SELECT count(*) FROM sessions WHERE {EMPTY}"), [], |r| r.get(0)).map_err(err)?;
    Ok(Usage { db_bytes, files_bytes, files, strays, empty_sessions })
}

/// Removes a stored copy; the record stays, marked deleted (its index goes).
pub fn delete_file(conn: &Connection, dir: &Path, id: i64) -> Result<(), String> {
    let name: String = conn.query_row("SELECT file_name FROM artifacts WHERE id = ?1 AND deleted_at IS NULL", [id], |r| r.get(0)).map_err(|_| "这个文件已经删掉了".to_string())?;
    if name.contains('/') || name.contains("..") {
        return Err("文件路径无效".into());
    }
    let tx = Tx::begin(conn)?;
    tx.execute("UPDATE artifacts SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1", [id]).map_err(err)?;
    tx.execute("DELETE FROM artifact_indexes WHERE artifact_id = ?1", [id]).map_err(err)?;
    let p = dir.join(&name);
    if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_file()) {
        std::fs::remove_file(&p).map_err(|e| format!("没能删除文件：{e}"))?;
    }
    tx.commit()
}

/// A file in picky-files/ that no record knows.
pub fn delete_stray(conn: &Connection, dir: &Path, name: &str) -> Result<(), String> {
    if name.contains('/') || name.contains("..") || name.starts_with('.') {
        return Err("文件名无效".into());
    }
    let known: i64 = conn.query_row("SELECT count(*) FROM artifacts WHERE file_name = ?1", [name], |r| r.get(0)).map_err(err)?;
    if known > 0 {
        return Err("这个文件还在用".into());
    }
    let p = dir.join(name);
    if !std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_file()) {
        return Err("找不到这个文件".into());
    }
    std::fs::remove_file(&p).map_err(|e| format!("没能删除文件：{e}"))
}

pub fn clean_empty(conn: &Connection) -> Result<usize, String> {
    conn.execute(&format!("DELETE FROM sessions WHERE {EMPTY}"), []).map_err(err)
}

// ---------- commands ----------

fn db_path(state: &AppState) -> std::path::PathBuf {
    state.paths.picky_db.clone()
}

#[tauri::command]
pub fn picky_storage(state: State<AppState>) -> Result<Usage, String> {
    let (db, dir) = (db_path(&state), state.picky.files_dir().to_path_buf());
    state.picky.with_db(|c| usage(c, &db, &dir))
}

#[tauri::command]
pub fn picky_storage_delete_file(state: State<AppState>, id: i64) -> Result<(), String> {
    if state.picky.runs.busy() {
        return Err("正在生成回复，等它写完再删。".into());
    }
    let dir = state.picky.files_dir().to_path_buf();
    state.picky.with_db(|c| delete_file(c, &dir, id))
}

#[tauri::command]
pub fn picky_storage_delete_stray(state: State<AppState>, name: String) -> Result<(), String> {
    let dir = state.picky.files_dir().to_path_buf();
    state.picky.with_db(|c| delete_stray(c, &dir, &name))
}

#[tauri::command]
pub fn picky_storage_clean_empty(state: State<AppState>) -> Result<usize, String> {
    state.with_picky_undo("清理空对话", |c| clean_empty(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::chat::{begin_run_in, Extras, NewSession};
    use crate::picky::files::{self, tests::add};
    use crate::picky::store::tests::memory_db;

    #[test]
    fn sizes_uses_deleting_and_empty_conversations() {
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("picky-storage");
        let files_dir = dir.join("files");
        let a = add(&c, &dir, "a.md", b"hello world");
        let b = add(&c, &dir, "b.txt", b"hi");
        let extras = Extras { attachments: vec![a], ..Default::default() };
        begin_run_in(&c, None, &NewSession::default(), "q", "local:x", "X", &extras).unwrap();
        std::fs::write(files_dir.join("stray.pdf"), b"left over").unwrap();
        let u = usage(&c, &dir.join("picky.db"), &files_dir).unwrap();
        assert_eq!(u.files.iter().map(|f| (f.file.id, f.messages)).collect::<Vec<_>>(), [(a, 1), (b, 0)]);
        assert_eq!(u.files_bytes, 11 + 2 + 9);
        assert_eq!(u.strays, [Stray { name: "stray.pdf".into(), bytes: 9 }]);
        delete_file(&c, &files_dir, a).unwrap();
        assert!(files::info(&c, a).unwrap().unwrap().deleted);
        assert!(delete_file(&c, &files_dir, a).is_err());
        assert!(delete_stray(&c, &files_dir, &std::fs::read_dir(&files_dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).find(|n| n != "stray.pdf").unwrap()).is_err());
        delete_stray(&c, &files_dir, "stray.pdf").unwrap();
        let u = usage(&c, &dir.join("picky.db"), &files_dir).unwrap();
        assert_eq!((u.files.len(), u.strays.len()), (1, 0));
        // an empty conversation untouched for 30 days goes; others stay
        c.execute("INSERT INTO sessions (title, updated_at) VALUES ('空', '2020-01-01T00:00:00.000Z'), ('新的空', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))", []).unwrap();
        assert_eq!(usage(&c, &dir.join("picky.db"), &files_dir).unwrap().empty_sessions, 1);
        assert_eq!(clean_empty(&c).unwrap(), 1);
        assert_eq!(c.query_row("SELECT count(*) FROM sessions", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    }
}
