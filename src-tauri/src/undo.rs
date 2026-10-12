//! Undo / redo (⌘Z / ⌘⇧Z) for UI edits. Each mutating command runs inside a SQLite session
//! (`record`); the resulting changeset (all tables except `settings`, FK cascades included) goes
//! on an in-memory stack. Undo applies its inverse, redo re-applies it. Stacks live only while the
//! app runs, and import / restore clear them (their changesets would no longer match the data).
//! Two databases: each step remembers its store (pomelo.db or 小拾's picky.db) and is replayed there.

use crate::backup::Store;
use crate::modules::common::err;
use crate::AppState;
use rusqlite::session::{invert_strm, ConflictAction, ConflictType, Session};
use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

const LIMIT: usize = 100;
/// Not user data (UI state like the last page; 芝麻开门 unlock state; 小拾 settings; 小拾 file indexes,
/// and memory vectors, which are derived and large); never undone.
const SKIP: &[&str] = &["settings", "vault_meta", "preferences", "artifact_indexes", "artifact_chunks", "memory_vectors"];

#[derive(Debug, Clone)]
struct Entry {
    store: Store,
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

    /// The store the next undo (or redo) step belongs to.
    fn next_store(&self, redo: bool) -> Option<Store> {
        (if redo { &self.redo } else { &self.undo }).last().map(|e| e.store)
    }

    fn push(&mut self, store: Store, label: &str, forward: Vec<u8>) {
        self.undo.push(Entry { store, label: label.into(), forward });
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
        self.with_undo_in(Store::Pomelo, label, f)
    }

    /// Same for 小拾's picky.db.
    pub(crate) fn with_picky_undo<T>(&self, label: &str, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        self.with_undo_in(Store::Picky, label, f)
    }

    fn with_undo_in<T>(&self, store: Store, label: &str, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        self.with_store(store, |c| {
            let (out, cs) = record(c, f)?;
            if let Some(bytes) = cs {
                self.history.lock().map_err(|_| "撤销记录异常，请重启 App".to_string())?.push(store, label, bytes);
            }
            Ok(out)
        })
    }

    pub(crate) fn with_store<T>(&self, store: Store, f: impl FnOnce(&mut Connection) -> Result<T, String>) -> Result<T, String> {
        match store {
            Store::Pomelo => self.with_db(f),
            Store::Picky => self.picky.with_db(f),
        }
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

/// Replays the next step on its own store. Lock order is always database → history (as in
/// `with_undo_in`), so the store is looked up first and checked again under both locks.
fn step_back(state: &AppState, redo_it: bool) -> Result<UndoResult, String> {
    let busy = || "撤销记录异常，请重启 App".to_string();
    let Some(store) = state.history.lock().map_err(|_| busy())?.next_store(redo_it) else {
        return Ok(UndoResult { label: None });
    };
    state.with_store(store, |c| {
        let mut h = state.history.lock().map_err(|_| busy())?;
        if h.next_store(redo_it) != Some(store) {
            return Err("撤销记录刚刚变了，请再试一次".into());
        }
        Ok(UndoResult { label: if redo_it { redo(c, &mut h)? } else { undo(c, &mut h)? } })
    })
}

#[tauri::command]
pub fn undo_last(state: State<AppState>) -> Result<UndoResult, String> {
    step_back(&state, false)
}

#[tauri::command]
pub fn redo_last(state: State<AppState>) -> Result<UndoResult, String> {
    step_back(&state, true)
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
            h.push(Store::Pomelo, label, b);
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

    /// 小拾 steps carry their store and replay on picky.db (cascades included; preferences skipped).
    #[test]
    fn picky_steps_replay_on_picky_db() {
        use crate::picky::{chat, store};
        let c = store::tests::memory_db();
        let mut h = History::default();
        let b = chat::begin_run(&c, None, None, "q", "local:x", "X").unwrap();
        let ((), cs) = record(&c, |c| {
            store::pref_set(c, "model.selection", "cloud:x")?;
            c.execute("DELETE FROM sessions", []).map(|_| ()).map_err(err)
        })
        .unwrap();
        h.push(Store::Picky, "删除对话", cs.unwrap());
        assert_eq!(h.next_store(false), Some(Store::Picky));
        assert_eq!(n(&c, "messages"), 0);
        assert_eq!(undo(&c, &mut h).unwrap().as_deref(), Some("删除对话"));
        assert_eq!(chat::path(&c, b.session_id).unwrap().len(), 2);
        assert_eq!(store::pref_get::<String>(&c, "model.selection").unwrap().as_deref(), Some("cloud:x")); // not undone
        assert_eq!(h.next_store(true), Some(Store::Picky));
    }

    /// 小拾's list: a drop (membership + hand order) and deleting a project come back whole.
    #[test]
    fn picky_list_changes_undo() {
        use crate::picky::{chat, library, store};
        let c = store::tests::memory_db();
        let mut h = History::default();
        let a = chat::begin_run(&c, None, None, "a", "local:x", "X").unwrap().session_id;
        let b = chat::begin_run(&c, None, None, "b", "local:x", "X").unwrap().session_id;
        let p = library::create_project(&c, "P", "").unwrap();
        let snapshot = |c: &Connection| serde_json::to_string(&library::library(c).unwrap()).unwrap();
        let before = snapshot(&c);
        let step = |c: &Connection, h: &mut History, label: &str, f: &dyn Fn(&Connection) -> Result<(), String>| {
            let ((), cs) = record(c, f).unwrap();
            h.push(Store::Picky, label, cs.unwrap());
        };
        step(&c, &mut h, "移到项目", &|c| library::drop(c, a, library::List::Project(p), None, None).map(|_| ()));
        step(&c, &mut h, "删除项目", &|c| library::delete_project(c, p));
        step(&c, &mut h, "调整顺序", &|c| library::drop(c, b, library::List::Recent, None, Some(a)).map(|_| ()));
        assert_ne!(snapshot(&c), before);
        for _ in 0..3 {
            undo(&c, &mut h).unwrap();
        }
        assert_eq!(snapshot(&c), before);
        assert_eq!(n(&c, "manual_lists"), 0);
    }

    /// 小拾's project files: add / replace / remove come back; indexes (derived) are never recorded.
    #[test]
    fn picky_project_file_changes_undo() {
        use crate::picky::files::{self, tests::add};
        use crate::picky::{index, library, store};
        let c = store::tests::memory_db();
        let dir = crate::db::tests::temp_dir("undo-project-files");
        let mut h = History::default();
        let p = library::create_project(&c, "P", "").unwrap();
        let v1 = add(&c, &dir, "a.md", b"one");
        let v2 = add(&c, &dir, "a.md", b"two");
        let step = |c: &Connection, h: &mut History, f: &dyn Fn(&Connection) -> Result<(), String>| {
            let ((), cs) = record(c, f).unwrap();
            h.push(Store::Picky, "资料", cs.unwrap());
        };
        step(&c, &mut h, &|c| files::add_project_file(c, p, v1).map(|_| ()));
        let f = files::project_files(&c, Some(p)).unwrap()[0].id;
        step(&c, &mut h, &|c| files::set_indexed(c, f, true));
        index::save(&c, v1, "m", &[index::Chunk { segment: 0, content: "one".into(), vector: vec![1.0] }]).unwrap();
        step(&c, &mut h, &|c| files::replace_project_file(c, f, v2).map(|_| ()));
        step(&c, &mut h, &|c| files::remove_project_file(c, f));
        assert!(files::project_files(&c, Some(p)).unwrap().is_empty());
        undo(&c, &mut h).unwrap();
        assert_eq!(files::project_file(&c, f).unwrap().current_version, 2);
        undo(&c, &mut h).unwrap();
        let back = files::project_file(&c, f).unwrap();
        assert_eq!((back.current_version, back.versions.len(), back.indexed), (1, 1, true));
        assert!(index::indexed(&c, "m").unwrap().is_empty()); // the index stays gone: rebuild it
        undo(&c, &mut h).unwrap();
        undo(&c, &mut h).unwrap();
        assert!(files::project_files(&c, Some(p)).unwrap().is_empty());
    }

    /// 小拾's memories: edits come back, and deleting a project brings its memories back with it.
    #[test]
    fn picky_memory_changes_undo() {
        use crate::picky::memory::{self, MemoryInput};
        use crate::picky::{library, store};
        let c = store::tests::memory_db();
        let mut h = History::default();
        let p = library::create_project(&c, "P", "").unwrap();
        let m = memory::create(&c, &MemoryInput { content: "论文用 APA".into(), project_id: Some(p), enabled: true, ..Default::default() }, "manual", None).unwrap();
        let snapshot = |c: &Connection| serde_json::to_string(&memory::list(c).unwrap()).unwrap();
        let before = snapshot(&c);
        let step = |c: &Connection, h: &mut History, f: &dyn Fn(&Connection) -> Result<(), String>| {
            let ((), cs) = record(c, f).unwrap();
            h.push(Store::Picky, "记忆", cs.unwrap());
        };
        step(&c, &mut h, &|c| memory::set_flag(c, m, "localOnly", true));
        step(&c, &mut h, &|c| library::delete_project(c, p));
        assert_eq!(memory::list(&c).unwrap()[0].project_id, None);
        step(&c, &mut h, &|c| memory::delete(c, m));
        for _ in 0..3 {
            undo(&c, &mut h).unwrap();
        }
        assert_eq!(snapshot(&c), before);
    }

    /// 小拾's templates, presets and instructions: deleting a preset in use comes back with its sessions.
    #[test]
    fn picky_instruction_changes_undo() {
        use crate::picky::{chat, instructions as ins, store, templates as tpl};
        let c = store::tests::memory_db();
        let mut h = History::default();
        let s = chat::begin_run(&c, None, None, "a", "local:x", "X").unwrap().session_id;
        let preset = ins::create_preset(&c, "老师", "像老师").unwrap();
        let snapshot = |c: &Connection| {
            serde_json::to_string(&(tpl::list(c).unwrap(), ins::presets(c).unwrap(), ins::global(c).unwrap(), ins::session_setting(c, s).unwrap())).unwrap()
        };
        let before = snapshot(&c);
        let step = |c: &Connection, h: &mut History, label: &str, f: &dyn Fn(&Connection) -> Result<(), String>| {
            let ((), cs) = record(c, f).unwrap();
            h.push(Store::Picky, label, cs.unwrap());
        };
        let t = tpl::TemplateInput { title: "润色".into(), category: "写作".into(), body: "{{原文}}".into() };
        step(&c, &mut h, "新建模板", &|c| tpl::create(c, &t).and_then(|id| tpl::set_pinned(c, id, true)));
        step(&c, &mut h, "修改全局指令", &|c| ins::set_global(c, "用中文"));
        step(&c, &mut h, "修改会话指令", &|c| ins::set_session(c, s, &ins::SessionSetting { content: "简短".into(), preset_id: Some(preset) }));
        step(&c, &mut h, "删除预设", &|c| ins::delete_preset(c, preset));
        assert_eq!(ins::session_setting(&c, s).unwrap().preset_id, None);
        undo(&c, &mut h).unwrap();
        assert_eq!(ins::session_setting(&c, s).unwrap().preset_id, Some(preset)); // the FK's unset comes back too
        for _ in 0..3 {
            undo(&c, &mut h).unwrap();
        }
        assert_eq!(snapshot(&c), before);
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
