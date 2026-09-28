//! Snapshot backups via `VACUUM INTO` (consistent single-file copy, WAL-safe).

use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Writes `pomelo-YYYYMMDD-HHMMSS-<label>.db` into `dir`. Never overwrites.
pub fn snapshot(conn: &Connection, dir: &Path, label: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("无法创建备份目录：{e}"))?;
    let stamp: String = conn
        .query_row("SELECT strftime('%Y%m%d-%H%M%S', 'now', 'localtime')", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let mut path = dir.join(format!("pomelo-{stamp}-{label}.db"));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("pomelo-{stamp}-{label}-{n}.db"));
    }
    let target = path.to_str().ok_or("备份路径包含无法识别的字符")?;
    conn.execute("VACUUM INTO ?1", [target])
        .map_err(|e| format!("备份失败：{e}"))?;
    Ok(path)
}
