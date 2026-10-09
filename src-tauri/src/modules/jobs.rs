//! 投石问路 (job applications). Statuses are user-editable (name, order); each also has a stage that
//! drives the rules — not shown in the UI since round 7: the seeded ones keep theirs, new ones are active:
//!   wish 准备 · applied 等回音 · active 进行中 · got 拿到 · closed 结束
//! Each application picks its own steps (`job_steps`: the statuses it really goes through — BOCSG had
//! no 笔试 — shown in the status list's order) and checks each one off with the day it happened.
//! Derived, never stored (all here):
//! - Current status = the last checked step; nothing checked = 想投 (the first 准备 status). 准备
//!   statuses are never steps.
//! - 投递日期 (`applied_on`) = the earliest checked day.
//! - Follow-up: current stage applied and ≥ 14 days since max(its day, followed_on).
//! - Funnel (UI): per status, checked / applications that have the step.
//! 渠道 = kinds.rs module "job_channel".

use super::common::{self, err, some, str_enum};
use super::{kinds, todo};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::State;

pub const KEY: &str = "jobs";
pub const LABEL: &str = "投石问路";
pub const CHANNELS: &str = "job_channel";
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
    /// Applications whose current status this is.
    pub count: i64,
    /// Applications with it on their timeline.
    pub used: i64,
}

fn base_statuses(conn: &Connection) -> Result<Vec<Status>, String> {
    let mut st = conn.prepare("SELECT id, name, stage, sort_order FROM job_statuses ORDER BY sort_order, id").map_err(err)?;
    let rows = st
        .query_map([], |r| Ok(Status { id: r.get(0)?, name: r.get(1)?, stage: r.get(2)?, sort_order: r.get(3)?, count: 0, used: 0 }))
        .map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

/// Current status of every application (None: nothing checked and no 准备 status exists).
fn current_statuses(conn: &Connection) -> Result<Vec<Option<i64>>, String> {
    let wish: Option<i64> = conn
        .query_row("SELECT id FROM job_statuses WHERE stage = 'wish' ORDER BY sort_order, id LIMIT 1", [], |r| r.get(0))
        .optional()
        .map_err(err)?;
    let mut st = conn
        .prepare(
            "SELECT (SELECT st.status_id FROM job_steps st JOIN job_statuses s ON s.id = st.status_id
                     WHERE st.job_id = j.id AND st.done = 1 ORDER BY s.sort_order DESC, s.id DESC LIMIT 1)
             FROM jobs j",
        )
        .map_err(err)?;
    let rows = st.query_map([], |r| r.get::<_, Option<i64>>(0)).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    Ok(rows.into_iter().map(|s| s.or(wish)).collect())
}

pub fn statuses(conn: &Connection) -> Result<Vec<Status>, String> {
    let mut all = base_statuses(conn)?;
    for cur in current_statuses(conn)?.into_iter().flatten() {
        if let Some(s) = all.iter_mut().find(|s| s.id == cur) {
            s.count += 1;
        }
    }
    for s in &mut all {
        s.used = steps_using(conn, s.id)?;
    }
    Ok(all)
}

pub fn status(conn: &Connection, id: i64) -> Result<Status, String> {
    statuses(conn)?.into_iter().find(|s| s.id == id).ok_or_else(|| gone("状态"))
}

fn steps_using(conn: &Connection, status_id: i64) -> Result<i64, String> {
    conn.query_row("SELECT count(DISTINCT job_id) FROM job_steps WHERE status_id = ?1", [status_id], |r| r.get(0)).map_err(err)
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

/// A status on some application's steps can't become 准备 (准备 means "nothing checked yet").
pub fn update_status(conn: &Connection, id: i64, name: Option<&str>, stage: Option<Stage>) -> Result<Status, String> {
    let old = status(conn, id)?;
    let name = match name {
        Some(n) => clean_status_name(conn, n, Some(id))?,
        None => old.name.clone(),
    };
    let stage = stage.unwrap_or(old.stage);
    if stage == Stage::Wish && old.stage != Stage::Wish {
        let n = steps_using(conn, id)?;
        if n > 0 {
            return Err(format!("有 {n} 条投递的时间线上有「{}」，不能改成准备阶段", old.name));
        }
    }
    conn.execute(&format!("UPDATE job_statuses SET name = ?1, stage = ?2, updated_at = {NOW} WHERE id = ?3"), params![name, stage, id])
        .map_err(err)?;
    status(conn, id)
}

/// Only a status no application uses (as its current status or on its steps) can go; one must remain.
pub fn delete_status(conn: &Connection, id: i64) -> Result<(), String> {
    let s = status(conn, id)?;
    if s.count > 0 {
        return Err(format!("还有 {} 条投递是「{}」，先把它们改成别的状态", s.count, s.name));
    }
    let n = steps_using(conn, id)?;
    if n > 0 {
        return Err(format!("还有 {n} 条投递的时间线上有「{}」，先从这些投递里去掉它", s.name));
    }
    if base_statuses(conn)?.len() <= 1 {
        return Err("至少要留一个状态".into());
    }
    conn.execute("DELETE FROM job_statuses WHERE id = ?1", [id]).map_err(err)?;
    Ok(())
}

pub fn reorder_statuses(conn: &Connection, ids: &[i64]) -> Result<(), String> {
    common::reorder(conn, "job_statuses", "1 = 1", &[], ids)
}

// ---------- applications ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Step {
    pub status_id: i64,
    #[serde(default)]
    pub done: bool,
    /// The day it was done (only when done; may be unknown).
    #[serde(default)]
    pub done_on: Option<String>,
}

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
    pub note: Option<String>,
    pub followed_on: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// In the status list's order.
    pub steps: Vec<Step>,
    /// Derived: the last checked step, else the first 准备 status.
    pub status_id: Option<i64>,
    /// Derived: that status's stage (准备 when nothing is checked).
    pub stage: Stage,
    /// Derived: the earliest checked day.
    pub applied_on: Option<String>,
    /// Derived (stage applied): days since its day / the last follow-up.
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
    pub note: Option<Option<String>>,
    /// The whole list (replaces); omitted on create = every status that isn't 准备 / 结束, none checked.
    #[serde(default)]
    pub steps: Option<Vec<Step>>,
}

const JOB_COLS: &str = "id, company, position, location, link, kind_id, resume, note, followed_on, created_at, updated_at";

fn job_row(r: &Row) -> rusqlite::Result<Job> {
    Ok(Job {
        id: r.get(0)?,
        company: r.get(1)?,
        position: r.get(2)?,
        location: r.get(3)?,
        link: r.get(4)?,
        kind_id: r.get(5)?,
        resume: r.get(6)?,
        note: r.get(7)?,
        followed_on: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        steps: Vec::new(),
        status_id: None,
        stage: Stage::Wish,
        applied_on: None,
        waiting_days: None,
        follow_up: false,
    })
}

/// Fills steps and the derived fields.
fn derive(mut j: Job, steps: Vec<Step>, sts: &[Status], today: &str) -> Job {
    let order = |id: i64| sts.iter().position(|s| s.id == id).unwrap_or(usize::MAX);
    let mut steps = steps;
    steps.sort_by_key(|s| order(s.status_id));
    let last = steps.iter().rev().find(|s| s.done);
    let current = last.map(|s| s.status_id).or_else(|| sts.iter().find(|s| s.stage == Stage::Wish).map(|s| s.id));
    j.stage = current.and_then(|id| sts.iter().find(|s| s.id == id)).map_or(Stage::Wish, |s| s.stage);
    j.status_id = current;
    j.applied_on = steps.iter().filter(|s| s.done).filter_map(|s| s.done_on.clone()).min();
    if j.stage == Stage::Applied {
        let since = [last.and_then(|s| s.done_on.clone()), j.followed_on.clone()].into_iter().flatten().max();
        j.waiting_days = since.map(|d| common::day_number(today) - common::day_number(&d));
        j.follow_up = j.waiting_days.is_some_and(|n| n >= FOLLOW_UP_DAYS);
    }
    j.steps = steps;
    j
}

fn steps_by_job(conn: &Connection, job: Option<i64>) -> Result<HashMap<i64, Vec<Step>>, String> {
    let mut st = conn
        .prepare("SELECT job_id, status_id, done, done_on FROM job_steps WHERE ?1 IS NULL OR job_id = ?1 ORDER BY job_id, id")
        .map_err(err)?;
    let rows = st
        .query_map([job], |r| Ok((r.get::<_, i64>(0)?, Step { status_id: r.get(1)?, done: r.get(2)?, done_on: r.get(3)? })))
        .map_err(err)?;
    let mut out: HashMap<i64, Vec<Step>> = HashMap::new();
    for row in rows {
        let (job_id, step) = row.map_err(err)?;
        out.entry(job_id).or_default().push(step);
    }
    Ok(out)
}

/// The latest day an application moved (its last checked day, else the day it was added).
fn last_moved(j: &Job) -> String {
    j.steps.iter().filter_map(|s| s.done_on.clone()).max().unwrap_or_else(|| j.created_at[..10].to_string())
}

/// All applications: by current status (list order), then the most recently moved first.
pub fn jobs(conn: &Connection, today: &str) -> Result<Vec<Job>, String> {
    let sts = base_statuses(conn)?;
    let mut steps = steps_by_job(conn, None)?;
    let mut st = conn.prepare(&format!("SELECT {JOB_COLS} FROM jobs ORDER BY id")).map_err(err)?;
    let rows = st.query_map([], job_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let mut out: Vec<Job> = rows.into_iter().map(|j| { let s = steps.remove(&j.id).unwrap_or_default(); derive(j, s, &sts, today) }).collect();
    let order = |j: &Job| j.status_id.and_then(|id| sts.iter().position(|s| s.id == id)).unwrap_or(usize::MAX);
    out.sort_by(|a, b| order(a).cmp(&order(b)).then_with(|| last_moved(b).cmp(&last_moved(a))).then(b.id.cmp(&a.id)));
    Ok(out)
}

pub fn job(conn: &Connection, id: i64, today: &str) -> Result<Job, String> {
    let j = conn
        .query_row(&format!("SELECT {JOB_COLS} FROM jobs WHERE id = ?1"), [id], job_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("投递"))?;
    let steps = steps_by_job(conn, Some(id))?.remove(&id).unwrap_or_default();
    Ok(derive(j, steps, &base_statuses(conn)?, today))
}

/// A new application's steps: every status that isn't 准备 / 结束, none checked.
fn default_steps(conn: &Connection) -> Result<Vec<Step>, String> {
    Ok(base_statuses(conn)?
        .into_iter()
        .filter(|s| !matches!(s.stage, Stage::Wish | Stage::Closed))
        .map(|s| Step { status_id: s.id, done: false, done_on: None })
        .collect())
}

fn clean_steps(conn: &Connection, steps: Vec<Step>) -> Result<Vec<Step>, String> {
    let sts = base_statuses(conn)?;
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(steps.len());
    for s in steps {
        let st = sts.iter().find(|x| x.id == s.status_id).ok_or_else(|| gone("状态"))?;
        if st.stage == Stage::Wish {
            return Err(format!("「{}」是准备阶段，不能放进时间线", st.name));
        }
        if !seen.insert(s.status_id) {
            return Err(format!("「{}」重复了", st.name));
        }
        let done_on = if s.done { common::clean(s.done_on) } else { None };
        common::check_date(&done_on, &format!("「{}」的日期", st.name))?;
        out.push(Step { status_id: s.status_id, done: s.done, done_on });
    }
    Ok(out)
}

fn write_steps(conn: &Connection, job_id: i64, steps: &[Step]) -> Result<(), String> {
    conn.execute("DELETE FROM job_steps WHERE job_id = ?1", [job_id]).map_err(err)?;
    for s in steps {
        conn.execute(
            "INSERT INTO job_steps (job_id, status_id, done, done_on) VALUES (?1, ?2, ?3, ?4)",
            params![job_id, s.status_id, s.done, s.done_on],
        )
        .map_err(err)?;
    }
    Ok(())
}

struct JobFields {
    company: String,
    position: Option<String>,
    location: Option<String>,
    link: Option<String>,
    kind_id: Option<i64>,
    resume: Option<String>,
    note: Option<String>,
}

impl JobFields {
    fn validated(self, conn: &Connection) -> Result<Self, String> {
        let f = Self {
            company: common::require(&self.company, "公司", 100)?,
            position: common::clean(self.position),
            location: common::clean(self.location),
            link: common::clean(self.link),
            resume: common::clean(self.resume),
            note: common::clean(self.note),
            ..self
        };
        common::check_len(&f.position, "岗位", 100)?;
        common::check_len(&f.location, "地点", 50)?;
        common::check_len(&f.link, "JD 链接", 1000)?;
        common::check_len(&f.resume, "简历", 1000)?;
        common::check_len(&f.note, "备注", 5000)?;
        kinds::check(conn, CHANNELS, f.kind_id).map_err(|_| "选择的渠道不存在，可能已经被删除了".to_string())?;
        Ok(f)
    }
}

pub fn create_job(conn: &Connection, p: JobPatch, today: &str) -> Result<Job, String> {
    let f = JobFields {
        company: p.company.unwrap_or_default(),
        position: p.position.flatten(),
        location: p.location.flatten(),
        link: p.link.flatten(),
        kind_id: p.kind_id.flatten(),
        resume: p.resume.flatten(),
        note: p.note.flatten(),
    }
    .validated(conn)?;
    let steps = clean_steps(conn, match p.steps {
        Some(s) => s,
        None => default_steps(conn)?,
    })?;
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        "INSERT INTO jobs (company, position, location, link, kind_id, resume, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![f.company, f.position, f.location, f.link, f.kind_id, f.resume, f.note],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    write_steps(&tx, id, &steps)?;
    tx.commit().map_err(err)?;
    job(conn, id, today)
}

pub fn update_job(conn: &Connection, id: i64, p: JobPatch, today: &str) -> Result<Job, String> {
    let old = job(conn, id, today)?;
    let f = JobFields {
        company: p.company.unwrap_or(old.company.clone()),
        position: p.position.unwrap_or(old.position.clone()),
        location: p.location.unwrap_or(old.location.clone()),
        link: p.link.unwrap_or(old.link.clone()),
        kind_id: p.kind_id.unwrap_or(old.kind_id),
        resume: p.resume.unwrap_or(old.resume.clone()),
        note: p.note.unwrap_or(old.note.clone()),
    }
    .validated(conn)?;
    let steps = p.steps.map(|s| clean_steps(conn, s)).transpose()?;
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        &format!(
            "UPDATE jobs SET company = ?1, position = ?2, location = ?3, link = ?4, kind_id = ?5, resume = ?6, note = ?7,
               updated_at = {NOW} WHERE id = ?8"
        ),
        params![f.company, f.position, f.location, f.link, f.kind_id, f.resume, f.note, id],
    )
    .map_err(err)?;
    if let Some(s) = steps.filter(|s| *s != old.steps) {
        write_steps(&tx, id, &s)?;
    }
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

/// Deletes the application with its steps.
pub fn delete_job(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM jobs WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(gone("投递")),
        _ => Ok(()),
    }
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
pub struct StepRecord {
    pub status: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
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
    pub note: Option<String>,
    #[serde(default)]
    pub steps: Vec<StepRecord>,
    #[serde(default)]
    pub followed_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    // v8 and older (one current status): turned into steps and 备注 like migration 19
    #[serde(default, skip_serializing)]
    pub status: Option<String>,
    #[serde(default, skip_serializing)]
    pub reached: Option<String>,
    #[serde(default, skip_serializing)]
    pub applied_on: Option<String>,
    #[serde(default, skip_serializing)]
    pub step_on: Option<String>,
    #[serde(default, skip_serializing)]
    pub deadline: Option<String>,
    #[serde(default, skip_serializing)]
    pub salary: Option<String>,
    #[serde(default, skip_serializing)]
    pub cover_letter: Option<String>,
}

impl JobRecord {
    /// (备注, steps by status name). `sts` = the status list the record's names refer to.
    fn converted(&self, sts: &[StatusRecord]) -> Result<(Option<String>, Vec<StepRecord>), String> {
        let Some(cur) = self.status.as_deref().map(str::trim) else {
            return Ok((common::clean(self.note.clone()), self.steps.clone()));
        };
        let at = |n: &str| sts.iter().position(|s| s.name == n.trim()).ok_or_else(|| format!("找不到状态「{n}」"));
        let cur_i = at(cur)?;
        let reached_i = match self.reached.as_deref() {
            Some(r) => Some(at(r)?),
            None => Some(cur_i),
        }
        .filter(|&i| sts[i].stage != Stage::Closed);
        let mut steps = Vec::new();
        for (i, s) in sts.iter().enumerate() {
            let closed_now = s.stage == Stage::Closed && i == cur_i;
            if !(closed_now || !matches!(s.stage, Stage::Wish | Stage::Closed)) {
                continue;
            }
            let done = closed_now || reached_i.is_some_and(|r| i <= r);
            let on = match () {
                _ if !done || closed_now => None,
                _ if i == cur_i && s.stage == Stage::Active => common::clean(self.step_on.clone()),
                _ if s.stage == Stage::Applied => common::clean(self.applied_on.clone()),
                _ => None,
            };
            steps.push(StepRecord { status: s.name.clone(), done, on });
        }
        let extra = [("截止日期", &self.deadline), ("薪资", &self.salary), ("求职信", &self.cover_letter)]
            .into_iter()
            .filter_map(|(label, v)| common::clean(v.clone()).map(|v| format!("{label}：{v}")));
        let note = [common::clean(self.note.clone())].into_iter().flatten().chain(extra).collect::<Vec<_>>().join("\n");
        Ok((common::clean(Some(note)), steps))
    }
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

/// Checks a record against a status list (the file's, or the app's when the file has none).
fn check_record(j: &JobRecord, sts: &[StatusRecord]) -> Result<(), String> {
    common::require(&j.company, "公司", 100)?;
    for (d, what) in [(&j.applied_on, "投递日期"), (&j.step_on, "这一步的日期"), (&j.deadline, "截止日期"), (&j.followed_on, "跟进日期")] {
        common::check_date(d, what)?;
    }
    kinds::check_name(&j.channel)?;
    let (_, steps) = j.converted(sts)?;
    let mut seen = HashSet::new();
    for s in &steps {
        let st = sts.iter().find(|x| x.name == s.status.trim()).ok_or_else(|| format!("找不到状态「{}」", s.status))?;
        if st.stage == Stage::Wish {
            return Err(format!("「{}」是准备阶段，不能放进时间线", st.name));
        }
        if !seen.insert(st.name.as_str()) {
            return Err(format!("「{}」重复了", st.name));
        }
        common::check_date(&s.on, &format!("「{}」的日期", st.name))?;
    }
    Ok(())
}

pub fn validate_records(rec: &JobsRecord) -> Result<(), String> {
    let mut names = HashSet::new();
    for s in &rec.statuses {
        let n = common::require(&s.name, "状态名", 20).map_err(|e| format!("{LABEL}的状态：{e}"))?;
        if !names.insert(n.clone()) {
            return Err(format!("{LABEL}：状态「{n}」重复了"));
        }
    }
    if rec.statuses.is_empty() {
        return Ok(()); // checked against the app's statuses in replace_all
    }
    for (i, j) in rec.jobs.iter().enumerate() {
        check_record(j, &rec.statuses).map_err(|e| format!("{LABEL}第 {} 条（{}）：{e}", i + 1, j.company.trim()))?;
    }
    Ok(())
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM jobs", [], |r| r.get(0)).map_err(err)
}

/// Replaces all applications (and the statuses, if the file has them). Caller owns the transaction.
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
    let sts = base_statuses(conn)?;
    let names: Vec<StatusRecord> = sts.iter().map(|s| StatusRecord { name: s.name.clone(), stage: s.stage }).collect();
    let ids: HashMap<&str, i64> = sts.iter().map(|s| (s.name.as_str(), s.id)).collect();
    for (i, j) in rec.jobs.iter().enumerate() {
        check_record(j, &names).map_err(|e| format!("{LABEL}第 {} 条（{}）：{e}", i + 1, j.company.trim()))?;
        let (note, steps) = j.converted(&names)?;
        let channel = kinds::resolve(conn, CHANNELS, j.channel.as_deref())?;
        conn.execute(
            &format!(
                "INSERT INTO jobs (company, position, location, link, kind_id, resume, note, followed_on, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, coalesce(?9, {NOW}), coalesce(?10, ?9, {NOW}))"
            ),
            params![
                j.company.trim(), common::clean(j.position.clone()), common::clean(j.location.clone()), common::clean(j.link.clone()), channel,
                common::clean(j.resume.clone()), note, common::clean(j.followed_on.clone()), j.created_at, j.updated_at
            ],
        )
        .map_err(err)?;
        let job_id = conn.last_insert_rowid();
        let steps: Vec<Step> = steps
            .iter()
            .map(|s| Step { status_id: ids[s.status.trim()], done: s.done, done_on: if s.done { common::clean(s.on.clone()) } else { None } })
            .collect();
        write_steps(conn, job_id, &steps)?;
    }
    Ok(rec.len())
}

pub fn export(conn: &Connection) -> Result<JobsRecord, String> {
    let sts = base_statuses(conn)?;
    let name: HashMap<i64, String> = sts.iter().map(|s| (s.id, s.name.clone())).collect();
    let channels = kinds::name_map(conn, CHANNELS)?;
    let mut all = jobs(conn, &todo::today(conn)?)?;
    all.sort_by_key(|j| j.id);
    let jobs = all
        .into_iter()
        .map(|j| JobRecord {
            company: j.company,
            position: j.position,
            location: j.location,
            link: j.link,
            channel: j.kind_id.and_then(|k| channels.get(&k).cloned()),
            resume: j.resume,
            note: j.note,
            steps: j.steps.into_iter().map(|s| StepRecord { status: name[&s.status_id].clone(), done: s.done, on: s.done_on }).collect(),
            followed_on: j.followed_on,
            created_at: Some(j.created_at),
            updated_at: Some(j.updated_at),
            status: None,
            reached: None,
            applied_on: None,
            step_on: None,
            deadline: None,
            salary: None,
            cover_letter: None,
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
pub fn job_status_create(state: State<AppState>, name: String, stage: Option<Stage>) -> Result<Status, String> {
    state.with_undo("添加状态", |c| create_status(c, &name, stage.unwrap_or(Stage::Active)))
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
        base_statuses(c).unwrap().into_iter().find(|s| s.name == name).unwrap().id
    }
    /// Steps as (status name, done, day).
    fn shape(c: &Connection, j: &Job) -> Vec<(String, bool, Option<String>)> {
        let sts = base_statuses(c).unwrap();
        j.steps.iter().map(|s| (sts.iter().find(|x| x.id == s.status_id).unwrap().name.clone(), s.done, s.done_on.clone())).collect()
    }
    fn step(id: i64, done: bool, on: Option<&str>) -> serde_json::Value {
        json!({"statusId": id, "done": done, "doneOn": on})
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
    fn steps_make_the_status() {
        let c = memory_db();
        let t = "2026-10-08";
        let j = create_job(&c, jp(json!({"company": " 中国银行 ", "position": "实习"})), t).unwrap();
        // default steps: not 想投 / 被拒, none checked → 想投
        assert_eq!(shape(&c, &j).iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["已投递", "网申测评 / 笔试", "面试", "Offer", "已接受"]);
        assert_eq!((j.company.as_str(), j.status_id, j.stage, j.applied_on.clone()), ("中国银行", Some(sid(&c, "想投")), Stage::Wish, None));
        assert!(create_job(&c, jp(json!({"company": " "})), t).is_err());
        // BOCSG: no 网申 / 笔试; checked steps (any order in) come back in list order
        let steps = json!([step(sid(&c, "面试"), true, Some("2026-08-13")), step(sid(&c, "已投递"), true, Some("2026-07-20")), step(sid(&c, "Offer"), false, Some("2026-09-01"))]);
        let j = update_job(&c, j.id, jp(json!({"steps": steps})), t).unwrap();
        assert_eq!(
            shape(&c, &j),
            [("已投递".into(), true, Some("2026-07-20".into())), ("面试".into(), true, Some("2026-08-13".into())), ("Offer".into(), false, None)]
        );
        assert_eq!((j.status_id, j.stage, j.applied_on.as_deref()), (Some(sid(&c, "面试")), Stage::Active, Some("2026-07-20")));
        // other fields alone leave the steps
        let j = update_job(&c, j.id, jp(json!({"note": "金融科技部"})), t).unwrap();
        assert_eq!(j.steps.len(), 3);
        // bad steps
        for bad in [json!([step(sid(&c, "想投"), true, None)]), json!([step(999, false, None)]), json!([step(sid(&c, "面试"), true, Some("2026-13-01"))]),
            json!([step(sid(&c, "面试"), false, None), step(sid(&c, "面试"), true, None)])] {
            assert!(update_job(&c, j.id, jp(json!({"steps": bad})), t).is_err());
        }
        assert!(serde_json::from_value::<JobPatch>(json!({"salary": "20k"})).is_err());
        // rejected: 被拒 picked and checked → current, and still counts as having been applied
        let k = create_job(&c, jp(json!({"company": "KeyReply", "steps": [step(sid(&c, "已投递"), true, Some("2026-09-01")), step(sid(&c, "被拒"), true, Some("2026-09-20"))]})), t).unwrap();
        assert_eq!((k.stage, k.applied_on.as_deref()), (Stage::Closed, Some("2026-09-01")));
        let counts: HashMap<String, i64> = statuses(&c).unwrap().into_iter().map(|s| (s.name, s.count)).collect();
        assert_eq!((counts["面试"], counts["被拒"], counts["想投"]), (1, 1, 0));
        delete_job(&c, k.id).unwrap();
        assert!(delete_job(&c, k.id).is_err());
    }

    #[test]
    fn follow_up_after_two_weeks() {
        let c = memory_db();
        let j = create_job(&c, jp(json!({"company": "A", "steps": [step(sid(&c, "已投递"), true, Some("2026-09-20")), step(sid(&c, "面试"), false, None)]})), "2026-10-03").unwrap();
        assert_eq!((j.waiting_days, j.follow_up), (Some(13), false));
        assert!(job(&c, j.id, "2026-10-04").unwrap().follow_up);
        follow_up(&c, j.id, "2026-10-05").unwrap();
        let later = job(&c, j.id, "2026-10-18").unwrap();
        assert_eq!((later.waiting_days, later.follow_up), (Some(13), false));
        assert!(job(&c, j.id, "2026-10-19").unwrap().follow_up);
        let moved = update_job(&c, j.id, jp(json!({"steps": [step(sid(&c, "已投递"), true, Some("2026-09-20")), step(sid(&c, "面试"), true, Some("2026-10-19"))]})), "2026-10-19").unwrap();
        assert_eq!((moved.waiting_days, moved.follow_up), (None, false)); // only while waiting
    }

    #[test]
    fn list_order_and_status_rules() {
        let c = memory_db();
        let t = "2026-10-08";
        let a = create_job(&c, jp(json!({"company": "A"})), t).unwrap();
        let b = create_job(&c, jp(json!({"company": "B", "steps": [step(sid(&c, "已投递"), true, Some("2026-10-01"))]})), t).unwrap();
        let d = create_job(&c, jp(json!({"company": "D", "steps": [step(sid(&c, "已投递"), true, Some("2026-10-05"))]})), t).unwrap();
        assert_eq!(jobs(&c, t).unwrap().iter().map(|j| j.id).collect::<Vec<_>>(), [a.id, d.id, b.id]); // 想投 first, then newest move
        // a status on someone's steps can't be deleted or become 准备; an unused one can
        let hr = create_status(&c, "HR 面", Stage::Active).unwrap();
        update_job(&c, b.id, jp(json!({"steps": [step(sid(&c, "已投递"), true, Some("2026-10-01")), step(hr.id, false, None)]})), t).unwrap();
        assert!(delete_status(&c, hr.id).unwrap_err().contains("时间线"));
        assert!(update_status(&c, hr.id, None, Some(Stage::Wish)).is_err());
        assert!(delete_status(&c, sid(&c, "想投")).unwrap_err().contains("1 条")); // A is 想投
        delete_job(&c, a.id).unwrap();
        delete_status(&c, sid(&c, "想投")).unwrap();
        assert_eq!(job(&c, d.id, t).unwrap().status_id, Some(sid(&c, "已投递")));
        let fresh = create_job(&c, jp(json!({"company": "E"})), t).unwrap();
        assert_eq!((fresh.status_id, fresh.stage), (None, Stage::Wish)); // no 准备 status left
        let ids: Vec<i64> = base_statuses(&c).unwrap().iter().rev().map(|s| s.id).collect();
        reorder_statuses(&c, &ids).unwrap();
        assert_eq!(statuses(&c).unwrap()[0].id, *ids.first().unwrap());
    }

    #[test]
    fn records_roundtrip() {
        let c = memory_db();
        let rec: JobsRecord = serde_json::from_value(json!({
            "statuses": [{"name": "想投", "stage": "wish"}, {"name": "已投递", "stage": "applied"}, {"name": "笔试", "stage": "active"},
                         {"name": "面试", "stage": "active"}, {"name": "已接受", "stage": "got"}, {"name": "被拒", "stage": "closed"}],
            "jobs": [
                {"company": "BOC", "position": "实习", "location": "新加坡", "channel": "学校", "resume": "~/Documents/Seeker/cv/a.docx",
                 "steps": [{"status": "已投递", "done": true, "on": "2026-07-20"}, {"status": "面试", "done": true}, {"status": "已接受", "done": true, "on": "2026-08-20"}]},
                {"company": "KeyReply", "channel": "新渠道", "followedOn": "2026-09-10",
                 "steps": [{"status": "已投递", "done": true, "on": "2026-09-01"}, {"status": "被拒", "done": true}]},
                {"company": "X", "steps": [{"status": "已投递"}, {"status": "笔试"}]}
            ]
        }))
        .unwrap();
        assert_eq!(replace_all(&c, &rec).unwrap(), 3);
        let out = export(&c).unwrap();
        let c2 = memory_db();
        replace_all(&c2, &out).unwrap();
        assert_eq!(serde_json::to_value(&out).unwrap(), serde_json::to_value(export(&c2).unwrap()).unwrap());
        let v = serde_json::to_value(&out).unwrap();
        assert_eq!(v["jobs"][0]["steps"][1], json!({"status": "面试", "done": true}));
        assert!(v["jobs"][2].get("status").is_none());
        assert!(kinds::list(&c, CHANNELS).unwrap().iter().any(|k| k.name == "新渠道"));

        let bad = |v: serde_json::Value| validate_records(&serde_json::from_value(v).unwrap()).is_err();
        assert!(bad(json!({"statuses": [{"name": "a", "stage": "applied"}], "jobs": [{"company": "X", "steps": [{"status": "b"}]}]})));
        assert!(bad(json!({"statuses": [{"name": "a", "stage": "wish"}], "jobs": [{"company": "X", "steps": [{"status": "a"}]}]})));
        assert!(bad(json!({"statuses": [{"name": "a", "stage": "applied"}], "jobs": [{"company": " "}]})));
        // without a status list, names are checked against the app's
        let c3 = memory_db();
        let e = replace_all(&c3, &serde_json::from_value(json!({"jobs": [{"company": "X", "steps": [{"status": "没有"}]}]})).unwrap()).unwrap_err();
        assert!(e.contains("没有"), "{e}");
    }

    #[test]
    fn reads_v8_records() {
        // one current status + 最远到过, dates, and the fields that are gone (like migration 19)
        let rec: JobsRecord = serde_json::from_value(json!({"jobs": [
            {"company": "A", "status": "想投", "deadline": "2026-10-20", "salary": "20k"},
            {"company": "B", "status": "面试", "reached": "面试", "appliedOn": "2026-10-01", "stepOn": "2026-10-09", "note": "内推"},
            {"company": "C", "status": "被拒", "reached": "已投递", "appliedOn": "2026-09-01", "coverLetter": "~/cl.docx"}
        ]}))
        .unwrap();
        let c = memory_db();
        replace_all(&c, &rec).unwrap();
        let all = jobs(&c, "2026-10-08").unwrap();
        let by = |n: &str| all.iter().find(|j| j.company == n).unwrap().clone();
        let s = |d: &str| Some(d.to_string());
        assert_eq!((by("A").stage, by("A").note), (Stage::Wish, s("截止日期：2026-10-20\n薪资：20k")));
        assert_eq!(
            shape(&c, &by("B")),
            [("已投递".into(), true, s("2026-10-01")), ("网申测评 / 笔试".into(), true, None), ("面试".into(), true, s("2026-10-09")), ("Offer".into(), false, None), ("已接受".into(), false, None)]
        );
        let cc = by("C");
        assert_eq!((cc.stage, cc.steps.len(), cc.note), (Stage::Closed, 6, s("求职信：~/cl.docx")));
        assert_eq!(cc.applied_on.as_deref(), Some("2026-09-01"));
    }
}
