//! 首页 (simple version): one read-only summary over the modules, so the page (and later AI) asks once.
//! Modules hidden in the 模块坞 are passed in and left out (their parts are None / skipped).
//! 芝麻开门 never appears. 今天的待办 isn't here: the page uses todo_agenda (same list as 一步一步 → 今天).
//! - 今年: 读完 / 看完 = books / films finished this year (their 完成时间, or a 重读 / 重看 in this year),
//!   each counted once; 画完 = lessons done with a 完成时间 in this year; 完成 = what 回看 counts for
//!   the year (checked tasks + logs); 笔记 = notes written this year; 投出 = 投递日期 in this year.
//! - 总资产: the days with a full CNY total (finance::assets), oldest first.
//! - 求职: 本周投了 / 进行中 / 拿到 as in 投石问路 → 统计; 该跟进 (longest wait first) and the ones at
//!   a 进行中-stage status (笔试、面试…).
//! - 在读在看: books 在读, then films 在看, latest touched first.
//! - 绘画: unfinished courses (with lessons) in order, each with its next lesson (one in progress,
//!   else the first not done).
//! - 快到的日子 (来日方长): what days.rs marks `soon` (a reminder's window, expired 证件), in its order.
//! - 最近: the newest `RECENT` of notes, books, films, courses, lessons, trips, jobs and 收支 added,
//!   and tasks checked off (books / lessons keep no finish time, so they count when added).

use crate::modules::common::{self, err};
use crate::modules::jobs::{self, Stage};
use crate::modules::{days, drawing, finance, notes, todo};
use crate::search::Hit;
use crate::AppState;
use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

pub const RECENT: usize = 8;

fn hit(module: &'static str, kind: &'static str, id: i64, title: String, detail: Option<String>, parent: Option<i64>) -> Hit {
    Hit { module: module.into(), kind, id, title, detail, parent, day: None, amount: None, currency: None, flow: None }
}

/// 今年的数字; None = module hidden.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Year {
    pub year: String,
    pub books: Option<i64>,
    pub films: Option<i64>,
    pub lessons: Option<i64>,
    pub todos: Option<i64>,
    pub notes: Option<i64>,
    pub jobs: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub day: String,
    /// CNY cents.
    pub total: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobLine {
    #[serde(flatten)]
    pub hit: Hit,
    /// Current status name.
    pub status: String,
    pub stage: Stage,
    pub waiting_days: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobsCard {
    pub total: i64,
    pub this_week: i64,
    /// Stage 等回音 or 进行中.
    pub ongoing: i64,
    pub got: i64,
    pub follow: Vec<JobLine>,
    pub active: Vec<JobLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseLine {
    pub id: i64,
    pub name: String,
    pub total: i64,
    pub done: i64,
    pub doing: i64,
    /// The lesson to open (in progress, else the first not done).
    pub next: Option<Hit>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recent {
    /// UTC timestamp.
    pub at: String,
    /// A task checked off (else something added).
    pub done: bool,
    #[serde(flatten)]
    pub hit: Hit,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub today: String,
    pub year: Year,
    pub assets: Option<Vec<Point>>,
    pub jobs: Option<JobsCard>,
    /// Books then films; None when both are hidden.
    pub reading: Option<Vec<Hit>>,
    pub drawing: Option<Vec<CourseLine>>,
    /// 快到的日子; None = 来日方长 hidden.
    pub days: Option<Vec<days::Day>>,
    pub recent: Vec<Recent>,
}

fn count(conn: &Connection, sql: &str, year: &str) -> Result<i64, String> {
    conn.query_row(sql, [year], |r| r.get(0)).map_err(err)
}

fn year_numbers(conn: &Connection, year: &str, shown: &impl Fn(&str) -> bool, job_list: Option<&[jobs::Job]>) -> Result<Year, String> {
    let finished = |table: &str, rereads: &str, fk: &str| {
        format!(
            "SELECT count(*) FROM {table} x WHERE (x.status = 'done' AND x.finished_year = ?1)
               OR EXISTS (SELECT 1 FROM {rereads} r WHERE r.{fk} = x.id AND r.year = ?1)"
        )
    };
    let when = |module: &str, sql: &str| -> Result<Option<i64>, String> { if shown(module) { count(conn, sql, year).map(Some) } else { Ok(None) } };
    Ok(Year {
        year: year.to_string(),
        books: when("books", &finished("books", "book_rereads", "book_id"))?,
        films: when("films", &finished("films", "film_rereads", "film_id"))?,
        lessons: when("drawing", "SELECT count(*) FROM drawing_lessons WHERE status = 'done' AND substr(finished_month, 1, 4) = ?1")?,
        // = todo::done_between(year).len()
        todos: when(
            "todo",
            "SELECT (SELECT count(*) FROM todo_tasks WHERE done = 1 AND strftime('%Y', done_at, 'localtime') = ?1)
                  + (SELECT count(*) FROM todo_logs WHERE substr(day, 1, 4) = ?1)",
        )?,
        notes: when("notes", "SELECT count(*) FROM notes WHERE strftime('%Y', created_at, 'localtime') = ?1")?,
        jobs: job_list.map(|js| js.iter().filter(|j| j.applied_on.as_deref().is_some_and(|d| d.starts_with(year))).count() as i64),
    })
}

fn jobs_card(conn: &Connection, js: &[jobs::Job], today: &str) -> Result<JobsCard, String> {
    let sts = jobs::statuses(conn)?;
    let numbers = jobs::stats_of(js, &sts, today);
    let line = |j: &jobs::Job| {
        let title = match &j.position {
            Some(p) => format!("{} · {p}", j.company),
            None => j.company.clone(),
        };
        JobLine {
            hit: hit("jobs", "job", j.id, title, j.location.clone(), None),
            status: j.status_id.and_then(|id| sts.iter().find(|s| s.id == id)).map(|s| s.name.clone()).unwrap_or_default(),
            stage: j.stage,
            waiting_days: j.waiting_days,
        }
    };
    let mut follow: Vec<&jobs::Job> = js.iter().filter(|j| j.follow_up).collect();
    follow.sort_by_key(|j| std::cmp::Reverse(j.waiting_days));
    Ok(JobsCard {
        total: js.len() as i64,
        this_week: numbers.this_week,
        ongoing: numbers.ongoing,
        got: numbers.got,
        follow: follow.into_iter().map(line).collect(),
        active: js.iter().filter(|j| j.stage == Stage::Active).map(line).collect(),
    })
}

fn reading(conn: &Connection, shown: &impl Fn(&str) -> bool) -> Result<Vec<Hit>, String> {
    let mut out = Vec::new();
    for (module, kind, table, by) in [("books", "book", "books", "author"), ("films", "film", "films", "director")] {
        if !shown(module) {
            continue;
        }
        let mut st = conn
            .prepare(&format!("SELECT id, title, {by} FROM {table} WHERE status = 'doing' ORDER BY updated_at DESC, id DESC"))
            .map_err(err)?;
        let rows = st.query_map([], |r| Ok(hit(module, kind, r.get(0)?, r.get(1)?, r.get(2)?, None))).map_err(err)?;
        out.extend(rows.collect::<Result<Vec<_>, _>>().map_err(err)?);
    }
    Ok(out)
}

fn drawing_card(conn: &Connection) -> Result<Vec<CourseLine>, String> {
    let mut st = conn
        .prepare(
            "SELECT id, coalesce(code || ' ', '') || title FROM drawing_lessons
             WHERE course_id = ?1 AND status != 'done' ORDER BY status = 'doing' DESC, sort_order, id LIMIT 1",
        )
        .map_err(err)?;
    let mut out = Vec::new();
    for c in drawing::courses(conn)? {
        if c.total == 0 || c.done == c.total {
            continue;
        }
        let next = st
            .query_map([c.id], |r| Ok(hit("drawing", "lesson", r.get(0)?, r.get(1)?, Some(c.name.clone()), Some(c.id))))
            .map_err(err)?
            .next()
            .transpose()
            .map_err(err)?;
        out.push(CourseLine { id: c.id, name: c.name, total: c.total, done: c.done, doing: c.doing, next });
    }
    Ok(out)
}

/// One 最近 source: its SELECT yields, by position, id, title, detail, parent, day, amount, currency,
/// flow, at (UTC timestamp).
struct Source {
    module: &'static str,
    kind: &'static str,
    done: bool,
    sql: &'static str,
}

const SOURCES: &[Source] = &[
    Source { module: "notes", kind: "note", done: false, sql: "SELECT id, content, NULL, NULL, date(created_at, 'localtime'), NULL, NULL, NULL, created_at FROM notes" },
    Source {
        module: "todo",
        kind: "task",
        done: true,
        // parent as in search.rs: -goal id inside a goal, else the group id
        sql: "SELECT t.id, replace(t.title, '**', ''), coalesce(go.name, gr.name), CASE WHEN t.goal_id IS NULL THEN t.group_id ELSE -t.goal_id END,
                NULL, NULL, NULL, NULL, t.done_at
              FROM todo_tasks t JOIN todo_groups gr ON gr.id = t.group_id LEFT JOIN todo_goals go ON go.id = t.goal_id
              WHERE t.done = 1 AND t.done_at IS NOT NULL",
    },
    Source { module: "books", kind: "book", done: false, sql: "SELECT id, title, author, NULL, NULL, NULL, NULL, NULL, created_at FROM books" },
    Source { module: "films", kind: "film", done: false, sql: "SELECT id, title, director, NULL, NULL, NULL, NULL, NULL, created_at FROM films" },
    Source { module: "drawing", kind: "course", done: false, sql: "SELECT id, name, NULL, NULL, NULL, NULL, NULL, NULL, created_at FROM drawing_courses" },
    Source {
        module: "drawing",
        kind: "lesson",
        done: false,
        sql: "SELECT l.id, coalesce(l.code || ' ', '') || l.title, c.name, l.course_id, NULL, NULL, NULL, NULL, l.created_at
              FROM drawing_lessons l JOIN drawing_courses c ON c.id = l.course_id",
    },
    Source { module: "packing", kind: "trip", done: false, sql: "SELECT id, name, NULL, NULL, depart_on, NULL, NULL, NULL, created_at FROM trips" },
    Source {
        module: "jobs",
        kind: "job",
        done: false,
        sql: "SELECT id, company || coalesce(' · ' || position, ''), location, NULL, NULL, NULL, NULL, NULL, created_at FROM jobs",
    },
    Source {
        module: "finance",
        kind: "entry",
        done: false,
        sql: "SELECT e.id, coalesce(e.note, k.name, '未分类'), k.name, NULL, e.day, e.amount, e.currency, e.flow, e.created_at
              FROM fin_entries e LEFT JOIN kinds k ON k.id = e.kind_id",
    },
];

fn recent(conn: &Connection, shown: &impl Fn(&str) -> bool) -> Result<Vec<Recent>, String> {
    let mut out = Vec::new();
    for s in SOURCES.iter().filter(|s| shown(s.module)) {
        let sql = format!(
            "WITH src(id, title, detail, parent, day, amount, currency, flow, at) AS ({})
             SELECT * FROM src ORDER BY at DESC, id DESC LIMIT {RECENT}",
            s.sql
        );
        let mut st = conn.prepare(&sql).map_err(err)?;
        let rows = st
            .query_map([], |r| {
                Ok(Recent {
                    at: r.get(8)?,
                    done: s.done,
                    hit: Hit {
                        module: s.module.into(),
                        kind: s.kind,
                        id: r.get(0)?,
                        title: r.get(1)?,
                        detail: r.get(2)?,
                        parent: r.get(3)?,
                        day: r.get(4)?,
                        amount: r.get(5)?,
                        currency: r.get(6)?,
                        flow: r.get(7)?,
                    },
                })
            })
            .map_err(err)?;
        for row in rows {
            let mut x = row.map_err(err)?;
            if s.kind == "note" {
                x.hit.title = notes::excerpt(&x.hit.title);
            }
            out.push(x);
        }
    }
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out.truncate(RECENT);
    Ok(out)
}

pub fn overview(conn: &Connection, today: &str, hidden: &[String]) -> Result<Overview, String> {
    common::check_date(&Some(today.to_string()), "日期")?;
    let shown = |m: &str| !hidden.iter().any(|h| h == m);
    let job_list = if shown("jobs") { Some(jobs::jobs(conn, today)?) } else { None };
    let assets = if shown("finance") {
        Some(finance::assets(conn)?.totals.into_iter().filter_map(|t| t.total_cny.map(|total| Point { day: t.day, total })).collect())
    } else {
        None
    };
    Ok(Overview {
        today: today.to_string(),
        year: year_numbers(conn, &today[..4], &shown, job_list.as_deref())?,
        assets,
        jobs: job_list.as_deref().map(|js| jobs_card(conn, js, today)).transpose()?,
        reading: if shown("books") || shown("films") { Some(reading(conn, &shown)?) } else { None },
        drawing: if shown("drawing") { Some(drawing_card(conn)?) } else { None },
        days: if shown("days") { Some(days::soon(conn, today)?) } else { None },
        recent: recent(conn, &shown)?,
    })
}

#[tauri::command]
pub fn home_overview(state: State<AppState>, hidden: Vec<String>) -> Result<Overview, String> {
    state.with_db(|c| {
        let today = todo::today(c)?;
        overview(c, &today, &hidden)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;

    const TODAY: &str = "2026-10-08"; // a Thursday

    fn seed(c: &Connection) {
        c.execute_batch(
            "INSERT INTO books (title, author, status, finished_year, created_at) VALUES
               ('读完了', 'A', 'done', '2026', '2026-01-01T00:00:00.000Z'),
               ('去年读的', 'B', 'done', '2025', '2025-01-01T00:00:00.000Z'),
               ('今年重读', 'C', 'done', '2024', '2024-01-01T00:00:00.000Z'),
               ('在读', 'D', 'doing', NULL, '2026-10-01T00:00:00.000Z');
             INSERT INTO book_rereads (book_id, year, sort_order) VALUES (3, '2026', 1), (1, '2026', 1);
             INSERT INTO films (title, director, status, finished_year) VALUES ('在看', 'X', 'doing', NULL), ('看完', 'Y', 'done', '2026');
             INSERT INTO drawing_courses (name, sort_order) VALUES ('速写', 1), ('画完的课', 2), ('空课', 3);
             INSERT INTO drawing_lessons (course_id, code, title, status, finished_month, sort_order) VALUES
               (1, '1', '线条', 'done', '2026-03', 1), (1, '2', '排线', 'todo', NULL, 2), (1, '3', '透视', 'doing', NULL, 3),
               (1, '4', '旧课', 'done', '2025-12', 4), (2, '1', '唯一', 'done', '2026-09', 1);
             INSERT INTO todo_groups (name, sort_order) VALUES ('学业', 1);
             INSERT INTO todo_goals (group_id, name, sort_order) VALUES (1, '毕业', 1);
             INSERT INTO todo_tasks (group_id, goal_id, title, sort_order, done, done_at) VALUES
               (1, 1, '交**论文**', 1, 1, '2026-10-08T03:00:00.000Z'),
               (1, NULL, '去年的', 2, 1, '2025-06-01T03:00:00.000Z'),
               (1, NULL, '没做完', 3, 0, NULL);
             INSERT INTO todo_logs (task_id, day, amount) VALUES (3, '2026-10-07', 1), (3, '2025-10-07', 1);
             INSERT INTO notes (content, created_at) VALUES ('#读书 关于**时间**的想法', '2026-10-08T05:00:00.000Z'), ('旧笔记', '2025-02-01T05:00:00.000Z');
             INSERT INTO fin_accounts (name, currency, sort_order) VALUES ('招行', 'CNY', 1), ('DBS', 'SGD', 2);
             INSERT INTO fin_balances (account_id, day, amount) VALUES (1, '2026-09-01', 100000), (2, '2026-09-01', 1000), (1, '2026-10-01', 200000);
             INSERT INTO fin_rates (day, currency, rate) VALUES ('2026-10-01', 'SGD', 5.5);
             INSERT INTO fin_entries (flow, amount, currency, day, note, created_at) VALUES ('out', 3200, 'CNY', '2026-10-07', '午饭', '2026-10-07T04:00:00.000Z');
             INSERT INTO jobs (company, position, created_at) VALUES ('字节', '后端', '2026-09-01T00:00:00.000Z'), ('腾讯', NULL, '2026-09-02T00:00:00.000Z'), ('想投的', NULL, '2026-09-03T00:00:00.000Z');
             INSERT INTO job_steps (job_id, status_id, done, done_on) VALUES
               (1, 2, 1, '2026-09-10'), (1, 4, 0, NULL),
               (2, 2, 1, '2026-10-06'), (2, 4, 1, '2026-10-07'),
               (3, 2, 0, NULL);
             UPDATE films SET created_at = '2026-05-01T00:00:00.000Z';
             UPDATE drawing_courses SET created_at = '2026-04-01T00:00:00.000Z';
             UPDATE drawing_lessons SET created_at = '2026-04-02T00:00:00.000Z';",
        )
        .unwrap();
    }

    #[test]
    fn sums_up_every_module() {
        let c = memory_db();
        seed(&c);
        let o = overview(&c, TODAY, &[]).unwrap();
        let y = &o.year;
        assert_eq!((y.year.as_str(), y.books, y.films, y.lessons, y.notes, y.jobs), ("2026", Some(2), Some(1), Some(2), Some(1), Some(2)));
        // 完成 = what 回看 counts for the year
        assert_eq!(y.todos, Some(todo::done_between(&c, "2026-01-01", "2026-12-31").unwrap().len() as i64));
        assert_eq!(y.todos, Some(2));

        // total = latest balance per account × latest rate; 9/1 has no SGD rate yet → left out
        assert_eq!(o.assets, Some(vec![Point { day: "2026-10-01".into(), total: 200000 + 5500 }]));

        let j = o.jobs.unwrap();
        assert_eq!((j.total, j.this_week, j.ongoing, j.got), (3, 1, 2, 0)); // 腾讯 applied on Tue of this week
        assert_eq!(j.follow.iter().map(|l| (l.hit.title.as_str(), l.waiting_days)).collect::<Vec<_>>(), [("字节 · 后端", Some(28))]);
        assert_eq!(j.active.iter().map(|l| (l.hit.title.as_str(), l.status.as_str())).collect::<Vec<_>>(), [("腾讯", "面试")]);

        let r = o.reading.unwrap();
        assert_eq!(r.iter().map(|h| (h.kind, h.title.as_str(), h.detail.as_deref())).collect::<Vec<_>>(), [("book", "在读", Some("D")), ("film", "在看", Some("X"))]);

        let d = o.drawing.unwrap();
        assert_eq!(d.len(), 1); // finished and empty courses are left out
        assert_eq!((d[0].name.as_str(), d[0].total, d[0].done, d[0].doing), ("速写", 4, 2, 1));
        let next = d[0].next.as_ref().unwrap();
        assert_eq!((next.kind, next.title.as_str(), next.parent), ("lesson", "3 透视", Some(1))); // in progress beats the first undone

        // newest first; the checked task with its goal as parent; notes as excerpts
        let top: Vec<_> = o.recent.iter().take(3).map(|x| (x.hit.kind, x.hit.title.as_str(), x.done)).collect();
        assert_eq!(top, [("note", "关于时间的想法", false), ("task", "交论文", true), ("entry", "午饭", false)]);
        assert_eq!(o.recent[1].hit.parent, Some(-1));
        assert_eq!((o.recent[2].hit.amount, o.recent[2].hit.day.as_deref()), (Some(3200), Some("2026-10-07")));
        assert_eq!(o.recent.len(), RECENT);
        assert!(o.recent.windows(2).all(|w| w[0].at >= w[1].at));
    }

    #[test]
    fn hidden_modules_are_left_out() {
        let c = memory_db();
        seed(&c);
        let hidden: Vec<String> = ["books", "finance", "jobs", "drawing", "notes", "todo"].map(String::from).to_vec();
        let o = overview(&c, TODAY, &hidden).unwrap();
        let y = &o.year;
        assert_eq!((y.books, y.films, y.lessons, y.todos, y.notes, y.jobs), (None, Some(1), None, None, None, None));
        assert_eq!((o.assets, o.jobs, o.drawing), (None, None, None));
        assert_eq!(o.reading.unwrap().iter().map(|h| h.kind).collect::<Vec<_>>(), ["film"]);
        assert!(o.recent.iter().all(|x| x.hit.module == "films"));
        let o = overview(&c, TODAY, &["books".into(), "films".into(), "days".into()]).unwrap();
        assert_eq!((o.reading, o.days), (None, None));
    }

    #[test]
    fn coming_days() {
        let c = memory_db();
        let mk = |v: serde_json::Value| days::create(&c, serde_json::from_value(v).unwrap(), TODAY).unwrap();
        mk(serde_json::json!({"kind": "birthday", "name": "妈妈", "date": "1966-10-10", "remind": [7]}));
        mk(serde_json::json!({"name": "还早", "date": "2026-12-31", "remind": [7]}));
        mk(serde_json::json!({"kind": "document", "name": "签证", "date": "2026-10-01"}));
        let names: Vec<String> = overview(&c, TODAY, &[]).unwrap().days.unwrap().into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["签证", "妈妈"]);
    }

    #[test]
    fn empty_database() {
        let c = memory_db();
        let o = overview(&c, TODAY, &[]).unwrap();
        assert_eq!((o.year.books, o.year.todos, o.year.jobs), (Some(0), Some(0), Some(0)));
        assert_eq!(o.assets, Some(vec![]));
        assert_eq!(o.jobs.unwrap().total, 0);
        assert!(o.reading.unwrap().is_empty() && o.drawing.unwrap().is_empty() && o.recent.is_empty() && o.days.unwrap().is_empty());
        assert!(overview(&c, "2026-13-01", &[]).is_err());
    }
}
