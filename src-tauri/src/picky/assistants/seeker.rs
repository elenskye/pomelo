//! Seeker (助手, v0.7): the user's resume workflow, a Chinese and (阶段 ③) an English resume. picky.db
//! tables (migrations 0020, 0022, 0023):
//! - `seeker_settings` (one row): the remembered Seeker folder (only a path; nothing is read until
//!   the user previews an import), workflow options (models, thinking, cloud, 纯开发岗, the language
//!   the page shows) and the Word template mapping per language (`template`, `template_en`).
//!   `seeker_draft` (one row per language, `cn` / `en`): the current draft (JSON `Draft`) and the
//!   baseline 「修改差异」 compares with (set by 新建 / 重新组装 / 确认 / 另开草稿; 增加 keeps it).
//! - `seeker_records`: materials, 基本信息, facts, rules, 范例, 网申 (catalog.rs); `seeker_record_history`
//!   keeps every replaced or deleted version. `seeker_versions`: confirmed drafts, never changed.
//! - `seeker_exports` / `seeker_job_exports`: Word exports (immutable snapshots) and job links (output.rs).
//!   `seeker_job_companies` (0022): a job's company folder in knowledge/ when the user chose one (company.rs).
//! - Languages: a record's `lang` (catalog.rs: materials and 基本信息 are one language; rules, 约定 and 范例
//!   one or both; facts, 档案 and 网申 always both); a draft's `lang` says which row it is and goes with
//!   its versions and exports. Everything a command does is in one language; nothing is translated.
//!   An English draft is a Resume or a CV (`form`): only reminders differ (`warnings`).
//! - The draft can be for a 投石问路 job (`Draft.job`, set by 「做简历」 or the job list of 岗位与建议):
//!   its 备注 is the JD, exports default to linking it, 选材 / 审查 search its company's knowledge/.
//! - Edits go through `with_picky_undo` (one step each, batch deletes included); settings (folder,
//!   options, templates) aren't undone (like the other assistants'), nor is a Word export and its
//!   snapshot (like Work's files). Editing the draft never writes back to materials.
//! - Models only propose (model.rs); the user sees a diff and accepts. Block ids (header fields,
//!   sections, entries, points) are unique and non-empty; they key selection, sorting and deletion.
//! - Data file (format 19; 20 with the job and company choices; 22 with English): the whole workspace
//!   replaces Seeker's tables; paths are metadata only.
pub mod catalog;
pub mod company;
pub mod files;
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

pub const LANGS: [&str; 2] = ["cn", "en"];
/// An English draft's form.
pub const FORMS: [&str; 2] = ["resume", "cv"];

pub fn lang_of(lang: &str) -> Result<&'static str, String> {
    LANGS.iter().find(|l| **l == lang).copied().ok_or_else(|| "未知的简历语言".to_string())
}

fn cn() -> String {
    "cn".into()
}

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
    /// The grey line under the heading (a project's 概览; 教育: none, its role line goes there); a
    /// Summary's text.
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
    /// The material category it was assembled from (摘要, 教育, 技能, 项目, 工作, 证书…); "" for one the user added.
    #[serde(default)]
    pub category: String,
}

/// A 投石问路 job (ids can come back after a delete: the creation time tells them apart).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRef {
    pub id: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    /// `cn` or `en` (drafts saved before 阶段 ③ are Chinese).
    #[serde(default = "cn")]
    pub lang: String,
    /// English: `resume` or `cv`; Chinese: "".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub form: String,
    pub name: String,
    pub header: Vec<HeaderField>,
    pub sections: Vec<Section>,
    pub jd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<JobRef>,
}

/// One language's draft, its baseline and the reminders about it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Desk {
    pub draft: Option<Draft>,
    pub updated_at: Option<String>,
    #[serde(default)]
    pub baseline: Option<Draft>,
    /// Worked out on reading (not in the data file): too many projects or points, entries whose
    /// source is gone.
    #[serde(default, skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub folder: String,
    /// The Chinese desk keeps the names it had before English (`draft`, `baseline`, `updatedAt`).
    #[serde(flatten)]
    pub cn: Desk,
    #[serde(default)]
    pub en: Desk,
    #[serde(default)]
    pub catalog: catalog::Catalog,
    #[serde(default)]
    pub output: output::Output,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub job_companies: Vec<company::Choice>,
    /// The default choice of points per enabled material (技术栈 only for 纯开发岗).
    #[serde(default, skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<workflow::Selection>,
}

impl Workspace {
    pub fn len(&self) -> usize {
        let c = &self.catalog;
        usize::from(self.cn.draft.is_some() || self.en.draft.is_some() || !self.folder.is_empty() || c.settings != catalog::Settings::default())
            + c.records.len()
            + c.versions.len()
            + self.output.exports.len()
            + usize::from(self.output.template.is_some())
            + usize::from(self.output.template_en.is_some())
            + self.job_companies.len()
    }

    pub fn desk(&self, lang: &str) -> &Desk {
        if lang == "en" { &self.en } else { &self.cn }
    }

    pub fn draft(&self, lang: &str) -> Option<&Draft> {
        self.desk(lang).draft.as_ref()
    }

    /// As the data file has it (reminders are worked out on reading).
    pub fn without_warnings(mut self) -> Self {
        self.cn.warnings.clear();
        self.en.warnings.clear();
        self
    }
}

/// What the program can count: 项目 / 工作 over 3 (中文), an English Resume's projects over 2 and
/// entries over 3 points, more than one Summary, entries whose material is gone.
pub fn warnings(d: &Draft, records: &[catalog::Record]) -> Vec<String> {
    let mut out = Vec::new();
    let count = |category: &str| d.sections.iter().filter(|s| s.category == category).map(|s| s.entries.len()).sum::<usize>();
    if d.lang == "en" {
        let projects = count("项目");
        if d.form == "resume" && projects > 2 {
            out.push(format!("Resume 建议 1–2 个项目，现在 {projects} 个"));
        }
        for e in d.sections.iter().filter(|s| s.category == "项目" || s.category == "工作").flat_map(|s| &s.entries) {
            if e.points.len() > 3 {
                out.push(format!("「{}」{} 条，每段最多 3 条", e.name, e.points.len()));
            }
        }
        if count("摘要") > 1 {
            out.push("Summary 只留一条".into());
        }
    } else {
        for category in ["项目", "工作"] {
            let n = count(category);
            if n > 3 {
                out.push(format!("{category}经历 {n} 段，建议 3 段以内"));
            }
        }
    }
    for e in d.sections.iter().flat_map(|s| &s.entries) {
        if e.source.as_ref().is_some_and(|s| !records.iter().any(|r| r.id == s.record_id)) {
            out.push(format!("「{}」的素材已删除", e.name));
        }
    }
    out
}

pub fn read(conn: &Connection) -> Result<Workspace, String> {
    let settings: Option<(String, Option<String>, Option<String>)> = conn.query_row("SELECT folder, template, template_en FROM seeker_settings WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().map_err(err)?;
    let (folder, template, template_en) = settings.unwrap_or_default();
    let parse = |s: &str| serde_json::from_str::<Draft>(s).map_err(|e| format!("Seeker 草稿读不出来：{e}"));
    let mut st = conn.prepare("SELECT lang, body, baseline, updated_at FROM seeker_draft").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?))).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let catalog = catalog::load(conn)?;
    let (mut cn, mut en) = (Desk::default(), Desk::default());
    for (lang, body, baseline, at) in rows {
        let mut draft = parse(&body)?;
        draft.lang = lang.clone();
        let desk = Desk { warnings: warnings(&draft, &catalog.records), draft: Some(draft), baseline: baseline.as_deref().map(parse).transpose()?, updated_at: Some(at) };
        if lang == "en" { en = desk } else { cn = desk }
    }
    let mut output = output::load(conn)?;
    let parsed = |t: Option<String>| t.map(|t| serde_json::from_str(&t).map_err(err)).transpose();
    output.template = parsed(template)?;
    output.template_en = parsed(template_en)?;
    let choices = catalog.records.iter().filter(|r| r.kind == "material" && r.enabled).map(|r| workflow::Selection { record_id: r.id.clone(), point_ids: r.choosable().iter().filter(|p| catalog.settings.developer_role || !workflow::is_stack(p)).map(|p| p.id.clone()).collect() }).collect();
    let job_companies = company::load(conn)?;
    Ok(Workspace { folder, cn, en, catalog, output, job_companies, choices })
}

pub fn validate(draft: &Draft) -> Result<(), String> {
    lang_of(&draft.lang)?;
    if (draft.lang == "en") != FORMS.contains(&draft.form.as_str()) {
        return Err("简历的类型不对".into());
    }
    let mut ids = HashSet::new();
    let all = draft.header.iter().map(|h| &h.id).chain(draft.sections.iter().flat_map(|s| std::iter::once(&s.id).chain(s.entries.iter().flat_map(|e| std::iter::once(&e.id).chain(e.points.iter().map(|p| &p.id))))));
    for id in all {
        if id.is_empty() || !ids.insert(id) {
            return Err("简历中有空白或重复的块编号".into());
        }
    }
    Ok(())
}

/// Into its language's row.
pub fn save(conn: &Connection, draft: &Draft) -> Result<(), String> {
    validate(draft)?;
    let body = serde_json::to_string(draft).map_err(err)?;
    conn.execute(&format!("INSERT INTO seeker_draft (lang, body) VALUES (?1, ?2) ON CONFLICT(lang) DO UPDATE SET body = excluded.body, updated_at = {NOW} WHERE body != excluded.body"), params![draft.lang, body]).map_err(err)?;
    Ok(())
}

/// Seeker's 「允许发给云端」 (the 「Seeker 资料」 skill and Rater follow it).
pub fn cloud_allowed(conn: &Connection) -> Result<bool, String> {
    let raw: Option<String> = conn.query_row("SELECT options FROM seeker_settings WHERE id = 1", [], |r| r.get(0)).optional().map_err(err)?;
    Ok(raw.and_then(|s| serde_json::from_str::<catalog::Settings>(&s).ok()).is_some_and(|s| s.cloud_allowed))
}

/// Settings (folder, options, template) are not undone, like the other assistants' settings.
pub(crate) fn set_settings(conn: &Connection, column: &str, value: &str) -> Result<(), String> {
    conn.execute(&format!("INSERT INTO seeker_settings (id, {column}) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET {column} = excluded.{column}"), [value]).map_err(err)?;
    Ok(())
}

/// A data file before format 22 had no languages: the rules from SEEKER.md (and 共用准则) are both
/// languages' (like migration 0023); everything else stays Chinese.
pub fn from_before_english(w: &mut Workspace) {
    let shared = |r: &catalog::Record| ["rule", "decision", "example"].contains(&r.kind.as_str()) && (r.source.as_deref().is_some_and(|s| s.starts_with("SEEKER.md#")) || r.category == "共用准则");
    for r in w.catalog.records.iter_mut().chain(w.catalog.revisions.iter_mut().map(|v| &mut v.body)) {
        if shared(r) {
            r.lang = String::new();
        }
    }
}

/// The data-file import (inside its transaction): the file's workspace replaces Seeker's tables.
pub fn replace(conn: &Connection, w: &Workspace) -> Result<(), String> {
    conn.execute_batch("DELETE FROM seeker_job_companies; DELETE FROM seeker_job_exports; DELETE FROM seeker_exports; DELETE FROM seeker_versions; DELETE FROM seeker_record_history; DELETE FROM seeker_records; DELETE FROM seeker_draft; DELETE FROM seeker_settings;").map_err(err)?;
    let template = |t: &Option<output::Template>| t.as_ref().map(serde_json::to_string).transpose().map_err(err);
    conn.execute("INSERT INTO seeker_settings (id, folder, options, template, template_en) VALUES (1, ?1, ?2, ?3, ?4)", params![w.folder, serde_json::to_string(&w.catalog.settings).map_err(err)?, template(&w.output.template)?, template(&w.output.template_en)?]).map_err(err)?;
    for (lang, desk) in [("cn", &w.cn), ("en", &w.en)] {
        let Some(draft) = &desk.draft else { continue };
        validate(draft)?;
        if draft.lang != lang || desk.baseline.as_ref().is_some_and(|b| b.lang != lang) {
            return Err("Seeker：草稿的语言对不上".into());
        }
        let baseline = desk.baseline.as_ref().map(serde_json::to_string).transpose().map_err(err)?;
        conn.execute(&format!("INSERT INTO seeker_draft (lang, body, baseline, updated_at) VALUES (?1, ?2, ?3, coalesce(?4, {NOW}))"), params![lang, serde_json::to_string(draft).map_err(err)?, baseline, desk.updated_at]).map_err(err)?;
    }
    catalog::replace(conn, &w.catalog)?;
    output::replace(conn, &w.output)?;
    company::replace(conn, &w.job_companies)
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
pub async fn picky_seeker_new(state: State<'_, AppState>, lang: String) -> Result<Workspace, String> {
    let lang = lang_of(&lang)?;
    state.with_picky_undo(if lang == "en" { "新建英文简历" } else { "新建简历" }, |c| {
        let tx = crate::modules::common::Tx::begin(c)?;
        let draft = workflow::initial(&catalog::load(&tx)?.records, lang)?;
        save(&tx, &draft)?;
        workflow::baseline(&tx, &draft)?;
        tx.commit()?;
        read(c)
    })
}

/// 「做简历」 (投石问路) and the job list of 岗位与建议: the draft (of `lang`) is for this job (None: for
/// none); its 备注 becomes the JD (an empty one leaves the JD as it is). No draft yet: a new one. One
/// undo step.
pub fn use_job(c: &Connection, job: Option<&output::JobSnapshot>, lang: &str) -> Result<(), String> {
    let tx = crate::modules::common::Tx::begin(c)?;
    let view = read(&tx)?;
    let current = view.draft(lang).cloned();
    let fresh = current.is_none();
    let mut draft = match current {
        Some(d) => d,
        None => workflow::initial(&view.catalog.records, lang)?,
    };
    draft.job = job.map(|j| JobRef { id: j.id, created_at: j.created_at.clone() });
    if let Some(note) = job.and_then(|j| j.note.as_deref()).map(str::trim).filter(|n| !n.is_empty()) {
        draft.jd = note.into();
    }
    save(&tx, &draft)?;
    if fresh {
        workflow::baseline(&tx, &draft)?;
    }
    tx.commit()
}

#[tauri::command]
pub async fn picky_seeker_use_job(state: State<'_, AppState>, job_id: Option<i64>, lang: String) -> Result<Workspace, String> {
    let lang = lang_of(&lang)?;
    let job = job_id.map(|id| output::job(&state, id)).transpose()?;
    state.with_picky_undo(if job.is_some() { "为岗位做简历" } else { "简历不关联岗位" }, |c| {
        use_job(c, job.as_ref(), lang)?;
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
pub async fn picky_seeker_delete_blocks(state: State<'_, AppState>, ids: Vec<String>, lang: String) -> Result<Workspace, String> {
    let lang = lang_of(&lang)?;
    state.with_picky_undo("删除简历内容", |c| {
        let mut draft = read(c)?.draft(lang).cloned().ok_or("还没有简历草稿")?;
        delete_blocks(&mut draft, &ids);
        save(c, &draft)?;
        read(c)
    })
}

#[cfg(test)]
pub mod tests;
