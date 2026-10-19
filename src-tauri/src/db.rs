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
    Migration { version: 16, name: "goal_no_abandon", sql: include_str!("../migrations/0016_goal_no_abandon.sql") },
    Migration { version: 17, name: "finished_year", sql: include_str!("../migrations/0017_finished_year.sql") },
    Migration { version: 18, name: "finance_days", sql: include_str!("../migrations/0018_finance_days.sql") },
    Migration { version: 19, name: "job_steps", sql: include_str!("../migrations/0019_job_steps.sql") },
    Migration { version: 20, name: "note_tag_meta", sql: include_str!("../migrations/0020_note_tag_meta.sql") },
    Migration { version: 21, name: "rereads", sql: include_str!("../migrations/0021_rereads.sql") },
    Migration { version: 22, name: "root_pins", sql: include_str!("../migrations/0022_root_pins.sql") },
    Migration { version: 23, name: "vault_order", sql: include_str!("../migrations/0023_vault_order.sql") },
    Migration { version: 24, name: "vault_groups", sql: include_str!("../migrations/0024_vault_groups.sql") },
    Migration { version: 25, name: "attachments", sql: include_str!("../migrations/0025_attachments.sql") },
    Migration { version: 26, name: "days", sql: include_str!("../migrations/0026_days.sql") },
    Migration { version: 27, name: "custom", sql: include_str!("../migrations/0027_custom.sql") },
    Migration { version: 28, name: "drawing_status", sql: include_str!("../migrations/0028_drawing_status.sql") },
    Migration { version: 29, name: "task_from_note", sql: include_str!("../migrations/0029_task_from_note.sql") },
    Migration { version: 30, name: "custom_year_kinds", sql: include_str!("../migrations/0030_custom_year_kinds.sql") },
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
    migrate_as(conn, migrations, backups_dir, backup::Store::Pomelo)
}

/// Same for either database; `store` names the pre-upgrade snapshot (`picky-…-before-v2.db`).
pub fn migrate_as(conn: &mut Connection, migrations: &[Migration], backups_dir: Option<&Path>, store: backup::Store) -> Result<i64, String> {
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
        backup::snapshot_of(store, conn, dir, &format!("before-v{}", first.version))?;
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

    /// Fresh dir under $TMPDIR, removed on drop (also on panic).
    pub struct TempDir(PathBuf);

    impl std::ops::Deref for TempDir {
        type Target = std::path::Path;
        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl AsRef<std::path::Path> for TempDir {
        fn as_ref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub fn temp_dir(name: &str) -> TempDir {
        let d = std::env::temp_dir().join(format!("pomelo-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        TempDir(d)
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

    /// v16: 「已放弃」 goals become 暂停; the rest stay.
    #[test]
    fn v15_abandoned_goals_become_paused() {
        let dir = temp_dir("v15-goals");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..15], &dir.join("backups")).unwrap();
        c.execute_batch(
            "INSERT INTO todo_groups (name, sort_order) VALUES ('A', 1);
             INSERT INTO todo_goals (group_id, name, status, sort_order) VALUES (1, '放弃了', 'abandoned', 1), (1, '做完了', 'done', 2);",
        )
        .unwrap();
        assert_eq!(migrate(&mut c, &MIGRATIONS[..16], &dir.join("backups")).unwrap(), 16);
        let s: Vec<String> = c.prepare("SELECT status FROM todo_goals ORDER BY id").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(s, ["paused", "done"]);
    }

    #[test]
    fn v16_years_and_dated_finance() {
        let dir = temp_dir("v16-finance");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..16], &dir.join("backups")).unwrap();
        // timestamps at noon UTC keep the local day the same in any timezone from UTC-11 to UTC+11
        c.execute_batch(
            "INSERT INTO books (title, status, finished_month) VALUES ('读完', 'done', '2025-11'), ('没读', 'todo', NULL);
             INSERT INTO films (title, status, finished_month) VALUES ('看完', 'done', '2026-01');
             INSERT INTO fin_accounts (name, kind, currency, sort_order) VALUES ('招行', 'bank', 'CNY', 1), ('嘉信', 'broker', 'USD', 2);
             INSERT INTO fin_balances (account_id, month, amount, updated_at) VALUES
               (1, '2026-09', 100000, '2026-10-07T12:00:00.000Z'),  -- saved after the month → its last day
               (1, '2026-10', 120000, '2026-10-07T12:00:00.000Z'),  -- saved inside the month → that day
               (2, '2026-10', 500000, '2026-09-20T12:00:00.000Z');  -- saved before the month → its first day
             INSERT INTO fin_rates (month, currency, rate, updated_at) VALUES ('2026-10', 'USD', 7.1, '2026-10-07T12:00:00.000Z');
             INSERT INTO fin_holdings (account_id, month, name, quantity, value, sort_order, updated_at) VALUES
               (2, '2026-10', 'VOO', 4, 200000, 1, '2026-10-07T12:00:00.000Z'), (2, '2026-10', '现金', NULL, 12345, 2, '2026-10-07T12:00:00.000Z');
             INSERT INTO fin_fixed (name, amount, currency, account_id, rule, next_day, sort_order) VALUES ('房租', 150000, 'CNY', 1, 'monthly:1', '2026-11-01', 1);
             INSERT INTO fin_entries (flow, amount, currency, day, account_id, fixed_id, note) VALUES ('out', 150000, 'CNY', '2026-10-01', 1, 1, '房租');",
        )
        .unwrap();
        assert_eq!(migrate(&mut c, &MIGRATIONS[..18], &dir.join("backups")).unwrap(), 18);
        let col = |sql: &str| -> Vec<String> { c.prepare(sql).unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect() };
        assert_eq!(col("SELECT coalesce(finished_year, '-') FROM books ORDER BY id"), ["2025", "-"]);
        assert_eq!(col("SELECT finished_year FROM films"), ["2026"]);
        assert_eq!(col("SELECT day FROM fin_balances ORDER BY id"), ["2026-09-30", "2026-10-07", "2026-10-01"]);
        assert_eq!(col("SELECT day FROM fin_rates"), ["2026-10-07"]);
        let h: Vec<(String, f64, f64)> =
            c.prepare("SELECT day, quantity, price FROM fin_holdings ORDER BY sort_order").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(h, [("2026-10-07".into(), 4.0, 500.0), ("2026-10-07".into(), 1.0, 123.45)]);
        // the entry keeps its 固定支出 link (FK now points at the rebuilt fin_fixed); accounts are gone
        assert_eq!(col("SELECT f.name FROM fin_entries e JOIN fin_fixed f ON f.id = e.fixed_id"), ["房租"]);
        assert_eq!(col("SELECT \"table\" FROM pragma_foreign_key_list('fin_entries') ORDER BY 1"), ["fin_fixed", "kinds"]);
        assert_eq!(col("SELECT name FROM pragma_table_info('fin_fixed') WHERE name = 'account_id'").len(), 0);
        c.execute("DELETE FROM fin_fixed", []).unwrap();
        assert_eq!(col("SELECT typeof(fixed_id) FROM fin_entries"), ["null"]);
        let fk: i64 = c.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 0);
    }

    #[test]
    fn v18_jobs_become_steps() {
        let dir = temp_dir("v18-jobs");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..18], &dir.join("backups")).unwrap();
        // default statuses: 1 想投 · 2 已投递 · 3 网申测评 / 笔试 · 4 面试 · 5 Offer · 6 已接受 · 7 被拒
        c.execute_batch(
            "INSERT INTO todo_groups (name, sort_order) VALUES ('💼 求职', 1);
             INSERT INTO todo_tasks (group_id, title, sort_order) VALUES (1, '面试 B', 1);
             INSERT INTO jobs (company, status_id, reached_id, deadline, salary, note) VALUES ('A', 1, 1, '2026-10-20', '20k', NULL);
             INSERT INTO jobs (company, status_id, reached_id, applied_on, step_on, task_id, note) VALUES ('B', 4, 4, '2026-10-01', '2026-10-09', 1, '内推');
             INSERT INTO jobs (company, status_id, reached_id, applied_on, cover_letter) VALUES ('C', 7, 2, '2026-09-01', '~/cl.docx');",
        )
        .unwrap();
        assert_eq!(migrate(&mut c, &MIGRATIONS[..19], &dir.join("backups")).unwrap(), 19);
        let steps = |company: &str| -> Vec<(i64, i64, Option<String>)> {
            c.prepare("SELECT st.status_id, st.done, st.done_on FROM job_steps st JOIN jobs j ON j.id = st.job_id WHERE j.company = ?1 ORDER BY st.status_id")
                .unwrap()
                .query_map([company], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        let s = |d: &str| Some(d.to_string());
        assert_eq!(steps("A"), [(2, 0, None), (3, 0, None), (4, 0, None), (5, 0, None), (6, 0, None)]); // 想投: nothing checked
        assert_eq!(steps("B"), [(2, 1, s("2026-10-01")), (3, 1, None), (4, 1, s("2026-10-09")), (5, 0, None), (6, 0, None)]);
        assert_eq!(steps("C"), [(2, 1, s("2026-09-01")), (3, 0, None), (4, 0, None), (5, 0, None), (6, 0, None), (7, 1, None)]);
        let notes: Vec<Option<String>> = c.prepare("SELECT note FROM jobs ORDER BY id").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(notes, [s("截止日期：2026-10-20\n薪资：20k"), s("内推"), s("求职信：~/cl.docx")]);
        // the linked task stays as an ordinary task; steps go with their application
        let n: i64 = c.query_row("SELECT count(*) FROM todo_tasks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        c.execute("DELETE FROM jobs WHERE company = 'C'", []).unwrap();
        assert!(steps("C").is_empty());
        let fk: i64 = c.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 0);
    }

    /// v20 (标签置顶 / 图标) and v21 (重读重看) only add tables.
    #[test]
    fn v19_upgrades_to_tag_meta_and_rereads() {
        let dir = temp_dir("v19-rereads");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..19], &dir.join("backups")).unwrap();
        c.execute_batch("INSERT INTO books (title) VALUES ('A'); INSERT INTO notes (content) VALUES ('#读书 一条');").unwrap();
        assert_eq!(migrate(&mut c, &MIGRATIONS[..21], &dir.join("backups")).unwrap(), 21);
        c.execute_batch(
            "INSERT INTO book_rereads (book_id, year, rating, sort_order) VALUES (1, '2026', 5, 1);
             INSERT INTO note_tag_meta (tag, pin_order, icon) VALUES ('读书', 1, '📖');",
        )
        .unwrap();
        c.execute("DELETE FROM books", []).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM book_rereads", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0); // rereads go with their book
        let files: Vec<_> = std::fs::read_dir(dir.join("backups")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(files.iter().any(|f| f.to_str().unwrap().ends_with("-before-v20.db")));
    }

    #[test]
    fn v21_unpins_sub_tags() {
        let dir = temp_dir("v21-pins");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..21], &dir.join("backups")).unwrap();
        c.execute_batch("INSERT INTO note_tag_meta (tag, pin_order, icon) VALUES ('读书', 1, NULL), ('读书/摘抄', 2, '📖'), ('a/b', 3, NULL);").unwrap();
        assert_eq!(migrate(&mut c, &MIGRATIONS[..22], &dir.join("backups")).unwrap(), 22);
        let rows: Vec<(String, Option<i64>, Option<String>)> =
            c.prepare("SELECT tag, pin_order, icon FROM note_tag_meta ORDER BY tag").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(rows, [("读书".into(), Some(1), None), ("读书/摘抄".into(), None, Some("📖".into()))]);
    }

    /// v30: 五花八门 日期 fields become 年份 (values keep the year, other values as they were);
    /// 循序渐进 lessons get a kind.
    #[test]
    fn v29_dates_become_years() {
        let dir = temp_dir("v29-years");
        let mut c = open(&dir.join("pomelo.db")).unwrap();
        migrate(&mut c, &MIGRATIONS[..29], &dir.join("backups")).unwrap();
        c.execute_batch(
            r#"INSERT INTO custom_modules (template, name, icon) VALUES ('list', '游戏', 'x'), ('list', '电影', 'x');
             INSERT INTO list_fields (module_id, name, type, options, sort_order) VALUES (1, '平台', 'select', '["Switch"]', 1), (1, '玩过', 'date', '[]', 2), (1, '通关', 'check', '[]', 3), (1, '标签', 'multi', '["a","b"]', 4), (2, '时长', 'number', '[]', 1);
             INSERT INTO list_items (module_id, name, data, sort_order) VALUES (1, '塞尔达', '{"1":"Switch","2":"2025-03-04","3":true,"4":["a","b"]}', 1), (1, '空', '{}', 2), (2, '千与千寻', '{"5":12.5}', 1);"#,
        )
        .unwrap();
        assert_eq!(migrate(&mut c, MIGRATIONS, &dir.join("backups")).unwrap(), latest_version());
        let ty: String = c.query_row("SELECT type FROM list_fields WHERE id = 2", [], |r| r.get(0)).unwrap();
        assert_eq!(ty, "year");
        let data: Vec<String> = c.prepare("SELECT data FROM list_items ORDER BY id").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(data, [r#"{"1":"Switch","2":"2025","3":true,"4":["a","b"]}"#, "{}", r#"{"5":12.5}"#]);
        assert!(c.execute("INSERT INTO list_fields (module_id, name, type, sort_order) VALUES (1, 'x', 'date', 9)", []).is_err());
        c.execute_batch("INSERT INTO custom_modules (template, name, icon) VALUES ('progress', '日语', 'x'); INSERT INTO progress_courses (module_id, name, sort_order) VALUES (3, 'N5', 1); INSERT INTO kinds (module, name, sort_order) VALUES ('progress/3', '语法', 1); INSERT INTO progress_lessons (course_id, title, sort_order, kind_id) VALUES (1, '五十音', 1, (SELECT id FROM kinds WHERE module = 'progress/3'));").unwrap();
        c.execute("DELETE FROM kinds WHERE module = 'progress/3'", []).unwrap();
        let k: Option<i64> = c.query_row("SELECT kind_id FROM progress_lessons", [], |r| r.get(0)).unwrap();
        assert_eq!(k, None); // 未分类
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
