//! 小拾's part of the Pomelo data file (「设置 → 导出 / 导入」, module `picky`, from format version
//! 11): the global instructions, projects (name, background instructions, whether they use global
//! memories), purpose presets, templates and memories (sharing by project name). Conversations,
//! files and indexes stay out (they live in picky.db and its backups).
//! Import replaces the global instructions, templates and memories; projects and presets are
//! matched by name (updated or added; others are kept, so conversations keep theirs).
//! Export → import into an empty picky.db → export gives the same record (see test).

use super::store::NOW;
use super::{instructions, memory, templates};
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

fn yes() -> bool {
    true
}
fn manual() -> String {
    "manual".into()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickyRecord {
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
}

impl PickyRecord {
    pub fn len(&self) -> usize {
        self.projects.len() + self.presets.len() + self.templates.len() + self.memories.len()
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
    })
}

/// Checked before anything is written.
pub fn validate(r: &PickyRecord) -> Result<(), String> {
    let names: Vec<&str> = r.projects.iter().map(|p| p.name.trim()).collect();
    if names.iter().any(|n| n.is_empty()) {
        return Err("小拾：有项目没有名称".into());
    }
    if r.presets.iter().any(|p| p.name.trim().is_empty()) {
        return Err("小拾：有用途预设没有名称".into());
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
    conn.query_row("SELECT (SELECT count(*) FROM projects) + (SELECT count(*) FROM presets) + (SELECT count(*) FROM templates) + (SELECT count(*) FROM memories)", [], |r| r.get(0)).map_err(err)
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
    tx.commit()?;
    Ok(r.len())
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
        let rec = export(&a).unwrap();
        assert_eq!(rec.len(), 2 + 1 + 2 + 2);
        assert_eq!(rec.memories[1].shared_projects, ["求职"]);

        let b = memory_db();
        replace(&b, &serde_json::from_value(serde_json::to_value(&rec).unwrap()).unwrap()).unwrap();
        assert_eq!(export(&b).unwrap(), rec);

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
