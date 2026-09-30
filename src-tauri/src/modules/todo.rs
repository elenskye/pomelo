//! 一步一步 (todo, modeled on the app GoalMap): groups → goals → tasks → multi-level subtasks.
//! Placement: every task has group_id (+ goal_id under a goal); subtasks inherit both from their root
//! and only root tasks can be moved. Siblings are ordered by sort_order (new ones append).
//! Completion (`set_done`): checking a task checks its whole subtree, and a parent whose children are
//! all done is checked too (recursively); unchecking a task (or adding an undone subtask) unchecks
//! its ancestors. Goal status stays manual; progress = done leaf tasks / leaf tasks.
//! 今天 (`agenda`): undone tasks outside paused/closed goals that are planned on or before today, due
//! today, or have today inside their [start, due] range. 已过期: due before today, or — with no due
//! date — planned before today.

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
    Abandoned => "abandoned",
});

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
    let tx = conn.unchecked_transaction().map_err(err)?;
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
}

/// Create: place by `parentId` > `goalId` > `groupId`. Update: `groupId` / `goalId` move a root task
/// (with its subtree); `parentId` is create-only. Completion goes through `set_done`.
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
}

const TASK_COLS: &str = "t.id, t.group_id, t.goal_id, t.parent_id, t.title, t.note, t.link, t.plan_date, t.start_date,
  t.due_date, t.done, t.done_at, t.collapsed, t.sort_order, t.created_at, t.updated_at";

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
    })
}

pub fn task(conn: &Connection, id: i64) -> Result<Task, String> {
    conn.query_row(&format!("SELECT {TASK_COLS} FROM todo_tasks t WHERE t.id = ?1"), [id], task_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("任务"))
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
    let rows = st.query_map([id], task_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
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

struct TaskFields {
    title: String,
    note: Option<String>,
    link: Option<String>,
    plan_date: Option<String>,
    start_date: Option<String>,
    due_date: Option<String>,
}

impl TaskFields {
    fn validated(self) -> Result<Self, String> {
        let f = Self {
            title: common::require(&self.title, "任务", 500)?,
            note: common::clean(self.note),
            link: common::clean(self.link),
            plan_date: common::clean(self.plan_date),
            start_date: common::clean(self.start_date),
            due_date: common::clean(self.due_date),
        };
        common::check_len(&f.note, "备注", 5000)?;
        common::check_len(&f.link, "链接", 1000)?;
        common::check_date(&f.plan_date, "计划日期")?;
        common::check_date(&f.start_date, "开始日期")?;
        common::check_date(&f.due_date, "截止日期")?;
        if f.start_date.is_some() && f.due_date.is_none() {
            return Err("时间段要有结束日期".into());
        }
        check_range(&f.start_date, &f.due_date, "时间段")?;
        Ok(f)
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
    let place = resolve_place(conn, patch.group_id, patch.goal_id.flatten(), patch.parent_id)?;
    let f = TaskFields {
        title: patch.title.unwrap_or_default(),
        note: patch.note.flatten(),
        link: patch.link.flatten(),
        plan_date: patch.plan_date.flatten(),
        start_date: patch.start_date.flatten(),
        due_date: patch.due_date.flatten(),
    }
    .validated()?;
    let tx = conn.unchecked_transaction().map_err(err)?;
    tx.execute(
        "INSERT INTO todo_tasks (group_id, goal_id, parent_id, title, note, link, plan_date, start_date, due_date, collapsed, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            place.group_id, place.goal_id, place.parent_id, f.title, f.note, f.link, f.plan_date, f.start_date, f.due_date,
            patch.collapsed.unwrap_or(false), next_sort(&tx, place)?
        ],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    reopen_ancestors(&tx, id)?;
    tx.commit().map_err(err)?;
    task(conn, id)
}

pub fn update_task(conn: &Connection, id: i64, patch: TaskPatch) -> Result<Task, String> {
    let old = task(conn, id)?;
    if patch.parent_id.is_some() {
        return Err("不能在这里改上一层任务".into());
    }
    let f = TaskFields {
        title: patch.title.unwrap_or(old.title.clone()),
        note: patch.note.unwrap_or(old.note.clone()),
        link: patch.link.unwrap_or(old.link.clone()),
        plan_date: patch.plan_date.unwrap_or(old.plan_date.clone()),
        start_date: patch.start_date.unwrap_or(old.start_date.clone()),
        due_date: patch.due_date.unwrap_or(old.due_date.clone()),
    }
    .validated()?;
    let tx = conn.unchecked_transaction().map_err(err)?;
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
               collapsed = ?7, updated_at = {NOW} WHERE id = ?8"
        ),
        params![f.title, f.note, f.link, f.plan_date, f.start_date, f.due_date, patch.collapsed.unwrap_or(old.collapsed), id],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    task(conn, id)
}

/// See module docs for the cascade rules.
pub fn set_done(conn: &Connection, id: i64, done: bool) -> Result<(), String> {
    task(conn, id)?;
    let tx = conn.unchecked_transaction().map_err(err)?;
    if done {
        tx.execute(
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
        while let Some(parent) = task(&tx, child)?.parent_id {
            let open: i64 = tx
                .query_row("SELECT count(*) FROM todo_tasks WHERE parent_id = ?1 AND done = 0", [parent], |r| r.get(0))
                .map_err(err)?;
            if open > 0 {
                break;
            }
            tx.execute(
                &format!("UPDATE todo_tasks SET done = 1, done_at = {NOW}, updated_at = {NOW} WHERE id = ?1 AND done = 0"),
                [parent],
            )
            .map_err(err)?;
            child = parent;
        }
    } else {
        tx.execute(&format!("UPDATE todo_tasks SET done = 0, done_at = NULL, updated_at = {NOW} WHERE id = ?1"), [id])
            .map_err(err)?;
        reopen_ancestors(&tx, id)?;
    }
    tx.commit().map_err(err)
}

/// Deletes the task and all its subtasks (FK cascade).
pub fn delete_task(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM todo_tasks WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(gone("任务")),
        _ => Ok(()),
    }
}

// ---------- 今天 / 回看 ----------

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

fn placed(conn: &Connection, filter: &str, order: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<PlacedTask>, String> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {TASK_COLS}, g.name, go.name, p.title
             FROM todo_tasks t JOIN todo_groups g ON g.id = t.group_id
               LEFT JOIN todo_goals go ON go.id = t.goal_id LEFT JOIN todo_tasks p ON p.id = t.parent_id
             WHERE {filter} ORDER BY {order}"
        ))
        .map_err(err)?;
    let rows = st
        .query_map(args, |r| {
            Ok(PlacedTask { task: task_row(r)?, group_name: r.get(16)?, goal_name: r.get(17)?, parent_title: r.get(18)? })
        })
        .map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

fn is_overdue(t: &Task, today: &str) -> bool {
    match (&t.due_date, &t.plan_date) {
        (Some(d), _) => d.as_str() < today,
        (None, Some(p)) => p.as_str() < today,
        (None, None) => false,
    }
}

pub fn agenda(conn: &Connection, today: &str) -> Result<Agenda, String> {
    common::check_date(&Some(today.to_string()), "日期")?;
    let open = placed(
        conn,
        "t.done = 0 AND (go.id IS NULL OR go.status = 'active')
         AND (t.plan_date <= ?1 OR t.due_date <= ?1 OR (t.start_date <= ?1 AND ?1 <= t.due_date))",
        "g.sort_order, g.id, go.sort_order, go.id, t.sort_order, t.id",
        &[&today],
    )?;
    let (mut overdue, due): (Vec<_>, Vec<_>) = open.into_iter().partition(|p| is_overdue(&p.task, today));
    overdue.sort_by(|a, b| {
        let key = |p: &PlacedTask| p.task.due_date.clone().or(p.task.plan_date.clone());
        key(a).cmp(&key(b))
    });
    let done_today = placed(conn, "t.done = 1 AND date(t.done_at, 'localtime') = ?1", "t.done_at", &[&today])?;
    Ok(Agenda { today: today.into(), overdue, due, done_today })
}

/// Tasks checked off between two local dates (inclusive), in completion order.
pub fn done_between(conn: &Connection, from: &str, to: &str) -> Result<Vec<PlacedTask>, String> {
    common::check_date(&Some(from.to_string()), "开始日期")?;
    common::check_date(&Some(to.to_string()), "结束日期")?;
    placed(conn, "t.done = 1 AND date(t.done_at, 'localtime') BETWEEN ?1 AND ?2", "t.done_at", &[&from, &to])
}

fn today(conn: &Connection) -> Result<String, String> {
    conn.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0)).map_err(err)
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
        TaskFields {
            title: self.title.clone(),
            note: self.note.clone(),
            link: self.link.clone(),
            plan_date: self.plan_date.clone(),
            start_date: self.start_date.clone(),
            due_date: self.due_date.clone(),
        }
        .validated()
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
                status: goal.status,
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
                   done, done_at, collapsed, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, coalesce(?14, {NOW}), coalesce(?15, ?14, {NOW}))"
            ),
            params![
                place.group_id, place.goal_id, place.parent_id, f.title, f.note, f.link, f.plan_date, f.start_date, f.due_date,
                t.done, t.done_at, t.collapsed, i as i64 + 1, t.created_at, t.updated_at
            ],
        )
        .map_err(err)?;
        let child = Place { parent_id: Some(conn.last_insert_rowid()), ..place };
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
                    group_id, goal.name.trim(), goal.status, common::clean(goal.start_date.clone()), common::clean(goal.end_date.clone()),
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

fn task_tree(all: &[Task]) -> Vec<TaskRecord> {
    let mut by_parent: HashMap<Option<i64>, Vec<&Task>> = HashMap::new();
    for t in all {
        by_parent.entry(t.parent_id).or_default().push(t);
    }
    fn build(parent: Option<i64>, by_parent: &HashMap<Option<i64>, Vec<&Task>>) -> Vec<TaskRecord> {
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
                        children: build(Some(t.id), by_parent),
                        created_at: Some(t.created_at.clone()),
                        updated_at: Some(t.updated_at.clone()),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    build(None, &by_parent)
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
                        tasks: task_tree(&tasks(conn, None, Some(x.id))?),
                        created_at: Some(x.created_at.clone()),
                        updated_at: Some(x.updated_at.clone()),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(GroupRecord {
                tasks: task_tree(&tasks(conn, Some(g.id), None)?),
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
    state.with_db(|c| create_group(c, &name))
}
#[tauri::command]
pub fn todo_group_rename(state: State<AppState>, id: i64, name: String) -> Result<Group, String> {
    state.with_db(|c| rename_group(c, id, &name))
}
#[tauri::command]
pub fn todo_group_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_db(|c| delete_group(c, id))
}
#[tauri::command]
pub fn todo_goals(state: State<AppState>) -> Result<Vec<Goal>, String> {
    state.with_db(|c| goals(c))
}
#[tauri::command]
pub fn todo_goal_create(state: State<AppState>, patch: GoalPatch) -> Result<Goal, String> {
    state.with_db(|c| create_goal(c, patch))
}
#[tauri::command]
pub fn todo_goal_update(state: State<AppState>, id: i64, patch: GoalPatch) -> Result<Goal, String> {
    state.with_db(|c| update_goal(c, id, patch))
}
#[tauri::command]
pub fn todo_goal_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_db(|c| delete_goal(c, id))
}
#[tauri::command]
pub fn todo_tasks(state: State<AppState>, group_id: Option<i64>, goal_id: Option<i64>) -> Result<Vec<Task>, String> {
    state.with_db(|c| tasks(c, group_id, goal_id))
}
#[tauri::command]
pub fn todo_task_create(state: State<AppState>, patch: TaskPatch) -> Result<Task, String> {
    state.with_db(|c| create_task(c, patch))
}
#[tauri::command]
pub fn todo_task_update(state: State<AppState>, id: i64, patch: TaskPatch) -> Result<Task, String> {
    state.with_db(|c| update_task(c, id, patch))
}
#[tauri::command]
pub fn todo_task_set_done(state: State<AppState>, id: i64, done: bool) -> Result<(), String> {
    state.with_db(|c| set_done(c, id, done))
}
#[tauri::command]
pub fn todo_task_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_db(|c| delete_task(c, id))
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
pub fn todo_done_between(state: State<AppState>, from: String, to: String) -> Result<Vec<PlacedTask>, String> {
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
        assert_eq!(titles(&done_between(&c, &today, &today).unwrap()), ["finished"]);
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
            {"name": "📌 Pick Up", "goals": [{"name": "Personal Website", "status": "paused", "review": "r", "tasks": [{"title": "选框架"}]}], "tasks": [{"title": "嘉信入金", "link": "https://x"}]}
        ]}))
        .unwrap();
        assert_eq!(rec.len(), 2 + 1 + 8);
        assert_eq!(replace_all(&c, &rec).unwrap(), 11);
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

        let dup: TodoRecord = serde_json::from_value(json!({"groups": [{"name": "X"}, {"name": " X "}]})).unwrap();
        assert!(validate_records(&dup).unwrap_err().contains("重复"));
        let bad: TodoRecord = serde_json::from_value(json!({"groups": [{"name": "X", "tasks": [{"title": "ok", "children": [{"title": " "}]}]}]})).unwrap();
        assert!(validate_records(&bad).is_err());
    }
}
