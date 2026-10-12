//! Memories (from Picky's Memories / MemorySelection):
//! - One or two sentences (1–300 characters) the user asked 小拾 to remember: global, or one
//!   project's (optionally also used by other projects). A project can leave out global memories.
//!   A memory can be turned off, or kept 「仅本地」 (never sent to a cloud model). 「记于」 is the
//!   day it was noted (a changed text is noted again today). Creating, editing, switching and
//!   deleting are undoable; vectors are a cache (never undone).
//! - Which memories the next reply may use (`plan`): none when 使用记忆 is off in settings, when the
//!   conversation turned memories off, or when 「本条使用记忆」 is unticked; otherwise enabled ones in
//!   scope, without local-only ones for a cloud model. The preview and sending use the same rule.
//! - Before a question is saved, at most 6 of them (cosine ≥ 0.35 to the question, through 8081) are
//!   chosen; the reply's run keeps them as they read then (`run_memories`) and the reply lists them.
//!   They go after the instructions in the system message, with their dates. Reading material in
//!   segments gets the instructions only.
//! - Deleting a project keeps its memories: they become global and turned off.

use super::embed::{self, Input};
use super::store::{self, NOW};
use crate::modules::common::{err, Tx};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

pub const MAX_CHARS: usize = 300;
pub const LIMIT: usize = 6;
const MIN_SCORE: f32 = 0.35;
pub const ENABLED_KEY: &str = "memory.enabled";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: i64,
    pub content: String,
    pub project_id: Option<i64>,
    pub shared_project_ids: Vec<i64>,
    pub enabled: bool,
    pub local_only: bool,
    /// manual | command | import
    pub source: String,
    pub source_session_id: Option<i64>,
    pub noted_on: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Memory {
    /// Applies to a conversation in `project` (None = not in a project).
    pub fn applies(&self, project: Option<i64>, include_global: bool) -> bool {
        if !self.enabled {
            return false;
        }
        match (project, self.project_id) {
            (None, p) => p.is_none(),
            (Some(_), None) => include_global,
            (Some(p), Some(own)) => own == p || self.shared_project_ids.contains(&p),
        }
    }
}

/// What the editor saves.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInput {
    pub content: String,
    pub project_id: Option<i64>,
    #[serde(default)]
    pub shared_project_ids: Vec<i64>,
    pub enabled: bool,
    pub local_only: bool,
}

/// A memory as a reply's run received it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsedMemory {
    pub memory_id: Option<i64>,
    pub content: String,
    pub noted_on: String,
}

fn valid(content: &str) -> Result<String, String> {
    let t = content.trim();
    if t.is_empty() || t.chars().count() > MAX_CHARS {
        return Err(format!("记忆需要 1 至 {MAX_CHARS} 个字。"));
    }
    Ok(t.to_string())
}

fn today(conn: &Connection) -> Result<String, String> {
    conn.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0)).map_err(err)
}

pub fn list(conn: &Connection) -> Result<Vec<Memory>, String> {
    let mut st = conn.prepare("SELECT memory_id, project_id FROM memory_projects ORDER BY rowid").map_err(err)?;
    let shared: Vec<(i64, i64)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut st = conn
        .prepare("SELECT id, content, project_id, enabled, local_only, source, source_session_id, noted_on, created_at, updated_at FROM memories ORDER BY created_at DESC, id DESC")
        .map_err(err)?;
    let rows = st
        .query_map([], |r| {
            let id: i64 = r.get(0)?;
            Ok(Memory {
                id,
                content: r.get(1)?,
                project_id: r.get(2)?,
                shared_project_ids: shared.iter().filter(|s| s.0 == id).map(|s| s.1).collect(),
                enabled: r.get(3)?,
                local_only: r.get(4)?,
                source: r.get(5)?,
                source_session_id: r.get(6)?,
                noted_on: r.get(7)?,
                created_at: r.get(8)?,
                updated_at: r.get(9)?,
            })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

fn check_project(conn: &Connection, p: Option<i64>) -> Result<(), String> {
    if let Some(p) = p {
        conn.query_row("SELECT id FROM projects WHERE id = ?1", [p], |r| r.get::<_, i64>(0)).optional().map_err(err)?.ok_or("这个项目已经被删除了")?;
    }
    Ok(())
}

fn replace_sharing(conn: &Connection, id: i64, project: Option<i64>, shared: &[i64]) -> Result<(), String> {
    conn.execute("DELETE FROM memory_projects WHERE memory_id = ?1", [id]).map_err(err)?;
    if project.is_none() {
        return Ok(());
    }
    let mut done = vec![];
    for p in shared.iter().filter(|p| Some(**p) != project) {
        if done.contains(p) {
            continue;
        }
        check_project(conn, Some(*p))?;
        conn.execute("INSERT INTO memory_projects (memory_id, project_id) VALUES (?1, ?2)", [id, *p]).map_err(err)?;
        done.push(*p);
    }
    Ok(())
}

/// `source`: manual | command; `session`: where a /记住 came from.
pub fn create(conn: &Connection, m: &MemoryInput, source: &str, session: Option<i64>) -> Result<i64, String> {
    let content = valid(&m.content)?;
    let tx = Tx::begin(conn)?;
    check_project(&tx, m.project_id)?;
    tx.execute(
        "INSERT INTO memories (content, project_id, enabled, local_only, source, source_session_id, noted_on) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![content, m.project_id, m.enabled, m.local_only, source, session, today(&tx)?],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    replace_sharing(&tx, id, m.project_id, &m.shared_project_ids)?;
    tx.commit()?;
    Ok(id)
}

/// A changed text is noted again today.
pub fn update(conn: &Connection, id: i64, m: &MemoryInput) -> Result<(), String> {
    let content = valid(&m.content)?;
    let tx = Tx::begin(conn)?;
    let old: Option<String> = tx.query_row("SELECT content FROM memories WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(err)?;
    let old = old.ok_or("这条记忆已经不在了")?;
    check_project(&tx, m.project_id)?;
    let noted = if old == content { None } else { Some(today(&tx)?) };
    tx.execute(
        &format!("UPDATE memories SET content = ?1, project_id = ?2, enabled = ?3, local_only = ?4, noted_on = coalesce(?5, noted_on), updated_at = {NOW} WHERE id = ?6"),
        params![content, m.project_id, m.enabled, m.local_only, noted, id],
    )
    .map_err(err)?;
    replace_sharing(&tx, id, m.project_id, &m.shared_project_ids)?;
    tx.commit()
}

/// 启用 / 停用, 仅本地 on / off.
pub fn set_flag(conn: &Connection, id: i64, flag: &str, on: bool) -> Result<(), String> {
    let col = match flag {
        "enabled" => "enabled",
        "localOnly" => "local_only",
        _ => return Err(format!("不认识的开关：{flag}")),
    };
    match conn.execute(&format!("UPDATE memories SET {col} = ?1, updated_at = {NOW} WHERE id = ?2"), params![on, id]).map_err(err)? {
        0 => Err("这条记忆已经不在了".into()),
        _ => Ok(()),
    }
}

pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM memories WHERE id = ?1", [id]).map_err(err)? {
        0 => Err("这条记忆已经不在了".into()),
        _ => Ok(()),
    }
}

/// Deleting a project keeps its memories: global and turned off (call inside its transaction).
pub fn orphan(conn: &Connection, project: i64) -> Result<(), String> {
    conn.execute("DELETE FROM memory_projects WHERE memory_id IN (SELECT id FROM memories WHERE project_id = ?1)", [project]).map_err(err)?;
    conn.execute(&format!("UPDATE memories SET project_id = NULL, enabled = 0, updated_at = {NOW} WHERE project_id = ?1"), [project]).map_err(err)?;
    Ok(())
}

pub fn enabled_everywhere(conn: &Connection) -> Result<bool, String> {
    Ok(store::pref_get::<bool>(conn, ENABLED_KEY)?.unwrap_or(true))
}

pub fn conversation_off(conn: &Connection, session: i64) -> Result<bool, String> {
    Ok(conn.query_row("SELECT memory_disabled FROM sessions WHERE id = ?1", [session], |r| r.get(0)).optional().map_err(err)?.unwrap_or(false))
}

pub fn set_conversation_off(conn: &Connection, session: i64, off: bool) -> Result<(), String> {
    let found: Option<i64> = conn.query_row("SELECT id FROM sessions WHERE id = ?1", [session], |r| r.get(0)).optional().map_err(err)?;
    found.ok_or("找不到这个对话，可能已经被删除了")?;
    conn.execute("UPDATE sessions SET memory_disabled = ?1 WHERE id = ?2 AND memory_disabled <> ?1", params![off, session]).map(|_| ()).map_err(err)
}

// ---------- the plan ----------

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// everywhere (settings) | conversation | nextMessage
    pub off: Option<&'static str>,
    #[serde(skip)]
    pub candidates: Vec<Memory>,
    pub count: usize,
    pub global: usize,
    pub project: usize,
    pub shared: usize,
    /// Local-only ones left out for a cloud model.
    pub skipped_local_only: usize,
    /// The conversation (or draft) turned memories off.
    pub conversation_off: bool,
}

/// `draft_off`: a draft's own 「本会话关闭记忆」 (a saved conversation's comes from the database).
pub fn plan(conn: &Connection, session: Option<i64>, project: Option<i64>, cloud: bool, draft_off: bool, next_off: bool) -> Result<Plan, String> {
    let project = match session {
        Some(s) => conn.query_row("SELECT project_id FROM sessions WHERE id = ?1", [s], |r| r.get(0)).optional().map_err(err)?.flatten(),
        None => project,
    };
    let mut p = Plan { conversation_off: session.map_or(Ok(draft_off), |s| conversation_off(conn, s))?, ..Default::default() };
    if !enabled_everywhere(conn)? {
        p.off = Some("everywhere");
    } else if p.conversation_off {
        p.off = Some("conversation");
    } else if next_off {
        p.off = Some("nextMessage");
    }
    if p.off.is_some() {
        return Ok(p);
    }
    let include_global: bool = match project {
        Some(pid) => conn.query_row("SELECT include_global_memories FROM projects WHERE id = ?1", [pid], |r| r.get(0)).optional().map_err(err)?.unwrap_or(true),
        None => true,
    };
    for m in list(conn)?.into_iter().filter(|m| m.applies(project, include_global)) {
        if cloud && m.local_only {
            p.skipped_local_only += 1;
            continue;
        }
        if m.project_id.is_none() {
            p.global += 1;
        } else if m.project_id == project {
            p.project += 1;
        } else {
            p.shared += 1;
        }
        p.candidates.push(m);
    }
    p.count = p.candidates.len();
    Ok(p)
}

/// The system-message section for a reply's memories (empty for none).
pub fn section(used: &[UsedMemory]) -> String {
    if used.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = used.iter().map(|m| format!("- （记于 {}）{}", m.noted_on, m.content)).collect();
    format!("以下是用户明确让你记住的信息，括号内是记录日期。只在与当前问题相关时参考；信息可能已经过时，请结合日期判断，不要逐条复述。\n{}", lines.join("\n"))
}

/// Best first (ties keep their order), at most LIMIT scoring ≥ MIN_SCORE.
pub fn rank(query: &[f32], candidates: &[(Memory, Vec<f32>)]) -> Vec<UsedMemory> {
    let mut scored: Vec<(f32, usize, &Memory)> = candidates.iter().enumerate().map(|(i, (m, v))| (embed::cosine(query, v), i, m)).filter(|x| x.0 >= MIN_SCORE).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    scored.into_iter().take(LIMIT).map(|(_, _, m)| UsedMemory { memory_id: Some(m.id), content: m.content.clone(), noted_on: m.noted_on.clone() }).collect()
}

fn hash(content: &str) -> String {
    super::pdf::sha256_hex(content.as_bytes())
}

fn cached(conn: &Connection, model: &str) -> Result<Vec<(i64, String, Vec<f32>)>, String> {
    let mut st = conn.prepare("SELECT memory_id, content_hash, vector FROM memory_vectors WHERE model = ?1").map_err(err)?;
    let rows = st
        .query_map([model], |r| {
            let b: Vec<u8> = r.get(2)?;
            Ok((r.get(0)?, r.get(1)?, b.chunks_exact(4).map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]])).collect()))
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

fn save_vector(conn: &Connection, id: i64, model: &str, content_hash: &str, v: &[f32]) -> Result<(), String> {
    let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
    conn.execute(
        "INSERT INTO memory_vectors (memory_id, model, content_hash, vector) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(memory_id) DO UPDATE SET model = excluded.model, content_hash = excluded.content_hash, vector = excluded.vector",
        params![id, model, content_hash, bytes],
    )
    .map(|_| ())
    .map_err(err)
}

/// Embeds the question (and memories whose cached vector is missing or stale) and picks the best.
pub async fn choose(app: &AppHandle, question: &str, candidates: &[Memory]) -> Result<Vec<UsedMemory>, String> {
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    let st = app.state::<AppState>();
    let mode = st.picky.mode;
    let model = embed::model(mode);
    embed::prepare(app).await?;
    let have = st.picky.with_db(|c| cached(c, model))?;
    let mut vectors: Vec<Option<Vec<f32>>> = candidates.iter().map(|m| have.iter().find(|h| h.0 == m.id && h.1 == hash(&m.content)).map(|h| h.2.clone())).collect();
    let missing: Vec<usize> = (0..candidates.len()).filter(|i| vectors[*i].is_none()).collect();
    if !missing.is_empty() {
        let texts: Vec<String> = missing.iter().map(|i| candidates[*i].content.clone()).collect();
        let made = embed::embed(mode, &texts, Input::Document).await.map_err(String::from)?;
        if made.len() != missing.len() {
            return Err("向量服务返回的数量不对。".into());
        }
        st.picky.with_db(|c| {
            for (i, v) in missing.iter().zip(&made) {
                save_vector(c, candidates[*i].id, model, &hash(&candidates[*i].content), v)?;
            }
            Ok(())
        })?;
        for (i, v) in missing.into_iter().zip(made) {
            vectors[i] = Some(v);
        }
    }
    let q = embed::embed(mode, &[question.to_string()], Input::MemoryQuery).await.map_err(String::from)?.pop().ok_or("向量服务没有返回问题的向量。")?;
    let pairs: Vec<(Memory, Vec<f32>)> = candidates.iter().cloned().zip(vectors.into_iter().map(|v| v.unwrap_or_default())).collect();
    Ok(rank(&q, &pairs))
}

pub fn record_used(conn: &Connection, run_id: i64, used: &[UsedMemory]) -> Result<(), String> {
    for (i, m) in used.iter().enumerate() {
        conn.execute("INSERT INTO run_memories (run_id, position, memory_id, content, noted_on) VALUES (?1, ?2, ?3, ?4, ?5)", params![run_id, i as i64, m.memory_id, m.content, m.noted_on])
            .map_err(err)?;
    }
    Ok(())
}

pub fn used_of(conn: &Connection, run_id: i64) -> Result<Vec<UsedMemory>, String> {
    let mut st = conn.prepare("SELECT memory_id, content, noted_on FROM run_memories WHERE run_id = ?1 ORDER BY position").map_err(err)?;
    let rows = st.query_map([run_id], |r| Ok(UsedMemory { memory_id: r.get(0)?, content: r.get(1)?, noted_on: r.get(2)? })).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

// ---------- commands ----------

#[tauri::command]
pub fn picky_memories(state: State<AppState>) -> Result<Vec<Memory>, String> {
    state.picky.with_db(|c| list(c))
}

/// `id` None = a new memory (from 管理, or confirmed /记住 entries with `session`); returns its id.
#[tauri::command]
pub fn picky_memory_save(state: State<AppState>, id: Option<i64>, memory: MemoryInput) -> Result<i64, String> {
    match id {
        Some(id) => state.with_picky_undo("修改记忆", |c| update(c, id, &memory).map(|_| id)),
        None => state.with_picky_undo("新建记忆", |c| create(c, &memory, "manual", None)),
    }
}

/// Confirmed /记住 entries (saved together, one undo step).
#[tauri::command]
pub fn picky_remember(state: State<AppState>, entries: Vec<String>, project_id: Option<i64>, local_only: bool, session_id: Option<i64>) -> Result<(), String> {
    state.with_picky_undo("记住", |c| {
        let tx = Tx::begin(c)?;
        for e in &entries {
            create(&tx, &MemoryInput { content: e.clone(), project_id, enabled: true, local_only, shared_project_ids: vec![] }, "command", session_id)?;
        }
        tx.commit()
    })
}

#[tauri::command]
pub fn picky_memory_flag(state: State<AppState>, id: i64, flag: String, on: bool) -> Result<(), String> {
    let label = match (flag.as_str(), on) {
        ("enabled", true) => "启用记忆",
        ("enabled", false) => "停用记忆",
        (_, true) => "设为仅本地",
        _ => "允许云端使用",
    };
    state.with_picky_undo(label, |c| set_flag(c, id, &flag, on))
}

#[tauri::command]
pub fn picky_memory_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_picky_undo("删除记忆", |c| delete(c, id))
}

/// Settings › 使用记忆 (a setting: not undone).
#[tauri::command]
pub fn picky_memory_enabled(state: State<AppState>) -> Result<bool, String> {
    state.picky.with_db(|c| enabled_everywhere(c))
}

#[tauri::command]
pub fn picky_memory_enabled_set(state: State<AppState>, on: bool) -> Result<(), String> {
    state.picky.with_db(|c| store::pref_set(c, ENABLED_KEY, &on))
}

#[tauri::command]
pub fn picky_conversation_memory(state: State<AppState>, session_id: i64, off: bool) -> Result<(), String> {
    state.with_picky_undo(if off { "本会话关闭记忆" } else { "本会话使用记忆" }, |c| set_conversation_off(c, session_id, off))
}

fn is_cloud(state: &AppState) -> bool {
    state.picky.status().selection.starts_with("cloud:")
}

#[tauri::command]
pub fn picky_memory_plan(state: State<AppState>, session_id: Option<i64>, project_id: Option<i64>, draft_off: bool, next_off: bool) -> Result<Plan, String> {
    let cloud = is_cloud(&state);
    state.picky.with_db(|c| plan(c, session_id, project_id, cloud, draft_off, next_off))
}

/// The memories for a question (empty when none apply); an error means 8081 couldn't be used.
#[tauri::command]
pub async fn picky_choose_memories(app: AppHandle, session_id: Option<i64>, project_id: Option<i64>, draft_off: bool, next_off: bool, question: String) -> Result<Vec<UsedMemory>, String> {
    let st = app.state::<AppState>();
    let cloud = is_cloud(&st);
    let p = st.picky.with_db(|c| plan(c, session_id, project_id, cloud, draft_off, next_off))?;
    if p.off.is_some() {
        return Ok(vec![]);
    }
    choose(&app, question.trim(), &p.candidates).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::chat::begin_run;
    use crate::picky::library;
    use crate::picky::store::tests::memory_db;

    fn input(content: &str, project: Option<i64>) -> MemoryInput {
        MemoryInput { content: content.into(), project_id: project, enabled: true, ..Default::default() }
    }

    #[test]
    fn scopes_sharing_switches_and_plans() {
        let c = memory_db();
        let a = library::create_project(&c, "论文", "").unwrap();
        let b = library::create_project(&c, "求职", "").unwrap();
        let g = create(&c, &input(" 我住在新加坡 ", None), "manual", None).unwrap();
        let pa = create(&c, &MemoryInput { shared_project_ids: vec![b, a, b], ..input("论文用 APA 格式", Some(a)) }, "manual", None).unwrap();
        let local = create(&c, &MemoryInput { local_only: true, ..input("工资 1 万", None) }, "command", Some(9)).unwrap();
        assert!(create(&c, &input(&"长".repeat(301), None), "manual", None).is_err());
        let all = list(&c).unwrap();
        assert_eq!(all.iter().find(|m| m.id == pa).unwrap().shared_project_ids, [b]); // own project and repeats left out
        assert_eq!(all.iter().find(|m| m.id == g).unwrap().content, "我住在新加坡");

        // outside projects: global ones; a cloud model skips local-only ones
        let p = plan(&c, None, None, false, false, false).unwrap();
        assert_eq!((p.count, p.global), (2, 2));
        let p = plan(&c, None, None, true, false, false).unwrap();
        assert_eq!((p.count, p.skipped_local_only), (1, 1));
        // in project b: global + shared; project b may leave out global ones
        let p = plan(&c, None, Some(b), false, false, false).unwrap();
        assert_eq!((p.global, p.shared, p.project), (2, 1, 0));
        c.execute("UPDATE projects SET include_global_memories = 0 WHERE id = ?1", [b]).unwrap();
        assert_eq!(plan(&c, None, Some(b), false, false, false).unwrap().count, 1);
        // switches
        assert_eq!(plan(&c, None, None, false, false, true).unwrap().off, Some("nextMessage"));
        assert_eq!(plan(&c, None, None, false, true, false).unwrap().off, Some("conversation"));
        let s = begin_run(&c, None, None, "q", "local:x", "X").unwrap().session_id;
        set_conversation_off(&c, s, true).unwrap();
        assert_eq!(plan(&c, Some(s), None, false, false, false).unwrap().off, Some("conversation"));
        store::pref_set(&c, ENABLED_KEY, &false).unwrap();
        assert_eq!(plan(&c, None, None, false, false, false).unwrap().off, Some("everywhere"));
        store::pref_set(&c, ENABLED_KEY, &true).unwrap();
        set_flag(&c, local, "enabled", false).unwrap();
        assert_eq!(plan(&c, None, None, false, false, false).unwrap().count, 1);

        // a changed text is noted again; deleting the project keeps its memory, global and off
        c.execute("UPDATE memories SET noted_on = '2020-01-01' WHERE id = ?1", [pa]).unwrap();
        update(&c, pa, &MemoryInput { shared_project_ids: vec![b], ..input("论文用 APA 格式", Some(a)) }).unwrap();
        assert_eq!(list(&c).unwrap().iter().find(|m| m.id == pa).unwrap().noted_on, "2020-01-01");
        update(&c, pa, &input("论文用 MLA 格式", Some(a))).unwrap();
        assert_ne!(list(&c).unwrap().iter().find(|m| m.id == pa).unwrap().noted_on, "2020-01-01");
        library::delete_project(&c, a).unwrap();
        let m = list(&c).unwrap().into_iter().find(|m| m.id == pa).unwrap();
        assert_eq!((m.project_id, m.enabled, m.shared_project_ids.len()), (None, false, 0));
    }

    #[test]
    fn ranking_and_the_prompt_section() {
        let m = |id: i64, content: &str| (Memory { id, content: content.into(), project_id: None, shared_project_ids: vec![], enabled: true, local_only: false, source: "manual".into(), source_session_id: None, noted_on: "2026-10-01".into(), created_at: String::new(), updated_at: String::new() }, embed::mock_vector(content));
        let cands = vec![m(1, "周末喜欢去公园散步"), m(2, "论文用 APA 格式引用"), m(3, "论文的导师姓王")];
        let picked = rank(&embed::mock_vector("论文引用格式怎么写"), &cands);
        assert_eq!(picked.first().and_then(|u| u.memory_id), Some(2));
        assert!(picked.iter().all(|u| u.memory_id != Some(1)));
        assert_eq!(section(&[]), "");
        assert!(section(&picked).contains("- （记于 2026-10-01）论文用 APA 格式引用"));
    }

    #[test]
    fn chosen_memories_are_kept_with_the_run() {
        let c = memory_db();
        let b = begin_run(&c, None, None, "q", "local:x", "X").unwrap();
        let used = vec![UsedMemory { memory_id: Some(5), content: "住在新加坡".into(), noted_on: "2026-10-01".into() }];
        record_used(&c, b.run_id, &used).unwrap();
        assert_eq!(used_of(&c, b.run_id).unwrap(), used);
    }
}
