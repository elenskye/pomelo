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
    Migration { version: 9, name: "todo_types", sql: include_str!("../migrations/0009_todo_types.sql") },
    Migration { version: 10, name: "todo_dates", sql: include_str!("../migrations/0010_todo_dates.sql") },
    Migration { version: 11, name: "todo_checkin", sql: include_str!("../migrations/0011_todo_checkin.sql") },
    Migration { version: 12, name: "finance", sql: include_str!("../migrations/0012_finance.sql") },
    Migration { version: 13, name: "jobs", sql: include_str!("../migrations/0013_jobs.sql") },
    Migration { version: 14, name: "notes", sql: include_str!("../migrations/0014_notes.sql") },
    Migration { version: 15, name: "vault", sql: include_str!("../migrations/0015_vault.sql") },
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
        assert_eq!(migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap(), latest_version());
        let n: i64 = c.query_row("SELECT count(*) FROM books", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        c.execute("INSERT INTO todo_groups (name, sort_order) VALUES ('📌 Pick Up', 1)", []).unwrap();
        let files: Vec<_> = std::fs::read_dir(dir.join("backups")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(files.len(), 1);
        assert!(files[0].to_str().unwrap().ends_with("-before-v8.db"));
    }

    /// Phase-1 installs are at v8 with tasks; v9 adds task types (existing tasks → normal) and logs.
    #[test]
    fn v8_tasks_become_normal_type() {
        let dir = temp_dir("v8-types");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..8], &dir.join("backups")).unwrap();
        c.execute_batch(
            "INSERT INTO todo_groups (name, sort_order) VALUES ('📌 Pick Up', 1);
             INSERT INTO todo_tasks (group_id, title, sort_order) VALUES (1, '嘉信入金', 1);",
        )
        .unwrap();
        migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap();
        let t: String = c.query_row("SELECT task_type FROM todo_tasks", [], |r| r.get(0)).unwrap();
        assert_eq!(t, "normal");
        c.execute("INSERT INTO todo_logs (task_id, day) VALUES (1, '2026-09-30')", []).unwrap();
        let files: Vec<_> = std::fs::read_dir(dir.join("backups")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(files[0].to_str().unwrap().ends_with("-before-v9.db"));
    }

    #[test]
    fn v9_plan_dates_fold_into_ranges() {
        let dir = temp_dir("v9-dates");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..9], &dir.join("backups")).unwrap();
        c.execute_batch(
            "INSERT INTO todo_groups (name, sort_order) VALUES ('A', 1);
             INSERT INTO todo_tasks (group_id, title, sort_order, plan_date, start_date, due_date, task_type, repeat_rule) VALUES
               (1, 'plan only', 1, '2026-10-01', NULL, NULL, 'normal', NULL),
               (1, 'plan then due', 2, '2026-10-01', NULL, '2026-10-05', 'normal', NULL),
               (1, 'plan after due', 3, '2026-10-09', NULL, '2026-10-05', 'normal', NULL),
               (1, 'range kept', 4, NULL, '2026-10-01', '2026-10-02', 'normal', NULL),
               (1, 'weekly', 5, '2026-10-03', NULL, NULL, 'repeat', 'weekly:6'),
               (1, 'checkin', 6, '2026-09-01', NULL, NULL, 'checkin', 'daily');",
        )
        .unwrap();
        migrate(&mut c, &MIGRATIONS[..10], &dir.join("backups")).unwrap();
        let rows: Vec<(Option<String>, Option<String>, Option<String>, i64)> = c
            .prepare("SELECT plan_date, start_date, due_date, done_before FROM todo_tasks ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let s = |v: &str| Some(v.to_string());
        assert_eq!(
            rows,
            [
                (None, s("2026-10-01"), s("2026-10-01"), 0),
                (None, s("2026-10-01"), s("2026-10-05"), 0),
                (None, None, s("2026-10-05"), 0),
                (None, s("2026-10-01"), s("2026-10-02"), 0),
                (None, None, s("2026-10-03"), 0),
                (s("2026-09-01"), None, None, 0),
            ]
        );
    }

    #[test]
    fn v10_checkins_use_ranges_and_monthly_repeats_count() {
        let dir = temp_dir("v10-checkin");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..10], &dir.join("backups")).unwrap();
        c.execute_batch(
            "INSERT INTO todo_groups (name, sort_order) VALUES ('A', 1);
             INSERT INTO todo_tasks (group_id, title, sort_order, plan_date, repeat_until, repeat_total, due_date, task_type, repeat_rule) VALUES
               (1, 'ranged', 1, '2026-09-01', '2026-12-31', 30, NULL, 'checkin', 'daily'),
               (1, 'open', 2, '2026-09-01', NULL, 30, NULL, 'checkin', 'weekly:1'),
               (1, 'rent', 3, '2026-09-01', '2026-12-31', NULL, '2026-10-01', 'repeat', 'monthly:1'),
               (1, 'hair', 4, '2026-09-01', NULL, NULL, '2026-10-01', 'repeat', 'after:30d');",
        )
        .unwrap();
        migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap();
        type R = (Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>);
        let rows: Vec<R> = c
            .prepare("SELECT plan_date, start_date, due_date, repeat_until, repeat_total FROM todo_tasks ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let s = |v: &str| Some(v.to_string());
        assert_eq!(
            rows,
            [
                (None, s("2026-09-01"), s("2026-12-31"), None, None),
                (None, None, None, None, None),
                (None, None, s("2026-10-01"), None, None),
                (s("2026-09-01"), None, s("2026-10-01"), None, None),
            ]
        );
        let n: i64 = c.query_row("SELECT count(*) FROM kinds WHERE module = 'fin_out'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 11);
    }

    /// Phase 2–4 installs are at v13; v14 (念念不忘) only adds tables, backing up first.
    #[test]
    fn v13_upgrades_to_notes() {
        let dir = temp_dir("v13-notes");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..13], &dir.join("backups")).unwrap();
        c.execute_batch("INSERT INTO jobs (company, status_id) VALUES ('X', 1);").unwrap();
        assert_eq!(migrate(&mut c, &MIGRATIONS[..14], &dir.join("backups")).unwrap(), 14);
        c.execute("INSERT INTO notes (content) VALUES ('#想法 一条')", []).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM jobs", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let files: Vec<_> = std::fs::read_dir(dir.join("backups")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(files.len(), 1);
        assert!(files[0].to_str().unwrap().ends_with("-before-v14.db"));
    }

    /// Phase 5 installs are at v14; v15 (芝麻开门) only adds empty tables (no master password yet).
    #[test]
    fn v14_upgrades_to_vault() {
        let dir = temp_dir("v14-vault");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..14], &dir.join("backups")).unwrap();
        c.execute("INSERT INTO notes (content) VALUES ('#想法 一条')", []).unwrap();
        assert_eq!(migrate(&mut c, &MIGRATIONS[..15], &dir.join("backups")).unwrap(), 15);
        let n: i64 = c.query_row("SELECT (SELECT count(*) FROM notes) * 10 + (SELECT count(*) FROM vault_meta)", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 10);
        let files: Vec<_> = std::fs::read_dir(dir.join("backups")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(files[0].to_str().unwrap().ends_with("-before-v15.db"));
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
