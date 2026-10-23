//! Conversations (projects / attachments come in later phases):
//! - A session's displayed path starts at `active_root_id` and follows each message's
//!   `active_child_id`; versions are siblings. Damaged links fall back to the newest child.
//! - Sending appends a question and an empty streaming reply to the end of that path in one
//!   transaction (a new session is created in the same step: drafts never leave empty sessions),
//!   then one background task streams the reply: deltas go to the page, the text is checkpointed
//!   every 0.75 s, and the run is finished exactly once (completed / failed / cancelled /
//!   interrupted on quit). One reply (or summary) at a time per conversation; different conversations,
//!   定时任务 and assistants run side by side (cloud models answer together; the local server takes
//!   requests in turn, provider.rs `local_turn`, and a waiting reply shows 排队中).
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
//! - Writing (v0.4, act/): write tools only propose; proposals join the reply's card (saved and
//!   sent as it grows) and are not 「查了」 lookups. Confirming happens later, from the page. After the
//!   lookup rounds one more round offers only the write tools (looking up the record took them all —
//!   seen on the real model), then the reply must answer.
//! - Skills (v0.4 phase 6, skills/): the ones on and automatic are offered with every reply, the ones
//!   a question asks for (runs.skills, copied to its new versions) too. 联网 asked for: Rust searches
//!   with the question before the first round and puts the results after it (no `tool_choice:
//!   required` — a service that refuses it left nothing searched); the tool stays for more searches.
//!   Their results are web sources and 「查了」 entries. A 新书影 proposal takes its creator / country /
//!   类型 from 书单 / 影单检索 first.
//! - Citations (v0.4, cite.rs): retrieved passages and what 小拾's search finds are numbered as
//!   the reply goes (saved and sent as they grow); the reply cites them as [n].
//! - 知识库 (v0.4.5, knowledge.rs): its passages come like project file passages (the page retrieves
//!   them before sending); a new conversation keeps the draft's switch and libraries. A folder's
//!   passage has no stored copy, so reading in segments keeps no note row for it.
//! - Work (v0.4.5 阶段 9, work.rs): a question may carry a file to change (改文件; its new versions keep
//!   it); its reply proposes changes paragraph by paragraph into a card instead of answering. Any reply
//!   can be saved as Word.
//! - Sending / generating / switching are not undoable. No branch changes in a conversation while
//!   its reply or summary is written (they share its slot).

use super::act;
use super::knowledge;
use super::work;
use super::skills;
use super::cite::{self, Sources};
use super::context::{self, Counter, Request, Sizes, SummaryJob, Turn};
use super::data::{self, Query};
use super::files::{self, MessageFile};
use super::index::Passage;
use super::memory::{self, UsedMemory};
use super::reading::{self, Reader};
use super::instructions::{self, SessionSetting};
use super::provider::{self, Event, Image, Msg, Options, Target, ToolCall};
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
    /// Replies: what their run was given, numbered for its [n] citations.
    pub sources: Vec<cite::Source>,
    /// Replies: the confirmation card of what 小拾 proposed to write.
    pub card: Option<act::Card>,
    /// Questions asked in the 小窗: the page they went with.
    pub page: Option<Query>,
    /// Questions: the skills they asked for (「/书单检索」, 联网), as names.
    pub skills: Vec<String>,
    /// Questions: the file they asked to change (改文件).
    pub edit_file: Option<work::Target>,
    /// Replies: the changes they proposed to it.
    pub edit: Option<work::FileEdit>,
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
        sources: vec![],
        card: None,
        page: None,
        skills: vec![],
        edit_file: None,
        edit: None,
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
            m.edit_file = work::target_of(conn, m.id)?;
            if let Some(run) = m.run_id {
                m.page = data::page_of(conn, run)?;
                m.skills = skills_of(conn, run)?.iter().filter_map(|k| skills::skill(k)).map(|s| s.name.to_string()).collect();
            }
        } else if let Some(run) = m.run_id {
            m.notes = reading::notes_of(conn, run)?.into_iter().chain(super::yearly::notes_of(conn, run)?).map(|(label, content)| Note { label, content }).collect();
            m.memories = memory::used_of(conn, run)?;
            m.queries = data::lookups_of(conn, run)?;
            m.sources = cite::of_run(conn, run)?;
            m.card = act::card_of(conn, run)?;
            m.edit = work::edit_of(conn, run)?;
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
    /// The page it was asked on (the 小窗), and the page's own report.
    pub page: Option<(Query, String)>,
    /// Skills it asked for (keys, checked).
    pub skills: Vec<String>,
}

/// The skills a run's question asked for.
fn skills_of(conn: &Connection, run: i64) -> Result<Vec<String>, String> {
    let raw: Option<String> = conn.query_row("SELECT skills FROM runs WHERE id = ?1", [run], |r| r.get(0)).optional().map_err(err)?.flatten();
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
}

/// Where a new session goes and the draft's own instructions.
#[derive(Debug, Clone, Default)]
pub struct NewSession {
    pub project: Option<i64>,
    pub setting: SessionSetting,
    /// The draft's 「本会话关闭记忆」.
    pub memory_off: bool,
    /// The draft's 思考 switch is off.
    pub thinking_off: bool,
    /// The draft's 知识库 switch and libraries.
    pub knowledge: knowledge::Draft,
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
                "INSERT INTO sessions (title, project_id, instructions, preset_id, memory_disabled, thinking) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![NEW_TITLE, project, setting.content, setting.preset_id, new.memory_off, !new.thinking_off],
            )
            .map_err(err)?;
            let id = tx.last_insert_rowid();
            knowledge::save_draft(&tx, id, &new.knowledge)?;
            id
        }
    };
    let shown = path(&tx, session_id)?;
    let mut turns = vec![];
    for m in &shown {
        let queries = if m.role == "assistant" { data::queries_of_message(&tx, m.id)? } else { vec![] };
        turns.push(Turn { id: m.id, role: m.role.clone(), content: m.content.clone(), state: m.state.clone(), queries });
    }
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
    if let Some((q, report)) = &extras.page {
        data::save_page(&tx, run_id, report, q)?;
        request.page = Some(q.clone());
    }
    if !extras.skills.is_empty() {
        tx.execute("UPDATE runs SET skills = ?1 WHERE id = ?2", params![serde_json::to_string(&extras.skills).map_err(err)?, run_id]).map_err(err)?;
        request.skills = extras.skills.clone();
    }
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
    let mut turns = vec![];
    for m in chain {
        let queries = if m.role == "assistant" { data::queries_of_message(&tx, m.id)? } else { vec![] };
        turns.push(Turn { id: m.id, role: m.role, content: m.content, state: m.state, queries });
    }
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
            if let Some(t) = work::target_of(&tx, asked)? {
                work::record_target(&tx, id, &t)?;
            }
            id
        }
    };
    request.edit = work::target_of(&tx, asked)?;
    reading::record_passages(&tx, run_id, passages)?;
    memory::record_used(&tx, run_id, memories)?;
    // the question keeps the page it was asked on (an edit's copy shows on the new question)
    let asked_run: Option<i64> = tx.query_row("SELECT run_id FROM messages WHERE id = ?1", [asked], |r| r.get(0)).map_err(err)?;
    if let Some(from) = asked_run.filter(|r| *r != run_id) {
        request.page = data::copy_page(&tx, from, run_id)?;
        request.skills = skills_of(&tx, from)?;
        if !request.skills.is_empty() {
            tx.execute("UPDATE runs SET skills = (SELECT skills FROM runs WHERE id = ?1) WHERE id = ?2", [from, run_id]).map_err(err)?;
        }
    }
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
    /// The reply's numbered sources so far (all of them).
    Sources { list: Vec<cite::Source> },
    /// The reply's confirmation card, after each proposal.
    Card { card: act::Card },
    /// 改文件: the changes the reply proposes.
    Edit { edit: work::FileEdit },
    /// Waiting for the local model (it is answering something else) / its turn now.
    Queued { on: bool },
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
    /// Waiting for the local model's turn (also in the status).
    queued: AtomicBool,
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
    /// "local:<key>" | "cloud:<id>"
    model: String,
    job: Job,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
}

/// A reply or summary being written (the status: what the pages show and can stop).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveRun {
    pub run: i64,
    pub session: i64,
    /// reply | summary
    pub kind: &'static str,
    pub model: String,
    /// Reading material in segments: where it is.
    pub reading: Option<Reading>,
    /// Waiting for the local model.
    pub queued: bool,
}

/// Model work being written: one slot per conversation (a reply or a summary).
#[derive(Default)]
pub struct Runs {
    active: Mutex<Vec<Active>>,
}

impl Runs {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Active>> {
        self.active.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Anything at all being written.
    pub fn busy(&self) -> bool {
        !self.lock().is_empty()
    }
    /// The conversation's slot is taken.
    pub fn busy_in(&self, session_id: i64) -> bool {
        self.lock().iter().any(|a| a.session_id == session_id)
    }
    /// Some run uses `model` ("cloud:<id>"), or with "local", any local model.
    pub fn using(&self, model: &str) -> bool {
        self.lock().iter().any(|a| if model == "local" { a.model.starts_with("local:") } else { a.model == model })
    }
    pub fn holds(&self, run_id: i64) -> bool {
        self.lock().iter().any(|a| a.run_id == run_id)
    }
    pub fn list(&self) -> Vec<ActiveRun> {
        self.lock()
            .iter()
            .map(|a| {
                let (kind, reading, queued) = match &a.job {
                    Job::Reply(s) => ("reply", s.reading.lock().unwrap_or_else(|e| e.into_inner()).clone(), s.queued.load(Ordering::SeqCst)),
                    Job::Summary(_) => ("summary", None, false),
                };
                ActiveRun { run: a.run_id, session: a.session_id, kind, model: a.model.clone(), reading, queued }
            })
            .collect()
    }
    pub fn occupy(&self, run_id: i64, session_id: i64, model: &str, job: Job) {
        self.lock().push(Active { run_id, session_id, model: model.to_string(), job, task: None });
    }
    pub fn attach(&self, run_id: i64, task: tauri::async_runtime::JoinHandle<()>) {
        if let Some(a) = self.lock().iter_mut().find(|a| a.run_id == run_id) {
            a.task = Some(task);
        }
    }
    /// Frees `run_id`'s slot if it still holds one.
    pub fn release(&self, run_id: i64) {
        self.lock().retain(|a| a.run_id != run_id);
    }
    fn take(&self, run_id: i64) -> Option<Active> {
        let mut g = self.lock();
        let i = g.iter().position(|a| a.run_id == run_id)?;
        Some(g.remove(i))
    }
    fn take_all(&self) -> Vec<Active> {
        std::mem::take(&mut *self.lock())
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

/// The messages for the reply and its cap: the question with its material when that fits;
/// otherwise, with 读文件 offered (`reads_files`), the files' outline and the segments picked for the
/// question (the material sent: the last value); else notes from reading the material in segments.
async fn compose(app: &AppHandle, shared: &Shared, route: &Route, request: &Request, window: i64, reads_files: bool) -> Result<(Vec<provider::Msg>, i64, bool, Option<Vec<reading::Source>>), String> {
    let counter = Counter::for_target(&route.target);
    let sizes = Sizes::of(request, &counter).await;
    let plain = context::fit(request, &sizes, window)?; // the question itself must fit
    if request.sources.is_empty() {
        return Ok((plain.0, plain.1, false, None));
    }
    // images go to cloud models only; the text says which (a local model tells the user it can't see them)
    let vision = route.model.starts_with("cloud:");
    let sources: Vec<reading::Source> = request
        .sources
        .iter()
        .cloned()
        .map(|mut s| {
            if s.kind == reading::Kind::Image {
                s.content = (if vision { IMAGE_SENT } else { IMAGE_NOT_SEEN }).into();
            }
            s
        })
        .collect();
    let images = if vision { load_images(app, &sources).await? } else { vec![] };
    let image_cost = images.len() as i64 * IMAGE_TOKENS;
    let attach = |mut msgs: Vec<Msg>| {
        if let Some(m) = msgs.iter_mut().rev().find(|m| m.role == "user") {
            m.images = images.clone();
        }
        msgs
    };
    let with = |prompt: i64| Sizes { prompt: prompt + image_cost, ..sizes.clone() };
    let full = reading::prompt(&request.prompt, &sources);
    if let Some(sel) = context::select(&with(counter.count(&full).await), window) {
        return Ok((attach(request.messages_with(sel.dropped, &full)), sel.max_tokens, false, None));
    }
    // 读文件: the outline and the picked segments (smaller until they fit), the rest read with the tool
    if reads_files && sources.iter().any(|s| matches!(s.kind, reading::Kind::Attachment | reading::Kind::ProjectFile)) {
        let st = app.state::<AppState>();
        let kept: Vec<reading::Source> = sources.iter().filter(|s| !matches!(s.kind, reading::Kind::Attachment | reading::Kind::ProjectFile)).cloned().collect();
        let mut budget = window / 2;
        while budget >= 400 {
            let (directory, picked) = st.picky.with_db(|c| skills::files::excerpts(c, &request.prompt, &sources, budget))?;
            let mut sent = kept.clone();
            sent.extend(picked);
            let prompt = reading::prompt(&skills::files::long_prompt(&request.prompt, &directory), &sent);
            if let Some(sel) = context::select(&with(counter.count(&prompt).await), window) {
                return Ok((attach(request.messages_with(sel.dropped, &prompt)), sel.max_tokens, false, Some(sent)));
            }
            budget /= 2;
        }
    }
    let (pictures, sources): (Vec<reading::Source>, Vec<reading::Source>) = sources.into_iter().partition(|s| s.kind == reading::Kind::Image);
    let pictures = if pictures.is_empty() { String::new() } else { format!("\n\n{}", reading::prompt("", &pictures).trim()) };
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
            if artifact > 0 {
                let _ = app.state::<AppState>().picky.with_db(|c| reading::save_note(c, shared.run_id, artifact, ordinal, content));
            }
            artifacts_of(label, content);
        },
        usage: Default::default(),
        thinking: request.thinking,
    };
    let kinds = reading::kinds(&sources);
    let earlier = reading::earlier_names(&sources);
    let mut material = reader.read(&request.prompt, &sources).await?;
    let picked = loop {
        let notes = material.join("\n\n");
        let prompt = format!("{}{pictures}", reading::notes_prompt(&request.prompt, &notes, &kinds, &earlier));
        if let Some(sel) = context::select(&with(counter.count(&prompt).await), window) {
            break (attach(request.messages_with(sel.dropped, &prompt)), sel.max_tokens);
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
    Ok((picked.0, picked.1, true, None))
}

const IMAGE_SENT: &str = "（图片随这条消息附上）";
const IMAGE_NOT_SEEN: &str = "（现在用的是本地模型，看不了图片：只知道附了这张图，看不到内容。请在回答开头说明这一点，并提醒换成云端模型再问。）";
/// What an image takes in the window (a 1568-px image is about this many tokens).
const IMAGE_TOKENS: i64 = 1600;
/// Images sent with one question at most (the latest).
const MAX_IMAGES: usize = 8;

/// The question's images as sent (shrunk; the latest `MAX_IMAGES`).
async fn load_images(app: &AppHandle, sources: &[reading::Source]) -> Result<Vec<provider::Image>, String> {
    let st = app.state::<AppState>();
    let dir = st.picky.files_dir().to_path_buf();
    let ids: Vec<i64> = sources.iter().filter(|s| s.kind == reading::Kind::Image).map(|s| s.artifact).collect();
    let skip = ids.len().saturating_sub(MAX_IMAGES);
    let mut found = vec![];
    for id in &ids[skip..] {
        let (path, media) = st.picky.with_db(|c| Ok((files::path_of(c, &dir, *id)?, files::media_of(c, *id)?)))?;
        found.push((path, media));
    }
    let tmp = std::env::temp_dir().join("pomelo-picky-images");
    tauri::async_runtime::spawn_blocking(move || {
        found.iter().map(|(p, m)| super::office::image_for_model(p, m, &tmp).map(|(media_type, data)| provider::Image { media_type, data })).collect::<Result<Vec<_>, String>>()
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Big tables the question carries (attachments and project files): (artifact, name).
fn big_tables(conn: &Connection, sources: &[reading::Source]) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = vec![];
    for s in sources.iter().filter(|s| matches!(s.kind, reading::Kind::Attachment | reading::Kind::ProjectFile)) {
        if out.iter().any(|t| t.0 == s.artifact) {
            continue;
        }
        if let Ok(Some(f)) = files::info(conn, s.artifact) {
            if f.row_count.unwrap_or(0) > super::office::BIG_ROWS as i64 {
                out.push((f.id, f.name));
            }
        }
    }
    out
}

/// pomelo_table: rows of one of the question's big tables (read again from its stored copy).
fn table_lookup(app: &AppHandle, tables: &[(i64, String)], arguments: &str, budget: i64) -> data::Answer {
    let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
    let want = args["file"].as_str().unwrap_or("").trim().to_string();
    let pick = tables.iter().find(|(_, n)| !want.is_empty() && (*n == want || n.contains(&want) || want.contains(n.as_str()))).or(if tables.len() == 1 { tables.first() } else { None });
    let failed = |label: String, e: String| data::Answer { module: None, label, state: "failed", text: format!("没查成：{e}"), records: vec![] };
    let Some((id, name)) = pick else {
        return failed("查表 · 没有这个文件".into(), format!("要用 file 说是哪个表格（有：{}）", tables.iter().map(|t| t.1.as_str()).collect::<Vec<_>>().join("、")));
    };
    let st = app.state::<AppState>();
    let dir = st.picky.files_dir().to_path_buf();
    let result = st.picky.with_db(|c| files::path_of(c, &dir, *id)).and_then(|p| {
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        super::office::read_table(&p, &ext)
    });
    match result.and_then(|sheets| super::office::table_query(&sheets, &args)) {
        Ok(text) => {
            let mut lines: Vec<&str> = text.lines().collect();
            while lines.len() > 3 && context::estimate(&lines.join("\n")) > budget.min(data::MAX_RESULT) {
                lines.pop();
            }
            let cut = if lines.len() < text.lines().count() { "\n……后面的行放不下了，可以缩小范围再查" } else { "" };
            data::Answer { module: None, label: format!("查表 · {name}"), state: "ok", text: format!("{}{cut}", lines.join("\n")), records: vec![] }
        }
        Err(e) => failed(format!("查表 · {name} · 没查成"), e),
    }
}

/// What this answer may look up in Pomelo; None = nothing to offer (every module hidden…).
pub(crate) struct Lookup {
    access: data::Access,
    today: String,
    /// The writing part of the system message (act/; empty: nothing may be written).
    writing: String,
    /// The skills part (skills/), and whether recommendations get checked.
    skilled: String,
    verify: bool,
    /// The tools of the skills the question asked for.
    forced: Vec<String>,
    tools: Vec<serde_json::Value>,
    /// Tokens the definitions take.
    cost: i64,
    /// 读文件 is offered: long material goes as an outline and excerpts (skills/files.rs).
    reads_files: bool,
}

impl Lookup {
    /// The lookup part of the system message.
    pub(crate) fn instructions(&self) -> String {
        [data::instructions(&self.today, &self.access, self.verify), self.writing.clone(), self.skilled.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n")
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

/// Who may be read this time, and today (None: the databases couldn't say — then nothing is read).
pub(crate) fn access_for(app: &AppHandle, route: &Route) -> Option<(data::Access, String)> {
    let st = app.state::<AppState>();
    let (hidden, today, made) = st.with_db(|c| Ok((data::hidden(c), crate::modules::todo::today(c)?, data::made(c)))).ok()?;
    let keys: Vec<String> = made.iter().map(|m| m.key.clone()).collect();
    let allowed = st.picky.with_db(|c| Ok(data::cloud_allowed(c, &keys))).ok()?;
    Some((data::Access { hidden, cloud: route.model.starts_with("cloud:"), cloud_allowed: allowed, made }, today))
}

/// `asked`: the skills the question asked for (keys). `read_only`: no write tools (定时任务).
/// `extra`: tools of this question only (查表).
/// `ctx`: what the question has for 读文件 / 查知识库.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn lookup_plan(app: &AppHandle, access: data::Access, today: String, counter: &Counter, asked: &[String], read_only: bool, extra: Vec<serde_json::Value>, ctx: skills::Ctx) -> Option<Lookup> {
    let st = app.state::<AppState>();
    let mut offered = st.picky.with_db(|c| Ok(skills::offered(c, &st.picky, asked, ctx))).unwrap_or_default();
    if read_only {
        offered.retain(|s| !s.writes);
    }
    let reads_files = offered.iter().any(|s| s.key == skills::FILES);
    let mut tools = data::tools(&access);
    tools.extend(extra);
    if read_only {
        tools.retain(|t| !t["function"]["name"].as_str().is_some_and(act::is_act));
    }
    tools.extend(skills::tools(&offered));
    if tools.is_empty() {
        return None;
    }
    let writing = if read_only { String::new() } else { st.with_db(|c| act::instructions(c, &access, &today)).unwrap_or_default() };
    let forced: Vec<String> = offered.iter().filter(|s| asked.iter().any(|a| a == s.key)).map(|s| s.tool.to_string()).collect();
    let skilled = skills::instructions(&offered, asked);
    let cost = counter.count(&serde_json::to_string(&tools).unwrap_or_default()).await + TOOLS_OVERHEAD;
    Some(Lookup { access, today, writing, skilled, verify: skills::verifies(&offered), forced, tools, cost, reads_files })
}

/// What one request streamed besides the shared buffer.
#[derive(Default)]
struct Round {
    calls: Vec<ToolCall>,
    content: String,
    reasoning: String,
    /// OpenRouter's reasoning blocks (go back with the calls).
    details: Vec<serde_json::Value>,
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
            Event::ReasoningDetails(d) => r.details = d,
            Event::Usage { prompt, completion } => b.usage = Some((prompt, completion)),
            Event::Finished(_) => {}
            Event::Queued(on) => {
                shared.queued.store(on, Ordering::SeqCst);
                let _ = shared.channel.send(StreamEvent::Queued { on });
                app.state::<AppState>().picky.changed();
            }
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
    /// Runs one call within `budget` tokens.
    async fn lookup(&mut self, call: &ToolCall, budget: i64) -> data::Answer;
    /// Saves a lookup with the run and shows it.
    fn record(&self, position: usize, round: usize, call: &ToolCall, q: Query);
    /// A round that asked for lookups: its usage joins the total.
    fn fold_usage(&self);
    /// Images a lookup looked at (查知识库's look), sent after the round's results.
    fn take_images(&mut self) -> Vec<Image> {
        vec![]
    }
}

struct AppHost<'a> {
    app: &'a AppHandle,
    shared: &'a Shared,
    target: Target,
    last: Instant,
    access: data::Access,
    today: String,
    sources: Sources,
    /// The question as asked (date words for proposals).
    question: String,
    /// Its big tables (pomelo_table).
    tables: Vec<(i64, String)>,
    /// Its files (读文件) and the 知识库 it may search (查知识库).
    materials: Vec<skills::files::Material>,
    kb: Option<knowledge::Scope>,
    /// What Word 工具箱 / 文本工具箱 may change.
    word_files: Vec<skills::toolbox::Editable>,
    text_files: Vec<skills::toolbox::Editable>,
    /// 查知识库's look: images waiting to be sent, and how many this reply looked at.
    images: Vec<Image>,
    looked: usize,
}

impl AppHost<'_> {
    /// A write tool: checked, then into the reply's card (sent to the page). A 新书影 gets its
    /// creator / country / 类型 from 书单 / 影单检索 first (when on).
    async fn propose(&self, call: &ToolCall) -> data::Answer {
        if let Some(refusal) = act::refused(&self.access, &call.name, &call.arguments) {
            return refusal;
        }
        let st = self.app.state::<AppState>();
        let arguments = match act::work_to_look_up(&call.name, &call.arguments) {
            Some((module, title, hint)) => act::with_work(&call.arguments, skills::work(self.app, module, &title, hint.as_deref()).await.as_ref()),
            None => call.arguments.clone(),
        };
        let proposal = st.with_db(|c| act::propose(c, &call.name, &arguments, &self.question, &self.today));
        let added = proposal.and_then(|p| {
            let n = st.picky.with_db(|c| act::add(c, self.shared.run_id, &p))?;
            if let Ok(Some(card)) = st.picky.with_db(|c| act::card_of(c, self.shared.run_id)) {
                let _ = self.shared.channel.send(StreamEvent::Card { card });
            }
            Ok((n, p))
        });
        act::answer(&call.name, added.as_ref().map(|(n, p)| (*n, p)).map_err(|e| e.clone()))
    }
}

/// Saves the sources from `from` on and sends the whole list.
fn publish(app: &AppHandle, shared: &Shared, sources: &Sources, from: usize) {
    if sources.0.len() <= from {
        return;
    }
    let _ = app.state::<AppState>().picky.with_db(|c| sources.0[from..].iter().try_for_each(|s| cite::save(c, shared.run_id, s)));
    let _ = shared.channel.send(StreamEvent::Sources { list: sources.0.clone() });
}

impl Host for AppHost<'_> {
    async fn stream(&mut self, messages: Vec<Msg>, opts: Options) -> (Result<(), String>, Round) {
        stream_round(self.app, self.shared, &self.target, messages, opts, &mut self.last).await
    }
    async fn lookup(&mut self, call: &ToolCall, budget: i64) -> data::Answer {
        match call.name.as_str() {
            cite::SEARCH => {
                let before = self.sources.0.len();
                let a = cite::search(self.app, &self.access, &mut self.sources, &call.arguments, budget).await;
                publish(self.app, self.shared, &self.sources, before);
                a
            }
            cite::READ => cite::read(self.app, &self.access, &self.sources, &call.arguments, &self.today, budget),
            super::office::TABLE_TOOL => table_lookup(self.app, &self.tables, &call.arguments, budget),
            skills::files::TOOL => {
                let before = self.sources.0.len();
                let (mats, sources) = (&self.materials, &mut self.sources);
                let a = self.app.state::<AppState>().picky.with_db(|c| Ok(skills::files::run(c, mats, sources, &call.arguments, budget))).unwrap_or_else(|e| data::Answer { module: None, label: "读文件 · 没读成".into(), state: "failed", text: format!("没读成：{e}"), records: vec![] });
                publish(self.app, self.shared, &self.sources, before);
                a
            }
            skills::KB_TOOL => {
                let before = self.sources.0.len();
                let args: serde_json::Value = serde_json::from_str(&call.arguments).unwrap_or_default();
                let a = match &self.kb {
                    // a cloud model looks at an image (at most MAX_IMAGES a reply)
                    Some(scope) if args["view"] == "look" && matches!(self.target, Target::Cloud { .. }) => {
                        if self.looked >= MAX_IMAGES {
                            data::Answer { module: None, label: "知识库 · 看图 · 太多了".into(), state: "failed", text: format!("这条回答已经看了 {MAX_IMAGES} 张图片，不能再看了。"), records: vec![] }
                        } else {
                            let (a, image) = knowledge::look(self.app, scope, &mut self.sources, &args).await;
                            if let Some(i) = image {
                                self.looked += 1;
                                self.images.push(i);
                            }
                            a
                        }
                    }
                    Some(scope) => knowledge::kb_tool(self.app, scope, &mut self.sources, &call.arguments, budget).await,
                    None => data::Answer { module: None, label: "查知识库 · 没有可查的".into(), state: "failed", text: "这个对话没有可以查的知识库。".into(), records: vec![] },
                };
                publish(self.app, self.shared, &self.sources, before);
                a
            }
            skills::toolbox::WORD_TOOL | skills::toolbox::TEXT_TOOL => {
                let word = call.name == skills::toolbox::WORD_TOOL;
                let files = if word { &self.word_files } else { &self.text_files };
                let st = self.app.state::<AppState>();
                let (a, changed) = st
                    .picky
                    .with_db(|c| Ok(skills::toolbox::run(c, self.shared.run_id, files, word, &call.arguments, budget)))
                    .unwrap_or_else(|e| (data::Answer { module: None, label: "工具箱 · 没成".into(), state: "failed", text: format!("没成：{e}"), records: vec![] }, false));
                if changed {
                    if let Ok(Some(edit)) = st.picky.with_db(|c| work::edit_of(c, self.shared.run_id)) {
                        let _ = self.shared.channel.send(StreamEvent::Edit { edit });
                    }
                }
                a
            }
            name if act::is_act(name) => self.propose(call).await,
            name if skills::is_tool(name) => {
                let before = self.sources.0.len();
                let a = skills::run(self.app, &mut self.sources, name, &call.arguments, budget).await;
                publish(self.app, self.shared, &self.sources, before);
                a
            }
            _ => self
                .app
                .state::<AppState>()
                .with_db(|c| Ok(data::run(c, &self.access, &call.name, &call.arguments, &self.today, budget)))
                .unwrap_or_else(|e| data::Answer { module: None, label: "没查成".into(), state: "failed", text: format!("没查成：{e}"), records: vec![] }),
        }
    }
    fn record(&self, position: usize, round: usize, call: &ToolCall, q: Query) {
        if act::is_act(&call.name) {
            return; // proposals show as the card, not as 「查了」
        }
        let _ = self.app.state::<AppState>().picky.with_db(|c| data::save_query(c, self.shared.run_id, position, round, call, &q));
        let _ = self.shared.channel.send(StreamEvent::Query(q));
    }
    fn fold_usage(&self) {
        let mut b = self.shared.buf.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((p, c)) = b.usage.take() {
            b.extra = (b.extra.0 + p, b.extra.1 + c);
        }
    }
    fn take_images(&mut self) -> Vec<Image> {
        std::mem::take(&mut self.images)
    }
}

/// Answers, looking things up for up to `data::MAX_ROUNDS` rounds (then the request must answer).
/// `used`: what `messages` take; `cost`: the definitions. Ok = completed.
/// `forced`: the tools of the skills the question asked for; `question`: as asked (联网 searches it).
#[allow(clippy::too_many_arguments)]
async fn answer_with_lookups(host: &mut impl Host, tools: &[serde_json::Value], forced: &[String], question: &str, cost: i64, counter: &Counter, request: &Request, mut messages: Vec<Msg>, window: i64, mut used: i64) -> Result<(), String> {
    let (mut round, mut position) = (0usize, 0usize);
    let writes: Vec<serde_json::Value> = tools.iter().filter(|t| t["function"]["name"].as_str().is_some_and(|n| act::is_act(n) || skills::toolbox::is_tool(n))).cloned().collect();
    // 联网 asked for: the question searched first, the results after it
    if forced.iter().any(|f| f == skills::WEB_TOOL) {
        let q: String = question.trim().chars().take(300).collect();
        let call = ToolCall { id: "web0".into(), name: skills::WEB_TOOL.into(), arguments: serde_json::json!({ "query": q }).to_string() };
        let a = host.lookup(&call, (window - used - cost - context::reserve(window)).max(0)).await;
        host.record(position, 0, &call, Query::of(&a));
        position += 1;
        if let Some(last) = messages.last_mut() {
            let block = format!("\n\n{}{}\n{}", skills::SEARCHED_FIRST, q, a.text);
            used += counter.count(&block).await;
            last.content.push_str(&block);
        }
    }
    loop {
        // lookups and writes, then writes only, then the answer
        let (answer_now, offered) = match round {
            r if r < data::MAX_ROUNDS => (false, tools.to_vec()),
            r if r == data::MAX_ROUNDS && !writes.is_empty() => (false, writes.clone()),
            _ => (true, tools.to_vec()),
        };
        let names: Vec<&str> = offered.iter().filter_map(|t| t["function"]["name"].as_str()).collect();
        let opts = Options { max_tokens: Some(window - used - cost), thinking: Some(request.thinking), effort: None, search: false, tools: offered.clone(), no_calls: answer_now, one_off: false };
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
        messages.push(Msg { role: "assistant".into(), content: r.content, tool_calls: calls.clone(), reasoning: Some(r.reasoning), reasoning_details: r.details, ..Default::default() });
        let room = window - used - cost - context::reserve(window);
        let mut spent = 0;
        for (i, call) in calls.iter().enumerate() {
            let budget = (room - spent) / (calls.len() - i) as i64 - context::PER_MESSAGE;
            let answer = if budget < 64 {
                data::Answer { module: None, label: "放不下更多查到的内容了".into(), state: "failed", text: "上下文放不下更多查到的内容了，请根据已经查到的回答。".into(), records: vec![] }
            } else if !names.contains(&call.name.as_str()) {
                data::Answer { module: None, label: "不能再查了".into(), state: "failed", text: "已经查了好几轮，不能再查了，请根据已经查到的回答。".into(), records: vec![] }
            } else {
                host.lookup(call, budget).await
            };
            let c = counter.count(&answer.text).await + context::PER_MESSAGE;
            used += c;
            spent += c;
            messages.push(Msg::tool(&call.id, &answer.text));
            host.record(position, round, call, Query::of(&answer));
            position += 1;
        }
        // images looked at: a user message after the results (tool results carry text only)
        let images = host.take_images();
        if !images.is_empty() {
            let n = images.len() as i64;
            messages.push(Msg { role: "user".into(), content: format!("（这是刚才用 pomelo_kb 看的 {n} 张图片）"), images, ..Default::default() });
            used += n * IMAGE_TOKENS + context::PER_MESSAGE;
            spent += n * IMAGE_TOKENS;
        }
        if room - spent < MIN_ROOM {
            round = data::MAX_ROUNDS + 1;
        }
    }
}

async fn generate(app: AppHandle, shared: Arc<Shared>, route: Route, mut request: Request) {
    let window = context::window(&route).await;
    {
        let mut b = shared.buf.lock().unwrap_or_else(|e| e.into_inner());
        b.window = Some(window);
        if !request.notice.is_empty() {
            b.content = request.notice.clone();
            let _ = shared.channel.send(StreamEvent::Content { text: request.notice.clone() });
        }
    }
    // 年度总结: the year read whole (or month by month) instead of answering with lookups
    if let Some(year) = request.yearly.clone() {
        let _ = app.state::<AppState>().picky.with_db(|c| super::yearly::mark_year(c, shared.run_id, &year));
        let result = match access_for(&app, &route) {
            Some((access, today)) => {
                let mut sink = YearSink { app: &app, shared: &shared, target: route.target.clone(), last: Instant::now() };
                super::yearly::compose(&mut sink, &route.target, (&route.model, &route.name), window, request.thinking, &access, &year, &today, &request.system, &request.prompt).await
            }
            None => Err("读不到柚儿园的数据。".to_string()),
        };
        return match result {
            Ok(()) => finalize(&app, &shared, "completed", None),
            Err(e) => finalize(&app, &shared, "failed", Some(e)),
        };
    }
    let counter = Counter::for_target(&route.target);
    let question = request.prompt.clone(); // before the page joins it
    let tables = app.state::<AppState>().picky.with_db(|c| Ok(big_tables(c, &request.sources))).unwrap_or_default();
    // 读文件: the question's files; 查知识库: the conversation's libraries under this model's rules
    let materials = skills::files::materials(&request.sources);
    let kb = app
        .state::<AppState>()
        .picky
        .with_db(|c| {
            let session: i64 = c.query_row("SELECT session_id FROM runs WHERE id = ?1", [shared.run_id], |r| r.get(0)).map_err(err)?;
            knowledge::tool_scope(c, session, route.model.starts_with("cloud:"))
        })
        .ok()
        .flatten();
    // Word 工具箱 / 文本工具箱: the files with an original (attachments from disk, an older question's
    // file) and the 知识库's
    let (word_files, text_files) = {
        let st = app.state::<AppState>();
        let lists = |word: bool| -> Vec<skills::toolbox::Editable> {
            st.picky.with_db(|c| Ok(st.search.with_db(|s| Ok(skills::toolbox::editable(c, Some(s), &request.sources, request.edit.as_ref(), kb.as_ref(), word))).unwrap_or_else(|_| skills::toolbox::editable(c, None, &request.sources, request.edit.as_ref(), None, word)))).unwrap_or_default()
        };
        (lists(true), lists(false))
    };
    let ctx = skills::Ctx { files: !materials.is_empty(), kb: kb.is_some(), text: !text_files.is_empty() };
    // earlier lookups go with their questions only where this model may still see them
    let mut lookup = match access_for(&app, &route) {
        Some((access, today)) => {
            request.with_lookups(|q| access.may_send(q));
            request.with_page(|q| access.may_send(q));
            let extra = if tables.is_empty() { vec![] } else { vec![super::office::table_tool()] };
            lookup_plan(&app, access, today, &counter, &request.skills, request.read_only, extra, ctx).await
        }
        None => {
            request.looked.clear();
            request.with_page(|_| false);
            None
        }
    };
    if let Some(l) = &mut lookup {
        let files = skills::toolbox::note(&l.tools, &word_files, &text_files);
        if !files.is_empty() {
            l.skilled = format!("{}\n{files}", l.skilled);
        }
        request.data = l.instructions();
        // the definitions and room for results come first; a question that only fits without them goes without
        if context::select(&Sizes::of(&request, &counter).await, window - l.kept(window)).is_none() {
            request.data.clear();
            lookup = None;
        }
    }
    let kept = lookup.as_ref().map_or(0, |l| l.kept(window));
    let reads_files = lookup.as_ref().is_some_and(|l| l.reads_files);
    let (mut messages, max_tokens, read, sent) = match compose(&app, &shared, &route, &request, window - kept, reads_files).await {
        Ok(x) => x,
        Err(e) => return finalize(&app, &shared, "failed", Some(e)),
    };
    // passages sent with the question are [1]…; material read in segments went as notes instead
    let sources = if read { Sources::default() } else { Sources::of_material(sent.as_deref().unwrap_or(&request.sources)) };
    publish(&app, &shared, &sources, 0);
    let result = match lookup {
        Some(l) if !read => {
            let mut host = AppHost { app: &app, shared: &shared, target: route.target.clone(), last: Instant::now(), access: l.access, today: l.today, sources, question: question.clone(), tables, materials, kb, word_files, text_files, images: vec![], looked: 0 };
            answer_with_lookups(&mut host, &l.tools, &l.forced, &question, l.cost, &counter, &request, messages, window, window - kept - max_tokens).await
        }
        _ => {
            without_lookups(&mut messages, &request);
            let opts = Options { max_tokens: Some(max_tokens + kept), thinking: Some(request.thinking), ..Default::default() };
            stream_round(&app, &shared, &route.target, messages, opts, &mut Instant::now()).await.0
        }
    };
    match result {
        Ok(()) => finalize(&app, &shared, "completed", None),
        Err(e) => finalize(&app, &shared, "failed", Some(e)),
    }
}

/// 年度总结 into the run's reply: progress as reading, months as notes, the year streamed.
struct YearSink<'a> {
    app: &'a AppHandle,
    shared: &'a Shared,
    target: Target,
    last: Instant,
}

impl super::yearly::Sink for YearSink<'_> {
    fn with_pomelo<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        self.app.state::<AppState>().with_db(|c| f(c))
    }
    fn with_picky<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        self.app.state::<AppState>().picky.with_db(|c| f(c))
    }
    fn progress(&self, at: Option<(usize, usize, String)>) {
        let (current, total, label) = at.clone().unwrap_or((0, 0, String::new()));
        *self.shared.reading.lock().unwrap_or_else(|e| e.into_inner()) = at.map(|(current, total, label)| Reading { current, total, label });
        let _ = self.shared.channel.send(StreamEvent::Reading { current, total, label });
    }
    fn month(&self, month: &str, content: &str) {
        let _ = self.with_picky(|c| super::yearly::record(c, self.shared.run_id, month, content));
        let _ = self.shared.channel.send(StreamEvent::Note { label: super::data::month_label(month), content: content.to_string() });
    }
    fn asked(&self, usage: Option<(i64, i64)>) {
        if let Some((p, c)) = usage {
            let mut b = self.shared.buf.lock().unwrap_or_else(|e| e.into_inner());
            b.extra = (b.extra.0 + p, b.extra.1 + c);
        }
    }
    fn say(&self, text: &str) {
        let mut b = self.shared.buf.lock().unwrap_or_else(|e| e.into_inner());
        let piece = if b.content.is_empty() { text.to_string() } else { format!("\n\n{text}") };
        b.content.push_str(&piece);
        let _ = self.shared.channel.send(StreamEvent::Content { text: piece });
    }
    async fn article(&mut self, messages: Vec<Msg>, opts: Options) -> Result<(), String> {
        stream_round(self.app, self.shared, &self.target, messages, opts, &mut self.last).await.0
    }
}

/// App quitting: keep what arrived, marked interrupted.
pub fn shutdown(app: &AppHandle) {
    let st = app.state::<AppState>();
    for a in st.picky.runs.take_all() {
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
    let mut shown = state.picky.with_db(|c| path(c, session_id))?;
    let _ = state.with_db(|c| {
        shown.iter_mut().filter_map(|m| m.card.as_mut()).for_each(|card| act::mark_gone(c, card));
        Ok(())
    });
    Ok(shown)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    pub session_id: i64,
    pub run_id: i64,
    pub user_message_id: i64,
    pub message_id: i64,
}

const BUSY_HERE: &str = "这个对话正在生成回复，等它写完或先停止。";

/// What a new question / version needs before anything is saved: the conversation's slot is free
/// (None: a new one), and where it goes (the selected model) resolves.
fn prepare(app: &AppHandle, session_id: Option<i64>) -> Result<Route, String> {
    let st = app.state::<AppState>();
    if session_id.is_some_and(|s| st.picky.runs.busy_in(s)) {
        return Err(BUSY_HERE.into());
    }
    st.picky.target()
}

fn session_of_message(app: &AppHandle, message_id: i64) -> Result<i64, String> {
    app.state::<AppState>().picky.with_db(|c| c.query_row("SELECT session_id FROM messages WHERE id = ?1", [message_id], |r| r.get(0)).map_err(|_| "找不到这条消息，可能已经被删除了".to_string()))
}

/// Streams the begun reply over `on_event` (reasoning / content deltas, then one `done`).
pub(crate) fn start(app: &AppHandle, b: Begun, route: Route, on_event: Channel<StreamEvent>) -> Sent {
    let st = app.state::<AppState>();
    let shared = Arc::new(Shared { run_id: b.run_id, reply_id: b.reply_id, buf: Mutex::default(), done: AtomicBool::new(false), channel: on_event, reading: Mutex::default(), queued: AtomicBool::new(false) });
    st.picky.runs.occupy(b.run_id, b.session_id, &route.model, Job::Reply(shared.clone()));
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
    thinking: Option<bool>,
    text: String,
    attachments: Vec<i64>,
    files: Vec<i64>,
    passages: Vec<Passage>,
    memories: Vec<UsedMemory>,
    page: Option<data::PageContext>,
    skills: Option<Vec<String>>,
    knowledge: Option<knowledge::Draft>,
    on_event: Channel<StreamEvent>,
) -> Result<Sent, String> {
    let prompt = match text.trim() {
        "" if !attachments.is_empty() => "请总结这些附件。",
        "" => return Err("问题不能为空".into()),
        t => t,
    };
    let route = prepare(&app, session_id)?;
    let new = NewSession { project: project_id, setting: instructions.unwrap_or_default(), memory_off, thinking_off: thinking == Some(false), knowledge: knowledge.unwrap_or_default() };
    // the page it was asked on (the 小窗): Rust fetches what it may, under this model's rules
    let page = page.and_then(|ctx| {
        let (access, today) = access_for(&app, &route)?;
        let q = app.state::<AppState>().with_db(|c| Ok(data::page_query(c, &access, &today, &ctx))).ok().flatten()?;
        Some((q, serde_json::to_string(&ctx).unwrap_or_default()))
    });
    let st = app.state::<AppState>();
    let skills = st.picky.with_db(|c| skills::checked_forced(c, &st.picky, &skills.unwrap_or_default()))?;
    let extras = Extras { attachments, files, passages, memories, page, skills };
    let b = app.state::<AppState>().picky.with_db(|c| begin_run_in(c, session_id, &new, prompt, &route.model, &route.name, &extras))?;
    Ok(start(&app, b, route, on_event))
}

/// A new version of a question (edited text) and its reply.
#[tauri::command]
pub async fn picky_edit(app: AppHandle, message_id: i64, text: String, passages: Vec<Passage>, memories: Vec<UsedMemory>, on_event: Channel<StreamEvent>) -> Result<Sent, String> {
    let route = prepare(&app, Some(session_of_message(&app, message_id)?))?;
    let b = app.state::<AppState>().picky.with_db(|c| begin_version(c, message_id, Some(&text), &route.model, &route.name, &passages, &memories))?;
    Ok(start(&app, b, route, on_event))
}

/// A new version of a reply (regenerate, or retry a failed / stopped / interrupted one).
#[tauri::command]
pub async fn picky_regenerate(app: AppHandle, message_id: i64, passages: Vec<Passage>, memories: Vec<UsedMemory>, on_event: Channel<StreamEvent>) -> Result<Sent, String> {
    let route = prepare(&app, Some(session_of_message(&app, message_id)?))?;
    let b = app.state::<AppState>().picky.with_db(|c| begin_version(c, message_id, None, &route.model, &route.name, &passages, &memories))?;
    Ok(start(&app, b, route, on_event))
}

/// Shows the previous (-1) / next (+1) version of a message. Returns the shown version's id.
#[tauri::command]
pub fn picky_switch_version(state: State<AppState>, message_id: i64, delta: i64) -> Result<i64, String> {
    let session: Option<i64> = state.picky.with_db(|c| Ok(c.query_row("SELECT session_id FROM messages WHERE id = ?1", [message_id], |r| r.get(0)).ok()))?;
    if session.is_some_and(|s| state.picky.runs.busy_in(s)) {
        return Err("这个对话正在生成，等它写完再切换版本。".into());
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
    if let Some(a) = st.picky.runs.take(run_id) {
        end_active(&app, a, "cancelled");
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::picky::provider::Msg;
    use crate::picky::store::tests::memory_db;

    fn reply(c: &Connection, b: &Begun, text: &str) {
        finish(c, b.run_id, b.reply_id, &Outcome { content: text.into(), state: "completed", ..Default::default() }).unwrap();
    }

    /// One slot per conversation: others write beside it; the local model in use is known.
    #[test]
    fn a_slot_per_conversation() {
        let runs = Runs::default();
        let job = |run_id| Job::Reply(Arc::new(Shared { run_id, reply_id: run_id, buf: Mutex::default(), done: AtomicBool::new(false), channel: Channel::new(|_| Ok(())), reading: Mutex::default(), queued: AtomicBool::new(false) }));
        runs.occupy(1, 10, "cloud:claude", job(1));
        runs.occupy(2, 20, "local:qwen", job(2));
        assert!(runs.busy_in(10) && runs.busy_in(20) && !runs.busy_in(30));
        assert!(runs.using("local") && runs.using("cloud:claude") && !runs.using("cloud:deepseek"));
        assert_eq!(runs.list().iter().map(|a| (a.run, a.session, a.kind, a.queued)).collect::<Vec<_>>(), [(1, 10, "reply", false), (2, 20, "reply", false)]);
        runs.release(2);
        assert!(!runs.using("local") && runs.holds(1) && !runs.holds(2));
        assert!(runs.take(1).is_some() && !runs.busy());
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
        let new = NewSession { project: None, setting: SessionSetting { content: " 简短 ".into(), preset_id: Some(preset) }, memory_off: false, thinking_off: false, knowledge: Default::default() };
        let b = begin_run_in(&c, None, &new, "q", "local:x", "X", &Extras::default()).unwrap();
        assert_eq!(b.request.system, "全局\n\n简短\n\n像老师");
        assert_eq!(instructions::session_setting(&c, b.session_id).unwrap(), SessionSetting { content: "简短".into(), preset_id: Some(preset) });
        reply(&c, &b, "a");
        // a saved session keeps its own; a draft's choice only applies to a new one
        let b2 = begin_run_in(&c, Some(b.session_id), &NewSession::default(), "q2", "local:x", "X", &Extras::default()).unwrap();
        assert_eq!(b2.request.system, "全局\n\n简短\n\n像老师");
        // the draft's 知识库 switch and libraries come along
        let lib = knowledge::create_library(&c, "工作").unwrap();
        let new = NewSession { knowledge: knowledge::Draft { on: true, libraries: vec![lib, 999] }, ..Default::default() };
        let b3 = begin_run_in(&c, None, &new, "q3", "local:x", "X", &Extras::default()).unwrap();
        let k = knowledge::scope(&c, Some(b3.session_id), None, None, false).unwrap().unwrap();
        assert_eq!((k.libraries, knowledge::chosen(&c, knowledge::Owner::Session(b3.session_id)).unwrap()), (vec!["工作".to_string()], vec![lib]));
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
        let passage = Passage { file_id: 99, artifact_id: 98, segment_ordinal: 0, label: "别的.md · 第 1 段".into(), content: "检索到的".into(), kb: None };
        let extras = Extras { attachments: vec![att], files: vec![f], passages: vec![passage.clone()], memories: vec![], page: None, skills: vec![] };
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
        /// The first round asks for these.
        script: Vec<ToolCall>,
        /// 查知识库's look: images waiting for the next request; a real image to look at (else a stand-in).
        images: Vec<Image>,
        kb_image: Option<std::path::PathBuf>,
        /// Refuse requests with tools (like a model without tool support).
        reject_tools: bool,
        /// Mock by default; the real-model harness points it at 8080.
        target: Target,
        reasoning: String,
        /// The reply's numbered sources (pomelo_search here finds by words in a search.db made from `pomelo`).
        sources: Sources,
        /// What the write tools proposed (the card), and the question they read dates from.
        proposals: Vec<act::Proposal>,
        question: String,
        /// Skills the question asks for (keys).
        asked: Vec<String>,
        /// 读文件: picky.db with the question's files.
        picky: Option<Connection>,
        materials: Vec<skills::files::Material>,
        /// Word 工具箱 / 文本工具箱: what they may change, the reply whose card they fill (in `picky`).
        word_files: Vec<skills::toolbox::Editable>,
        text_files: Vec<skills::toolbox::Editable>,
        run_id: i64,
    }

    impl TestHost {
        fn new(pomelo: Connection, access: data::Access) -> Self {
            TestHost { pomelo, access, requests: vec![], recorded: Default::default(), content: String::new(), greedy: false, script: vec![], images: vec![], kb_image: None, reject_tools: false, target: Target::Mock { thinking: false }, reasoning: String::new(), sources: Sources::default(), proposals: vec![], question: String::new(), asked: vec![], picky: None, materials: vec![], word_files: vec![], text_files: vec![], run_id: 0 }
        }
    }

    impl Host for TestHost {
        async fn stream(&mut self, messages: Vec<Msg>, opts: Options) -> (Result<(), String>, Round) {
            self.requests.push((messages.clone(), opts.clone()));
            let mut r = Round::default();
            if self.reject_tools && !opts.tools.is_empty() {
                return (Err(provider::failure::REQUEST.into()), r);
            }
            if !self.script.is_empty() && !opts.no_calls {
                r.calls = std::mem::take(&mut self.script);
                return (Ok(()), r);
            }
            if self.greedy && !opts.no_calls {
                r.calls = vec![ToolCall { id: String::new(), name: "pomelo_jobs".into(), arguments: r#"{"view":"stats"}"#.into() }];
                r.details = vec![serde_json::json!({ "type": "reasoning.encrypted", "data": format!("sig{}", self.requests.len()) })];
                return (Ok(()), r);
            }
            let res = provider::stream(self.target.clone(), messages, opts, |e| match e {
                Event::Content(t) => r.content.push_str(&t),
                Event::Reasoning(t) => r.reasoning.push_str(&t),
                Event::ToolCalls(c) => r.calls = c,
                _ => {}
            })
            .await;
            self.content.push_str(&r.content);
            self.reasoning.push_str(&r.reasoning);
            (res, r)
        }
        async fn lookup(&mut self, call: &ToolCall, budget: i64) -> data::Answer {
            match call.name.as_str() {
                cite::SEARCH => {
                    let req = match cite::request(&self.access, &call.arguments) {
                        Ok(r) => r,
                        Err(a) => return a,
                    };
                    let s = crate::search::store::tests::memory_db();
                    for (i, src) in crate::search::sources::SOURCES.iter().enumerate().filter(|(_, x)| x.db == crate::search::sources::Db::Pomelo) {
                        crate::search::indexer::apply(&s, i, crate::search::sources::read(&self.pomelo, src).unwrap()).unwrap();
                    }
                    let mask = crate::search::query::allowed(&req.modules, &[], &self.access.hidden, self.access.cloud.then_some(&self.access.cloud_allowed[..]));
                    let exclude = crate::search::query::excluded(&data::custom_keys(&self.pomelo), &req.modules, &self.access.hidden, self.access.cloud.then_some(&self.access.cloud_allowed[..]));
                    let found = crate::search::query::found_by_words(&s, &req.text, mask, &exclude, 12).unwrap();
                    cite::results(&req.text, &crate::search::query::Answer { found, note: None }, &mut self.sources, budget)
                }
                skills::files::TOOL => match &self.picky {
                    Some(p) => skills::files::run(p, &self.materials, &mut self.sources, &call.arguments, budget),
                    None => data::Answer { module: None, label: "读文件 · 没有文件".into(), state: "failed", text: "没有文件".into(), records: vec![] },
                },
                skills::KB_TOOL if call.arguments.contains("files") && self.kb_image.is_some() => {
                    data::Answer { module: None, label: "知识库 · 文件（1 个）".into(), state: "ok", text: "知识库（资料）里一共 1 个文件：\n- 资料/The World This Week.png（图片，用 look 看 · 80 KB · 2026-10-18）".into(), records: vec![] }
                }
                skills::KB_TOOL if call.arguments.contains("look") => {
                    let image = match &self.kb_image {
                        Some(p) => {
                            let (media_type, data) = crate::picky::office::image_for_model(p, "image/png", &std::env::temp_dir().join("pomelo-test-images")).unwrap();
                            Image { media_type, data }
                        }
                        None => Image { media_type: "image/png".into(), data: "iVBOR".into() },
                    };
                    self.images.push(image);
                    data::Answer { module: None, label: "知识库 · 看图《白板.png》".into(), state: "ok", text: "[1] 图片《白板.png》附在下面那条消息里".into(), records: vec![] }
                }
                skills::toolbox::WORD_TOOL | skills::toolbox::TEXT_TOOL => {
                    let word = call.name == skills::toolbox::WORD_TOOL;
                    match &self.picky {
                        Some(p) => skills::toolbox::run(p, self.run_id, if word { &self.word_files } else { &self.text_files }, word, &call.arguments, budget).0,
                        None => data::Answer { module: None, label: "工具箱 · 没有文件".into(), state: "failed", text: "没有文件".into(), records: vec![] },
                    }
                }
                cite::READ => cite::read_with(&self.access, &self.sources, &call.arguments, budget, |_, kind, id| {
                    data::record_text(&self.pomelo, data::tests::TODAY, &data::PageRef { kind: kind.into(), id }).map(|(_, t, _)| t)
                }),
                // POMELO_REAL_WEB=1: real book / film lookups (web search stays mocked)
                name if skills::is_tool(name) => {
                    let reach = skills::works::Reach { tmdb: std::env::var("POMELO_TMDB_TOKEN").ok(), contact: std::env::var("POMELO_CONTACT").ok() };
                    skills::run_with(!real_web(), None, reach, &mut self.sources, name, &call.arguments, budget).await
                }
                name if act::is_act(name) => {
                    if let Some(r) = act::refused(&self.access, name, &call.arguments) {
                        return r;
                    }
                    let arguments = match act::work_to_look_up(name, &call.arguments) {
                        Some((module, title, hint)) => {
                            let which = if module == "books" { skills::works::Which::Books } else { skills::works::Which::Films };
                            act::with_work(&call.arguments, skills::works::find(which, &title, hint.as_deref(), skills::works::Reach::default(), !real_web()).await.ok().flatten().as_ref())
                        }
                        None => call.arguments.clone(),
                    };
                    match act::propose(&self.pomelo, name, &arguments, &self.question, data::tests::TODAY) {
                        Ok(p) => {
                            self.proposals.push(p);
                            let n = self.proposals.len();
                            act::answer(name, Ok((n, &self.proposals[n - 1])))
                        }
                        Err(e) => act::answer(name, Err(e)),
                    }
                }
                _ => data::run(&self.pomelo, &self.access, &call.name, &call.arguments, data::tests::TODAY, budget),
            }
        }
        fn record(&self, position: usize, round: usize, call: &ToolCall, q: Query) {
            self.recorded.borrow_mut().push((position, round, call.clone(), q));
        }
        fn fold_usage(&self) {}
        fn take_images(&mut self) -> Vec<Image> {
            std::mem::take(&mut self.images)
        }
    }

    fn real_web() -> bool {
        std::env::var("POMELO_REAL_WEB").as_deref() == Ok("1")
    }

    fn ask(host: &mut TestHost, question: &str) -> Result<(), String> {
        ask_in(host, question, context::MOCK_WINDOW)
    }

    fn ask_in(host: &mut TestHost, question: &str, window: i64) -> Result<(), String> {
        ask_after(host, Request::default(), question, window)
    }

    /// `before`: earlier turns (history + what they looked up), as chat.rs builds them.
    fn ask_after(host: &mut TestHost, before: Request, question: &str, window: i64) -> Result<(), String> {
        // skills as by default (books / films automatic), plus what the question asked for
        let picky = crate::picky::store::tests::memory_db();
        let all: Vec<&skills::Skill> = skills::SKILLS.iter().filter(|s| skills::switch(&picky, s.key).auto || host.asked.iter().any(|a| a == s.key)).collect();
        // POMELO_SCHEDULED=1: asked as a 定时任务 (read-only, its note in the system message)
        let scheduled = std::env::var("POMELO_SCHEDULED").as_deref() == Ok("1");
        let mut tools = data::tools(&host.access);
        if scheduled {
            tools.retain(|t| !t["function"]["name"].as_str().is_some_and(act::is_act));
        }
        tools.extend(skills::tools(&all));
        let forced: Vec<String> = all.iter().filter(|s| host.asked.iter().any(|a| a == s.key)).map(|s| s.tool.to_string()).collect();
        let writing = if scheduled { String::new() } else { act::instructions(&host.pomelo, &host.access, data::tests::TODAY).unwrap() };
        let data_text = [data::instructions(data::tests::TODAY, &host.access, skills::verifies(&all)), writing, skills::instructions(&all, &host.asked)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
        let mut request = Request { data: data_text, prompt: question.into(), ..before };
        if scheduled {
            let rule = crate::picky::schedule::Rule { every: "week".into(), day: Some(1), month: None, at: "09:00".into() };
            request.system = crate::picky::schedule::note("每周回顾", &rule, &format!("{} 09:00", data::tests::TODAY));
        }
        host.question = question.into();
        let access = host.access.clone();
        request.with_lookups(|q| access.may_send(q));
        // POMELO_PAGE='jobs:job:甲公司' (module:kind:title words): asked in the 小窗 on that record
        if let Some(spec) = std::env::var("POMELO_PAGE").ok().filter(|_| request.history.is_empty()) {
            let parts: Vec<&str> = spec.splitn(3, ':').collect();
            let id: i64 = host.pomelo.query_row("SELECT id FROM jobs WHERE company LIKE '%' || ?1 || '%' LIMIT 1", [parts.get(2).copied().unwrap_or("")], |r| r.get(0)).unwrap_or(0);
            let ctx = data::PageContext { module: parts[0].into(), page: Some("投递".into()), record: Some(data::PageRef { kind: parts.get(1).copied().unwrap_or("job").into(), id }), ..Default::default() };
            request.page = data::page_query(&host.pomelo, &host.access, data::tests::TODAY, &ctx);
            request.with_page(|q| access.may_send(q));
        }
        let messages = request.messages_with(0, question);
        let est = |s: &str| context::estimate(s);
        let cost = est(&serde_json::to_string(&tools).unwrap()) + TOOLS_OVERHEAD;
        let used = context::PROMPT_OVERHEAD + messages.iter().map(|m| est(&m.content) + context::PER_MESSAGE).sum::<i64>();
        tauri::async_runtime::block_on(answer_with_lookups(host, &tools, &forced, question, cost, &Counter::for_target(&host.target), &request, messages, window, used))
    }

    pub(crate) fn real_pomelo() -> Connection {
        real_db()
    }

    /// The real import files (local only) with made-up finance and jobs around 2026-10-12.
    fn real_db() -> Connection {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype");
        let mut c = crate::db::tests::memory_db();
        let scratch = crate::db::tests::temp_dir("real-llm");
        for f in ["pomelo-import.json", "todo-import.json", "jobs-import.json", "notes-import.json"] {
            if let Ok(text) = std::fs::read_to_string(dir.join(f)) {
                crate::transfer::apply(&mut c, &text, true, &scratch).unwrap();
            }
        }
        data::tests::seed_jobs(&c);
        data::tests::seed_finance(&c);
        seed_more(&c);
        c
    }

    /// Made-up data only (what a cloud model may be sent in a test).
    fn made_up_db() -> Connection {
        let c = crate::db::tests::memory_db();
        data::tests::seed_jobs(&c);
        data::tests::seed_finance(&c);
        seed_more(&c);
        c
    }

    /// What the import files lack for the seven questions: holdings, recent task activity, a trip.
    fn seed_more(c: &Connection) {
        use crate::modules::{finance, packing, todo};
        let v = |j: serde_json::Value| j;
        let ib = finance::create_account(c, serde_json::from_value(v(serde_json::json!({ "name": "盈透", "kind": "broker", "currency": "USD" }))).unwrap()).unwrap().id;
        let h = |n: &str, q: f64, p: f64| -> finance::HoldingInput { serde_json::from_value(serde_json::json!({ "name": n, "quantity": q, "price": p })).unwrap() };
        finance::save_holdings(c, ib, "2026-05-01", &[h("VOO", 10.0, 480.0), h("AAPL", 20.0, 190.0)]).unwrap();
        finance::save_holdings(c, ib, "2026-10-01", &[h("VOO", 12.0, 530.0), h("AAPL", 20.0, 170.0), h("NVDA", 5.0, 150.0)]).unwrap();
        let bal = |id: i64, amount: i64| -> finance::BalanceInput { serde_json::from_value(serde_json::json!({ "accountId": id, "amount": amount })).unwrap() };
        let usd = |r: f64| -> finance::RateInput { serde_json::from_value(serde_json::json!({ "currency": "USD", "rate": r })).unwrap() };
        finance::save_balances(c, "2026-05-01", &[bal(ib, 860_000)], &[usd(7.2)]).unwrap();
        finance::save_balances(c, "2026-10-01", &[bal(ib, 1_051_000)], &[usd(7.1)]).unwrap();

        let g = todo::create_group(c, "毕业").unwrap().id;
        let goal = |name: &str| todo::create_goal(c, serde_json::from_value(serde_json::json!({ "groupId": g, "name": name, "endDate": "2026-12-31" })).unwrap()).unwrap().id;
        let (thesis, gym) = (goal("论文初稿"), goal("考雅思"));
        let task = |title: &str, goal: Option<i64>, due: Option<&str>| todo::create_task_on(c, serde_json::from_value(serde_json::json!({ "groupId": g, "goalId": goal, "title": title, "dueDate": due })).unwrap(), "2026-09-01").unwrap().id;
        for (t, day) in [("读完三篇相关文献", "2026-10-06"), ("整理实验数据", "2026-10-08"), ("和导师开会", "2026-10-09")] {
            let id = task(t, Some(thesis), None);
            todo::set_done_on(c, id, true, day).unwrap();
            c.execute("UPDATE todo_tasks SET done_at = ?1 WHERE id = ?2", (format!("{day}T06:00:00Z"), id)).unwrap();
        }
        task("交开题报告", Some(thesis), Some("2026-10-01"));
        task("背单词 List 5", Some(gym), Some("2026-09-20"));
        let old = task("整理书架", None, None);
        c.execute("UPDATE todo_tasks SET created_at = '2026-08-20T04:00:00Z' WHERE id = ?1", [old]).unwrap();

        if let Some(tp) = packing::templates(c).unwrap().first() {
            let trip = packing::create_trip(c, serde_json::from_value(serde_json::json!({ "name": "东京五天", "departOn": "2026-10-24" })).unwrap(), Some(tp.id)).unwrap().id;
            for it in packing::items(c, packing::List::Trip, trip).unwrap().into_iter().take(3) {
                packing::delete_item(c, packing::List::Trip, it.id).unwrap();
            }
        }
    }

    /// The real local model (8080) through the lookup loop: what it looked up and what it said.
    /// Opt-in: POMELO_REAL_LLM=1 [POMELO_MODEL=qwen3.5-9b] [POMELO_AS_CLOUD=1] [POMELO_ASK='问题1|问题2']
    ///   cargo test real_local_model -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_local_model() {
        let cloud = cloud_target();
        if std::env::var("POMELO_REAL_LLM").as_deref() != Ok("1") && cloud.is_none() {
            return;
        }
        let model = std::env::var("POMELO_MODEL").unwrap_or_else(|_| "qwen3.5-9b".into());
        let window = match &cloud {
            Some((_, w)) => *w,
            None => tauri::async_runtime::block_on(provider::local_window()).unwrap_or(16384),
        };
        // '|' separates conversations, '>>' follow-ups in one conversation
        let asks = std::env::var("POMELO_ASK").unwrap_or_else(|_| "这周该跟进哪些投递|总资产这半年怎么变的".into());
        for conversation in asks.split('|') {
            let mut access = data::Access::default();
            if std::env::var("POMELO_AS_CLOUD").as_deref() == Ok("1") {
                access = data::Access { cloud: true, cloud_allowed: data::MODULES.iter().filter(|m| m.key != "finance").map(|m| m.key.to_string()).collect(), ..access };
            }
            // a cloud model gets made-up data only (the import files are real)
            let mut h = TestHost::new(if cloud.is_some() { made_up_db() } else { real_db() }, access);
            // a made-up note to change (the real ones aren't read here)
            h.pomelo.execute("INSERT INTO notes (content, created_at) VALUES ('周末去看了两套房，押一付三，月租 4500。#租房', '2026-10-11T03:00:00.000Z')", []).unwrap();
            h.target = cloud.as_ref().map_or_else(|| Target::Local { model: model.clone() }, |c| c.0.clone());
            h.asked = std::env::var("POMELO_SKILLS").map(|s| s.split(',').map(String::from).collect()).unwrap_or_default();
            let mut before = Request::default();
            for q in conversation.split(">>") {
                (h.requests, h.content, h.reasoning) = (vec![], String::new(), String::new());
                h.recorded.borrow_mut().clear();
                let t = std::time::Instant::now();
                let proposed = h.proposals.len();
                let r = ask_after(&mut h, before.clone(), q, window);
                println!("\n==================== {q}  ({:.1}s, {} requests, {r:?})", t.elapsed().as_secs_f64(), h.requests.len());
                for (_, round, call, q) in h.recorded.borrow().iter() {
                    println!("  [{round}] {} {} → {} ({})", call.name, call.arguments, q.label, q.state);
                    if std::env::var("POMELO_SHOW").as_deref() == Ok("1") {
                        println!("{}", q.result);
                    }
                }
                for p in &h.proposals[proposed..] {
                    for c in &p.choices {
                        let fields: Vec<String> = c.fields.iter().map(|f| match &f.before { Some(b) => format!("{} {b} → {}", f.label, f.after), None => format!("{} {}", f.label, f.after) }).collect();
                        println!("  ▸ 卡片 {}：{}  [{}]", p.label, c.title, fields.join("；"));
                    }
                }
                println!("--- reasoning: {} chars\n{}", h.reasoning.chars().count(), h.content);
                // this turn joins the history with what it looked up
                let queries: Vec<Query> = h.recorded.borrow().iter().map(|r| r.3.clone()).collect();
                before.history.push(Msg::new("user", q));
                if !queries.is_empty() {
                    before.looked.push((before.history.len() - 1, queries));
                }
                before.history.push(Msg::new("assistant", &h.content));
            }
        }
    }

    /// 读文件 with the real local model: a long made-up manual goes as its outline and the passages that
    /// share words with the question; the answer is in a chapter only the outline points to.
    ///   POMELO_REAL_LLM=1 [POMELO_THINK=1] [POMELO_ASK='问题1|问题2'] cargo test real_read_file -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_read_file() {
        if std::env::var("POMELO_REAL_LLM").as_deref() != Ok("1") {
            return;
        }
        let model = std::env::var("POMELO_MODEL").unwrap_or_else(|_| "qwen3.5-9b".into());
        let window = tauri::async_runtime::block_on(provider::local_window()).unwrap_or(16384);
        let topics = ["垃圾分类", "快递代收", "停车位", "电梯维保", "水电缴费", "宠物管理", "装修报备", "访客登记", "消防通道", "绿化养护", "噪音投诉", "公共维修"];
        let mut manual = String::from("# 物业手册\n\n这是小区物业整理的住户手册，按章节列出常见事项。\n\n");
        for i in 1..=60 {
            let (title, body) = match i {
                47 => ("进不了门怎么办".to_string(), "如果把自己关在门外，先别着急：工作日 8 点到 20 点可以到物业前台借备用门卡；夜里打值班的王师傅电话 138-0000-4827，他十五分钟内会到。换锁需要带身份证到前台登记。".to_string()),
                52 => ("年底大扫除".to_string(), "每年 12 月 20 日前后，物业会安排外墙清洗和楼道消毒，届时请关好窗户，阳台上不要晾衣服。".to_string()),
                _ => (format!("{}（{}）", topics[i % topics.len()], i), format!("关于{}的规定：请住户按照物业公告执行，有疑问可在工作时间咨询前台。", topics[i % topics.len()]).repeat(6)),
            };
            manual.push_str(&format!("## 第 {i} 章 · {title}\n\n{body}\n\n"));
        }
        let asks = std::env::var("POMELO_ASK").unwrap_or_else(|_| "我把自己锁在外面了，大半夜该找谁？|年底物业会做什么，我要注意什么？".into());
        for q in asks.split('|') {
            let picky = crate::picky::store::tests::memory_db();
            let dir = crate::db::tests::temp_dir("real-read-file");
            let a = files::tests::add(&picky, &dir, "物业手册.md", manual.as_bytes());
            let sources = reading::sources(&picky, &[], &[a], &[], &[]).unwrap();
            let total: i64 = sources.iter().map(|s| context::estimate(&s.content)).sum();
            let (directory, picked) = skills::files::excerpts(&picky, q, &sources, window / 3).unwrap();
            println!("\n==================== {q}\nmanual ≈ {total} tokens, window {window}; picked: {:?}", picked.iter().map(|p| p.label.clone()).collect::<Vec<_>>());
            let prompt = reading::prompt(&skills::files::long_prompt(q, &directory), &picked);
            let mut h = TestHost::new(crate::db::tests::memory_db(), data::Access::default());
            h.target = Target::Local { model: model.clone() };
            h.materials = skills::files::materials(&sources);
            h.picky = Some(picky);
            h.sources = Sources::of_material(&picked);
            let tools = vec![skills::files::tool()];
            let think = std::env::var("POMELO_THINK").as_deref() == Ok("1");
            let request = Request { data: skills::instructions(&[skills::skill(skills::FILES).unwrap()], &[]), prompt: prompt.clone(), thinking: think, ..Request::default() };
            let messages = request.messages_with(0, &prompt);
            let cost = context::estimate(&serde_json::to_string(&tools).unwrap()) + TOOLS_OVERHEAD;
            let used: i64 = messages.iter().map(|m| context::estimate(&m.content) + context::PER_MESSAGE).sum();
            let counter = Counter::for_target(&h.target);
            let t = std::time::Instant::now();
            let r = tauri::async_runtime::block_on(answer_with_lookups(&mut h, &tools, &[], q, cost, &counter, &request, messages, window, used));
            println!("({:.1}s, {} requests, {r:?})", t.elapsed().as_secs_f64(), h.requests.len());
            for (_, round, call, a) in h.recorded.borrow().iter() {
                println!("  [{round}] {} {} → {} ({})", call.name, call.arguments, a.label, a.state);
            }
            println!("--- answer (cites {:?})\n{}", cite::cited(&h.content, &h.sources.0), h.content);
        }
    }

    /// A case of real_toolbox: files (name, bytes, in the 知识库 rather than attached) and the
    /// questions of one conversation ('>>' between them).
    struct TbCase {
        name: &'static str,
        files: Vec<(String, Vec<u8>, bool)>,
        ask: String,
    }

    /// A .docx made by macOS (TextEdit's writer) from HTML: another program's XML than ours.
    fn cocoa_docx(dir: &std::path::Path, name: &str, html: &str) -> Vec<u8> {
        let src = dir.join(format!("{name}.html"));
        let out = dir.join(format!("{name}.docx"));
        std::fs::write(&src, html).unwrap();
        let ok = std::process::Command::new("textutil").args(["-convert", "docx", "-output"]).arg(&out).arg(&src).status().unwrap();
        assert!(ok.success());
        std::fs::read(&out).unwrap()
    }

    fn toolbox_cases(dir: &std::path::Path) -> Vec<TbCase> {
        let make = dir.join("made");
        std::fs::create_dir_all(&make).unwrap();
        let report = cocoa_docx(&make, "report", r#"<html><body>
<h1>SA62 Week 4 Report</h1>
<p><b>Name:</b> Alex Chen&nbsp;&nbsp;<b>Mentor:</b> Dr. Wang</p>
<p><b>1. Work Completed</b></p>
<ul><li>Finished the data-cleaning scripts for the FX-trading RPA project.</li><li>Joined three requirement meetings with the business users and wrote the minutes.</li><li>Fixed two bugs in the reconciliation bot reported by the operations team.</li></ul>
<p><b>2. Milestones to Be Met</b></p>
<ul><li>Complete the private-banking FX-trading RPA project and confirm completion with the mentor and business users.</li><li>Complete the carried-over AI-training deck support and the AI-platform usage data analysis.</li><li>Confirm next-stage RPA tasks and priorities with the mentor.</li></ul>
<p><b>3. Problems and Support Needed</b></p>
<p>The test environment was down for two days, which delayed <i>user acceptance testing</i>. I need access to the new <u>staging server</u> before Friday.</p>
<table border="1"><tr><td>Task</td><td>Status</td></tr><tr><td>Data cleaning</td><td>Done</td></tr><tr><td>UAT</td><td>Delayed</td></tr></table>
<p>Thanks for your support this week.</p>
</body></html>"#);
        let resume = crate::picky::word::markdown_to_docx("# 张小柚\n\n三年产品经验，负责过两个 App 从零到上线。我擅长和工程师一起把模糊的需求拆成能做的小步。\n\n## 工作经历\n\n**柚子科技** · 产品经理 · 2023–2026\n\n- 负责记账 App 的会员体系，付费转化提升 30%。\n- 带三个人的小组做用户访谈。\n\n## 技能\n\n熟悉数据分析，会用一点 SQL。\n\n| 技能 | 程度 |\n|---|---|\n| SQL | 熟练 |\n| Figma | 一般 |\n\n## 其他\n\n爱好：跑步、做饭。\n\n以上信息真实有效。", &[]).unwrap();
        let letter = crate::picky::word::markdown_to_docx("# 求职信\n\n您好：\n\n我在招聘网站上看到贵公司招聘产品经理，很感兴趣。我有三年产品经验，做过记账和健身两个 App。\n\n我希望能加入贵公司，和团队一起把产品做好。期待您的回复。\n\n张小柚\n\n2026 年 10 月 19 日", &[]).unwrap();
        let mut long = String::from("# 客户服务手册\n\n");
        for i in 1..=90 {
            long.push_str(&format!("第 {i} 条：客户来电时，先确认客户的姓名和订单号；{}。\n\n", ["客户情绪激动时先道歉", "客户要求退款时按流程登记", "客户投诉物流时联系仓库", "客户询问发票时转财务"][i % 4]));
        }
        let manual = crate::picky::word::markdown_to_docx(&long, &[]).unwrap();
        let plan = crate::picky::word::markdown_to_docx("# 十月计划\n\n| 项目 | 负责人 | 截止 |\n|---|---|---|\n| 官网改版 |  |  |\n| 年度总结 | 小王 | 10 月 25 日 |\n| 招聘 |  |  |\n\n有问题找小王。", &[]).unwrap();
        let questionnaire = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/archive/Questionnaire-round7.docx")).unwrap();
        let notes = "# 周末计划\n\n周六上午去超市，买牛奶、鸡蛋、面包。\n\n周六下午 打扫房间 洗衣服 给妈妈打电话\n\n周日看一部电影。\n\n（这段是草稿，可以删掉）\n".as_bytes().to_vec();
        let crlf = "第一段：这周的工作很多。\r\n\r\n第二段：周三之前要交季度报告，需要和财务确认数字。\r\n\r\n第三段：周五团建。\r\n".as_bytes().to_vec();
        let f = |n: &str, b: &Vec<u8>, kb: bool| (n.to_string(), b.clone(), kb);
        vec![
            TbCase { name: "translate", files: vec![f("SA62_Week4_Report.docx", &report, false)], ask: "把这份周报翻译成中文，格式不要变".into() },
            TbCase { name: "translate-file", files: vec![f("SA62_Week4_Report.docx", &report, false)], ask: "把这份 Word 直接改成中文版，格式不要变".into() },
            TbCase { name: "format", files: vec![f("简历.docx", &resume, false)], ask: "简历里：第一段的第一句加粗；「其他」改成「兴趣爱好」；在「熟悉数据分析」那段后面加一段「英语：CET-6，能读英文文档」；删掉最后一段「以上信息真实有效」".into() },
            TbCase { name: "kb", files: vec![f("求职信.docx", &letter, true)], ask: "知识库里那封求职信，把开头的称呼改成「尊敬的招聘经理：」，结尾那句写得更正式一点".into() },
            TbCase { name: "table", files: vec![f("简历.docx", &resume, false)], ask: "把简历表格里 Figma 的程度改成熟练".into() },
            TbCase { name: "table-fill", files: vec![f("十月计划.docx", &plan, false)], ask: "表格里空着的负责人都填成我（张小柚），空着的截止日期都写 10 月 31 日".into() },
            TbCase { name: "long", files: vec![f("客户服务手册.docx", &manual, false)], ask: "把这份手册里所有的「客户」都改成「用户」".into() },
            TbCase { name: "create", files: vec![], ask: "帮我写一份东京三天的行程（每天上午、下午、晚上），存成 Word".into() },
            TbCase { name: "two", files: vec![f("简历.docx", &resume, false), f("求职信.docx", &letter, false)], ask: "求职信里的「三年产品经验」改成「三年多产品经验」".into() },
            TbCase { name: "word-made", files: vec![f("问卷.docx", &questionnaire, false)], ask: "把问卷 0.3 题的题目改短一点，再把 1.2 题第二个选项的说明改得更清楚".into() },
            TbCase { name: "md", files: vec![f("周末计划.md", &notes, false)], ask: "把周六下午那段改成列表，删掉最后那段草稿".into() },
            TbCase { name: "crlf", files: vec![f("周报.txt", &crlf, false)], ask: "第二段写得更口语一点".into() },
            TbCase { name: "followup", files: vec![f("求职信.docx", &letter, false)], ask: "求职信第二段写得更具体一点>>第三段也改一下，语气更自信".into() },
            TbCase { name: "advice", files: vec![f("简历.docx", &resume, false)], ask: "帮我看看这份简历有什么问题".into() },
        ]
    }

    fn short(s: &str, n: usize) -> String {
        let one = s.replace('\n', "⏎");
        if one.chars().count() > n { format!("{}…", one.chars().take(n).collect::<String>()) } else { one }
    }

    fn run_toolbox_case(target: &Target, window: i64, dir: &std::path::Path, case: TbCase) {
        let here = dir.join(case.name);
        std::fs::create_dir_all(here.join("资料")).unwrap();
        let picky = memory_db();
        let (mut ids, mut kb_word, mut kb_text) = (vec![], vec![], vec![]);
        for (name, bytes, kb) in &case.files {
            let path = if *kb { here.join("资料").join(name) } else { here.join(name) };
            std::fs::write(&path, bytes).unwrap();
            if *kb {
                let e = skills::toolbox::Editable { name: format!("资料/{name}"), path: path.display().to_string(), from: "知识库 · 测试".into() };
                if name.ends_with(".docx") { kb_word.push(e) } else { kb_text.push(e) }
            } else {
                let a = files::tests::add(&picky, &here.join("store"), name, bytes);
                files::remember_source(&picky, a, &path).unwrap();
                ids.push(a);
            }
        }
        let sources = reading::sources(&picky, &[], &ids, &[], &[]).unwrap();
        let mut word = skills::toolbox::editable(&picky, None, &sources, None, None, true);
        word.extend(kb_word);
        let mut text = skills::toolbox::editable(&picky, None, &sources, None, None, false);
        text.extend(kb_text);
        // POMELO_TB_DUMP=1: how the files read (paragraphs as the model gets them), nothing asked
        if std::env::var("POMELO_TB_DUMP").as_deref() == Ok("1") {
            for (f, w) in word.iter().map(|f| (f, true)).chain(text.iter().map(|f| (f, false))) {
                let rid = begin_run(&picky, None, None, "x", "cloud:test", "Test").unwrap().run_id;
                println!("
==== [{}] {}
{}", case.name, f.name, skills::toolbox::run(&picky, rid, std::slice::from_ref(f), w, r#"{"view":"read"}"#, 100_000).0.text);
            }
            return;
        }
        let mut h = TestHost::new(crate::db::tests::memory_db(), data::Access::default());
        h.target = target.clone();
        h.materials = skills::files::materials(&sources);
        h.word_files = word;
        h.text_files = text.clone();
        let mut keys = vec![skills::BOOKS, skills::FILMS, skills::WORD];
        if !text.is_empty() {
            keys.push(skills::TEXT);
        }
        if !h.materials.is_empty() {
            keys.push(skills::FILES);
        }
        let all: Vec<&skills::Skill> = keys.iter().filter_map(|k| skills::skill(k)).collect();
        let mut tools = data::tools(&h.access);
        tools.extend(skills::tools(&all));
        let thinking = std::env::var("POMELO_THINK").as_deref() == Ok("1");
        let skilled = format!("{}\n{}", skills::instructions(&all, &[]), skills::toolbox::note(&tools, &h.word_files, &h.text_files));
        let data_text = [data::instructions(data::tests::TODAY, &h.access, skills::verifies(&all)), act::instructions(&h.pomelo, &h.access, data::tests::TODAY).unwrap(), skilled].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
        h.picky = Some(picky);
        let mut before = Request::default();
        for (turn, q) in case.ask.split(">>").enumerate() {
            let p = h.picky.as_ref().unwrap();
            h.run_id = begin_run(p, None, None, q, "cloud:test", "Test").unwrap().run_id;
            let shown: Vec<reading::Source> = sources.iter().cloned().map(|mut s| { s.earlier = turn > 0; s }).collect();
            let prompt = reading::prompt(q, &shown);
            let request = Request { data: data_text.clone(), prompt: prompt.clone(), thinking, ..before.clone() };
            let messages = request.messages_with(0, &prompt);
            let cost = context::estimate(&serde_json::to_string(&tools).unwrap()) + TOOLS_OVERHEAD;
            let used = context::PROMPT_OVERHEAD + messages.iter().map(|m| context::estimate(&m.content) + context::PER_MESSAGE).sum::<i64>();
            (h.requests, h.content, h.reasoning) = (vec![], String::new(), String::new());
            h.recorded.borrow_mut().clear();
            let t = std::time::Instant::now();
            let counter = Counter::for_target(&h.target);
            let r = tauri::async_runtime::block_on(answer_with_lookups(&mut h, &tools, &[], q, cost, &counter, &request, messages, window, used));
            println!("\n==================== [{}] {q}  ({:.1}s, {} requests, {r:?})", case.name, t.elapsed().as_secs_f64(), h.requests.len());
            for (_, round, call, a) in h.recorded.borrow().iter() {
                println!("  [{round}] {} {} → {} ({})", call.name, short(&call.arguments, 160), a.label, a.state);
                if a.state != "ok" || std::env::var("POMELO_SHOW").as_deref() == Ok("1") {
                    println!("      {}", short(&a.result, 400));
                }
            }
            println!("--- answer ({} chars reasoning)\n{}", h.reasoning.chars().count(), h.content.trim());
            let p = h.picky.as_ref().unwrap();
            match work::edit_of(p, h.run_id).unwrap() {
                None => println!("--- no card"),
                Some(card) if card.kind == work::NEW_WORD => {
                    println!("--- card: 新建 {}", card.name);
                    let saved = work::create_word(p, h.run_id, &here.display().to_string(), "").unwrap();
                    let doc = crate::picky::word::read_docx(&std::fs::read(&saved.path).unwrap()).unwrap();
                    println!("    saved {} ({} paragraphs): {}", saved.path, doc.items.len(), doc.items.iter().take(6).map(|i| short(&i.text, 40)).collect::<Vec<_>>().join(" | "));
                }
                Some(card) => {
                    println!("--- card: {} ({}, {} items)", card.name, card.kind, card.items.len());
                    for it in &card.items {
                        println!("    第 {} 段: {}\n        ⟶ {}", it.paragraph, short(&it.before, 120), short(&it.after, 160));
                    }
                    let positions: Vec<i64> = card.items.iter().map(|i| i.position).collect();
                    match work::write(p, h.run_id, &positions) {
                        Err(e) => println!("    ✗ write failed: {e}"),
                        Ok(_) => {
                            let doc = work::read(std::path::Path::new(&card.path)).unwrap();
                            let now: Vec<&str> = doc.items.iter().map(|(t, _)| t.trim_end()).collect();
                            let mut bad = 0;
                            for it in &card.items {
                                for piece in it.after.split("\n\n").map(|x| x.trim_matches('\n')).filter(|x| !x.trim().is_empty()) {
                                    if !now.contains(&piece) {
                                        bad += 1;
                                        println!("    ✗ not in the file as written: {}", short(piece, 120));
                                    }
                                }
                            }
                            let opens = std::process::Command::new("textutil").args(["-convert", "txt", "-stdout"]).arg(&card.path).output().map(|o| o.status.success()).unwrap_or(false);
                            println!("    written: {} paragraphs now, {} mismatches, opens in textutil: {opens} ({})", doc.items.len(), bad, card.path);
                        }
                    }
                }
            }
            let queries: Vec<Query> = h.recorded.borrow().iter().map(|r| r.3.clone()).collect();
            before.history.push(Msg::new("user", q));
            if !queries.is_empty() {
                before.looked.push((before.history.len() - 1, queries));
            }
            before.history.push(Msg::new("assistant", &h.content));
        }
    }

    /// A real cloud model for the opt-in tests: POMELO_CLOUD_KEY_FILE=<a file holding the key>
    /// [POMELO_CLOUD_SERVICE=deepseek|openrouter|openai|anthropic|google|aliyun] [POMELO_CLOUD_MODEL=<api model>]
    /// [POMELO_CLOUD_WINDOW=<context>] [POMELO_CLOUD_MAX_OUT=<max output>]. Never the keychain.
    pub(crate) fn cloud_target() -> Option<(Target, i64)> {
        let f = std::env::var("POMELO_CLOUD_KEY_FILE").ok()?;
        let key = std::fs::read_to_string(f).expect("the key file").trim().to_string();
        let mut m = crate::picky::cloud::defaults().remove(0);
        use crate::picky::cloud::Service;
        let service = match std::env::var("POMELO_CLOUD_SERVICE").as_deref() {
            Ok("openrouter") => Some(Service::OpenRouter),
            Ok("openai") => Some(Service::OpenAi),
            Ok("anthropic") => Some(Service::Anthropic),
            Ok("google") => Some(Service::Google),
            Ok("aliyun") => Some(Service::Aliyun),
            _ => None,
        };
        if let Some(s) = service {
            m.service = s;
            (m.context_window, m.max_output_tokens) = (262_144, 32_768);
        }
        if let Ok(api) = std::env::var("POMELO_CLOUD_MODEL") {
            (m.id, m.name, m.api_model) = ("test".into(), api.clone(), api);
        }
        if let Some(w) = std::env::var("POMELO_CLOUD_WINDOW").ok().and_then(|w| w.parse().ok()) {
            m.context_window = w;
        }
        if let Some(n) = std::env::var("POMELO_CLOUD_MAX_OUT").ok().and_then(|w| w.parse().ok()) {
            m.max_output_tokens = n;
        }
        // a model that can't stop thinking (GLM-5.3 on OpenRouter) or can't think
        match std::env::var("POMELO_CLOUD_THINK").as_deref() {
            Ok("always") => m.thinking = crate::picky::cloud::Think::Always,
            Ok("never") => m.thinking = crate::picky::cloud::Think::Never,
            _ => {}
        }
        let w = m.context_window;
        Some((Target::Cloud { model: m, key }, w))
    }

    /// 查知识库's look with a real cloud model: a made-up page as a PNG in the 知识库; it should list,
    /// look, and say what the image shows (see cloud_target for the switches).
    ///   POMELO_CLOUD_KEY_FILE=… cargo test real_kb_image -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_kb_image() {
        let Some((target, window)) = cloud_target() else { return };
        let dir = crate::db::tests::temp_dir("real-kb-image");
        let pdf = dir.join("page.pdf");
        std::fs::write(&pdf, crate::picky::pdf::tests::sample_pdf(&["The World This Week: the central bank held rates at 4.25%. A heatwave closed schools in Madrid."])).unwrap();
        let png = dir.join("The World This Week.png");
        assert!(std::process::Command::new("/usr/bin/sips").args(["-s", "format", "png"]).arg(&pdf).arg("--out").arg(&png).output().unwrap().status.success());
        let mut h = TestHost::new(crate::db::tests::memory_db(), data::Access::default());
        h.target = target;
        h.kb_image = Some(png);
        let q = std::env::var("POMELO_ASK").unwrap_or_else(|_| "知识库里那张 The World This Week 的图片上写了什么？".into());
        let r = ask_in(&mut h, &q, window);
        println!("\n==================== {q} ({} requests, {r:?})", h.requests.len());
        for (_, round, call, a) in h.recorded.borrow().iter() {
            println!("  [{round}] {} {} → {} ({})", call.name, call.arguments, a.label, a.state);
        }
        println!("--- answer\n{}", h.content);
    }

    /// Word 工具箱 / 文本工具箱 with a real model on made-up files in a temp folder (each case a
    /// conversation): what it called, the card, and the file after writing all of the card.
    ///   POMELO_CLOUD_KEY_FILE=<a file holding the key> [POMELO_CLOUD_MODEL=<api model>]  (DeepSeek)
    ///   or POMELO_REAL_LLM=1 [POMELO_MODEL=…]  (local);  [POMELO_TB_CASE=translate,kb,…] [POMELO_THINK=1] [POMELO_SHOW=1]
    ///   cargo test real_toolbox -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_toolbox() {
        let (target, window) = if let Some(t) = cloud_target() {
            t
        } else if std::env::var("POMELO_REAL_LLM").as_deref() == Ok("1") {
            let model = std::env::var("POMELO_MODEL").unwrap_or_else(|_| "qwen3.5-9b".into());
            (Target::Local { model }, tauri::async_runtime::block_on(provider::local_window()).unwrap_or(16384))
        } else if std::env::var("POMELO_TB_MOCK").as_deref() == Ok("1") {
            (Target::Mock { thinking: false }, context::MOCK_WINDOW)
        } else {
            return;
        };
        let pick = std::env::var("POMELO_TB_CASE").unwrap_or_else(|_| "all".into());
        let dir = crate::db::tests::temp_dir("real-toolbox");
        println!("files in {}", dir.display());
        for case in toolbox_cases(&dir) {
            if pick == "all" || pick.split(',').any(|p| p == case.name) {
                run_toolbox_case(&target, window, &dir, case);
            }
        }
    }

    /// 「我关于租房记过什么」: 小拾 searches across modules, the results are numbered and the answer cites them.
    #[test]
    fn a_search_numbers_what_it_found_and_the_answer_cites_it() {
        let pomelo = crate::db::tests::memory_db();
        pomelo.execute_batch(
            "INSERT INTO notes (content, created_at) VALUES ('周末去看了两套租房的公寓，押一付三', '2026-10-01T01:00:00.000Z'), ('读完了一本小说', '2026-10-02T01:00:00.000Z');
             INSERT INTO todo_groups (name, sort_order) VALUES ('生活', 1);
             INSERT INTO todo_tasks (group_id, title, sort_order) VALUES (1, '续签租房合同', 1);",
        )
        .unwrap();
        let mut h = TestHost::new(pomelo, data::Access::default());
        ask(&mut h, "我关于租房记过什么").unwrap();
        let rec = h.recorded.borrow();
        let (_, _, call, q) = &rec[0];
        assert_eq!((call.name.as_str(), call.arguments.as_str(), q.state.as_str()), (cite::SEARCH, r#"{"query":"租房"}"#, "ok"));
        assert_eq!(q.label, "搜索「租房」· 2 条");
        assert!(q.result.contains("[1] ") && q.result.contains("[2] ") && q.result.contains("押一付三"), "{}", q.result);
        assert_eq!(h.sources.0.len(), 2);
        assert!(h.sources.0.iter().all(|s| s.kind == "record" && s.module.is_some()));
        assert!(h.content.contains("[1][2]"), "{}", h.content);
        assert_eq!(cite::cited(&h.content, &h.sources.0), [1, 2]);
        // reading one in full
        let n = h.sources.0.iter().find(|s| s.record_kind.as_deref() == Some("task")).unwrap().n;
        let a = cite::read_with(&h.access, &h.sources, &format!(r#"{{"n":{n}}}"#), 2000, |_, kind, id| data::record_text(&h.pomelo, data::tests::TODAY, &data::PageRef { kind: kind.into(), id }).map(|(_, t, _)| t));
        assert!(a.state == "ok" && a.text.contains("续签租房合同") && a.text.contains("生活"), "{a:?}");
        assert_eq!(cite::read_with(&h.access, &h.sources, r#"{"n":9}"#, 2000, |_, _, _| Ok(String::new())).state, "failed");
        // a hidden module is neither found nor read
        let hidden = data::Access { hidden: vec!["notes".into()], ..data::Access::default() };
        let note = h.sources.0.iter().find(|s| s.module.as_deref() == Some("notes")).unwrap().n;
        assert_eq!(cite::read_with(&hidden, &h.sources, &format!(r#"{{"n":{note}}}"#), 2000, |_, _, _| Ok("x".into())).state, "blocked");
    }

    /// 「午饭 35」: 小拾 proposes (a card, nothing written, not a 「查了」); a cloud model is refused.
    #[test]
    fn writing_is_proposed_into_a_card_and_refused_for_the_cloud() {
        let pomelo = crate::db::tests::memory_db();
        pomelo.execute_batch("INSERT INTO todo_groups (name, sort_order) VALUES ('生活', 1);").unwrap();
        let mut h = TestHost::new(pomelo, data::Access::default());
        ask(&mut h, "午饭 35，昨天打车 20").unwrap();
        assert_eq!(h.proposals.len(), 2, "{}", h.content);
        let patch = |p: &act::Proposal| match &p.choices[0].op {
            act::Op::AddEntry { patch } | act::Op::AddTask { patch } => patch.clone(),
            other => panic!("{other:?}"),
        };
        let (lunch, taxi) = (&h.proposals[0], &h.proposals[1]);
        assert_eq!((patch(lunch)["amount"].as_i64(), patch(lunch)["day"].as_str()), (Some(3500), Some(data::tests::TODAY)));
        assert_eq!(lunch.choices[0].title, "10/12 午饭 ¥35");
        assert!(lunch.choices[0].fields.iter().any(|f| f.label == "分类" && f.after == "餐饮"));
        assert_eq!(patch(taxi)["day"].as_str(), Some(crate::modules::common::add_days(data::tests::TODAY, -1).as_str()));
        assert!(h.content.contains("确认卡片"), "{}", h.content);
        assert_eq!(h.pomelo.query_row("SELECT count(*) FROM fin_entries", [], |r| r.get::<_, i64>(0)).unwrap(), 0);

        ask(&mut h, "提醒我下周五前交开题报告").unwrap();
        let t = patch(h.proposals.last().unwrap());
        assert_eq!((t["title"].as_str(), t["dueDate"].as_str()), (Some("交开题报告"), Some("2026-10-23")));
        // an edit nothing fits: told to ask
        ask(&mut h, "把晚饭改成 40").unwrap();
        assert!(h.content.contains("没找到") && h.content.contains("请问用户"), "{}", h.content);

        let cloud = data::Access { cloud: true, cloud_allowed: vec!["todo".into()], ..data::Access::default() };
        let mut h = TestHost::new(crate::db::tests::memory_db(), cloud);
        ask(&mut h, "午饭 35").unwrap();
        assert!(h.proposals.is_empty());
        assert!(h.content.starts_with("钱来钱往不发给云端，这次不能记"), "{}", h.content);
    }

    /// 联网 / 「/书单检索」: the asked-for skill alone first (a call required), its pages numbered; on its
    /// own 小拾 checks a book with 书单检索; a 新书影 takes the lookup's creator and country.
    #[test]
    fn skills_asked_for_and_on_their_own() {
        let mut h = TestHost::new(crate::db::tests::memory_db(), data::Access::default());
        h.asked = vec![skills::WEB.into()];
        ask(&mut h, "今天有什么新闻").unwrap();
        // searched before the model was asked: the results go after the question; the tool stays
        let (first, opts) = &h.requests[0];
        assert!(first.last().unwrap().content.contains(skills::SEARCHED_FIRST) && first.last().unwrap().content.contains("[1] 模拟网页 1"));
        assert!(opts.tools.iter().any(|t| t["function"]["name"] == "pomelo_web_search"));
        let rec = h.recorded.borrow();
        assert_eq!((rec[0].2.name.as_str(), rec[0].2.arguments.as_str(), rec[0].3.module.as_deref()), ("pomelo_web_search", r#"{"query":"今天有什么新闻"}"#, Some(data::WEB)));
        assert!(h.sources.0.iter().all(|s| s.kind == "web" && s.url.is_some()) && h.sources.0.len() == 3);
        assert!(h.content.contains("[1]"), "{}", h.content);
        drop(rec);

        // not asked: web search isn't offered (not automatic), 书单检索 is and gets used
        let mut h = TestHost::new(crate::db::tests::memory_db(), data::Access::default());
        ask(&mut h, "查一下《三体》的作者").unwrap();
        assert!(!h.requests[0].1.tools.iter().any(|t| t["function"]["name"] == "pomelo_web_search"));
        assert!(h.requests[0].0[0].content.contains("推荐的每一部先用书单检索"));
        assert_eq!(h.recorded.borrow()[0].3.label, "书单检索「三体」 · 查到 1 个");
        assert_eq!(h.sources.0[0].url.as_deref(), Some("https://openlibrary.org/works/OL17267881W"));

        // a 新书影: author, country and 类型 from the lookup
        let mut h = TestHost::new(crate::db::tests::memory_db(), data::Access::default());
        ask(&mut h, "想读《三体》").unwrap();
        let p = h.proposals.last().expect("a card");
        let f = |l: &str| p.choices[0].fields.iter().find(|x| x.label == l).map(|x| x.after.clone());
        assert_eq!((f("作者").as_deref(), f("国家").as_deref(), f("类型").as_deref()), (Some("刘慈欣"), Some("中国"), Some("虚构")));
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

    /// 查知识库's look: the image goes in a user message after the round's results, then the answer.
    #[test]
    fn an_image_looked_at_follows_the_results() {
        let mut h = TestHost::new(crate::db::tests::memory_db(), data::Access::default());
        h.script = vec![ToolCall { id: "k1".into(), name: skills::KB_TOOL.into(), arguments: r#"{"view":"look","file":"白板.png"}"#.into() }];
        ask(&mut h, "知识库里的白板照片写了什么").unwrap();
        let sent = &h.requests[1].0;
        let n = sent.len();
        assert_eq!((sent[n - 2].role.as_str(), sent[n - 1].role.as_str(), sent[n - 1].images.len()), ("tool", "user", 1), "{sent:?}");
        assert!(sent[n - 1].content.contains("1 张图片"));
        // the window counts it
        assert!(h.requests[1].1.max_tokens.unwrap() <= h.requests[0].1.max_tokens.unwrap() - IMAGE_TOKENS);
    }

    #[test]
    fn at_most_four_rounds_then_it_must_answer() {
        let pomelo = crate::db::tests::memory_db();
        let mut h = TestHost::new(pomelo, data::Access::default());
        h.greedy = true;
        ask(&mut h, "随便").unwrap();
        assert_eq!(h.requests.len(), data::MAX_ROUNDS + 2);
        assert!(h.requests[..data::MAX_ROUNDS].iter().all(|(_, o)| !o.no_calls && !o.tools.is_empty()));
        // each round's reasoning blocks go back with its calls (OpenRouter: Gemini's thought signatures)
        let sent: Vec<String> = h.requests[2].0.iter().filter(|m| !m.tool_calls.is_empty()).map(|m| m.reasoning_details[0]["data"].as_str().unwrap().to_string()).collect();
        assert_eq!(sent, ["sig1", "sig2"]);
        // then a round for writing only — proposals and the toolboxes (the lookup it still asks for is
        // turned away), then the answer
        let last_call = &h.requests[data::MAX_ROUNDS].1;
        let names: Vec<&str> = last_call.tools.iter().map(|t| t["function"]["name"].as_str().unwrap()).collect();
        assert!(!last_call.no_calls && names.iter().all(|n| act::is_act(n) || skills::toolbox::is_tool(n)) && names.contains(&skills::toolbox::WORD_TOOL), "{names:?}");
        assert!(h.requests[data::MAX_ROUNDS + 1].1.no_calls);
        let rec = h.recorded.borrow();
        assert_eq!(rec.iter().map(|r| r.0).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);
        assert_eq!(rec[4].3.label, "不能再查了");
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
