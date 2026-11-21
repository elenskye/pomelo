//! 月历 (v0.9): 一步一步、来日方长、出门走走、投石问路 on one calendar, read-only. The page (and the
//! 首页 desk's small month) asks for a span of local dates; modules hidden in the 模块坞 are left out.
//! What lands on a day (站主 2026-11-07):
//! - 待办 (todo::month): undone normal / progress tasks on their due date (a range task only there, with
//!   its start as a hint), a repeat task on its current due date and on every later occurrence inside
//!   the span — except daily ones (every day: too dense) — and what got done on the day it was done.
//!   Check-ins are left out, done or not. Tasks in paused / closed goals as todo::month leaves them.
//! - 日子 (days::on_calendar): one-offs and 证件 on their date, yearly ones and birthdays every year,
//!   monthly and weekly ones on each of theirs.
//! - 出行: on the day it departs (trips keep no return date).
//! - 面试: only what already happened — a step at a 进行中 or 拿到 status (笔试、面试、Offer…) on the day
//!   it was checked (job_steps keep no planned dates).
//! Order inside a day: tasks (as todo::month lists them), then days, trips, jobs.

use crate::modules::common::{self, err};
use crate::modules::days::{self, Kind};
use crate::modules::todo::{self, Rule, TaskType};
use crate::search::Hit;
use crate::AppState;
use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

/// The longest span one call reads (a 6-week month grid is 42 days).
pub const MAX_DAYS: i64 = 62;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub day: String,
    /// todo | day | trip | job
    pub source: &'static str,
    /// todo: due | next (a later occurrence) | done; day: day | birthday | document; trip: depart;
    /// job: active | got.
    pub state: &'static str,
    /// A short hint: 第 3 次, 11/03 起, 30 岁, 第 2 周年, 证件到期, the job's status…
    pub note: Option<String>,
    #[serde(flatten)]
    pub hit: Hit,
}

fn hit(module: &str, kind: &'static str, id: i64, title: String, detail: Option<String>, parent: Option<i64>, day: &str) -> Hit {
    Hit { module: module.into(), kind, id, title, detail, parent, day: Some(day.to_string()), amount: None, currency: None, flow: None }
}

fn task_hit(p: &todo::PlacedTask, day: &str) -> Hit {
    let t = &p.task;
    let place = p.goal_name.clone().unwrap_or_else(|| p.group_name.clone());
    // parent as in search.rs: -goal id inside a goal, else the group id
    hit("todo", "task", t.id, t.title.replace("**", ""), Some(place), Some(t.goal_id.map_or(t.group_id, |g| -g)), day)
}

fn md(d: &str) -> String {
    format!("{}/{}", &d[5..7], &d[8..10])
}

fn tasks(conn: &Connection, from: &str, to: &str) -> Result<Vec<Entry>, String> {
    let mut out = vec![];
    for x in todo::month(conn, from, to)? {
        let t = &x.task.task;
        if t.task_type == TaskType::Checkin {
            continue;
        }
        let (state, note) = match x.what.as_str() {
            // a range task: only on its due date, its start as the hint (a repeat task's start is its series start)
            "range" if t.due_date.as_deref() == Some(x.day.as_str()) => {
                ("due", if t.task_type == TaskType::Repeat { None } else { t.start_date.as_deref().map(|s| format!("{} 起", md(s))) })
            }
            "range" => continue,
            "due" => ("due", None),
            "next" => {
                let rule = t.repeat_rule.as_deref().and_then(|r| Rule::parse(r).ok());
                if matches!(rule, Some(Rule::Daily) | Some(Rule::EveryDays(1))) {
                    continue;
                }
                ("next", x.detail.clone())
            }
            "done" => ("done", x.detail.clone().filter(|d| d != "全部完成")),
            _ => continue,
        };
        out.push(Entry { day: x.day.clone(), source: "todo", state, note, hit: task_hit(&x.task, &x.day) });
    }
    Ok(out)
}

fn days_on(conn: &Connection, from: &str, to: &str) -> Result<Vec<Entry>, String> {
    Ok(days::on_calendar(conn, from, to)?
        .into_iter()
        .map(|h| {
            let (state, note) = match h.kind {
                Kind::Birthday => ("birthday", h.nth.map(|n| format!("{n} 岁"))),
                Kind::Document => ("document", Some("证件到期".to_string())),
                Kind::Day => ("day", h.nth.map(|n| format!("第 {n} 周年"))),
            };
            Entry { day: h.on.clone(), source: "day", state, note, hit: hit("days", "day", h.id, h.name, None, None, &h.on) }
        })
        .collect())
}

fn trips(conn: &Connection, from: &str, to: &str) -> Result<Vec<Entry>, String> {
    let mut st = conn.prepare("SELECT id, name, depart_on FROM trips WHERE depart_on BETWEEN ?1 AND ?2 ORDER BY depart_on, id").map_err(err)?;
    let rows = st
        .query_map([from, to], |r| {
            let day: String = r.get(2)?;
            Ok(Entry { day: day.clone(), source: "trip", state: "depart", note: None, hit: hit("packing", "trip", r.get(0)?, r.get(1)?, None, None, &day) })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

fn jobs(conn: &Connection, from: &str, to: &str) -> Result<Vec<Entry>, String> {
    let mut st = conn
        .prepare(
            "SELECT j.id, j.company || coalesce(' · ' || j.position, ''), j.location, st.name, st.stage, s.done_on
             FROM job_steps s JOIN jobs j ON j.id = s.job_id JOIN job_statuses st ON st.id = s.status_id
             WHERE s.done = 1 AND s.done_on BETWEEN ?1 AND ?2 AND st.stage IN ('active', 'got')
             ORDER BY s.done_on, st.sort_order, j.id",
        )
        .map_err(err)?;
    let rows = st
        .query_map([from, to], |r| {
            let day: String = r.get(5)?;
            let stage: String = r.get(4)?;
            Ok(Entry {
                day: day.clone(),
                source: "job",
                state: if stage == "got" { "got" } else { "active" },
                note: r.get(3)?,
                hit: hit("jobs", "job", r.get(0)?, r.get(1)?, r.get(2)?, None, &day),
            })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Everything on the calendar between two local dates (inclusive, at most `MAX_DAYS`), by day.
pub fn span(conn: &Connection, from: &str, to: &str, hidden: &[String]) -> Result<Vec<Entry>, String> {
    common::check_date(&Some(from.to_string()), "开始日期")?;
    common::check_date(&Some(to.to_string()), "结束日期")?;
    let n = common::day_number(to) - common::day_number(from) + 1;
    if n < 1 {
        return Err("结束日期不能早于开始日期".into());
    }
    if n > MAX_DAYS {
        return Err(format!("一次最多看 {MAX_DAYS} 天"));
    }
    let shown = |m: &str| !hidden.iter().any(|h| h == m);
    let mut out = vec![];
    if shown(todo::KEY) {
        out.extend(tasks(conn, from, to)?);
    }
    if shown(days::KEY) {
        out.extend(days_on(conn, from, to)?);
    }
    if shown("packing") {
        out.extend(trips(conn, from, to)?);
    }
    if shown("jobs") {
        out.extend(jobs(conn, from, to)?);
    }
    // stable: each source keeps its own order inside a day
    out.sort_by(|a, b| a.day.cmp(&b.day));
    Ok(out)
}

/// The month a local date is in: (first day, last day).
pub fn month_of(day: &str) -> (String, String) {
    let y: i64 = day[..4].parse().unwrap_or(2000);
    let m: u32 = day[5..7].parse().unwrap_or(1);
    (format!("{}-01", &day[..7]), format!("{}-{:02}", &day[..7], common::days_in_month(y, m)))
}

#[tauri::command]
pub fn calendar_span(state: State<AppState>, from: String, to: String, hidden: Vec<String>) -> Result<Vec<Entry>, String> {
    state.with_db(|c| span(c, &from, &to, &hidden))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    const TODAY: &str = "2026-11-07";

    fn seed(c: &Connection) {
        c.execute_batch(
            "INSERT INTO todo_groups (name, sort_order) VALUES ('学业', 1);
             INSERT INTO todo_goals (group_id, name, sort_order) VALUES (1, '毕业', 1);
             INSERT INTO todo_tasks (group_id, goal_id, title, task_type, start_date, due_date, repeat_rule, done, done_at, sort_order) VALUES
               (1, NULL, '交**报告**', 'normal', NULL, '2026-11-10', NULL, 0, NULL, 1),
               (1, 1, '写论文', 'normal', '2026-11-03', '2026-11-20', NULL, 0, NULL, 2),
               (1, NULL, '周会', 'repeat', NULL, '2026-11-09', 'weekly:1', 0, NULL, 3),
               (1, NULL, '吃药', 'repeat', NULL, '2026-11-07', 'daily', 0, NULL, 4),
               (1, NULL, '背单词', 'checkin', '2026-11-01', '2026-11-30', 'daily', 0, NULL, 5),
               (1, NULL, '买书', 'normal', NULL, '2026-11-02', NULL, 1, '2026-11-02T03:00:00.000Z', 6),
               (1, NULL, '下个月的', 'normal', NULL, '2026-12-02', NULL, 0, NULL, 7);
             INSERT INTO todo_logs (task_id, day, amount) VALUES (5, '2026-11-04', 1);
             INSERT INTO trips (name, depart_on) VALUES ('东京', '2026-11-21'), ('去年', '2025-11-21');
             INSERT INTO jobs (company, position, location) VALUES ('字节', '后端', '北京'), ('腾讯', NULL, NULL);
             INSERT INTO job_steps (job_id, status_id, done, done_on) VALUES
               (1, 2, 1, '2026-11-02'), (1, 4, 1, '2026-11-12'), (2, 4, 0, NULL), (2, 5, 1, '2026-11-25');",
        )
        .unwrap();
        let mk = |v: serde_json::Value| days::create(c, serde_json::from_value(v).unwrap(), TODAY).unwrap();
        mk(json!({"kind": "birthday", "name": "妈妈", "date": "1966-11-15"}));
        mk(json!({"kind": "birthday", "name": "不知道哪年", "date": "--11-30"}));
        mk(json!({"name": "在一起", "date": "2024-11-08", "repeat": "year"}));
        mk(json!({"name": "发工资", "date": "2026-10-31", "repeat": "month"}));
        mk(json!({"name": "游泳", "date": "2026-11-18", "repeat": "week"}));
        mk(json!({"kind": "document", "name": "签证", "date": "2026-11-28"}));
        mk(json!({"name": "毕业典礼", "date": "2027-06-30"}));
    }

    fn rows(es: &[Entry]) -> Vec<(String, &'static str, &'static str, String, Option<String>)> {
        es.iter().map(|e| (e.day[5..].to_string(), e.source, e.state, e.hit.title.clone(), e.note.clone())).collect()
    }

    #[test]
    fn a_month_of_everything() {
        let c = memory_db();
        seed(&c);
        let (from, to) = month_of(TODAY);
        assert_eq!((from.as_str(), to.as_str()), ("2026-11-01", "2026-11-30"));
        let es = span(&c, &from, &to, &[]).unwrap();
        let r = rows(&es);
        let has = |day: &str, source: &str, state: &str, title: &str, note: Option<&str>| {
            assert!(
                r.iter().any(|x| x.0 == day && x.1 == source && x.2 == state && x.3 == title && x.4.as_deref() == note),
                "missing {day} {source} {state} {title} {note:?} in {r:#?}"
            )
        };
        has("11-10", "todo", "due", "交报告", None);
        has("11-20", "todo", "due", "写论文", Some("11/03 起")); // a range only on its due date
        assert_eq!(r.iter().filter(|x| x.3 == "写论文").count(), 1);
        has("11-09", "todo", "due", "周会", None);
        for d in ["11-16", "11-23", "11-30"] {
            has(d, "todo", "next", "周会", None.or(r.iter().find(|x| x.0 == d && x.3 == "周会").and_then(|x| x.4.as_deref())));
        }
        assert_eq!(r.iter().filter(|x| x.3 == "周会").count(), 4);
        // the daily repeat only on its current date; check-ins never, not even their logs
        assert_eq!(r.iter().filter(|x| x.3 == "吃药").map(|x| x.0.as_str()).collect::<Vec<_>>(), ["11-07"]);
        assert!(!r.iter().any(|x| x.3 == "背单词"));
        has("11-02", "todo", "done", "买书", None);
        assert!(!r.iter().any(|x| x.3 == "下个月的"));

        has("11-15", "day", "birthday", "妈妈", Some("60 岁"));
        has("11-30", "day", "birthday", "不知道哪年", None);
        has("11-08", "day", "day", "在一起", Some("第 2 周年"));
        has("11-30", "day", "day", "发工资", None); // the 31st in a 30-day month
        assert_eq!(r.iter().filter(|x| x.3 == "游泳").map(|x| x.0.as_str()).collect::<Vec<_>>(), ["11-18", "11-25"]); // from its date on
        has("11-28", "day", "document", "签证", Some("证件到期"));
        assert!(!r.iter().any(|x| x.3 == "毕业典礼"));

        has("11-21", "trip", "depart", "东京", None);
        assert!(!r.iter().any(|x| x.3 == "去年"));
        has("11-12", "job", "active", "字节 · 后端", Some("面试"));
        has("11-25", "job", "got", "腾讯", Some("Offer"));
        assert!(!r.iter().any(|x| x.0 == "11-02" && x.1 == "job")); // 已投递 isn't an interview

        assert!(es.windows(2).all(|w| w[0].day <= w[1].day));
        let t = es.iter().find(|e| e.hit.title == "写论文").unwrap();
        assert_eq!((t.hit.module.as_str(), t.hit.kind, t.hit.parent, t.hit.detail.as_deref()), ("todo", "task", Some(-1), Some("毕业")));
        let d = es.iter().find(|e| e.hit.title == "签证").unwrap();
        assert_eq!((d.hit.module.as_str(), d.hit.kind, d.hit.day.as_deref()), ("days", "day", Some("2026-11-28")));
    }

    #[test]
    fn hidden_modules_and_bad_spans() {
        let c = memory_db();
        seed(&c);
        let hidden: Vec<String> = ["todo", "days", "packing"].map(String::from).to_vec();
        let es = span(&c, "2026-11-01", "2026-11-30", &hidden).unwrap();
        assert!(es.iter().all(|e| e.source == "job"));
        // a 6-week grid crossing two months
        let grid = span(&c, "2026-10-26", "2026-12-06", &[]).unwrap();
        assert!(grid.iter().any(|e| e.hit.title == "下个月的" && e.day == "2026-12-02"));
        assert!(grid.iter().any(|e| e.hit.title == "发工资" && e.day == "2026-10-31"));
        assert!(span(&c, "2026-11-30", "2026-11-01", &[]).is_err());
        assert!(span(&c, "2026-01-01", "2026-12-31", &[]).is_err());
        assert!(span(&c, "2026-11-31", "2026-12-01", &[]).is_err());
    }
}
