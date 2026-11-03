//! 速记小窗 (v0.8): ⌥Space anywhere (a global shortcut) shows a small window of Pomelo's own (label
//! `capture`, page capture.html) to jot a 念念不忘 note, a 一步一步 task, or ask 小拾. Rules:
//! - Same pomelo.db, undo history and menu bar as the main window: a saved note / task is one ⌘Z step
//!   (the main window's ⌘Z, or 撤销 right after saving); the main window is told (`capture-saved`), also
//!   when a 小拾 card is confirmed here (act/card.rs).
//! - 问小拾 chats right here, in a conversation of the window's own (picky.db preference `capture.chat`,
//!   library.rs): private and out of 小拾's list and searches, at most one; 新对话 deletes it, 「在 Pomelo 里
//!   继续对话」 makes it an ordinary one and opens it on the 小拾 page (event `capture-chat`). The page's
//!   model and memories; no attachments, 知识库 or page. The window grows for a conversation and goes
//!   back to its size after (`capture_fit`).
//! - The draft (`capture.draft`: tab, note text, task title / date / group, the question), the note's
//!   attachments (`capture.files`, ids of stored copies) and the pin (`capture.pin`) are settings: not
//!   undoable, not exported. 「清理没用到的附件」 keeps the draft's files (attachments.rs `USED`).
//! - The window is made on first use, then only hidden. Esc / ⌘W / its close button hide it (the page
//!   writes the draft first) and give the focus back to the app that was in front; losing the focus hides
//!   it too unless pinned. It floats on every desktop, also over full-screen apps.
//! - A tab whose module is hidden (念念不忘, 一步一步, 小拾) is not offered.

use crate::modules::common::{err, Tx};
use crate::modules::{attachments, notes, todo};
use crate::picky::library;
use crate::{settings, AppState};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "capture";
const DRAFT: &str = "capture.draft";
const FILES: &str = "capture.files";
const PIN: &str = "capture.pin";
const HIDDEN: &str = "ui.module_hidden";
/// Shown in the menu bar when ⌥Space could not be had (another app holds it); None: registered.
static SHORTCUT: Mutex<Option<String>> = Mutex::new(None);
/// The app in front before ⌥Space (its pid), to give the focus back on Esc.
static BEFORE: Mutex<Option<i32>> = Mutex::new(None);
/// The height before a conversation grew the window (to go back to).
static GROWN: Mutex<Option<f64>> = Mutex::new(None);
/// A conversation's height (logical px).
const CHAT_HEIGHT: f64 = 560.;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Draft {
    /// note | todo | ask
    pub tab: String,
    pub note: String,
    pub todo: String,
    /// None: today.
    pub due: Option<String>,
    /// The group last used (None: the first).
    pub group: Option<i64>,
    pub ask: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupName {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub draft: Draft,
    pub files: Vec<attachments::FileInfo>,
    pub pinned: bool,
    pub today: String,
    /// For `#` in the note.
    pub tags: Vec<String>,
    pub groups: Vec<GroupName>,
    /// Module ids hidden in the 模块坞.
    pub hidden: Vec<String>,
}

pub fn draft(conn: &Connection) -> Result<Draft, String> {
    Ok(settings::get(conn, DRAFT)?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

fn draft_files(conn: &Connection) -> Result<Vec<i64>, String> {
    Ok(settings::get(conn, FILES)?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

fn files_info(conn: &Connection) -> Result<Vec<attachments::FileInfo>, String> {
    // a copy cleaned up some other way drops out of the draft
    Ok(draft_files(conn)?.into_iter().filter_map(|id| attachments::info(conn, id).ok()).collect())
}

pub fn snapshot(conn: &Connection) -> Result<Snapshot, String> {
    let hidden = settings::get(conn, HIDDEN)?.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    Ok(Snapshot {
        draft: draft(conn)?,
        files: files_info(conn)?,
        pinned: settings::get(conn, PIN)?.and_then(|v| v.as_bool()).unwrap_or(false),
        today: todo::today(conn)?,
        tags: notes::overview(conn)?.tags.into_iter().map(|t| t.tag).collect(),
        groups: todo::groups(conn)?.into_iter().map(|g| GroupName { id: g.id, name: g.name }).collect(),
        hidden,
    })
}

/// The draft's attachments (stored copies, in order); returns them as the page shows them.
pub fn set_files(conn: &Connection, ids: &[i64]) -> Result<Vec<attachments::FileInfo>, String> {
    let ids = attachments::clean_ids(conn, ids)?;
    settings::set(conn, FILES, &json!(ids))?;
    files_info(conn)
}

/// Keeps only what changed in the draft: the page writes the text fields, Rust the attachments.
fn clear(conn: &Connection, f: impl FnOnce(&mut Draft)) -> Result<(), String> {
    let mut d = draft(conn)?;
    f(&mut d);
    settings::set(conn, DRAFT, &json!(d))
}

/// The note with the draft's attachments; the draft empties in the same transaction (or nothing changes).
pub fn save_note(conn: &Connection, content: &str) -> Result<notes::Note, String> {
    let tx = Tx::begin(conn)?;
    let files = draft_files(&tx)?;
    let note = notes::create(&tx, notes::NotePatch { content: Some(content.into()), attachments: Some(files), ..Default::default() })?;
    settings::set(&tx, FILES, &json!([]))?;
    clear(&tx, |d| d.note.clear())?;
    tx.commit()?;
    Ok(note)
}

/// A plain task due `due` (None: today) in `group`; the title empties, the group is remembered.
pub fn save_todo(conn: &Connection, title: &str, due: Option<&str>, group: i64) -> Result<todo::Task, String> {
    let tx = Tx::begin(conn)?;
    let due = match due {
        Some(d) => d.to_string(),
        None => todo::today(&tx)?,
    };
    let task = todo::create_task(&tx, todo::TaskPatch { group_id: Some(group), title: Some(title.into()), due_date: Some(Some(due)), ..Default::default() })?;
    clear(&tx, |d| {
        d.todo.clear();
        d.due = None;
        d.group = Some(group);
    })?;
    tx.commit()?;
    Ok(task)
}

// ---------- the window ----------

/// Shows the window (made on first use) on the screen under the pointer, focused.
pub fn show(app: &AppHandle) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(e) = show_now(&a) {
            eprintln!("[capture] {e}");
        }
    });
}

fn show_now(app: &AppHandle) -> Result<(), String> {
    remember_front();
    let (w, made) = match app.get_webview_window(LABEL) {
        Some(w) => (w, false),
        None => (make(app)?, true),
    };
    if made || !w.is_visible().map_err(err)? {
        center_on_pointer(&w)?;
    }
    w.show().map_err(err)?;
    w.set_focus().map_err(err)?;
    w.emit_to(LABEL, "capture-shown", ()).map_err(err)
}

fn make(app: &AppHandle) -> Result<WebviewWindow, String> {
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("capture.html".into()))
        .title("速记")
        .inner_size(520., 320.)
        .min_inner_size(420., 280.)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .maximizable(false)
        .minimizable(false)
        .visible(false)
        .build()
        .map_err(err)?;
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
        let ns = w.ns_window().map_err(err)?;
        // SAFETY: on the main thread (run_on_main_thread); the pointer is the window Tauri just made
        let ns: &NSWindow = unsafe { &*ns.cast() };
        ns.setCollectionBehavior(ns.collectionBehavior() | NSWindowCollectionBehavior::FullScreenAuxiliary);
    }
    Ok(w)
}

fn center_on_pointer(w: &WebviewWindow) -> Result<(), String> {
    let at = w.cursor_position().map_err(err)?;
    let Some(m) = w.monitor_from_point(at.x, at.y).map_err(err)? else { return Ok(()) };
    let area = m.work_area();
    let size = w.outer_size().map_err(err)?;
    let x = area.position.x + (area.size.width.saturating_sub(size.width) / 2) as i32;
    let y = area.position.y + (area.size.height.saturating_sub(size.height) / 3) as i32;
    w.set_position(tauri::PhysicalPosition::new(x, y)).map_err(err)
}

/// Hides the window; `refocus`: back to the app that was in front before ⌥Space (Esc, close), not
/// when the focus already went elsewhere (a click outside).
pub fn hide(app: &AppHandle, refocus: bool) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(LABEL) {
        w.hide().map_err(err)?;
    }
    let before = BEFORE.lock().map_err(err)?.take();
    if refocus {
        if let Some(pid) = before {
            give_back(pid);
        }
    }
    Ok(())
}

/// The page writes the draft, then calls `capture_hide` (close button, ⌘W).
pub fn request_close(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "capture-close", ());
}

#[cfg(target_os = "macos")]
fn remember_front() {
    use objc2_app_kit::NSWorkspace;
    let me = std::process::id() as i32;
    let front = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier()).filter(|p| *p != me && *p > 0);
    if let Ok(mut b) = BEFORE.lock() {
        // shown again while open: keep the app from the first time
        if front.is_some() || b.is_none() {
            *b = front;
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn remember_front() {}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn give_back(pid: i32) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    if let Some(a) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        a.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}
#[cfg(not(target_os = "macos"))]
fn give_back(_pid: i32) {}

// ---------- the shortcut ----------

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if shortcut.matches(Modifiers::ALT, Code::Space) && event.state() == ShortcutState::Pressed {
                show(app);
            }
        })
        .build()
}

/// ⌥Space for this run; when another app holds it the menu bar says so (速记 still opens from there).
pub fn register(app: &AppHandle) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
    let failed = app.global_shortcut().register(Shortcut::new(Some(Modifiers::ALT), Code::Space)).err().map(|e| e.to_string());
    if let Some(e) = &failed {
        eprintln!("[capture] ⌥Space: {e}");
    }
    *SHORTCUT.lock().unwrap_or_else(|e| e.into_inner()) = failed;
}

pub fn unregister(app: &AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let _ = app.global_shortcut().unregister_all();
}

/// ⌥Space works (for the menu bar's 速记 line).
pub fn shortcut_ok() -> bool {
    SHORTCUT.lock().map(|s| s.is_none()).unwrap_or(true)
}

// ---------- commands ----------

#[tauri::command]
pub fn capture_load(state: State<AppState>) -> Result<Snapshot, String> {
    state.with_db(|c| snapshot(c))
}

#[tauri::command]
pub fn capture_draft(state: State<AppState>, draft: Draft) -> Result<(), String> {
    state.with_db(|c| settings::set(c, DRAFT, &json!(draft)))
}

#[tauri::command]
pub fn capture_files(state: State<AppState>, ids: Vec<i64>) -> Result<Vec<attachments::FileInfo>, String> {
    state.with_db(|c| set_files(c, &ids))
}

#[tauri::command]
pub fn capture_pin(state: State<AppState>, on: bool) -> Result<(), String> {
    state.with_db(|c| settings::set(c, PIN, &json!(on)))
}

#[tauri::command]
pub fn capture_note(app: AppHandle, state: State<AppState>, content: String) -> Result<(), String> {
    state.with_undo("添加笔记", |c| save_note(c, &content))?;
    let _ = app.emit_to("main", "capture-saved", ());
    Ok(())
}

#[tauri::command]
pub fn capture_todo(app: AppHandle, state: State<AppState>, title: String, due: Option<String>, group: i64) -> Result<(), String> {
    state.with_undo("添加任务", |c| save_todo(c, &title, due.as_deref(), group))?;
    let _ = app.emit_to("main", "capture-saved", ());
    Ok(())
}

/// 撤销 right after saving: the last step (whatever it was), told to the main window too.
#[tauri::command]
pub fn capture_undo(app: AppHandle, state: State<AppState>) -> Result<crate::undo::UndoResult, String> {
    let r = crate::undo::step_back(&state, false)?;
    let _ = app.emit_to("main", "capture-saved", ());
    Ok(r)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: i64,
    pub thinking: bool,
    pub effort: Option<String>,
}

/// The window's conversation, if there is one.
#[tauri::command]
pub fn capture_chat(state: State<AppState>) -> Result<Option<Chat>, String> {
    state.picky.with_db(|c| {
        let Some(id) = library::capture_chat(c)? else { return Ok(None) };
        let (thinking, effort) = c.query_row("SELECT thinking, effort FROM sessions WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
        Ok(Some(Chat { id, thinking, effort }))
    })
}

/// Its 思考 switch (not undoable: it isn't in the list).
#[tauri::command]
pub fn capture_chat_thinking(state: State<AppState>, on: bool) -> Result<(), String> {
    state.picky.with_db(|c| match library::capture_chat(c)? {
        Some(id) => library::set_thinking(c, id, on),
        None => Ok(()),
    })
}

/// Its 思考强度.
#[tauri::command]
pub fn capture_chat_effort(state: State<AppState>, effort: Option<String>) -> Result<(), String> {
    state.picky.with_db(|c| match library::capture_chat(c)? {
        Some(id) => library::set_effort(c, id, effort.as_deref()),
        None => Ok(()),
    })
}

/// 新对话: the conversation goes.
#[tauri::command]
pub fn capture_chat_clear(state: State<AppState>) -> Result<(), String> {
    if state.picky.with_db(|c| library::capture_chat(c))?.is_some_and(|id| state.picky.runs.busy_in(id)) {
        return Err("小拾还在写，等它写完再开新对话。".into());
    }
    state.picky.with_db(|c| library::drop_capture_chat(c))
}

/// 「在 Pomelo 里继续对话」: an ordinary conversation now, opened on the 小拾 page; this window hides.
#[tauri::command]
pub fn capture_chat_keep(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let id = state.picky.with_db(|c| library::keep_capture_chat(c))?.ok_or("这个对话已经没有了")?;
    hide(&app, false)?;
    crate::tray::show_main(&app);
    app.emit_to("main", "capture-chat", id).map_err(err)
}

/// A record a reply links (or a card wrote) opens in the main window, as from 全局搜索 (`path` and the
/// hit); this window hides once the focus goes there, unless pinned.
#[tauri::command]
pub fn capture_open(app: AppHandle, path: String, hit: serde_json::Value) -> Result<(), String> {
    crate::tray::show_main(&app);
    app.emit_to("main", "capture-open", json!({ "path": path, "hit": hit })).map_err(err)
}

/// `chat`: a conversation is shown (the window grows to `CHAT_HEIGHT`); else back to the height before.
#[tauri::command]
pub fn capture_fit(app: AppHandle, chat: bool) -> Result<(), String> {
    let Some(w) = app.get_webview_window(LABEL) else { return Ok(()) };
    let scale = w.scale_factor().map_err(err)?;
    let inner = w.inner_size().map_err(err)?;
    let size = inner.to_logical::<f64>(scale);
    let mut grown = GROWN.lock().map_err(err)?;
    let height = match (chat, *grown) {
        (true, None) if size.height < CHAT_HEIGHT => {
            *grown = Some(size.height);
            CHAT_HEIGHT
        }
        (false, Some(h)) => {
            *grown = None;
            h
        }
        _ => return Ok(()),
    };
    // the top stays where it is (moved up when the bottom would leave the screen)
    let mut at = w.outer_position().map_err(err)?;
    let outer = w.outer_size().map_err(err)?.height as i32 - inner.height as i32 + (height * scale) as i32;
    if let Some(m) = w.current_monitor().map_err(err)? {
        let area = m.work_area();
        let bottom = area.position.y + area.size.height as i32;
        if at.y + outer > bottom {
            at.y = (bottom - outer).max(area.position.y);
        }
    }
    w.set_size(tauri::LogicalSize::new(size.width, height)).map_err(err)?;
    w.set_position(at).map_err(err)
}

#[tauri::command]
pub fn capture_hide(app: AppHandle, refocus: bool) -> Result<(), String> {
    hide(&app, refocus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::{memory_db, temp_dir};

    fn group(c: &Connection) -> i64 {
        todo::create_group(c, "收集箱").unwrap().id
    }

    #[test]
    fn note_takes_the_draft_files_and_empties_the_draft_at_once() {
        let c = memory_db();
        let dir = temp_dir("capture-note");
        let shot = attachments::store(&c, &attachments::folder_of(&dir), "截图.png", b"png bytes").unwrap();
        settings::set(&c, DRAFT, &json!(Draft { tab: "note".into(), note: "想到 #灵感".into(), todo: "买牛奶".into(), ..Default::default() })).unwrap();
        assert_eq!(set_files(&c, &[shot.id, shot.id]).unwrap().len(), 1);
        let n = save_note(&c, "想到 #灵感").unwrap();
        assert_eq!((n.tags.clone(), n.attachments.len()), (vec!["灵感".to_string()], 1));
        let d = draft(&c).unwrap();
        assert_eq!((d.note.as_str(), d.todo.as_str(), d.tab.as_str()), ("", "买牛奶", "note"));
        assert!(snapshot(&c).unwrap().files.is_empty());
        // the draft's copies count as used until saved
        let other = attachments::store(&c, &attachments::folder_of(&dir), "a.txt", b"draft").unwrap();
        set_files(&c, &[other.id]).unwrap();
        assert_eq!(attachments::clean(&c, &attachments::folder_of(&dir)).unwrap(), 0);
        set_files(&c, &[]).unwrap();
        assert_eq!(attachments::clean(&c, &attachments::folder_of(&dir)).unwrap(), 1);
    }

    #[test]
    fn a_failed_note_keeps_the_draft() {
        let c = memory_db();
        settings::set(&c, DRAFT, &json!(Draft { note: "keep".into(), ..Default::default() })).unwrap();
        assert!(save_note(&c, " \n ").is_err());
        assert!(save_note(&c, &"x".repeat(notes::MAX_CHARS + 1)).is_err());
        assert_eq!(draft(&c).unwrap().note, "keep");
        assert_eq!(notes::count(&c).unwrap(), 0);
        assert!(set_files(&c, &[999]).is_err());
    }

    #[test]
    fn todo_is_due_today_unless_given_and_remembers_the_group() {
        let c = memory_db();
        let g = group(&c);
        settings::set(&c, DRAFT, &json!(Draft { todo: "交房租".into(), due: Some("2026-11-03".into()), note: "留着".into(), ..Default::default() })).unwrap();
        let t = save_todo(&c, "交房租", None, g).unwrap();
        assert_eq!((t.due_date.clone(), t.group_id), (Some(todo::today(&c).unwrap()), g));
        let d = draft(&c).unwrap();
        assert_eq!((d.todo.as_str(), d.due.clone(), d.group, d.note.as_str()), ("", None, Some(g), "留着"));
        assert_eq!(save_todo(&c, "体检", Some("2026-12-01"), g).unwrap().due_date.as_deref(), Some("2026-12-01"));
        assert!(save_todo(&c, "  ", None, g).is_err());
        assert!(save_todo(&c, "x", None, g + 99).is_err());
        assert_eq!(draft(&c).unwrap().group, Some(g));
    }

    #[test]
    fn saving_is_one_undo_step_that_leaves_the_draft_alone() {
        let c = memory_db();
        let mut h = crate::undo::History::default();
        crate::undo::tests::step(&c, &mut h, "添加笔记", |c| save_note(c, "第一条").map(|_| ()));
        settings::set(&c, DRAFT, &json!(Draft { note: "下一条".into(), ..Default::default() })).unwrap();
        crate::undo::undo(&c, &mut h).unwrap();
        assert_eq!(notes::count(&c).unwrap(), 0);
        assert_eq!(draft(&c).unwrap().note, "下一条");
        crate::undo::redo(&c, &mut h).unwrap();
        assert_eq!(notes::count(&c).unwrap(), 1);
    }

    #[test]
    fn snapshot_reads_an_old_or_odd_draft_as_empty() {
        let c = memory_db();
        settings::set(&c, DRAFT, &json!("旧格式")).unwrap();
        settings::set(&c, HIDDEN, &json!(["picky"])).unwrap();
        group(&c);
        let s = snapshot(&c).unwrap();
        assert_eq!((s.draft, s.pinned, s.hidden, s.groups.len()), (Draft::default(), false, vec!["picky".to_string()], 1));
    }
}
