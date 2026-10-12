//! Organising 小拾's conversations (from Picky's ConversationLibrary / SessionGroups / SidebarOrder):
//! - Lists: 置顶 = pinned and not archived (shown only there, whatever its project) · each project =
//!   its sessions not pinned / archived · 最近 = the rest · 归档 = archived.
//! - A list follows last activity until the user drags within it (`manual_lists`); then it keeps
//!   that order and sessions arriving since (moved in, new) come first, by activity. A drop that
//!   leaves the order as it was changes nothing (an automatic list stays automatic).
//! - Dropping into a list changes membership: 置顶 pins (keeps the project), a project moves in and
//!   unpins, 最近 leaves the project and unpins, 归档 archives; out of 归档 restores. Sending in an
//!   archived session restores it too (chat.rs).
//! - None of this touches `updated_at` (= last activity). Everything here is undoable
//!   (`with_picky_undo`).
//! - Projects: name (60 chars; empty = 未命名项目) and background instructions (sent with its
//!   sessions, chat.rs). New ones go first. Deleting one keeps its sessions (they go to 最近).

use super::chat::{self, auto_title, NEW_TITLE};
use super::store::NOW;
use crate::modules::common::{err, Tx};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashSet;
use tauri::State;

pub const UNNAMED: &str = "未命名项目";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum List {
    Pinned,
    Recent,
    Archived,
    Project(i64),
}

impl List {
    pub fn key(self) -> String {
        match self {
            List::Pinned => "pinned".into(),
            List::Recent => "recent".into(),
            List::Archived => "archived".into(),
            List::Project(id) => format!("project:{id}"),
        }
    }
    pub fn parse(s: &str) -> Result<List, String> {
        Ok(match s {
            "pinned" => List::Pinned,
            "recent" => List::Recent,
            "archived" => List::Archived,
            _ => List::Project(s.strip_prefix("project:").and_then(|n| n.parse().ok()).ok_or_else(|| format!("不认识的列表：{s}"))?),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: i64,
    pub title: String,
    pub project_id: Option<i64>,
    pub pinned: bool,
    pub archived: bool,
    pub updated_at: String,
    #[serde(skip)]
    sort_order: Option<i64>,
}

impl SessionRow {
    fn list(&self, projects: &HashSet<i64>) -> List {
        if self.archived {
            List::Archived
        } else if self.pinned {
            List::Pinned
        } else {
            match self.project_id {
                Some(p) if projects.contains(&p) => List::Project(p),
                _ => List::Recent,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub id: i64,
    pub name: String,
    pub instructions: String,
    pub sessions: Vec<SessionRow>,
    /// How many project files it has.
    pub files: i64,
    /// Its conversations also use global memories.
    pub include_global_memories: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub pinned: Vec<SessionRow>,
    pub projects: Vec<ProjectRow>,
    pub recent: Vec<SessionRow>,
    pub archived: Vec<SessionRow>,
}

impl Library {
    pub fn members(&self, list: List) -> Option<Vec<i64>> {
        let ids = |v: &Vec<SessionRow>| v.iter().map(|s| s.id).collect();
        Some(match list {
            List::Pinned => ids(&self.pinned),
            List::Recent => ids(&self.recent),
            List::Archived => ids(&self.archived),
            List::Project(p) => ids(&self.projects.iter().find(|x| x.id == p)?.sessions),
        })
    }
}

fn sessions(conn: &Connection) -> Result<Vec<SessionRow>, String> {
    let mut st = conn
        .prepare("SELECT id, title, project_id, pinned_at IS NOT NULL, archived_at IS NOT NULL, updated_at, sort_order FROM sessions ORDER BY updated_at DESC, id DESC")
        .map_err(err)?;
    let rows = st
        .query_map([], |r| {
            Ok(SessionRow { id: r.get(0)?, title: r.get(1)?, project_id: r.get(2)?, pinned: r.get(3)?, archived: r.get(4)?, updated_at: r.get(5)?, sort_order: r.get(6)? })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

fn manual(conn: &Connection) -> Result<HashSet<String>, String> {
    let mut st = conn.prepare("SELECT list FROM manual_lists").map_err(err)?;
    let rows = st.query_map([], |r| r.get(0)).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Everything the conversation list shows, each list in display order.
pub fn library(conn: &Connection) -> Result<Library, String> {
    let mut st = conn.prepare("SELECT id, name, instructions, (SELECT count(*) FROM project_files f WHERE f.project_id = projects.id), include_global_memories FROM projects ORDER BY sort_order, id").map_err(err)?;
    let mut projects: Vec<ProjectRow> =
        st.query_map([], |r| Ok(ProjectRow { id: r.get(0)?, name: r.get(1)?, instructions: r.get(2)?, sessions: vec![], files: r.get(3)?, include_global_memories: r.get(4)? })).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let known: HashSet<i64> = projects.iter().map(|p| p.id).collect();
    let manual = manual(conn)?;
    let mut lib = Library { pinned: vec![], projects: vec![], recent: vec![], archived: vec![] };
    for s in sessions(conn)? {
        match s.list(&known) {
            List::Pinned => lib.pinned.push(s),
            List::Recent => lib.recent.push(s),
            List::Archived => lib.archived.push(s),
            List::Project(p) => projects.iter_mut().find(|x| x.id == p).expect("known project").sessions.push(s),
        }
    }
    // rows arrive newest activity first; a hand-arranged list puts newcomers (no place yet) on top
    let arrange = |v: &mut Vec<SessionRow>, list: List| {
        if manual.contains(&list.key()) {
            v.sort_by_key(|s| s.sort_order.map_or((0, 0), |o| (1, o)));
        }
    };
    arrange(&mut lib.pinned, List::Pinned);
    arrange(&mut lib.recent, List::Recent);
    arrange(&mut lib.archived, List::Archived);
    for p in projects.iter_mut() {
        arrange(&mut p.sessions, List::Project(p.id));
    }
    lib.projects = projects;
    Ok(lib)
}

fn row(conn: &Connection, id: i64) -> Result<SessionRow, String> {
    conn.query_row(
        "SELECT id, title, project_id, pinned_at IS NOT NULL, archived_at IS NOT NULL, updated_at, sort_order FROM sessions WHERE id = ?1",
        [id],
        |r| Ok(SessionRow { id: r.get(0)?, title: r.get(1)?, project_id: r.get(2)?, pinned: r.get(3)?, archived: r.get(4)?, updated_at: r.get(5)?, sort_order: r.get(6)? }),
    )
    .optional()
    .map_err(err)?
    .ok_or_else(|| "找不到这个对话，可能已经被删除了".into())
}

fn project_ids(conn: &Connection) -> Result<HashSet<i64>, String> {
    let mut st = conn.prepare("SELECT id FROM projects").map_err(err)?;
    let rows = st.query_map([], |r| r.get(0)).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Changes a session's grouping fields; when that moves it to another list it arrives there first
/// (no place yet).
fn regroup(conn: &Connection, id: i64, change: impl Fn(&mut SessionRow)) -> Result<(), String> {
    let known = project_ids(conn)?;
    let before = row(conn, id)?;
    let mut after = before.clone();
    change(&mut after);
    if after == before {
        return Ok(());
    }
    let moved = before.list(&known) != after.list(&known);
    conn.execute(
        &format!(
            "UPDATE sessions SET project_id = ?1,
               pinned_at = CASE WHEN ?2 THEN coalesce(pinned_at, {NOW}) END,
               archived_at = CASE WHEN ?3 THEN coalesce(archived_at, {NOW}) END,
               sort_order = CASE WHEN ?4 THEN NULL ELSE sort_order END
             WHERE id = ?5"
        ),
        params![after.project_id, after.pinned, after.archived, moved, id],
    )
    .map_err(err)?;
    Ok(())
}

pub fn rename(conn: &Connection, id: i64, title: &str) -> Result<(), String> {
    row(conn, id)?;
    let t = title.trim();
    if t.is_empty() {
        // back to the automatic title (from the first question shown)
        let first = chat::path(conn, id)?.into_iter().find(|m| m.role == "user").map(|m| auto_title(&m.content));
        conn.execute("UPDATE sessions SET title = ?1, title_source = 'auto' WHERE id = ?2", params![first.unwrap_or_else(|| NEW_TITLE.into()), id])
    } else {
        conn.execute("UPDATE sessions SET title = ?1, title_source = 'user' WHERE id = ?2", params![t.chars().take(120).collect::<String>(), id])
    }
    .map(|_| ())
    .map_err(err)
}

pub fn set_pinned(conn: &Connection, ids: &[i64], pinned: bool) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    for id in ids {
        regroup(&tx, *id, |s| s.pinned = pinned)?;
    }
    tx.commit()
}

pub fn set_archived(conn: &Connection, ids: &[i64], archived: bool) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    for id in ids {
        regroup(&tx, *id, |s| s.archived = archived)?;
    }
    tx.commit()
}

/// `project` None = out of any project.
pub fn move_to(conn: &Connection, ids: &[i64], project: Option<i64>) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    if let Some(p) = project {
        if !project_ids(&tx)?.contains(&p) {
            return Err("这个项目已经被删除了".into());
        }
    }
    for id in ids {
        regroup(&tx, *id, |s| s.project_id = project)?;
    }
    tx.commit()
}

/// Deletes sessions with all their messages and runs.
pub fn delete(conn: &Connection, ids: &[i64]) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    for id in ids {
        tx.execute("DELETE FROM sessions WHERE id = ?1", [id]).map_err(err)?;
    }
    tx.commit()
}

fn project_name(name: &str) -> String {
    let t = name.trim();
    if t.is_empty() {
        UNNAMED.into()
    } else {
        t.chars().take(60).collect()
    }
}

pub fn create_project(conn: &Connection, name: &str, instructions: &str) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO projects (name, instructions, sort_order) VALUES (?1, ?2, (SELECT coalesce(min(sort_order), 0) - 1 FROM projects))",
        params![project_name(name), instructions.trim()],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

pub fn set_include_global(conn: &Connection, id: i64, on: bool) -> Result<(), String> {
    conn.execute("UPDATE projects SET include_global_memories = ?1 WHERE id = ?2 AND include_global_memories <> ?1", params![on, id]).map(|_| ()).map_err(err)
}

pub fn update_project(conn: &Connection, id: i64, name: &str, instructions: &str) -> Result<(), String> {
    let n = conn
        .execute(&format!("UPDATE projects SET name = ?1, instructions = ?2, updated_at = {NOW} WHERE id = ?3"), params![project_name(name), instructions.trim(), id])
        .map_err(err)?;
    if n == 0 {
        return Err("这个项目已经被删除了".into());
    }
    Ok(())
}

/// Its sessions stay: the ones listed under it arrive first in 最近; pinned / archived ones keep
/// their place.
pub fn delete_project(conn: &Connection, id: i64) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    tx.execute("UPDATE sessions SET project_id = NULL, sort_order = NULL WHERE project_id = ?1 AND pinned_at IS NULL AND archived_at IS NULL", [id]).map_err(err)?;
    tx.execute("UPDATE sessions SET project_id = NULL WHERE project_id = ?1", [id]).map_err(err)?;
    tx.execute("DELETE FROM manual_lists WHERE list = ?1", [List::Project(id).key()]).map_err(err)?;
    super::memory::orphan(&tx, id)?;
    tx.execute("DELETE FROM projects WHERE id = ?1", [id]).map_err(err)?;
    tx.commit()
}

pub fn reorder_projects(conn: &Connection, ids: &[i64]) -> Result<(), String> {
    let have = project_ids(conn)?;
    if ids.len() != have.len() || ids.iter().collect::<HashSet<_>>().len() != ids.len() || !ids.iter().all(|i| have.contains(i)) {
        return Err("项目列表已经变了，请重新排列".into());
    }
    let tx = Tx::begin(conn)?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute("UPDATE projects SET sort_order = ?1 WHERE id = ?2", [i as i64, *id]).map_err(err)?;
    }
    tx.commit()
}

/// A drag in the conversation list: `id` goes into `list` before / after a member (neither = top).
/// Returns false when nothing changed (dropped back in place).
pub fn drop(conn: &Connection, id: i64, list: List, before: Option<i64>, after: Option<i64>) -> Result<bool, String> {
    if before.is_some() && after.is_some() {
        return Err("拖放位置不对".into());
    }
    let tx = Tx::begin(conn)?;
    let lib = library(&tx)?;
    let members = lib.members(list).ok_or("这个项目已经被删除了")?;
    let known = project_ids(&tx)?;
    let from = row(&tx, id)?.list(&known);
    let target = before.or(after);
    if let Some(t) = target {
        if !members.contains(&t) {
            return Err("对话列表已经变了，请重新拖一次".into());
        }
        if t == id {
            return Ok(false);
        }
    }
    let mut order: Vec<i64> = members.iter().copied().filter(|x| *x != id).collect();
    let at = match (before, after) {
        (Some(b), _) => order.iter().position(|x| *x == b).unwrap_or(0),
        (_, Some(a)) => order.iter().position(|x| *x == a).map_or(0, |i| i + 1),
        _ => 0,
    };
    order.insert(at, id);
    if from == list && order == members {
        return Ok(false);
    }
    regroup(&tx, id, |s| match list {
        List::Pinned => {
            s.pinned = true;
            s.archived = false;
        }
        List::Recent => {
            s.pinned = false;
            s.archived = false;
            s.project_id = None;
        }
        List::Project(p) => {
            s.pinned = false;
            s.archived = false;
            s.project_id = Some(p);
        }
        List::Archived => s.archived = true,
    })?;
    for (i, sid) in order.iter().enumerate() {
        tx.execute("UPDATE sessions SET sort_order = ?1 WHERE id = ?2", [i as i64, *sid]).map_err(err)?;
    }
    tx.execute("INSERT OR IGNORE INTO manual_lists (list) VALUES (?1)", [list.key()]).map_err(err)?;
    tx.commit()?;
    Ok(true)
}

// ---------- commands ----------

#[tauri::command]
pub fn picky_library(state: State<AppState>) -> Result<Library, String> {
    state.picky.with_db(|c| library(c))
}

#[tauri::command]
pub fn picky_rename(state: State<AppState>, id: i64, title: String) -> Result<(), String> {
    state.with_picky_undo("改名", |c| rename(c, id, &title))
}

#[tauri::command]
pub fn picky_pin(state: State<AppState>, ids: Vec<i64>, pinned: bool) -> Result<(), String> {
    state.with_picky_undo(if pinned { "置顶" } else { "取消置顶" }, |c| set_pinned(c, &ids, pinned))
}

#[tauri::command]
pub fn picky_archive(state: State<AppState>, ids: Vec<i64>, archived: bool) -> Result<(), String> {
    state.with_picky_undo(if archived { "归档" } else { "恢复" }, |c| set_archived(c, &ids, archived))
}

#[tauri::command]
pub fn picky_move(state: State<AppState>, ids: Vec<i64>, project_id: Option<i64>) -> Result<(), String> {
    state.with_picky_undo(if project_id.is_some() { "移到项目" } else { "移出项目" }, |c| move_to(c, &ids, project_id))
}

#[tauri::command]
pub fn picky_delete(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    if state.picky.runs.active_session().is_some_and(|s| ids.contains(&s)) {
        return Err("这个对话正在写回复，等它写完再删除。".into());
    }
    state.with_picky_undo("删除对话", |c| delete(c, &ids))
}

#[tauri::command]
pub fn picky_project_create(state: State<AppState>, name: String, instructions: String, include_global_memories: bool) -> Result<i64, String> {
    state.with_picky_undo("新建项目", |c| {
        let id = create_project(c, &name, &instructions)?;
        set_include_global(c, id, include_global_memories)?;
        Ok(id)
    })
}

#[tauri::command]
pub fn picky_project_update(state: State<AppState>, id: i64, name: String, instructions: String, include_global_memories: bool) -> Result<(), String> {
    state.with_picky_undo("修改项目", |c| {
        update_project(c, id, &name, &instructions)?;
        set_include_global(c, id, include_global_memories)
    })
}

#[tauri::command]
pub fn picky_project_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_picky_undo("删除项目", |c| delete_project(c, id))
}

#[tauri::command]
pub fn picky_projects_reorder(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    state.with_picky_undo("调整顺序", |c| reorder_projects(c, &ids))
}

/// `list`: "pinned" | "recent" | "archived" | "project:<id>".
#[tauri::command]
pub fn picky_drop(state: State<AppState>, id: i64, list: String, before: Option<i64>, after: Option<i64>) -> Result<bool, String> {
    let list = List::parse(&list)?;
    let label = match list {
        List::Pinned => "置顶",
        List::Archived => "归档",
        List::Project(_) => "移到项目",
        List::Recent => "调整顺序",
    };
    state.with_picky_undo(label, |c| drop(c, id, list, before, after))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::chat::begin_run;
    use crate::picky::store::tests::memory_db;

    fn new_session(c: &Connection, q: &str) -> i64 {
        let s = begin_run(c, None, None, q, "local:x", "X").unwrap().session_id;
        // distinct activity times, oldest first
        c.execute("UPDATE sessions SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?1) WHERE id = ?2", params![format!("+{s} seconds"), s]).unwrap();
        s
    }
    fn ids(v: &[SessionRow]) -> Vec<i64> {
        v.iter().map(|s| s.id).collect()
    }

    #[test]
    fn lists_group_and_follow_activity_until_arranged() {
        let c = memory_db();
        let (a, b, d) = (new_session(&c, "a"), new_session(&c, "b"), new_session(&c, "d"));
        let p = create_project(&c, "  ", "背景").unwrap();
        assert_eq!(library(&c).unwrap().projects[0].name, UNNAMED);
        move_to(&c, &[b], Some(p)).unwrap();
        set_pinned(&c, &[a], true).unwrap();
        let lib = library(&c).unwrap();
        assert_eq!((ids(&lib.pinned), ids(&lib.projects[0].sessions), ids(&lib.recent)), (vec![a], vec![b], vec![d]));

        // a pinned session keeps its project but shows only under 置顶
        move_to(&c, &[a], Some(p)).unwrap();
        let lib = library(&c).unwrap();
        assert_eq!((ids(&lib.pinned), ids(&lib.projects[0].sessions)), (vec![a], vec![b]));

        set_archived(&c, &[d], true).unwrap();
        assert_eq!(ids(&library(&c).unwrap().archived), [d]);
        assert!(library(&c).unwrap().recent.is_empty());
    }

    #[test]
    fn drops_reorder_and_move_between_lists() {
        let c = memory_db();
        let (a, b, d) = (new_session(&c, "a"), new_session(&c, "b"), new_session(&c, "d"));
        assert_eq!(ids(&library(&c).unwrap().recent), [d, b, a]); // newest activity first
        // back in place: nothing changes, the list stays automatic
        assert!(!drop(&c, b, List::Recent, Some(a), None).unwrap());
        assert!(manual(&c).unwrap().is_empty());
        // arranged by hand
        assert!(drop(&c, a, List::Recent, None, None).unwrap());
        assert_eq!(ids(&library(&c).unwrap().recent), [a, d, b]);
        // a new session arrives on top of an arranged list
        let e = new_session(&c, "e");
        assert_eq!(ids(&library(&c).unwrap().recent), [e, a, d, b]);

        // into a project: unpinned, moved in at the spot
        let p = create_project(&c, "论文", "").unwrap();
        set_pinned(&c, &[d], true).unwrap();
        assert!(drop(&c, d, List::Project(p), None, None).unwrap());
        let lib = library(&c).unwrap();
        assert!(lib.pinned.is_empty());
        assert_eq!(ids(&lib.projects[0].sessions), [d]);
        assert!(drop(&c, b, List::Project(p), None, Some(d)).unwrap());
        assert_eq!(ids(&library(&c).unwrap().projects[0].sessions), [d, b]);

        // to 置顶 keeps the project; to 最近 leaves it
        assert!(drop(&c, b, List::Pinned, None, None).unwrap());
        let r = row(&c, b).unwrap();
        assert_eq!((r.pinned, r.project_id), (true, Some(p)));
        assert!(drop(&c, b, List::Recent, None, None).unwrap());
        let r = row(&c, b).unwrap();
        assert_eq!((r.pinned, r.project_id), (false, None));
        // archive and back by drag
        assert!(drop(&c, a, List::Archived, None, None).unwrap());
        assert!(row(&c, a).unwrap().archived);
        assert!(drop(&c, a, List::Recent, None, None).unwrap());
        assert!(!row(&c, a).unwrap().archived);
        // a target outside the list is refused
        assert!(drop(&c, a, List::Recent, Some(d), None).is_err());
    }

    #[test]
    fn deleting_a_project_keeps_its_sessions_and_rename_reverts_to_auto() {
        let c = memory_db();
        let a = new_session(&c, "第一句问题");
        let p = create_project(&c, "项目", "说明").unwrap();
        move_to(&c, &[a], Some(p)).unwrap();
        delete_project(&c, p).unwrap();
        let lib = library(&c).unwrap();
        assert!(lib.projects.is_empty());
        assert_eq!(ids(&lib.recent), [a]);

        rename(&c, a, "  我的名字  ").unwrap();
        assert_eq!(library(&c).unwrap().recent[0].title, "我的名字");
        rename(&c, a, "").unwrap();
        assert_eq!(library(&c).unwrap().recent[0].title, "第一句问题");

        let q = create_project(&c, "Q", "").unwrap();
        let r = create_project(&c, "R", "").unwrap();
        assert_eq!(library(&c).unwrap().projects.iter().map(|p| p.id).collect::<Vec<_>>(), [r, q]); // newest first
        reorder_projects(&c, &[q, r]).unwrap();
        assert_eq!(library(&c).unwrap().projects[0].id, q);
        assert!(reorder_projects(&c, &[q]).is_err());
        delete(&c, &[a]).unwrap();
        assert!(library(&c).unwrap().recent.is_empty());
    }
}
