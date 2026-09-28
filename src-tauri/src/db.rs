//! Connection setup + migrations.
//! Rules: migrations are append-only (never edit a shipped one); versions are 1..=N
//! with no gaps; schema version lives in `PRAGMA user_version`.

use crate::backup;
use rusqlite::Connection;
use std::path::Path;

pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "settings",
    sql: include_str!("../migrations/0001_settings.sql"),
}];

pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("无法打开数据库：{e}"))?;
    configure(&conn)?;
    Ok(conn)
}

pub fn configure(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )
    .map_err(|e| e.to_string())
}

pub fn user_version(conn: &Connection) -> Result<i64, String> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())
}

/// Applies pending migrations. If the DB already has data (version > 0), takes a
/// snapshot backup into `backups_dir` first. Returns the resulting version.
pub fn migrate(conn: &mut Connection, migrations: &[Migration], backups_dir: &Path) -> Result<i64, String> {
    let current = user_version(conn)?;
    let latest = migrations.last().map_or(0, |m| m.version);
    if current > latest {
        return Err(format!(
            "数据库版本（{current}）比这个 App 支持的版本（{latest}）新。请使用新版 App 打开，不要用旧版覆盖数据。"
        ));
    }
    let pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > current).collect();
    let Some(first) = pending.first() else {
        return Ok(current);
    };
    if current > 0 {
        backup::snapshot(conn, backups_dir, &format!("before-v{}", first.version))?;
    }
    for m in pending {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute_batch(m.sql)
            .map_err(|e| format!("数据库升级 {:04}_{} 失败：{e}", m.version, m.name))?;
        tx.pragma_update(None, "user_version", m.version)
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    Ok(latest)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::path::PathBuf;

    pub fn latest_version() -> i64 {
        MIGRATIONS.last().map_or(0, |m| m.version)
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pomelo-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    pub fn memory_db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        configure(&c).unwrap();
        migrate(&mut c, MIGRATIONS, &std::env::temp_dir()).unwrap(); // fresh DB: no backup taken
        c
    }

    #[test]
    fn migrations_are_sequential() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version, i as i64 + 1, "gap or reorder at {}", m.name);
            assert!(!m.sql.trim().is_empty());
        }
    }

    #[test]
    fn fresh_db_migrates_without_backup() {
        let dir = temp_dir("fresh");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        assert_eq!(migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap(), latest_version());
        assert_eq!(user_version(&c).unwrap(), latest_version());
        assert!(!dir.join("backups").exists());
        // idempotent
        assert_eq!(migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap(), latest_version());
    }

    #[test]
    fn upgrade_backs_up_first() {
        let dir = temp_dir("upgrade");
        let backups = dir.join("backups");
        let v1 = [Migration { version: 1, name: "a", sql: "CREATE TABLE a (x INTEGER) STRICT; INSERT INTO a VALUES (7);" }];
        let v2 = [
            Migration { version: 1, name: "a", sql: v1[0].sql },
            Migration { version: 2, name: "b", sql: "CREATE TABLE b (y TEXT) STRICT;" },
        ];
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &v1, &backups).unwrap();
        assert_eq!(migrate(&mut c, &v2, &backups).unwrap(), 2);

        let files: Vec<_> = std::fs::read_dir(&backups).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(files.len(), 1);
        assert!(files[0].to_str().unwrap().ends_with("-before-v2.db"));
        let snap = Connection::open(&files[0]).unwrap();
        assert_eq!(user_version(&snap).unwrap(), 1);
        let x: i64 = snap.query_row("SELECT x FROM a", [], |r| r.get(0)).unwrap();
        assert_eq!(x, 7);
    }

    #[test]
    fn failed_migration_rolls_back() {
        let dir = temp_dir("rollback");
        let bad = [Migration { version: 1, name: "bad", sql: "CREATE TABLE ok (x INTEGER); SYNTAX ERROR;" }];
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        assert!(migrate(&mut c, &bad, &dir).is_err());
        assert_eq!(user_version(&c).unwrap(), 0);
        let n: i64 = c
            .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'ok'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn refuses_newer_db() {
        let dir = temp_dir("newer");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        c.pragma_update(None, "user_version", latest_version() + 1).unwrap();
        let err = migrate(&mut c, MIGRATIONS, &dir).unwrap_err();
        assert!(err.contains("新"), "{err}");
    }
}
