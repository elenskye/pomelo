//! 模块坞 (v0.7 phase 1; requirements「界面外框」): the one line of live text under each module in the
//! 模块坞 and its rows (「今天 3 件」). Read-only; a module with nothing worth saying is left out.
//! Where the dock's categories, pins and hidden modules live: pomelo.db settings `ui.dock` and
//! `ui.module_hidden` (the UI keeps them; not undoable).
//! - 一步一步: due today + overdue (as 小拾's 「今天」); 念念不忘: written today, else all; 来日方长: the
//!   nearest coming day; 钱来钱往: entries this month; 出门走走: the next trip with a date; 书影: in progress,
//!   else wanted; 随意画画: lessons done / all; 投石问路: to follow up, else active, else all; 芝麻开门:
//!   locked / open; made modules: items (五花八门) or lessons done / all (循序渐进).
//! - 小拾: writing, else conversations; Translator: its target; Pub2MD: being done + waiting; Seeker: a
//!   draft. 呦呦漫游 says nothing (its changes need git).

use crate::modules::common::{err, Status};
use crate::modules::{books, custom, days, films, jobs, todo, vault};
use crate::picky::assistants::{pub2md, translate};
use crate::AppState;
use rusqlite::Connection;
use std::collections::BTreeMap;
use tauri::State;

pub type Live = BTreeMap<String, String>;

fn count(conn: &Connection, sql: &str, args: impl rusqlite::Params) -> Result<i64, String> {
    conn.query_row(sql, args, |r| r.get(0)).map_err(err)
}

fn md(day: &str) -> String {
    format!("{} / {}", &day[5..7], &day[8..10])
}

/// pomelo.db's modules (and the made ones) on `today`.
pub fn live(conn: &Connection, today: &str) -> Result<Live, String> {
    let mut out = Live::new();
    let mut put = |k: &str, v: String| {
        out.insert(k.to_string(), v);
    };
    let ag = todo::agenda(conn, today)?;
    let n = ag.due.len() + ag.overdue.len();
    if n > 0 {
        put("todo", format!("今天 {n} 件"));
    }
    // created_at is UTC: a local day is compared as local time
    let today_notes = count(conn, "SELECT count(*) FROM notes WHERE date(created_at, 'localtime') = ?1", [today])?;
    let all_notes = count(conn, "SELECT count(*) FROM notes", [])?;
    if today_notes > 0 {
        put("notes", format!("今天 {today_notes} 条"));
    } else if all_notes > 0 {
        put("notes", format!("共 {all_notes} 条"));
    }
    if let Some(d) = days::list(conn, today)?.into_iter().filter(|d| d.days >= 0).min_by_key(|d| d.days) {
        put("days", if d.days == 0 { format!("{} 今天", d.name) } else { format!("{} {} 天", d.name, d.days) });
    }
    let entries = count(conn, "SELECT count(*) FROM fin_entries WHERE substr(day, 1, 7) = ?1", [&today[..7]])?;
    if entries > 0 {
        put("finance", format!("本月 {entries} 笔"));
    }
    let trip: Option<(String, String)> = conn
        .query_row("SELECT name, depart_on FROM trips WHERE depart_on >= ?1 ORDER BY depart_on, id LIMIT 1", [today], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok();
    if let Some((name, on)) = trip {
        put("packing", format!("{name} {}", md(&on)));
    }
    let shelf = |statuses: Vec<Status>, doing: &str, todo: &str| -> Option<String> {
        let (a, b) = (statuses.iter().filter(|s| **s == Status::Doing).count(), statuses.iter().filter(|s| **s == Status::Todo).count());
        if a > 0 {
            Some(format!("{doing} {a}"))
        } else if b > 0 {
            Some(format!("{todo} {b}"))
        } else {
            None
        }
    };
    if let Some(t) = shelf(books::list(conn)?.into_iter().map(|b| b.status).collect(), "在读", "想读") {
        put("books", format!("{t} 本"));
    }
    if let Some(t) = shelf(films::list(conn)?.into_iter().map(|f| f.status).collect(), "在看", "想看") {
        put("films", format!("{t} 部"));
    }
    let (done, total) = conn
        .query_row("SELECT count(*) FILTER (WHERE status = 'done'), count(*) FROM drawing_lessons", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
        .map_err(err)?;
    if total > 0 {
        put("drawing", format!("{done} / {total} 节"));
    }
    let js = jobs::jobs(conn, today)?;
    let follow = js.iter().filter(|j| j.follow_up).count();
    let active = js.iter().filter(|j| j.stage == jobs::Stage::Active).count();
    if follow > 0 {
        put("jobs", format!("该跟进 {follow}"));
    } else if active > 0 {
        put("jobs", format!("进行中 {active}"));
    } else if !js.is_empty() {
        put("jobs", format!("共 {} 个", js.len()));
    }
    for m in custom::all(conn)? {
        match m.template {
            custom::Template::List => put(&m.key, format!("{} 条", m.count)),
            custom::Template::Progress => {
                let (done, total) = conn
                    .query_row(
                        "SELECT count(*) FILTER (WHERE l.status = 'done'), count(*) FROM progress_lessons l JOIN progress_courses c ON c.id = l.course_id WHERE c.module_id = ?1",
                        [m.id],
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
                    )
                    .map_err(err)?;
                put(&m.key, format!("{done} / {total}"))
            }
        }
    }
    Ok(out)
}

/// picky.db's part: conversations and the 助手 (`generating` / `pub2md_busy` come from memory).
pub fn picky_live(conn: &Connection, generating: bool, pub2md_busy: usize) -> Result<Live, String> {
    let mut out = Live::new();
    let sessions = count(conn, "SELECT count(*) FROM sessions", [])?;
    if generating {
        out.insert("picky".into(), "正在写…".into());
    } else if sessions > 0 {
        out.insert("picky".into(), format!("{sessions} 个对话"));
    }
    let target = translate::load(conn).settings.target;
    let to = if target == translate::ZH_EN { "中英互译".to_string() } else { translate::LANGS.iter().find(|l| l.code == target).map_or(String::new(), |l| format!("译成{}", l.name)) };
    if !to.is_empty() {
        out.insert("translate".into(), to);
    }
    if pub2md_busy > 0 {
        out.insert("pub2md".into(), format!("{pub2md_busy} 份在做"));
    }
    if count(conn, "SELECT count(*) FROM seeker_draft", [])? > 0 {
        out.insert("seeker".into(), "有草稿".into());
    }
    let questions = count(conn, "SELECT count(*) FROM rater_questions", [])?;
    if questions > 0 {
        out.insert("rater".into(), format!("{questions} 道题"));
    }
    Ok(out)
}

#[tauri::command]
pub async fn dock_live(state: State<'_, AppState>) -> Result<Live, String> {
    let mut out = state.with_db(|c| {
        let today = todo::today(c)?;
        live(c, &today)
    })?;
    // separate locks, one after the other (never nested)
    let open = state.with_db(|c| Ok(vault::status(c, &mut *state.vault()?)?.state == "open"))?;
    out.insert("vault".into(), if open { "已打开".into() } else { "已锁上".into() });
    let generating = state.picky.runs.busy();
    let busy = pub2md::busy(&state.picky.assistants.pub2md);
    if let Ok(p) = state.picky.with_db(|c| picky_live(c, generating, busy)) {
        out.extend(p);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;

    const TODAY: &str = "2026-10-08";

    #[test]
    fn live_text_says_only_what_is_there() {
        let c = memory_db();
        // nothing yet: nothing to say
        assert!(live(&c, TODAY).unwrap().is_empty());
        c.execute_batch(
            "INSERT INTO books (title, status) VALUES ('甲', 'doing'), ('乙', 'todo');
             INSERT INTO films (title, status) VALUES ('丙', 'todo');
             INSERT INTO trips (name, depart_on) VALUES ('东京', '2026-11-08'), ('去年', '2025-01-01');
             INSERT INTO notes (content, created_at) VALUES ('旧', '2026-01-01T00:00:00.000Z');",
        )
        .unwrap();
        let l = live(&c, TODAY).unwrap();
        assert_eq!(l.get("books").map(String::as_str), Some("在读 1 本"));
        assert_eq!(l.get("films").map(String::as_str), Some("想看 1 部"));
        assert_eq!(l.get("packing").map(String::as_str), Some("东京 11 / 08"));
        assert_eq!(l.get("notes").map(String::as_str), Some("共 1 条"));
        assert!(!l.contains_key("todo") && !l.contains_key("jobs"));
    }

    #[test]
    fn picky_live_text() {
        let c = crate::picky::store::tests::memory_db();
        let l = picky_live(&c, false, 0).unwrap();
        assert_eq!(l.get("translate").map(String::as_str), Some("中英互译"));
        assert!(!l.contains_key("picky") && !l.contains_key("pub2md") && !l.contains_key("seeker") && !l.contains_key("rater"));
        c.execute("INSERT INTO rater_questions (id, body) VALUES ('q', '{}')", []).unwrap();
        let l = picky_live(&c, true, 2).unwrap();
        assert_eq!((l["picky"].as_str(), l["pub2md"].as_str(), l["rater"].as_str()), ("正在写…", "2 份在做", "1 道题"));
    }
}
