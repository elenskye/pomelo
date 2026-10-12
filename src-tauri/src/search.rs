//! 全局搜索 (⌘K): one box over every module except 芝麻开门 (never searched). A record matches when
//! each word of the query appears somewhere in its text (case-insensitive substring). Per source at
//! most `PER_SOURCE` hits; titles that start with the first word come first, then the newest.
//! A hit carries what the UI needs to open it: module + kind + id (and parent / day where a page
//! needs them). 小拾's conversations come from picky.db (picky/find.rs), after the rest.

use crate::modules::common::err;
use crate::modules::notes;
use crate::AppState;
use rusqlite::{params_from_iter, Connection};
use serde::Serialize;
use tauri::State;

pub const PER_SOURCE: usize = 6;
const MAX_WORDS: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    /// Module id (route), e.g. "books".
    pub module: &'static str,
    /// What it is inside the module, e.g. "lesson" (see SOURCES).
    pub kind: &'static str,
    pub id: i64,
    pub title: String,
    /// Where it lives / a short hint (group, course, author…).
    pub detail: Option<String>,
    /// The containing record a page must select first (course, trip, template, todo group / goal…).
    pub parent: Option<i64>,
    /// A date the page navigates by (收支: the entry's day) or shows.
    pub day: Option<String>,
    /// 收支 only: amount in cents, its currency and flow.
    pub amount: Option<i64>,
    pub currency: Option<String>,
    pub flow: Option<String>,
}

/// One searchable source: its SELECT yields, by position, id, title, detail, parent, day, amount,
/// currency, flow, hay (the text the words are looked for in) and ord (bigger = listed first on ties).
struct Source {
    module: &'static str,
    kind: &'static str,
    sql: &'static str,
}

const SOURCES: &[Source] = &[
    Source { module: "todo", kind: "group", sql: "SELECT id, name, NULL, NULL, NULL, NULL, NULL, NULL, name, id FROM todo_groups" },
    Source {
        module: "todo",
        kind: "goal",
        sql: "SELECT g.id, g.name, gr.name, g.group_id, g.end_date, NULL, NULL, NULL,
                g.name || ' ' || coalesce(g.note, '') || ' ' || coalesce(g.review, ''), g.id
              FROM todo_goals g JOIN todo_groups gr ON gr.id = g.group_id",
    },
    Source {
        module: "todo",
        kind: "task",
        // parent = -goal id when in a goal (the page opens that goal), else the group id; undone first
        sql: "SELECT t.id, replace(t.title, '**', ''), coalesce(go.name, gr.name) || CASE WHEN t.done THEN ' · 已完成' ELSE '' END,
                CASE WHEN t.goal_id IS NULL THEN t.group_id ELSE -t.goal_id END, t.due_date, NULL, NULL, NULL,
                t.title || ' ' || coalesce(t.note, ''), (1 - t.done) * 1000000000 + t.id
              FROM todo_tasks t JOIN todo_groups gr ON gr.id = t.group_id LEFT JOIN todo_goals go ON go.id = t.goal_id",
    },
    Source {
        module: "books",
        kind: "book",
        sql: "SELECT id, title, author, NULL, NULL, NULL, NULL, NULL,
                title || ' ' || coalesce(author, '') || ' ' || coalesce(country, '') || ' ' || coalesce(note, ''), id FROM books",
    },
    Source {
        module: "films",
        kind: "film",
        sql: "SELECT id, title, director, NULL, NULL, NULL, NULL, NULL,
                title || ' ' || coalesce(director, '') || ' ' || coalesce(country, '') || ' ' || coalesce(note, ''), id FROM films",
    },
    Source {
        module: "drawing",
        kind: "course",
        sql: "SELECT id, name, NULL, NULL, NULL, NULL, NULL, NULL, name || ' ' || coalesce(note, ''), id FROM drawing_courses",
    },
    Source {
        module: "drawing",
        kind: "lesson",
        sql: "SELECT l.id, coalesce(l.code || ' ', '') || l.title, c.name, l.course_id, NULL, NULL, NULL, NULL,
                coalesce(l.code, '') || ' ' || l.title || ' ' || coalesce(l.note, ''), l.id
              FROM drawing_lessons l JOIN drawing_courses c ON c.id = l.course_id",
    },
    Source {
        module: "packing",
        kind: "trip",
        sql: "SELECT id, name, NULL, NULL, depart_on, NULL, NULL, NULL, name || ' ' || coalesce(note, ''), id FROM trips",
    },
    Source {
        module: "packing",
        kind: "template",
        sql: "SELECT id, name, '模板', NULL, NULL, NULL, NULL, NULL, name || ' ' || coalesce(note, ''), id FROM packing_templates",
    },
    Source {
        module: "packing",
        kind: "trip_item",
        sql: "SELECT i.id, i.name, t.name, i.trip_id, NULL, NULL, NULL, NULL, i.name || ' ' || coalesce(i.note, ''), i.id
              FROM trip_items i JOIN trips t ON t.id = i.trip_id",
    },
    Source {
        module: "packing",
        kind: "template_item",
        sql: "SELECT i.id, i.name, '模板 · ' || t.name, i.template_id, NULL, NULL, NULL, NULL, i.name || ' ' || coalesce(i.note, ''), i.id
              FROM packing_template_items i JOIN packing_templates t ON t.id = i.template_id",
    },
    Source {
        module: "finance",
        kind: "entry",
        sql: "SELECT e.id, coalesce(e.note, k.name, '未分类'), k.name, NULL, e.day, e.amount, e.currency, e.flow,
                coalesce(e.note, '') || ' ' || coalesce(k.name, ''), e.day || printf('%012d', e.id)
              FROM fin_entries e LEFT JOIN kinds k ON k.id = e.kind_id",
    },
    Source {
        module: "finance",
        kind: "fixed",
        sql: "SELECT id, name, '固定支出', NULL, next_day, amount, currency, flow, name || ' ' || coalesce(note, ''), id FROM fin_fixed",
    },
    Source {
        module: "finance",
        kind: "account",
        sql: "SELECT id, name, '账户', NULL, NULL, NULL, currency, NULL, name || ' ' || coalesce(note, ''), id FROM fin_accounts",
    },
    Source {
        module: "jobs",
        kind: "job",
        sql: "SELECT id, company || coalesce(' · ' || position, ''), location, NULL, NULL, NULL, NULL, NULL,
                company || ' ' || coalesce(position, '') || ' ' || coalesce(location, '') || ' ' || coalesce(note, ''), id FROM jobs",
    },
    Source {
        module: "notes",
        kind: "note",
        sql: "SELECT id, content, NULL, NULL, date(created_at, 'localtime'), NULL, NULL, NULL, content, created_at FROM notes",
    },
];

/// Hits for `q`, grouped by source in SOURCES order.
pub fn search(conn: &Connection, q: &str) -> Result<Vec<Hit>, String> {
    let words: Vec<String> = q.split_whitespace().take(MAX_WORDS).map(str::to_lowercase).collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let cond = (1..=words.len()).map(|i| format!("instr(lower(hay), ?{i}) > 0")).collect::<Vec<_>>().join(" AND ");
    let mut out = Vec::new();
    for s in SOURCES {
        let sql = format!(
            "WITH src(id, title, detail, parent, day, amount, currency, flow, hay, ord) AS ({})
             SELECT id, title, detail, parent, day, amount, currency, flow FROM src WHERE {cond}
             ORDER BY instr(lower(title), ?1) = 1 DESC, ord DESC LIMIT {PER_SOURCE}",
            s.sql
        );
        let mut st = conn.prepare(&sql).map_err(err)?;
        let rows = st
            .query_map(params_from_iter(words.iter()), |r| {
                Ok(Hit {
                    module: s.module,
                    kind: s.kind,
                    id: r.get(0)?,
                    title: r.get(1)?,
                    detail: r.get(2)?,
                    parent: r.get(3)?,
                    day: r.get(4)?,
                    amount: r.get(5)?,
                    currency: r.get(6)?,
                    flow: r.get(7)?,
                })
            })
            .map_err(err)?;
        for h in rows {
            let mut h = h.map_err(err)?;
            if s.kind == "note" {
                h.title = notes::excerpt(&h.title);
            }
            out.push(h);
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn search_all(state: State<AppState>, q: String) -> Result<Vec<Hit>, String> {
    let mut hits = state.with_db(|c| search(c, &q))?;
    if state.picky.is_open() {
        hits.extend(state.picky.with_db(|c| crate::picky::find::global(c, &q))?);
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;

    fn titles(hits: &[Hit], kind: &str) -> Vec<String> {
        hits.iter().filter(|h| h.kind == kind).map(|h| h.title.clone()).collect()
    }

    #[test]
    fn searches_every_module_but_the_vault() {
        let c = memory_db();
        c.execute_batch(
            "INSERT INTO books (title, author, note) VALUES ('时间管理', '某人', NULL), ('小王子', '圣埃克苏佩里', '讲时间的书');
             INSERT INTO films (title, director) VALUES ('时间旅行者的妻子', 'X');
             INSERT INTO drawing_courses (name, sort_order) VALUES ('速写', 1);
             INSERT INTO drawing_lessons (course_id, code, title, status, sort_order) VALUES (1, '2B', '时间线构图', 'todo', 1);
             INSERT INTO todo_groups (name, sort_order) VALUES ('学业', 1);
             INSERT INTO todo_goals (group_id, name, sort_order) VALUES (1, '毕业', 1);
             INSERT INTO todo_tasks (group_id, goal_id, title, sort_order, done) VALUES (1, 1, '整理**时间**表', 1, 1), (1, NULL, '时间都去哪了', 2, 0);
             INSERT INTO trips (name) VALUES ('东京');
             INSERT INTO trip_items (trip_id, name, sort_order) VALUES (1, '时间胶囊', 1);
             INSERT INTO fin_entries (flow, amount, currency, day, note) VALUES ('out', 1250, 'SGD', '2026-10-01', '买时间管理的书');
             INSERT INTO jobs (company, position, note) VALUES ('字节', '后端', '面试时间待定');
             INSERT INTO notes (content, created_at) VALUES ('#读书 关于**时间**的想法', '2026-10-01T01:00:00.000Z');",
        )
        .unwrap();
        let hits = search(&c, " 时间 ").unwrap();
        assert_eq!(titles(&hits, "book"), ["时间管理", "小王子"]); // title starting with the word first
        assert_eq!(titles(&hits, "task"), ["时间都去哪了", "整理时间表"]); // undone first, ** dropped
        let task = hits.iter().find(|h| h.title == "整理时间表").unwrap();
        assert_eq!((task.parent, task.detail.as_deref()), (Some(-1), Some("毕业 · 已完成"))); // goal view
        let lesson = hits.iter().find(|h| h.kind == "lesson").unwrap();
        assert_eq!((lesson.title.as_str(), lesson.parent, lesson.detail.as_deref()), ("2B 时间线构图", Some(1), Some("速写")));
        let entry = hits.iter().find(|h| h.kind == "entry").unwrap();
        assert_eq!((entry.day.as_deref(), entry.amount, entry.currency.as_deref()), (Some("2026-10-01"), Some(1250), Some("SGD")));
        assert_eq!(titles(&hits, "note"), ["关于时间的想法"]);
        for k in ["film", "trip_item", "job"] {
            assert_eq!(titles(&hits, k).len(), 1, "{k}");
        }
        // every word must match; case doesn't matter; empty finds nothing
        assert_eq!(titles(&search(&c, "时间 王子").unwrap(), "book"), ["小王子"]);
        c.execute("INSERT INTO books (title) VALUES ('Atomic Habits')", []).unwrap();
        assert_eq!(titles(&search(&c, "atomic").unwrap(), "book"), ["Atomic Habits"]);
        assert!(search(&c, "   ").unwrap().is_empty());
        // capped per source
        for i in 0..10 {
            c.execute("INSERT INTO books (title) VALUES (?1)", [format!("书 {i}")]).unwrap();
        }
        assert_eq!(titles(&search(&c, "书").unwrap(), "book").len(), PER_SOURCE);
    }
}
