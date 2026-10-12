//! 一步一步 (todo, modeled on the app GoalMap): groups → goals → tasks → multi-level subtasks.
//! Placement: every task has group_id (+ goal_id under a goal); subtasks inherit both from their root
//! and only root tasks can be moved. Siblings are ordered by sort_order (new ones append).
//! Completion (`set_done`): checking a task checks its whole subtree, and a parent whose children are
//! all done is checked too (recursively); unchecking a task (or adding an undone subtask) unchecks
//! its ancestors. Goal status stays manual; progress = done leaf tasks / leaf tasks.
//! Dates: normal / progress tasks have none or a [start, due] range (start optional); a repeat task's
//! due date is its current occurrence, derived from the rule (see `TaskFields::validated`).
//! 今天 (`agenda`): undone tasks outside paused/closed goals that are due today or earlier, or have
//! today inside their [start, due] range; check-ins on their scheduled days. 已过期: due before today.
//! (plan_date on normal tasks only exists in data older than v10 and is read as a range start.)

use super::common::{self, err, some, str_enum};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

pub const KEY: &str = "todo";
pub const LABEL: &str = "一步一步";
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";
/// A leaf = a task without subtasks (progress counts leaves).
const IS_LEAF: &str = "NOT EXISTS (SELECT 1 FROM todo_tasks c WHERE c.parent_id = t.id)";

fn gone(what: &str) -> String {
    format!("找不到这个{what}，可能已经被删除了")
}

// ---------- groups ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    /// Undone leaf tasks in the group (including its goals).
    pub open: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn group_sql() -> String {
    format!(
        "SELECT g.id, g.name, g.sort_order,
           (SELECT count(*) FROM todo_tasks t WHERE t.group_id = g.id AND t.done = 0 AND {IS_LEAF}),
           g.created_at, g.updated_at
         FROM todo_groups g"
    )
}

fn group_row(r: &Row) -> rusqlite::Result<Group> {
    Ok(Group { id: r.get(0)?, name: r.get(1)?, sort_order: r.get(2)?, open: r.get(3)?, created_at: r.get(4)?, updated_at: r.get(5)? })
}

pub fn groups(conn: &Connection) -> Result<Vec<Group>, String> {
    let mut st = conn.prepare(&format!("{} ORDER BY g.sort_order, g.id", group_sql())).map_err(err)?;
    let rows = st.query_map([], group_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn group(conn: &Connection, id: i64) -> Result<Group, String> {
    conn.query_row(&format!("{} WHERE g.id = ?1", group_sql()), [id], group_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("分组"))
}

fn clean_group_name(conn: &Connection, name: &str, except: Option<i64>) -> Result<String, String> {
    let name = common::require(name, "分组名", 30)?;
    let dup: i64 = conn
        .query_row(
            "SELECT count(*) FROM todo_groups WHERE name = ?1 AND id != coalesce(?2, -1)",
            params![name, except],
            |r| r.get(0),
        )
        .map_err(err)?;
    if dup > 0 {
        return Err(format!("已经有叫「{name}」的分组了"));
    }
    Ok(name)
}

pub fn create_group(conn: &Connection, name: &str) -> Result<Group, String> {
    let name = clean_group_name(conn, name, None)?;
    conn.execute(
        "INSERT INTO todo_groups (name, sort_order) VALUES (?1, (SELECT coalesce(max(sort_order), 0) + 1 FROM todo_groups))",
        [&name],
    )
    .map_err(err)?;
    group(conn, conn.last_insert_rowid())
}

pub fn rename_group(conn: &Connection, id: i64, name: &str) -> Result<Group, String> {
    group(conn, id)?;
    let name = clean_group_name(conn, name, Some(id))?;
    conn.execute(&format!("UPDATE todo_groups SET name = ?1, updated_at = {NOW} WHERE id = ?2"), params![name, id])
        .map_err(err)?;
    group(conn, id)
}

/// Deletes the group with its goals and tasks (FK cascade).
pub fn delete_group(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM todo_groups WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(gone("分组")),
        _ => Ok(()),
    }
}

// ---------- goals ----------

str_enum!(GoalStatus, default = Active, {
    Active => "active",
    Paused => "paused",
    Done => "done",
    // gone since v16 (an unwanted goal is deleted); still parsed so old data files import (as paused)
    Abandoned => "abandoned",
});

impl GoalStatus {
    /// Old 「已放弃」 reads as 暂停.
    fn current(self) -> Self {
        if self == GoalStatus::Abandoned {
            GoalStatus::Paused
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub id: i64,
    pub group_id: i64,
    pub name: String,
    pub status: GoalStatus,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub note: Option<String>,
    pub review: Option<String>,
    pub sort_order: i64,
    /// Leaf tasks / done leaf tasks.
    pub total: i64,
    pub done: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoalPatch {
    #[serde(default)]
    pub group_id: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<GoalStatus>,
    #[serde(default, deserialize_with = "some")]
    pub start_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub end_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub review: Option<Option<String>>,
}

fn goal_sql() -> String {
    format!(
        "SELECT g.id, g.group_id, g.name, g.status, g.start_date, g.end_date, g.note, g.review, g.sort_order,
           (SELECT count(*) FROM todo_tasks t WHERE t.goal_id = g.id AND {IS_LEAF}),
           (SELECT count(*) FROM todo_tasks t WHERE t.goal_id = g.id AND t.done = 1 AND {IS_LEAF}),
           g.created_at, g.updated_at
         FROM todo_goals g"
    )
}

fn goal_row(r: &Row) -> rusqlite::Result<Goal> {
    Ok(Goal {
        id: r.get(0)?,
        group_id: r.get(1)?,
        name: r.get(2)?,
        status: r.get(3)?,
        start_date: r.get(4)?,
        end_date: r.get(5)?,
        note: r.get(6)?,
        review: r.get(7)?,
        sort_order: r.get(8)?,
        total: r.get(9)?,
        done: r.get(10)?,
        created_at: r.get(11)?,
        updated_at: r.get(12)?,
    })
}

/// All goals, each group's in order.
pub fn goals(conn: &Connection) -> Result<Vec<Goal>, String> {
    let mut st = conn.prepare(&format!("{} ORDER BY g.sort_order, g.id", goal_sql())).map_err(err)?;
    let rows = st.query_map([], goal_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn goal(conn: &Connection, id: i64) -> Result<Goal, String> {
    conn.query_row(&format!("{} WHERE g.id = ?1", goal_sql()), [id], goal_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("目标"))
}

struct GoalFields {
    group_id: i64,
    name: String,
    status: GoalStatus,
    start_date: Option<String>,
    end_date: Option<String>,
    note: Option<String>,
    review: Option<String>,
}

fn check_range(start: &Option<String>, end: &Option<String>, what: &str) -> Result<(), String> {
    match (start, end) {
        (Some(s), Some(e)) if s > e => Err(format!("{what}的开始日期不能晚于结束日期")),
        _ => Ok(()),
    }
}

fn validate_goal(f: GoalFields) -> Result<GoalFields, String> {
    if f.status == GoalStatus::Abandoned {
        return Err("目标没有「已放弃」了，不要的目标直接删除".into());
    }
    let f = GoalFields {
        name: common::require(&f.name, "目标名", 100)?,
        start_date: common::clean(f.start_date),
        end_date: common::clean(f.end_date),
        note: common::clean(f.note),
        review: common::clean(f.review),
        ..f
    };
    common::check_date(&f.start_date, "开始日期")?;
    common::check_date(&f.end_date, "结束日期")?;
    check_range(&f.start_date, &f.end_date, "目标")?;
    common::check_len(&f.note, "备注", 5000)?;
    common::check_len(&f.review, "复盘", 5000)?;
    Ok(f)
}

pub fn create_goal(conn: &Connection, patch: GoalPatch) -> Result<Goal, String> {
    let group_id = patch.group_id.ok_or("请选择分组")?;
    group(conn, group_id)?;
    let f = validate_goal(GoalFields {
        group_id,
        name: patch.name.unwrap_or_default(),
        status: patch.status.unwrap_or_default(),
        start_date: patch.start_date.flatten(),
        end_date: patch.end_date.flatten(),
        note: patch.note.flatten(),
        review: patch.review.flatten(),
    })?;
    conn.execute(
        "INSERT INTO todo_goals (group_id, name, status, start_date, end_date, note, review, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, (SELECT coalesce(max(sort_order), 0) + 1 FROM todo_goals WHERE group_id = ?1))",
        params![f.group_id, f.name, f.status, f.start_date, f.end_date, f.note, f.review],
    )
    .map_err(err)?;
    goal(conn, conn.last_insert_rowid())
}

/// Moving a goal to another group moves its tasks along and appends it there.
pub fn update_goal(conn: &Connection, id: i64, patch: GoalPatch) -> Result<Goal, String> {
    let old = goal(conn, id)?;
    let f = validate_goal(GoalFields {
        group_id: patch.group_id.unwrap_or(old.group_id),
        name: patch.name.unwrap_or(old.name),
        status: patch.status.unwrap_or(old.status),
        start_date: patch.start_date.unwrap_or(old.start_date),
        end_date: patch.end_date.unwrap_or(old.end_date),
        note: patch.note.unwrap_or(old.note),
        review: patch.review.unwrap_or(old.review),
    })?;
    let tx = common::Tx::begin(conn)?;
    if f.group_id != old.group_id {
        group(&tx, f.group_id)?;
        tx.execute(
            "UPDATE todo_goals SET group_id = ?1,
               sort_order = (SELECT coalesce(max(sort_order), 0) + 1 FROM todo_goals WHERE group_id = ?1) WHERE id = ?2",
            params![f.group_id, id],
        )
        .map_err(err)?;
        tx.execute("UPDATE todo_tasks SET group_id = ?1 WHERE goal_id = ?2", params![f.group_id, id]).map_err(err)?;
    }
    tx.execute(
        &format!(
            "UPDATE todo_goals SET name = ?1, status = ?2, start_date = ?3, end_date = ?4, note = ?5, review = ?6,
               updated_at = {NOW} WHERE id = ?7"
        ),
        params![f.name, f.status, f.start_date, f.end_date, f.note, f.review, id],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    goal(conn, id)
}

/// Deletes the goal and its tasks (FK cascade).
pub fn delete_goal(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM todo_goals WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(gone("目标")),
        _ => Ok(()),
    }
}

// ---------- task types & repeat rules ----------

str_enum!(TaskType, default = Normal, {
    Normal => "normal",
    Repeat => "repeat",
    Checkin => "checkin",
    Progress => "progress",
});

str_enum!(Period, default = All, {
    All => "all",
    Day => "day",
    Week => "week",
    Month => "month",
});

/// Parsed `repeat_rule`: daily | weekly:<1-7,…> (Mon=1) | monthly:<1-31,…> | yearly:<MM-DD>[/<n> years]
/// | every:<n>d | every:<n>w | after:<n><d|w|m|y>. Monthly days past a month's end fall on its last
/// day; yearly 02-29 falls on 02-28 in other years; after-months keep the day (or the month's end).
#[derive(Debug, Clone, PartialEq)]
pub enum Rule {
    Daily,
    Weekly(Vec<u32>),
    Monthly(Vec<u32>),
    /// Month, day, every how many years.
    Yearly(u32, u32, i64),
    EveryDays(i64),
    EveryWeeks(i64),
    /// 完成后: n of a unit (d / w / m / y) after the day it was done.
    After(i64, char),
}

use common::add_months;

/// MM-DD in year `y` (02-29 → 02-28 when not a leap year).
fn on_year(y: i64, m: u32, d: u32) -> String {
    format!("{y:04}-{m:02}-{:02}", d.min(common::days_in_month(y, m)))
}

impl Rule {
    pub fn parse(s: &str) -> Result<Rule, String> {
        let bad = || format!("重复规则不对：{s}");
        let num = |v: &str, max: i64| v.parse::<i64>().ok().filter(|n| (1..=max).contains(n)).ok_or_else(bad);
        let unit = |v: &str, u: char| v.strip_suffix(u).map(str::to_string);
        let days = |v: &str, max: i64| -> Result<Vec<u32>, String> {
            let mut ds = v.split(',').map(|d| num(d, max).map(|n| n as u32)).collect::<Result<Vec<_>, _>>()?;
            ds.sort_unstable();
            ds.dedup();
            Ok(ds)
        };
        Ok(match s.split_once(':') {
            None if s == "daily" => Rule::Daily,
            Some(("weekly", ds)) => Rule::Weekly(days(ds, 7)?),
            Some(("monthly", ds)) => Rule::Monthly(days(ds, 31)?),
            Some(("yearly", v)) => {
                let (md, every) = v.split_once('/').map_or((v, "1"), |(a, b)| (a, b));
                let (m, d) = md.split_once('-').ok_or_else(bad)?;
                let m = num(m, 12)? as u32;
                let d = num(d, common::days_in_month(2000, m) as i64)? as u32; // 2000: leap, so 02-29 is fine
                Rule::Yearly(m, d, num(every, 50)?)
            }
            Some(("every", n)) => match (unit(n, 'd'), unit(n, 'w')) {
                (Some(d), _) => Rule::EveryDays(num(&d, 365)?),
                (_, Some(w)) => Rule::EveryWeeks(num(&w, 52)?),
                _ => return Err(bad()),
            },
            Some(("after", n)) => {
                let u = n.chars().last().ok_or_else(bad)?;
                let max = match u {
                    'd' => 365,
                    'w' => 52,
                    'm' => 120,
                    'y' => 20,
                    _ => return Err(bad()),
                };
                Rule::After(num(&n[..n.len() - 1], max)?, u)
            }
            _ => return Err(bad()),
        })
    }

    /// Next occurrence after `anchor` (after-rules count from `completed`).
    pub fn next(&self, anchor: &str, completed: &str) -> String {
        match self {
            Rule::Daily => common::add_days(anchor, 1),
            Rule::Weekly(days) => (1..=7).map(|i| common::add_days(anchor, i)).find(|d| days.contains(&common::weekday(d))).unwrap(),
            Rule::Monthly(days) => {
                let (mut y, mut m) = (anchor[..4].parse::<i64>().unwrap(), anchor[5..7].parse::<u32>().unwrap());
                loop {
                    let last = common::days_in_month(y, m);
                    if let Some(d) = days.iter().map(|d| format!("{y:04}-{m:02}-{:02}", (*d).min(last))).find(|d| d.as_str() > anchor) {
                        return d;
                    }
                    (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                }
            }
            Rule::Yearly(m, d, every) => {
                // the first MM-DD after the anchor, then every-1 more years
                let y = anchor[..4].parse::<i64>().unwrap();
                let first = if on_year(y, *m, *d).as_str() > anchor { y } else { y + 1 };
                on_year(first + every - 1, *m, *d)
            }
            Rule::EveryDays(n) => common::add_days(anchor, *n),
            Rule::EveryWeeks(n) => common::add_days(anchor, 7 * n),
            Rule::After(n, u) => match u {
                'w' => common::add_days(completed, 7 * n),
                'm' => add_months(completed, *n),
                'y' => add_months(completed, 12 * n),
                _ => common::add_days(completed, *n),
            },
        }
    }

    /// First occurrence on or after `day` (interval rules start on `day` itself).
    pub fn first_from(&self, day: &str) -> String {
        match self {
            Rule::Weekly(_) | Rule::Monthly(_) => self.next(&common::add_days(day, -1), day),
            Rule::Yearly(m, d, _) => Rule::Yearly(*m, *d, 1).next(&common::add_days(day, -1), day),
            _ => day.to_string(),
        }
    }

    /// Series that end by count (一共几次); after-rules (完成后) run from a start date and never end.
    fn counted(&self) -> bool {
        !matches!(self, Rule::After(..))
    }

    /// Check-in schedule (daily / weekly rules only).
    fn scheduled(&self, day: &str) -> bool {
        match self {
            Rule::Daily => true,
            Rule::Weekly(ds) => ds.contains(&common::weekday(day)),
            _ => false,
        }
    }
}

// ---------- tasks ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: i64,
    pub group_id: i64,
    pub goal_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub title: String,
    pub note: Option<String>,
    pub link: Option<String>,
    pub plan_date: Option<String>,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub done: bool,
    pub done_at: Option<String>,
    pub collapsed: bool,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    pub task_type: TaskType,
    pub repeat_rule: Option<String>,
    pub repeat_total: Option<i64>,
    pub repeat_until: Option<String>,
    pub target: Option<i64>,
    pub unit: Option<String>,
    pub period: Option<Period>,
    /// Repeat: occurrences done before logging started.
    pub done_before: i64,
    /// Derived (`enrich`): repeat = occurrences done, checkin = days checked, progress = amount this period.
    pub count: i64,
    /// Derived, repeat series with a total: date of the last occurrence.
    pub last_date: Option<String>,
    /// Derived, progress: amount all-time.
    pub total_amount: i64,
    /// Derived: has a log today.
    pub logged_today: bool,
    /// Derived, checkin: consecutive scheduled days checked up to today (today may still be open).
    pub streak: i64,
}

/// Create: place by `parentId` > `goalId` > `groupId`. Update: `groupId` / `goalId` move a root task
/// (with its subtree); `parentId` is create-only. Completion goes through `set_done` / logs.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskPatch {
    #[serde(default)]
    pub group_id: Option<i64>,
    #[serde(default, deserialize_with = "some")]
    pub goal_id: Option<Option<i64>>,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub link: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub plan_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub start_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub due_date: Option<Option<String>>,
    #[serde(default)]
    pub collapsed: Option<bool>,
    #[serde(default)]
    pub task_type: Option<TaskType>,
    #[serde(default, deserialize_with = "some")]
    pub repeat_rule: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub repeat_total: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub repeat_until: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub target: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub unit: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub period: Option<Option<Period>>,
    /// Repeat: occurrences done so far (sets `done_before` = this − logs).
    #[serde(default)]
    pub done_count: Option<i64>,
}

const TASK_COLS: &str = "t.id, t.group_id, t.goal_id, t.parent_id, t.title, t.note, t.link, t.plan_date, t.start_date,
  t.due_date, t.done, t.done_at, t.collapsed, t.sort_order, t.created_at, t.updated_at,
  t.task_type, t.repeat_rule, t.repeat_total, t.repeat_until, t.target, t.unit, t.period, t.done_before";
const TASK_NCOLS: usize = 24;

fn task_row(r: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        group_id: r.get(1)?,
        goal_id: r.get(2)?,
        parent_id: r.get(3)?,
        title: r.get(4)?,
        note: r.get(5)?,
        link: r.get(6)?,
        plan_date: r.get(7)?,
        start_date: r.get(8)?,
        due_date: r.get(9)?,
        done: r.get(10)?,
        done_at: r.get(11)?,
        collapsed: r.get(12)?,
        sort_order: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
        task_type: r.get(16)?,
        repeat_rule: r.get(17)?,
        repeat_total: r.get(18)?,
        repeat_until: r.get(19)?,
        target: r.get(20)?,
        unit: r.get(21)?,
        period: r.get(22)?,
        done_before: r.get(23)?,
        count: 0,
        last_date: None,
        total_amount: 0,
        logged_today: false,
        streak: 0,
    })
}

fn count_of(conn: &Connection, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<i64, String> {
    conn.query_row(sql, args, |r| r.get(0)).map_err(err)
}

/// Fills the derived counters (no-op for normal tasks).
fn enrich(conn: &Connection, t: &mut Task, today: &str) -> Result<(), String> {
    if t.task_type == TaskType::Normal {
        return Ok(());
    }
    t.logged_today = count_of(conn, "SELECT count(*) FROM todo_logs WHERE task_id = ?1 AND day = ?2", &[&t.id, &today])? > 0;
    match t.task_type {
        TaskType::Repeat => {
            t.count = t.done_before + count_of(conn, "SELECT count(*) FROM todo_logs WHERE task_id = ?1", &[&t.id])?;
            t.last_date = last_date(t);
        }
        TaskType::Checkin => t.count = count_of(conn, "SELECT count(*) FROM todo_logs WHERE task_id = ?1", &[&t.id])?,
        TaskType::Progress => {
            t.total_amount = count_of(conn, "SELECT coalesce(sum(amount), 0) FROM todo_logs WHERE task_id = ?1", &[&t.id])?;
            t.count = t.total_amount;
        }
        TaskType::Normal => {}
    }
    if t.task_type == TaskType::Checkin {
        t.streak = streak(conn, t, today)?;
    }
    Ok(())
}

/// Open counted series: walk from the current occurrence to the last one.
fn last_date(t: &Task) -> Option<String> {
    let rule = Rule::parse(t.repeat_rule.as_deref()?).ok().filter(Rule::counted)?;
    let left = t.repeat_total? - t.count - 1; // occurrences after the current one
    let mut d = t.due_date.clone().filter(|_| !t.done && left >= 0)?;
    for _ in 0..left {
        d = rule.next(&d, &d);
    }
    Some(d)
}

fn streak(conn: &Connection, t: &Task, today: &str) -> Result<i64, String> {
    let Some(rule) = t.repeat_rule.as_deref().and_then(|r| Rule::parse(r).ok()) else { return Ok(0) };
    let mut st = conn.prepare("SELECT day FROM todo_logs WHERE task_id = ?1").map_err(err)?;
    let days: std::collections::HashSet<String> =
        st.query_map([t.id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let start = t.start_date.clone().unwrap_or_default();
    let mut n = 0;
    for i in 0..=730 {
        let d = common::add_days(today, -i);
        if d < start {
            break;
        }
        if !rule.scheduled(&d) {
            continue;
        }
        if days.contains(&d) {
            n += 1;
        } else if i > 0 {
            break; // today not checked yet doesn't break the streak
        }
    }
    Ok(n)
}

/// Raw row (no derived counters) — for internal rules.
pub fn task(conn: &Connection, id: i64) -> Result<Task, String> {
    conn.query_row(&format!("SELECT {TASK_COLS} FROM todo_tasks t WHERE t.id = ?1"), [id], task_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("任务"))
}

/// Row with derived counters, as the UI sees it.
pub fn task_view(conn: &Connection, id: i64) -> Result<Task, String> {
    let mut t = task(conn, id)?;
    enrich(conn, &mut t, &today(conn)?)?;
    Ok(t)
}

/// Tasks of one place, flat (the UI builds the tree from parent_id): a group's loose tasks
/// (not under any goal), or a goal's tasks.
pub fn tasks(conn: &Connection, group_id: Option<i64>, goal_id: Option<i64>) -> Result<Vec<Task>, String> {
    let (sql, id) = match (goal_id, group_id) {
        (Some(g), _) => {
            goal(conn, g)?;
            ("t.goal_id = ?1", g)
        }
        (None, Some(g)) => {
            group(conn, g)?;
            ("t.group_id = ?1 AND t.goal_id IS NULL", g)
        }
        (None, None) => return Err("请选择分组或目标".into()),
    };
    let mut st = conn
        .prepare(&format!("SELECT {TASK_COLS} FROM todo_tasks t WHERE {sql} ORDER BY t.sort_order, t.id"))
        .map_err(err)?;
    let mut out = st.query_map([id], task_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let today = today(conn)?;
    for t in &mut out {
        enrich(conn, t, &today)?;
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Place {
    group_id: i64,
    goal_id: Option<i64>,
    parent_id: Option<i64>,
}

fn resolve_place(conn: &Connection, group_id: Option<i64>, goal_id: Option<i64>, parent_id: Option<i64>) -> Result<Place, String> {
    if let Some(p) = parent_id {
        let parent = task(conn, p).map_err(|_| gone("上一层任务"))?;
        if parent.task_type != TaskType::Normal {
            return Err("重复、打卡、进度条任务下面不能加子任务".into());
        }
        return Ok(Place { group_id: parent.group_id, goal_id: parent.goal_id, parent_id: Some(p) });
    }
    if let Some(g) = goal_id {
        let goal = goal(conn, g)?;
        return Ok(Place { group_id: goal.group_id, goal_id: Some(g), parent_id: None });
    }
    let g = group_id.ok_or("请选择分组")?;
    group(conn, g)?;
    Ok(Place { group_id: g, goal_id: None, parent_id: None })
}

fn next_sort(conn: &Connection, p: Place) -> Result<i64, String> {
    conn.query_row(
        "SELECT coalesce(max(sort_order), 0) + 1 FROM todo_tasks WHERE group_id = ?1 AND goal_id IS ?2 AND parent_id IS ?3",
        params![p.group_id, p.goal_id, p.parent_id],
        |r| r.get(0),
    )
    .map_err(err)
}

#[derive(Default)]
struct TaskFields {
    title: String,
    note: Option<String>,
    link: Option<String>,
    plan_date: Option<String>,
    start_date: Option<String>,
    due_date: Option<String>,
    task_type: TaskType,
    repeat_rule: Option<String>,
    repeat_total: Option<i64>,
    repeat_until: Option<String>,
    target: Option<i64>,
    unit: Option<String>,
    period: Option<Period>,
    done_before: i64,
}

impl TaskFields {
    fn from_task(t: &Task) -> Self {
        Self {
            title: t.title.clone(),
            note: t.note.clone(),
            link: t.link.clone(),
            plan_date: t.plan_date.clone(),
            start_date: t.start_date.clone(),
            due_date: t.due_date.clone(),
            task_type: t.task_type,
            repeat_rule: t.repeat_rule.clone(),
            repeat_total: t.repeat_total,
            repeat_until: t.repeat_until.clone(),
            target: t.target,
            unit: t.unit.clone(),
            period: t.period,
            done_before: t.done_before,
        }
    }

    fn apply(mut self, p: &TaskPatch) -> Self {
        macro_rules! take {
            ($($f:ident),*) => { $( if let Some(v) = p.$f.clone() { self.$f = v; } )* };
        }
        take!(note, link, plan_date, start_date, due_date, repeat_rule, repeat_total, repeat_until, target, unit, period);
        if let Some(v) = p.title.clone() {
            self.title = v;
        }
        if let Some(v) = p.task_type {
            self.task_type = v;
        }
        self
    }

    /// Normalizes and checks; fields that don't belong to the type are cleared. Dates by type:
    /// normal / progress = optional [start, due] range; repeat = due (current occurrence, set by
    /// create / update) + plan as series start and until as end (monthly) or plan only (after);
    /// checkin = plan as start, until as end.
    fn validated(self, has_children: bool) -> Result<Self, String> {
        let mut f = Self {
            title: common::require(&self.title, "标题", 500)?,
            note: common::clean(self.note),
            link: common::clean(self.link),
            plan_date: common::clean(self.plan_date),
            start_date: common::clean(self.start_date),
            due_date: common::clean(self.due_date),
            repeat_rule: common::clean(self.repeat_rule),
            repeat_until: common::clean(self.repeat_until),
            unit: common::clean(self.unit),
            ..self
        };
        common::check_len(&f.note, "内容", 5000)?;
        common::check_len(&f.link, "链接", 1000)?;
        common::check_date(&f.plan_date, "开始日期")?;
        common::check_date(&f.start_date, "开始日期")?;
        common::check_date(&f.due_date, "截止日期")?;
        common::check_date(&f.repeat_until, "结束日期")?;
        if f.task_type != TaskType::Normal && has_children {
            return Err("有子任务的任务只能是普通任务".into());
        }
        let total_ok = |n: Option<i64>, what: &str| match n {
            Some(n) if !(1..=10000).contains(&n) => Err(format!("{what}要在 1～10000 之间")),
            _ => Ok(()),
        };
        if f.task_type != TaskType::Repeat {
            f.done_before = 0;
        }
        match f.task_type {
            TaskType::Normal | TaskType::Progress => {
                // older data: a plan date folds into the range
                if let Some(p) = f.plan_date.take() {
                    if f.start_date.is_none() && f.due_date.as_deref().is_none_or(|d| p.as_str() <= d) {
                        f.start_date = Some(p.clone());
                    }
                    f.due_date.get_or_insert(p);
                }
                (f.repeat_rule, f.repeat_total, f.repeat_until) = (None, None, None);
                if f.task_type == TaskType::Normal {
                    (f.target, f.unit, f.period) = (None, None, None);
                } else {
                    match f.target {
                        Some(n) if (1..=100000).contains(&n) => {}
                        _ => return Err("请设置目标数量（1～100000）".into()),
                    }
                    common::check_len(&f.unit, "单位", 10)?;
                    f.period = Some(Period::All);
                }
            }
            TaskType::Repeat => {
                let rule = Rule::parse(f.repeat_rule.as_deref().ok_or("请设置重复规则")?)?;
                if rule.counted() {
                    total_ok(f.repeat_total, "一共几次")?;
                    f.plan_date = None;
                } else {
                    f.repeat_total = None;
                }
                f.repeat_until = None;
                if !(0..=10000).contains(&f.done_before) {
                    return Err("已经做了几次要在 0～10000 之间".into());
                }
                (f.start_date, f.target, f.unit, f.period) = (None, None, None, None);
            }
            TaskType::Checkin => {
                let rule = Rule::parse(f.repeat_rule.as_deref().ok_or("请设置打卡的日子")?)?;
                if !matches!(rule, Rule::Daily | Rule::Weekly(_)) {
                    return Err("打卡只能按每天或每周固定几天".into());
                }
                (f.plan_date, f.repeat_total, f.repeat_until, f.target, f.unit, f.period) = (None, None, None, None, None, None);
            }
        }
        if f.start_date.is_some() && f.due_date.is_none() {
            return Err("时间段要有结束日期".into());
        }
        check_range(&f.start_date, &f.due_date, "时间段")?;
        Ok(f)
    }

    /// Repeat: the current occurrence of a new or re-planned series — the first one from today, or
    /// from the series start if that is later.
    fn first_due(&self, today: &str) -> Result<String, String> {
        let rule = Rule::parse(self.repeat_rule.as_deref().unwrap_or_default())?;
        let from = self.plan_date.as_deref().filter(|s| *s > today).unwrap_or(today);
        let d = rule.first_from(from);
        if self.repeat_until.as_deref().is_some_and(|u| d.as_str() > u) {
            return Err("时间段里没有要做的日子".into());
        }
        Ok(d)
    }

    /// Repeat: `done_count` (occurrences done so far, None = unchanged) → done_before, given the
    /// occurrences already logged.
    fn set_done_count(&mut self, done_count: Option<i64>, logged: i64) -> Result<(), String> {
        if let Some(n) = done_count {
            if n < logged {
                return Err(format!("已经记了 {logged} 次，不能少于这个数"));
            }
            self.done_before = n - logged;
        }
        if !(0..=10000).contains(&self.done_before) {
            return Err("已经做了几次要在 0～10000 之间".into());
        }
        if self.repeat_total.is_some_and(|t| self.done_before + logged > t) {
            return Err("已经做了的次数不能多于一共几次".into());
        }
        Ok(())
    }
}

/// Unchecks every checked ancestor of `id` (a subtree below them is no longer all done).
fn reopen_ancestors(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute(
        &format!(
            "WITH RECURSIVE up(id) AS (
               SELECT parent_id FROM todo_tasks WHERE id = ?1 AND parent_id IS NOT NULL
               UNION ALL SELECT t.parent_id FROM todo_tasks t JOIN up ON t.id = up.id WHERE t.parent_id IS NOT NULL)
             UPDATE todo_tasks SET done = 0, done_at = NULL, updated_at = {NOW} WHERE id IN (SELECT id FROM up) AND done = 1"
        ),
        [id],
    )
    .map_err(err)?;
    Ok(())
}

pub fn create_task(conn: &Connection, patch: TaskPatch) -> Result<Task, String> {
    create_task_on(conn, patch, &today(conn)?)
}

pub fn create_task_on(conn: &Connection, patch: TaskPatch, today: &str) -> Result<Task, String> {
    let place = resolve_place(conn, patch.group_id, patch.goal_id.flatten(), patch.parent_id)?;
    let mut f = TaskFields::default().apply(&patch).validated(false)?;
    if f.task_type == TaskType::Repeat {
        f.set_done_count(patch.done_count, 0)?;
        f.due_date = Some(f.first_due(today)?);
    }
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        "INSERT INTO todo_tasks (group_id, goal_id, parent_id, title, note, link, plan_date, start_date, due_date, collapsed, sort_order,
           task_type, repeat_rule, repeat_total, repeat_until, target, unit, period, done_before)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![
            place.group_id, place.goal_id, place.parent_id, f.title, f.note, f.link, f.plan_date, f.start_date, f.due_date,
            patch.collapsed.unwrap_or(false), next_sort(&tx, place)?, f.task_type, f.repeat_rule, f.repeat_total,
            f.repeat_until, f.target, f.unit, f.period, f.done_before
        ],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    reopen_ancestors(&tx, id)?;
    settle(&tx, id, today)?;
    tx.commit().map_err(err)?;
    task_view(conn, id)
}

fn has_children(conn: &Connection, id: i64) -> Result<bool, String> {
    Ok(count_of(conn, "SELECT count(*) FROM todo_tasks WHERE parent_id = ?1", &[&id])? > 0)
}

pub fn update_task(conn: &Connection, id: i64, patch: TaskPatch) -> Result<Task, String> {
    update_task_on(conn, id, patch, &today(conn)?)
}

pub fn update_task_on(conn: &Connection, id: i64, patch: TaskPatch, today: &str) -> Result<Task, String> {
    let old = task(conn, id)?;
    if patch.parent_id.is_some() {
        return Err("不能在这里改上一层任务".into());
    }
    let mut f = TaskFields::from_task(&old).apply(&patch).validated(has_children(conn, id)?)?;
    if f.task_type == TaskType::Repeat {
        f.set_done_count(patch.done_count, count_of(conn, "SELECT count(*) FROM todo_logs WHERE task_id = ?1", &[&id])?)?;
        let replanned = old.task_type != TaskType::Repeat || f.repeat_rule != old.repeat_rule || f.plan_date != old.plan_date;
        if replanned || f.due_date.is_none() {
            f.due_date = Some(f.first_due(today)?);
        }
    }
    let tx = common::Tx::begin(conn)?;
    let moving = patch.group_id.is_some_and(|g| g != old.group_id) || patch.goal_id.is_some_and(|g| g != old.goal_id);
    if moving {
        if old.parent_id.is_some() {
            return Err("子任务跟着上一层任务走，不能单独移动".into());
        }
        let goal_id = patch.goal_id.unwrap_or(old.goal_id);
        let place = resolve_place(&tx, Some(patch.group_id.unwrap_or(old.group_id)), goal_id, None)?;
        tx.execute(
            "UPDATE todo_tasks SET group_id = ?1, goal_id = ?2, sort_order = ?3 WHERE id = ?4",
            params![place.group_id, place.goal_id, next_sort(&tx, place)?, id],
        )
        .map_err(err)?;
        tx.execute(
            "WITH RECURSIVE sub(id) AS (
               SELECT id FROM todo_tasks WHERE parent_id = ?1
               UNION ALL SELECT t.id FROM todo_tasks t JOIN sub ON t.parent_id = sub.id)
             UPDATE todo_tasks SET group_id = ?2, goal_id = ?3 WHERE id IN (SELECT id FROM sub)",
            params![id, place.group_id, place.goal_id],
        )
        .map_err(err)?;
    }
    tx.execute(
        &format!(
            "UPDATE todo_tasks SET title = ?1, note = ?2, link = ?3, plan_date = ?4, start_date = ?5, due_date = ?6,
               collapsed = ?7, task_type = ?8, repeat_rule = ?9, repeat_total = ?10, repeat_until = ?11, target = ?12,
               unit = ?13, period = ?14, done_before = ?15, updated_at = {NOW} WHERE id = ?16"
        ),
        params![
            f.title, f.note, f.link, f.plan_date, f.start_date, f.due_date, patch.collapsed.unwrap_or(old.collapsed),
            f.task_type, f.repeat_rule, f.repeat_total, f.repeat_until, f.target, f.unit, f.period, f.done_before, id
        ],
    )
    .map_err(err)?;
    settle(&tx, id, today)?; // e.g. a lowered target may complete it
    tx.commit().map_err(err)?;
    task_view(conn, id)
}

/// Checks the task and its subtree, then any parent whose children are now all done.
fn mark_done(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute(
        &format!(
            "WITH RECURSIVE sub(id) AS (
               SELECT ?1 UNION ALL SELECT t.id FROM todo_tasks t JOIN sub ON t.parent_id = sub.id)
             UPDATE todo_tasks SET done = 1, done_at = {NOW}, updated_at = {NOW}
             WHERE id IN (SELECT id FROM sub) AND done = 0"
        ),
        [id],
    )
    .map_err(err)?;
    let mut child = id;
    while let Some(parent) = task(conn, child)?.parent_id {
        if count_of(conn, "SELECT count(*) FROM todo_tasks WHERE parent_id = ?1 AND done = 0", &[&parent])? > 0 {
            break;
        }
        conn.execute(&format!("UPDATE todo_tasks SET done = 1, done_at = {NOW}, updated_at = {NOW} WHERE id = ?1 AND done = 0"), [parent])
            .map_err(err)?;
        child = parent;
    }
    Ok(())
}

fn mark_undone(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute(&format!("UPDATE todo_tasks SET done = 0, done_at = NULL, updated_at = {NOW} WHERE id = ?1"), [id])
        .map_err(err)?;
    reopen_ancestors(conn, id)
}

/// Completes / reopens a counted repeat or progress task from its count, and a check-in whose
/// 时间段 is over or fully checked (un-checking a day before the end reopens it).
fn settle(conn: &Connection, id: i64, today: &str) -> Result<(), String> {
    let mut t = task(conn, id)?;
    enrich(conn, &mut t, today)?;
    let reached = match t.task_type {
        TaskType::Repeat => match t.repeat_total {
            Some(n) => t.count >= n,
            None => return Ok(()),
        },
        TaskType::Checkin => match &t.due_date {
            Some(end) => end.as_str() < today || all_checked(conn, &t, end)?, // over, or nothing left to check
            None => return Ok(()),
        },
        TaskType::Progress => t.total_amount >= t.target.unwrap_or(i64::MAX),
        TaskType::Normal => return Ok(()),
    };
    match (reached, t.done) {
        (true, false) => mark_done(conn, id),
        (false, true) => mark_undone(conn, id),
        _ => Ok(()),
    }
}

/// Check-in with an end: every scheduled day from the start (or the day it was added) to the end
/// has its check. A range without any scheduled day doesn't count (it ends with the range).
fn all_checked(conn: &Connection, t: &Task, end: &str) -> Result<bool, String> {
    let Ok(rule) = Rule::parse(t.repeat_rule.as_deref().unwrap_or_default()) else { return Ok(false) };
    let start = match &t.start_date {
        Some(s) => s.clone(),
        None => conn.query_row("SELECT date(?1, 'localtime')", [&t.created_at], |r| r.get(0)).map_err(err)?,
    };
    let mut st = conn.prepare("SELECT day FROM todo_logs WHERE task_id = ?1").map_err(err)?;
    let logged: std::collections::HashSet<String> = st.query_map([t.id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let (a, b) = (common::day_number(&start), common::day_number(end));
    let days: Vec<String> = (a..=b.min(a + 3660)).map(common::from_day_number).filter(|d| rule.scheduled(d)).collect();
    Ok(!days.is_empty() && days.iter().all(|d| logged.contains(d)))
}

/// Moves a repeat task (`count` occurrences done) to its next occurrence, or ends the series when
/// the total or end date is reached. The next one comes after both the current occurrence and the
/// day it was done / skipped, so a late finish doesn't leave a trail of overdue occurrences.
fn advance(conn: &Connection, t: &Task, completed: &str, count: i64) -> Result<(), String> {
    let rule = Rule::parse(t.repeat_rule.as_deref().unwrap_or_default())?;
    let anchor = t.due_date.as_deref().ok_or("重复任务没有日期")?;
    let next = rule.next(anchor.max(completed), completed);
    let ended = t.repeat_total.is_some_and(|n| count >= n) || t.repeat_until.as_deref().is_some_and(|u| next.as_str() > u);
    if ended {
        return mark_done(conn, t.id);
    }
    conn.execute(&format!("UPDATE todo_tasks SET due_date = ?1, updated_at = {NOW} WHERE id = ?2"), params![next, t.id])
        .map_err(err)?;
    Ok(())
}

/// The circle in the list. normal: check / uncheck (cascades). repeat: finish this occurrence (logged,
/// dates move on). checkin: check in / undo today. progress: +1 today. Unchecking a finished
/// repeat / progress task reopens it.
pub fn set_done_on(conn: &Connection, id: i64, done: bool, today: &str) -> Result<(), String> {
    let t = task(conn, id)?;
    let tx = common::Tx::begin(conn)?;
    match (t.task_type, done) {
        (TaskType::Normal, true) => mark_done(&tx, id)?,
        (TaskType::Normal, false) => mark_undone(&tx, id)?,
        (TaskType::Repeat, true) if !t.done => {
            tx.execute("INSERT INTO todo_logs (task_id, day) VALUES (?1, ?2)", params![id, today]).map_err(err)?;
            let count = t.done_before + count_of(&tx, "SELECT count(*) FROM todo_logs WHERE task_id = ?1", &[&id])?;
            advance(&tx, &t, today, count)?;
        }
        (TaskType::Checkin, true) => {
            tx.execute(
                "INSERT INTO todo_logs (task_id, day) SELECT ?1, ?2 WHERE NOT EXISTS (SELECT 1 FROM todo_logs WHERE task_id = ?1 AND day = ?2)",
                params![id, today],
            )
            .map_err(err)?;
            settle(&tx, id, today)?;
        }
        (TaskType::Checkin, false) => {
            tx.execute("DELETE FROM todo_logs WHERE task_id = ?1 AND day = ?2", params![id, today]).map_err(err)?;
            settle(&tx, id, today)?;
        }
        (TaskType::Progress, true) if !t.done => {
            tx.execute("INSERT INTO todo_logs (task_id, day, amount) VALUES (?1, ?2, 1)", params![id, today]).map_err(err)?;
            settle(&tx, id, today)?;
        }
        (TaskType::Repeat | TaskType::Progress, false) if t.done => mark_undone(&tx, id)?,
        _ => {}
    }
    tx.commit().map_err(err)
}

/// Daily sweep: check-ins whose 时间段 ended before `today`, or whose every day is already checked
/// (e.g. checked in before that rule existed), are completed. Returns how many.
pub fn close_ended(conn: &Connection, today: &str) -> Result<usize, String> {
    let mut st = conn
        .prepare("SELECT id FROM todo_tasks WHERE task_type = 'checkin' AND done = 0 AND due_date IS NOT NULL")
        .map_err(err)?;
    let ids: Vec<i64> = st.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let tx = common::Tx::begin(conn)?;
    for id in &ids {
        settle(&tx, *id, today)?;
    }
    let closed = ids.iter().map(|id| task(&tx, *id).map(|t| t.done)).filter(|d| matches!(d, Ok(true))).count();
    tx.commit().map_err(err)?;
    Ok(closed)
}

pub fn set_done(conn: &Connection, id: i64, done: bool) -> Result<(), String> {
    set_done_on(conn, id, done, &today(conn)?)
}

/// Repeat tasks: move on to the next occurrence without counting this one.
pub fn skip_on(conn: &Connection, id: i64, today: &str) -> Result<(), String> {
    let t = task(conn, id)?;
    if t.task_type != TaskType::Repeat || t.done {
        return Err("只有没结束的重复任务能跳过".into());
    }
    let tx = common::Tx::begin(conn)?;
    let count = t.done_before + count_of(&tx, "SELECT count(*) FROM todo_logs WHERE task_id = ?1", &[&id])?;
    advance(&tx, &t, today, count)?;
    tx.commit().map_err(err)
}

/// Deletes the task and all its subtasks (FK cascade).
pub fn delete_task(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM todo_tasks WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(gone("任务")),
        _ => Ok(()),
    }
}

// ---------- logs (repeat occurrences, check-ins, progress amounts) ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Log {
    pub id: i64,
    pub task_id: i64,
    pub day: String,
    pub amount: i64,
    pub created_at: String,
}

/// Newest first.
pub fn logs(conn: &Connection, task_id: i64) -> Result<Vec<Log>, String> {
    task(conn, task_id)?;
    let mut st = conn
        .prepare("SELECT id, task_id, day, amount, created_at FROM todo_logs WHERE task_id = ?1 ORDER BY day DESC, id DESC")
        .map_err(err)?;
    let rows = st
        .query_map([task_id], |r| Ok(Log { id: r.get(0)?, task_id: r.get(1)?, day: r.get(2)?, amount: r.get(3)?, created_at: r.get(4)? }))
        .map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

/// Progress: count `amount` on `day`. Check-in: check in (or back-fill) `day` — a scheduled day
/// between the start and today; checking a checked day again un-checks it.
pub fn log_on(conn: &Connection, task_id: i64, day: &str, amount: i64, today: &str) -> Result<(), String> {
    let t = task(conn, task_id)?;
    common::check_date(&Some(day.to_string()), "日期")?;
    if day > today {
        return Err("不能记未来的日子".into());
    }
    let tx = common::Tx::begin(conn)?;
    match t.task_type {
        TaskType::Progress => {
            if !(1..=100000).contains(&amount) {
                return Err("数量要在 1～100000 之间".into());
            }
            tx.execute("INSERT INTO todo_logs (task_id, day, amount) VALUES (?1, ?2, ?3)", params![task_id, day, amount])
                .map_err(err)?;
        }
        TaskType::Checkin => {
            let rule = Rule::parse(t.repeat_rule.as_deref().unwrap_or_default())?;
            if !rule.scheduled(day) || t.start_date.as_deref().is_some_and(|s| day < s) || t.due_date.as_deref().is_some_and(|e| day > e) {
                return Err("这天不用打卡".into());
            }
            if tx.execute("DELETE FROM todo_logs WHERE task_id = ?1 AND day = ?2", params![task_id, day]).map_err(err)? == 0 {
                tx.execute("INSERT INTO todo_logs (task_id, day) VALUES (?1, ?2)", params![task_id, day]).map_err(err)?;
            }
        }
        _ => return Err("这个任务不用记数".into()),
    }
    settle(&tx, task_id, today)?;
    tx.commit().map_err(err)
}

pub fn delete_log(conn: &Connection, id: i64) -> Result<(), String> {
    let task_id: i64 = conn
        .query_row("SELECT task_id FROM todo_logs WHERE id = ?1", [id], |r| r.get(0))
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("记录"))?;
    let tx = common::Tx::begin(conn)?;
    tx.execute("DELETE FROM todo_logs WHERE id = ?1", [id]).map_err(err)?;
    settle(&tx, task_id, &today(&tx)?)?;
    tx.commit().map_err(err)
}

// ---------- 今天 / 回看 / 月历 ----------

/// A task with where it lives, for cross-place lists.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedTask {
    #[serde(flatten)]
    pub task: Task,
    pub group_name: String,
    pub goal_name: Option<String>,
    pub parent_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agenda {
    pub today: String,
    pub overdue: Vec<PlacedTask>,
    pub due: Vec<PlacedTask>,
    pub done_today: Vec<PlacedTask>,
}

const PLACE_ORDER: &str = "g.sort_order, g.id, go.sort_order, go.id, t.sort_order, t.id";

fn placed(conn: &Connection, filter: &str, order: &str, args: &[&dyn rusqlite::ToSql], today: &str) -> Result<Vec<PlacedTask>, String> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {TASK_COLS}, g.name, go.name, p.title
             FROM todo_tasks t JOIN todo_groups g ON g.id = t.group_id
               LEFT JOIN todo_goals go ON go.id = t.goal_id LEFT JOIN todo_tasks p ON p.id = t.parent_id
             WHERE {filter} ORDER BY {order}"
        ))
        .map_err(err)?;
    let n = TASK_NCOLS;
    let mut out = st
        .query_map(args, |r| Ok(PlacedTask { task: task_row(r)?, group_name: r.get(n)?, goal_name: r.get(n + 1)?, parent_title: r.get(n + 2)? }))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    for p in &mut out {
        enrich(conn, &mut p.task, today)?;
    }
    Ok(out)
}

/// Dated tasks (normal / repeat / progress): due today or earlier, or today inside [start, due].
/// A repeat task's plan date is its series start, not a day to do it.
fn dated_hit(t: &Task, today: &str) -> bool {
    let le = |d: &Option<String>| d.as_deref().is_some_and(|d| d <= today);
    (t.task_type != TaskType::Repeat && le(&t.plan_date))
        || le(&t.due_date)
        || (le(&t.start_date) && t.due_date.as_deref().is_some_and(|d| today <= d))
}

fn is_overdue(t: &Task, today: &str) -> bool {
    match (&t.due_date, &t.plan_date) {
        (Some(d), _) => d.as_str() < today,
        (None, Some(p)) => p.as_str() < today,
        (None, None) => false,
    }
}

/// Check-ins: today inside their optional [start, due] range.
fn started(t: &Task, today: &str) -> bool {
    t.start_date.as_deref().is_none_or(|s| s <= today) && t.due_date.as_deref().is_none_or(|e| today <= e)
}

pub fn agenda(conn: &Connection, today: &str) -> Result<Agenda, String> {
    common::check_date(&Some(today.to_string()), "日期")?;
    let open = placed(conn, "t.done = 0 AND (go.id IS NULL OR go.status = 'active')", PLACE_ORDER, &[], today)?;
    let (mut overdue, mut due, mut done_today) = (Vec::new(), Vec::new(), Vec::new());
    for p in open {
        let t = &p.task;
        match t.task_type {
            TaskType::Checkin => {
                let rule = Rule::parse(t.repeat_rule.as_deref().unwrap_or_default())?;
                if t.logged_today {
                    done_today.push(p);
                } else if started(t, today) && rule.scheduled(today) {
                    due.push(p);
                }
            }
            _ => {
                if t.logged_today {
                    done_today.push(p.clone()); // an occurrence / amount done today
                }
                if dated_hit(t, today) {
                    if is_overdue(t, today) { overdue.push(p) } else { due.push(p) }
                }
            }
        }
    }
    overdue.sort_by(|a, b| {
        let key = |p: &PlacedTask| p.task.due_date.clone().or(p.task.plan_date.clone());
        key(a).cmp(&key(b))
    });
    let finished = placed(conn, "t.done = 1 AND date(t.done_at, 'localtime') = ?1", "t.done_at", &[&today], today)?;
    for p in finished {
        if !done_today.iter().any(|d: &PlacedTask| d.task.id == p.task.id) {
            done_today.push(p);
        }
    }
    Ok(Agenda { today: today.into(), overdue, due, done_today })
}

/// Unfinished tasks everywhere (any goal status), in place order (小拾).
pub fn open_tasks(conn: &Connection, today: &str) -> Result<Vec<PlacedTask>, String> {
    placed(conn, "t.done = 0", PLACE_ORDER, &[], today)
}

/// Per goal: the last local day something in it got done (a checked task or a log).
pub fn goals_last_done(conn: &Connection) -> Result<HashMap<i64, String>, String> {
    let mut st = conn
        .prepare(
            "SELECT goal_id, max(day) FROM (
               SELECT goal_id, date(done_at, 'localtime') AS day FROM todo_tasks WHERE done = 1 AND goal_id IS NOT NULL AND done_at IS NOT NULL
               UNION ALL SELECT t.goal_id, l.day FROM todo_logs l JOIN todo_tasks t ON t.id = l.task_id WHERE t.goal_id IS NOT NULL)
             GROUP BY goal_id",
        )
        .map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Something finished on a day: a checked task, or one log of a repeat / check-in / progress task.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoneItem {
    pub day: String,
    /// Sort key within the day (UTC timestamp).
    pub at: String,
    /// 第 3 次 / 打卡 / +2 本; None for plain checks.
    pub detail: Option<String>,
    #[serde(flatten)]
    pub task: PlacedTask,
}

/// What got done between two local dates (inclusive), in order.
pub fn done_between(conn: &Connection, from: &str, to: &str) -> Result<Vec<DoneItem>, String> {
    common::check_date(&Some(from.to_string()), "开始日期")?;
    common::check_date(&Some(to.to_string()), "结束日期")?;
    let today = today(conn)?;
    let mut out: Vec<DoneItem> = placed(conn, "t.done = 1 AND date(t.done_at, 'localtime') BETWEEN ?1 AND ?2", "t.done_at", &[&from, &to], &today)?
        .into_iter()
        .map(|p| {
            let at = p.task.done_at.clone().unwrap_or_default();
            let day = conn.query_row("SELECT date(?1, 'localtime')", [&at], |r| r.get(0)).unwrap_or_default();
            DoneItem { day, at, detail: (p.task.task_type != TaskType::Normal).then(|| "全部完成".into()), task: p }
        })
        .collect();
    let mut st = conn
        .prepare(
            "SELECT l.task_id, l.day, l.created_at, l.amount,
               (SELECT count(*) FROM todo_logs x WHERE x.task_id = l.task_id AND (x.day < l.day OR (x.day = l.day AND x.id <= l.id)))
             FROM todo_logs l WHERE l.day BETWEEN ?1 AND ?2 ORDER BY l.id",
        )
        .map_err(err)?;
    let logs: Vec<(i64, String, String, i64, i64)> = st
        .query_map([from, to], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    for (task_id, day, at, amount, nth) in logs {
        let Some(p) = placed(conn, "t.id = ?1", "t.id", &[&task_id], &today)?.pop() else { continue };
        let detail = match p.task.task_type {
            TaskType::Repeat => format!("第 {} 次", p.task.done_before + nth),
            TaskType::Checkin => "打卡".into(),
            _ => format!("+{amount}{}", p.task.unit.as_deref().map(|u| format!(" {u}")).unwrap_or_default()),
        };
        out.push(DoneItem { day, at, detail: Some(detail), task: p });
    }
    out.sort_by(|a, b| (&a.day, &a.at).cmp(&(&b.day, &b.at)));
    Ok(out)
}

/// One entry in the month grid.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalItem {
    pub day: String,
    /// due | range | next (a later occurrence of a repeat task) | done
    pub what: String,
    pub detail: Option<String>,
    #[serde(flatten)]
    pub task: PlacedTask,
}

/// Dated work between two local dates: open tasks by their dates (time ranges on every day, repeat
/// tasks with their upcoming occurrences), plus what got done. Check-ins are left out (every day).
pub fn month(conn: &Connection, from: &str, to: &str) -> Result<Vec<CalItem>, String> {
    let done = done_between(conn, from, to)?;
    let today = today(conn)?;
    let open = placed(conn, "t.done = 0 AND t.task_type != 'checkin' AND (go.id IS NULL OR go.status = 'active')", PLACE_ORDER, &[], &today)?;
    let inside = |d: &str| from <= d && d <= to;
    let mut out = Vec::new();
    let mut push = |day: String, what: &str, detail: Option<String>, p: &PlacedTask| {
        out.push(CalItem { day, what: what.into(), detail, task: p.clone() })
    };
    for p in &open {
        let t = &p.task;
        match (&t.start_date, &t.due_date) {
            (Some(s), Some(d)) => {
                let (a, b) = (common::day_number(s.max(&from.to_string())), common::day_number(d.min(&to.to_string())));
                for n in a..=b.min(a + 62) {
                    push(common::from_day_number(n), "range", None, p);
                }
            }
            (None, Some(d)) if inside(d) => push(d.clone(), "due", None, p),
            _ => {}
        }
        if t.task_type == TaskType::Repeat {
            let Ok(rule) = Rule::parse(t.repeat_rule.as_deref().unwrap_or_default()) else { continue };
            let Some(mut anchor) = t.due_date.clone() else { continue };
            for k in 1..=62 {
                let next = rule.next(&anchor, &anchor);
                let n = t.count + k + 1; // occurrence number of `next`
                if next.as_str() > to || t.repeat_total.is_some_and(|tot| n > tot) || t.repeat_until.as_deref().is_some_and(|u| next.as_str() > u) {
                    break;
                }
                if inside(&next) {
                    push(next.clone(), "next", Some(format!("第 {n} 次")), p);
                }
                anchor = next;
            }
        }
    }
    for d in done {
        let DoneItem { day, detail, task, .. } = d;
        out.push(CalItem { day, what: "done".into(), detail, task });
    }
    Ok(out)
}

pub fn today(conn: &Connection) -> Result<String, String> {
    conn.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0)).map_err(err)
}

// ---------- order (drag and drop) ----------

pub fn reorder_groups(conn: &Connection, ids: &[i64]) -> Result<(), String> {
    common::reorder(conn, "todo_groups", "1 = 1", &[], ids)
}

pub fn reorder_goals(conn: &Connection, group_id: i64, ids: &[i64]) -> Result<(), String> {
    common::reorder(conn, "todo_goals", "group_id = ?1", &[&group_id], ids)
}

/// `ids` = all siblings of one parent (or one place's top level), in the new order.
pub fn reorder_tasks(conn: &Connection, ids: &[i64]) -> Result<(), String> {
    let first = task(conn, *ids.first().ok_or("列表是空的")?)?;
    common::reorder(
        conn,
        "todo_tasks",
        "group_id = ?1 AND goal_id IS ?2 AND parent_id IS ?3",
        &[&first.group_id, &first.goal_id, &first.parent_id],
        ids,
    )
}

// ---------- import / export: { groups: [{ name, goals: [{…, tasks }], tasks }] }, subtasks nested ----------

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskRecord {
    pub title: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub plan_date: Option<String>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_at: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TaskRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "is_normal")]
    pub task_type: TaskType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_total: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<Period>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub done_before: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs: Vec<LogRecord>,
}

fn is_normal(t: &TaskType) -> bool {
    *t == TaskType::Normal
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

fn one() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogRecord {
    pub day: String,
    #[serde(default = "one")]
    pub amount: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoalRecord {
    pub name: String,
    #[serde(default)]
    pub status: GoalStatus,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub review: Option<String>,
    #[serde(default)]
    pub tasks: Vec<TaskRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupRecord {
    pub name: String,
    #[serde(default)]
    pub goals: Vec<GoalRecord>,
    #[serde(default)]
    pub tasks: Vec<TaskRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TodoRecord {
    #[serde(default)]
    pub groups: Vec<GroupRecord>,
}

fn walk<'a>(tasks: &'a [TaskRecord], out: &mut Vec<&'a TaskRecord>) {
    for t in tasks {
        out.push(t);
        walk(&t.children, out);
    }
}

impl TodoRecord {
    fn all_tasks(&self) -> Vec<&TaskRecord> {
        let mut out = Vec::new();
        for g in &self.groups {
            walk(&g.tasks, &mut out);
            for goal in &g.goals {
                walk(&goal.tasks, &mut out);
            }
        }
        out
    }

    /// Records of any kind (groups + goals + tasks), for import summaries.
    pub fn len(&self) -> usize {
        self.groups.len() + self.groups.iter().map(|g| g.goals.len()).sum::<usize>() + self.all_tasks().len()
    }
}

impl TaskRecord {
    fn fields(&self) -> Result<TaskFields, String> {
        let (mut plan_date, mut start_date, mut due_date) = (self.plan_date.clone(), self.start_date.clone(), self.due_date.clone());
        if self.task_type == TaskType::Repeat && due_date.is_none() {
            due_date = plan_date.take(); // format ≤ 3: the occurrence was the plan date
        }
        if self.task_type == TaskType::Checkin && due_date.is_none() && self.repeat_until.is_some() {
            // format ≤ 4: start / end were plan_date / repeat_until
            (start_date, due_date) = (plan_date.take(), self.repeat_until.clone());
        }
        let f = TaskFields {
            title: self.title.clone(),
            note: self.note.clone(),
            link: self.link.clone(),
            plan_date,
            start_date,
            due_date,
            task_type: self.task_type,
            repeat_rule: self.repeat_rule.clone(),
            repeat_total: self.repeat_total,
            repeat_until: self.repeat_until.clone(),
            target: self.target,
            unit: self.unit.clone(),
            period: self.period,
            done_before: self.done_before,
        }
        .validated(!self.children.is_empty())?;
        if f.task_type == TaskType::Repeat && f.due_date.is_none() {
            return Err("重复任务没有日期".into());
        }
        for l in &self.logs {
            common::check_date(&Some(l.day.clone()), "记录日期")?;
            if l.amount < 1 {
                return Err("记录的数量要大于 0".into());
            }
        }
        Ok(f)
    }
}

pub fn validate_records(rec: &TodoRecord) -> Result<(), String> {
    let mut names = std::collections::HashSet::new();
    for (i, g) in rec.groups.iter().enumerate() {
        let name = common::require(&g.name, "分组名", 30).map_err(|e| format!("{LABEL}第 {} 个分组：{e}", i + 1))?;
        if !names.insert(name.clone()) {
            return Err(format!("{LABEL}：分组「{name}」重复了"));
        }
        for goal in &g.goals {
            validate_goal(GoalFields {
                group_id: 0,
                name: goal.name.clone(),
                status: goal.status.current(),
                start_date: goal.start_date.clone(),
                end_date: goal.end_date.clone(),
                note: goal.note.clone(),
                review: goal.review.clone(),
            })
            .map_err(|e| format!("{LABEL}「{name}」的目标「{}」：{e}", goal.name.trim()))?;
        }
        let mut all = Vec::new();
        walk(&g.tasks, &mut all);
        for goal in &g.goals {
            walk(&goal.tasks, &mut all);
        }
        for t in all {
            t.fields().map_err(|e| format!("{LABEL}「{name}」的任务「{}」：{e}", t.title.trim()))?;
        }
    }
    Ok(())
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        "SELECT (SELECT count(*) FROM todo_groups) + (SELECT count(*) FROM todo_goals) + (SELECT count(*) FROM todo_tasks)",
        [],
        |r| r.get(0),
    )
    .map_err(err)
}

fn insert_tasks(conn: &Connection, place: Place, tasks: &[TaskRecord]) -> Result<(), String> {
    for (i, t) in tasks.iter().enumerate() {
        let f = t.fields()?;
        conn.execute(
            &format!(
                "INSERT INTO todo_tasks (group_id, goal_id, parent_id, title, note, link, plan_date, start_date, due_date,
                   done, done_at, collapsed, sort_order, created_at, updated_at,
                   task_type, repeat_rule, repeat_total, repeat_until, target, unit, period, done_before)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, coalesce(?14, {NOW}), coalesce(?15, ?14, {NOW}),
                   ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)"
            ),
            params![
                place.group_id, place.goal_id, place.parent_id, f.title, f.note, f.link, f.plan_date, f.start_date, f.due_date,
                t.done, t.done_at, t.collapsed, i as i64 + 1, t.created_at, t.updated_at, f.task_type, f.repeat_rule,
                f.repeat_total, f.repeat_until, f.target, f.unit, f.period, f.done_before
            ],
        )
        .map_err(err)?;
        let id = conn.last_insert_rowid();
        for l in &t.logs {
            conn.execute(
                &format!("INSERT INTO todo_logs (task_id, day, amount, created_at) VALUES (?1, ?2, ?3, coalesce(?4, {NOW}))"),
                params![id, l.day, l.amount, l.created_at],
            )
            .map_err(err)?;
        }
        let child = Place { parent_id: Some(id), ..place };
        insert_tasks(conn, child, &t.children)?;
    }
    Ok(())
}

/// Replaces all groups, goals and tasks (as-is: done flags are not re-derived). Caller owns the transaction.
pub fn replace_all(conn: &Connection, rec: &TodoRecord) -> Result<usize, String> {
    validate_records(rec)?;
    conn.execute("DELETE FROM todo_groups", []).map_err(err)?; // goals + tasks cascade
    for (i, g) in rec.groups.iter().enumerate() {
        conn.execute(
            &format!(
                "INSERT INTO todo_groups (name, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, coalesce(?3, {NOW}), coalesce(?4, ?3, {NOW}))"
            ),
            params![g.name.trim(), i as i64 + 1, g.created_at, g.updated_at],
        )
        .map_err(err)?;
        let group_id = conn.last_insert_rowid();
        for (j, goal) in g.goals.iter().enumerate() {
            conn.execute(
                &format!(
                    "INSERT INTO todo_goals (group_id, name, status, start_date, end_date, note, review, sort_order, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, coalesce(?9, {NOW}), coalesce(?10, ?9, {NOW}))"
                ),
                params![
                    group_id, goal.name.trim(), goal.status.current(), common::clean(goal.start_date.clone()), common::clean(goal.end_date.clone()),
                    common::clean(goal.note.clone()), common::clean(goal.review.clone()), j as i64 + 1, goal.created_at, goal.updated_at
                ],
            )
            .map_err(err)?;
            let place = Place { group_id, goal_id: Some(conn.last_insert_rowid()), parent_id: None };
            insert_tasks(conn, place, &goal.tasks)?;
        }
        insert_tasks(conn, Place { group_id, goal_id: None, parent_id: None }, &g.tasks)?;
    }
    Ok(rec.len())
}

fn task_tree(conn: &Connection, all: &[Task]) -> Result<Vec<TaskRecord>, String> {
    let mut by_parent: HashMap<Option<i64>, Vec<&Task>> = HashMap::new();
    let mut logs: HashMap<i64, Vec<LogRecord>> = HashMap::new();
    for t in all {
        by_parent.entry(t.parent_id).or_default().push(t);
        if t.task_type != TaskType::Normal {
            let mut st = conn
                .prepare("SELECT day, amount, created_at FROM todo_logs WHERE task_id = ?1 ORDER BY day, id")
                .map_err(err)?;
            let ls = st
                .query_map([t.id], |r| Ok(LogRecord { day: r.get(0)?, amount: r.get(1)?, created_at: r.get(2)? }))
                .map_err(err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(err)?;
            logs.insert(t.id, ls);
        }
    }
    fn build(parent: Option<i64>, by_parent: &HashMap<Option<i64>, Vec<&Task>>, logs: &HashMap<i64, Vec<LogRecord>>) -> Vec<TaskRecord> {
        by_parent
            .get(&parent)
            .map(|ts| {
                ts.iter()
                    .map(|t| TaskRecord {
                        title: t.title.clone(),
                        note: t.note.clone(),
                        link: t.link.clone(),
                        plan_date: t.plan_date.clone(),
                        start_date: t.start_date.clone(),
                        due_date: t.due_date.clone(),
                        done: t.done,
                        done_at: t.done_at.clone(),
                        collapsed: t.collapsed,
                        children: build(Some(t.id), by_parent, logs),
                        created_at: Some(t.created_at.clone()),
                        updated_at: Some(t.updated_at.clone()),
                        task_type: t.task_type,
                        repeat_rule: t.repeat_rule.clone(),
                        repeat_total: t.repeat_total,
                        repeat_until: t.repeat_until.clone(),
                        target: t.target,
                        unit: t.unit.clone(),
                        period: t.period,
                        done_before: t.done_before,
                        logs: logs.get(&t.id).cloned().unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    Ok(build(None, &by_parent, &logs))
}

pub fn export(conn: &Connection) -> Result<TodoRecord, String> {
    let all_goals = goals(conn)?;
    let groups = groups(conn)?
        .into_iter()
        .map(|g| {
            let goals = all_goals
                .iter()
                .filter(|x| x.group_id == g.id)
                .map(|x| {
                    Ok(GoalRecord {
                        name: x.name.clone(),
                        status: x.status,
                        start_date: x.start_date.clone(),
                        end_date: x.end_date.clone(),
                        note: x.note.clone(),
                        review: x.review.clone(),
                        tasks: task_tree(conn, &tasks(conn, None, Some(x.id))?)?,
                        created_at: Some(x.created_at.clone()),
                        updated_at: Some(x.updated_at.clone()),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(GroupRecord {
                tasks: task_tree(conn, &tasks(conn, Some(g.id), None)?)?,
                name: g.name,
                goals,
                created_at: Some(g.created_at),
                updated_at: Some(g.updated_at),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(TodoRecord { groups })
}

// ---------- commands ----------

#[tauri::command]
pub fn todo_groups(state: State<AppState>) -> Result<Vec<Group>, String> {
    state.with_db(|c| groups(c))
}
#[tauri::command]
pub fn todo_group_create(state: State<AppState>, name: String) -> Result<Group, String> {
    state.with_undo("添加分组", |c| create_group(c, &name))
}
#[tauri::command]
pub fn todo_group_rename(state: State<AppState>, id: i64, name: String) -> Result<Group, String> {
    state.with_undo("重命名分组", |c| rename_group(c, id, &name))
}
#[tauri::command]
pub fn todo_group_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除分组", |c| delete_group(c, id))
}
#[tauri::command]
pub fn todo_goals(state: State<AppState>) -> Result<Vec<Goal>, String> {
    state.with_db(|c| goals(c))
}
#[tauri::command]
pub fn todo_goal_create(state: State<AppState>, patch: GoalPatch) -> Result<Goal, String> {
    state.with_undo("添加目标", |c| create_goal(c, patch))
}
#[tauri::command]
pub fn todo_goal_update(state: State<AppState>, id: i64, patch: GoalPatch) -> Result<Goal, String> {
    state.with_undo("修改目标", |c| update_goal(c, id, patch))
}
#[tauri::command]
pub fn todo_goal_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除目标", |c| delete_goal(c, id))
}
#[tauri::command]
pub fn todo_tasks(state: State<AppState>, group_id: Option<i64>, goal_id: Option<i64>) -> Result<Vec<Task>, String> {
    state.with_db(|c| tasks(c, group_id, goal_id))
}
#[tauri::command]
pub fn todo_task_create(state: State<AppState>, patch: TaskPatch) -> Result<Task, String> {
    state.with_undo("添加任务", |c| create_task(c, patch))
}
#[tauri::command]
pub fn todo_task_update(state: State<AppState>, id: i64, patch: TaskPatch) -> Result<Task, String> {
    state.with_undo("修改任务", |c| update_task(c, id, patch))
}
#[tauri::command]
pub fn todo_task_set_done(state: State<AppState>, id: i64, done: bool) -> Result<(), String> {
    state.with_undo(if done { "完成任务" } else { "取消完成" }, |c| set_done(c, id, done))
}
#[tauri::command]
pub fn todo_task_skip(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("跳过一次", |c| skip_on(c, id, &today(c)?))
}
#[tauri::command]
pub fn todo_logs(state: State<AppState>, task_id: i64) -> Result<Vec<Log>, String> {
    state.with_db(|c| logs(c, task_id))
}
/// `day` defaults to today.
#[tauri::command]
pub fn todo_log(state: State<AppState>, task_id: i64, day: Option<String>, amount: Option<i64>) -> Result<(), String> {
    state.with_undo("记录", |c| {
        let t = today(c)?;
        log_on(c, task_id, day.as_deref().unwrap_or(&t), amount.unwrap_or(1), &t)
    })
}
#[tauri::command]
pub fn todo_log_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除记录", |c| delete_log(c, id))
}
#[tauri::command]
pub fn todo_month(state: State<AppState>, from: String, to: String) -> Result<Vec<CalItem>, String> {
    state.with_db(|c| month(c, &from, &to))
}
#[tauri::command]
pub fn todo_groups_reorder(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| reorder_groups(c, &ids))
}
#[tauri::command]
pub fn todo_goals_reorder(state: State<AppState>, group_id: i64, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| reorder_goals(c, group_id, &ids))
}
#[tauri::command]
pub fn todo_tasks_reorder(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| reorder_tasks(c, &ids))
}
#[tauri::command]
pub fn todo_task_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除任务", |c| delete_task(c, id))
}
/// `today` defaults to the Mac's local date.
#[tauri::command]
pub fn todo_agenda(state: State<AppState>, today: Option<String>) -> Result<Agenda, String> {
    state.with_db(|c| {
        let d = match today {
            Some(d) => d,
            None => self::today(c)?,
        };
        agenda(c, &d)
    })
}
#[tauri::command]
pub fn todo_done_between(state: State<AppState>, from: String, to: String) -> Result<Vec<DoneItem>, String> {
    state.with_db(|c| done_between(c, &from, &to))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    fn tp(v: serde_json::Value) -> TaskPatch {
        serde_json::from_value(v).unwrap()
    }
    fn gp(v: serde_json::Value) -> GoalPatch {
        serde_json::from_value(v).unwrap()
    }
    fn done_ids(c: &Connection) -> Vec<i64> {
        let mut st = c.prepare("SELECT id FROM todo_tasks WHERE done = 1 ORDER BY id").unwrap();
        st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    }
    fn titles(ts: &[PlacedTask]) -> Vec<&str> {
        ts.iter().map(|p| p.task.title.as_str()).collect()
    }

    #[test]
    fn groups_unique_and_cascade() {
        let c = memory_db();
        let a = create_group(&c, " 🖥️ Diploma ").unwrap();
        assert_eq!((a.name.as_str(), a.sort_order, a.open), ("🖥️ Diploma", 1, 0));
        assert!(create_group(&c, "🖥️ Diploma").unwrap_err().contains("已经有"));
        assert!(create_group(&c, " ").is_err());
        let b = create_group(&c, "📌 Pick Up").unwrap();
        assert_eq!(b.sort_order, 2);
        assert_eq!(rename_group(&c, b.id, "📌 Later").unwrap().name, "📌 Later");
        let g = create_goal(&c, gp(json!({"groupId": a.id, "name": "毕业"}))).unwrap();
        create_task(&c, tp(json!({"goalId": g.id, "title": "x"}))).unwrap();
        create_task(&c, tp(json!({"groupId": a.id, "title": "y"}))).unwrap();
        assert_eq!(group(&c, a.id).unwrap().open, 2);
        delete_group(&c, a.id).unwrap();
        assert_eq!(count(&c).unwrap(), 1); // only 📌 Later left
        assert!(delete_group(&c, a.id).is_err());
    }

    #[test]
    fn goals_progress_counts_leaves_and_move_group() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        let b = create_group(&c, "B").unwrap().id;
        assert!(create_goal(&c, gp(json!({"name": "x"}))).is_err());
        assert!(create_goal(&c, gp(json!({"groupId": a, "name": "x", "startDate": "2026-10-02", "endDate": "2026-10-01"}))).is_err());
        let g = create_goal(&c, gp(json!({"groupId": a, "name": "Personal Website", "startDate": "2026-10-01"}))).unwrap();
        assert_eq!((g.status, g.total, g.sort_order), (GoalStatus::Active, 0, 1));
        let t1 = create_task(&c, tp(json!({"goalId": g.id, "title": "选框架"}))).unwrap();
        let t2 = create_task(&c, tp(json!({"goalId": g.id, "title": "写页面"}))).unwrap();
        create_task(&c, tp(json!({"parentId": t2.id, "title": "首页"}))).unwrap();
        create_task(&c, tp(json!({"parentId": t2.id, "title": "关于"}))).unwrap();
        set_done(&c, t1.id, true).unwrap();
        let g = goal(&c, g.id).unwrap();
        assert_eq!((g.total, g.done), (3, 1)); // 写页面 has children → not a leaf

        let g = update_goal(&c, g.id, gp(json!({"groupId": b, "status": "paused", "review": " 慢慢来 "}))).unwrap();
        assert_eq!((g.group_id, g.status, g.review.as_deref()), (b, GoalStatus::Paused, Some("慢慢来")));
        assert!(tasks(&c, None, Some(g.id)).unwrap().iter().all(|t| t.group_id == b));
        delete_goal(&c, g.id).unwrap();
        assert_eq!(count(&c).unwrap(), 2); // groups only
    }

    #[test]
    fn tasks_place_order_and_validate() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        let g = create_goal(&c, gp(json!({"groupId": a, "name": "G"}))).unwrap().id;
        let r1 = create_task(&c, tp(json!({"groupId": a, "title": " 嘉信入金 "}))).unwrap();
        let r2 = create_task(&c, tp(json!({"groupId": a, "title": "b"}))).unwrap();
        let s = create_task(&c, tp(json!({"parentId": r1.id, "groupId": 999, "title": "sub"}))).unwrap();
        let in_goal = create_task(&c, tp(json!({"goalId": g, "title": "g1"}))).unwrap();
        assert_eq!((r1.title.as_str(), r1.sort_order, r2.sort_order), ("嘉信入金", 1, 2));
        assert_eq!((s.group_id, s.goal_id, s.parent_id, s.sort_order), (a, None, Some(r1.id), 1));
        assert_eq!((in_goal.group_id, in_goal.goal_id, in_goal.sort_order), (a, Some(g), 1));
        assert_eq!(tasks(&c, Some(a), None).unwrap().len(), 3); // loose only
        assert_eq!(tasks(&c, None, Some(g)).unwrap().len(), 1);

        for bad in [
            json!({"groupId": a}),
            json!({"title": "no place"}),
            json!({"groupId": a, "title": "x", "startDate": "2026-01-22"}),
            json!({"groupId": a, "title": "x", "startDate": "2026-01-25", "dueDate": "2026-01-22"}),
            json!({"groupId": a, "title": "x", "planDate": "2026-02-30"}),
        ] {
            assert!(create_task(&c, tp(bad.clone())).is_err(), "{bad}");
        }
        let u = update_task(&c, r2.id, tp(json!({"startDate": "2027-01-22", "dueDate": "2027-01-25", "link": " https://canvas ", "collapsed": true}))).unwrap();
        assert_eq!((u.start_date.as_deref(), u.link.as_deref(), u.collapsed), (Some("2027-01-22"), Some("https://canvas"), true));
        assert!(update_task(&c, s.id, tp(json!({"goalId": g}))).unwrap_err().contains("子任务"));
        assert!(update_task(&c, r1.id, tp(json!({"parentId": r2.id}))).is_err());
    }

    #[test]
    fn moving_a_root_task_moves_its_subtree() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        let b = create_group(&c, "B").unwrap().id;
        let g = create_goal(&c, gp(json!({"groupId": b, "name": "G"}))).unwrap().id;
        create_task(&c, tp(json!({"goalId": g, "title": "already there"}))).unwrap();
        let root = create_task(&c, tp(json!({"groupId": a, "title": "root"}))).unwrap();
        let kid = create_task(&c, tp(json!({"parentId": root.id, "title": "kid"}))).unwrap();
        let grand = create_task(&c, tp(json!({"parentId": kid.id, "title": "grand"}))).unwrap();
        let moved = update_task(&c, root.id, tp(json!({"goalId": g}))).unwrap();
        assert_eq!((moved.group_id, moved.goal_id, moved.sort_order), (b, Some(g), 2));
        assert_eq!((task(&c, grand.id).unwrap().group_id, task(&c, grand.id).unwrap().goal_id), (b, Some(g)));
        let back = update_task(&c, root.id, tp(json!({"goalId": null, "groupId": a}))).unwrap();
        assert_eq!((back.group_id, back.goal_id), (a, None));
        assert_eq!(task(&c, kid.id).unwrap().goal_id, None);
    }

    #[test]
    fn completion_cascades_down_and_up() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        let p = create_task(&c, tp(json!({"groupId": a, "title": "答辩前提交"}))).unwrap().id;
        let k1 = create_task(&c, tp(json!({"parentId": p, "title": "IA Report"}))).unwrap().id;
        let k2 = create_task(&c, tp(json!({"parentId": p, "title": "Poster"}))).unwrap().id;
        let g1 = create_task(&c, tp(json!({"parentId": k2, "title": "draft"}))).unwrap().id;

        set_done(&c, k1, true).unwrap();
        assert_eq!(done_ids(&c), [k1]);
        set_done(&c, g1, true).unwrap(); // last leaf → k2 done → all children of p done → p done
        assert_eq!(done_ids(&c), [p, k1, k2, g1]);
        assert!(task(&c, p).unwrap().done_at.is_some());

        set_done(&c, g1, false).unwrap(); // reopens k2 and p, keeps k1
        assert_eq!(done_ids(&c), [k1]);
        set_done(&c, p, true).unwrap(); // whole subtree
        assert_eq!(done_ids(&c), [p, k1, k2, g1]);
        create_task(&c, tp(json!({"parentId": k2, "title": "new step"}))).unwrap(); // reopens k2 and p
        assert_eq!(done_ids(&c), [k1, g1]);
        delete_task(&c, p).unwrap();
        assert_eq!(count(&c).unwrap(), 1);
        assert!(set_done(&c, p, true).is_err());
    }

    #[test]
    fn agenda_today_overdue_and_paused_goals() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        let paused = create_goal(&c, gp(json!({"groupId": a, "name": "P", "status": "paused"}))).unwrap().id;
        let active = create_goal(&c, gp(json!({"groupId": a, "name": "Act"}))).unwrap().id;
        let mk = |title: &str, extra: serde_json::Value| {
            let mut v = json!({"groupId": a, "title": title});
            v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            create_task(&c, tp(v)).unwrap()
        };
        let t = "2026-09-30";
        mk("plan today", json!({"planDate": t}));
        mk("due today", json!({"dueDate": t}));
        mk("in window", json!({"startDate": "2026-09-28", "dueDate": "2026-10-02"}));
        mk("planned earlier, due later", json!({"planDate": "2026-09-25", "dueDate": "2026-10-10"}));
        mk("late due", json!({"dueDate": "2026-09-28"}));
        mk("late plan", json!({"planDate": "2026-09-01"}));
        mk("future", json!({"planDate": "2026-10-01", "dueDate": "2026-10-05"}));
        mk("window not yet", json!({"startDate": "2026-10-01", "dueDate": "2026-10-02"}));
        mk("no dates", json!({}));
        create_task(&c, tp(json!({"goalId": paused, "title": "paused goal", "planDate": t}))).unwrap();
        let in_goal = create_task(&c, tp(json!({"goalId": active, "title": "active goal", "dueDate": t}))).unwrap();
        let sub = create_task(&c, tp(json!({"parentId": in_goal.id, "title": "sub", "planDate": t}))).unwrap();
        let finished = mk("finished", json!({"planDate": t}));
        set_done(&c, finished.id, true).unwrap();

        let ag = agenda(&c, t).unwrap();
        assert_eq!(titles(&ag.overdue), ["late plan", "late due"]); // earliest date first
        assert_eq!(titles(&ag.due), ["plan today", "due today", "in window", "planned earlier, due later", "active goal", "sub"]);
        let s = ag.due.iter().find(|p| p.task.id == sub.id).unwrap();
        assert_eq!((s.group_name.as_str(), s.goal_name.as_deref(), s.parent_title.as_deref()), ("A", Some("Act"), Some("active goal")));
        let today = today(&c).unwrap(); // done_at is "now", so done_today uses the real date
        assert_eq!(titles(&agenda(&c, &today).unwrap().done_today), ["finished"]);
        assert_eq!(done_between(&c, &today, &today).unwrap().iter().map(|d| d.task.task.title.as_str()).collect::<Vec<_>>(), ["finished"]);
        assert!(done_between(&c, "2020-01-01", "2020-01-07").unwrap().is_empty());
        assert!(agenda(&c, "2026-9-30").is_err());
    }

    #[test]
    fn records_roundtrip_tree() {
        let c = memory_db();
        let rec: TodoRecord = serde_json::from_value(json!({"groups": [
            {"name": "🖥️ Diploma", "tasks": [
                {"title": "交 Project Plan", "done": true},
                {"title": "答辩前提交", "collapsed": true, "children": [{"title": "IA Report"}, {"title": "Poster", "children": [{"title": "draft", "dueDate": "2027-01-20"}]}]},
                {"title": "提交窗口", "startDate": "2027-01-22", "dueDate": "2027-01-25"}
            ]},
            {"name": "📌 Pick Up", "goals": [{"name": "Personal Website", "status": "paused", "review": "r", "tasks": [{"title": "选框架"}]}], "tasks": [{"title": "嘉信入金", "link": "https://x"},
                {"title": "周报", "taskType": "repeat", "repeatRule": "weekly:6", "dueDate": "2026-10-03", "repeatTotal": 20, "doneBefore": 6, "logs": [{"day": "2026-09-26"}]},
                {"title": "读书", "taskType": "progress", "target": 20, "unit": "本", "period": "all", "logs": [{"day": "2026-09-01", "amount": 2}]}]}
        ]}))
        .unwrap();
        assert_eq!(rec.len(), 2 + 1 + 10);
        assert_eq!(replace_all(&c, &rec).unwrap(), 13);
        let out = export(&c).unwrap();
        let a = serde_json::to_value(&out).unwrap();
        assert!(a["groups"][0]["createdAt"].is_string());
        // second trip: import the export into a fresh DB → identical export
        let c2 = memory_db();
        replace_all(&c2, &out).unwrap();
        assert_eq!(a, serde_json::to_value(export(&c2).unwrap()).unwrap());
        let d = &out.groups[0];
        assert_eq!(d.tasks[1].children[1].children[0].due_date.as_deref(), Some("2027-01-20"));
        assert!(d.tasks[0].done && d.tasks[0].done_at.is_none() && d.tasks[1].collapsed);
        assert_eq!(out.groups[1].goals[0].status, GoalStatus::Paused);
        assert_eq!(out.groups[1].tasks[1].done_before, 6);
        let reading = &out.groups[1].tasks[2];
        assert_eq!((reading.task_type, reading.target, reading.logs.len(), reading.logs[0].amount), (TaskType::Progress, Some(20), 1, 2));

        // older files: a repeat occurrence in planDate, plan dates on normal tasks
        let old: TodoRecord = serde_json::from_value(json!({"groups": [{"name": "X", "tasks": [
            {"title": "周报", "taskType": "repeat", "repeatRule": "weekly:6", "planDate": "2026-10-03"},
            {"title": "p", "planDate": "2026-10-01", "dueDate": "2026-10-05"}]}]}))
        .unwrap();
        let c3 = memory_db();
        replace_all(&c3, &old).unwrap();
        let ts = &export(&c3).unwrap().groups[0].tasks;
        assert_eq!((ts[0].plan_date.as_deref(), ts[0].due_date.as_deref()), (None, Some("2026-10-03")));
        assert_eq!((ts[1].plan_date.as_deref(), ts[1].start_date.as_deref(), ts[1].due_date.as_deref()), (None, Some("2026-10-01"), Some("2026-10-05")));
        let dateless: TodoRecord = serde_json::from_value(json!({"groups": [{"name": "X", "tasks": [{"title": "r", "taskType": "repeat", "repeatRule": "daily"}]}]})).unwrap();
        assert!(validate_records(&dateless).is_err());

        let dup: TodoRecord = serde_json::from_value(json!({"groups": [{"name": "X"}, {"name": " X "}]})).unwrap();
        assert!(validate_records(&dup).unwrap_err().contains("重复"));
        let bad: TodoRecord = serde_json::from_value(json!({"groups": [{"name": "X", "tasks": [{"title": "ok", "children": [{"title": " "}]}]}]})).unwrap();
        assert!(validate_records(&bad).is_err());
    }

    #[test]
    fn rules_parse_and_next() {
        let next = |r: &str, anchor: &str, done: &str| Rule::parse(r).unwrap().next(anchor, done);
        assert_eq!(next("weekly:6", "2026-09-30", "2026-09-30"), "2026-10-03");
        assert_eq!(next("weekly:3,4", "2026-09-30", ""), "2026-10-01");
        assert_eq!(next("monthly:31", "2026-09-30", ""), "2026-10-31");
        assert_eq!(next("monthly:31", "2026-01-31", ""), "2026-02-28");
        assert_eq!(next("monthly:15", "2026-09-10", ""), "2026-09-15");
        assert_eq!(next("monthly:30,1,15", "2026-02-15", ""), "2026-02-28"); // 30 → month end
        assert_eq!(next("monthly:1,15,30", "2026-02-28", ""), "2026-03-01");
        let first = |r: &str, d: &str| Rule::parse(r).unwrap().first_from(d);
        assert_eq!((first("weekly:3", "2026-09-30"), first("weekly:6", "2026-09-30")), ("2026-09-30".into(), "2026-10-03".into()));
        assert_eq!((first("monthly:30", "2026-09-30"), first("every:2w", "2026-09-30")), ("2026-09-30".into(), "2026-09-30".into()));
        assert_eq!(next("every:2w", "2026-09-30", ""), "2026-10-14");
        assert_eq!(next("every:3d", "2026-09-30", ""), "2026-10-03");
        assert_eq!(next("after:30d", "2026-09-01", "2026-09-30"), "2026-10-30");
        assert_eq!(next("daily", "2026-12-31", ""), "2027-01-01");
        // 每年: the next MM-DD; every n years; 02-29 outside leap years → 02-28
        assert_eq!(next("yearly:12-31", "2026-12-31", ""), "2027-12-31");
        assert_eq!(next("yearly:12-31", "2027-01-05", "2027-01-05"), "2027-12-31"); // done late: this year's still counts
        assert_eq!(next("yearly:03-01/3", "2026-03-01", ""), "2029-03-01");
        assert_eq!(next("yearly:02-29", "2024-02-29", ""), "2025-02-28");
        assert_eq!(next("yearly:02-29", "2027-03-01", ""), "2028-02-29");
        assert_eq!((first("yearly:10-06", "2026-10-06"), first("yearly:10-05/3", "2026-10-06")), ("2026-10-06".into(), "2027-10-05".into()));
        // 完成后 in weeks / months / years (month ends kept)
        assert_eq!(next("after:2w", "", "2026-10-06"), "2026-10-20");
        assert_eq!(next("after:1m", "", "2026-01-31"), "2026-02-28");
        assert_eq!(next("after:1y", "", "2024-02-29"), "2025-02-28");
        assert_eq!(next("after:3y", "", "2026-10-06"), "2029-10-06");
        assert!(!Rule::parse("after:3y").unwrap().counted() && Rule::parse("yearly:01-01").unwrap().counted());
        for bad in ["weekly:", "weekly:8", "monthly:0", "every:3", "every:0d", "after:2x", "after:21y", "after:", "yearly:13-01", "yearly:02-30", "yearly:12-31/0", "yearly:1231", "hourly", ""] {
            assert!(Rule::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn repeat_occurrences_skip_and_end() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        let today = "2026-09-30"; // Wednesday
        let mk = |v: serde_json::Value| {
            let mut p = json!({"groupId": a, "taskType": "repeat"});
            p.as_object_mut().unwrap().extend(v.as_object().unwrap().clone());
            create_task_on(&c, tp(p), today)
        };
        let view = |id: i64| {
            let mut v = task(&c, id).unwrap();
            enrich(&c, &mut v, today).unwrap();
            v
        };
        assert!(mk(json!({"title": "x"})).unwrap_err().contains("规则"));
        // counted series: dates come from the rule (a sent date is ignored); 7 of 10 done before
        let t = mk(json!({"title": "周报", "repeatRule": "weekly:6", "repeatTotal": 10, "doneCount": 7, "dueDate": "2020-01-01", "planDate": "2020-01-01"})).unwrap();
        assert_eq!((t.due_date.as_deref(), t.plan_date.as_deref(), t.count, t.last_date.as_deref()), (Some("2026-10-03"), None, 7, Some("2026-10-17")));
        assert!(create_task_on(&c, tp(json!({"parentId": t.id, "title": "sub"})), today).is_err());
        set_done_on(&c, t.id, true, "2026-10-02").unwrap(); // early: the next one is still after 10-03
        let u = view(t.id);
        assert_eq!((u.due_date.as_deref(), u.count, u.done), (Some("2026-10-10"), 8, false));
        skip_on(&c, t.id, "2026-10-09").unwrap(); // not counted
        assert_eq!(task(&c, t.id).unwrap().due_date.as_deref(), Some("2026-10-17"));
        set_done_on(&c, t.id, true, "2026-10-20").unwrap(); // late: the next one comes after the finish day
        assert_eq!((view(t.id).due_date.as_deref(), view(t.id).count), (Some("2026-10-24"), 9));
        set_done_on(&c, t.id, true, "2026-10-24").unwrap(); // 10th → series ends
        assert_eq!((view(t.id).done, view(t.id).count, view(t.id).last_date), (true, 10, None));
        assert!(skip_on(&c, t.id, "2026-10-25").is_err());
        assert!(update_task_on(&c, t.id, tp(json!({"doneCount": 2})), today).unwrap_err().contains("不能少于")); // 3 logged
        assert!(update_task_on(&c, t.id, tp(json!({"doneCount": 11})), today).is_err()); // more than the total
        assert!(!update_task_on(&c, t.id, tp(json!({"repeatTotal": 12})), today).unwrap().done); // reopened
        let u = update_task_on(&c, t.id, tp(json!({"repeatRule": "weekly:1"})), "2026-10-25").unwrap(); // new rule: re-planned
        assert_eq!(u.due_date.as_deref(), Some("2026-10-26"));
        let u = update_task_on(&c, t.id, tp(json!({"title": "周报 v2"})), "2026-11-30").unwrap(); // other edits keep the date
        assert_eq!(u.due_date.as_deref(), Some("2026-10-26"));

        // monthly: several days a month, counted like weekly ones (dates from today)
        let m = mk(json!({"title": "房租", "repeatRule": "monthly:31,15", "planDate": "2026-10-01", "repeatUntil": "2026-11-20", "repeatTotal": 3})).unwrap();
        assert_eq!((m.due_date.as_deref(), m.plan_date, m.repeat_until, m.last_date.as_deref()), (Some("2026-09-30"), None, None, Some("2026-10-31")));
        set_done_on(&c, m.id, true, "2026-09-30").unwrap();
        assert_eq!(task(&c, m.id).unwrap().due_date.as_deref(), Some("2026-10-15"));
        set_done_on(&c, m.id, true, "2026-10-15").unwrap();
        set_done_on(&c, m.id, true, "2026-10-31").unwrap(); // 3 of 3
        assert!(task(&c, m.id).unwrap().done);

        // after: first on its start day (today if that has passed), then n days after each finish
        let hair = mk(json!({"title": "理发", "repeatRule": "after:30d", "planDate": "2026-10-05", "repeatUntil": "2026-12-01"})).unwrap();
        assert_eq!((hair.due_date.as_deref(), hair.repeat_until), (Some("2026-10-05"), None));
        set_done_on(&c, hair.id, true, "2026-10-07").unwrap();
        assert_eq!(task(&c, hair.id).unwrap().due_date.as_deref(), Some("2026-11-06"));
        assert_eq!(mk(json!({"title": "p", "repeatRule": "after:7d", "planDate": "2026-09-01"})).unwrap().due_date.as_deref(), Some(today));
        // switching back to normal clears the repeat fields; the occurrence stays as its deadline
        let n = update_task_on(&c, hair.id, tp(json!({"taskType": "normal"})), today).unwrap();
        assert_eq!((n.task_type, n.repeat_rule, n.plan_date, n.due_date.as_deref()), (TaskType::Normal, None, None, Some("2026-11-06")));
        assert!(!agenda(&c, today).unwrap().due.iter().any(|p| p.task.title == "房租")); // series start is not a day to do it
    }

    #[test]
    fn checkin_streak_backfill_and_target() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        assert!(create_task(&c, tp(json!({"groupId": a, "title": "x", "taskType": "checkin", "repeatRule": "every:2d"}))).is_err());
        let open = create_task(&c, tp(json!({"groupId": a, "title": "open", "taskType": "checkin", "repeatRule": "daily", "repeatTotal": 9}))).unwrap();
        assert_eq!((open.start_date, open.due_date, open.repeat_total), (None, None, None)); // 不设: no range, no target
        let t = create_task(&c, tp(json!({"groupId": a, "title": "背单词", "taskType": "checkin", "repeatRule": "weekly:1,2,3,4,5",
            "planDate": "2026-01-01", "startDate": "2026-09-21", "dueDate": "2026-10-02"}))).unwrap();
        assert_eq!((t.plan_date.as_deref(), t.start_date.as_deref()), (None, Some("2026-09-21")));
        let today = "2026-09-30"; // Wednesday
        let view = |d: &str| {
            let mut v = task(&c, t.id).unwrap();
            enrich(&c, &mut v, d).unwrap();
            v
        };
        log_on(&c, t.id, "2026-09-28", 1, today).unwrap(); // Mon
        log_on(&c, t.id, "2026-09-29", 1, today).unwrap(); // Tue
        assert!(log_on(&c, t.id, "2026-09-27", 1, today).is_err()); // Sunday: not scheduled
        assert!(log_on(&c, t.id, "2026-10-01", 1, today).is_err()); // future
        assert!(log_on(&c, t.id, "2026-09-18", 1, today).is_err()); // before the start
        let v = view(today);
        assert_eq!((v.count, v.streak, v.logged_today), (2, 2, false)); // today still open
        set_done_on(&c, t.id, true, today).unwrap();
        let v = view(today);
        assert_eq!((v.count, v.streak, v.logged_today, v.done), (3, 3, true, false));
        log_on(&c, t.id, "2026-09-25", 1, today).unwrap(); // back-fill Fri
        assert_eq!((view(today).count, view(today).streak), (4, 4));
        log_on(&c, t.id, "2026-09-25", 1, today).unwrap(); // same day again = un-check
        assert_eq!(view("2026-10-02").streak, 0); // Thu 10-01 missed
        set_done_on(&c, t.id, false, today).unwrap(); // undo today's check-in
        assert_eq!(view(today).count, 2);
        // in 今天 only inside the range; once the range is over the sweep completes it
        assert!(agenda(&c, "2026-10-01").unwrap().due.iter().any(|p| p.task.id == t.id));
        assert!(!agenda(&c, "2026-10-05").unwrap().due.iter().any(|p| p.task.id == t.id));
        assert_eq!(close_ended(&c, "2026-10-02").unwrap(), 0);
        assert_eq!(close_ended(&c, "2026-10-03").unwrap(), 1);
        assert!(task(&c, t.id).unwrap().done && !task(&c, open.id).unwrap().done);
        // moving the end later reopens it
        assert!(!update_task_on(&c, t.id, tp(json!({"dueDate": "2026-10-31"})), "2026-10-03").unwrap().done);
    }

    /// 「已放弃」 is gone: it can't be set any more, and old data files import it as 暂停.
    #[test]
    fn no_abandoned_goals() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        assert!(create_goal(&c, serde_json::from_value(json!({"groupId": a, "name": "x", "status": "abandoned"})).unwrap()).is_err());
        let g = create_goal(&c, serde_json::from_value(json!({"groupId": a, "name": "G"})).unwrap()).unwrap().id;
        assert!(update_goal(&c, g, serde_json::from_value(json!({"status": "abandoned"})).unwrap()).is_err());
        let rec: TodoRecord = serde_json::from_value(json!({"groups": [{"name": "B", "goals": [{"name": "旧", "status": "abandoned"}]}]})).unwrap();
        replace_all(&c, &rec).unwrap();
        assert_eq!(goals(&c).unwrap()[0].status, GoalStatus::Paused);
    }

    /// Every scheduled day of the 时间段 checked → done at once (not only when the range is over);
    /// un-checking one of them reopens it. Without a start the range begins on the day it was added.
    #[test]
    fn checkin_done_when_every_day_is_checked() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        let g = create_goal(&c, serde_json::from_value(json!({"groupId": a, "name": "G"})).unwrap()).unwrap().id;
        let t = create_task(&c, tp(json!({"goalId": g, "title": "早起", "taskType": "checkin", "repeatRule": "daily", "startDate": "2099-10-05", "dueDate": "2099-10-07"}))).unwrap();
        log_on(&c, t.id, "2099-10-05", 1, "2099-10-07").unwrap();
        log_on(&c, t.id, "2099-10-06", 1, "2099-10-07").unwrap();
        assert!(!task(&c, t.id).unwrap().done);
        set_done_on(&c, t.id, true, "2099-10-07").unwrap(); // the last day
        assert!(task(&c, t.id).unwrap().done);
        assert_eq!((goal(&c, g).unwrap().done, goal(&c, g).unwrap().total), (1, 1)); // the goal sees it
        log_on(&c, t.id, "2099-10-06", 1, "2099-10-07").unwrap(); // un-check a day
        assert!(!task(&c, t.id).unwrap().done);
        // weekly: only the scheduled days matter
        let w = create_task(&c, tp(json!({"groupId": a, "title": "周会", "taskType": "checkin", "repeatRule": format!("weekly:{}", common::weekday("2099-10-05")), "startDate": "2099-10-05", "dueDate": "2099-10-11"}))).unwrap();
        log_on(&c, w.id, "2099-10-05", 1, "2099-10-06").unwrap();
        assert!(task(&c, w.id).unwrap().done);
        // only an end date: from the day it was added (the screenshot's case)
        let today = today(&c).unwrap();
        let s = create_task(&c, tp(json!({"groupId": a, "title": "今天", "taskType": "checkin", "repeatRule": "daily", "dueDate": today}))).unwrap();
        set_done_on(&c, s.id, true, &today).unwrap();
        assert!(task(&c, s.id).unwrap().done);
        // checked in under the old rule (still open) → the daily sweep completes it
        let old = create_task(&c, tp(json!({"groupId": a, "title": "旧", "taskType": "checkin", "repeatRule": "daily", "startDate": "2099-10-05", "dueDate": "2099-10-06"}))).unwrap();
        c.execute("INSERT INTO todo_logs (task_id, day) VALUES (?1, '2099-10-05'), (?1, '2099-10-06')", [old.id]).unwrap();
        assert!(!task(&c, old.id).unwrap().done);
        assert_eq!(close_ended(&c, "2099-10-06").unwrap(), 1);
        assert!(task(&c, old.id).unwrap().done);
        // no end date: never completes by itself
        let open = create_task(&c, tp(json!({"groupId": a, "title": "一直", "taskType": "checkin", "repeatRule": "daily"}))).unwrap();
        set_done_on(&c, open.id, true, &today).unwrap();
        assert!(!task(&c, open.id).unwrap().done);
    }

    #[test]
    fn progress_counts_toward_target() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        assert!(create_task(&c, tp(json!({"groupId": a, "title": "x", "taskType": "progress"}))).is_err());
        let books = create_task(&c, tp(json!({"groupId": a, "title": "读书", "taskType": "progress", "target": 3, "unit": "本", "period": "week"}))).unwrap();
        assert_eq!(books.period, Some(Period::All)); // per-period counting was dropped
        set_done_on(&c, books.id, true, "2026-09-30").unwrap();
        log_on(&c, books.id, "2026-09-30", 2, "2026-09-30").unwrap();
        let v = task_view(&c, books.id).unwrap();
        assert_eq!((v.done, v.count, v.total_amount), (true, 3, 3));
        let newest = logs(&c, books.id).unwrap()[0].id;
        delete_log(&c, newest).unwrap(); // back to 1 → reopened
        assert!(!task(&c, books.id).unwrap().done);
        assert!(log_on(&c, books.id, "2026-09-30", 0, "2026-09-30").is_err());
        // a time range works as for normal tasks: 今天 every day inside it
        let cv = create_task(&c, tp(json!({"groupId": a, "title": "投简历", "taskType": "progress", "target": 5, "startDate": "2026-09-28", "dueDate": "2026-10-04"}))).unwrap();
        assert!(agenda(&c, "2026-09-30").unwrap().due.iter().any(|p| p.task.id == cv.id));
        assert!(agenda(&c, "2026-10-06").unwrap().overdue.iter().any(|p| p.task.id == cv.id));
    }

    #[test]
    fn today_review_and_month_with_types() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        let today = today(&c).unwrap();
        let ck = create_task(&c, tp(json!({"groupId": a, "title": "打卡", "taskType": "checkin", "repeatRule": "daily", "planDate": today}))).unwrap();
        let rep = create_task(&c, tp(json!({"groupId": a, "title": "周报", "taskType": "repeat", "repeatRule": "daily", "repeatTotal": 2}))).unwrap();
        assert_eq!(titles(&agenda(&c, &today).unwrap().due), ["打卡", "周报"]);
        set_done(&c, ck.id, true).unwrap();
        set_done(&c, rep.id, true).unwrap();
        let ag = agenda(&c, &today).unwrap();
        assert!(ag.due.is_empty()); // 周报 moved on to tomorrow
        assert_eq!(titles(&ag.done_today), ["打卡", "周报"]);
        let rv = done_between(&c, &today, &today).unwrap();
        let d: Vec<_> = rv.iter().map(|x| (x.task.task.title.as_str(), x.detail.as_deref())).collect();
        assert_eq!(d, [("打卡", Some("打卡")), ("周报", Some("第 1 次"))]);

        let water = create_task(&c, tp(json!({"groupId": a, "title": "浇花", "taskType": "repeat", "repeatRule": "every:2d"}))).unwrap();
        let cal = month(&c, &today, &common::add_days(&today, 6)).unwrap();
        let of = |id: i64| cal.iter().filter(|x| x.task.task.id == id).map(|x| (x.what.as_str(), x.detail.clone())).collect::<Vec<_>>();
        assert_eq!(
            of(water.id),
            [("due", None), ("next", Some("第 2 次".into())), ("next", Some("第 3 次".into())), ("next", Some("第 4 次".into()))]
        );
        assert_eq!(of(rep.id), [("due", None), ("done", Some("第 1 次".into()))]); // tomorrow is the 2nd of 2: no more projections
        assert!(of(ck.id).iter().all(|(w, _)| *w == "done")); // check-ins only show when done
    }

    #[test]
    fn reorder_siblings_goals_groups() {
        let c = memory_db();
        let a = create_group(&c, "A").unwrap().id;
        let b = create_group(&c, "B").unwrap().id;
        reorder_groups(&c, &[b, a]).unwrap();
        assert_eq!(groups(&c).unwrap().iter().map(|g| g.id).collect::<Vec<_>>(), [b, a]);
        let mk = |t: &str| create_task(&c, tp(json!({"groupId": a, "title": t}))).unwrap().id;
        let (t1, t2, t3) = (mk("1"), mk("2"), mk("3"));
        let sub = create_task(&c, tp(json!({"parentId": t1, "title": "s"}))).unwrap().id;
        reorder_tasks(&c, &[t3, t1, t2]).unwrap();
        assert_eq!(tasks(&c, Some(a), None).unwrap().iter().filter(|t| t.parent_id.is_none()).map(|t| t.id).collect::<Vec<_>>(), [t3, t1, t2]);
        reorder_tasks(&c, &[t2, t3]).unwrap(); // part of the list (e.g. the undone ones): t1 keeps its slot
        assert_eq!(tasks(&c, Some(a), None).unwrap().iter().filter(|t| t.parent_id.is_none()).map(|t| t.id).collect::<Vec<_>>(), [t2, t1, t3]);
        assert!(reorder_tasks(&c, &[t3, t1, t2, sub]).is_err()); // not siblings
        let g1 = create_goal(&c, gp(json!({"groupId": a, "name": "g1"}))).unwrap().id;
        let g2 = create_goal(&c, gp(json!({"groupId": a, "name": "g2"}))).unwrap().id;
        reorder_goals(&c, a, &[g2, g1]).unwrap();
        assert_eq!(goals(&c).unwrap().iter().map(|g| g.id).collect::<Vec<_>>(), [g2, g1]);
    }
}
