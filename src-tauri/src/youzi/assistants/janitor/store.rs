//! janitor.db: the last scan, computed from the disk (like search.db: never backed up, exported or undone;
//! a missing, broken or older-version file is deleted and made again, and the next scan fills it).
//! - meta: `summary` (JSON: when, disk totals, trash, Docker, what couldn't be read).
//! - nodes: the folder tree, only what's big enough to draw (`scan::KEEP`) — the rest of a folder is its size
//!   minus its kept children. `path` is virtual; the root `/` is the whole disk, `kind` `system` is 系统和其他.
//! - items: the 一键清理 list (`listed` 0; `kind` = a category) and the listed groups (`listed` 1; `kind` = a
//!   group). `paths` (JSON) are what cleaning touches; `size` is what it frees (hard links counted once);
//!   `project_newest` is the newest file of the item's project outside its rebuildable dirs; `extra` (JSON)
//!   holds the rest (`how`, `inUse`, `protected`, `note`, `tag`, `dup`, `opened`).
//! - meta `envs` (阶段 7): the 开发环境 (env.rs `Env`, JSON), replaced with the rest.

use crate::db;
use crate::modules::common::err;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub const VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;
CREATE TABLE nodes (
  id     INTEGER PRIMARY KEY,
  parent INTEGER,
  name   TEXT NOT NULL,
  path   TEXT NOT NULL UNIQUE,
  kind   TEXT NOT NULL,
  size   INTEGER NOT NULL,
  files  INTEGER NOT NULL,
  newest INTEGER NOT NULL,
  denied INTEGER NOT NULL
) STRICT;
CREATE INDEX nodes_parent ON nodes(parent);
CREATE TABLE items (
  id             INTEGER PRIMARY KEY,
  listed         INTEGER NOT NULL,
  kind           TEXT NOT NULL,
  title          TEXT NOT NULL,
  paths          TEXT NOT NULL,
  size           INTEGER NOT NULL,
  newest         INTEGER NOT NULL,
  project        TEXT,
  project_newest INTEGER,
  extra          TEXT NOT NULL
) STRICT;
";

pub fn open(path: &Path) -> Result<Connection, String> {
    match open_as_is(path) {
        Ok(c) => Ok(c),
        Err(_) => {
            remove(path);
            open_as_is(path).map_err(|e| format!("DevJanitor 的扫描结果打不开：{e}"))
        }
    }
}

pub fn remove(path: &Path) {
    for p in [path.to_path_buf(), path.with_extension("db-wal"), path.with_extension("db-shm")] {
        let _ = std::fs::remove_file(p);
    }
}

fn open_as_is(path: &Path) -> Result<Connection, String> {
    let c = db::open(path)?;
    prepare(&c)?;
    Ok(c)
}

pub fn prepare(c: &Connection) -> Result<(), String> {
    match db::user_version(c)? {
        VERSION => c.query_row("SELECT count(*) FROM items", [], |r| r.get::<_, i64>(0)).map(|_| ()).map_err(err),
        0 => {
            let tables: i64 = c.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get(0)).map_err(err)?;
            if tables > 0 {
                return Err("不是 DevJanitor 的扫描结果".into());
            }
            c.execute_batch(&format!("BEGIN; {SCHEMA} PRAGMA user_version = {VERSION}; COMMIT;")).map_err(err)
        }
        v => Err(format!("版本 {v} 不是现在的 {VERSION}")),
    }
}

pub fn meta(c: &Connection, key: &str) -> Result<Option<String>, String> {
    c.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional().map_err(err)
}

pub fn set_meta(c: &Connection, key: &str, value: &str) -> Result<(), String> {
    c.execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [key, value]).map(|_| ()).map_err(err)
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeRow {
    pub id: i64,
    pub parent: Option<i64>,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub files: u64,
    pub newest: i64,
    pub denied: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemRow {
    pub id: i64,
    pub listed: bool,
    pub kind: String,
    pub title: String,
    pub paths: Vec<String>,
    pub size: u64,
    pub newest: i64,
    pub project: Option<String>,
    pub project_newest: Option<i64>,
    pub extra: serde_json::Value,
}

/// Replaces the whole result (one transaction: the page sees the old one until it's done).
pub fn replace(c: &mut Connection, summary: &serde_json::Value, nodes: &[NodeRow], items: &[ItemRow], envs: &serde_json::Value) -> Result<(), String> {
    let tx = c.transaction().map_err(err)?;
    tx.execute_batch("DELETE FROM nodes; DELETE FROM items;").map_err(err)?;
    {
        let mut n = tx.prepare("INSERT INTO nodes (id, parent, name, path, kind, size, files, newest, denied) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)").map_err(err)?;
        for r in nodes {
            n.execute(params![r.id, r.parent, r.name, r.path, r.kind, r.size as i64, r.files as i64, r.newest, r.denied as i64]).map_err(err)?;
        }
        let mut i = tx.prepare("INSERT INTO items (id, listed, kind, title, paths, size, newest, project, project_newest, extra) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)").map_err(err)?;
        for r in items {
            let paths = serde_json::to_string(&r.paths).map_err(err)?;
            i.execute(params![r.id, r.listed, r.kind, r.title, paths, r.size as i64, r.newest, r.project, r.project_newest, r.extra.to_string()]).map_err(err)?;
        }
    }
    tx.execute("INSERT INTO meta (key, value) VALUES ('summary', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [summary.to_string()]).map_err(err)?;
    tx.execute("INSERT INTO meta (key, value) VALUES ('envs', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [envs.to_string()]).map_err(err)?;
    tx.commit().map_err(err)
}

fn node_of(r: &rusqlite::Row) -> rusqlite::Result<NodeRow> {
    Ok(NodeRow {
        id: r.get(0)?,
        parent: r.get(1)?,
        name: r.get(2)?,
        path: r.get(3)?,
        kind: r.get(4)?,
        size: r.get::<_, i64>(5)? as u64,
        files: r.get::<_, i64>(6)? as u64,
        newest: r.get(7)?,
        denied: r.get::<_, i64>(8)? as u64,
    })
}

const NODE_COLS: &str = "id, parent, name, path, kind, size, files, newest, denied";

pub fn node_at(c: &Connection, path: &str) -> Result<Option<NodeRow>, String> {
    c.query_row(&format!("SELECT {NODE_COLS} FROM nodes WHERE path = ?1"), [path], node_of).optional().map_err(err)
}

pub fn children(c: &Connection, parent: i64) -> Result<Vec<NodeRow>, String> {
    let mut s = c.prepare_cached(&format!("SELECT {NODE_COLS} FROM nodes WHERE parent = ?1 ORDER BY size DESC, name")).map_err(err)?;
    let rows = s.query_map([parent], node_of).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    Ok(rows)
}

pub fn parent_of(c: &Connection, id: i64) -> Result<Option<NodeRow>, String> {
    c.query_row(&format!("SELECT {NODE_COLS} FROM nodes WHERE id = (SELECT parent FROM nodes WHERE id = ?1)"), [id], node_of).optional().map_err(err)
}

fn item_of(r: &rusqlite::Row) -> rusqlite::Result<ItemRow> {
    let paths: String = r.get(4)?;
    let extra: String = r.get(9)?;
    Ok(ItemRow {
        id: r.get(0)?,
        listed: r.get(1)?,
        kind: r.get(2)?,
        title: r.get(3)?,
        paths: serde_json::from_str(&paths).unwrap_or_default(),
        size: r.get::<_, i64>(5)? as u64,
        newest: r.get(6)?,
        project: r.get(7)?,
        project_newest: r.get(8)?,
        extra: serde_json::from_str(&extra).unwrap_or(serde_json::Value::Null),
    })
}

const ITEM_COLS: &str = "id, listed, kind, title, paths, size, newest, project, project_newest, extra";

pub fn items(c: &Connection) -> Result<Vec<ItemRow>, String> {
    let mut s = c.prepare(&format!("SELECT {ITEM_COLS} FROM items ORDER BY size DESC, id")).map_err(err)?;
    let rows = s.query_map([], item_of).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    Ok(rows)
}

pub fn items_by_id(c: &Connection, ids: &[i64]) -> Result<Vec<ItemRow>, String> {
    let mut s = c.prepare_cached(&format!("SELECT {ITEM_COLS} FROM items WHERE id = ?1")).map_err(err)?;
    let mut out = Vec::new();
    for id in ids {
        if let Some(r) = s.query_row([id], item_of).optional().map_err(err)? {
            out.push(r);
        }
    }
    Ok(out)
}

/// After cleaning: the items are gone, and what they freed comes off the folders holding them.
pub fn forget(c: &mut Connection, gone: &[(i64, Vec<String>, u64)]) -> Result<(), String> {
    let tx = c.transaction().map_err(err)?;
    for (id, paths, freed) in gone {
        tx.execute("DELETE FROM items WHERE id = ?1", [id]).map_err(err)?;
        // the deepest kept folder holding each path loses its share; a kept node of the path itself goes
        let share = if paths.is_empty() { 0 } else { freed / paths.len() as u64 };
        for p in paths {
            tx.execute("DELETE FROM nodes WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2", params![p, format!("{p}/")]).map_err(err)?;
            let mut at = p.as_str();
            while let Some((up, _)) = at.rsplit_once('/') {
                let up = if up.is_empty() { "/" } else { up };
                tx.execute("UPDATE nodes SET size = max(size - ?1, 0) WHERE path = ?2", params![share as i64, up]).map_err(err)?;
                if up == "/" {
                    break;
                }
                at = up;
            }
        }
    }
    // folders that fell under what the chart draws go (the whole disk and its first ring stay)
    tx.execute("DELETE FROM nodes WHERE size < ?1 AND parent IS NOT NULL AND parent != 1", [super::scan::KEEP as i64]).map_err(err)?;
    tx.commit().map_err(err)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn memory_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        db::configure(&c).unwrap();
        prepare(&c).unwrap();
        c
    }

    fn node(id: i64, parent: Option<i64>, path: &str, size: u64) -> NodeRow {
        NodeRow { id, parent, name: path.rsplit('/').next().unwrap_or("/").to_string(), path: path.into(), kind: "dir".into(), size, files: 1, newest: 0, denied: 0 }
    }

    #[test]
    fn replace_read_and_forget() {
        let mut c = memory_db();
        let mb = super::super::scan::KEEP;
        let nodes = [node(1, None, "/", 100 * mb), node(2, Some(1), "/Users", 90 * mb), node(3, Some(2), "/Users/me", 90 * mb), node(4, Some(3), "/Users/me/a", 40 * mb), node(5, Some(3), "/Users/me/b", 45 * mb)];
        let item = ItemRow { id: 1, listed: false, kind: "pkg_cache".into(), title: "npm".into(), paths: vec!["/Users/me/a".into()], size: 40 * mb, newest: 5, project: None, project_newest: None, extra: serde_json::json!({ "how": "remove" }) };
        replace(&mut c, &serde_json::json!({ "at": 1 }), &nodes, std::slice::from_ref(&item), &serde_json::json!([])).unwrap();
        assert_eq!(items(&c).unwrap(), vec![item.clone()]);
        assert_eq!(children(&c, 2).unwrap().len(), 1);
        assert_eq!(parent_of(&c, 3).unwrap().unwrap().path, "/Users");
        forget(&mut c, &[(1, item.paths.clone(), 40 * mb)]).unwrap();
        assert!(items(&c).unwrap().is_empty() && node_at(&c, "/Users/me/a").unwrap().is_none());
        assert_eq!(node_at(&c, "/Users/me").unwrap().unwrap().size, 50 * mb);
        assert_eq!(node_at(&c, "/").unwrap().unwrap().size, 60 * mb);
        // a folder that fell under what the chart draws goes; the first ring stays
        forget(&mut c, &[(0, vec!["/Users/me/b/x".into()], 45 * mb - 1)]).unwrap();
        assert!(node_at(&c, "/Users/me/b").unwrap().is_none() && node_at(&c, "/Users").unwrap().is_some());
        assert_eq!(meta(&c, "summary").unwrap().as_deref(), Some("{\"at\":1}"));
        // an old version is refused (open() then makes a new file)
        c.pragma_update(None, "user_version", 99).unwrap();
        assert!(prepare(&c).is_err());
    }
}
