//! 类型 lists per module, user-editable (add / rename / delete / reorder).
//! Items store `kind_id` (FK ON DELETE SET NULL → 未分类). Import/export uses kind *names*.

use super::common::{self, err};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tauri::State;

/// module key → item tables that reference kinds (counts sum over all of them).
const TABLES: &[(&str, &[&str])] = &[
    ("books", &["books"]),
    ("films", &["films"]),
    ("drawing", &["drawing_lessons"]),
    ("packing", &["packing_template_items", "trip_items"]), // 类别
];

fn tables(module: &str) -> Result<&'static [&'static str], String> {
    TABLES
        .iter()
        .find(|(m, _)| *m == module)
        .map(|(_, t)| *t)
        .ok_or_else(|| format!("未知模块：{module}"))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Kind {
    pub id: i64,
    pub module: String,
    pub name: String,
    pub sort_order: i64,
    /// Items currently using this kind.
    pub count: i64,
}

pub fn list(conn: &Connection, module: &str) -> Result<Vec<Kind>, String> {
    let count = tables(module)?
        .iter()
        .map(|t| format!("(SELECT count(*) FROM {t} WHERE kind_id = k.id)"))
        .collect::<Vec<_>>()
        .join(" + ");
    let mut st = conn
        .prepare(&format!(
            "SELECT k.id, k.module, k.name, k.sort_order, {count}
             FROM kinds k WHERE k.module = ?1 ORDER BY k.sort_order, k.id"
        ))
        .map_err(err)?;
    let rows = st
        .query_map([module], |r| {
            Ok(Kind { id: r.get(0)?, module: r.get(1)?, name: r.get(2)?, sort_order: r.get(3)?, count: r.get(4)? })
        })
        .map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

fn module_of(conn: &Connection, id: i64) -> Result<Option<String>, String> {
    conn.query_row("SELECT module FROM kinds WHERE id = ?1", [id], |r| r.get(0))
        .optional()
        .map_err(err)
}

fn get(conn: &Connection, id: i64) -> Result<Kind, String> {
    let module = module_of(conn, id)?.ok_or("找不到这个类型，可能已经被删除了")?;
    list(conn, &module)?.into_iter().find(|k| k.id == id).ok_or_else(|| "找不到这个类型".into())
}

fn clean_name(conn: &Connection, module: &str, name: &str, except: Option<i64>) -> Result<String, String> {
    let name = common::require(name, "名称", 20)?;
    let dup: i64 = conn
        .query_row(
            "SELECT count(*) FROM kinds WHERE module = ?1 AND name = ?2 AND id != coalesce(?3, -1)",
            params![module, name, except],
            |r| r.get(0),
        )
        .map_err(err)?;
    if dup > 0 {
        return Err(format!("「{name}」已经有了，换个名字吧"));
    }
    Ok(name)
}

pub fn create(conn: &Connection, module: &str, name: &str) -> Result<Kind, String> {
    tables(module)?;
    let name = clean_name(conn, module, name, None)?;
    conn.execute(
        "INSERT INTO kinds (module, name, sort_order)
         VALUES (?1, ?2, (SELECT coalesce(max(sort_order), 0) + 1 FROM kinds WHERE module = ?1))",
        params![module, name],
    )
    .map_err(err)?;
    get(conn, conn.last_insert_rowid())
}

pub fn rename(conn: &Connection, id: i64, name: &str) -> Result<Kind, String> {
    let k = get(conn, id)?;
    let name = clean_name(conn, &k.module, name, Some(id))?;
    conn.execute(
        "UPDATE kinds SET name = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2",
        params![name, id],
    )
    .map_err(err)?;
    get(conn, id)
}

/// Items of this kind become 未分类 (FK ON DELETE SET NULL).
pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM kinds WHERE id = ?1", [id]).map_err(err)? {
        0 => Err("找不到这个类型，可能已经被删除了".into()),
        _ => Ok(()),
    }
}

/// `ids` must be exactly the module's kinds, in the new order.
pub fn reorder(conn: &Connection, module: &str, ids: &[i64]) -> Result<Vec<Kind>, String> {
    let mut current: Vec<i64> = list(conn, module)?.iter().map(|k| k.id).collect();
    let mut wanted = ids.to_vec();
    current.sort_unstable();
    wanted.sort_unstable();
    if current != wanted {
        return Err("类型列表已经变了，请重新打开后再试".into());
    }
    let tx = conn.unchecked_transaction().map_err(err)?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute("UPDATE kinds SET sort_order = ?1 WHERE id = ?2", params![i as i64 + 1, id])
            .map_err(err)?;
    }
    tx.commit().map_err(err)?;
    list(conn, module)
}

/// Validates an item's kind_id belongs to `module` (None = 未分类 is always fine).
pub fn check(conn: &Connection, module: &str, kind_id: Option<i64>) -> Result<(), String> {
    match kind_id {
        Some(id) if module_of(conn, id)?.as_deref() != Some(module) => Err("选择的类型不存在，可能已经被删除了".into()),
        _ => Ok(()),
    }
}

/// v0.1 fixed codes (phase 3 files) → names.
fn legacy(code: &str) -> Option<&'static str> {
    Some(match code {
        "fiction" => "虚构",
        "nonfiction" => "非虚构",
        "comic" => "漫画",
        "other" => "其他",
        "movie" => "电影",
        "series" => "剧集",
        "anime" => "动漫",
        _ => return None,
    })
}

pub fn check_name(name: &Option<String>) -> Result<(), String> {
    match name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) => common::require(n, "类型名称", 20).map(|_| ()),
        None => Ok(()),
    }
}

/// Import helper: kind name (or legacy code) → id, creating the kind if missing. Blank → None.
pub fn resolve(conn: &Connection, module: &str, name: Option<&str>) -> Result<Option<i64>, String> {
    let Some(raw) = name.map(str::trim).filter(|s| !s.is_empty()) else { return Ok(None) };
    let name = legacy(raw).unwrap_or(raw);
    let found: Option<i64> = conn
        .query_row("SELECT id FROM kinds WHERE module = ?1 AND name = ?2", params![module, name], |r| r.get(0))
        .optional()
        .map_err(err)?;
    match found {
        Some(id) => Ok(Some(id)),
        None => create(conn, module, name).map(|k| Some(k.id)),
    }
}

// ---------- commands ----------

#[tauri::command]
pub fn kinds_list(state: State<AppState>, module: String) -> Result<Vec<Kind>, String> {
    state.with_db(|c| list(c, &module))
}

#[tauri::command]
pub fn kinds_create(state: State<AppState>, module: String, name: String) -> Result<Kind, String> {
    state.with_db(|c| create(c, &module, &name))
}

#[tauri::command]
pub fn kinds_rename(state: State<AppState>, id: i64, name: String) -> Result<Kind, String> {
    state.with_db(|c| rename(c, id, &name))
}

#[tauri::command]
pub fn kinds_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_db(|c| delete(c, id))
}

#[tauri::command]
pub fn kinds_reorder(state: State<AppState>, module: String, ids: Vec<i64>) -> Result<Vec<Kind>, String> {
    state.with_db(|c| reorder(c, &module, &ids))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::db::tests::memory_db;

    pub fn kind_id(c: &Connection, module: &str, name: &str) -> i64 {
        list(c, module).unwrap().into_iter().find(|k| k.name == name).unwrap().id
    }

    #[test]
    fn seeded_lists() {
        let c = memory_db();
        let names = |m| list(&c, m).unwrap().into_iter().map(|k| k.name).collect::<Vec<_>>();
        assert_eq!(names("books"), ["虚构", "非虚构", "漫画", "其他"]);
        assert_eq!(names("films"), ["电影", "剧集", "动漫"]);
        assert_eq!(names("drawing"), ["正课", "DLC 跟画", "LIVE 辅导"]);
        assert_eq!(names("packing"), ["重要物品", "随身", "护理", "兴趣"]);
        assert!(list(&c, "nope").is_err());
    }

    #[test]
    fn create_rename_unique() {
        let c = memory_db();
        let k = create(&c, "books", " 杂志 ").unwrap();
        assert_eq!((k.name.as_str(), k.sort_order, k.count), ("杂志", 5, 0));
        assert!(create(&c, "books", "杂志").unwrap_err().contains("已经有了"));
        assert!(create(&c, "films", "杂志").is_ok()); // unique per module only
        assert!(create(&c, "books", " ").is_err());
        assert_eq!(rename(&c, k.id, "期刊").unwrap().name, "期刊");
        assert!(rename(&c, k.id, "虚构").is_err());
        assert_eq!(rename(&c, k.id, "期刊").unwrap().name, "期刊"); // same name ok
    }

    #[test]
    fn delete_uncategorizes_items() {
        let c = memory_db();
        let comic = kind_id(&c, "books", "漫画");
        c.execute("INSERT INTO books (title, kind_id) VALUES ('海贼王', ?1)", [comic]).unwrap();
        assert_eq!(list(&c, "books").unwrap().iter().find(|k| k.id == comic).unwrap().count, 1);
        delete(&c, comic).unwrap();
        let kid: Option<i64> = c.query_row("SELECT kind_id FROM books", [], |r| r.get(0)).unwrap();
        assert_eq!(kid, None);
        assert!(delete(&c, comic).is_err());
    }

    #[test]
    fn reorder_and_check() {
        let c = memory_db();
        let ids: Vec<i64> = list(&c, "films").unwrap().iter().map(|k| k.id).rev().collect();
        let out = reorder(&c, "films", &ids).unwrap();
        assert_eq!(out.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(), ["动漫", "剧集", "电影"]);
        assert!(reorder(&c, "films", &ids[..2]).is_err());
        assert!(check(&c, "films", Some(ids[0])).is_ok());
        assert!(check(&c, "books", Some(ids[0])).is_err());
        assert!(check(&c, "books", None).is_ok());
    }

    #[test]
    fn resolve_names_and_legacy_codes() {
        let c = memory_db();
        assert_eq!(resolve(&c, "books", Some("nonfiction")).unwrap(), Some(kind_id(&c, "books", "非虚构")));
        assert_eq!(resolve(&c, "books", Some("虚构")).unwrap(), Some(kind_id(&c, "books", "虚构")));
        let new = resolve(&c, "books", Some("绘本")).unwrap().unwrap();
        assert_eq!(new, kind_id(&c, "books", "绘本"));
        assert_eq!(resolve(&c, "books", Some("  ")).unwrap(), None);
        assert_eq!(resolve(&c, "books", None).unwrap(), None);
    }
}
