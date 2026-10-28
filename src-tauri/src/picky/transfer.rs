//! 小拾's part of the Pomelo data file (「设置 → 导出 / 导入」, module `picky`, from format version
//! 11): the global instructions, projects (name, background instructions, whether they use global
//! memories), purpose presets, templates, memories (sharing by project name) and, from 12, 定时任务
//! (from 13 with 思考; before: on), and from 19 the Seeker workspace (seeker.rs; replaced whole; from 20 with
//! the draft's job and the jobs' 公司资料 choices; from 21 records with 技术栈, 档案 links and 个人约定; from
//! 22 the English draft, template and records, older ones: `seeker::from_before_english`), and from 23
//! Rater's question bank (rater.rs; replaced whole).
//! Conversations (also the ones schedules wrote), files and indexes stay out (they live in picky.db
//! and its backups).
//! Import replaces the global instructions, templates and memories; projects and presets are
//! matched by name (updated or added; others are kept, so conversations keep theirs). Schedules
//! (when the file has them) replace too, matched by name so their conversations stay linked; they
//! count from the import on (nothing runs for the past); a cloud model not set up here → 小拾's
//! selection.
//! Export → import into an empty picky.db → export gives the same record (see test).

use super::assistants::{rater, seeker};
use super::store::NOW;
use super::{instructions, memory, schedule, templates};
use crate::modules::common::{err, Tx};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub const KEY: &str = "picky";
pub const LABEL: &str = "小拾";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRec {
    pub name: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default = "yes")]
    pub include_global_memories: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetRec {
    pub name: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateRec {
    pub title: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRec {
    pub content: String,
    /// The project's name (None = global).
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub shared_projects: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub local_only: bool,
    #[serde(default = "manual")]
    pub source: String,
    pub noted_on: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRec {
    pub name: String,
    pub prompt: String,
    pub every: String,
    #[serde(default)]
    pub day: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month: Option<i64>,
    pub at: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub builtin: Option<String>,
    /// From format 13 (before: on).
    #[serde(default = "yes")]
    pub thinking: bool,
}

impl ScheduleRec {
    fn input(&self) -> schedule::ScheduleInput {
        schedule::ScheduleInput { name: self.name.clone(), prompt: self.prompt.clone(), every: self.every.clone(), day: self.day, month: self.month, at: self.at.clone(), model: self.model.clone(), thinking: self.thinking }
    }
}

fn yes() -> bool {
    true
}
fn manual() -> String {
    "manual".into()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickyRecord {
    /// From format 19; an older file leaves Seeker as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seeker: Option<seeker::Workspace>,
    /// From format 23 (its practice rounds from 24); an older file leaves Rater as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rater: Option<rater::Bank>,
    #[serde(default)]
    pub global_instructions: String,
    #[serde(default)]
    pub projects: Vec<ProjectRec>,
    #[serde(default)]
    pub presets: Vec<PresetRec>,
    #[serde(default)]
    pub templates: Vec<TemplateRec>,
    #[serde(default)]
    pub memories: Vec<MemoryRec>,
    /// None: a file from before schedules (format 11) — the ones here stay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedules: Option<Vec<ScheduleRec>>,
}

impl PickyRecord {
    pub fn len(&self) -> usize {
        self.projects.len() + self.presets.len() + self.templates.len() + self.memories.len() + self.schedules.as_ref().map_or(0, Vec::len) + self.seeker.as_ref().map_or(0, seeker::Workspace::len) + self.rater.as_ref().map_or(0, rater::Bank::len)
    }
}

pub fn export(conn: &Connection) -> Result<PickyRecord, String> {
    let mut st = conn.prepare("SELECT id, name, instructions, include_global_memories FROM projects ORDER BY sort_order, id").map_err(err)?;
    let projects: Vec<(i64, ProjectRec)> = st
        .query_map([], |r| Ok((r.get(0)?, ProjectRec { name: r.get(1)?, instructions: r.get(2)?, include_global_memories: r.get(3)? })))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let name = |id: i64| projects.iter().find(|p| p.0 == id).map(|p| p.1.name.clone());
    let mut mems = memory::list(conn)?;
    mems.reverse(); // oldest first
    Ok(PickyRecord {
        seeker: Some(seeker::Workspace { choices: vec![], ..seeker::read(conn)?.without_warnings() }),
        rater: Some(rater::export(conn)?),
        global_instructions: instructions::global(conn)?,
        presets: instructions::presets(conn)?.into_iter().map(|p| PresetRec { name: p.name, content: p.content }).collect(),
        templates: templates::list(conn)?.into_iter().map(|t| TemplateRec { title: t.title, category: t.category, body: t.body, pinned: t.pinned }).collect(),
        memories: mems
            .into_iter()
            .map(|m| MemoryRec {
                content: m.content,
                project: m.project_id.and_then(name),
                shared_projects: m.shared_project_ids.into_iter().filter_map(name).collect(),
                enabled: m.enabled,
                local_only: m.local_only,
                source: m.source,
                noted_on: m.noted_on,
            })
            .collect(),
        projects: projects.into_iter().map(|p| p.1).collect(),
        schedules: Some(
            schedule::list(conn, "2000-01-01 00:00")?
                .into_iter()
                .map(|s| ScheduleRec { name: s.name, prompt: s.prompt, every: s.rule.every, day: s.rule.day, month: s.rule.month, at: s.rule.at, model: s.model, enabled: s.enabled, builtin: s.builtin, thinking: s.thinking })
                .collect(),
        ),
    })
}

/// Checked before anything is written.
pub fn validate(r: &PickyRecord) -> Result<(), String> {
    for draft in r.seeker.iter().flat_map(|s| [&s.cn.draft, &s.en.draft]).flatten() {
        seeker::validate(draft)?;
    }
    let names: Vec<&str> = r.projects.iter().map(|p| p.name.trim()).collect();
    if names.iter().any(|n| n.is_empty()) {
        return Err("小拾：有项目没有名称".into());
    }
    if r.presets.iter().any(|p| p.name.trim().is_empty()) {
        return Err("小拾：有用途预设没有名称".into());
    }
    let mut seen = std::collections::HashSet::new();
    for s in r.schedules.iter().flatten() {
        schedule::check_input(&s.input()).map_err(|e| format!("小拾：定时任务「{}」：{e}", s.name.trim()))?;
        if !seen.insert(s.name.trim()) {
            return Err(format!("小拾：有两个定时任务都叫「{}」", s.name.trim()));
        }
    }
    for m in &r.memories {
        let n = m.content.trim().chars().count();
        if n == 0 || n > memory::MAX_CHARS {
            return Err(format!("小拾：记忆需要 1 至 {} 个字", memory::MAX_CHARS));
        }
        for p in m.project.iter().chain(&m.shared_projects) {
            if !names.contains(&p.trim()) {
                return Err(format!("小拾：记忆用到的项目「{p}」不在文件里"));
            }
        }
    }
    Ok(())
}

/// What an import would replace or update.
pub fn count(conn: &Connection) -> Result<i64, String> {
    let base: i64 = conn.query_row("SELECT (SELECT count(*) FROM projects) + (SELECT count(*) FROM presets) + (SELECT count(*) FROM templates) + (SELECT count(*) FROM memories) + (SELECT count(*) FROM schedules)", [], |r| r.get(0)).map_err(err)?;
    Ok(base + seeker::read(conn)?.len() as i64 + rater::read(conn)?.len() as i64)
}

pub fn replace(conn: &Connection, r: &PickyRecord) -> Result<usize, String> {
    validate(r)?;
    let tx = Tx::begin(conn)?;
    instructions::set_global(&tx, &r.global_instructions)?;
    // projects by name
    let base: i64 = tx.query_row("SELECT coalesce(max(sort_order), 0) + 1 FROM projects", [], |x| x.get(0)).map_err(err)?;
    let mut ids: Vec<(String, i64)> = vec![];
    for (i, p) in r.projects.iter().enumerate() {
        let name = p.name.trim().to_string();
        let found: Option<i64> = tx.query_row("SELECT id FROM projects WHERE name = ?1 ORDER BY id LIMIT 1", [&name], |x| x.get(0)).optional().map_err(err)?;
        let id = match found {
            Some(id) => {
                tx.execute(&format!("UPDATE projects SET instructions = ?1, include_global_memories = ?2, updated_at = {NOW} WHERE id = ?3"), params![p.instructions.trim(), p.include_global_memories, id])
                    .map_err(err)?;
                id
            }
            None => {
                tx.execute("INSERT INTO projects (name, instructions, sort_order, include_global_memories) VALUES (?1, ?2, ?3, ?4)", params![name, p.instructions.trim(), base + i as i64, p.include_global_memories])
                    .map_err(err)?;
                tx.last_insert_rowid()
            }
        };
        ids.push((name, id));
    }
    let project = |n: &str| ids.iter().find(|x| x.0 == n.trim()).map(|x| x.1);
    // presets by name
    for p in &r.presets {
        let name = p.name.split_whitespace().collect::<Vec<_>>().join(" ");
        let found: Option<i64> = tx.query_row("SELECT id FROM presets WHERE name = ?1 ORDER BY id LIMIT 1", [&name], |x| x.get(0)).optional().map_err(err)?;
        match found {
            Some(id) => instructions::update_preset(&tx, id, &name, &p.content)?,
            None => {
                instructions::create_preset(&tx, &name, &p.content)?;
            }
        }
    }
    // templates and memories: replaced (pinned ones keep their order)
    tx.execute("DELETE FROM templates", []).map_err(err)?;
    for (i, t) in r.templates.iter().enumerate() {
        let id = templates::create(&tx, &templates::TemplateInput { title: t.title.clone(), category: t.category.clone(), body: t.body.clone() })?;
        if t.pinned {
            tx.execute("UPDATE templates SET pinned_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?1) WHERE id = ?2", params![format!("+{:.3} seconds", i as f64 / 1000.0), id]).map_err(err)?;
        }
    }
    tx.execute("DELETE FROM memories", []).map_err(err)?;
    for m in &r.memories {
        let input = memory::MemoryInput {
            content: m.content.clone(),
            project_id: m.project.as_deref().and_then(project),
            shared_project_ids: m.shared_projects.iter().filter_map(|n| project(n)).collect(),
            enabled: m.enabled,
            local_only: m.local_only,
        };
        let source = if ["manual", "command", "import"].contains(&m.source.as_str()) { m.source.as_str() } else { "manual" };
        let id = memory::create(&tx, &input, source, None)?;
        tx.execute("UPDATE memories SET noted_on = ?1 WHERE id = ?2", params![m.noted_on, id]).map_err(err)?;
    }
    if let Some(list) = &r.schedules {
        import_schedules(&tx, list)?;
    }
    if let Some(workspace) = &r.seeker {
        seeker::replace(&tx, workspace)?;
    }
    if let Some(bank) = &r.rater {
        rater::replace(&tx, bank)?;
    }
    tx.commit()?;
    Ok(r.len())
}

/// By name: kept ones updated (their conversations stay linked), the rest deleted, new ones added.
fn import_schedules(conn: &Connection, list: &[ScheduleRec]) -> Result<(), String> {
    let names: Vec<String> = list.iter().map(|s| s.name.trim().to_string()).collect();
    let mut st = conn.prepare("SELECT id, name FROM schedules").map_err(err)?;
    let have: Vec<(i64, String)> = st.query_map([], |x| Ok((x.get(0)?, x.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for (id, name) in &have {
        if !names.contains(name) {
            schedule::delete(conn, *id)?;
        }
    }
    for (i, s) in list.iter().enumerate() {
        let mut input = s.input();
        if let Some(id) = input.model.as_deref().and_then(|m| m.strip_prefix("cloud:")) {
            if super::cloud::model(conn, id)?.is_none() {
                input.model = None;
            }
        }
        let id = match have.iter().find(|h| h.1 == s.name.trim()) {
            Some((id, _)) => {
                schedule::update(conn, *id, &input)?;
                *id
            }
            None => schedule::create(conn, &input)?,
        };
        conn.execute(
            &format!("UPDATE schedules SET enabled = ?1, builtin = ?2, sort_order = ?3, since = {NOW} WHERE id = ?4"),
            params![s.enabled, s.builtin, i as i64, id],
        )
        .map_err(err)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::store::tests::memory_db;
    use crate::picky::{chat, library};

    #[test]
    fn export_import_roundtrip_and_conversations_keep_their_projects() {
        let a = memory_db();
        instructions::set_global(&a, "用中文").unwrap();
        let p1 = library::create_project(&a, "论文", "引用注明出处").unwrap();
        let p2 = library::create_project(&a, "求职", "").unwrap();
        library::set_include_global(&a, p2, false).unwrap();
        instructions::create_preset(&a, "老师", "像老师").unwrap();
        let t = templates::create(&a, &templates::TemplateInput { title: "润色".into(), category: "写作".into(), body: "{{原文}}".into() }).unwrap();
        templates::set_pinned(&a, t, true).unwrap();
        templates::create(&a, &templates::TemplateInput { title: "解释".into(), category: "".into(), body: "x".into() }).unwrap();
        memory::create(&a, &memory::MemoryInput { content: "住在新加坡".into(), enabled: true, local_only: true, ..Default::default() }, "manual", None).unwrap();
        memory::create(&a, &memory::MemoryInput { content: "用 APA".into(), project_id: Some(p1), shared_project_ids: vec![p2], enabled: false, ..Default::default() }, "command", Some(1)).unwrap();
        let mine = schedule::ScheduleInput { name: "早报".into(), prompt: "今天要做什么".into(), every: "day".into(), day: None, month: None, at: "08:30".into(), model: Some("local:qwen".into()), thinking: false };
        let early = schedule::create(&a, &mine).unwrap();
        schedule::set_enabled(&a, early, false).unwrap();
        let monthly = schedule::list(&a, "2026-10-14 10:00").unwrap()[1].id;
        schedule::delete(&a, monthly).unwrap();
        let rec = export(&a).unwrap();
        assert_eq!(rec.len(), 2 + 1 + 2 + 2 + 3);
        assert_eq!(rec.schedules.as_ref().unwrap().iter().map(|s| (s.name.as_str(), s.enabled, s.month)).collect::<Vec<_>>(), [("每周回顾", true, None), ("年度总结", true, Some(12)), ("早报", false, None)]);
        assert_eq!(rec.memories[1].shared_projects, ["求职"]);

        let b = memory_db();
        replace(&b, &serde_json::from_value(serde_json::to_value(&rec).unwrap()).unwrap()).unwrap();
        assert_eq!(export(&b).unwrap(), rec);
        // a file from before schedules leaves them; a cloud model not set up here → the selection
        let old = PickyRecord { schedules: None, ..rec.clone() };
        replace(&b, &old).unwrap();
        assert_eq!(export(&b).unwrap(), rec);
        let mut cloud = rec.clone();
        cloud.schedules.as_mut().unwrap()[1].model = Some("cloud:gone".into());
        replace(&b, &cloud).unwrap();
        assert_eq!(export(&b).unwrap().schedules.unwrap()[1].model, None);
        let mut twice = rec.clone();
        twice.schedules.as_mut().unwrap()[1].name = "每周回顾".into();
        assert!(validate(&twice).is_err());

        // into a picky.db with a conversation in 论文: the project is updated, not replaced
        let c = memory_db();
        let own = library::create_project(&c, "论文", "旧的").unwrap();
        let s = chat::begin_run(&c, None, Some(own), "q", "local:x", "X").unwrap().session_id;
        replace(&c, &rec).unwrap();
        assert_eq!(library::library(&c).unwrap().projects.iter().find(|p| p.id == own).unwrap().instructions, "引用注明出处");
        assert_eq!(library::library(&c).unwrap().projects.iter().find(|p| p.id == own).unwrap().sessions[0].id, s);
        let mut bad = rec.clone();
        bad.memories[1].project = Some("没有".into());
        assert!(validate(&bad).is_err());
    }
}
