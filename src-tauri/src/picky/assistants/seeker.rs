//! Seeker (助手, v0.7): the user's Chinese resume workflow. picky.db tables (migration 0020):
//! - `seeker_settings` (one row): the remembered Seeker folder (only a path; nothing is read until
//!   the user previews an import), workflow options (models, thinking, cloud, 纯开发岗) and the Word
//!   template mapping. `seeker_draft` (one row): the current draft (JSON `Draft`) and the baseline
//!   「修改差异」 compares with (set by 新建 / 重新组装 / 确认 / 另开草稿; 增加 keeps it).
//! - `seeker_records`: materials, 基本信息, facts, rules, 范例, 网申 (catalog.rs); `seeker_record_history`
//!   keeps every replaced or deleted version. `seeker_versions`: confirmed drafts, never changed.
//! - `seeker_exports` / `seeker_job_exports`: Word exports (immutable snapshots) and job links (output.rs).
//! - Edits go through `with_picky_undo` (one step each, batch deletes included); settings (folder,
//!   options, template) aren't undone (like the other assistants'), nor is a Word export and its
//!   snapshot (like Work's files). Editing the draft never writes back to materials.
//! - Models only propose (model.rs); the user sees a diff and accepts. Block ids (header fields,
//!   sections, entries, points) are unique and non-empty; they key selection, sorting and deletion.
//! - Data file (format 19): the whole workspace replaces Seeker's tables; paths are metadata only.
pub mod catalog;
pub mod import;
pub mod model;
pub mod output;
pub mod workflow;
use crate::modules::common::err;
use crate::picky::store::NOW;
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tauri::State;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderField {
    pub id: String,
    pub label: String,
    pub value: String,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub time: String,
    pub name: String,
    pub role: String,
    pub location: String,
    /// The grey line under the heading (a project's 概览; 教育: none, its role line goes there).
    #[serde(default)]
    pub note: String,
    pub points: Vec<Point>,
    #[serde(default)]
    pub source: Option<workflow::Source>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub entries: Vec<Entry>,
    /// The material category it was assembled from (教育, 技能, 项目, 工作…); "" for one the user added.
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub name: String,
    pub header: Vec<HeaderField>,
    pub sections: Vec<Section>,
    pub jd: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub folder: String,
    pub draft: Option<Draft>,
    pub updated_at: Option<String>,
    #[serde(default)]
    pub baseline: Option<Draft>,
    #[serde(default)]
    pub catalog: catalog::Catalog,
    #[serde(default)]
    pub output: output::Output,
    /// Worked out on reading (not in the data file): 项目 / 工作 over 3, entries whose source is gone.
    #[serde(default, skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// The default choice of points per enabled material (技术栈 only for 纯开发岗).
    #[serde(default, skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<workflow::Selection>,
}

impl Workspace {
    pub fn len(&self) -> usize {
        let c = &self.catalog;
        usize::from(self.draft.is_some() || !self.folder.is_empty() || c.settings != catalog::Settings::default())
            + c.records.len()
            + c.versions.len()
            + self.output.exports.len()
            + usize::from(self.output.template.is_some())
    }
}

pub fn read(conn: &Connection) -> Result<Workspace, String> {
    let settings: Option<(String, Option<String>)> = conn.query_row("SELECT folder, template FROM seeker_settings WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(err)?;
    let (folder, template) = settings.unwrap_or_default();
    let row: Option<(String, Option<String>, String)> = conn.query_row("SELECT body, baseline, updated_at FROM seeker_draft WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().map_err(err)?;
    let parse = |s: &str| serde_json::from_str::<Draft>(s).map_err(|e| format!("Seeker 草稿读不出来：{e}"));
    let (draft, baseline, updated_at) = match row {
        Some((body, baseline, at)) => (Some(parse(&body)?), baseline.as_deref().map(parse).transpose()?, Some(at)),
        None => (None, None, None),
    };
    let catalog = catalog::load(conn)?;
    let mut output = output::load(conn)?;
    output.template = template.map(|t| serde_json::from_str(&t).map_err(err)).transpose()?;
    let mut warnings = Vec::new();
    if let Some(d) = &draft {
        for category in ["项目", "工作"] {
            let count: usize = d.sections.iter().filter(|s| s.category == category).map(|s| s.entries.len()).sum();
            if count > 3 {
                warnings.push(format!("{category}经历 {count} 段，建议 3 段以内"));
            }
        }
        for e in d.sections.iter().flat_map(|s| &s.entries) {
            if e.source.as_ref().is_some_and(|s| !catalog.records.iter().any(|r| r.id == s.record_id)) {
                warnings.push(format!("「{}」的素材已删除", e.name));
            }
        }
    }
    let choices = catalog.records.iter().filter(|r| r.kind == "material" && r.enabled).map(|r| workflow::Selection { record_id: r.id.clone(), point_ids: r.points.iter().filter(|p| catalog.settings.developer_role || !workflow::is_stack(p)).map(|p| p.id.clone()).collect() }).collect();
    Ok(Workspace { folder, draft, updated_at, baseline, catalog, output, warnings, choices })
}

pub fn validate(draft: &Draft) -> Result<(), String> {
    let mut ids = HashSet::new();
    let all = draft.header.iter().map(|h| &h.id).chain(draft.sections.iter().flat_map(|s| std::iter::once(&s.id).chain(s.entries.iter().flat_map(|e| std::iter::once(&e.id).chain(e.points.iter().map(|p| &p.id))))));
    for id in all {
        if id.is_empty() || !ids.insert(id) {
            return Err("简历中有空白或重复的块编号".into());
        }
    }
    Ok(())
}

pub fn save(conn: &Connection, draft: &Draft) -> Result<(), String> {
    validate(draft)?;
    let body = serde_json::to_string(draft).map_err(err)?;
    conn.execute(&format!("INSERT INTO seeker_draft (id, body) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET body = excluded.body, updated_at = {NOW} WHERE body != excluded.body"), [body]).map_err(err)?;
    Ok(())
}

/// Settings (folder, options, template) are not undone, like the other assistants' settings.
pub(crate) fn set_settings(conn: &Connection, column: &str, value: &str) -> Result<(), String> {
    conn.execute(&format!("INSERT INTO seeker_settings (id, {column}) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET {column} = excluded.{column}"), [value]).map_err(err)?;
    Ok(())
}

/// The data-file import (inside its transaction): the file's workspace replaces Seeker's tables.
pub fn replace(conn: &Connection, w: &Workspace) -> Result<(), String> {
    conn.execute_batch("DELETE FROM seeker_job_exports; DELETE FROM seeker_exports; DELETE FROM seeker_versions; DELETE FROM seeker_record_history; DELETE FROM seeker_records; DELETE FROM seeker_draft; DELETE FROM seeker_settings;").map_err(err)?;
    let template = w.output.template.as_ref().map(serde_json::to_string).transpose().map_err(err)?;
    conn.execute("INSERT INTO seeker_settings (id, folder, options, template) VALUES (1, ?1, ?2, ?3)", params![w.folder, serde_json::to_string(&w.catalog.settings).map_err(err)?, template]).map_err(err)?;
    if let Some(draft) = &w.draft {
        validate(draft)?;
        let baseline = w.baseline.as_ref().map(serde_json::to_string).transpose().map_err(err)?;
        conn.execute(&format!("INSERT INTO seeker_draft (id, body, baseline, updated_at) VALUES (1, ?1, ?2, coalesce(?3, {NOW}))"), params![serde_json::to_string(draft).map_err(err)?, baseline, w.updated_at]).map_err(err)?;
    }
    catalog::replace(conn, &w.catalog)?;
    output::replace(conn, &w.output)
}

#[tauri::command]
pub async fn picky_seeker(state: State<'_, AppState>) -> Result<Workspace, String> {
    state.picky.with_db(|c| read(c))
}

#[tauri::command]
pub async fn picky_seeker_folder(state: State<'_, AppState>, folder: String) -> Result<Workspace, String> {
    state.picky.with_db(|c| {
        set_settings(c, "folder", folder.trim())?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_seeker_new(state: State<'_, AppState>) -> Result<Workspace, String> {
    state.with_picky_undo("新建简历", |c| {
        let tx = crate::modules::common::Tx::begin(c)?;
        let draft = workflow::initial(&catalog::load(&tx)?.records)?;
        save(&tx, &draft)?;
        workflow::baseline(&tx, &draft)?;
        tx.commit()?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_seeker_save(state: State<'_, AppState>, draft: Draft) -> Result<Workspace, String> {
    state.with_picky_undo("编辑简历", |c| {
        save(c, &draft)?;
        read(c)
    })
}

/// Deleting a section or entry takes what is in it; one undo step for the whole selection.
pub fn delete_blocks(draft: &mut Draft, ids: &[String]) {
    let selected: HashSet<&str> = ids.iter().map(String::as_str).collect();
    draft.header.retain(|h| !selected.contains(h.id.as_str()));
    draft.sections.retain(|s| !selected.contains(s.id.as_str()));
    for section in &mut draft.sections {
        section.entries.retain(|e| !selected.contains(e.id.as_str()));
        for entry in &mut section.entries {
            entry.points.retain(|p| !selected.contains(p.id.as_str()));
        }
    }
}

#[tauri::command]
pub async fn picky_seeker_delete_blocks(state: State<'_, AppState>, ids: Vec<String>) -> Result<Workspace, String> {
    state.with_picky_undo("删除简历内容", |c| {
        let mut draft = read(c)?.draft.ok_or("还没有简历草稿")?;
        delete_blocks(&mut draft, &ids);
        save(c, &draft)?;
        read(c)
    })
}

#[cfg(test)]
pub mod tests;
