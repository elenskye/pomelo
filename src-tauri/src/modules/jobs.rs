//! 投石问路 (job applications). One row per application, from 想投 to its end; no status history,
//! only the furthest status reached (`reached_id`, for the funnel). Statuses are user-editable and
//! each belongs to a fixed stage:
//!   wish 准备 · applied 等回音 · active 进行中 · got 拿到 · closed 结束
//! Rules (all here):
//! - Moving to applied / active / got stamps `applied_on` (if empty); a status change clears
//!   `step_on` unless a new one is given; `reached_id` only moves forward (closed never counts).
//! - Follow-up: stage applied and ≥ 14 days since max(applied_on, followed_on).
//! - 一步一步 link: an application wants at most one task — 「投递 公司 · 岗位」 due on the deadline
//!   while in a wish status, 「<状态> 公司 · 岗位」 due on `step_on` while active. `sync_task`
//!   creates / updates it (in the group of the other linked tasks, else 「💼 求职」) and checks it off
//!   when nothing is wanted any more.
//! 渠道 = kinds.rs module "job_channel".

use super::common::{self, err, some, str_enum};
use super::{kinds, todo};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

pub const KEY: &str = "jobs";
pub const LABEL: &str = "投石问路";
pub const CHANNELS: &str = "job_channel";
pub const TODO_GROUP: &str = "💼 求职";
pub const FOLLOW_UP_DAYS: i64 = 14;
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

fn gone(what: &str) -> String {
    format!("找不到这个{what}，可能已经被删除了")
}

str_enum!(Stage, default = Wish, {
    Wish => "wish",
    Applied => "applied",
    Active => "active",
    Got => "got",
    Closed => "closed",
});

// ---------- statuses ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub id: i64,
    pub name: String,
    pub stage: Stage,
    pub sort_order: i64,
    /// Applications currently in this status.
    pub count: i64,
}

fn status_row(r: &Row) -> rusqlite::Result<Status> {
    Ok(Status { id: r.get(0)?, name: r.get(1)?, stage: r.get(2)?, sort_order: r.get(3)?, count: r.get(4)? })
}

const STATUS_SQL: &str = "SELECT s.id, s.name, s.stage, s.sort_order, (SELECT count(*) FROM jobs j WHERE j.status_id = s.id) FROM job_statuses s";

pub fn statuses(conn: &Connection) -> Result<Vec<Status>, String> {
    let mut st = conn.prepare(&format!("{STATUS_SQL} ORDER BY s.sort_order, s.id")).map_err(err)?;
    let rows = st.query_map([], status_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn status(conn: &Connection, id: i64) -> Result<Status, String> {
    conn.query_row(&format!("{STATUS_SQL} WHERE s.id = ?1"), [id], status_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("状态"))
}

fn clean_status_name(conn: &Connection, name: &str, except: Option<i64>) -> Result<String, String> {
    let name = common::require(name, "状态名", 20)?;
    let dup: i64 = conn
        .query_row("SELECT count(*) FROM job_statuses WHERE name = ?1 AND id != coalesce(?2, -1)", params![name, except], |r| r.get(0))
        .map_err(err)?;
    if dup > 0 {
        return Err(format!("已经有「{name}」这个状态了"));
    }
    Ok(name)
}

pub fn create_status(conn: &Connection, name: &str, stage: Stage) -> Result<Status, String> {
    let name = clean_status_name(conn, name, None)?;
    conn.execute(
        "INSERT INTO job_statuses (name, stage, sort_order) VALUES (?1, ?2, (SELECT coalesce(max(sort_order), 0) + 1 FROM job_statuses))",
        params![name, stage],
    )
    .map_err(err)?;
    status(conn, conn.last_insert_rowid())
}

/// Renaming / changing the stage re-checks every linked task (titles and what is wanted change).
pub fn update_status(conn: &Connection, id: i64, name: Option<&str>, stage: Option<Stage>) -> Result<Status, String> {
    let old = status(conn, id)?;
    let name = match name {
        Some(n) => clean_status_name(conn, n, Some(id))?,
        None => old.name.clone(),
    };
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        &format!("UPDATE job_statuses SET name = ?1, stage = ?2, updated_at = {NOW} WHERE id = ?3"),
        params![name, stage.unwrap_or(old.stage), id],
    )
    .map_err(err)?;
    for job_id in ids_where(&tx, "status_id = ?1", id)? {
        sync_task(&tx, job_id)?;
    }
    tx.commit().map_err(err)?;
    status(conn, id)
}

/// Only an unused status can go, and one must remain.
pub fn delete_status(conn: &Connection, id: i64) -> Result<(), String> {
    let s = status(conn, id)?;
    if s.count > 0 {
        return Err(format!("还有 {} 条投递是「{}」，先把它们改成别的状态", s.count, s.name));
    }
    if statuses(conn)?.len() <= 1 {
        return Err("至少要留一个状态".into());
    }
    conn.execute("DELETE FROM job_statuses WHERE id = ?1", [id]).map_err(err)?;
    Ok(())
}

pub fn reorder_statuses(conn: &Connection, ids: &[i64]) -> Result<(), String> {
    common::reorder(conn, "job_statuses", "1 = 1", &[], ids)
}

// ---------- applications ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: i64,
    pub company: String,
    pub position: Option<String>,
    pub location: Option<String>,
    pub link: Option<String>,
    pub kind_id: Option<i64>,
    pub resume: Option<String>,
    pub cover_letter: Option<String>,
    pub deadline: Option<String>,
    pub salary: Option<String>,
    pub note: Option<String>,
    pub status_id: i64,
    pub applied_on: Option<String>,
    pub step_on: Option<String>,
    pub followed_on: Option<String>,
    pub reached_id: Option<i64>,
    pub task_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    /// Derived: the status's stage.
    pub stage: Stage,
    /// Derived (stage applied): days since applied / last followed up.
    pub waiting_days: Option<i64>,
    /// Derived: waiting ≥ 14 days.
    pub follow_up: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobPatch {
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default, deserialize_with = "some")]
    pub position: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub location: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub link: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub kind_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub resume: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub cover_letter: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub deadline: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub salary: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
    #[serde(default)]
    pub status_id: Option<i64>,
    #[serde(default, deserialize_with = "some")]
    pub applied_on: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub step_on: Option<Option<String>>,
}

const JOB_COLS: &str = "j.id, j.company, j.position, j.location, j.link, j.kind_id, j.resume, j.cover_letter, j.deadline, j.salary,
  j.note, j.status_id, j.applied_on, j.step_on, j.followed_on, j.reached_id, j.task_id, j.created_at, j.updated_at, s.stage";

fn job_row(r: &Row) -> rusqlite::Result<Job> {
    Ok(Job {
        id: r.get(0)?,
        company: r.get(1)?,
        position: r.get(2)?,
        location: r.get(3)?,
        link: r.get(4)?,
        kind_id: r.get(5)?,
        resume: r.get(6)?,
        cover_letter: r.get(7)?,
        deadline: r.get(8)?,
        salary: r.get(9)?,
        note: r.get(10)?,
        status_id: r.get(11)?,
        applied_on: r.get(12)?,
        step_on: r.get(13)?,
        followed_on: r.get(14)?,
        reached_id: r.get(15)?,
        task_id: r.get(16)?,
        created_at: r.get(17)?,
        updated_at: r.get(18)?,
        stage: r.get(19)?,
        waiting_days: None,
        follow_up: false,
    })
}

fn derive(mut j: Job, today: &str) -> Job {
    if j.stage == Stage::Applied {
        let since = match (&j.applied_on, &j.followed_on) {
            (Some(a), Some(f)) => Some(a.max(f).clone()),
            (a, f) => a.clone().or(f.clone()),
        };
        j.waiting_days = since.map(|d| common::day_number(today) - common::day_number(&d));
        j.follow_up = j.waiting_days.is_some_and(|n| n >= FOLLOW_UP_DAYS);
    }
    j
}

fn query(conn: &Connection, filter: &str, args: &[&dyn rusqlite::ToSql], today: &str) -> Result<Vec<Job>, String> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {JOB_COLS} FROM jobs j JOIN job_statuses s ON s.id = j.status_id WHERE {filter}
             ORDER BY s.sort_order, coalesce(j.applied_on, substr(j.created_at, 1, 10)) DESC, j.id DESC"
        ))
        .map_err(err)?;
    let rows = st.query_map(args, job_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    Ok(rows.into_iter().map(|j| derive(j, today)).collect())
}

/// All applications: by status order, newest application first.
pub fn jobs(conn: &Connection, today: &str) -> Result<Vec<Job>, String> {
    query(conn, "1 = 1", &[], today)
}

pub fn job(conn: &Connection, id: i64, today: &str) -> Result<Job, String> {
    query(conn, "j.id = ?1", &[&id], today)?.pop().ok_or_else(|| gone("投递"))
}

fn ids_where(conn: &Connection, filter: &str, arg: i64) -> Result<Vec<i64>, String> {
    let mut st = conn.prepare(&format!("SELECT id FROM jobs WHERE {filter}")).map_err(err)?;
    let ids = st.query_map([arg], |r| r.get(0)).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    Ok(ids)
}

struct JobFields {
    company: String,
    position: Option<String>,
    location: Option<String>,
    link: Option<String>,
    kind_id: Option<i64>,
    resume: Option<String>,
    cover_letter: Option<String>,
    deadline: Option<String>,
    salary: Option<String>,
    note: Option<String>,
    status_id: i64,
    applied_on: Option<String>,
    step_on: Option<String>,
}

impl JobFields {
    fn validated(self, conn: &Connection) -> Result<Self, String> {
        let f = Self {
            company: common::require(&self.company, "公司", 100)?,
            position: common::clean(self.position),
            location: common::clean(self.location),
            link: common::clean(self.link),
            resume: common::clean(self.resume),
            cover_letter: common::clean(self.cover_letter),
            deadline: common::clean(self.deadline),
            salary: common::clean(self.salary),
            note: common::clean(self.note),
            applied_on: common::clean(self.applied_on),
            step_on: common::clean(self.step_on),
            ..self
        };
        common::check_len(&f.position, "岗位", 100)?;
        common::check_len(&f.location, "地点", 50)?;
        common::check_len(&f.link, "JD 链接", 1000)?;
        common::check_len(&f.resume, "简历", 1000)?;
        common::check_len(&f.cover_letter, "求职信", 1000)?;
        common::check_len(&f.salary, "薪资", 50)?;
        common::check_len(&f.note, "备注", 5000)?;
        common::check_date(&f.deadline, "截止日期")?;
        common::check_date(&f.applied_on, "投递日期")?;
        common::check_date(&f.step_on, "这一步的日期")?;
        kinds::check(conn, CHANNELS, f.kind_id).map_err(|_| "选择的渠道不存在，可能已经被删除了".to_string())?;
        status(conn, f.status_id)?;
        Ok(f)
    }
}

/// `reached` after moving to `to`: only forward, never a closed status.
fn reached_after(conn: &Connection, reached: Option<i64>, to: &Status) -> Result<Option<i64>, String> {
    if to.stage == Stage::Closed {
        return Ok(reached);
    }
    Ok(match reached.map(|r| status(conn, r)).transpose().ok().flatten() {
        Some(r) if r.sort_order >= to.sort_order => Some(r.id),
        _ => Some(to.id),
    })
}

fn default_status(conn: &Connection) -> Result<i64, String> {
    let all = statuses(conn)?;
    all.iter().find(|s| s.stage == Stage::Wish).or(all.first()).map(|s| s.id).ok_or_else(|| "还没有任何状态".into())
}

fn stamps_applied(stage: Stage) -> bool {
    matches!(stage, Stage::Applied | Stage::Active | Stage::Got)
}

pub fn create_job(conn: &Connection, p: JobPatch, today: &str) -> Result<Job, String> {
    let status_id = match p.status_id {
        Some(id) => id,
        None => default_status(conn)?,
    };
    let mut f = JobFields {
        company: p.company.unwrap_or_default(),
        position: p.position.flatten(),
        location: p.location.flatten(),
        link: p.link.flatten(),
        kind_id: p.kind_id.flatten(),
        resume: p.resume.flatten(),
        cover_letter: p.cover_letter.flatten(),
        deadline: p.deadline.flatten(),
        salary: p.salary.flatten(),
        note: p.note.flatten(),
        status_id,
        applied_on: p.applied_on.flatten(),
        step_on: p.step_on.flatten(),
    }
    .validated(conn)?;
    let s = status(conn, f.status_id)?;
    if stamps_applied(s.stage) && f.applied_on.is_none() {
        f.applied_on = Some(today.into());
    }
    let reached = reached_after(conn, None, &s)?;
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        "INSERT INTO jobs (company, position, location, link, kind_id, resume, cover_letter, deadline, salary, note, status_id,
           applied_on, step_on, reached_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            f.company, f.position, f.location, f.link, f.kind_id, f.resume, f.cover_letter, f.deadline, f.salary, f.note,
            f.status_id, f.applied_on, f.step_on, reached
        ],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    sync_task(&tx, id)?;
    tx.commit().map_err(err)?;
    job(conn, id, today)
}

pub fn update_job(conn: &Connection, id: i64, p: JobPatch, today: &str) -> Result<Job, String> {
    let old = job(conn, id, today)?;
    let status_changed = p.status_id.is_some_and(|s| s != old.status_id);
    let mut f = JobFields {
        company: p.company.unwrap_or(old.company.clone()),
        position: p.position.unwrap_or(old.position.clone()),
        location: p.location.unwrap_or(old.location.clone()),
        link: p.link.unwrap_or(old.link.clone()),
        kind_id: p.kind_id.unwrap_or(old.kind_id),
        resume: p.resume.unwrap_or(old.resume.clone()),
        cover_letter: p.cover_letter.unwrap_or(old.cover_letter.clone()),
        deadline: p.deadline.unwrap_or(old.deadline.clone()),
        salary: p.salary.unwrap_or(old.salary.clone()),
        note: p.note.unwrap_or(old.note.clone()),
        status_id: p.status_id.unwrap_or(old.status_id),
        applied_on: p.applied_on.unwrap_or(old.applied_on.clone()),
        // a new step starts without a date unless one is given
        step_on: match p.step_on {
            Some(v) => v,
            None if status_changed => None,
            None => old.step_on.clone(),
        },
    }
    .validated(conn)?;
    let s = status(conn, f.status_id)?;
    if stamps_applied(s.stage) && f.applied_on.is_none() {
        f.applied_on = Some(today.into());
    }
    let reached = if status_changed { reached_after(conn, old.reached_id, &s)? } else { old.reached_id };
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        &format!(
            "UPDATE jobs SET company = ?1, position = ?2, location = ?3, link = ?4, kind_id = ?5, resume = ?6, cover_letter = ?7,
               deadline = ?8, salary = ?9, note = ?10, status_id = ?11, applied_on = ?12, step_on = ?13, reached_id = ?14,
               updated_at = {NOW} WHERE id = ?15"
        ),
        params![
            f.company, f.position, f.location, f.link, f.kind_id, f.resume, f.cover_letter, f.deadline, f.salary, f.note,
            f.status_id, f.applied_on, f.step_on, reached, id
        ],
    )
    .map_err(err)?;
    sync_task(&tx, id)?;
    tx.commit().map_err(err)?;
    job(conn, id, today)
}

/// 已跟进: the 14-day clock restarts today.
pub fn follow_up(conn: &Connection, id: i64, today: &str) -> Result<Job, String> {
    job(conn, id, today)?;
    conn.execute(&format!("UPDATE jobs SET followed_on = ?1, updated_at = {NOW} WHERE id = ?2"), params![today, id])
        .map_err(err)?;
    job(conn, id, today)
}

/// Deletes the application; its linked task goes too unless already done (kept as history).
pub fn delete_job(conn: &Connection, id: i64) -> Result<(), String> {
    let task_id: Option<i64> = conn
        .query_row("SELECT task_id FROM jobs WHERE id = ?1", [id], |r| r.get(0))
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("投递"))?;
    let tx = common::Tx::begin(conn)?;
    tx.execute("DELETE FROM jobs WHERE id = ?1", [id]).map_err(err)?;
    if let Some(t) = task_id.and_then(|t| todo::task(&tx, t).ok()).filter(|t| !t.done) {
        todo::delete_task(&tx, t.id)?;
    }
    tx.commit().map_err(err)
}

// ---------- 一步一步 link ----------

fn label(company: &str, position: &Option<String>) -> String {
    match position {
        Some(p) => format!("{company} · {p}"),
        None => company.to_string(),
    }
}

/// The task an application wants right now: (title, due date).
fn wanted(conn: &Connection, id: i64) -> Result<Option<(String, String)>, String> {
    let (company, position, status_id, deadline, step_on): (String, Option<String>, i64, Option<String>, Option<String>) = conn
        .query_row("SELECT company, position, status_id, deadline, step_on FROM jobs WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(err)?;
    let s = status(conn, status_id)?;
    Ok(match s.stage {
        Stage::Wish => deadline.map(|d| (format!("投递 {}", label(&company, &position)), d)),
        Stage::Active => step_on.map(|d| (format!("{} {}", s.name, label(&company, &position)), d)),
        _ => None,
    })
}

/// Group for new linked tasks: where the other linked tasks live, else 「💼 求职」 (created).
fn task_group(conn: &Connection) -> Result<i64, String> {
    let linked: Option<i64> = conn
        .query_row("SELECT t.group_id FROM jobs j JOIN todo_tasks t ON t.id = j.task_id ORDER BY j.updated_at DESC LIMIT 1", [], |r| r.get(0))
        .optional()
        .map_err(err)?;
    if let Some(g) = linked {
        return Ok(g);
    }
    let named: Option<i64> = conn
        .query_row("SELECT id FROM todo_groups WHERE name = ?1", [TODO_GROUP], |r| r.get(0))
        .optional()
        .map_err(err)?;
    match named {
        Some(g) => Ok(g),
        None => Ok(todo::create_group(conn, TODO_GROUP)?.id),
    }
}

fn patch(v: serde_json::Value) -> Result<todo::TaskPatch, String> {
    serde_json::from_value(v).map_err(err)
}

/// Brings the linked task in line with what the application wants (see module doc).
pub fn sync_task(conn: &Connection, id: i64) -> Result<(), String> {
    let task_id: Option<i64> = conn.query_row("SELECT task_id FROM jobs WHERE id = ?1", [id], |r| r.get(0)).map_err(err)?;
    let task = task_id.and_then(|t| todo::task(conn, t).ok());
    match (wanted(conn, id)?, task) {
        (Some((title, due)), Some(t)) => {
            let changed = t.title != title || t.due_date.as_deref() != Some(due.as_str());
            if changed {
                todo::update_task(conn, t.id, patch(serde_json::json!({"title": title, "startDate": null, "dueDate": due}))?)?;
                if t.done {
                    todo::set_done(conn, t.id, false)?;
                }
            }
        }
        (Some((title, due)), None) => {
            let group_id = task_group(conn)?;
            let t = todo::create_task(conn, patch(serde_json::json!({"groupId": group_id, "title": title, "dueDate": due}))?)?;
            conn.execute("UPDATE jobs SET task_id = ?1 WHERE id = ?2", params![t.id, id]).map_err(err)?;
        }
        (None, Some(t)) if !t.done => todo::set_done(conn, t.id, true)?,
        _ => {}
    }
    Ok(())
}

/// After an import: link applications to existing undone tasks with the wanted title (so a
/// restored 一步一步 isn't duplicated), creating the rest.
pub fn relink(conn: &Connection) -> Result<(), String> {
    let mut st = conn.prepare("SELECT id FROM jobs WHERE task_id IS NULL").map_err(err)?;
    let ids: Vec<i64> = st.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for id in ids {
        if let Some((title, _)) = wanted(conn, id)? {
            let found: Option<i64> = conn
                .query_row(
                    "SELECT id FROM todo_tasks WHERE title = ?1 AND done = 0 AND id NOT IN (SELECT task_id FROM jobs WHERE task_id IS NOT NULL) LIMIT 1",
                    [&title],
                    |r| r.get(0),
                )
                .optional()
                .map_err(err)?;
            if let Some(t) = found {
                conn.execute("UPDATE jobs SET task_id = ?1 WHERE id = ?2", params![t, id]).map_err(err)?;
            }
            sync_task(conn, id)?;
        }
    }
    Ok(())
}

// ---------- import / export: { statuses: [{name, stage}], jobs: [...] } (names, not ids) ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusRecord {
    pub name: String,
    pub stage: Stage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobRecord {
    pub company: String,
    #[serde(default)]
    pub position: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub resume: Option<String>,
    #[serde(default)]
    pub cover_letter: Option<String>,
    #[serde(default)]
    pub deadline: Option<String>,
    #[serde(default)]
    pub salary: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    pub status: String,
    #[serde(default)]
    pub applied_on: Option<String>,
    #[serde(default)]
    pub step_on: Option<String>,
    #[serde(default)]
    pub followed_on: Option<String>,
    /// Furthest status reached (name).
    #[serde(default)]
    pub reached: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobsRecord {
    /// Omitted = keep the current statuses (the file's jobs must use their names).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statuses: Vec<StatusRecord>,
    #[serde(default)]
    pub jobs: Vec<JobRecord>,
}

impl JobsRecord {
    pub fn len(&self) -> usize {
        self.jobs.len()
    }
}

pub fn validate_records(rec: &JobsRecord) -> Result<(), String> {
    let mut names = std::collections::HashSet::new();
    for s in &rec.statuses {
        let n = common::require(&s.name, "状态名", 20).map_err(|e| format!("{LABEL}的状态：{e}"))?;
        if !names.insert(n.clone()) {
            return Err(format!("{LABEL}：状态「{n}」重复了"));
        }
    }
    for (i, j) in rec.jobs.iter().enumerate() {
        let ctx = |e: String| format!("{LABEL}第 {} 条（{}）：{e}", i + 1, j.company.trim());
        common::require(&j.company, "公司", 100).map_err(ctx)?;
        for (d, what) in [(&j.deadline, "截止日期"), (&j.applied_on, "投递日期"), (&j.step_on, "这一步的日期"), (&j.followed_on, "跟进日期")] {
            common::check_date(d, what).map_err(ctx)?;
        }
        kinds::check_name(&j.channel).map_err(ctx)?;
        if !rec.statuses.is_empty() {
            for s in std::iter::once(&j.status).chain(j.reached.iter()) {
                if !names.contains(s.trim()) {
                    return Err(ctx(format!("找不到状态「{s}」")));
                }
            }
        }
    }
    Ok(())
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM jobs", [], |r| r.get(0)).map_err(err)
}

/// Replaces all applications (and the statuses, if the file has them). Linked tasks are re-made
/// by `relink` once the whole import is in. Caller owns the transaction.
pub fn replace_all(conn: &Connection, rec: &JobsRecord) -> Result<usize, String> {
    validate_records(rec)?;
    conn.execute("DELETE FROM jobs", []).map_err(err)?;
    if !rec.statuses.is_empty() {
        conn.execute("DELETE FROM job_statuses", []).map_err(err)?;
        for (i, s) in rec.statuses.iter().enumerate() {
            conn.execute("INSERT INTO job_statuses (name, stage, sort_order) VALUES (?1, ?2, ?3)", params![s.name.trim(), s.stage, i as i64 + 1])
                .map_err(err)?;
        }
    }
    let ids: HashMap<String, i64> = statuses(conn)?.into_iter().map(|s| (s.name, s.id)).collect();
    let id_of = |n: &str| ids.get(n.trim()).copied().ok_or_else(|| format!("{LABEL}：找不到状态「{n}」"));
    for j in &rec.jobs {
        let channel = kinds::resolve(conn, CHANNELS, j.channel.as_deref())?;
        let reached = j.reached.as_deref().map(id_of).transpose()?;
        conn.execute(
            &format!(
                "INSERT INTO jobs (company, position, location, link, kind_id, resume, cover_letter, deadline, salary, note, status_id,
                   applied_on, step_on, followed_on, reached_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, coalesce(?16, {NOW}), coalesce(?17, ?16, {NOW}))"
            ),
            params![
                j.company.trim(), common::clean(j.position.clone()), common::clean(j.location.clone()), common::clean(j.link.clone()), channel,
                common::clean(j.resume.clone()), common::clean(j.cover_letter.clone()), common::clean(j.deadline.clone()),
                common::clean(j.salary.clone()), common::clean(j.note.clone()), id_of(&j.status)?, common::clean(j.applied_on.clone()),
                common::clean(j.step_on.clone()), common::clean(j.followed_on.clone()), reached, j.created_at, j.updated_at
            ],
        )
        .map_err(err)?;
    }
    Ok(rec.len())
}

pub fn export(conn: &Connection) -> Result<JobsRecord, String> {
    let sts = statuses(conn)?;
    let name: HashMap<i64, String> = sts.iter().map(|s| (s.id, s.name.clone())).collect();
    let channels = kinds::name_map(conn, CHANNELS)?;
    let mut st = conn.prepare(&format!("SELECT {JOB_COLS} FROM jobs j JOIN job_statuses s ON s.id = j.status_id ORDER BY j.id")).map_err(err)?;
    let jobs = st
        .query_map([], job_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?
        .into_iter()
        .map(|j| JobRecord {
            company: j.company,
            position: j.position,
            location: j.location,
            link: j.link,
            channel: j.kind_id.and_then(|k| channels.get(&k).cloned()),
            resume: j.resume,
            cover_letter: j.cover_letter,
            deadline: j.deadline,
            salary: j.salary,
            note: j.note,
            status: name[&j.status_id].clone(),
            applied_on: j.applied_on,
            step_on: j.step_on,
            followed_on: j.followed_on,
            reached: j.reached_id.and_then(|r| name.get(&r).cloned()),
            created_at: Some(j.created_at),
            updated_at: Some(j.updated_at),
        })
        .collect();
    Ok(JobsRecord { statuses: sts.into_iter().map(|s| StatusRecord { name: s.name, stage: s.stage }).collect(), jobs })
}

// ---------- commands ----------

fn today(conn: &Connection) -> Result<String, String> {
    todo::today(conn)
}

#[tauri::command]
pub fn job_statuses(state: State<AppState>) -> Result<Vec<Status>, String> {
    state.with_db(|c| statuses(c))
}
#[tauri::command]
pub fn job_status_create(state: State<AppState>, name: String, stage: Stage) -> Result<Status, String> {
    state.with_undo("添加状态", |c| create_status(c, &name, stage))
}
#[tauri::command]
pub fn job_status_update(state: State<AppState>, id: i64, name: Option<String>, stage: Option<Stage>) -> Result<Status, String> {
    state.with_undo("修改状态", |c| update_status(c, id, name.as_deref(), stage))
}
#[tauri::command]
pub fn job_status_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除状态", |c| delete_status(c, id))
}
#[tauri::command]
pub fn job_statuses_reorder(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| reorder_statuses(c, &ids))
}
#[tauri::command]
pub fn jobs_list(state: State<AppState>) -> Result<Vec<Job>, String> {
    state.with_db(|c| jobs(c, &today(c)?))
}
#[tauri::command]
pub fn job_create(state: State<AppState>, patch: JobPatch) -> Result<Job, String> {
    state.with_undo("添加投递", |c| create_job(c, patch, &today(c)?))
}
#[tauri::command]
pub fn job_update(state: State<AppState>, id: i64, patch: JobPatch) -> Result<Job, String> {
    state.with_undo("修改投递", |c| update_job(c, id, patch, &today(c)?))
}
#[tauri::command]
pub fn job_follow_up(state: State<AppState>, id: i64) -> Result<Job, String> {
    state.with_undo("已跟进", |c| follow_up(c, id, &today(c)?))
}
#[tauri::command]
pub fn job_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除投递", |c| delete_job(c, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    fn jp(v: serde_json::Value) -> JobPatch {
        serde_json::from_value(v).unwrap()
    }
    fn sid(c: &Connection, name: &str) -> i64 {
        statuses(c).unwrap().into_iter().find(|s| s.name == name).unwrap().id
    }

    #[test]
    fn seeded_statuses_and_channels() {
        let c = memory_db();
        let s: Vec<_> = statuses(&c).unwrap().into_iter().map(|s| (s.name, s.stage)).collect();
        assert_eq!(s[0], ("想投".into(), Stage::Wish));
        assert_eq!(s[6], ("被拒".into(), Stage::Closed));
        assert_eq!(kinds::list(&c, CHANNELS).unwrap().len(), 7);
    }

    #[test]
    fn status_moves_stamp_dates_and_reach_forward_only() {
        let c = memory_db();
        let t = "2026-10-05";
        let j = create_job(&c, jp(json!({"company": " 字节 ", "position": "后端"})), t).unwrap();
        assert_eq!((j.company.as_str(), j.stage, j.applied_on, j.reached_id), ("字节", Stage::Wish, None, Some(sid(&c, "想投"))));
        assert!(create_job(&c, jp(json!({"company": " "})), t).is_err());
        let j = update_job(&c, j.id, jp(json!({"statusId": sid(&c, "已投递")})), t).unwrap();
        assert_eq!(j.applied_on.as_deref(), Some(t)); // stamped once
        let j = update_job(&c, j.id, jp(json!({"statusId": sid(&c, "面试"), "stepOn": "2026-10-09"})), "2026-10-07").unwrap();
        assert_eq!((j.applied_on.as_deref(), j.step_on.as_deref(), j.reached_id), (Some(t), Some("2026-10-09"), Some(sid(&c, "面试"))));
        let j = update_job(&c, j.id, jp(json!({"statusId": sid(&c, "被拒")})), t).unwrap();
        assert_eq!((j.step_on, j.reached_id), (None, Some(sid(&c, "面试")))); // step date cleared; closed doesn't count
        let j = update_job(&c, j.id, jp(json!({"statusId": sid(&c, "已投递")})), t).unwrap();
        assert_eq!(j.reached_id, Some(sid(&c, "面试"))); // never backwards
    }

    #[test]
    fn follow_up_after_two_weeks() {
        let c = memory_db();
        let j = create_job(&c, jp(json!({"company": "A", "statusId": sid(&c, "已投递"), "appliedOn": "2026-09-20"})), "2026-10-03").unwrap();
        assert_eq!((j.waiting_days, j.follow_up), (Some(13), false));
        assert!(job(&c, j.id, "2026-10-04").unwrap().follow_up);
        follow_up(&c, j.id, "2026-10-05").unwrap();
        let later = job(&c, j.id, "2026-10-18").unwrap();
        assert_eq!((later.waiting_days, later.follow_up), (Some(13), false));
        assert!(job(&c, j.id, "2026-10-19").unwrap().follow_up);
        let moved = update_job(&c, j.id, jp(json!({"statusId": sid(&c, "面试")})), "2026-10-19").unwrap();
        assert_eq!((moved.waiting_days, moved.follow_up), (None, false)); // only while waiting
    }

    #[test]
    fn linked_task_follows_the_application() {
        let c = memory_db();
        let t = "2026-10-05";
        let j = create_job(&c, jp(json!({"company": "腾讯", "position": "后台开发", "deadline": "2026-10-20"})), t).unwrap();
        let task = todo::task(&c, j.task_id.unwrap()).unwrap();
        assert_eq!((task.title.as_str(), task.due_date.as_deref()), ("投递 腾讯 · 后台开发", Some("2026-10-20")));
        assert_eq!(todo::group(&c, task.group_id).unwrap().name, TODO_GROUP);
        // applied → nothing wanted → checked off
        let j = update_job(&c, j.id, jp(json!({"statusId": sid(&c, "已投递")})), t).unwrap();
        assert!(todo::task(&c, j.task_id.unwrap()).unwrap().done);
        // interview with a date → same task reopened with the new title and date
        let j = update_job(&c, j.id, jp(json!({"statusId": sid(&c, "面试"), "stepOn": "2026-10-12"})), t).unwrap();
        let task = todo::task(&c, j.task_id.unwrap()).unwrap();
        assert_eq!((task.title.as_str(), task.due_date.as_deref(), task.done), ("面试 腾讯 · 后台开发", Some("2026-10-12"), false));
        // renaming the status renames the task; a renamed group keeps being used
        let g = task.group_id;
        todo::rename_group(&c, g, "找工作").unwrap();
        update_status(&c, sid(&c, "面试"), Some("一面"), None).unwrap();
        assert_eq!(todo::task(&c, j.task_id.unwrap()).unwrap().title, "一面 腾讯 · 后台开发");
        let k = create_job(&c, jp(json!({"company": "阿里", "deadline": "2026-10-30"})), t).unwrap();
        assert_eq!(todo::task(&c, k.task_id.unwrap()).unwrap().group_id, g);
        // deleting the application removes its undone task
        let kt = k.task_id.unwrap();
        delete_job(&c, k.id).unwrap();
        assert!(todo::task(&c, kt).is_err());
        // a deleted task is simply made again on the next change
        todo::delete_task(&c, j.task_id.unwrap()).unwrap();
        let j = update_job(&c, j.id, jp(json!({"stepOn": "2026-10-13"})), t).unwrap();
        assert_eq!(todo::task(&c, j.task_id.unwrap()).unwrap().due_date.as_deref(), Some("2026-10-13"));
    }

    #[test]
    fn statuses_edit_rules() {
        let c = memory_db();
        let extra = create_status(&c, "HR 面", Stage::Active).unwrap();
        assert!(create_status(&c, "HR 面", Stage::Active).is_err());
        create_job(&c, jp(json!({"company": "A", "statusId": extra.id})), "2026-10-05").unwrap();
        assert!(delete_status(&c, extra.id).unwrap_err().contains("1 条"));
        delete_status(&c, sid(&c, "Offer")).unwrap();
        let ids: Vec<i64> = statuses(&c).unwrap().iter().rev().map(|s| s.id).collect();
        reorder_statuses(&c, &ids).unwrap();
        assert_eq!(statuses(&c).unwrap()[0].id, extra.id);
    }

    #[test]
    fn records_roundtrip() {
        let c = memory_db();
        let rec: JobsRecord = serde_json::from_value(json!({
            "statuses": [{"name": "想投", "stage": "wish"}, {"name": "已投递", "stage": "applied"}, {"name": "面试", "stage": "active"},
                         {"name": "已接受", "stage": "got"}, {"name": "被拒", "stage": "closed"}],
            "jobs": [
                {"company": "BOC", "position": "实习", "location": "新加坡", "channel": "学校", "resume": "~/Documents/Seeker/cv/a.docx",
                 "status": "已接受", "reached": "已接受", "stepOn": "2026-08-13"},
                {"company": "KeyReply", "status": "被拒", "reached": "已投递", "channel": "新渠道"},
                {"company": "X", "status": "面试", "reached": "面试", "stepOn": "2026-10-09"}
            ]
        }))
        .unwrap();
        assert_eq!(replace_all(&c, &rec).unwrap(), 3);
        relink(&c).unwrap(); // the interview gets its task
        let out = export(&c).unwrap();
        assert!(jobs(&c, "2026-10-05").unwrap().iter().any(|j| j.company == "X" && j.task_id.is_some()));
        let c2 = memory_db();
        replace_all(&c2, &out).unwrap();
        assert_eq!(serde_json::to_value(&out).unwrap(), serde_json::to_value(export(&c2).unwrap()).unwrap());
        assert!(kinds::list(&c, CHANNELS).unwrap().iter().any(|k| k.name == "新渠道"));
        // a restored 一步一步 task is linked, not duplicated
        let c3 = memory_db();
        let g = todo::create_group(&c3, TODO_GROUP).unwrap().id;
        todo::create_task(&c3, patch(json!({"groupId": g, "title": "面试 X", "dueDate": "2026-10-09"})).unwrap()).unwrap();
        replace_all(&c3, &out).unwrap();
        relink(&c3).unwrap();
        assert_eq!(todo::tasks(&c3, Some(g), None).unwrap().len(), 1);

        let bad = |v: serde_json::Value| validate_records(&serde_json::from_value(v).unwrap()).is_err();
        assert!(bad(json!({"statuses": [{"name": "a", "stage": "wish"}], "jobs": [{"company": "X", "status": "b"}]})));
        assert!(bad(json!({"jobs": [{"company": " ", "status": "想投"}]})));
        assert!(bad(json!({"jobs": [{"company": "X", "status": "想投", "deadline": "2026-13-01"}]})));
    }
}
