//! The user's records, one small item each (v0.7 阶段 ② 改进: rules and facts by section / line):
//! - 素材 page: 简历素材 (material: category 教育 / 技能 / 项目 / 工作…; `stack` 技术栈), 基本信息 (profile: fields).
//! - 档案 page: 已确认事实 (fact: category = its group, title 事项, text 口径) and 经历档案 (fact, category
//!   经历档案: a Markdown body; `material` = the material it is about).
//! - 规则 page: 规则 (rule: a title and its points, one sentence each, in order; category 共用准则 /
//!   写作规则 / 素材润色 / 整体审查 = which model task reads it, 写作规则: every Seeker task), 个人范例 (example).
//!   v0.7 refine R5: a rule's points (no body text), sent to models as `## 标题` and `1. …` (`rule_markdown`);
//!   an older rule's text is split into points when loaded (`normalize`: one per list item, table row or
//!   sentence), and an older 个人约定 (decision: title 名称, text 内容; no longer made) becomes a rule of
//!   one point. Loading does this, so the pages, models and data files only see the new shape; the
//!   database row keeps the old one until the record is saved again.
//! - 网申 page: 网申资料 (application: never a resume candidate).
//! Languages (阶段 ③): `lang` is `cn` or `en` for materials and 基本信息, `cn` / `en` / "" (both) for
//! rules, 约定 and 范例; facts, 经历档案 and 网申 are both languages' whatever it says. A 经历档案 hangs on a
//! Chinese material (`material`) and an English one (`material_en`).
//! Saving checks the revision the user started from; the replaced version goes to the history.
//! Groups (v0.7 refine R3): a built-in group of 素材 (`MATERIAL_GROUPS`) or 规则 (`RULE_GROUPS`) keeps
//! its name inside (the program lays out 教育, sends 素材润色 to polishing…) and may show another one
//! (`seeker_groups`); renaming a group of the user's own changes its records' category. Deleting a
//! group is deleting its records (the page sends their ids).
use super::*;
use crate::modules::common::Tx;

pub const KINDS: [&str; 6] = ["material", "profile", "fact", "rule", "example", "application"];
/// A 技术栈 line's point id (chosen like a point; by default only when the draft's JD is a development job).
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
    /// The language the page shows (`en`; else Chinese); 投石问路's 「做简历」 uses it.
    pub lang: String,
    /// knowledge/'s companies named by the user: folder → name (company.rs `named`); set by its own
    /// command, never by `picky_seeker_settings`.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub company_names: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub id: i64,
    pub record_id: String,
    pub body: Record,
    pub saved_at: String,
}

/// The name a built-in group shows (`lang`: the page's for 素材, "" for 规则).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupName {
    pub kind: String,
    pub lang: String,
    pub category: String,
    pub label: String,
}

/// 素材's groups the program knows (layout, 技术栈, Summary…).
pub const MATERIAL_GROUPS: [&str; 6] = ["教育", "技能", "项目", "工作", "证书", "摘要"];
/// 规则's groups: which functions read them (model.rs `reads`, Rater's qbank / practice).
pub const RULE_GROUPS: [&str; 9] = ["共用准则", "素材润色", "整体审查", "写作规则", "题库维护", "面试", "简历题", "通用题", "突击题"];

/// Older data files' 确认版本 are read and dropped (`versions`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub records: Vec<Record>,
    pub revisions: Vec<Revision>,
    pub groups: Vec<GroupName>,
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

/// A rule's text as points: a list item, a table row (「格：格」, the header row and the rule line left
/// out) or a sentence of a paragraph each; heading marks dropped. Ids `p1`, `p2`… (stable).
pub fn split_points(text: &str) -> Vec<Point> {
    let mut out: Vec<String> = vec![];
    let mut table_head = true;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            table_head = true;
            continue;
        }
        if line.starts_with('|') {
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            let rule_line = cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')));
            if !std::mem::take(&mut table_head) && !rule_line {
                out.push(cells.iter().filter(|c| !c.is_empty()).copied().collect::<Vec<_>>().join("："));
            }
            continue;
        }
        table_head = true;
        let bare = line.trim_start_matches('#').trim();
        let item = bare.strip_prefix("- ").or_else(|| bare.strip_prefix("* ")).or_else(|| bare.strip_prefix("• ")).or_else(|| {
            let digits = bare.chars().take_while(char::is_ascii_digit).count();
            (digits > 0).then(|| &bare[digits..]).and_then(|r| r.strip_prefix(". ").or_else(|| r.strip_prefix('、')).or_else(|| r.strip_prefix('.')))
        });
        match item {
            Some(item) => out.push(item.trim().to_string()),
            None => {
                // a paragraph: a sentence a point
                let mut sentence = String::new();
                for ch in bare.chars() {
                    sentence.push(ch);
                    if matches!(ch, '。' | '！' | '？') {
                        out.push(std::mem::take(&mut sentence).trim().to_string());
                    }
                }
                out.push(sentence.trim().to_string());
            }
        }
    }
    out.into_iter().filter(|p| !p.is_empty()).enumerate().map(|(i, text)| Point { id: format!("p{}", i + 1), text }).collect()
}

/// An older record in today's shape: a rule's text as its points, a 个人约定 as a rule of one point.
pub fn normalize(r: &mut Record) {
    if r.kind == "decision" {
        r.kind = "rule".into();
        r.points = vec![Point { id: "p1".into(), text: r.text.trim().to_string() }].into_iter().filter(|p| !p.text.is_empty()).collect();
        r.text.clear();
    }
    if r.kind == "rule" && !r.text.trim().is_empty() {
        if r.points.is_empty() {
            r.points = split_points(&r.text);
        }
        r.text.clear();
    }
}

/// The points numbered, one a line.
pub fn numbered(points: &[Point]) -> String {
    points.iter().filter(|p| !p.text.trim().is_empty()).enumerate().map(|(i, p)| format!("{}. {}", i + 1, p.text.trim())).collect::<Vec<_>>().join("\n")
}

/// A rule as models read it: `## 标题` and its points numbered.
pub fn rule_markdown(r: &Record) -> String {
    format!("## {}\n{}", r.title.trim(), numbered(&r.points))
}

/// Groups of rules as models read them: `# 组` (the heading given) and its rules; empty groups left out.
pub fn rules_markdown(groups: &[(String, Vec<&Record>)]) -> Vec<String> {
    groups.iter().filter(|(_, rules)| !rules.is_empty()).map(|(heading, rules)| format!("# {heading}\n\n{}", rules.iter().map(|r| rule_markdown(r)).collect::<Vec<_>>().join("\n\n"))).collect()
}

/// A rule group's name on the page (the user's for a built-in one).
pub fn group_label(catalog: &Catalog, category: &str) -> String {
    catalog.groups.iter().find(|g| g.kind == "rule" && g.category == category).map_or_else(|| category.to_string(), |g| g.label.clone())
}

pub fn load(c: &Connection) -> Result<Catalog, String> {
    let records = rows(c, "SELECT body FROM seeker_records ORDER BY sort, rowid", |r| r.get::<_, String>(0))?
        .iter()
        .map(|b| json::<Record>(b).map(|mut r| {
            normalize(&mut r);
            r
        }))
        .collect::<Result<_, _>>()?;
    let revisions = rows(c, "SELECT id, record_id, body, saved_at FROM seeker_record_history ORDER BY id DESC", |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?, r.get(3)?)))?
        .into_iter()
        .map(|(id, record_id, body, saved_at)| {
            let mut body: Record = json(&body)?;
            normalize(&mut body);
            Ok(Revision { id, record_id, body, saved_at })
        })
        .collect::<Result<_, String>>()?;
    let groups = rows(c, "SELECT kind, lang, category, label FROM seeker_groups ORDER BY kind, lang, category", |r| Ok(GroupName { kind: r.get(0)?, lang: r.get(1)?, category: r.get(2)?, label: r.get(3)? }))?;
    let raw: Option<String> = c.query_row("SELECT options FROM seeker_settings WHERE id = 1", [], |r| r.get(0)).optional().map_err(err)?;
    let settings = raw.map(|s| json(&s)).transpose()?.unwrap_or_default();
    Ok(Catalog { records, revisions, groups, settings })
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
    if r.kind == "rule" && (!r.text.trim().is_empty() || !r.points.iter().any(|p| !p.text.trim().is_empty())) {
        return Err("规则要有至少一个要点".into());
    }
    Ok(())
}

/// New (revision 0) or the next version of the one the user opened (same revision). An older shape is
/// taken as today's (`normalize`); a rule's empty points are dropped.
pub fn put(c: &Connection, input: &Record) -> Result<(), String> {
    let mut record = input.clone();
    normalize(&mut record);
    if record.kind == "rule" {
        record.points.retain(|p| !p.text.trim().is_empty());
        record.points.iter_mut().for_each(|p| p.text = p.text.trim().to_string());
    }
    validate_record(&record)?;
    let old: Option<String> = c.query_row("SELECT body FROM seeker_records WHERE id = ?1", [&input.id], |r| r.get(0)).optional().map_err(err)?;
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
    c.execute("INSERT INTO seeker_records (id, source, body, revision, sort) VALUES (?1, ?2, ?3, ?4, (SELECT coalesce(max(sort), 0) + 1 FROM seeker_records)) ON CONFLICT(id) DO UPDATE SET source = excluded.source, body = excluded.body, revision = excluded.revision", params![record.id, record.source, serde_json::to_string(&record).map_err(err)?, record.revision])
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

/// The user's order for some records (a group on a page): they take the places they had, in the new
/// order; the rest stay where they are.
pub fn reorder(c: &Connection, ids: &[String]) -> Result<(), String> {
    let mut places = vec![];
    for id in ids {
        places.push(c.query_row("SELECT sort FROM seeker_records WHERE id = ?1", [id], |r| r.get::<_, i64>(0)).map_err(|_| "有资料已经删除了，请刷新".to_string())?);
    }
    places.sort_unstable();
    for (id, place) in ids.iter().zip(places) {
        c.execute("UPDATE seeker_records SET sort = ?1 WHERE id = ?2", params![place, id]).map_err(err)?;
    }
    Ok(())
}

fn set_group_name(c: &Connection, kind: &str, lang: &str, category: &str, label: &str) -> Result<(), String> {
    let built_in = match kind {
        "material" => MATERIAL_GROUPS.contains(&category),
        "rule" => RULE_GROUPS.contains(&category),
        _ => false,
    };
    if !built_in {
        return Err("只有程序自带的分组能另起名字".into());
    }
    if label.trim().is_empty() || label.trim() == category {
        c.execute("DELETE FROM seeker_groups WHERE kind = ?1 AND lang = ?2 AND category = ?3", params![kind, lang, category]).map_err(err)?;
    } else {
        c.execute("INSERT INTO seeker_groups (kind, lang, category, label) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(kind, lang, category) DO UPDATE SET label = excluded.label", params![kind, lang, category, label.trim()]).map_err(err)?;
    }
    Ok(())
}

/// 重命名 a group of 素材 (`material`, one language) or 规则 (`rule`, both languages):
/// a built-in one only shows the new name; one of the user's own moves its records to the new category
/// (each a new version; into an existing group when the name is taken).
pub fn rename_group(c: &Connection, kind: &str, lang: &str, from: &str, to: &str) -> Result<(), String> {
    let to = to.trim();
    if to.is_empty() || to.chars().count() > 30 {
        return Err("分组名要 1～30 个字".into());
    }
    let (built_in, lang) = match kind {
        "material" => (MATERIAL_GROUPS.contains(&from), lang_of(lang)?),
        "rule" => (RULE_GROUPS.contains(&from), ""),
        _ => return Err("只有素材和规则的分组能改名".into()),
    };
    if built_in {
        return set_group_name(c, kind, lang, from, to);
    }
    let tx = Tx::begin(c)?;
    let all = load(&tx)?;
    let mine = |r: &&Record| r.category == from && if kind == "material" { r.kind == "material" && r.lang == lang } else { r.kind == "rule" };
    for r in all.records.iter().filter(mine) {
        put(&tx, &Record { category: to.into(), ..r.clone() })?;
    }
    tx.commit()
}

/// 启用 / 停用 several at once (each a new version, like an edit).
pub fn set_enabled(c: &Connection, ids: &[String], enabled: bool) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    let all = load(&tx)?;
    for id in ids {
        let r = all.records.iter().find(|r| &r.id == id).ok_or("有资料已经删除了，请刷新")?;
        if r.enabled != enabled {
            put(&tx, &Record { enabled, ..r.clone() })?;
        }
    }
    tx.commit()
}

pub fn replace(c: &Connection, catalog: &Catalog) -> Result<(), String> {
    for r in &catalog.records {
        let mut r = r.clone();
        normalize(&mut r);
        let r = &r;
        validate_record(r)?;
        c.execute("INSERT INTO seeker_records (id, source, body, revision, sort) VALUES (?1, ?2, ?3, ?4, (SELECT coalesce(max(sort), 0) + 1 FROM seeker_records))", params![r.id, r.source, serde_json::to_string(r).map_err(err)?, r.revision]).map_err(err)?;
    }
    for r in &catalog.revisions {
        let mut body = r.body.clone();
        normalize(&mut body);
        c.execute("INSERT INTO seeker_record_history (id, record_id, body, saved_at) VALUES (?1, ?2, ?3, ?4)", params![r.id, r.record_id, serde_json::to_string(&body).map_err(err)?, r.saved_at]).map_err(err)?;
    }
    for g in &catalog.groups {
        set_group_name(c, &g.kind, &g.lang, &g.category, &g.label)?;
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
pub async fn picky_seeker_records_order(state: State<'_, AppState>, ids: Vec<String>) -> Result<Workspace, String> {
    state.with_picky_undo("调整 Seeker 资料的顺序", |c| {
        reorder(c, &ids)?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_seeker_records_enable(state: State<'_, AppState>, ids: Vec<String>, enabled: bool) -> Result<Workspace, String> {
    state.with_picky_undo(&format!("{} {} 条 Seeker 资料", if enabled { "启用" } else { "停用" }, ids.len()), |c| {
        set_enabled(c, &ids, enabled)?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_seeker_records_delete(state: State<'_, AppState>, ids: Vec<String>) -> Result<Workspace, String> {
    state.with_picky_undo(&format!("删除 {} 条 Seeker 资料", ids.len()), |c| {
        let tx = Tx::begin(c)?;
        for id in &ids {
            delete(&tx, id)?;
        }
        tx.commit()?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_seeker_group_rename(state: State<'_, AppState>, kind: String, lang: String, from: String, to: String) -> Result<Workspace, String> {
    state.with_picky_undo(&format!("分组「{from}」改名"), |c| {
        rename_group(c, &kind, &lang, &from, &to)?;
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
    // Like the other assistants' settings: not undone. The company names stay (company.rs sets them).
    state.picky.with_db(|c| {
        let settings = Settings { company_names: settings_of(c)?.company_names, ..settings };
        set_settings(c, "options", &serde_json::to_string(&settings).map_err(err)?)?;
        read(c)
    })
}
