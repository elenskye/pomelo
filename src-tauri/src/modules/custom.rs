//! 五花八门 / 循序渐进 (v0.4.5): templates, not modules. Each module made from one (`custom_modules`)
//! is its own sidebar module with the key "list/<id>" / "progress/<id>" (also its route); sidebar
//! order, 隐藏, 发给云端, search hits and 小拾 all go by that key. Rules (all here):
//! - Name: 1–20 chars, unique among them and not a built-in module's. Icon: one of `ICONS`; the color
//!   comes from the template (index.css `--module-list` / `--module-progress`).
//! - 循序渐进 names its two levels (default 课程 / 课节, 1–6 chars each); 五花八门 has none.
//! - The template never changes. Deleting a module deletes everything in it (FK cascade, one ⌘Z).
//!   Ids are never reused (AUTOINCREMENT), so a new module never inherits an old one's settings.
//! - 五花八门: lists.rs; 循序渐进: progress.rs (its 类型 list lives in kinds.rs as "progress/<id>",
//!   deleted with the module). Export / import: every made module in `custom` (fields, items, kinds,
//!   courses and lessons by name); importing replaces them all (new ids).

use super::common::{self, err, str_enum};
use super::{kinds, lists, progress};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tauri::State;

pub const KEY: &str = "custom";
pub const LABEL: &str = "五花八门 · 循序渐进";
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";
pub const MAX_NAME: usize = 20;
pub const MAX_WORD: usize = 6;
pub const DEFAULT_WORDS: (&str, &str) = ("课程", "课节");

/// Lucide icon names the 新建 dialog offers (src/modules/custom/icons.ts mirrors this list).
pub const ICONS: &[&str] = &[
    "shapes", "list-checks", "gamepad-2", "music", "headphones", "camera", "tv", "coffee", "utensils", "shopping-bag", "gift", "shirt",
    "plane", "mountain", "leaf", "sprout", "dumbbell", "heart", "star", "graduation-cap", "languages", "code", "puzzle", "trophy",
];

/// Names a made module can't take (built-in modules, pages and the templates themselves).
const TAKEN: &[&str] = &[
    "首页", "设置", "一步一步", "摸鱼读读", "好奇看看", "随意画画", "出门走走", "钱来钱往", "投石问路", "念念不忘", "来日方长", "芝麻开门", "小拾", "五花八门",
    "循序渐进",
];

fn gone() -> String {
    "找不到这个模块，可能已经被删除了".into()
}

str_enum!(Template, default = List, {
    List => "list",
    Progress => "progress",
});

impl Template {
    pub fn label(self) -> &'static str {
        match self {
            Template::List => "五花八门",
            Template::Progress => "循序渐进",
        }
    }
}

/// The module key (route) of a made module.
pub fn key(template: Template, id: i64) -> String {
    format!("{}/{id}", template.as_str())
}

/// "list/3" → (List, 3).
pub fn parse_key(key: &str) -> Option<(Template, i64)> {
    let (t, id) = key.split_once('/')?;
    Some((Template::parse(t)?, id.parse().ok()?))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Custom {
    pub id: i64,
    pub template: Template,
    pub name: String,
    pub icon: String,
    /// 循序渐进 only (empty for 五花八门).
    pub course_word: String,
    pub lesson_word: String,
    pub key: String,
    /// Items (五花八门) / courses (循序渐进).
    pub count: i64,
    pub created_at: String,
    pub updated_at: String,
}

const SQL: &str = "SELECT m.id, m.template, m.name, m.icon, m.course_word, m.lesson_word, m.created_at, m.updated_at,
    CASE m.template WHEN 'list' THEN (SELECT count(*) FROM list_items i WHERE i.module_id = m.id)
                    ELSE (SELECT count(*) FROM progress_courses c WHERE c.module_id = m.id) END
  FROM custom_modules m";

fn row(r: &Row) -> rusqlite::Result<Custom> {
    let (id, template): (i64, Template) = (r.get(0)?, r.get(1)?);
    let words = |i: usize, d: &str| -> rusqlite::Result<String> { Ok(if template == Template::Progress { r.get::<_, Option<String>>(i)?.unwrap_or_else(|| d.into()) } else { String::new() }) };
    Ok(Custom {
        id,
        template,
        name: r.get(2)?,
        icon: r.get(3)?,
        course_word: words(4, DEFAULT_WORDS.0)?,
        lesson_word: words(5, DEFAULT_WORDS.1)?,
        key: key(template, id),
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
        count: r.get(8)?,
    })
}

/// All, oldest first (the sidebar puts new ones last).
pub fn all(conn: &Connection) -> Result<Vec<Custom>, String> {
    let mut st = conn.prepare(&format!("{SQL} ORDER BY m.id")).map_err(err)?;
    let rows = st.query_map([], row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn get(conn: &Connection, id: i64) -> Result<Custom, String> {
    conn.query_row(&format!("{SQL} WHERE m.id = ?1"), [id], row).optional().map_err(err)?.ok_or_else(gone)
}

/// The module `id` made from `template` (a list id given to 循序渐进 is refused).
pub fn of(conn: &Connection, template: Template, id: i64) -> Result<Custom, String> {
    get(conn, id).ok().filter(|m| m.template == template).ok_or_else(gone)
}

/// By key ("list/3"); None for built-in modules and deleted ones.
pub fn by_key(conn: &Connection, k: &str) -> Option<Custom> {
    let (t, id) = parse_key(k)?;
    of(conn, t, id).ok()
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomPatch {
    /// Create only.
    #[serde(default)]
    pub template: Option<Template>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub course_word: Option<String>,
    #[serde(default)]
    pub lesson_word: Option<String>,
}

struct Fields {
    name: String,
    icon: String,
    words: Option<(String, String)>,
}

fn checked(conn: &Connection, template: Template, name: &str, icon: &str, words: (&str, &str), except: Option<i64>) -> Result<Fields, String> {
    let name = common::require(name, "名称", MAX_NAME)?;
    if TAKEN.contains(&name.as_str()) {
        return Err(format!("「{name}」已经是柚儿园里的名字了，换一个吧"));
    }
    let dup: i64 = conn.query_row("SELECT count(*) FROM custom_modules WHERE name = ?1 AND id != coalesce(?2, -1)", params![name, except], |r| r.get(0)).map_err(err)?;
    if dup > 0 {
        return Err(format!("已经有叫「{name}」的模块了"));
    }
    if !ICONS.contains(&icon) {
        return Err("请从列出的图标里选一个".into());
    }
    let words = match template {
        Template::List => None,
        Template::Progress => {
            let (c, l) = (common::require(words.0, "上一层的叫法", MAX_WORD)?, common::require(words.1, "下一层的叫法", MAX_WORD)?);
            if c == l {
                return Err("两层的叫法不能一样".into());
            }
            Some((c, l))
        }
    };
    Ok(Fields { name, icon: icon.to_string(), words })
}

pub fn create(conn: &Connection, p: CustomPatch) -> Result<Custom, String> {
    let template = p.template.ok_or("请选五花八门或循序渐进")?;
    let words = (p.course_word.as_deref().unwrap_or(DEFAULT_WORDS.0), p.lesson_word.as_deref().unwrap_or(DEFAULT_WORDS.1));
    let f = checked(conn, template, p.name.as_deref().unwrap_or(""), p.icon.as_deref().unwrap_or(ICONS[0]), words, None)?;
    let (c, l) = f.words.unzip();
    conn.execute("INSERT INTO custom_modules (template, name, icon, course_word, lesson_word) VALUES (?1, ?2, ?3, ?4, ?5)", params![template, f.name, f.icon, c, l]).map_err(err)?;
    get(conn, conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, p: CustomPatch) -> Result<Custom, String> {
    let old = get(conn, id)?;
    if p.template.is_some_and(|t| t != old.template) {
        return Err("建好以后不能换模板".into());
    }
    let words = (p.course_word.as_deref().unwrap_or(&old.course_word), p.lesson_word.as_deref().unwrap_or(&old.lesson_word));
    let f = checked(conn, old.template, p.name.as_deref().unwrap_or(&old.name), p.icon.as_deref().unwrap_or(&old.icon), words, Some(id))?;
    let (c, l) = f.words.unzip();
    conn.execute(
        &format!("UPDATE custom_modules SET name = ?1, icon = ?2, course_word = ?3, lesson_word = ?4, updated_at = {NOW} WHERE id = ?5"),
        params![f.name, f.icon, c, l, id],
    )
    .map_err(err)?;
    get(conn, id)
}

/// Deletes the module and everything in it (its 类型 list too).
pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    let m = get(conn, id).map_err(|_| gone())?;
    let tx = common::Tx::begin(conn)?;
    tx.execute("DELETE FROM kinds WHERE module = ?1", [key(m.template, id)]).map_err(err)?;
    tx.execute("DELETE FROM custom_modules WHERE id = ?1", [id]).map_err(err)?;
    tx.commit()
}

// ---------- export / import ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomRecord {
    pub template: Template,
    pub name: String,
    pub icon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub course_word: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lesson_word: Option<String>,
    /// 五花八门
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<lists::FieldRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<lists::ItemRecord>,
    /// 循序渐进: its 类型 (in order), then courses
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub courses: Vec<progress::CourseRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM custom_modules", [], |r| r.get(0)).map_err(err)
}

pub fn validate_records(rec: &[CustomRecord]) -> Result<(), String> {
    let mut names = std::collections::HashSet::new();
    for (i, m) in rec.iter().enumerate() {
        let name = common::require(&m.name, "名称", MAX_NAME).map_err(|e| format!("{LABEL}第 {} 个：{e}", i + 1))?;
        if !names.insert(name.clone()) {
            return Err(format!("{LABEL}：「{name}」重复了"));
        }
        let at = |e: String| format!("{}「{name}」：{e}", m.template.label());
        if TAKEN.contains(&name.as_str()) {
            return Err(at("和柚儿园里的名字重复了".into()));
        }
        if !ICONS.contains(&m.icon.as_str()) {
            return Err(at(format!("不认识的图标 {}", m.icon)));
        }
        match m.template {
            Template::List => {
                if !m.courses.is_empty() || !m.kinds.is_empty() {
                    return Err(at("五花八门没有课程和类型".into()));
                }
                lists::validate_records(&m.fields, &m.items).map_err(at)?;
            }
            Template::Progress => {
                if !m.fields.is_empty() || !m.items.is_empty() {
                    return Err(at("循序渐进没有字段和条目".into()));
                }
                let words = (m.course_word.as_deref().unwrap_or(DEFAULT_WORDS.0), m.lesson_word.as_deref().unwrap_or(DEFAULT_WORDS.1));
                for (w, what) in [(words.0, "上一层的叫法"), (words.1, "下一层的叫法")] {
                    common::require(w, what, MAX_WORD).map_err(at)?;
                }
                kinds::validate_names(&name, &m.kinds)?;
                progress::validate_records(&m.courses).map_err(at)?;
            }
        }
    }
    Ok(())
}

/// Replaces every made module (as-is). Caller owns the transaction.
pub fn replace_all(conn: &Connection, rec: &[CustomRecord]) -> Result<usize, String> {
    validate_records(rec)?;
    conn.execute("DELETE FROM kinds WHERE module LIKE 'progress/%'", []).map_err(err)?;
    conn.execute("DELETE FROM custom_modules", []).map_err(err)?;
    for m in rec {
        let words = match m.template {
            Template::List => (None, None),
            Template::Progress => (Some(m.course_word.as_deref().unwrap_or(DEFAULT_WORDS.0).trim()), Some(m.lesson_word.as_deref().unwrap_or(DEFAULT_WORDS.1).trim())),
        };
        conn.execute(
            &format!("INSERT INTO custom_modules (template, name, icon, course_word, lesson_word, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, coalesce(?6, {NOW}), coalesce(?7, ?6, {NOW}))"),
            params![m.template, m.name.trim(), m.icon, words.0, words.1, m.created_at, m.updated_at],
        )
        .map_err(err)?;
        let id = conn.last_insert_rowid();
        match m.template {
            Template::List => lists::import(conn, id, &m.fields, &m.items)?,
            Template::Progress => {
                kinds::replace_names(conn, &key(Template::Progress, id), &m.kinds)?;
                progress::import(conn, id, &m.courses)?
            }
        }
    }
    Ok(rec.len())
}

pub fn export(conn: &Connection) -> Result<Vec<CustomRecord>, String> {
    all(conn)?
        .into_iter()
        .map(|m| {
            let (fields, items, kind_names, courses) = match m.template {
                Template::List => {
                    let (f, i) = lists::export(conn, m.id)?;
                    (f, i, vec![], vec![])
                }
                Template::Progress => (vec![], vec![], kinds::list(conn, &key(Template::Progress, m.id))?.into_iter().map(|k| k.name).collect(), progress::export(conn, m.id)?),
            };
            let progress = m.template == Template::Progress;
            Ok(CustomRecord {
                template: m.template,
                name: m.name,
                icon: m.icon,
                course_word: progress.then_some(m.course_word),
                lesson_word: progress.then_some(m.lesson_word),
                fields,
                items,
                kinds: kind_names,
                courses,
                created_at: Some(m.created_at),
                updated_at: Some(m.updated_at),
            })
        })
        .collect()
}

// ---------- commands ----------

#[tauri::command]
pub fn custom_list(state: State<AppState>) -> Result<Vec<Custom>, String> {
    state.with_db(|c| all(c))
}

#[tauri::command]
pub fn custom_create(state: State<AppState>, patch: CustomPatch) -> Result<Custom, String> {
    let label = format!("新建{}", patch.template.map_or("模块", |t| t.label()));
    state.with_undo(&label, |c| create(c, patch))
}

#[tauri::command]
pub fn custom_update(state: State<AppState>, id: i64, patch: CustomPatch) -> Result<Custom, String> {
    state.with_undo("修改模块", |c| update(c, id, patch))
}

#[tauri::command]
pub fn custom_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    let name = state.with_db(|c| get(c, id))?.name;
    state.with_undo(&format!("删除「{name}」"), |c| delete(c, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    fn make(c: &Connection, v: serde_json::Value) -> Result<Custom, String> {
        create(c, serde_json::from_value(v).unwrap())
    }

    #[test]
    fn names_icons_words_and_keys() {
        let c = memory_db();
        let games = make(&c, json!({ "template": "list", "name": " 游戏 ", "icon": "gamepad-2" })).unwrap();
        assert_eq!((games.name.as_str(), games.key.as_str(), games.course_word.as_str()), ("游戏", "list/1", ""));
        let course = make(&c, json!({ "template": "progress", "name": "日语", "icon": "languages" })).unwrap();
        assert_eq!((course.course_word.as_str(), course.lesson_word.as_str(), course.key.as_str()), ("课程", "课节", "progress/2"));
        assert!(make(&c, json!({ "template": "list", "name": "游戏", "icon": "shapes" })).unwrap_err().contains("已经有"));
        assert!(make(&c, json!({ "template": "list", "name": "来日方长", "icon": "shapes" })).unwrap_err().contains("柚儿园"));
        assert!(make(&c, json!({ "template": "list", "name": "x", "icon": "bomb" })).unwrap_err().contains("图标"));
        assert!(make(&c, json!({ "template": "progress", "name": "y", "icon": "shapes", "courseWord": "季", "lessonWord": "季" })).is_err());
        let course = update(&c, course.id, serde_json::from_value(json!({ "courseWord": "季", "lessonWord": "集", "name": "动画" })).unwrap()).unwrap();
        assert_eq!((course.name.as_str(), course.course_word.as_str(), course.lesson_word.as_str()), ("动画", "季", "集"));
        assert!(update(&c, course.id, serde_json::from_value(json!({ "template": "list" })).unwrap()).is_err());
        assert_eq!(parse_key("progress/2"), Some((Template::Progress, 2)));
        assert!(by_key(&c, "list/2").is_none() && by_key(&c, "books").is_none());
        assert!(of(&c, Template::Progress, games.id).is_err());
        // ids are never reused
        delete(&c, course.id).unwrap();
        assert_eq!(make(&c, json!({ "template": "list", "name": "书单外", "icon": "shapes" })).unwrap().id, 3);
    }
}
