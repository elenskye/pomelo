//! The user's records, one small item each (v0.7 阶段 ② 改进: rules and facts by section / line):
//! - 素材 page: 简历素材 (material: category 教育 / 技能 / 项目 / 工作…; `stack` 技术栈), 基本信息 (profile: fields).
//! - 档案 page: 已确认事实 (fact: category = its group, title 事项, text 口径) and 经历档案 (fact, category
//!   经历档案: a Markdown body; `material` = the material it is about).
//! - 规则 page: 写作规则 (rule: one section; category 共用准则 / 素材润色 / 整体审查 / 写作规则 = which model task
//!   reads it, 写作规则: every task), 有效个人约定 (decision: category likewise, title 名称, text 内容), 个人范例 (example).
//! - 网申 page: 网申资料 (application: never a resume candidate).
//! Languages (阶段 ③): `lang` is `cn` or `en` for materials and 基本信息, `cn` / `en` / "" (both) for
//! rules, 约定 and 范例; facts, 经历档案 and 网申 are both languages' whatever it says. A 经历档案 hangs on a
//! Chinese material (`material`) and an English one (`material_en`).
//! Saving checks the revision the user started from; the replaced version goes to the history.
use super::*;
use crate::modules::common::Tx;

pub const KINDS: [&str; 7] = ["material", "profile", "fact", "rule", "decision", "example", "application"];
/// A 技术栈 line's point id (chosen like a point; in a resume by default only for 纯开发岗).
pub const STACK: &str = "技术栈";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    pub id: String,
    pub kind: String,
    pub category: String,
    pub title: String,
    /// A material's note line; the body of everything else.
    pub text: String,
    pub time: String,
    pub role: String,
    pub location: String,
    pub points: Vec<Point>,
    pub fields: Vec<HeaderField>,
    /// A material's 技术栈 (项目, 工作).
    pub stack: String,
    /// A 经历档案's material (its id), its English material.
    pub material: Option<String>,
    pub material_en: Option<String>,
    /// `cn`, `en`, or "" (both languages; rules, 约定, 范例).
    pub lang: String,
    /// `<path under transfer/>#<title>:<n>` for an imported one: a later import matches it.
    pub source: Option<String>,
    pub revision: i64,
    pub enabled: bool,
}

impl Default for Record {
    fn default() -> Self {
        Self { id: String::new(), kind: "material".into(), category: "项目".into(), title: String::new(), text: String::new(), time: String::new(), role: String::new(), location: String::new(), points: vec![], fields: vec![], stack: String::new(), material: None, material_en: None, lang: "cn".into(), source: None, revision: 0, enabled: true }
    }
}

impl Record {
    /// What can be chosen for a resume: the 技术栈 line (as the point `STACK`), then the points.
    pub fn choosable(&self) -> Vec<Point> {
        let label = if self.lang == "en" { "Tech Stack: " } else { "技术栈：" };
        let stack = (!self.stack.trim().is_empty()).then(|| Point { id: STACK.into(), text: format!("{label}{}", self.stack.trim()) });
        stack.into_iter().chain(self.points.iter().filter(|p| !(p.id == STACK && !self.stack.trim().is_empty())).cloned()).collect()
    }

    pub fn is_dossier(&self) -> bool {
        self.kind == "fact" && self.category == "经历档案"
    }

    /// Whether a task in `lang` uses it.
    pub fn for_lang(&self, lang: &str) -> bool {
        SHARED.contains(&self.kind.as_str()) || self.lang.is_empty() || self.lang == lang
    }
}

/// Kinds that are both languages' whatever their `lang` says.
pub const SHARED: [&str; 2] = ["fact", "application"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub selection_model: Option<String>,
    pub review_model: Option<String>,
    pub thinking: bool,
    pub cloud_allowed: bool,
    pub developer_role: bool,
    /// The language the page shows (`en`; else Chinese); 投石问路's 「做简历」 uses it.
    pub lang: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub id: i64,
    pub record_id: String,
    pub body: Record,
    pub saved_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub id: i64,
    pub label: String,
    pub body: Draft,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub records: Vec<Record>,
    pub revisions: Vec<Revision>,
    pub versions: Vec<Version>,
    pub settings: Settings,
}

fn rows<T>(c: &Connection, sql: &str, f: impl Fn(&rusqlite::Row) -> rusqlite::Result<T>) -> Result<Vec<T>, String> {
    let mut st = c.prepare(sql).map_err(err)?;
    let out = st.query_map([], f).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err);
    out
}

fn json<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, String> {
    serde_json::from_str(s).map_err(err)
}

pub fn load(c: &Connection) -> Result<Catalog, String> {
    let records = rows(c, "SELECT body FROM seeker_records ORDER BY rowid", |r| r.get::<_, String>(0))?.iter().map(|b| json(b)).collect::<Result<_, _>>()?;
    let revisions = rows(c, "SELECT id, record_id, body, saved_at FROM seeker_record_history ORDER BY id DESC", |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?, r.get(3)?)))?
        .into_iter()
        .map(|(id, record_id, body, saved_at)| Ok(Revision { id, record_id, body: json(&body)?, saved_at }))
        .collect::<Result<_, String>>()?;
    let versions = rows(c, "SELECT id, label, body, created_at FROM seeker_versions ORDER BY id DESC", |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?, r.get(3)?)))?
        .into_iter()
        .map(|(id, label, body, created_at)| Ok(Version { id, label, body: json(&body)?, created_at }))
        .collect::<Result<_, String>>()?;
    let raw: Option<String> = c.query_row("SELECT options FROM seeker_settings WHERE id = 1", [], |r| r.get(0)).optional().map_err(err)?;
    let settings = raw.map(|s| json(&s)).transpose()?.unwrap_or_default();
    Ok(Catalog { records, revisions, versions, settings })
}

pub fn validate_record(r: &Record) -> Result<(), String> {
    if r.id.is_empty() || r.title.trim().is_empty() {
        return Err("请填写名称".into());
    }
    if !KINDS.contains(&r.kind.as_str()) {
        return Err("未知的资料类型".into());
    }
    if !(super::LANGS.contains(&r.lang.as_str()) || (r.lang.is_empty() && !["material", "profile"].contains(&r.kind.as_str()))) {
        return Err("资料的语言不对".into());
    }
    let mut ids = HashSet::new();
    if r.points.iter().map(|p| &p.id).chain(r.fields.iter().map(|f| &f.id)).any(|id| id.is_empty() || !ids.insert(id)) {
        return Err("资料中有重复或空的内容编号".into());
    }
    Ok(())
}

/// New (revision 0) or the next version of the one the user opened (same revision).
pub fn put(c: &Connection, input: &Record) -> Result<(), String> {
    validate_record(input)?;
    let old: Option<String> = c.query_row("SELECT body FROM seeker_records WHERE id = ?1", [&input.id], |r| r.get(0)).optional().map_err(err)?;
    let mut record = input.clone();
    if let Some(body) = old {
        let prior: Record = json(&body)?;
        if prior.revision != input.revision {
            return Err("这条资料刚刚改过，请重新打开".into());
        }
        if prior == record {
            return Ok(());
        }
        c.execute("INSERT INTO seeker_record_history (record_id, body) VALUES (?1, ?2)", params![prior.id, body]).map_err(err)?;
        record.revision = prior.revision + 1;
    } else {
        if input.revision != 0 {
            return Err("这条资料已经删除了".into());
        }
        record.revision = 1;
    }
    c.execute("INSERT INTO seeker_records (id, source, body, revision) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(id) DO UPDATE SET source = excluded.source, body = excluded.body, revision = excluded.revision", params![record.id, record.source, serde_json::to_string(&record).map_err(err)?, record.revision])
        .map_err(|e| if e.to_string().contains("UNIQUE") { "这个来源已经导入过了".to_string() } else { err(e) })?;
    Ok(())
}

/// Removes a record; its last version goes to the history.
pub fn delete(c: &Connection, id: &str) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    let body: String = tx.query_row("SELECT body FROM seeker_records WHERE id = ?1", [id], |r| r.get(0)).map_err(|_| "这条资料已经删除了".to_string())?;
    tx.execute("INSERT INTO seeker_record_history (record_id, body) VALUES (?1, ?2)", params![id, body]).map_err(err)?;
    tx.execute("DELETE FROM seeker_records WHERE id = ?1", [id]).map_err(err)?;
    tx.commit()
}

pub fn replace(c: &Connection, catalog: &Catalog) -> Result<(), String> {
    for r in &catalog.records {
        validate_record(r)?;
        c.execute("INSERT INTO seeker_records (id, source, body, revision) VALUES (?1, ?2, ?3, ?4)", params![r.id, r.source, serde_json::to_string(r).map_err(err)?, r.revision]).map_err(err)?;
    }
    for r in &catalog.revisions {
        c.execute("INSERT INTO seeker_record_history (id, record_id, body, saved_at) VALUES (?1, ?2, ?3, ?4)", params![r.id, r.record_id, serde_json::to_string(&r.body).map_err(err)?, r.saved_at]).map_err(err)?;
    }
    for v in &catalog.versions {
        validate(&v.body)?;
        c.execute("INSERT INTO seeker_versions (id, label, body, created_at) VALUES (?1, ?2, ?3, ?4)", params![v.id, v.label, serde_json::to_string(&v.body).map_err(err)?, v.created_at]).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn picky_seeker_record_save(state: State<'_, AppState>, record: Record) -> Result<Workspace, String> {
    state.with_picky_undo("保存 Seeker 资料", |c| {
        put(c, &record)?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_seeker_record_delete(state: State<'_, AppState>, id: String) -> Result<Workspace, String> {
    state.with_picky_undo("删除 Seeker 资料", |c| {
        delete(c, &id)?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_seeker_settings(state: State<'_, AppState>, settings: Settings) -> Result<Workspace, String> {
    // Like the other assistants' settings: not undone.
    state.picky.with_db(|c| {
        set_settings(c, "options", &serde_json::to_string(&settings).map_err(err)?)?;
        read(c)
    })
}
