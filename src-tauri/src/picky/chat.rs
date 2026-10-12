//! Conversations (projects / attachments come in later phases):
//! - A session's displayed path starts at `active_root_id` and follows each message's
//!   `active_child_id`; versions are siblings. Damaged links fall back to the newest child.
//! - Sending appends a question and an empty streaming reply to the end of that path in one
//!   transaction (a new session is created in the same step: drafts never leave empty sessions),
//!   then one background task streams the reply: deltas go to the page, the text is checkpointed
//!   every 0.75 s, and the run is finished exactly once (completed / failed / cancelled /
//!   interrupted on quit). One reply at a time (a local server has a single slot).
//! - Versions (phase 2): editing a question adds a sibling question + reply; regenerate / retry add
//!   a sibling reply under the same question. Each version keeps its own continuation; switching
//!   just repoints the path (not undoable: it is navigation). A version's request only carries the
//!   question's ancestors.
//! - Title = the first non-empty line of the first question (40 chars) unless the user renamed it
//!   (editing the first question re-titles an auto title).
//! - What the model gets (phase 4, context.rs): the composed instructions, the applicable summary,
//!   the history after it and the question, fitted into the model's window when the reply starts.
//!   A new session saves the draft's own instructions / preset with its first question.
//! - Files (phase 5): a question keeps its attachments and the project files (versions) it used;
//!   an edited question keeps them too. The material goes with the question, or is read in
//!   segments when it doesn't fit (reading.rs). Retrieved passages are kept with the run.
//! - Memories (phase 6) chosen before the question is saved go with the run (memory.rs).
//! - Looking things up in Pomelo (v0.3.5, data.rs): a reply may call the modules' read-only tools
//!   for up to `data::MAX_ROUNDS` rounds, then must answer. Room is kept for the definitions and the
//!   results (cut to fit); each lookup is saved with the run (run_queries) and shown as it happens.
//!   A service that refuses tools gets the question again without them (one 「不支持查数据」 entry).
//!   Not with material read in segments.
//! - Sending / generating / switching are not undoable. No branch changes while a reply or a
//!   summary is written (they share one slot).

use super::context::{self, Counter, Request, Sizes, SummaryJob, Turn};
use super::data::{self, Query};
use super::files::{self, MessageFile};
use super::index::Passage;
use super::memory::{self, UsedMemory};
use super::reading::{self, Reader};
use super::instructions::{self, SessionSetting};
use super::provider::{self, Event, Msg, Options, Target, ToolCall};
use super::store::NOW;
use super::Route;
use crate::modules::common::{err, Tx};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

pub const NEW_TITLE: &str = "新对话";
const CHECKPOINT: Duration = Duration::from_millis(750);

pub fn auto_title(prompt: &str) -> String {
    let line = prompt.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).find(|l| !l.is_empty()).unwrap_or_default();
    if line.is_empty() {
        NEW_TITLE.into()
    } else if line.chars().count() > 40 {
        format!("{}…", line.chars().take(40).collect::<String>())
    } else {
        line
    }
}

// ---------- reads ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: i64,
    pub title: String,
    pub updated_at: String,
}

pub fn sessions(conn: &Connection) -> Result<Vec<Session>, String> {
    let mut st = conn.prepare("SELECT id, title, updated_at FROM sessions ORDER BY updated_at DESC, id DESC").map_err(err)?;
    let rows = st.query_map([], |r| Ok(Session { id: r.get(0)?, title: r.get(1)?, updated_at: r.get(2)? })).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub reasoning: String,
    pub reasoning_ms: Option<i64>,
    /// complete | streaming | cancelled | failed | interrupted
    pub state: String,
    pub created_at: String,
    /// Replies: the model's name at the time.
    pub model: Option<String>,
    /// Failed replies: why (user-facing).
    pub error: Option<String>,
    /// 1-based position among its versions, and how many there are.
    pub version_index: i64,
    pub version_count: i64,
    /// Questions: attachments, then project files.
    pub files: Vec<MessageFile>,
    /// Replies: notes from reading material in segments.
    pub notes: Vec<Note>,
    /// Replies: the memories their run was given.
    pub memories: Vec<UsedMemory>,
    /// Replies: what their run looked up in Pomelo.
    pub queries: Vec<Query>,
    #[serde(skip)]
    active_child: Option<i64>,
    #[serde(skip)]
    parent: Option<i64>,
    #[serde(skip)]
    run_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Note {
    /// 文件名 · 第 3 页
    pub label: String,
    pub content: String,
}

const MESSAGE_COLS: &str = "m.id, m.role, m.content, m.reasoning, m.reasoning_ms, m.state, m.created_at, \
     CASE WHEN m.role = 'assistant' THEN r.model_name END, CASE WHEN m.role = 'assistant' THEN r.error END, m.active_child_id, m.parent_id, m.run_id";

fn message_row(r: &rusqlite::Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: r.get(0)?,
        role: r.get(1)?,
        content: r.get(2)?,
        reasoning: r.get(3)?,
        reasoning_ms: r.get(4)?,
        state: r.get(5)?,
        created_at: r.get(6)?,
        model: r.get(7)?,
        error: r.get(8)?,
        active_child: r.get(9)?,
        parent: r.get(10)?,
        run_id: r.get(11)?,
        version_index: 1,
        version_count: 1,
        files: vec![],
        notes: vec![],
        memories: vec![],
        queries: vec![],
    })
}

/// The displayed path of a session, oldest first.
pub fn path(conn: &Connection, session_id: i64) -> Result<Vec<Message>, String> {
    let root: Option<Option<i64>> = conn.query_row("SELECT active_root_id FROM sessions WHERE id = ?1", [session_id], |r| r.get(0)).optional().map_err(err)?;
    let Some(root) = root else { return Err("找不到这个对话，可能已经被删除了".into()) };
    let one = |id: i64| -> Result<Option<Message>, String> {
        conn.query_row(&format!("SELECT {MESSAGE_COLS} FROM messages m LEFT JOIN runs r ON r.id = m.run_id WHERE m.id = ?1 AND m.session_id = ?2"), [id, session_id], message_row)
            .optional()
            .map_err(err)
    };
    // newest child of `parent` (NULL = roots): the fallback when a link is missing or broken
    let newest = |parent: Option<i64>| -> Result<Option<i64>, String> {
        conn.query_row(
            "SELECT id FROM messages WHERE session_id = ?1 AND parent_id IS ?2 ORDER BY created_at DESC, id DESC LIMIT 1",
            params![session_id, parent],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)
    };
    let mut out: Vec<Message> = vec![];
    let mut seen = HashSet::new();
    let mut next = match root {
        Some(id) if one(id)?.is_some_and(|m| m.parent.is_none()) => Some(id),
        _ => newest(None)?,
    };
    while let Some(id) = next {
        if !seen.insert(id) {
            break; // a cycle: stop rather than loop
        }
        let Some(m) = one(id)? else { break };
        let parent = Some(m.id);
        next = match m.active_child {
            Some(c) if conn.query_row("SELECT 1 FROM messages WHERE id = ?1 AND parent_id = ?2", [c, m.id], |_| Ok(())).optional().map_err(err)?.is_some() => Some(c),
            _ => newest(parent)?,
        };
        out.push(m);
    }
    for m in out.iter_mut() {
        let list = siblings(conn, session_id, m.parent, &m.role)?;
        m.version_count = list.len().max(1) as i64;
        m.version_index = list.iter().position(|x| *x == m.id).map_or(1, |i| i as i64 + 1);
        if m.role == "user" {
            m.files = files::message_files(conn, m.id)?;
        } else if let Some(run) = m.run_id {
            m.notes = reading::notes_of(conn, run)?.into_iter().map(|(label, content)| Note { label, content }).collect();
            m.memories = memory::used_of(conn, run)?;
            m.queries = data::queries_of(conn, run)?;
        }
    }
    Ok(out)
}

// ---------- writes ----------

#[derive(Debug, Clone)]
pub struct Begun {
    pub session_id: i64,
    pub run_id: i64,
    pub user_id: i64,
    pub reply_id: i64,
    /// What the model gets, before fitting into its window.
    pub request: Request,
}

/// What a question brings besides its text: attachments (stored copies), project files (ids; their
/// current versions are recorded) and passages retrieved for it.
#[derive(Debug, Clone, Default)]
pub struct Extras {
    pub attachments: Vec<i64>,
    pub files: Vec<i64>,
    pub passages: Vec<Passage>,
    /// Memories chosen for it (memory.rs).
    pub memories: Vec<UsedMemory>,
}

/// Where a new session goes and the draft's own instructions.
#[derive(Debug, Clone, Default)]
pub struct NewSession {
    pub project: Option<i64>,
    pub setting: SessionSetting,
    /// The draft's 「本会话关闭记忆」.
    pub memory_off: bool,
}

/// `project`: where a new session (`session` None) goes; ignored for an existing one.
#[cfg(test)]
pub fn begin_run(conn: &Connection, session: Option<i64>, project: Option<i64>, prompt: &str, model: &str, model_name: &str) -> Result<Begun, String> {
    begin_run_in(conn, session, &NewSession { project, ..Default::default() }, prompt, model, model_name, &Extras::default())
}

pub fn begin_run_in(conn: &Connection, session: Option<i64>, new: &NewSession, prompt: &str, model: &str, model_name: &str, extras: &Extras) -> Result<Begun, String> {
    let tx = Tx::begin(conn)?;
    let session_id = match session {
        Some(id) => {
            tx.query_row("SELECT id FROM sessions WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(err)?.ok_or("找不到这个对话，可能已经被删除了")?
        }
        None => {
            let project = match new.project {
                Some(p) => Some(tx.query_row("SELECT id FROM projects WHERE id = ?1", [p], |r| r.get::<_, i64>(0)).optional().map_err(err)?.ok_or("这个项目已经被删除了")?),
                None => None,
            };
            let setting = instructions::checked(&tx, &new.setting)?;
            tx.execute(
                "INSERT INTO sessions (title, project_id, instructions, preset_id, memory_disabled) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![NEW_TITLE, project, setting.content, setting.preset_id, new.memory_off],
            )
            .map_err(err)?;
            tx.last_insert_rowid()
        }
    };
    let shown = path(&tx, session_id)?;
    let turns: Vec<Turn> = shown.iter().map(|m| Turn { id: m.id, role: m.role.clone(), content: m.content.clone(), state: m.state.clone() }).collect();
    let mut request = context::request(&tx, session_id, &turns, prompt)?;
    let leaf = shown.last().map(|m| m.id);
    let project: Option<i64> = tx.query_row("SELECT project_id FROM sessions WHERE id = ?1", [session_id], |r| r.get(0)).map_err(err)?;
    let used = files::current_versions(&tx, project, &extras.files)?;
    let earlier: Vec<i64> = shown.iter().filter(|m| m.role == "user").map(|m| m.id).collect();
    request.sources = reading::sources(&tx, &earlier, &extras.attachments, &used, &extras.passages)?;
    request.memory = memory::section(&extras.memories);

    tx.execute("INSERT INTO runs (session_id, model, model_name, state) VALUES (?1, ?2, ?3, 'running')", params![session_id, model, model_name]).map_err(err)?;
    let run_id = tx.last_insert_rowid();
    tx.execute("INSERT INTO messages (session_id, run_id, parent_id, role, content, state) VALUES (?1, ?2, ?3, 'user', ?4, 'complete')", params![session_id, run_id, leaf, prompt])
        .map_err(err)?;
    let user_id = tx.last_insert_rowid();
    files::attach(&tx, user_id, &extras.attachments)?;
    files::record_used(&tx, user_id, &used)?;
    reading::record_passages(&tx, run_id, &extras.passages)?;
    memory::record_used(&tx, run_id, &extras.memories)?;
    tx.execute("INSERT INTO messages (session_id, run_id, parent_id, role, state) VALUES (?1, ?2, ?3, 'assistant', 'streaming')", params![session_id, run_id, user_id]).map_err(err)?;
    let reply_id = tx.last_insert_rowid();
    tx.execute("UPDATE messages SET active_child_id = ?1 WHERE id = ?2", [reply_id, user_id]).map_err(err)?;
    match leaf {
        Some(l) => tx.execute("UPDATE messages SET active_child_id = ?1 WHERE id = ?2", [user_id, l]),
        None => tx.execute("UPDATE sessions SET active_root_id = ?1 WHERE id = ?2", [user_id, session_id]),
    }
    .map_err(err)?;
    tx.execute(
        &format!("UPDATE sessions SET updated_at = {NOW}, title = CASE WHEN title = ?1 AND title_source = 'auto' THEN ?2 ELSE title END WHERE id = ?3"),
        params![NEW_TITLE, auto_title(prompt), session_id],
    )
    .map_err(err)?;
    restore_archived(&tx, session_id)?;
    tx.commit()?;
    Ok(Begun { session_id, run_id, user_id, reply_id, request })
}

// ---------- versions ----------

/// Messages one step from the same parent with the same role, oldest first (the versions of `id`).
fn siblings(conn: &Connection, session_id: i64, parent: Option<i64>, role: &str) -> Result<Vec<i64>, String> {
    let mut st = conn.prepare("SELECT id FROM messages WHERE session_id = ?1 AND parent_id IS ?2 AND role = ?3 ORDER BY created_at, id").map_err(err)?;
    let rows = st.query_map(params![session_id, parent, role], |r| r.get(0)).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

#[derive(Debug, Clone)]
struct Node {
    id: i64,
    session_id: i64,
    parent: Option<i64>,
    role: String,
    content: String,
    state: String,
}

fn node(conn: &Connection, id: i64) -> Result<Node, String> {
    conn.query_row("SELECT id, session_id, parent_id, role, content, state FROM messages WHERE id = ?1", [id], |r| {
        Ok(Node { id: r.get(0)?, session_id: r.get(1)?, parent: r.get(2)?, role: r.get(3)?, content: r.get(4)?, state: r.get(5)? })
    })
    .optional()
    .map_err(err)?
    .ok_or_else(|| "找不到这条消息，可能已经被删除了".into())
}

/// Points every ancestor of `id` (and the session's root) at it, so this version and its own
/// remembered continuation are displayed.
pub fn show_path(conn: &Connection, id: i64) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    let mut cur = node(&tx, id)?;
    let session_id = cur.session_id;
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(cur.id) {
            return Err("这个对话的消息链接坏了".into());
        }
        match cur.parent {
            None => {
                tx.execute("UPDATE sessions SET active_root_id = ?1 WHERE id = ?2", [cur.id, session_id]).map_err(err)?;
                break;
            }
            Some(p) => {
                tx.execute("UPDATE messages SET active_child_id = ?1 WHERE id = ?2 AND session_id = ?3", [cur.id, p, session_id]).map_err(err)?;
                cur = node(&tx, p)?;
            }
        }
    }
    tx.commit()
}

/// `delta` -1 / +1 among the message's versions; no wrapping. Returns the shown version.
pub fn switch_version(conn: &Connection, id: i64, delta: i64) -> Result<i64, String> {
    let n = node(conn, id)?;
    let list = siblings(conn, n.session_id, n.parent, &n.role)?;
    let i = list.iter().position(|x| *x == id).ok_or("找不到这条消息")? as i64;
    let Some(target) = usize::try_from(i + delta).ok().and_then(|j| list.get(j)) else { return Ok(id) };
    show_path(conn, *target)?;
    Ok(*target)
}

/// A new version: `edited` = a new sibling of a (finished) question plus its reply; otherwise a new
/// reply under the reply's question (regenerate / retry — the question is never duplicated).
/// The model only gets the question's ancestors: later messages of any branch never leak in.
/// The question's attachments and project files (same versions) come along; `passages` were
/// retrieved for this run.
pub fn begin_version(conn: &Connection, source: i64, edited: Option<&str>, model: &str, model_name: &str, passages: &[Passage], memories: &[UsedMemory]) -> Result<Begun, String> {
    let tx = Tx::begin(conn)?;
    let src = node(&tx, source)?;
    let (question, prompt) = match edited {
        Some(text) => {
            if src.role != "user" || src.state != "complete" {
                return Err("只能修改自己的问题".into());
            }
            let p = text.trim();
            if p.is_empty() {
                return Err("问题不能为空".into());
            }
            (None, p.to_string())
        }
        None => {
            if src.role != "assistant" || src.state == "streaming" {
                return Err("这条回复还在写，等它写完".into());
            }
            let q = src.parent.map(|p| node(&tx, p)).transpose()?.filter(|q| q.role == "user").ok_or("这条回复找不到它的问题")?;
            let p = q.content.clone();
            (Some(q), p)
        }
    };
    // the question's ancestors, oldest first
    let mut chain = vec![];
    let mut seen = HashSet::new();
    let mut up = match &question {
        Some(q) => q.parent,
        None => src.parent,
    };
    while let Some(id) = up {
        if !seen.insert(id) {
            return Err("这个对话的消息链接坏了".into());
        }
        let n = node(&tx, id)?;
        up = n.parent;
        chain.push(n);
    }
    chain.reverse();
    let earlier: Vec<i64> = chain.iter().filter(|m| m.role == "user").map(|m| m.id).collect();
    let turns: Vec<Turn> = chain.into_iter().map(|m| Turn { id: m.id, role: m.role, content: m.content, state: m.state }).collect();
    let mut request = context::request(&tx, src.session_id, &turns, &prompt)?;
    let asked = question.as_ref().map_or(src.id, |q| q.id);
    let attachments: Vec<i64> = files::attachments_of(&tx, asked)?.into_iter().map(|f| f.id).collect();
    let used = files::used_of(&tx, asked)?;
    request.sources = reading::sources(&tx, &earlier, &attachments, &used, passages)?;
    request.memory = memory::section(memories);

    let session_id = src.session_id;
    tx.execute("INSERT INTO runs (session_id, model, model_name, state) VALUES (?1, ?2, ?3, 'running')", params![session_id, model, model_name]).map_err(err)?;
    let run_id = tx.last_insert_rowid();
    let user_id = match &question {
        Some(q) => q.id,
        None => {
            tx.execute("INSERT INTO messages (session_id, run_id, parent_id, role, content, state) VALUES (?1, ?2, ?3, 'user', ?4, 'complete')", params![session_id, run_id, src.parent, prompt])
                .map_err(err)?;
            let id = tx.last_insert_rowid();
            files::attach(&tx, id, &attachments)?;
            files::record_used(&tx, id, &used)?;
            id
        }
    };
    reading::record_passages(&tx, run_id, passages)?;
    memory::record_used(&tx, run_id, memories)?;
    tx.execute("INSERT INTO messages (session_id, run_id, parent_id, role, state) VALUES (?1, ?2, ?3, 'assistant', 'streaming')", params![session_id, run_id, user_id]).map_err(err)?;
    let reply_id = tx.last_insert_rowid();
    show_path(&tx, reply_id)?;
    tx.execute(&format!("UPDATE sessions SET updated_at = {NOW} WHERE id = ?1"), [session_id]).map_err(err)?;
    restore_archived(&tx, session_id)?;
    if edited.is_some() && src.parent.is_none() {
        tx.execute("UPDATE sessions SET title = ?1 WHERE id = ?2 AND title_source = 'auto'", params![auto_title(&prompt), session_id]).map_err(err)?;
    }
    tx.commit()?;
    Ok(Begun { session_id, run_id, user_id, reply_id, request })
}

/// Sending in an archived session brings it back (first in its list).
fn restore_archived(conn: &Connection, session_id: i64) -> Result<(), String> {
    conn.execute("UPDATE sessions SET sort_order = NULL, archived_at = NULL WHERE id = ?1 AND archived_at IS NOT NULL", [session_id]).map(|_| ()).map_err(err)
}

pub fn checkpoint(conn: &Connection, reply_id: i64, content: &str, reasoning: &str) -> Result<(), String> {
    conn.execute("UPDATE messages SET content = ?1, reasoning = ?2 WHERE id = ?3 AND state = 'streaming'", params![content, reasoning, reply_id]).map(|_| ()).map_err(err)
}

#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub content: String,
    pub reasoning: String,
    pub reasoning_ms: Option<i64>,
    /// completed | failed | cancelled | interrupted
    pub state: &'static str,
    pub error: Option<String>,
    pub usage: Option<(i64, i64)>,
    pub window: Option<i64>,
}

pub fn finish(conn: &Connection, run_id: i64, reply_id: i64, o: &Outcome) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    let msg_state = if o.state == "completed" { "complete" } else { o.state };
    tx.execute(
        "UPDATE messages SET content = ?1, reasoning = ?2, reasoning_ms = ?3, state = ?4 WHERE id = ?5",
        params![o.content, o.reasoning, o.reasoning_ms, msg_state, reply_id],
    )
    .map_err(err)?;
    tx.execute(
        &format!("UPDATE runs SET state = ?1, error = ?2, ended_at = {NOW}, prompt_tokens = ?3, completion_tokens = ?4, context_window = ?5 WHERE id = ?6 AND state = 'running'"),
        params![o.state, o.error, o.usage.map(|u| u.0), o.usage.map(|u| u.1), o.window, run_id],
    )
    .map_err(err)?;
    tx.commit()
}

// ---------- the running reply ----------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StreamEvent {
    Reasoning { text: String },
    Content { text: String },
    /// Reading material in segments: the n-th of `total` steps.
    Reading { current: usize, total: usize, label: String },
    /// A segment's notes so far (replaces earlier text for the same label).
    Note { label: String, content: String },
    /// One lookup in Pomelo, done.
    Query(Query),
    #[serde(rename_all = "camelCase")]
    Done { state: String, error: Option<String>, reasoning_ms: Option<i64> },
}

#[derive(Default)]
struct Buf {
    content: String,
    reasoning: String,
    reasoning_started: Option<Instant>,
    reasoning_ms: Option<i64>,
    usage: Option<(i64, i64)>,
    /// Tokens used while reading in segments, added to the reply's.
    extra: (i64, i64),
    window: Option<i64>,
}

impl Buf {
    fn close_reasoning(&mut self) {
        if let (Some(t), None) = (self.reasoning_started, self.reasoning_ms) {
            self.reasoning_ms = Some(t.elapsed().as_millis() as i64);
        }
    }
}

pub struct Shared {
    run_id: i64,
    reply_id: i64,
    buf: Mutex<Buf>,
    done: AtomicBool,
    channel: Channel<StreamEvent>,
    /// Reading in segments now (also in the status: a page that lost the stream still shows it).
    reading: Mutex<Option<Reading>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reading {
    pub current: usize,
    pub total: usize,
    pub label: String,
}

/// What holds the slot: a reply or a summary.
pub enum Job {
    Reply(Arc<Shared>),
    Summary(Arc<SummaryJob>),
}

struct Active {
    run_id: i64,
    session_id: i64,
    job: Job,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
}

/// The one slot for model work (a local server has a single slot).
#[derive(Default)]
pub struct Runs {
    active: Mutex<Option<Active>>,
}

impl Runs {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Active>> {
        self.active.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn busy(&self) -> bool {
        self.lock().is_some()
    }
    pub fn active_run(&self) -> Option<i64> {
        self.lock().as_ref().map(|a| a.run_id)
    }
    pub fn active_session(&self) -> Option<i64> {
        self.lock().as_ref().map(|a| a.session_id)
    }
    /// "reply" | "summary"
    pub fn active_kind(&self) -> Option<&'static str> {
        self.lock().as_ref().map(|a| match a.job {
            Job::Reply(_) => "reply",
            Job::Summary(_) => "summary",
        })
    }
    pub fn active_reading(&self) -> Option<Reading> {
        self.lock().as_ref().and_then(|a| match &a.job {
            Job::Reply(s) => s.reading.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            Job::Summary(_) => None,
        })
    }
    pub fn occupy(&self, run_id: i64, session_id: i64, job: Job) {
        *self.lock() = Some(Active { run_id, session_id, job, task: None });
    }
    pub fn attach(&self, run_id: i64, task: tauri::async_runtime::JoinHandle<()>) {
        if let Some(a) = self.lock().as_mut().filter(|a| a.run_id == run_id) {
            a.task = Some(task);
        }
    }
    /// Frees the slot if `run_id` still holds it.
    pub fn release(&self, run_id: i64) {
        let mut g = self.lock();
        if g.as_ref().is_some_and(|a| a.run_id == run_id) {
            *g = None;
        }
    }
    fn take(&self, run_id: Option<i64>) -> Option<Active> {
        let mut g = self.lock();
        match g.as_ref() {
            Some(a) if run_id.is_none_or(|id| a.run_id == id) => g.take(),
            _ => None,
        }
    }
}

/// Ends whatever holds the slot (stop / quit).
fn end_active(app: &AppHandle, a: Active, state: &'static str) {
    if let Some(t) = &a.task {
        t.abort();
    }
    match &a.job {
        Job::Reply(shared) => finalize(app, shared, state, None),
        Job::Summary(job) => context::end(app, job, state, None, 0, None),
    }
}

/// Saves the outcome once, tells the page, frees the slot.
fn finalize(app: &AppHandle, shared: &Shared, state: &'static str, error: Option<String>) {
    if shared.done.swap(true, Ordering::SeqCst) {
        return;
    }
    let st = app.state::<AppState>();
    let outcome = {
        let mut b = shared.buf.lock().unwrap_or_else(|e| e.into_inner());
        b.close_reasoning();
        let usage = match (b.usage, b.extra) {
            (u, (0, 0)) => u,
            (Some((p, k)), (ep, ek)) => Some((p + ep, k + ek)),
            (None, e) => Some(e),
        };
        Outcome { content: b.content.clone(), reasoning: b.reasoning.clone(), reasoning_ms: b.reasoning_ms, state, error, usage, window: b.window }
    };
    let saved = st.picky.with_db(|c| finish(c, shared.run_id, shared.reply_id, &outcome));
    let error = match (&saved, outcome.error.clone()) {
        (Err(e), None) => Some(format!("回复没能保存：{e}")),
        (_, e) => e,
    };
    let _ = shared.channel.send(StreamEvent::Done { state: state.into(), error, reasoning_ms: outcome.reasoning_ms });
    st.picky.runs.release(shared.run_id);
    st.picky.changed();
}

/// The messages for the reply and its cap: the question with its material when that fits,
/// otherwise the question with notes from reading the material in segments.
async fn compose(app: &AppHandle, shared: &Shared, route: &Route, request: &Request, window: i64) -> Result<(Vec<provider::Msg>, i64, bool), String> {
    let counter = Counter::for_target(&route.target);
    let sizes = Sizes::of(request, &counter).await;
    let plain = context::fit(request, &sizes, window)?; // the question itself must fit
    if request.sources.is_empty() {
        return Ok((plain.0, plain.1, false));
    }
    let with = |prompt: i64| Sizes { prompt, ..sizes.clone() };
    let full = reading::prompt(&request.prompt, &request.sources);
    if let Some(sel) = context::select(&with(counter.count(&full).await), window) {
        return Ok((request.messages_with(sel.dropped, &full), sel.max_tokens, false));
    }
    let st = app.state::<AppState>();
    st.picky.with_db(|c| c.execute("UPDATE runs SET kind = 'reading' WHERE id = ?1 AND state = 'running'", [shared.run_id]).map(|_| ()).map_err(err))?;
    let artifacts_of = |label: &str, content: &str| {
        let _ = shared.channel.send(StreamEvent::Note { label: label.to_string(), content: content.to_string() });
    };
    let reader = Reader {
        target: &route.target,
        counter: &counter,
        window,
        instructions: &request.system,
        progress: &|current, total, label: String| {
            *shared.reading.lock().unwrap_or_else(|e| e.into_inner()) = Some(Reading { current, total, label: label.clone() });
            let _ = shared.channel.send(StreamEvent::Reading { current, total, label });
        },
        note: &|artifact, ordinal, label, content| {
            let _ = app.state::<AppState>().picky.with_db(|c| reading::save_note(c, shared.run_id, artifact, ordinal, content));
            artifacts_of(label, content);
        },
        usage: Default::default(),
    };
    let kinds = reading::kinds(&request.sources);
    let mut material = reader.read(&request.prompt, &request.sources).await?;
    let picked = loop {
        let notes = material.join("\n\n");
        let prompt = reading::notes_prompt(&request.prompt, &notes, &kinds);
        if let Some(sel) = context::select(&with(counter.count(&prompt).await), window) {
            break (request.messages_with(sel.dropped, &prompt), sel.max_tokens);
        }
        let before = counter.count(&notes).await;
        material = reader.compress(&request.prompt, &material).await?;
        if counter.count(&material.join("\n\n")).await >= before {
            return Err("阅读要点仍太长，无法放进当前上下文。已读要点已保存，请缩短问题或指令，或切换上下文更大的模型后重试。".into());
        }
    };
    let used = *reader.usage.lock().unwrap_or_else(|e| e.into_inner());
    shared.buf.lock().unwrap_or_else(|e| e.into_inner()).extra = used;
    *shared.reading.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let _ = shared.channel.send(StreamEvent::Reading { current: 0, total: 0, label: String::new() });
    Ok((picked.0, picked.1, true))
}

/// What this answer may look up in Pomelo; None = nothing to offer (every module hidden…).
pub(crate) struct Lookup {
    access: data::Access,
    today: String,
    tools: Vec<serde_json::Value>,
    /// Tokens the definitions take.
    cost: i64,
}

impl Lookup {
    /// The lookup part of the system message.
    pub(crate) fn instructions(&self) -> String {
        data::instructions(&self.today, &self.access)
    }
    /// Room kept in the window: the definitions and the results.
    pub(crate) fn kept(&self, window: i64) -> i64 {
        self.cost + context::reserve(window)
    }
}

/// Template tokens around the tool definitions.
const TOOLS_OVERHEAD: i64 = 64;
/// Below this the next request must answer (no more lookups).
const MIN_ROOM: i64 = 256;

pub(crate) async fn lookup_plan(app: &AppHandle, route: &Route, counter: &Counter) -> Option<Lookup> {
    let st = app.state::<AppState>();
    let allowed = st.picky.with_db(|c| Ok(data::cloud_allowed(c))).ok()?;
    let (hidden, today) = st.with_db(|c| Ok((data::hidden(c), crate::modules::todo::today(c)?))).ok()?;
    let access = data::Access { hidden, cloud: route.model.starts_with("cloud:"), cloud_allowed: allowed };
    let tools = data::tools(&access);
    if tools.is_empty() {
        return None;
    }
    let cost = counter.count(&serde_json::to_string(&tools).unwrap_or_default()).await + TOOLS_OVERHEAD;
    Some(Lookup { access, today, tools, cost })
}

/// What one request streamed besides the shared buffer.
#[derive(Default)]
struct Round {
    calls: Vec<ToolCall>,
    content: String,
    reasoning: String,
}

/// Streams one request into the reply (later rounds continue after a blank line).
async fn stream_round(app: &AppHandle, shared: &Shared, target: &Target, messages: Vec<Msg>, opts: Options, last: &mut Instant) -> (Result<(), String>, Round) {
    let mut r = Round::default();
    let on = |e: Event| {
        let mut b = shared.buf.lock().unwrap_or_else(|e| e.into_inner());
        match e {
            Event::Reasoning(t) => {
                b.reasoning_started.get_or_insert_with(Instant::now);
                if r.reasoning.is_empty() && !b.reasoning.is_empty() {
                    b.reasoning.push_str("\n\n");
                    let _ = shared.channel.send(StreamEvent::Reasoning { text: "\n\n".into() });
                }
                r.reasoning.push_str(&t);
                b.reasoning.push_str(&t);
                let _ = shared.channel.send(StreamEvent::Reasoning { text: t });
            }
            Event::Content(t) => {
                b.close_reasoning();
                if r.content.is_empty() && !b.content.is_empty() {
                    b.content.push_str("\n\n");
                    let _ = shared.channel.send(StreamEvent::Content { text: "\n\n".into() });
                }
                r.content.push_str(&t);
                b.content.push_str(&t);
                let _ = shared.channel.send(StreamEvent::Content { text: t });
            }
            Event::ToolCalls(c) => r.calls = c,
            Event::Usage { prompt, completion } => b.usage = Some((prompt, completion)),
            Event::Finished(_) => {}
        }
        if last.elapsed() >= CHECKPOINT {
            *last = Instant::now();
            let _ = app.state::<AppState>().picky.with_db(|c| checkpoint(c, shared.reply_id, &b.content, &b.reasoning));
        }
    };
    let result = provider::stream(target.clone(), messages, opts, on).await;
    (result, r)
}

/// A call id where the server gave none (9 letters / digits: Mistral templates need that shape).
fn call_id() -> String {
    const A: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut b = [0u8; 9];
    let _ = getrandom::fill(&mut b);
    b.iter().map(|x| A[*x as usize % A.len()] as char).collect()
}

/// The system message without the lookup part (no tools after all).
fn without_lookups(messages: &mut Vec<Msg>, request: &Request) {
    if messages.first().is_some_and(|m| m.role == "system") && !request.data.is_empty() {
        let text = Request { data: String::new(), ..request.clone() }.system_text();
        if text.is_empty() {
            messages.remove(0);
        } else {
            messages[0].content = text;
        }
    }
}

/// What the lookup loop needs around it: the app (`AppHost`) or a test.
trait Host {
    async fn stream(&mut self, messages: Vec<Msg>, opts: Options) -> (Result<(), String>, Round);
    /// Runs one call on pomelo.db within `budget` tokens.
    fn lookup(&self, call: &ToolCall, budget: i64) -> data::Answer;
    /// Saves a lookup with the run and shows it.
    fn record(&self, position: usize, round: usize, call: &ToolCall, q: Query);
    /// A round that asked for lookups: its usage joins the total.
    fn fold_usage(&self);
}

struct AppHost<'a> {
    app: &'a AppHandle,
    shared: &'a Shared,
    target: Target,
    last: Instant,
    access: data::Access,
    today: String,
}

impl Host for AppHost<'_> {
    async fn stream(&mut self, messages: Vec<Msg>, opts: Options) -> (Result<(), String>, Round) {
        stream_round(self.app, self.shared, &self.target, messages, opts, &mut self.last).await
    }
    fn lookup(&self, call: &ToolCall, budget: i64) -> data::Answer {
        self.app
            .state::<AppState>()
            .with_db(|c| Ok(data::run(c, &self.access, &call.name, &call.arguments, &self.today, budget)))
            .unwrap_or_else(|e| data::Answer { module: None, label: "没查成".into(), state: "failed", text: format!("没查成：{e}"), records: vec![] })
    }
    fn record(&self, position: usize, round: usize, call: &ToolCall, q: Query) {
        let _ = self.app.state::<AppState>().picky.with_db(|c| data::save_query(c, self.shared.run_id, position, round, call, &q));
        let _ = self.shared.channel.send(StreamEvent::Query(q));
    }
    fn fold_usage(&self) {
        let mut b = self.shared.buf.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((p, c)) = b.usage.take() {
            b.extra = (b.extra.0 + p, b.extra.1 + c);
        }
    }
}

/// Answers, looking things up for up to `data::MAX_ROUNDS` rounds (then the request must answer).
/// `used`: what `messages` take; `cost`: the definitions. Ok = completed.
async fn answer_with_lookups(host: &mut impl Host, tools: &[serde_json::Value], cost: i64, counter: &Counter, request: &Request, mut messages: Vec<Msg>, window: i64, mut used: i64) -> Result<(), String> {
    let (mut round, mut position) = (0usize, 0usize);
    loop {
        let answer_now = round >= data::MAX_ROUNDS;
        let opts = Options { max_tokens: Some(window - used - cost), thinking: None, tools: tools.to_vec(), no_calls: answer_now };
        let (result, r) = host.stream(messages.clone(), opts).await;
        if let Err(e) = result {
            if round == 0 && provider::rejected(&e) && r.content.is_empty() && r.reasoning.is_empty() {
                // the service doesn't take tools: say so, ask again without them
                host.record(position, 0, &ToolCall::default(), Query::unsupported());
                without_lookups(&mut messages, request);
                return host.stream(messages, Options { max_tokens: Some(window - used), ..Default::default() }).await.0;
            }
            return Err(e);
        }
        if r.calls.is_empty() || answer_now {
            return Ok(());
        }
        host.fold_usage();
        round += 1;
        let calls: Vec<ToolCall> = r.calls.into_iter().map(|c| if c.id.is_empty() { ToolCall { id: call_id(), ..c } } else { c }).collect();
        let args: String = calls.iter().map(|c| format!("{}{}", c.name, c.arguments)).collect();
        used += counter.count(&r.content).await + counter.count(&args).await + context::PER_MESSAGE;
        messages.push(Msg { role: "assistant".into(), content: r.content, tool_calls: calls.clone(), reasoning: Some(r.reasoning), ..Default::default() });
        let room = window - used - cost - context::reserve(window);
        let mut spent = 0;
        for (i, call) in calls.iter().enumerate() {
            let budget = (room - spent) / (calls.len() - i) as i64 - context::PER_MESSAGE;
            let answer = if budget < 64 {
                data::Answer { module: None, label: "放不下更多查到的内容了".into(), state: "failed", text: "上下文放不下更多查到的内容了，请根据已经查到的回答。".into(), records: vec![] }
            } else {
                host.lookup(call, budget)
            };
            let c = counter.count(&answer.text).await + context::PER_MESSAGE;
            used += c;
            spent += c;
            messages.push(Msg::tool(&call.id, &answer.text));
            host.record(position, round, call, Query::of(&answer));
            position += 1;
        }
        if room - spent < MIN_ROOM {
            round = data::MAX_ROUNDS;
        }
    }
}

async fn generate(app: AppHandle, shared: Arc<Shared>, route: Route, mut request: Request) {
    let window = context::window(&route).await;
    shared.buf.lock().unwrap_or_else(|e| e.into_inner()).window = Some(window);
    let counter = Counter::for_target(&route.target);
    let mut lookup = lookup_plan(&app, &route, &counter).await;
    if let Some(l) = &lookup {
        request.data = l.instructions();
        // the definitions and room for results come first; a question that only fits without them goes without
        if context::select(&Sizes::of(&request, &counter).await, window - l.kept(window)).is_none() {
            request.data.clear();
            lookup = None;
        }
    }
    let kept = lookup.as_ref().map_or(0, |l| l.kept(window));
    let (mut messages, max_tokens, read) = match compose(&app, &shared, &route, &request, window - kept).await {
        Ok(x) => x,
        Err(e) => return finalize(&app, &shared, "failed", Some(e)),
    };
    let result = match lookup {
        Some(l) if !read => {
            let mut host = AppHost { app: &app, shared: &shared, target: route.target.clone(), last: Instant::now(), access: l.access, today: l.today };
            answer_with_lookups(&mut host, &l.tools, l.cost, &counter, &request, messages, window, window - kept - max_tokens).await
        }
        _ => {
            without_lookups(&mut messages, &request);
            let opts = Options { max_tokens: Some(max_tokens + kept), thinking: if read { Some(false) } else { None }, ..Default::default() };
            stream_round(&app, &shared, &route.target, messages, opts, &mut Instant::now()).await.0
        }
    };
    match result {
        Ok(()) => finalize(&app, &shared, "completed", None),
        Err(e) => finalize(&app, &shared, "failed", Some(e)),
    }
}

/// App quitting: keep what arrived, marked interrupted.
pub fn shutdown(app: &AppHandle) {
    let st = app.state::<AppState>();
    if let Some(a) = st.picky.runs.take(None) {
        end_active(app, a, "interrupted");
    }
}

// ---------- commands ----------

#[tauri::command]
pub fn picky_sessions(state: State<AppState>) -> Result<Vec<Session>, String> {
    state.picky.with_db(|c| sessions(c))
}

#[tauri::command]
pub fn picky_messages(state: State<AppState>, session_id: i64) -> Result<Vec<Message>, String> {
    state.picky.with_db(|c| path(c, session_id))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    pub session_id: i64,
    pub run_id: i64,
    pub user_message_id: i64,
    pub message_id: i64,
}

/// What a new question / version needs before anything is saved: the slot is free, and where it
/// goes (the selected model) resolves.
fn prepare(app: &AppHandle) -> Result<Route, String> {
    let st = app.state::<AppState>();
    if st.picky.runs.busy() {
        return Err("正在生成回复，等它写完或先停止。".into());
    }
    st.picky.target()
}

/// Streams the begun reply over `on_event` (reasoning / content deltas, then one `done`).
fn start(app: &AppHandle, b: Begun, route: Route, on_event: Channel<StreamEvent>) -> Sent {
    let st = app.state::<AppState>();
    let shared = Arc::new(Shared { run_id: b.run_id, reply_id: b.reply_id, buf: Mutex::default(), done: AtomicBool::new(false), channel: on_event, reading: Mutex::default() });
    st.picky.runs.occupy(b.run_id, b.session_id, Job::Reply(shared.clone()));
    let task = tauri::async_runtime::spawn(generate(app.clone(), shared, route, b.request));
    st.picky.runs.attach(b.run_id, task);
    st.picky.changed();
    Sent { session_id: b.session_id, run_id: b.run_id, user_message_id: b.user_id, message_id: b.reply_id }
}

/// Saves the question and an empty reply at the end of the shown path (a new session when
/// `session_id` is None, in `project_id` with the draft's `instructions`), then streams the reply.
/// Returns at once.
#[tauri::command]
pub async fn picky_send(
    app: AppHandle,
    session_id: Option<i64>,
    project_id: Option<i64>,
    instructions: Option<SessionSetting>,
    memory_off: bool,
    text: String,
    attachments: Vec<i64>,
    files: Vec<i64>,
    passages: Vec<Passage>,
    memories: Vec<UsedMemory>,
    on_event: Channel<StreamEvent>,
) -> Result<Sent, String> {
    let prompt = match text.trim() {
        "" if !attachments.is_empty() => "请总结这些附件。",
        "" => return Err("问题不能为空".into()),
        t => t,
    };
    let route = prepare(&app)?;
    let new = NewSession { project: project_id, setting: instructions.unwrap_or_default(), memory_off };
    let extras = Extras { attachments, files, passages, memories };
    let b = app.state::<AppState>().picky.with_db(|c| begin_run_in(c, session_id, &new, prompt, &route.model, &route.name, &extras))?;
    Ok(start(&app, b, route, on_event))
}

/// A new version of a question (edited text) and its reply.
#[tauri::command]
pub async fn picky_edit(app: AppHandle, message_id: i64, text: String, passages: Vec<Passage>, memories: Vec<UsedMemory>, on_event: Channel<StreamEvent>) -> Result<Sent, String> {
    let route = prepare(&app)?;
    let b = app.state::<AppState>().picky.with_db(|c| begin_version(c, message_id, Some(&text), &route.model, &route.name, &passages, &memories))?;
    Ok(start(&app, b, route, on_event))
}

/// A new version of a reply (regenerate, or retry a failed / stopped / interrupted one).
#[tauri::command]
pub async fn picky_regenerate(app: AppHandle, message_id: i64, passages: Vec<Passage>, memories: Vec<UsedMemory>, on_event: Channel<StreamEvent>) -> Result<Sent, String> {
    let route = prepare(&app)?;
    let b = app.state::<AppState>().picky.with_db(|c| begin_version(c, message_id, None, &route.model, &route.name, &passages, &memories))?;
    Ok(start(&app, b, route, on_event))
}

/// Shows the previous (-1) / next (+1) version of a message. Returns the shown version's id.
#[tauri::command]
pub fn picky_switch_version(state: State<AppState>, message_id: i64, delta: i64) -> Result<i64, String> {
    if state.picky.runs.busy() {
        return Err("正在生成，等它写完再切换版本。".into());
    }
    if delta != -1 && delta != 1 {
        return Err("只能切到上一个或下一个版本".into());
    }
    state.picky.with_db(|c| switch_version(c, message_id, delta))
}

/// Stops the reply (what arrived so far is kept, marked cancelled) or the summary (nothing saved).
#[tauri::command]
pub fn picky_stop(app: AppHandle, run_id: i64) -> Result<(), String> {
    let st = app.state::<AppState>();
    if let Some(a) = st.picky.runs.take(Some(run_id)) {
        end_active(&app, a, "cancelled");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::provider::Msg;
    use crate::picky::store::tests::memory_db;

    fn reply(c: &Connection, b: &Begun, text: &str) {
        finish(c, b.run_id, b.reply_id, &Outcome { content: text.into(), state: "completed", ..Default::default() }).unwrap();
    }

    #[test]
    fn titles() {
        assert_eq!(auto_title("\n\n  你好   世界 \n第二行"), "你好 世界");
        assert_eq!(auto_title("   "), NEW_TITLE);
        let long = "字".repeat(50);
        assert_eq!(auto_title(&long), format!("{}…", "字".repeat(40)));
    }

    #[test]
    fn send_builds_the_path_and_history() {
        let c = memory_db();
        let b1 = begin_run(&c, None, None, "第一问\n细节", "local:qwen14b", "Qwen3 14B").unwrap();
        assert_eq!(b1.request.messages(), [Msg::new("user", "第一问\n细节")]);
        let p = path(&c, b1.session_id).unwrap();
        assert_eq!((p.len(), p[1].state.as_str(), p[1].model.as_deref()), (2, "streaming", Some("Qwen3 14B")));
        assert_eq!(p[0].model, None);
        reply(&c, &b1, "第一答");
        let b2 = begin_run(&c, Some(b1.session_id), None, "第二问", "cloud:deepseek-flash", "DeepSeek Flash").unwrap();
        assert_eq!(b2.request.messages(), [Msg::new("user", "第一问\n细节"), Msg::new("assistant", "第一答"), Msg::new("user", "第二问")]);
        let ids: Vec<i64> = path(&c, b1.session_id).unwrap().iter().map(|m| m.id).collect();
        assert_eq!(ids, [b1.user_id, b1.reply_id, b2.user_id, b2.reply_id]);
        let s = sessions(&c).unwrap();
        assert_eq!((s.len(), s[0].title.as_str()), (1, "第一问"));
    }

    #[test]
    fn failed_or_empty_replies_stay_out_of_history_and_errors_show() {
        let c = memory_db();
        let b1 = begin_run(&c, None, None, "a", "local:x", "X").unwrap();
        finish(&c, b1.run_id, b1.reply_id, &Outcome { state: "failed", error: Some("连不上本地模型".into()), ..Default::default() }).unwrap();
        let p = path(&c, b1.session_id).unwrap();
        assert_eq!((p[1].state.as_str(), p[1].error.as_deref()), ("failed", Some("连不上本地模型")));
        let b2 = begin_run(&c, Some(b1.session_id), None, "b", "local:x", "X").unwrap();
        assert_eq!(b2.request.messages(), [Msg::new("user", "a"), Msg::new("user", "b")]);
        // a cancelled reply with text is kept as context
        finish(&c, b2.run_id, b2.reply_id, &Outcome { content: "半句".into(), state: "cancelled", ..Default::default() }).unwrap();
        let b3 = begin_run(&c, Some(b1.session_id), None, "c", "local:x", "X").unwrap();
        assert_eq!(b3.request.messages().iter().filter(|m| m.role == "assistant").count(), 1);
    }

    #[test]
    fn checkpoint_only_while_streaming_and_finish_records_usage() {
        let c = memory_db();
        let b = begin_run(&c, None, None, "q", "local:x", "X").unwrap();
        checkpoint(&c, b.reply_id, "部分", "想").unwrap();
        assert_eq!(path(&c, b.session_id).unwrap()[1].content, "部分");
        finish(&c, b.run_id, b.reply_id, &Outcome { content: "全部".into(), reasoning: "想好了".into(), reasoning_ms: Some(1200), state: "completed", error: None, usage: Some((10, 20)), window: Some(8192) }).unwrap();
        checkpoint(&c, b.reply_id, "迟到的", "").unwrap(); // ignored after finishing
        let m = &path(&c, b.session_id).unwrap()[1];
        assert_eq!((m.content.as_str(), m.reasoning.as_str(), m.reasoning_ms, m.state.as_str()), ("全部", "想好了", Some(1200), "complete"));
        let (state, p, k): (String, i64, i64) = c.query_row("SELECT state, prompt_tokens, completion_tokens FROM runs", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!((state.as_str(), p, k), ("completed", 10, 20));
        // a run is finished once: a second finish doesn't change the run's state
        finish(&c, b.run_id, b.reply_id, &Outcome { state: "cancelled", ..Default::default() }).unwrap();
        let state: String = c.query_row("SELECT state FROM runs", [], |r| r.get(0)).unwrap();
        assert_eq!(state, "completed");
    }

    #[test]
    fn damaged_links_fall_back_to_the_newest_child() {
        let c = memory_db();
        let b = begin_run(&c, None, None, "q", "local:x", "X").unwrap();
        reply(&c, &b, "a");
        c.execute("UPDATE messages SET active_child_id = 9999 WHERE id = ?1", [b.user_id]).unwrap();
        c.execute("UPDATE sessions SET active_root_id = 8888", []).unwrap();
        let ids: Vec<i64> = path(&c, b.session_id).unwrap().iter().map(|m| m.id).collect();
        assert_eq!(ids, [b.user_id, b.reply_id]);
        // a cycle never loops
        c.execute("UPDATE messages SET active_child_id = ?1 WHERE id = ?2", [b.user_id, b.reply_id]).unwrap();
        assert_eq!(path(&c, b.session_id).unwrap().len(), 2);
        assert!(begin_run(&c, Some(4242), None, "x", "local:x", "X").is_err());
        assert!(path(&c, 4242).is_err());
    }

    fn ids(c: &Connection, s: i64) -> Vec<i64> {
        path(c, s).unwrap().iter().map(|m| m.id).collect()
    }

    #[test]
    fn regenerate_adds_a_sibling_reply_and_versions_keep_their_continuations() {
        let c = memory_db();
        let b1 = begin_run(&c, None, None, "q1", "local:a", "A").unwrap();
        reply(&c, &b1, "a1");
        let b2 = begin_run(&c, Some(b1.session_id), None, "q2", "local:a", "A").unwrap();
        reply(&c, &b2, "a2");

        // regenerate the first reply: only q1 goes to the model, q2/a2 stay on the old branch
        let r = begin_version(&c, b1.reply_id, None, "cloud:x", "X", &[], &[]).unwrap();
        assert_eq!(r.request.messages(), [Msg::new("user", "q1")]);
        assert_eq!(r.user_id, b1.user_id); // the question is not duplicated
        assert_eq!(ids(&c, b1.session_id), [b1.user_id, r.reply_id]);
        let p = path(&c, b1.session_id).unwrap();
        assert_eq!((p[1].version_index, p[1].version_count, p[1].model.as_deref()), (2, 2, Some("X")));
        reply(&c, &r, "a1 again");

        // back to version 1: its continuation (q2, a2) comes back
        assert_eq!(switch_version(&c, r.reply_id, -1).unwrap(), b1.reply_id);
        assert_eq!(ids(&c, b1.session_id), [b1.user_id, b1.reply_id, b2.user_id, b2.reply_id]);
        assert_eq!(switch_version(&c, b1.reply_id, -1).unwrap(), b1.reply_id); // no wrapping
        assert_eq!(switch_version(&c, b1.reply_id, 1).unwrap(), r.reply_id);
        assert_eq!(ids(&c, b1.session_id), [b1.user_id, r.reply_id]);
    }

    #[test]
    fn edit_adds_a_sibling_question_and_retitles_the_first() {
        let c = memory_db();
        let b1 = begin_run(&c, None, None, "第一问", "local:a", "A").unwrap();
        reply(&c, &b1, "答");
        let e = begin_version(&c, b1.user_id, Some("  改过的第一问  "), "local:a", "A", &[], &[]).unwrap();
        assert_ne!(e.user_id, b1.user_id);
        assert_eq!(e.request.messages(), [Msg::new("user", "改过的第一问")]);
        let p = path(&c, b1.session_id).unwrap();
        assert_eq!((p[0].content.as_str(), p[0].version_index, p[0].version_count), ("改过的第一问", 2, 2));
        assert_eq!(sessions(&c).unwrap()[0].title, "改过的第一问");

        // editing a later question keeps the earlier turns as context
        reply(&c, &e, "新答");
        let b2 = begin_run(&c, Some(b1.session_id), None, "第二问", "local:a", "A").unwrap();
        reply(&c, &b2, "答二");
        let e2 = begin_version(&c, b2.user_id, Some("第二问改"), "local:a", "A", &[], &[]).unwrap();
        assert_eq!(e2.request.messages(), [Msg::new("user", "改过的第一问"), Msg::new("assistant", "新答"), Msg::new("user", "第二问改")]);
        assert_eq!(sessions(&c).unwrap()[0].title, "改过的第一问"); // only the first question re-titles

        assert!(begin_version(&c, b2.user_id, Some("   "), "local:a", "A", &[], &[]).is_err());
        assert!(begin_version(&c, b2.reply_id, Some("x"), "local:a", "A", &[], &[]).is_err()); // replies aren't edited
        assert!(begin_version(&c, e2.reply_id, None, "local:a", "A", &[], &[]).is_err()); // still streaming
        assert!(begin_version(&c, b2.user_id, None, "local:a", "A", &[], &[]).is_err()); // questions aren't regenerated
    }

    #[test]
    fn projects_send_their_instructions_and_sending_restores_archived() {
        use crate::picky::library;
        let c = memory_db();
        let p = library::create_project(&c, "论文", "  用中文回答，引用要注明出处。 ").unwrap();
        let b = begin_run(&c, None, Some(p), "第一问", "local:x", "X").unwrap();
        assert_eq!(b.request.messages(), [Msg::new("system", "用中文回答，引用要注明出处。"), Msg::new("user", "第一问")]);
        assert_eq!(library::library(&c).unwrap().projects[0].sessions.len(), 1);
        reply(&c, &b, "答");
        let v = begin_version(&c, b.reply_id, None, "local:x", "X", &[], &[]).unwrap();
        assert_eq!(v.request.messages()[0], Msg::new("system", "用中文回答，引用要注明出处。"));
        reply(&c, &v, "再答");
        assert!(begin_run(&c, None, Some(9999), "x", "local:x", "X").is_err());

        library::set_archived(&c, &[b.session_id], true).unwrap();
        assert_eq!(library::library(&c).unwrap().archived.len(), 1);
        begin_run(&c, Some(b.session_id), None, "又来了", "local:x", "X").unwrap();
        let lib = library::library(&c).unwrap();
        assert!(lib.archived.is_empty());
        assert_eq!(lib.projects[0].sessions[0].id, b.session_id);
    }

    #[test]
    fn a_new_session_keeps_the_drafts_instructions() {
        let c = memory_db();
        instructions::set_global(&c, "全局").unwrap();
        let preset = instructions::create_preset(&c, "老师", "像老师").unwrap();
        let new = NewSession { project: None, setting: SessionSetting { content: " 简短 ".into(), preset_id: Some(preset) }, memory_off: false };
        let b = begin_run_in(&c, None, &new, "q", "local:x", "X", &Extras::default()).unwrap();
        assert_eq!(b.request.system, "全局\n\n简短\n\n像老师");
        assert_eq!(instructions::session_setting(&c, b.session_id).unwrap(), SessionSetting { content: "简短".into(), preset_id: Some(preset) });
        reply(&c, &b, "a");
        // a saved session keeps its own; a draft's choice only applies to a new one
        let b2 = begin_run_in(&c, Some(b.session_id), &NewSession::default(), "q2", "local:x", "X", &Extras::default()).unwrap();
        assert_eq!(b2.request.system, "全局\n\n简短\n\n像老师");
    }

    #[test]
    fn questions_keep_their_files_and_later_questions_resend_attachments() {
        use crate::picky::files::tests::add;
        use crate::picky::{files, library};
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("picky-chat-files");
        let p = library::create_project(&c, "论文", "").unwrap();
        let att = add(&c, &dir, "初稿.md", b"# Intro\nhello");
        let v1 = add(&c, &dir, "提纲.md", b"outline v1");
        let f = files::add_project_file(&c, p, v1).unwrap();
        let passage = Passage { file_id: 99, artifact_id: 98, segment_ordinal: 0, label: "别的.md · 第 1 段".into(), content: "检索到的".into() };
        let extras = Extras { attachments: vec![att], files: vec![f], passages: vec![passage.clone()], memories: vec![] };
        let b = begin_run_in(&c, None, &NewSession { project: Some(p), ..Default::default() }, "看看", "local:x", "X", &extras).unwrap();
        let kinds: Vec<_> = b.request.sources.iter().map(|s| s.kind).collect();
        assert_eq!(kinds, [reading::Kind::Attachment, reading::Kind::ProjectFile, reading::Kind::Passage]);
        assert_eq!(reading::passages_of(&c, b.run_id).unwrap(), [passage]);
        let shown = path(&c, b.session_id).unwrap();
        assert_eq!(shown[0].files.iter().map(|x| (x.file.name.as_str(), x.label.as_deref())).collect::<Vec<_>>(), [("初稿.md", None), ("提纲.md", Some("资料"))]);
        reply(&c, &b, "好");

        // a follow-up sends the earlier attachment again, but not the project file
        let b2 = begin_run(&c, Some(b.session_id), None, "继续", "local:x", "X").unwrap();
        assert_eq!(b2.request.sources.iter().map(|s| s.kind).collect::<Vec<_>>(), [reading::Kind::Attachment]);
        reply(&c, &b2, "嗯");

        // a new version of the file: the old question still shows version 1; an edit keeps it
        let v2 = add(&c, &dir, "提纲.md", b"outline v2");
        files::replace_project_file(&c, f, v2).unwrap();
        let e = begin_version(&c, b.user_id, Some("再看看"), "local:x", "X", &[], &[]).unwrap();
        let q = &path(&c, b.session_id).unwrap()[0];
        assert_eq!(q.content, "再看看");
        assert_eq!(q.files.iter().map(|x| (x.file.id, x.version)).collect::<Vec<_>>(), [(att, None), (v1, Some(1))]);
        assert_eq!(e.request.sources.len(), 2);
        // a deleted copy can't be sent
        c.execute("UPDATE artifacts SET deleted_at = 'x' WHERE id = ?1", [att]).unwrap();
        let bad = Extras { attachments: vec![att], ..Default::default() };
        assert!(begin_run_in(&c, Some(b.session_id), &NewSession::default(), "q", "local:x", "X", &bad).unwrap_err().contains("已删除"));
    }

    #[test]
    fn memories_follow_the_instructions_and_stay_with_the_run() {
        let c = memory_db();
        instructions::set_global(&c, "用中文").unwrap();
        let used = vec![UsedMemory { memory_id: Some(1), content: "住在新加坡".into(), noted_on: "2026-10-01".into() }];
        let extras = Extras { memories: used.clone(), ..Default::default() };
        let b = begin_run_in(&c, None, &NewSession { memory_off: true, ..Default::default() }, "天气", "local:x", "X", &extras).unwrap();
        let sys = &b.request.messages()[0].content;
        assert!(sys.starts_with("用中文\n\n以下是用户明确让你记住的信息") && sys.ends_with("- （记于 2026-10-01）住在新加坡"), "{sys}");
        assert!(memory::conversation_off(&c, b.session_id).unwrap());
        reply(&c, &b, "晴");
        assert_eq!(path(&c, b.session_id).unwrap()[1].memories, used);
        let r = begin_version(&c, b.reply_id, None, "local:x", "X", &[], &[]).unwrap();
        assert_eq!(r.request.messages()[0].content, "用中文"); // a version gets the memories chosen for it
    }

    /// The lookup loop with the mock model (or a scripted one) over an in-memory pomelo.db.
    struct TestHost {
        pomelo: Connection,
        access: data::Access,
        /// Every request: its messages and options.
        requests: Vec<(Vec<Msg>, Options)>,
        recorded: std::cell::RefCell<Vec<(usize, usize, ToolCall, Query)>>,
        content: String,
        /// Always ask for one more lookup.
        greedy: bool,
        /// Refuse requests with tools (like a model without tool support).
        reject_tools: bool,
    }

    impl TestHost {
        fn new(pomelo: Connection, access: data::Access) -> Self {
            TestHost { pomelo, access, requests: vec![], recorded: Default::default(), content: String::new(), greedy: false, reject_tools: false }
        }
    }

    impl Host for TestHost {
        async fn stream(&mut self, messages: Vec<Msg>, opts: Options) -> (Result<(), String>, Round) {
            self.requests.push((messages.clone(), opts.clone()));
            let mut r = Round::default();
            if self.reject_tools && !opts.tools.is_empty() {
                return (Err(provider::failure::REQUEST.into()), r);
            }
            if self.greedy && !opts.no_calls {
                r.calls = vec![ToolCall { id: String::new(), name: "pomelo_jobs".into(), arguments: r#"{"view":"stats"}"#.into() }];
                return (Ok(()), r);
            }
            let res = provider::stream(Target::Mock { thinking: false }, messages, opts, |e| match e {
                Event::Content(t) => r.content.push_str(&t),
                Event::ToolCalls(c) => r.calls = c,
                _ => {}
            })
            .await;
            self.content.push_str(&r.content);
            (res, r)
        }
        fn lookup(&self, call: &ToolCall, budget: i64) -> data::Answer {
            data::run(&self.pomelo, &self.access, &call.name, &call.arguments, data::tests::TODAY, budget)
        }
        fn record(&self, position: usize, round: usize, call: &ToolCall, q: Query) {
            self.recorded.borrow_mut().push((position, round, call.clone(), q));
        }
        fn fold_usage(&self) {}
    }

    fn ask(host: &mut TestHost, question: &str) -> Result<(), String> {
        let tools = data::tools(&host.access);
        let request = Request { data: data::instructions(data::tests::TODAY, &host.access), prompt: question.into(), ..Default::default() };
        let messages = request.messages_with(0, question);
        tauri::async_runtime::block_on(answer_with_lookups(host, &tools, 1500, &Counter::for_target(&Target::Mock { thinking: false }), &request, messages, context::MOCK_WINDOW, 600))
    }

    #[test]
    fn a_reply_looks_things_up_then_answers() {
        let pomelo = crate::db::tests::memory_db();
        data::tests::seed_jobs(&pomelo);
        let mut h = TestHost::new(pomelo, data::Access::default());
        ask(&mut h, "这周该跟进哪些投递").unwrap();
        let rec = h.recorded.borrow();
        assert_eq!(rec.len(), 1);
        let (position, round, call, q) = &rec[0];
        assert_eq!((*position, *round, call.name.as_str(), q.state.as_str()), (0, 1, "pomelo_jobs", "ok"));
        assert_eq!(q.label, "投石问路 · 该跟进的投递（1 份，本周还有 1 份）");
        // the second request carries the call and its result; the answer quotes it
        assert_eq!(h.requests.len(), 2);
        let second = &h.requests[1].0;
        let n = second.len();
        assert_eq!((second[n - 2].role.as_str(), second[n - 2].tool_calls.len()), ("assistant", 1));
        assert_eq!((second[n - 1].role.as_str(), second[n - 1].tool_call_id.as_deref()), ("tool", Some(call.id.as_str())));
        assert!(second[n - 1].content.contains("甲公司"));
        assert!(h.content.contains("模拟回答") && h.content.contains("现在该跟进 1 份"), "{}", h.content);
        assert!(second[0].content.contains("【查数据】"));
    }

    #[test]
    fn a_cloud_model_is_told_what_it_cannot_see() {
        let pomelo = crate::db::tests::memory_db();
        data::tests::seed_finance(&pomelo);
        let access = data::Access { cloud: true, cloud_allowed: vec!["jobs".into()], ..Default::default() };
        let mut h = TestHost::new(pomelo, access);
        ask(&mut h, "总资产这半年怎么变的").unwrap();
        let q = &h.recorded.borrow()[0].3;
        assert_eq!((q.state.as_str(), q.label.as_str()), ("blocked", "钱来钱往 · 不发给云端，没看"));
        assert!(!h.requests[1].0.iter().any(|m| m.content.contains("¥")), "no amount reaches the cloud model");
        assert!(h.content.starts_with("钱来钱往不发给云端，这部分没看。"), "{}", h.content);
    }

    #[test]
    fn at_most_four_rounds_then_it_must_answer() {
        let pomelo = crate::db::tests::memory_db();
        let mut h = TestHost::new(pomelo, data::Access::default());
        h.greedy = true;
        ask(&mut h, "随便").unwrap();
        assert_eq!(h.requests.len(), data::MAX_ROUNDS + 1);
        assert!(h.requests[..data::MAX_ROUNDS].iter().all(|(_, o)| !o.no_calls && !o.tools.is_empty()));
        assert!(h.requests[data::MAX_ROUNDS].1.no_calls);
        let rec = h.recorded.borrow();
        assert_eq!(rec.iter().map(|r| r.0).collect::<Vec<_>>(), [0, 1, 2, 3]);
        // calls without an id get one (letters / digits, 9 long)
        assert!(rec.iter().all(|r| r.2.id.len() == 9 && r.2.id.chars().all(|c| c.is_ascii_alphanumeric())));
        // the reply's cap shrinks as results come in
        let caps: Vec<i64> = h.requests.iter().map(|(_, o)| o.max_tokens.unwrap()).collect();
        assert!(caps.windows(2).all(|w| w[1] < w[0]), "{caps:?}");
    }

    #[test]
    fn a_service_without_tools_gets_the_question_again() {
        let pomelo = crate::db::tests::memory_db();
        let mut h = TestHost::new(pomelo, data::Access::default());
        h.reject_tools = true;
        ask(&mut h, "你好").unwrap();
        assert_eq!(h.requests.len(), 2);
        assert!(h.requests[1].1.tools.is_empty());
        assert!(!h.requests[1].0.iter().any(|m| m.content.contains("【查数据】")));
        assert_eq!(h.recorded.borrow()[0].3.state, "unsupported");
        assert!(h.content.contains("排版自检"));
    }

    #[test]
    fn renamed_titles_are_kept() {
        let c = memory_db();
        let b = begin_run(&c, None, None, "q", "local:x", "X").unwrap();
        c.execute("UPDATE sessions SET title = '我的', title_source = 'user'", []).unwrap();
        reply(&c, &b, "a");
        begin_run(&c, Some(b.session_id), None, "另一个问题", "local:x", "X").unwrap();
        assert_eq!(sessions(&c).unwrap()[0].title, "我的");
    }
}
