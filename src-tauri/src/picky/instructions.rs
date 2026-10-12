//! Instructions (from Picky's Instructions): four layers, each trimmed, empty ones left out, joined
//! by a blank line in this order: 全局 (one text) · 项目 (the project's background instructions) ·
//! 会话 (the session's own text) · 预设 (the session's purpose preset). The preview and the request
//! are built by the same `compose`, so what the preview shows is what is sent (as the system message).
//! - A draft (no session yet) keeps its 会话 / 预设 choice on the page; the first send saves it.
//! - Deleting a preset unsets it in the sessions using it (FK). All edits are undoable.
//! - Nothing is seeded: the personal global instructions and presets come from Picky in phase 7.

use super::store::NOW;
use crate::modules::common::err;
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

const TEXT_MAX: usize = 20_000;
const NAME_MAX: usize = 30;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    /// global | project | session | preset
    pub kind: &'static str,
    pub label: &'static str,
    pub content: String,
}

/// A session's own layer and its purpose preset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSetting {
    pub content: String,
    pub preset_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: i64,
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub layers: Vec<Layer>,
    /// What the session (or draft) has chosen; a deleted preset reads as none.
    pub session: SessionSetting,
}

pub fn compose(global: &str, project: &str, session: &str, preset: &str) -> Vec<Layer> {
    [("global", "全局", global), ("project", "项目", project), ("session", "会话", session), ("preset", "预设", preset)]
        .into_iter()
        .filter_map(|(kind, label, c)| {
            let t = c.trim();
            (!t.is_empty()).then(|| Layer { kind, label, content: t.to_string() })
        })
        .collect()
}

/// The system message the layers make (empty = none).
pub fn text(layers: &[Layer]) -> String {
    layers.iter().map(|l| l.content.as_str()).collect::<Vec<_>>().join("\n\n")
}

fn limit(s: &str, what: &str) -> Result<String, String> {
    let t = s.trim();
    if t.chars().count() > TEXT_MAX {
        return Err(format!("{what}太长了，最多 {TEXT_MAX} 字"));
    }
    Ok(t.to_string())
}

// ---------- global ----------

pub fn global(conn: &Connection) -> Result<String, String> {
    Ok(conn.query_row("SELECT content FROM global_instructions WHERE id = 1", [], |r| r.get(0)).optional().map_err(err)?.unwrap_or_default())
}

pub fn set_global(conn: &Connection, content: &str) -> Result<(), String> {
    let c = limit(content, "全局指令")?;
    conn.execute(
        &format!("INSERT INTO global_instructions (id, content) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET content = excluded.content, updated_at = {NOW} WHERE content <> excluded.content"),
        [c],
    )
    .map(|_| ())
    .map_err(err)
}

// ---------- presets ----------

pub fn presets(conn: &Connection) -> Result<Vec<Preset>, String> {
    let mut st = conn.prepare("SELECT id, name, content FROM presets ORDER BY sort_order, created_at, id").map_err(err)?;
    let rows = st.query_map([], |r| Ok(Preset { id: r.get(0)?, name: r.get(1)?, content: r.get(2)? })).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

fn preset_fields(name: &str, content: &str) -> Result<(String, String), String> {
    let n = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.is_empty() {
        return Err("预设要有名称".into());
    }
    if n.chars().count() > NAME_MAX {
        return Err(format!("预设名称最多 {NAME_MAX} 个字"));
    }
    Ok((n, limit(content, "预设内容")?))
}

/// New presets go last.
pub fn create_preset(conn: &Connection, name: &str, content: &str) -> Result<i64, String> {
    let (n, c) = preset_fields(name, content)?;
    conn.execute("INSERT INTO presets (name, content, sort_order) VALUES (?1, ?2, (SELECT coalesce(max(sort_order), -1) + 1 FROM presets))", params![n, c]).map_err(err)?;
    Ok(conn.last_insert_rowid())
}

pub fn update_preset(conn: &Connection, id: i64, name: &str, content: &str) -> Result<(), String> {
    let (n, c) = preset_fields(name, content)?;
    let found: i64 = conn.query_row("SELECT count(*) FROM presets WHERE id = ?1", [id], |r| r.get(0)).map_err(err)?;
    if found == 0 {
        return Err("找不到这个预设，可能已经被删除了".into());
    }
    conn.execute(&format!("UPDATE presets SET name = ?1, content = ?2, updated_at = {NOW} WHERE id = ?3 AND NOT (name = ?1 AND content = ?2)"), params![n, c, id]).map(|_| ()).map_err(err)
}

pub fn delete_preset(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM presets WHERE id = ?1", [id]).map_err(err)? {
        0 => Err("找不到这个预设，可能已经被删除了".into()),
        _ => Ok(()),
    }
}

fn preset_exists(conn: &Connection, id: Option<i64>) -> Result<Option<i64>, String> {
    match id {
        Some(p) => conn.query_row("SELECT id FROM presets WHERE id = ?1", [p], |r| r.get(0)).optional().map_err(err),
        None => Ok(None),
    }
}

// ---------- sessions ----------

pub fn session_setting(conn: &Connection, session_id: i64) -> Result<SessionSetting, String> {
    conn.query_row("SELECT instructions, preset_id FROM sessions WHERE id = ?1", [session_id], |r| Ok(SessionSetting { content: r.get(0)?, preset_id: r.get(1)? }))
        .optional()
        .map_err(err)?
        .ok_or_else(|| "找不到这个对话，可能已经被删除了".to_string())
}

/// A draft's choice, checked: the text is limited, a deleted preset becomes none.
pub fn checked(conn: &Connection, s: &SessionSetting) -> Result<SessionSetting, String> {
    Ok(SessionSetting { content: limit(&s.content, "会话指令")?, preset_id: preset_exists(conn, s.preset_id)? })
}

pub fn set_session(conn: &Connection, session_id: i64, s: &SessionSetting) -> Result<(), String> {
    session_setting(conn, session_id)?;
    let c = limit(&s.content, "会话指令")?;
    if s.preset_id.is_some() && preset_exists(conn, s.preset_id)?.is_none() {
        return Err("这个预设已经被删除了".into());
    }
    conn.execute("UPDATE sessions SET instructions = ?1, preset_id = ?2 WHERE id = ?3 AND (instructions <> ?1 OR preset_id IS NOT ?2)", params![c, s.preset_id, session_id])
        .map(|_| ())
        .map_err(err)
}

/// The layers for a session, or for a draft (`session` None) in `project` with its `draft` choice.
pub fn preview(conn: &Connection, session: Option<i64>, project: Option<i64>, draft: Option<&SessionSetting>) -> Result<Preview, String> {
    let (project_text, setting) = match session {
        Some(id) => {
            let p: Option<String> =
                conn.query_row("SELECT p.instructions FROM sessions s JOIN projects p ON p.id = s.project_id WHERE s.id = ?1", [id], |r| r.get(0)).optional().map_err(err)?;
            (p.unwrap_or_default(), session_setting(conn, id)?)
        }
        None => {
            let p: Option<String> = match project {
                Some(pid) => conn.query_row("SELECT instructions FROM projects WHERE id = ?1", [pid], |r| r.get(0)).optional().map_err(err)?,
                None => None,
            };
            (p.unwrap_or_default(), SessionSetting { content: draft.map(|d| d.content.clone()).unwrap_or_default(), preset_id: preset_exists(conn, draft.and_then(|d| d.preset_id))? })
        }
    };
    let preset: Option<String> = match setting.preset_id {
        Some(id) => conn.query_row("SELECT content FROM presets WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(err)?,
        None => None,
    };
    Ok(Preview { layers: compose(&global(conn)?, &project_text, &setting.content, preset.as_deref().unwrap_or("")), session: setting })
}

// ---------- commands ----------

#[tauri::command]
pub fn picky_instructions(state: State<AppState>, session_id: Option<i64>, project_id: Option<i64>, draft: Option<SessionSetting>) -> Result<Preview, String> {
    state.picky.with_db(|c| preview(c, session_id, project_id, draft.as_ref()))
}

#[tauri::command]
pub fn picky_session_instructions_save(state: State<AppState>, session_id: i64, setting: SessionSetting) -> Result<(), String> {
    let label = state.picky.with_db(|c| session_setting(c, session_id)).map(|old| if old.content.trim() == setting.content.trim() { "选用途预设" } else { "修改会话指令" })?;
    state.with_picky_undo(label, |c| set_session(c, session_id, &setting))
}

#[tauri::command]
pub fn picky_global_instructions(state: State<AppState>) -> Result<String, String> {
    state.picky.with_db(|c| global(c))
}

#[tauri::command]
pub fn picky_global_instructions_save(state: State<AppState>, content: String) -> Result<(), String> {
    state.with_picky_undo("修改全局指令", |c| set_global(c, &content))
}

#[tauri::command]
pub fn picky_presets(state: State<AppState>) -> Result<Vec<Preset>, String> {
    state.picky.with_db(|c| presets(c))
}

/// `id` None = a new preset; returns its id.
#[tauri::command]
pub fn picky_preset_save(state: State<AppState>, id: Option<i64>, name: String, content: String) -> Result<i64, String> {
    match id {
        Some(id) => state.with_picky_undo("修改预设", |c| update_preset(c, id, &name, &content).map(|_| id)),
        None => state.with_picky_undo("新建预设", |c| create_preset(c, &name, &content)),
    }
}

#[tauri::command]
pub fn picky_preset_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_picky_undo("删除预设", |c| delete_preset(c, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::chat::begin_run;
    use crate::picky::library;
    use crate::picky::store::tests::memory_db;

    #[test]
    fn layers_compose_in_order_and_skip_empty_ones() {
        let l = compose(" 用中文 ", "", "\n简短\n", "像老师一样");
        assert_eq!(l.iter().map(|x| x.label).collect::<Vec<_>>(), ["全局", "会话", "预设"]);
        assert_eq!(text(&l), "用中文\n\n简短\n\n像老师一样");
        assert!(text(&compose("", " ", "", "")).is_empty());
    }

    #[test]
    fn sessions_drafts_presets_and_projects() {
        let c = memory_db();
        set_global(&c, "  回复用中文  ").unwrap();
        assert_eq!(global(&c).unwrap(), "回复用中文");
        let p = library::create_project(&c, "论文", "引用注明出处").unwrap();
        let teacher = create_preset(&c, "老师", "像老师一样解释").unwrap();
        let editor = create_preset(&c, "编辑", "挑错字").unwrap();
        assert_eq!(presets(&c).unwrap().iter().map(|x| x.id).collect::<Vec<_>>(), [teacher, editor]);
        assert!(create_preset(&c, " ", "x").is_err());

        // a draft in the project, with its own choice
        let draft = SessionSetting { content: "只用要点".into(), preset_id: Some(teacher) };
        let v = preview(&c, None, Some(p), Some(&draft)).unwrap();
        assert_eq!(text(&v.layers), "回复用中文\n\n引用注明出处\n\n只用要点\n\n像老师一样解释");

        // a saved session: its project and setting
        let b = begin_run(&c, None, Some(p), "问题", "local:a", "A").unwrap();
        set_session(&c, b.session_id, &SessionSetting { content: "".into(), preset_id: Some(editor) }).unwrap();
        let v = preview(&c, Some(b.session_id), None, None).unwrap();
        assert_eq!(v.layers.iter().map(|x| x.kind).collect::<Vec<_>>(), ["global", "project", "preset"]);
        assert!(set_session(&c, b.session_id, &SessionSetting { content: "".into(), preset_id: Some(999) }).is_err());

        // deleting the preset unsets it; moving out of the project drops that layer
        delete_preset(&c, editor).unwrap();
        assert_eq!(session_setting(&c, b.session_id).unwrap().preset_id, None);
        library::move_to(&c, &[b.session_id], None).unwrap();
        assert_eq!(text(&preview(&c, Some(b.session_id), None, None).unwrap().layers), "回复用中文");
        update_preset(&c, teacher, "老师 ", "像老师").unwrap();
        assert_eq!(presets(&c).unwrap()[0].name, "老师");
        assert_eq!(checked(&c, &SessionSetting { content: " x ".into(), preset_id: Some(editor) }).unwrap(), SessionSetting { content: "x".into(), preset_id: None });
    }
}
