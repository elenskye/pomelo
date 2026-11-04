//! picky.db: its own file next to pomelo.db, its own migration chain (`migrations/picky/`) and
//! version, so 柚子 never changes pomelo.db. Backups / restore treat it as store `picky`.
//! `preferences` holds 柚子 settings (JSON values, never undone).

use crate::backup::Store;
use crate::db::{self, Migration};
use crate::modules::common::err;
use rusqlite::{Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;

pub const MIGRATIONS: &[Migration] = &[
    Migration { version: 1, name: "chat", sql: include_str!("../../migrations/picky/0001_chat.sql") },
    Migration { version: 2, name: "library", sql: include_str!("../../migrations/picky/0002_library.sql") },
    Migration { version: 3, name: "instructions", sql: include_str!("../../migrations/picky/0003_instructions.sql") },
    Migration { version: 4, name: "files", sql: include_str!("../../migrations/picky/0004_files.sql") },
    Migration { version: 5, name: "memories", sql: include_str!("../../migrations/picky/0005_memories.sql") },
    Migration { version: 6, name: "queries", sql: include_str!("../../migrations/picky/0006_queries.sql") },
    Migration { version: 7, name: "search_db", sql: include_str!("../../migrations/picky/0007_search_db.sql") },
    Migration { version: 8, name: "run_sources", sql: include_str!("../../migrations/picky/0008_run_sources.sql") },
    Migration { version: 9, name: "cards", sql: include_str!("../../migrations/picky/0009_cards.sql") },
    Migration { version: 10, name: "card_choices", sql: include_str!("../../migrations/picky/0010_card_choices.sql") },
    Migration { version: 11, name: "skills", sql: include_str!("../../migrations/picky/0011_skills.sql") },
    Migration { version: 12, name: "schedules", sql: include_str!("../../migrations/picky/0012_schedules.sql") },
    Migration { version: 13, name: "briefs", sql: include_str!("../../migrations/picky/0013_briefs.sql") },
    Migration { version: 14, name: "year_letter", sql: include_str!("../../migrations/picky/0014_year_letter.sql") },
    Migration { version: 15, name: "thinking", sql: include_str!("../../migrations/picky/0015_thinking.sql") },
    Migration { version: 16, name: "more_files", sql: include_str!("../../migrations/picky/0016_more_files.sql") },
    Migration { version: 17, name: "knowledge", sql: include_str!("../../migrations/picky/0017_knowledge.sql") },
    Migration { version: 18, name: "work", sql: include_str!("../../migrations/picky/0018_work.sql") },
    Migration { version: 19, name: "toolboxes", sql: include_str!("../../migrations/picky/0019_toolboxes.sql") },
    Migration { version: 20, name: "seeker", sql: include_str!("../../migrations/picky/0020_seeker.sql") },
    Migration { version: 21, name: "brief_mood", sql: include_str!("../../migrations/picky/0021_brief_mood.sql") },
    Migration { version: 22, name: "seeker_companies", sql: include_str!("../../migrations/picky/0022_seeker_companies.sql") },
    Migration { version: 23, name: "seeker_english", sql: include_str!("../../migrations/picky/0023_seeker_english.sql") },
    Migration { version: 24, name: "rater", sql: include_str!("../../migrations/picky/0024_rater.sql") },
    Migration { version: 25, name: "rater_practice", sql: include_str!("../../migrations/picky/0025_rater_practice.sql") },
    Migration { version: 26, name: "rater_interviews", sql: include_str!("../../migrations/picky/0026_rater_interviews.sql") },
    Migration { version: 27, name: "record_order", sql: include_str!("../../migrations/picky/0027_record_order.sql") },
    Migration { version: 28, name: "job_resumes", sql: include_str!("../../migrations/picky/0028_job_resumes.sql") },
    Migration { version: 29, name: "no_resume_copies", sql: include_str!("../../migrations/picky/0029_no_resume_copies.sql") },
    Migration { version: 30, name: "seeker_group_order", sql: include_str!("../../migrations/picky/0030_seeker_group_order.sql") },
    Migration { version: 31, name: "rater_records", sql: include_str!("../../migrations/picky/0031_rater_records.sql") },
    Migration { version: 32, name: "effort", sql: include_str!("../../migrations/picky/0032_effort.sql") },
    Migration { version: 33, name: "asks", sql: include_str!("../../migrations/picky/0033_asks.sql") },
    Migration { version: 34, name: "schedule_effort", sql: include_str!("../../migrations/picky/0034_schedule_effort.sql") },
    Migration { version: 35, name: "excel_cards", sql: include_str!("../../migrations/picky/0035_excel_cards.sql") },
    Migration { version: 36, name: "youzi", sql: include_str!("../../migrations/picky/0036_youzi.sql") },
    Migration { version: 37, name: "year_atlas", sql: include_str!("../../migrations/picky/0037_year_atlas.sql") },
];

pub const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// Opens (creating if needed) and migrates picky.db, then marks replies left streaming by a
/// previous run of the app as interrupted.
pub fn open(path: &Path, backups: &Path) -> Result<Connection, String> {
    let mut conn = db::open(path)?;
    db::migrate_as(&mut conn, MIGRATIONS, Some(backups), Store::Picky).map_err(|e| format!("柚子的{e}"))?;
    recover_interrupted(&conn)?;
    Ok(conn)
}

pub fn recover_interrupted(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(&format!(
        "UPDATE messages SET state = 'interrupted' WHERE state = 'streaming';
         UPDATE runs SET state = 'interrupted', ended_at = {NOW} WHERE state = 'running';"
    ))
    .map_err(err)
}

pub fn pref_get<T: DeserializeOwned>(conn: &Connection, key: &str) -> Result<Option<T>, String> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM preferences WHERE key = ?1", [key], |r| r.get(0))
        .optional()
        .map_err(err)?;
    raw.map(|s| serde_json::from_str(&s).map_err(|e| format!("柚子设置 {key} 读不出来：{e}"))).transpose()
}

pub fn pref_set<T: Serialize + ?Sized>(conn: &Connection, key: &str, value: &T) -> Result<(), String> {
    let json = serde_json::to_string(value).map_err(err)?;
    conn.execute(
        &format!("INSERT INTO preferences (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = {NOW}"),
        (key, json),
    )
    .map(|_| ())
    .map_err(err)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A migrated in-memory picky.db.
    pub fn memory_db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        db::configure(&c).unwrap();
        db::migrate_as(&mut c, MIGRATIONS, None, Store::Picky).unwrap();
        c
    }

    #[test]
    fn migrations_are_sequential() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version, i as i64 + 1, "gap or reorder at {}", m.name);
        }
    }

    #[test]
    fn open_creates_file_without_backup_and_upgrade_snapshots_as_picky() {
        let dir = db::tests::temp_dir("picky-open");
        let backups = dir.join("backups");
        let c = open(&dir.join("picky.db"), &backups).unwrap();
        assert_eq!(db::user_version(&c).unwrap(), MIGRATIONS.len() as i64);
        assert!(!backups.exists()); // fresh file: nothing to back up
        drop(c);

        // an older file is snapshotted as picky-…-before-vN before upgrading
        let mut v = db::open(&dir.join("old.db")).unwrap();
        let two = [
            Migration { version: 1, name: MIGRATIONS[0].name, sql: MIGRATIONS[0].sql },
            Migration { version: 2, name: "t", sql: "CREATE TABLE t (a INTEGER);" },
        ];
        db::migrate_as(&mut v, &two[..1], Some(&backups), Store::Picky).unwrap();
        db::migrate_as(&mut v, &two, Some(&backups), Store::Picky).unwrap();
        let names: Vec<String> = std::fs::read_dir(&backups).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names.len(), 1);
        assert!(names[0].starts_with("picky-") && names[0].ends_with("-before-v2.db"), "{names:?}");
    }

    #[test]
    fn newer_file_is_refused_without_touching_it() {
        let dir = db::tests::temp_dir("picky-newer");
        let p = dir.join("picky.db");
        let c = db::open(&p).unwrap();
        c.pragma_update(None, "user_version", 99).unwrap();
        drop(c);
        let e = open(&p, &dir.join("backups")).unwrap_err();
        assert!(e.contains("柚子") && e.contains("新"), "{e}");
    }

    #[test]
    fn interrupted_replies_are_marked_on_open() {
        let dir = db::tests::temp_dir("picky-recover");
        let p = dir.join("picky.db");
        let c = open(&p, &dir.join("backups")).unwrap();
        c.execute_batch(
            "INSERT INTO sessions (id, title) VALUES (1, 'a');
             INSERT INTO runs (id, session_id, model, model_name, state) VALUES (1, 1, 'local:x', 'X', 'running');
             INSERT INTO messages (session_id, run_id, role, content, state) VALUES (1, 1, 'assistant', '半句', 'streaming');",
        )
        .unwrap();
        drop(c);
        let c = open(&p, &dir.join("backups")).unwrap();
        let (m, r): (String, String) =
            c.query_row("SELECT m.state, r.state FROM messages m JOIN runs r ON r.id = m.run_id", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((m.as_str(), r.as_str()), ("interrupted", "interrupted"));
    }

    /// v36: what the user wrote for the model says 柚子; messages, other prefs and Seeker's material don't change.
    #[test]
    fn v35_renames_xiaoshi_in_what_the_user_wrote() {
        let mut c = Connection::open_in_memory().unwrap();
        db::configure(&c).unwrap();
        db::migrate_as(&mut c, &MIGRATIONS[..35], None, Store::Picky).unwrap();
        c.execute_batch(
            r#"INSERT INTO memories (content, noted_on) VALUES ('叫我小柚，小拾说话简短', '2026-10-01'), ('住在上海', '2026-10-01');
             UPDATE global_instructions SET content = '小拾用中文';
             INSERT INTO projects (name, instructions) VALUES ('论文', '小拾先列提纲');
             INSERT INTO sessions (id, title, instructions) VALUES (1, '和小拾聊', '小拾别客套');
             INSERT INTO runs (id, session_id, model, model_name, state) VALUES (1, 1, 'local:x', 'X', 'done');
             INSERT INTO messages (session_id, run_id, role, content, state) VALUES (1, 1, 'assistant', '我是小拾', 'done');
             INSERT INTO templates (title, body) VALUES ('问小拾', '小拾，帮我看看');
             INSERT INTO presets (name, content) VALUES ('小拾写作', '小拾写得短');
             INSERT INTO schedules (name, prompt, every, at) VALUES ('周报', '小拾总结这周', 'week', '09:00');
             INSERT INTO preferences (key, value) VALUES ('brief.prompt', '"你是小拾"'), ('work.folder', '"~/小拾"');
             INSERT INTO seeker_records (id, body) VALUES ('r', '{"kind":"rule","text":"小拾别编"}'), ('m', '{"kind":"material","text":"小拾项目"}'), ('x', 'not json 小拾');"#,
        )
        .unwrap();
        db::migrate_as(&mut c, &MIGRATIONS[..36], None, Store::Picky).unwrap();
        let one = |sql: &str| c.query_row(sql, [], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!(one("SELECT group_concat(content, '|') FROM memories"), "叫我小柚，柚子说话简短|住在上海");
        assert_eq!(one("SELECT content FROM global_instructions"), "柚子用中文");
        assert_eq!(one("SELECT instructions FROM projects"), "柚子先列提纲");
        assert_eq!(one("SELECT title || '/' || instructions FROM sessions"), "和小拾聊/柚子别客套"); // titles are the conversation's
        assert_eq!(one("SELECT content FROM messages"), "我是小拾");
        assert_eq!(one("SELECT title || '/' || body FROM templates"), "问柚子/柚子，帮我看看");
        assert_eq!(one("SELECT name || '/' || content FROM presets"), "柚子写作/柚子写得短");
        assert_eq!(one("SELECT prompt FROM schedules WHERE name = '周报'"), "柚子总结这周");
        assert_eq!(one("SELECT group_concat(value, '|') FROM preferences WHERE key IN ('brief.prompt', 'work.folder') ORDER BY key"), "\"你是柚子\"|\"~/小拾\"");
        assert_eq!(one("SELECT group_concat(body, '|') FROM (SELECT body FROM seeker_records ORDER BY id)"), r#"{"kind":"material","text":"小拾项目"}|{"kind":"rule","text":"柚子别编"}|not json 小拾"#);
    }

    #[test]
    fn prefs_roundtrip() {
        let c = memory_db();
        assert_eq!(pref_get::<String>(&c, "k").unwrap(), None);
        pref_set(&c, "k", "local").unwrap();
        pref_set(&c, "k", "cloud:x").unwrap();
        assert_eq!(pref_get::<String>(&c, "k").unwrap().as_deref(), Some("cloud:x"));
    }
}
