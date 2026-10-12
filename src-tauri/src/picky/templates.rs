//! Prompt templates (from Picky's PromptTemplates):
//! - A title (60 chars; empty = 未命名模板), a category (20 chars; empty = 未分类), a body where
//!   `{{名称}}` marks a place to fill (the page finds and steps through them), and pinning.
//! - Listed pinned first (in pinning order), then by category and title. The "/" menu and the empty
//!   page work from this list; using one records `last_used_at` (not undoable: it only orders the
//!   menu). Creating, editing, pinning and deleting are undoable.
//! - Nothing is seeded: the personal templates come from Picky in phase 7.

use super::store::NOW;
use crate::modules::common::err;
use crate::AppState;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

pub const UNNAMED: &str = "未命名模板";
const TITLE_MAX: usize = 60;
const CATEGORY_MAX: usize = 20;
const BODY_MAX: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: i64,
    pub title: String,
    pub category: String,
    pub body: String,
    pub pinned: bool,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInput {
    pub title: String,
    pub category: String,
    pub body: String,
}

pub fn list(conn: &Connection) -> Result<Vec<Template>, String> {
    let mut st = conn
        .prepare(
            "SELECT id, title, category, body, pinned_at IS NOT NULL, last_used_at FROM templates
             ORDER BY pinned_at IS NULL, pinned_at, category, title, created_at, id",
        )
        .map_err(err)?;
    let rows = st
        .query_map([], |r| Ok(Template { id: r.get(0)?, title: r.get(1)?, category: r.get(2)?, body: r.get(3)?, pinned: r.get(4)?, last_used_at: r.get(5)? }))
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

fn clean(t: &TemplateInput) -> Result<(String, String, String), String> {
    let title = t.title.split_whitespace().collect::<Vec<_>>().join(" ");
    let category = t.category.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.chars().count() > TITLE_MAX {
        return Err(format!("模板名称最多 {TITLE_MAX} 个字"));
    }
    if category.chars().count() > CATEGORY_MAX {
        return Err(format!("分类最多 {CATEGORY_MAX} 个字"));
    }
    if t.body.chars().count() > BODY_MAX {
        return Err("模板内容太长了".into());
    }
    Ok((if title.is_empty() { UNNAMED.into() } else { title }, category, t.body.trim_end().to_string()))
}

pub fn create(conn: &Connection, t: &TemplateInput) -> Result<i64, String> {
    let (title, category, body) = clean(t)?;
    conn.execute("INSERT INTO templates (title, category, body) VALUES (?1, ?2, ?3)", params![title, category, body]).map_err(err)?;
    Ok(conn.last_insert_rowid())
}

fn changed(n: usize) -> Result<(), String> {
    if n == 0 {
        Err("找不到这个模板，可能已经被删除了".into())
    } else {
        Ok(())
    }
}

pub fn update(conn: &Connection, id: i64, t: &TemplateInput) -> Result<(), String> {
    let (title, category, body) = clean(t)?;
    changed(
        conn.execute(
            &format!("UPDATE templates SET title = ?1, category = ?2, body = ?3, updated_at = {NOW} WHERE id = ?4 AND NOT (title = ?1 AND category = ?2 AND body = ?3)"),
            params![title, category, body, id],
        )
        .map_err(err)?
            + exists(conn, id)?,
    )
}

fn exists(conn: &Connection, id: i64) -> Result<usize, String> {
    conn.query_row("SELECT count(*) FROM templates WHERE id = ?1", [id], |r| r.get::<_, i64>(0)).map(|n| n as usize).map_err(err)
}

pub fn set_pinned(conn: &Connection, id: i64, pinned: bool) -> Result<(), String> {
    let sql = if pinned {
        format!("UPDATE templates SET pinned_at = {NOW} WHERE id = ?1 AND pinned_at IS NULL")
    } else {
        "UPDATE templates SET pinned_at = NULL WHERE id = ?1 AND pinned_at IS NOT NULL".into()
    };
    changed(conn.execute(&sql, [id]).map_err(err)? + exists(conn, id)?)
}

pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    changed(conn.execute("DELETE FROM templates WHERE id = ?1", [id]).map_err(err)?)
}

pub fn mark_used(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute(&format!("UPDATE templates SET last_used_at = {NOW} WHERE id = ?1"), [id]).map(|_| ()).map_err(err)
}

// ---------- commands ----------

#[tauri::command]
pub fn picky_templates(state: State<AppState>) -> Result<Vec<Template>, String> {
    state.picky.with_db(|c| list(c))
}

/// `id` None = a new template; returns its id.
#[tauri::command]
pub fn picky_template_save(state: State<AppState>, id: Option<i64>, template: TemplateInput) -> Result<i64, String> {
    match id {
        Some(id) => state.with_picky_undo("修改模板", |c| update(c, id, &template).map(|_| id)),
        None => state.with_picky_undo("新建模板", |c| create(c, &template)),
    }
}

#[tauri::command]
pub fn picky_template_pin(state: State<AppState>, id: i64, pinned: bool) -> Result<(), String> {
    state.with_picky_undo(if pinned { "置顶模板" } else { "取消置顶模板" }, |c| set_pinned(c, id, pinned))
}

#[tauri::command]
pub fn picky_template_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_picky_undo("删除模板", |c| delete(c, id))
}

#[tauri::command]
pub fn picky_template_used(state: State<AppState>, id: i64) -> Result<(), String> {
    state.picky.with_db(|c| mark_used(c, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::store::tests::memory_db;

    fn input(title: &str, category: &str, body: &str) -> TemplateInput {
        TemplateInput { title: title.into(), category: category.into(), body: body.into() }
    }

    #[test]
    fn pinned_first_then_category_and_title() {
        let c = memory_db();
        let a = create(&c, &input("润色", "写作", "请润色：\n\n{{原文}}\n\n")).unwrap();
        let b = create(&c, &input("  ", "", "x")).unwrap();
        let d = create(&c, &input("解释代码", "代码", "```\n{{代码}}\n```")).unwrap();
        let titles = |c: &Connection| list(c).unwrap().iter().map(|t| t.title.clone()).collect::<Vec<_>>();
        assert_eq!(titles(&c), [UNNAMED, "解释代码", "润色"]); // '' < 代码 < 写作
        assert_eq!(list(&c).unwrap()[2].body, "请润色：\n\n{{原文}}"); // trailing space trimmed
        set_pinned(&c, a, true).unwrap();
        c.execute("UPDATE templates SET pinned_at = '2000-01-01' WHERE id = ?1", [a]).unwrap(); // pinned earlier
        set_pinned(&c, b, true).unwrap();
        assert_eq!(titles(&c), ["润色", UNNAMED, "解释代码"]); // in pinning order
        set_pinned(&c, a, false).unwrap();
        assert_eq!(titles(&c), [UNNAMED, "解释代码", "润色"]);
        update(&c, d, &input("解释", "代码", "y")).unwrap();
        update(&c, d, &input("解释", "代码", "y")).unwrap(); // unchanged is fine
        assert!(create(&c, &input(&"长".repeat(61), "", "")).is_err());
        mark_used(&c, d).unwrap();
        assert!(list(&c).unwrap().iter().find(|t| t.id == d).unwrap().last_used_at.is_some());
        delete(&c, d).unwrap();
        assert!(delete(&c, d).is_err() && update(&c, d, &input("a", "", "")).is_err() && set_pinned(&c, d, true).is_err());
    }
}
