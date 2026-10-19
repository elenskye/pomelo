//! 五花八门 (v0.4.5): one list = its own fields + items. Rules (all here):
//! - Every item has 名称 (1–200 chars); a list adds up to `MAX_FIELDS` fields of 9 types: text 文字
//!   (200), longtext 长文字 (5000), number 数字, year 年份 ('2026', 1900–2100; files before v18 and
//!   0030 had 日期: a date keeps its year), select 单选 / multi 多选 (from the field's options),
//!   check 勾选, rating 评分 1–5, link 链接 (1000). A field's type never changes.
//! - Field names: 1–20 chars, unique in the list, not 名称. Options: up to `MAX_OPTIONS`, 1–20 chars,
//!   unique. `shown` = a column in the table (the panel always has every field).
//! - Values live in `list_items.data` as {"<field id>": value}; empty ones are left out (勾选: only
//!   true is kept). 单选 stores the option name, 多选 the names in the options' order. Renaming an
//!   option renames it in every item; removing one (or a field) takes it out of every item, in the
//!   same ⌘Z step.
//! - Item order: `sort_order` (new ones last; dragged in the table when it isn't sorted). Sorting and
//!   filtering are views in the page.

use super::common::{self, err, str_enum};
use super::custom::{self, Template};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use tauri::State;

pub const MAX_FIELDS: usize = 20;
pub const MAX_OPTIONS: usize = 50;
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

str_enum!(FieldType, default = Text, {
    Text => "text",
    LongText => "longtext",
    Number => "number",
    Year => "year",
    Select => "select",
    Multi => "multi",
    Check => "check",
    Rating => "rating",
    Link => "link",
});

impl FieldType {
    pub fn label(self) -> &'static str {
        match self {
            FieldType::Text => "文字",
            FieldType::LongText => "长文字",
            FieldType::Number => "数字",
            FieldType::Year => "年份",
            FieldType::Select => "单选",
            FieldType::Multi => "多选",
            FieldType::Check => "勾选",
            FieldType::Rating => "评分",
            FieldType::Link => "链接",
        }
    }
    fn has_options(self) -> bool {
        matches!(self, FieldType::Select | FieldType::Multi)
    }
}

// ---------- fields ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub id: i64,
    pub module_id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: FieldType,
    pub options: Vec<String>,
    pub shown: bool,
    pub sort_order: i64,
}

const FIELD_COLS: &str = "id, module_id, name, type, options, shown, sort_order";

fn field_row(r: &Row) -> rusqlite::Result<Field> {
    let options: String = r.get(4)?;
    Ok(Field { id: r.get(0)?, module_id: r.get(1)?, name: r.get(2)?, kind: r.get(3)?, options: serde_json::from_str(&options).unwrap_or_default(), shown: r.get(5)?, sort_order: r.get(6)? })
}

pub fn fields(conn: &Connection, module_id: i64) -> Result<Vec<Field>, String> {
    let mut st = conn.prepare(&format!("SELECT {FIELD_COLS} FROM list_fields WHERE module_id = ?1 ORDER BY sort_order, id")).map_err(err)?;
    let rows = st.query_map([module_id], field_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn field(conn: &Connection, id: i64) -> Result<Field, String> {
    conn.query_row(&format!("SELECT {FIELD_COLS} FROM list_fields WHERE id = ?1"), [id], field_row).optional().map_err(err)?.ok_or_else(|| "找不到这个字段，可能已经被删除了".to_string())
}

/// One option in an edit: `was` = its name before (None = new). Options left out are removed.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OptionInput {
    pub name: String,
    #[serde(default)]
    pub was: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldPatch {
    #[serde(default)]
    pub name: Option<String>,
    /// Create only.
    #[serde(default, rename = "type")]
    pub kind: Option<FieldType>,
    #[serde(default)]
    pub options: Option<Vec<OptionInput>>,
    #[serde(default)]
    pub shown: Option<bool>,
}

fn check_field_name(conn: &Connection, module_id: i64, name: &str, except: Option<i64>) -> Result<String, String> {
    let name = common::require(name, "字段名", 20)?;
    if name == "名称" {
        return Err("「名称」每个清单都有，换一个字段名吧".into());
    }
    let dup: i64 = conn.query_row("SELECT count(*) FROM list_fields WHERE module_id = ?1 AND name = ?2 AND id != coalesce(?3, -1)", params![module_id, name, except], |r| r.get(0)).map_err(err)?;
    if dup > 0 {
        return Err(format!("已经有叫「{name}」的字段了"));
    }
    Ok(name)
}

fn clean_options(names: impl IntoIterator<Item = String>) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = vec![];
    for n in names {
        let n = common::require(&n, "选项", 20)?;
        if out.contains(&n) {
            return Err(format!("选项「{n}」重复了"));
        }
        out.push(n);
    }
    if out.len() > MAX_OPTIONS {
        return Err(format!("选项最多 {MAX_OPTIONS} 个"));
    }
    Ok(out)
}

pub fn create_field(conn: &Connection, module_id: i64, p: FieldPatch) -> Result<Field, String> {
    custom::of(conn, Template::List, module_id)?;
    let kind = p.kind.ok_or("请选字段的类型")?;
    let name = check_field_name(conn, module_id, p.name.as_deref().unwrap_or(""), None)?;
    if fields(conn, module_id)?.len() >= MAX_FIELDS {
        return Err(format!("一个清单最多 {MAX_FIELDS} 个字段"));
    }
    let options = if kind.has_options() { clean_options(p.options.unwrap_or_default().into_iter().map(|o| o.name))? } else { vec![] };
    conn.execute(
        "INSERT INTO list_fields (module_id, name, type, options, shown, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, (SELECT coalesce(max(sort_order), 0) + 1 FROM list_fields WHERE module_id = ?1))",
        params![module_id, name, kind, serde_json::to_string(&options).map_err(err)?, p.shown.unwrap_or(true)],
    )
    .map_err(err)?;
    field(conn, conn.last_insert_rowid())
}

/// Renames / removes option values in every item of the list, along with the field.
pub fn update_field(conn: &Connection, id: i64, p: FieldPatch) -> Result<Field, String> {
    let old = field(conn, id)?;
    if p.kind.is_some_and(|k| k != old.kind) {
        return Err("字段建好以后不能换类型".into());
    }
    let name = check_field_name(conn, old.module_id, p.name.as_deref().unwrap_or(&old.name), Some(id))?;
    let tx = common::Tx::begin(conn)?;
    let mut options = old.options.clone();
    if let (Some(edit), true) = (p.options, old.kind.has_options()) {
        options = clean_options(edit.iter().map(|o| o.name.clone()))?;
        let renamed: HashMap<&str, &str> = edit.iter().filter_map(|o| o.was.as_deref().map(|w| (w, o.name.trim()))).collect();
        let key = id.to_string();
        for (item, mut data) in item_data(&tx, old.module_id)? {
            let Some(v) = data.get(&key).cloned() else { continue };
            let map = |s: &str| renamed.get(s).map(|n| n.to_string());
            let next = match (old.kind, v) {
                (FieldType::Select, Value::String(s)) => map(&s).map(Value::String),
                (_, Value::Array(xs)) => {
                    let mut ns: Vec<String> = xs.iter().filter_map(|x| x.as_str().and_then(map)).collect();
                    ns.sort_by_key(|n| options.iter().position(|o| o == n));
                    ns.dedup();
                    (!ns.is_empty()).then(|| Value::Array(ns.into_iter().map(Value::String).collect()))
                }
                _ => None,
            };
            match next {
                Some(n) => data.insert(key.clone(), n),
                None => data.remove(&key),
            };
            write_data(&tx, item, &data)?;
        }
    }
    tx.execute(
        &format!("UPDATE list_fields SET name = ?1, options = ?2, shown = ?3, updated_at = {NOW} WHERE id = ?4"),
        params![name, serde_json::to_string(&options).map_err(err)?, p.shown.unwrap_or(old.shown), id],
    )
    .map_err(err)?;
    tx.commit()?;
    field(conn, id)
}

/// Removes the field and its value from every item.
pub fn delete_field(conn: &Connection, id: i64) -> Result<(), String> {
    let f = field(conn, id)?;
    let tx = common::Tx::begin(conn)?;
    let key = id.to_string();
    for (item, mut data) in item_data(&tx, f.module_id)? {
        if data.remove(&key).is_some() {
            write_data(&tx, item, &data)?;
        }
    }
    tx.execute("DELETE FROM list_fields WHERE id = ?1", [id]).map_err(err)?;
    tx.commit()
}

// ---------- values ----------

fn text_of(v: &Value) -> Option<&str> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// A value for `f` as stored; None = empty (left out).
pub fn clean_value(f: &Field, v: &Value) -> Result<Option<Value>, String> {
    if v.is_null() {
        return Ok(None);
    }
    let bad = || format!("「{}」的值不对", f.name);
    let long = |n: usize| format!("「{}」太长了（最多 {n} 个字）", f.name);
    let string = |max: usize| -> Result<Option<Value>, String> {
        let s = v.as_str().ok_or_else(bad)?.trim();
        if s.chars().count() > max {
            return Err(long(max));
        }
        Ok((!s.is_empty()).then(|| Value::String(s.to_string())))
    };
    match f.kind {
        FieldType::Text => string(200),
        FieldType::LongText => string(5000),
        FieldType::Link => string(1000),
        FieldType::Number => {
            let n = match v {
                Value::Number(n) => n.as_f64(),
                Value::String(s) if s.trim().is_empty() => return Ok(None),
                Value::String(s) => s.trim().parse::<f64>().ok(),
                _ => None,
            };
            let n = n.filter(|n| n.is_finite() && n.abs() < 1e15).ok_or_else(|| format!("「{}」要填数字", f.name))?;
            // whole numbers stay integers (3, not 3.0)
            Ok(Some(if n.fract() == 0.0 { Value::from(n as i64) } else { serde_json::Number::from_f64(n).map(Value::Number).ok_or_else(bad)? }))
        }
        FieldType::Year => {
            let y = match v {
                Value::Number(n) => n.to_string(),
                _ => match text_of(v) {
                    Some(t) => t.to_string(),
                    None => return Ok(None),
                },
            };
            // a date (older files) keeps its year
            let y = if y.len() == 10 && common::check_date(&Some(y.clone()), &f.name).is_ok() { y[..4].to_string() } else { y };
            if !(y.len() == 4 && y.parse::<u32>().is_ok_and(|n| (1900..=2100).contains(&n))) {
                return Err(format!("「{}」要填 2026 这样的年份", f.name));
            }
            Ok(Some(Value::String(y)))
        }
        FieldType::Select => {
            let Some(s) = text_of(v) else { return Ok(None) };
            if !f.options.iter().any(|o| o == s) {
                return Err(format!("「{}」没有「{s}」这个选项", f.name));
            }
            Ok(Some(Value::String(s.to_string())))
        }
        FieldType::Multi => {
            let xs = v.as_array().ok_or_else(bad)?;
            let mut picked: Vec<&String> = vec![];
            for x in xs {
                let s = x.as_str().map(str::trim).ok_or_else(bad)?;
                let o = f.options.iter().find(|o| *o == s).ok_or_else(|| format!("「{}」没有「{s}」这个选项", f.name))?;
                if !picked.contains(&o) {
                    picked.push(o);
                }
            }
            picked.sort_by_key(|o| f.options.iter().position(|x| x == *o));
            Ok((!picked.is_empty()).then(|| Value::Array(picked.into_iter().map(|s| Value::String(s.clone())).collect())))
        }
        FieldType::Check => Ok(v.as_bool().ok_or_else(bad)?.then_some(Value::Bool(true))),
        FieldType::Rating => {
            let n = v.as_i64().filter(|n| (1..=5).contains(n)).ok_or_else(|| "评分只能是 1～5 星".to_string())?;
            Ok(Some(Value::from(n)))
        }
    }
}

/// How a value reads (小拾, search snippets).
pub fn value_text(f: &Field, v: &Value) -> String {
    match (f.kind, v) {
        (FieldType::Check, _) => "是".into(),
        (FieldType::Rating, Value::Number(n)) => "★".repeat(n.as_u64().unwrap_or(0) as usize),
        (FieldType::Number, Value::Number(n)) => n.as_f64().map(|x| if x.fract() == 0.0 { format!("{}", x as i64) } else { format!("{x}") }).unwrap_or_default(),
        (_, Value::Array(xs)) => xs.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("、"),
        (_, Value::String(s)) => s.clone(),
        _ => v.to_string(),
    }
}

// ---------- items ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: i64,
    pub module_id: i64,
    pub name: String,
    /// {"<field id>": value}
    pub values: Map<String, Value>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

const ITEM_COLS: &str = "id, module_id, name, data, sort_order, created_at, updated_at";

fn item_row(r: &Row) -> rusqlite::Result<Item> {
    let data: String = r.get(3)?;
    Ok(Item { id: r.get(0)?, module_id: r.get(1)?, name: r.get(2)?, values: serde_json::from_str(&data).unwrap_or_default(), sort_order: r.get(4)?, created_at: r.get(5)?, updated_at: r.get(6)? })
}

pub fn items(conn: &Connection, module_id: i64) -> Result<Vec<Item>, String> {
    let mut st = conn.prepare(&format!("SELECT {ITEM_COLS} FROM list_items WHERE module_id = ?1 ORDER BY sort_order, id")).map_err(err)?;
    let rows = st.query_map([module_id], item_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn item(conn: &Connection, id: i64) -> Result<Item, String> {
    conn.query_row(&format!("SELECT {ITEM_COLS} FROM list_items WHERE id = ?1"), [id], item_row).optional().map_err(err)?.ok_or_else(|| "找不到这一条，可能已经被删除了".to_string())
}

fn item_data(conn: &Connection, module_id: i64) -> Result<Vec<(i64, Map<String, Value>)>, String> {
    Ok(items(conn, module_id)?.into_iter().map(|i| (i.id, i.values)).collect())
}

fn write_data(conn: &Connection, id: i64, data: &Map<String, Value>) -> Result<(), String> {
    conn.execute("UPDATE list_items SET data = ?1 WHERE id = ?2", params![Value::Object(data.clone()).to_string(), id]).map(|_| ()).map_err(err)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemPatch {
    #[serde(default)]
    pub name: Option<String>,
    /// Only the fields given change (null empties one).
    #[serde(default)]
    pub values: Option<Map<String, Value>>,
}

/// `old` with `patch` applied, every value checked against the list's fields.
fn merged(conn: &Connection, module_id: i64, mut old: Map<String, Value>, patch: Option<Map<String, Value>>) -> Result<Map<String, Value>, String> {
    let fs = fields(conn, module_id)?;
    for (k, v) in patch.unwrap_or_default() {
        let f = fs.iter().find(|f| f.id.to_string() == k).ok_or("找不到这个字段，可能已经被删除了")?;
        match clean_value(f, &v)? {
            Some(x) => old.insert(k, x),
            None => old.remove(&k),
        };
    }
    Ok(old)
}

pub fn create_item(conn: &Connection, module_id: i64, p: ItemPatch) -> Result<Item, String> {
    custom::of(conn, Template::List, module_id)?;
    let name = common::require(p.name.as_deref().unwrap_or(""), "名称", 200)?;
    let data = merged(conn, module_id, Map::new(), p.values)?;
    conn.execute(
        "INSERT INTO list_items (module_id, name, data, sort_order) VALUES (?1, ?2, ?3, (SELECT coalesce(max(sort_order), 0) + 1 FROM list_items WHERE module_id = ?1))",
        params![module_id, name, Value::Object(data).to_string()],
    )
    .map_err(err)?;
    item(conn, conn.last_insert_rowid())
}

pub fn update_item(conn: &Connection, id: i64, p: ItemPatch) -> Result<Item, String> {
    let old = item(conn, id)?;
    let name = common::require(p.name.as_deref().unwrap_or(&old.name), "名称", 200)?;
    let data = merged(conn, old.module_id, old.values.clone(), p.values)?;
    if name == old.name && data == old.values {
        return Ok(old);
    }
    conn.execute(&format!("UPDATE list_items SET name = ?1, data = ?2, updated_at = {NOW} WHERE id = ?3"), params![name, Value::Object(data).to_string(), id]).map_err(err)?;
    item(conn, id)
}

pub fn delete_item(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM list_items WHERE id = ?1", [id]).map_err(err)? {
        0 => Err("找不到这一条，可能已经被删除了".into()),
        _ => Ok(()),
    }
}

// ---------- export / import (fields and values by name) ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldRecord {
    pub name: String,
    #[serde(rename = "type", deserialize_with = "record_type")]
    pub kind: FieldType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default = "yes")]
    pub shown: bool,
}

fn yes() -> bool {
    true
}

/// Files before v18 had 日期 fields: read as 年份 (their values keep the year).
fn record_type<'de, D: serde::Deserializer<'de>>(d: D) -> Result<FieldType, D::Error> {
    let s = String::deserialize(d)?;
    FieldType::parse(if s == "date" { "year" } else { &s }).ok_or_else(|| serde::de::Error::custom(format!("没有「{s}」这种字段")))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemRecord {
    pub name: String,
    /// {"<field name>": value}
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub values: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Fields as they would be stored (ids by position), for checking records.
fn record_fields(fs: &[FieldRecord]) -> Result<Vec<Field>, String> {
    if fs.len() > MAX_FIELDS {
        return Err(format!("字段最多 {MAX_FIELDS} 个"));
    }
    let mut out: Vec<Field> = vec![];
    for (i, f) in fs.iter().enumerate() {
        let name = common::require(&f.name, "字段名", 20)?;
        if name == "名称" || out.iter().any(|o| o.name == name) {
            return Err(format!("字段「{name}」重复了"));
        }
        let options = if f.kind.has_options() { clean_options(f.options.iter().cloned())? } else { vec![] };
        out.push(Field { id: i as i64, module_id: 0, name, kind: f.kind, options, shown: f.shown, sort_order: i as i64 });
    }
    Ok(out)
}

/// Values by field name → by field id (the ids `fs` carry).
fn values_by_id(fs: &[Field], values: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for (name, v) in values {
        let f = fs.iter().find(|f| f.name == name.trim()).ok_or_else(|| format!("没有叫「{name}」的字段"))?;
        if let Some(x) = clean_value(f, v)? {
            out.insert(f.id.to_string(), x);
        }
    }
    Ok(out)
}

pub fn validate_records(fs: &[FieldRecord], items: &[ItemRecord]) -> Result<(), String> {
    let fs = record_fields(fs)?;
    for (i, it) in items.iter().enumerate() {
        let at = |e: String| format!("第 {} 条（{}）：{e}", i + 1, it.name.trim());
        common::require(&it.name, "名称", 200).map_err(at)?;
        values_by_id(&fs, &it.values).map_err(at)?;
    }
    Ok(())
}

/// Writes a list's fields and items (already checked). Caller owns the transaction.
pub fn import(conn: &Connection, module_id: i64, fs: &[FieldRecord], items: &[ItemRecord]) -> Result<(), String> {
    let mut stored = record_fields(fs)?;
    for (i, f) in stored.iter_mut().enumerate() {
        conn.execute(
            "INSERT INTO list_fields (module_id, name, type, options, shown, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![module_id, f.name, f.kind, serde_json::to_string(&f.options).map_err(err)?, f.shown, i as i64 + 1],
        )
        .map_err(err)?;
        f.id = conn.last_insert_rowid();
    }
    for (i, it) in items.iter().enumerate() {
        conn.execute(
            &format!("INSERT INTO list_items (module_id, name, data, sort_order, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, coalesce(?5, {NOW}), coalesce(?6, ?5, {NOW}))"),
            params![module_id, it.name.trim(), Value::Object(values_by_id(&stored, &it.values)?).to_string(), i as i64 + 1, it.created_at, it.updated_at],
        )
        .map_err(err)?;
    }
    Ok(())
}

pub fn export(conn: &Connection, module_id: i64) -> Result<(Vec<FieldRecord>, Vec<ItemRecord>), String> {
    let fs = fields(conn, module_id)?;
    let records = fs.iter().map(|f| FieldRecord { name: f.name.clone(), kind: f.kind, options: f.options.clone(), shown: f.shown }).collect();
    let items = items(conn, module_id)?
        .into_iter()
        .map(|it| {
            // in the fields' order
            let values = fs.iter().filter_map(|f| it.values.get(&f.id.to_string()).map(|v| (f.name.clone(), v.clone()))).collect();
            ItemRecord { name: it.name, values, created_at: Some(it.created_at), updated_at: Some(it.updated_at) }
        })
        .collect();
    Ok((records, items))
}

// ---------- commands ----------

#[tauri::command]
pub fn list_fields(state: State<AppState>, module_id: i64) -> Result<Vec<Field>, String> {
    state.with_db(|c| fields(c, module_id))
}

#[tauri::command]
pub fn list_field_create(state: State<AppState>, module_id: i64, patch: FieldPatch) -> Result<Field, String> {
    state.with_undo("添加字段", |c| create_field(c, module_id, patch))
}

#[tauri::command]
pub fn list_field_update(state: State<AppState>, id: i64, patch: FieldPatch) -> Result<Field, String> {
    state.with_undo("修改字段", |c| update_field(c, id, patch))
}

#[tauri::command]
pub fn list_field_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除字段", |c| delete_field(c, id))
}

#[tauri::command]
pub fn list_fields_reorder(state: State<AppState>, module_id: i64, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| common::reorder(c, "list_fields", "module_id = ?1", &[&module_id], &ids))
}

#[tauri::command]
pub fn list_items(state: State<AppState>, module_id: i64) -> Result<Vec<Item>, String> {
    state.with_db(|c| items(c, module_id))
}

#[tauri::command]
pub fn list_item_create(state: State<AppState>, module_id: i64, patch: ItemPatch) -> Result<Item, String> {
    state.with_undo("添加条目", |c| create_item(c, module_id, patch))
}

#[tauri::command]
pub fn list_item_update(state: State<AppState>, id: i64, patch: ItemPatch) -> Result<Item, String> {
    state.with_undo("修改条目", |c| update_item(c, id, patch))
}

#[tauri::command]
pub fn list_item_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除条目", |c| delete_item(c, id))
}

#[tauri::command]
pub fn list_items_reorder(state: State<AppState>, module_id: i64, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| common::reorder(c, "list_items", "module_id = ?1", &[&module_id], &ids))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    fn from<T: serde::de::DeserializeOwned>(v: Value) -> T {
        serde_json::from_value(v).unwrap()
    }

    /// 游戏: 平台 (单选), 标签 (多选), 评分, 通关 (勾选), 时长 (数字).
    pub fn seed(c: &Connection) -> (i64, [i64; 5]) {
        let m = custom::create(c, from(json!({ "template": "list", "name": "游戏", "icon": "gamepad-2" }))).unwrap().id;
        let f = |v: Value| create_field(c, m, from(v)).unwrap().id;
        let ids = [
            f(json!({ "name": "平台", "type": "select", "options": [{ "name": "Switch" }, { "name": "PC" }] })),
            f(json!({ "name": "标签", "type": "multi", "options": [{ "name": "解谜" }, { "name": "冒险" }, { "name": "独立" }] })),
            f(json!({ "name": "评分", "type": "rating" })),
            f(json!({ "name": "通关", "type": "check" })),
            f(json!({ "name": "时长", "type": "number", "shown": false })),
        ];
        (m, ids)
    }

    fn vals(pairs: &[(i64, Value)]) -> Value {
        Value::Object(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
    }

    #[test]
    fn years_keep_only_the_year() {
        let c = memory_db();
        let (m, _) = seed(&c);
        let y = create_field(&c, m, from(json!({ "name": "玩过", "type": "year" }))).unwrap();
        assert_eq!(y.kind.label(), "年份");
        let it = create_item(&c, m, from(json!({ "name": "塞尔达", "values": vals(&[(y.id, json!("2025"))]) }))).unwrap();
        for (v, want) in [(json!(2024), json!("2024")), (json!("2023-05-06"), json!("2023")), (json!(""), Value::Null)] {
            let it = update_item(&c, it.id, from(json!({ "values": vals(&[(y.id, v)]) }))).unwrap();
            assert_eq!(it.values.get(&y.id.to_string()).cloned().unwrap_or(Value::Null), want);
        }
        for bad in [json!("26"), json!("1800"), json!("明年")] {
            assert!(update_item(&c, it.id, from(json!({ "values": vals(&[(y.id, bad)]) }))).unwrap_err().contains("年份"));
        }
        // files before v18: a 日期 field is read as 年份
        let rec: FieldRecord = from(json!({ "name": "玩过", "type": "date" }));
        assert_eq!(rec.kind, FieldType::Year);
        let items = vec![ItemRecord { name: "x".into(), values: json!({ "玩过": "2025-03-04" }).as_object().unwrap().clone(), created_at: None, updated_at: None }];
        validate_records(&[rec], &items).unwrap();
    }

    #[test]
    fn values_follow_their_fields() {
        let c = memory_db();
        let (m, [plat, tags, rate, done, hours]) = seed(&c);
        let it = create_item(&c, m, from(json!({ "name": "塞尔达", "values": vals(&[(plat, json!("Switch")), (tags, json!(["冒险", "解谜", "冒险"])), (rate, json!(5)), (done, json!(false)), (hours, json!("120.5"))]) }))).unwrap();
        assert_eq!(Value::Object(it.values.clone()), vals(&[(plat, json!("Switch")), (tags, json!(["解谜", "冒险"])), (rate, json!(5)), (hours, json!(120.5))]));
        for (f, v, e) in [(plat, json!("Xbox"), "没有「Xbox」"), (rate, json!(6), "1～5"), (hours, json!("很久"), "数字"), (999, json!(1), "字段")] {
            assert!(update_item(&c, it.id, from(json!({ "values": vals(&[(f, v)]) }))).unwrap_err().contains(e), "{e}");
        }
        // a partial update touches only what it names; null empties
        let it = update_item(&c, it.id, from(json!({ "values": vals(&[(done, json!(true)), (rate, Value::Null)]) }))).unwrap();
        assert_eq!((it.values.get(&done.to_string()), it.values.get(&rate.to_string())), (Some(&json!(true)), None));
        assert!(it.values.contains_key(&plat.to_string()));
        assert!(create_item(&c, m, from(json!({ "name": " " }))).is_err());
        // fields: name rules, type fixed
        assert!(create_field(&c, m, from(json!({ "name": "名称", "type": "text" }))).is_err());
        assert!(create_field(&c, m, from(json!({ "name": "平台", "type": "text" }))).unwrap_err().contains("已经有"));
        assert!(update_field(&c, plat, from(json!({ "type": "multi" }))).is_err());
    }

    #[test]
    fn options_renamed_and_removed_everywhere() {
        let c = memory_db();
        let (m, [plat, tags, _, _, hours]) = seed(&c);
        let a = create_item(&c, m, from(json!({ "name": "A", "values": vals(&[(plat, json!("PC")), (tags, json!(["解谜", "独立"])), (hours, json!(3))]) }))).unwrap();
        update_field(&c, tags, from(json!({ "options": [{ "name": "独立游戏", "was": "独立" }, { "name": "冒险", "was": "冒险" }, { "name": "动作" }] }))).unwrap();
        update_field(&c, plat, from(json!({ "options": [{ "name": "Switch", "was": "Switch" }] }))).unwrap();
        let a = item(&c, a.id).unwrap();
        assert_eq!(Value::Object(a.values.clone()), vals(&[(tags, json!(["独立游戏"])), (hours, json!(3))]));
        assert_eq!(field(&c, tags).unwrap().options, ["独立游戏", "冒险", "动作"]);
        delete_field(&c, hours).unwrap();
        assert_eq!(Value::Object(item(&c, a.id).unwrap().values), vals(&[(tags, json!(["独立游戏"]))]));
        assert_eq!(fields(&c, m).unwrap().len(), 4);
    }

    #[test]
    fn export_import_roundtrip() {
        let a = memory_db();
        let (m, [plat, tags, rate, done, _]) = seed(&a);
        create_item(&a, m, from(json!({ "name": "塞尔达", "values": vals(&[(plat, json!("Switch")), (tags, json!(["冒险"])), (rate, json!(5)), (done, json!(true))]) }))).unwrap();
        create_item(&a, m, from(json!({ "name": "空的" }))).unwrap();
        let (fs, items) = export(&a, m).unwrap();
        assert_eq!(serde_json::to_value(&items[0].values).unwrap(), json!({ "平台": "Switch", "标签": ["冒险"], "评分": 5, "通关": true }));
        validate_records(&fs, &items).unwrap();
        let b = memory_db();
        let m2 = custom::create(&b, from(json!({ "template": "list", "name": "游戏", "icon": "gamepad-2" }))).unwrap().id;
        import(&b, m2, &fs, &items).unwrap();
        let (fs2, items2) = export(&b, m2).unwrap();
        assert_eq!(serde_json::to_value(&fs).unwrap(), serde_json::to_value(&fs2).unwrap());
        assert_eq!(serde_json::to_value(&items).unwrap(), serde_json::to_value(&items2).unwrap());
        let bad = vec![ItemRecord { name: "x".into(), values: json!({ "平台": "Xbox" }).as_object().unwrap().clone(), created_at: None, updated_at: None }];
        assert!(validate_records(&fs, &bad).unwrap_err().contains("Xbox"));
    }
}
