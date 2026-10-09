//! 出门走走 (packing): templates (scene lists) → trips. Creating a trip from a template — or from an
//! earlier trip (its items, all unpacked again) — copies the items; afterwards the lists are
//! independent (history stays intact). A trip copied from a trip records no template.
//! 类别 = kinds.rs (module "packing"), shared by template items and trip items.

use super::common::{self, err, some};
use super::kinds;
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

pub const KEY: &str = "packing";
pub const LABEL: &str = "出门走走";
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

fn gone(what: &str) -> String {
    format!("找不到这个{what}，可能已经被删除了")
}

// ---------- templates ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub note: Option<String>,
    pub sort_order: i64,
    pub item_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplatePatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
}

const TEMPLATE_SQL: &str = "SELECT t.id, t.name, t.note, t.sort_order,
    (SELECT count(*) FROM packing_template_items i WHERE i.template_id = t.id), t.created_at, t.updated_at
  FROM packing_templates t";

fn template_row(r: &Row) -> rusqlite::Result<Template> {
    Ok(Template {
        id: r.get(0)?,
        name: r.get(1)?,
        note: r.get(2)?,
        sort_order: r.get(3)?,
        item_count: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

pub fn templates(conn: &Connection) -> Result<Vec<Template>, String> {
    let mut st = conn.prepare(&format!("{TEMPLATE_SQL} ORDER BY t.sort_order, t.id")).map_err(err)?;
    let rows = st.query_map([], template_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn template(conn: &Connection, id: i64) -> Result<Template, String> {
    conn.query_row(&format!("{TEMPLATE_SQL} WHERE t.id = ?1"), [id], template_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("模板"))
}

fn clean_template(conn: &Connection, name: &str, note: Option<String>, except: Option<i64>) -> Result<(String, Option<String>), String> {
    let name = common::require(name, "模板名", 50)?;
    let note = common::clean(note);
    common::check_len(&note, "备注", 1000)?;
    let dup: i64 = conn
        .query_row(
            "SELECT count(*) FROM packing_templates WHERE name = ?1 AND id != coalesce(?2, -1)",
            params![name, except],
            |r| r.get(0),
        )
        .map_err(err)?;
    if dup > 0 {
        return Err(format!("已经有叫「{name}」的模板了"));
    }
    Ok((name, note))
}

pub fn create_template(conn: &Connection, patch: TemplatePatch) -> Result<Template, String> {
    let (name, note) = clean_template(conn, patch.name.as_deref().unwrap_or(""), patch.note.flatten(), None)?;
    conn.execute(
        "INSERT INTO packing_templates (name, note, sort_order)
         VALUES (?1, ?2, (SELECT coalesce(max(sort_order), 0) + 1 FROM packing_templates))",
        params![name, note],
    )
    .map_err(err)?;
    template(conn, conn.last_insert_rowid())
}

pub fn update_template(conn: &Connection, id: i64, patch: TemplatePatch) -> Result<Template, String> {
    let old = template(conn, id)?;
    let (name, note) =
        clean_template(conn, patch.name.as_deref().unwrap_or(&old.name), patch.note.unwrap_or(old.note), Some(id))?;
    conn.execute(
        &format!("UPDATE packing_templates SET name = ?1, note = ?2, updated_at = {NOW} WHERE id = ?3"),
        params![name, note, id],
    )
    .map_err(err)?;
    template(conn, id)
}

/// Its items go too; trips made from it keep their own lists (template_id → NULL).
pub fn delete_template(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM packing_templates WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(gone("模板")),
        _ => Ok(()),
    }
}

// ---------- trips ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trip {
    pub id: i64,
    pub name: String,
    pub template_id: Option<i64>,
    pub template_name: Option<String>,
    pub depart_on: Option<String>,
    pub note: Option<String>,
    pub total: i64,
    pub packed: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TripPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "some")]
    pub depart_on: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
}

const TRIP_SQL: &str = "SELECT t.id, t.name, t.template_id, p.name, t.depart_on, t.note,
    (SELECT count(*) FROM trip_items i WHERE i.trip_id = t.id),
    (SELECT count(*) FROM trip_items i WHERE i.trip_id = t.id AND i.packed = 1),
    t.created_at, t.updated_at
  FROM trips t LEFT JOIN packing_templates p ON p.id = t.template_id";

fn trip_row(r: &Row) -> rusqlite::Result<Trip> {
    Ok(Trip {
        id: r.get(0)?,
        name: r.get(1)?,
        template_id: r.get(2)?,
        template_name: r.get(3)?,
        depart_on: r.get(4)?,
        note: r.get(5)?,
        total: r.get(6)?,
        packed: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

/// Newest first.
pub fn trips(conn: &Connection) -> Result<Vec<Trip>, String> {
    let mut st = conn.prepare(&format!("{TRIP_SQL} ORDER BY t.id DESC")).map_err(err)?;
    let rows = st.query_map([], trip_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn trip(conn: &Connection, id: i64) -> Result<Trip, String> {
    conn.query_row(&format!("{TRIP_SQL} WHERE t.id = ?1"), [id], trip_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("出行"))
}

fn clean_trip(name: &str, depart_on: Option<String>, note: Option<String>) -> Result<(String, Option<String>, Option<String>), String> {
    let name = common::require(name, "出行名称", 100)?;
    let depart_on = common::clean(depart_on);
    common::check_date(&depart_on, "出发日期")?;
    let note = common::clean(note);
    common::check_len(&note, "备注", 1000)?;
    Ok((name, depart_on, note))
}

/// With `template_id`, copies that template's items (unpacked) into the new trip.
pub fn create_trip(conn: &Connection, patch: TripPatch, template_id: Option<i64>) -> Result<Trip, String> {
    let (name, depart_on, note) =
        clean_trip(patch.name.as_deref().unwrap_or(""), patch.depart_on.flatten(), patch.note.flatten())?;
    if let Some(t) = template_id {
        template(conn, t)?;
    }
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        "INSERT INTO trips (name, template_id, depart_on, note) VALUES (?1, ?2, ?3, ?4)",
        params![name, template_id, depart_on, note],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    if let Some(t) = template_id {
        tx.execute(
            "INSERT INTO trip_items (trip_id, kind_id, name, note, sort_order)
             SELECT ?1, kind_id, name, note, sort_order FROM packing_template_items WHERE template_id = ?2",
            params![id, t],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)?;
    trip(conn, id)
}

/// A new trip with a copy of an earlier trip's list (nothing packed yet).
pub fn copy_trip(conn: &Connection, patch: TripPatch, from_trip: i64) -> Result<Trip, String> {
    trip(conn, from_trip)?;
    let tx = common::Tx::begin(conn)?;
    let t = create_trip(&tx, patch, None)?;
    tx.execute(
        "INSERT INTO trip_items (trip_id, kind_id, name, note, sort_order)
         SELECT ?1, kind_id, name, note, sort_order FROM trip_items WHERE trip_id = ?2",
        params![t.id, from_trip],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    trip(conn, t.id)
}

pub fn update_trip(conn: &Connection, id: i64, patch: TripPatch) -> Result<Trip, String> {
    let old = trip(conn, id)?;
    let (name, depart_on, note) = clean_trip(
        patch.name.as_deref().unwrap_or(&old.name),
        patch.depart_on.unwrap_or(old.depart_on),
        patch.note.unwrap_or(old.note),
    )?;
    conn.execute(
        &format!("UPDATE trips SET name = ?1, depart_on = ?2, note = ?3, updated_at = {NOW} WHERE id = ?4"),
        params![name, depart_on, note, id],
    )
    .map_err(err)?;
    trip(conn, id)
}

pub fn delete_trip(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM trips WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(gone("出行")),
        _ => Ok(()),
    }
}

// ---------- items (template items and trip items share one shape) ----------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum List {
    Template,
    Trip,
}

impl List {
    fn table(self) -> &'static str {
        match self {
            List::Template => "packing_template_items",
            List::Trip => "trip_items",
        }
    }
    fn parent_col(self) -> &'static str {
        match self {
            List::Template => "template_id",
            List::Trip => "trip_id",
        }
    }
    fn packed_col(self) -> &'static str {
        match self {
            List::Template => "NULL",
            List::Trip => "packed",
        }
    }
    fn check_parent(self, conn: &Connection, id: i64) -> Result<(), String> {
        match self {
            List::Template => template(conn, id).map(|_| ()),
            List::Trip => trip(conn, id).map(|_| ()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: i64,
    /// template_id or trip_id
    pub parent_id: i64,
    pub kind_id: Option<i64>,
    pub name: String,
    pub note: Option<String>,
    /// Trip items only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packed: Option<bool>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "some")]
    pub kind_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
    /// Trip items only.
    #[serde(default)]
    pub packed: Option<bool>,
}

fn item_sql(list: List) -> String {
    format!(
        "SELECT id, {}, kind_id, name, note, {}, sort_order, created_at, updated_at FROM {}",
        list.parent_col(),
        list.packed_col(),
        list.table()
    )
}

fn item_row(r: &Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        kind_id: r.get(2)?,
        name: r.get(3)?,
        note: r.get(4)?,
        packed: r.get(5)?,
        sort_order: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

pub fn items(conn: &Connection, list: List, parent_id: i64) -> Result<Vec<Item>, String> {
    list.check_parent(conn, parent_id)?;
    let sql = format!("{} WHERE {} = ?1 ORDER BY sort_order, id", item_sql(list), list.parent_col());
    let mut st = conn.prepare(&sql).map_err(err)?;
    let rows = st.query_map([parent_id], item_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn item(conn: &Connection, list: List, id: i64) -> Result<Item, String> {
    conn.query_row(&format!("{} WHERE id = ?1", item_sql(list)), [id], item_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("物品"))
}

struct ItemFields {
    name: String,
    kind_id: Option<i64>,
    note: Option<String>,
    packed: bool,
}

fn validate_item(conn: &Connection, list: List, f: ItemFields) -> Result<ItemFields, String> {
    let name = common::require(&f.name, "物品", 100)?;
    let note = common::clean(f.note);
    common::check_len(&note, "备注", 1000)?;
    kinds::check(conn, KEY, f.kind_id)?;
    if list == List::Template && f.packed {
        return Err("模板里的物品没有「已打包」状态".into());
    }
    Ok(ItemFields { name, kind_id: f.kind_id, note, packed: f.packed })
}

/// Appends to the end of the list.
pub fn create_item(conn: &Connection, list: List, parent_id: i64, patch: ItemPatch) -> Result<Item, String> {
    list.check_parent(conn, parent_id)?;
    let f = validate_item(
        conn,
        list,
        ItemFields {
            name: patch.name.unwrap_or_default(),
            kind_id: patch.kind_id.flatten(),
            note: patch.note.flatten(),
            packed: patch.packed.unwrap_or(false),
        },
    )?;
    let (t, p) = (list.table(), list.parent_col());
    let next = format!("(SELECT coalesce(max(sort_order), 0) + 1 FROM {t} WHERE {p} = ?1)");
    match list {
        List::Template => conn.execute(
            &format!("INSERT INTO {t} ({p}, kind_id, name, note, sort_order) VALUES (?1, ?2, ?3, ?4, {next})"),
            params![parent_id, f.kind_id, f.name, f.note],
        ),
        List::Trip => conn.execute(
            &format!("INSERT INTO {t} ({p}, kind_id, name, note, packed, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, {next})"),
            params![parent_id, f.kind_id, f.name, f.note, f.packed],
        ),
    }
    .map_err(err)?;
    item(conn, list, conn.last_insert_rowid())
}

pub fn update_item(conn: &Connection, list: List, id: i64, patch: ItemPatch) -> Result<Item, String> {
    let old = item(conn, list, id)?;
    let f = validate_item(
        conn,
        list,
        ItemFields {
            name: patch.name.unwrap_or(old.name),
            kind_id: patch.kind_id.unwrap_or(old.kind_id),
            note: patch.note.unwrap_or(old.note),
            packed: patch.packed.or(old.packed).unwrap_or(false),
        },
    )?;
    let t = list.table();
    conn.execute(
        &format!("UPDATE {t} SET name = ?1, kind_id = ?2, note = ?3, updated_at = {NOW} WHERE id = ?4"),
        params![f.name, f.kind_id, f.note, id],
    )
    .map_err(err)?;
    if list == List::Trip {
        conn.execute("UPDATE trip_items SET packed = ?1 WHERE id = ?2", params![f.packed, id]).map_err(err)?;
    }
    item(conn, list, id)
}

pub fn delete_item(conn: &Connection, list: List, id: i64) -> Result<(), String> {
    match conn.execute(&format!("DELETE FROM {} WHERE id = ?1", list.table()), [id]).map_err(err)? {
        0 => Err(gone("物品")),
        _ => Ok(()),
    }
}

// ---------- import / export: { templates: [...], trips: [...] }, 类别 by name ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemRecord {
    #[serde(default)]
    pub category: Option<String>,
    pub name: String,
    #[serde(default)]
    pub note: Option<String>,
    /// Trip items only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateRecord {
    pub name: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub items: Vec<ItemRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TripRecord {
    pub name: String,
    /// Source template name (provenance only).
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub depart_on: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub items: Vec<ItemRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackingRecord {
    #[serde(default)]
    pub templates: Vec<TemplateRecord>,
    #[serde(default)]
    pub trips: Vec<TripRecord>,
}

impl PackingRecord {
    pub fn len(&self) -> usize {
        self.templates.len() + self.trips.len()
    }
}

fn check_item_record(r: &ItemRecord, where_: &str, j: usize) -> Result<(), String> {
    let e = |e: String| format!("{LABEL}「{where_}」第 {} 件（{}）：{e}", j + 1, r.name.trim());
    common::require(&r.name, "物品", 100).map_err(e)?;
    common::check_len(&common::clean(r.note.clone()), "备注", 1000).map_err(e)?;
    kinds::check_name(&r.category).map_err(e)
}

pub fn validate_records(rec: &PackingRecord) -> Result<(), String> {
    let mut names = std::collections::HashSet::new();
    for (i, t) in rec.templates.iter().enumerate() {
        let name = common::require(&t.name, "模板名", 50).map_err(|e| format!("{LABEL}第 {} 个模板：{e}", i + 1))?;
        if !names.insert(name.clone()) {
            return Err(format!("{LABEL}：模板「{name}」重复了"));
        }
        for (j, it) in t.items.iter().enumerate() {
            check_item_record(it, &name, j)?;
        }
    }
    for (i, t) in rec.trips.iter().enumerate() {
        clean_trip(&t.name, t.depart_on.clone(), t.note.clone()).map_err(|e| format!("{LABEL}第 {} 次出行：{e}", i + 1))?;
        for (j, it) in t.items.iter().enumerate() {
            check_item_record(it, t.name.trim(), j)?;
        }
    }
    Ok(())
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT (SELECT count(*) FROM packing_templates) + (SELECT count(*) FROM trips)", [], |r| r.get(0))
        .map_err(err)
}

fn insert_items(conn: &Connection, list: List, parent_id: i64, items: &[ItemRecord]) -> Result<(), String> {
    for (j, it) in items.iter().enumerate() {
        let kind_id = kinds::resolve(conn, KEY, it.category.as_deref())?;
        conn.execute(
            &format!(
                "INSERT INTO {} ({}, kind_id, name, note, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, coalesce(?6, {NOW}), coalesce(?7, ?6, {NOW}))",
                list.table(),
                list.parent_col()
            ),
            params![
                parent_id, kind_id, it.name.trim(), common::clean(it.note.clone()), j as i64 + 1, it.created_at, it.updated_at
            ],
        )
        .map_err(err)?;
        if list == List::Trip && it.packed == Some(true) {
            conn.execute("UPDATE trip_items SET packed = 1 WHERE id = ?1", [conn.last_insert_rowid()]).map_err(err)?;
        }
    }
    Ok(())
}

fn export_items(conn: &Connection, list: List, parent_id: i64, names: &HashMap<i64, String>) -> Result<Vec<ItemRecord>, String> {
    Ok(items(conn, list, parent_id)?
        .into_iter()
        .map(|i| ItemRecord {
            category: i.kind_id.and_then(|id| names.get(&id).cloned()),
            name: i.name,
            note: i.note,
            packed: i.packed,
            created_at: Some(i.created_at),
            updated_at: Some(i.updated_at),
        })
        .collect())
}

/// Templates in display order; trips oldest first (re-import keeps their relative order).
pub fn export(conn: &Connection) -> Result<PackingRecord, String> {
    let names = kinds::name_map(conn, KEY)?;
    let templates = templates(conn)?
        .into_iter()
        .map(|t| {
            Ok(TemplateRecord {
                items: export_items(conn, List::Template, t.id, &names)?,
                name: t.name,
                note: t.note,
                created_at: Some(t.created_at),
                updated_at: Some(t.updated_at),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let trips = trips(conn)?
        .into_iter()
        .rev()
        .map(|t| {
            Ok(TripRecord {
                items: export_items(conn, List::Trip, t.id, &names)?,
                name: t.name,
                template: t.template_name,
                depart_on: t.depart_on,
                note: t.note,
                created_at: Some(t.created_at),
                updated_at: Some(t.updated_at),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(PackingRecord { templates, trips })
}

/// Replaces all templates + trips. Caller owns the transaction.
pub fn replace_all(conn: &Connection, rec: &PackingRecord) -> Result<usize, String> {
    validate_records(rec)?;
    conn.execute_batch("DELETE FROM trips; DELETE FROM packing_templates;").map_err(err)?;
    let mut ids = HashMap::new();
    for (i, t) in rec.templates.iter().enumerate() {
        conn.execute(
            &format!(
                "INSERT INTO packing_templates (name, note, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, coalesce(?4, {NOW}), coalesce(?5, ?4, {NOW}))"
            ),
            params![t.name.trim(), common::clean(t.note.clone()), i as i64 + 1, t.created_at, t.updated_at],
        )
        .map_err(err)?;
        let id = conn.last_insert_rowid();
        ids.insert(t.name.trim().to_string(), id);
        insert_items(conn, List::Template, id, &t.items)?;
    }
    for t in &rec.trips {
        let template_id = t.template.as_deref().and_then(|n| ids.get(n.trim()).copied());
        conn.execute(
            &format!(
                "INSERT INTO trips (name, template_id, depart_on, note, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, coalesce(?5, {NOW}), coalesce(?6, ?5, {NOW}))"
            ),
            params![
                t.name.trim(), template_id, common::clean(t.depart_on.clone()), common::clean(t.note.clone()), t.created_at,
                t.updated_at
            ],
        )
        .map_err(err)?;
        insert_items(conn, List::Trip, conn.last_insert_rowid(), &t.items)?;
    }
    Ok(rec.len())
}

// ---------- commands ----------

#[tauri::command]
pub fn packing_templates_reorder(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| common::reorder(c, "packing_templates", "1 = 1", &[], &ids))
}
/// `list`: "template" | "trip"; `ids` = all items of that template / trip in the new order.
#[tauri::command]
pub fn packing_items_reorder(state: State<AppState>, list: String, parent_id: i64, ids: Vec<i64>) -> Result<(), String> {
    let list = match list.as_str() {
        "template" => List::Template,
        "trip" => List::Trip,
        _ => return Err("未知清单".into()),
    };
    state.with_undo("调整顺序", |c| common::reorder(c, list.table(), &format!("{} = ?1", list.parent_col()), &[&parent_id], &ids))
}
#[tauri::command]
pub fn packing_templates(state: State<AppState>) -> Result<Vec<Template>, String> {
    state.with_db(|c| templates(c))
}
#[tauri::command]
pub fn packing_template_create(state: State<AppState>, patch: TemplatePatch) -> Result<Template, String> {
    state.with_undo("添加模板", |c| create_template(c, patch))
}
#[tauri::command]
pub fn packing_template_update(state: State<AppState>, id: i64, patch: TemplatePatch) -> Result<Template, String> {
    state.with_undo("修改模板", |c| update_template(c, id, patch))
}
#[tauri::command]
pub fn packing_template_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除模板", |c| delete_template(c, id))
}
#[tauri::command]
pub fn packing_trips(state: State<AppState>) -> Result<Vec<Trip>, String> {
    state.with_db(|c| trips(c))
}
#[tauri::command]
/// From a template, from an earlier trip (`from_trip_id`), or blank.
pub fn packing_trip_create(state: State<AppState>, patch: TripPatch, template_id: Option<i64>, from_trip_id: Option<i64>) -> Result<Trip, String> {
    state.with_undo("新建出行", |c| match from_trip_id {
        Some(from) => copy_trip(c, patch, from),
        None => create_trip(c, patch, template_id),
    })
}
#[tauri::command]
pub fn packing_trip_update(state: State<AppState>, id: i64, patch: TripPatch) -> Result<Trip, String> {
    state.with_undo("修改出行", |c| update_trip(c, id, patch))
}
#[tauri::command]
pub fn packing_trip_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除出行", |c| delete_trip(c, id))
}
#[tauri::command]
pub fn packing_template_items(state: State<AppState>, template_id: i64) -> Result<Vec<Item>, String> {
    state.with_db(|c| items(c, List::Template, template_id))
}
#[tauri::command]
pub fn packing_template_item_create(state: State<AppState>, template_id: i64, patch: ItemPatch) -> Result<Item, String> {
    state.with_undo("添加物品", |c| create_item(c, List::Template, template_id, patch))
}
#[tauri::command]
pub fn packing_template_item_update(state: State<AppState>, id: i64, patch: ItemPatch) -> Result<Item, String> {
    state.with_undo("修改物品", |c| update_item(c, List::Template, id, patch))
}
#[tauri::command]
pub fn packing_template_item_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除物品", |c| delete_item(c, List::Template, id))
}
#[tauri::command]
pub fn packing_trip_items(state: State<AppState>, trip_id: i64) -> Result<Vec<Item>, String> {
    state.with_db(|c| items(c, List::Trip, trip_id))
}
#[tauri::command]
pub fn packing_trip_item_create(state: State<AppState>, trip_id: i64, patch: ItemPatch) -> Result<Item, String> {
    state.with_undo("添加物品", |c| create_item(c, List::Trip, trip_id, patch))
}
#[tauri::command]
pub fn packing_trip_item_update(state: State<AppState>, id: i64, patch: ItemPatch) -> Result<Item, String> {
    state.with_undo("修改物品", |c| update_item(c, List::Trip, id, patch))
}
#[tauri::command]
pub fn packing_trip_item_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除物品", |c| delete_item(c, List::Trip, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::modules::kinds::tests::kind_id;
    use serde_json::json;

    fn tp(v: serde_json::Value) -> TemplatePatch {
        serde_json::from_value(v).unwrap()
    }
    fn trp(v: serde_json::Value) -> TripPatch {
        serde_json::from_value(v).unwrap()
    }
    fn ip(v: serde_json::Value) -> ItemPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn templates_and_items() {
        let c = memory_db();
        let t = create_template(&c, tp(json!({"name": " 回家 "}))).unwrap();
        assert_eq!((t.name.as_str(), t.item_count), ("回家", 0));
        assert!(create_template(&c, tp(json!({"name": "回家"}))).unwrap_err().contains("已经有"));
        let key = kind_id(&c, "packing", "重要物品");
        let a = create_item(&c, List::Template, t.id, ip(json!({"name": "身份证", "kindId": key}))).unwrap();
        let b = create_item(&c, List::Template, t.id, ip(json!({"name": "充电宝", "note": " "}))).unwrap();
        assert_eq!((a.sort_order, b.sort_order, a.packed, b.note.clone()), (1, 2, None, None));
        assert!(create_item(&c, List::Template, t.id, ip(json!({"name": "x", "packed": true}))).is_err());
        assert!(create_item(&c, List::Template, t.id, ip(json!({"name": "x", "kindId": kind_id(&c, "books", "虚构")}))).is_err());
        assert!(create_item(&c, List::Template, t.id, ip(json!({"name": " "}))).is_err());
        let b = update_item(&c, List::Template, b.id, ip(json!({"note": "满电", "kindId": key}))).unwrap();
        assert_eq!((b.note.as_deref(), b.kind_id), (Some("满电"), Some(key)));
        assert_eq!(template(&c, t.id).unwrap().item_count, 2);
        delete_item(&c, List::Template, a.id).unwrap();
        assert_eq!(items(&c, List::Template, t.id).unwrap().len(), 1);
    }

    #[test]
    fn trip_copies_template_then_stays_independent() {
        let c = memory_db();
        let t = create_template(&c, tp(json!({"name": "出国"}))).unwrap();
        let key = kind_id(&c, "packing", "重要物品");
        create_item(&c, List::Template, t.id, ip(json!({"name": "护照", "kindId": key}))).unwrap();
        create_item(&c, List::Template, t.id, ip(json!({"name": "护肤品", "note": "随身最多 1L"}))).unwrap();

        let trip = create_trip(&c, trp(json!({"name": "2026 国庆", "departOn": "2026-10-01"})), Some(t.id)).unwrap();
        assert_eq!((trip.total, trip.packed, trip.template_name.as_deref()), (2, 0, Some("出国")));
        let ti = items(&c, List::Trip, trip.id).unwrap();
        assert_eq!(ti.iter().map(|i| (i.name.as_str(), i.packed)).collect::<Vec<_>>(), [("护照", Some(false)), ("护肤品", Some(false))]);
        assert_eq!((ti[0].kind_id, ti[1].note.as_deref()), (Some(key), Some("随身最多 1L")));

        let u = update_item(&c, List::Trip, ti[0].id, ip(json!({"packed": true}))).unwrap();
        assert_eq!(u.packed, Some(true));
        create_item(&c, List::Trip, trip.id, ip(json!({"name": "转换插头", "packed": true}))).unwrap();
        let tr = self::trip(&c, trip.id).unwrap();
        assert_eq!((tr.total, tr.packed), (3, 2));
        assert_eq!(template(&c, t.id).unwrap().item_count, 2); // template untouched

        delete_template(&c, t.id).unwrap();
        let tr = self::trip(&c, trip.id).unwrap();
        assert_eq!((tr.template_id, tr.total), (None, 3)); // trip survives

        // reuse this trip's list: everything unpacked again, no template recorded
        let again = copy_trip(&c, trp(json!({"name": "2027 春节"})), trip.id).unwrap();
        assert_eq!((again.total, again.packed, again.template_id), (3, 0, None));
        let ai = items(&c, List::Trip, again.id).unwrap();
        assert_eq!(ai.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["护照", "护肤品", "转换插头"]);
        assert_eq!((ai[0].kind_id, ai[1].note.as_deref()), (Some(key), Some("随身最多 1L")));
        assert_eq!(self::trip(&c, trip.id).unwrap().packed, 2); // the old trip is untouched
        assert!(copy_trip(&c, trp(json!({"name": "x"})), 999).is_err());
        delete_trip(&c, again.id).unwrap();

        let blank = create_trip(&c, trp(json!({"name": "临时"})), None).unwrap();
        assert_eq!(blank.total, 0);
        assert!(create_trip(&c, trp(json!({"name": "x"})), Some(999)).is_err());
        assert!(create_trip(&c, trp(json!({"name": "x", "departOn": "2026-02-30"})), None).is_err());
        assert_eq!(trips(&c).unwrap()[0].id, blank.id); // newest first
        let e = update_trip(&c, blank.id, trp(json!({"departOn": null, "note": "周末"}))).unwrap();
        assert_eq!(e.note.as_deref(), Some("周末"));
        delete_trip(&c, trip.id).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM trip_items", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn kind_counts_span_templates_and_trips() {
        let c = memory_db();
        let key = kind_id(&c, "packing", "随身");
        let t = create_template(&c, tp(json!({"name": "A"}))).unwrap();
        create_item(&c, List::Template, t.id, ip(json!({"name": "x", "kindId": key}))).unwrap();
        create_trip(&c, trp(json!({"name": "T"})), Some(t.id)).unwrap();
        let k = kinds::list(&c, "packing").unwrap().into_iter().find(|k| k.id == key).unwrap();
        assert_eq!(k.count, 2);
    }

    #[test]
    fn replace_all_nested() {
        let c = memory_db();
        create_template(&c, tp(json!({"name": "old"}))).unwrap();
        let rec: PackingRecord = serde_json::from_value(json!({
            "templates": [
                {"name": "回家", "items": [{"category": "重要物品", "name": "钥匙"}, {"category": "新类别", "name": "画笔"}]}
            ],
            "trips": [{"name": "端午", "template": "回家", "departOn": "2026-06-19",
                       "items": [{"category": "重要物品", "name": "钥匙", "packed": true}, {"name": "伞"}]}]
        }))
        .unwrap();
        assert_eq!(replace_all(&c, &rec).unwrap(), 2);
        let ts = templates(&c).unwrap();
        assert_eq!((ts.len(), ts[0].name.as_str(), ts[0].item_count), (1, "回家", 2));
        let it = items(&c, List::Template, ts[0].id).unwrap();
        assert_eq!(it[1].kind_id, Some(kind_id(&c, "packing", "新类别")));
        let tr = &trips(&c).unwrap()[0];
        assert_eq!((tr.template_id, tr.total, tr.packed), (Some(ts[0].id), 2, 1));

        let dup: PackingRecord = serde_json::from_value(json!({"templates": [{"name": "A"}, {"name": "A"}]})).unwrap();
        assert!(validate_records(&dup).unwrap_err().contains("重复"));
        let bad: PackingRecord = serde_json::from_value(json!({"trips": [{"name": "x", "departOn": "2026-13-01"}]})).unwrap();
        assert!(validate_records(&bad).is_err());
    }
}
