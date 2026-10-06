//! Undo / redo (⌘Z / ⌘⇧Z) for UI edits. Each mutating command runs inside a SQLite session
//! (`record`); the resulting changeset (all tables except `settings`, FK cascades included) goes
//! on an in-memory stack. Undo applies its inverse, redo re-applies it. Stacks live only while the
//! app runs, and import / restore clear them (their changesets would no longer match the data).

use crate::modules::common::err;
use crate::AppState;
use rusqlite::session::{invert_strm, ConflictAction, ConflictType, Session};
use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

const LIMIT: usize = 100;
/// Not user data (UI state like the last page; 芝麻开门 unlock state); never undone.
const SKIP: &[&str] = &["settings", "vault_meta"];

#[derive(Debug, Clone)]
struct Entry {
    label: String,
    /// Forward changeset bytes (what the command did).
    forward: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

impl History {
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn push(&mut self, label: &str, forward: Vec<u8>) {
        self.undo.push(Entry { label: label.into(), forward });
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
}

/// Runs `f`, recording its changes; returns the changeset bytes if anything changed.
pub fn record<T>(conn: &Connection, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<(T, Option<Vec<u8>>), String> {
    let mut s = Session::new(conn).map_err(err)?;
    s.table_filter(Some(|t: &str| !SKIP.contains(&t)));
    s.attach(None::<&str>).map_err(err)?;
    let out = f(conn)?;
    if s.is_empty() {
        return Ok((out, None));
    }
    let mut bytes = Vec::new();
    s.changeset_strm(&mut bytes).map_err(err)?;
    Ok((out, Some(bytes)))
}

/// The changeset already holds every cascaded change, so FK actions are switched off while it is
/// replayed (otherwise a replayed delete would cascade again and collide with its own rows).
/// Any conflict means the data moved on since the step was recorded → abort (all-or-nothing).
fn apply(conn: &Connection, changeset: &[u8]) -> Result<(), String> {
    conn.execute_batch("PRAGMA foreign_keys = OFF").map_err(err)?;
    let mut input: &[u8] = changeset;
    let r = conn.apply_strm(&mut input, None::<fn(&str) -> bool>, |_: ConflictType, _| ConflictAction::SQLITE_CHANGESET_ABORT);
    conn.execute_batch("PRAGMA foreign_keys = ON").map_err(err)?;
    r.map_err(|_| "数据已经变了，没法撤销这一步".to_string())
}

fn invert(changeset: &[u8]) -> Result<Vec<u8>, String> {
    let mut input: &[u8] = changeset;
    let mut out = Vec::new();
    invert_strm(&mut input, &mut out).map_err(err)?;
    Ok(out)
}

/// Returns the undone step's label, or None if there is nothing to undo.
pub fn undo(conn: &Connection, h: &mut History) -> Result<Option<String>, String> {
    let Some(e) = h.undo.pop() else { return Ok(None) };
    if let Err(msg) = invert(&e.forward).and_then(|inv| apply(conn, &inv)) {
        h.clear();
        return Err(msg);
    }
    let label = e.label.clone();
    h.redo.push(e);
    Ok(Some(label))
}

pub fn redo(conn: &Connection, h: &mut History) -> Result<Option<String>, String> {
    let Some(e) = h.redo.pop() else { return Ok(None) };
    if let Err(msg) = apply(conn, &e.forward) {
        h.clear();
        return Err(msg.replace("撤销", "重做"));
    }
    let label = e.label.clone();
    h.undo.push(e);
    Ok(Some(label))
}

impl AppState {
    /// `with_db` for edits the user can undo; `label` names the step (「删除任务」).
    pub(crate) fn with_undo<T>(&self, label: &str, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        self.with_db(|c| {
            let (out, cs) = record(c, f)?;
            if let Some(bytes) = cs {
                self.history.lock().map_err(|_| "撤销记录异常，请重启 App".to_string())?.push(label, bytes);
            }
            Ok(out)
        })
    }

    pub(crate) fn clear_history(&self) {
        if let Ok(mut h) = self.history.lock() {
            h.clear();
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoResult {
    /// None: nothing to undo / redo.
    pub label: Option<String>,
}

#[tauri::command]
pub fn undo_last(state: State<AppState>) -> Result<UndoResult, String> {
    state.with_db(|c| {
        let mut h = state.history.lock().map_err(|_| "撤销记录异常，请重启 App".to_string())?;
        Ok(UndoResult { label: undo(c, &mut h)? })
    })
}

#[tauri::command]
pub fn redo_last(state: State<AppState>) -> Result<UndoResult, String> {
    state.with_db(|c| {
        let mut h = state.history.lock().map_err(|_| "撤销记录异常，请重启 App".to_string())?;
        Ok(UndoResult { label: redo(c, &mut h)? })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::modules::todo;
    use serde_json::json;

    fn step(c: &Connection, h: &mut History, label: &str, f: impl FnOnce(&Connection) -> Result<(), String>) {
        let ((), cs) = record(c, f).unwrap();
        if let Some(b) = cs {
            h.push(label, b);
        }
    }
    fn n(c: &Connection, table: &str) -> i64 {
        c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn undo_restores_cascaded_deletes_and_redo_repeats() {
        let c = memory_db();
        let mut h = History::default();
        let g = todo::create_group(&c, "A").unwrap().id;
        let root = todo::create_task(&c, serde_json::from_value(json!({"groupId": g, "title": "root"})).unwrap()).unwrap().id;
        todo::create_task(&c, serde_json::from_value(json!({"parentId": root, "title": "kid"})).unwrap()).unwrap();
        crate::settings::set(&c, "ui.last_route", &json!("/todo")).unwrap();

        step(&c, &mut h, "删除分组", |c| todo::delete_group(c, g));
        assert_eq!((n(&c, "todo_groups"), n(&c, "todo_tasks")), (0, 0));
        assert_eq!(undo(&c, &mut h).unwrap().as_deref(), Some("删除分组"));
        assert_eq!((n(&c, "todo_groups"), n(&c, "todo_tasks")), (1, 2));
        let kid_parent: i64 = c.query_row("SELECT parent_id FROM todo_tasks WHERE title = 'kid'", [], |r| r.get(0)).unwrap();
        assert_eq!(kid_parent, root); // same ids come back

        assert_eq!(redo(&c, &mut h).unwrap().as_deref(), Some("删除分组"));
        assert_eq!(n(&c, "todo_tasks"), 0);
        assert_eq!(undo(&c, &mut h).unwrap().as_deref(), Some("删除分组"));
        assert_eq!(undo(&c, &mut h).unwrap(), None);
    }

    #[test]
    fn set_null_cascade_round_trips() {
        let c = memory_db();
        let mut h = History::default();
        let comic = crate::modules::kinds::tests::kind_id(&c, "books", "漫画");
        c.execute("INSERT INTO books (title, kind_id) VALUES ('海贼王', ?1)", [comic]).unwrap();
        let kind_of = |c: &Connection| -> Option<i64> { c.query_row("SELECT kind_id FROM books", [], |r| r.get(0)).unwrap() };
        step(&c, &mut h, "删除类型", |c| crate::modules::kinds::delete(c, comic));
        assert_eq!(kind_of(&c), None);
        undo(&c, &mut h).unwrap();
        assert_eq!(kind_of(&c), Some(comic));
        redo(&c, &mut h).unwrap();
        assert_eq!(kind_of(&c), None);
        undo(&c, &mut h).unwrap();
        assert_eq!(kind_of(&c), Some(comic));
        let fk: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1); // switched back on
    }

    #[test]
    fn undo_steps_in_order_and_skips_settings() {
        let c = memory_db();
        let mut h = History::default();
        let g = todo::create_group(&c, "A").unwrap().id;
        let t = todo::create_task(&c, serde_json::from_value(json!({"groupId": g, "title": "t"})).unwrap()).unwrap().id;
        step(&c, &mut h, "完成任务", |c| todo::set_done(c, t, true));
        step(&c, &mut h, "改名", |c| todo::update_task(c, t, serde_json::from_value(json!({"title": "t2"})).unwrap()).map(|_| ()));
        step(&c, &mut h, "设置", |c| crate::settings::set(c, "ui.last_route", &json!("/books")));
        assert_eq!(h.undo.len(), 2); // settings-only change records nothing

        assert_eq!(undo(&c, &mut h).unwrap().as_deref(), Some("改名"));
        assert_eq!(todo::task(&c, t).unwrap().title, "t");
        assert!(todo::task(&c, t).unwrap().done);
        assert_eq!(undo(&c, &mut h).unwrap().as_deref(), Some("完成任务"));
        assert!(!todo::task(&c, t).unwrap().done);
        // a new edit clears redo
        step(&c, &mut h, "新增", |c| todo::create_task(c, serde_json::from_value(json!({"groupId": g, "title": "x"})).unwrap()).map(|_| ()));
        assert_eq!(redo(&c, &mut h).unwrap(), None);
    }

    /// Notes: derived tags and links in both directions come back with the note.
    #[test]
    fn notes_delete_and_tag_rename_undo() {
        use crate::modules::notes;
        let c = memory_db();
        let mut h = History::default();
        let np = |v| serde_json::from_value::<notes::NotePatch>(v).unwrap();
        let a = notes::create(&c, np(json!({"content": "#读书 a"}))).unwrap().id;
        let b = notes::create(&c, np(json!({"content": "#读书/摘抄 b", "refs": [a]}))).unwrap().id;
        let q = notes::create(&c, np(json!({"content": "q", "refs": [b]}))).unwrap().id;
        step(&c, &mut h, "删除笔记", |c| notes::delete(c, b));
        assert_eq!((n(&c, "notes"), n(&c, "note_tags"), n(&c, "note_refs")), (2, 1, 0));
        undo(&c, &mut h).unwrap();
        assert_eq!((n(&c, "notes"), n(&c, "note_tags"), n(&c, "note_refs")), (3, 2, 2));
        assert_eq!(notes::get(&c, q).unwrap().refs[0].id, b);
        step(&c, &mut h, "标签改名", |c| notes::rename_tag(c, "读书", "阅读").map(|_| ()));
        assert_eq!(notes::get(&c, b).unwrap().content, "#阅读/摘抄 b");
        undo(&c, &mut h).unwrap();
        assert_eq!(notes::get(&c, b).unwrap().content, "#读书/摘抄 b");
        let tags: Vec<String> = c.prepare("SELECT tag FROM note_tags ORDER BY tag").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(tags, ["读书", "读书/摘抄"]);
    }

    #[test]
    fn stale_step_is_refused_and_history_cleared() {
        let c = memory_db();
        let mut h = History::default();
        let g = todo::create_group(&c, "A").unwrap().id;
        step(&c, &mut h, "改名", |c| todo::rename_group(c, g, "B").map(|_| ()));
        todo::rename_group(&c, g, "C").unwrap(); // not recorded (e.g. import)
        assert!(undo(&c, &mut h).is_err());
        assert_eq!(todo::group(&c, g).unwrap().name, "C");
        assert!(h.undo.is_empty() && h.redo.is_empty());
    }
}
