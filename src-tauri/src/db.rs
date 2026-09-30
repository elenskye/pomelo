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

pub const MIGRATIONS: &[Migration] = &[
    Migration { version: 1, name: "settings", sql: include_str!("../migrations/0001_settings.sql") },
    Migration { version: 2, name: "books", sql: include_str!("../migrations/0002_books.sql") },
    Migration { version: 3, name: "books_kind", sql: include_str!("../migrations/0003_books_kind.sql") },
    Migration { version: 4, name: "films", sql: include_str!("../migrations/0004_films.sql") },
    Migration { version: 5, name: "kinds", sql: include_str!("../migrations/0005_kinds.sql") },
    Migration { version: 6, name: "drawing", sql: include_str!("../migrations/0006_drawing.sql") },
    Migration { version: 7, name: "packing", sql: include_str!("../migrations/0007_packing.sql") },
    Migration { version: 8, name: "todo", sql: include_str!("../migrations/0008_todo.sql") },
];

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
    migrate_with(conn, migrations, Some(backups_dir))
}

/// `backups_dir = None`: no pre-upgrade snapshot (caller already holds one, e.g. restore).
pub fn migrate_with(conn: &mut Connection, migrations: &[Migration], backups_dir: Option<&Path>) -> Result<i64, String> {
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
    if let (true, Some(dir)) = (current > 0, backups_dir) {
        backup::snapshot(conn, dir, &format!("before-v{}", first.version))?;
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

    /// Real path for installed apps: a v2 DB with books upgrades in place, backup first.
    #[test]
    fn real_migrations_upgrade_existing_books() {
        let dir = temp_dir("real-upgrade");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..2], &dir.join("backups")).unwrap();
        c.execute("INSERT INTO books (title, status, rating) VALUES ('小王子', 'done', 5)", []).unwrap();
        assert_eq!(migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap(), latest_version());
        let (kind, rating): (String, i64) = c
            .query_row("SELECT k.name, b.rating FROM books b JOIN kinds k ON k.id = b.kind_id", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((kind.as_str(), rating), ("虚构", 5));
        assert_eq!(std::fs::read_dir(dir.join("backups")).unwrap().count(), 1);
    }

    /// Installed phase-3 apps are at v4 with fixed kind codes; v5 maps them onto kinds rows.
    #[test]
    fn v4_kind_codes_map_to_kinds() {
        let dir = temp_dir("v4-kinds");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..4], &dir.join("backups")).unwrap();
        c.execute_batch(
            "INSERT INTO books (title, kind) VALUES ('a', 'fiction'), ('b', 'nonfiction'), ('c', 'comic'), ('d', 'other');
             INSERT INTO films (title, kind) VALUES ('e', 'movie'), ('f', 'series'), ('g', 'anime');",
        )
        .unwrap();
        migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap();
        let names = |sql: &str| -> Vec<String> {
            let mut st = c.prepare(sql).unwrap();
            st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        assert_eq!(names("SELECT k.name FROM books b JOIN kinds k ON k.id = b.kind_id ORDER BY b.id"), ["虚构", "非虚构", "漫画", "其他"]);
        assert_eq!(names("SELECT k.name FROM films f JOIN kinds k ON k.id = f.kind_id ORDER BY f.id"), ["电影", "剧集", "动漫"]);
        let cols = names("SELECT name FROM pragma_table_info('books')");
        assert!(cols.contains(&"kind_id".to_string()) && !cols.contains(&"kind".to_string()));
    }

    /// v0.1 installs are at v7; v8 (一步一步) adds tables only, backing up first.
    #[test]
    fn v7_upgrades_to_todo_tables() {
        let dir = temp_dir("v7-todo");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..7], &dir.join("backups")).unwrap();
        c.execute_batch(
            "INSERT INTO books (title) VALUES ('小王子');
             INSERT INTO packing_templates (name, sort_order) VALUES ('回家', 1);",
        )
        .unwrap();
        assert_eq!(migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap(), 8);
        let n: i64 = c.query_row("SELECT count(*) FROM books", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        c.execute("INSERT INTO todo_groups (name, sort_order) VALUES ('📌 Pick Up', 1)", []).unwrap();
        let files: Vec<_> = std::fs::read_dir(dir.join("backups")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(files.len(), 1);
        assert!(files[0].to_str().unwrap().ends_with("-before-v8.db"));
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
