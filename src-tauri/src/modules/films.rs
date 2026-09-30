//! 好奇看看 (films). Mirrors books.rs (author→director); keep them in step.
//! 类型 = kinds.rs (module "films"). Commands at the bottom.

use super::common::{self, err, some, Status};
use super::kinds;
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tauri::State;

pub const KEY: &str = "films";
pub const LABEL: &str = "好奇看看";
const NOT_FOUND: &str = "找不到这部作品，可能已经被删除了";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Film {
    pub id: i64,
    pub title: String,
    pub director: Option<String>,
    pub country: Option<String>,
    /// kinds.id (user-editable 类型); None = 未分类.
    pub kind_id: Option<i64>,
    pub status: Status,
    pub rating: Option<i64>,
    pub finished_month: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Caller-settable fields. Absent = keep (update) / default (create); explicit null clears.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilmPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "some")]
    pub director: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub country: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub kind_id: Option<Option<i64>>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default, deserialize_with = "some")]
    pub rating: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub finished_month: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Fields {
    title: String,
    director: Option<String>,
    country: Option<String>,
    kind_id: Option<i64>,
    status: Status,
    rating: Option<i64>,
    finished_month: Option<String>,
    note: Option<String>,
}

impl Fields {
    fn from_film(f: &Film) -> Self {
        Self {
            title: f.title.clone(),
            director: f.director.clone(),
            country: f.country.clone(),
            kind_id: f.kind_id,
            status: f.status,
            rating: f.rating,
            finished_month: f.finished_month.clone(),
            note: f.note.clone(),
        }
    }

    fn apply(mut self, p: FilmPatch) -> Self {
        if let Some(v) = p.title {
            self.title = v;
        }
        if let Some(v) = p.director {
            self.director = v;
        }
        if let Some(v) = p.country {
            self.country = v;
        }
        if let Some(v) = p.kind_id {
            self.kind_id = v;
        }
        if let Some(v) = p.status {
            self.status = v;
        }
        if let Some(v) = p.rating {
            self.rating = v;
        }
        if let Some(v) = p.finished_month {
            self.finished_month = v;
        }
        if let Some(v) = p.note {
            self.note = v;
        }
        self
    }

    fn validated(self) -> Result<Self, String> {
        let f = Self {
            title: common::require(&self.title, "名称", 200)?,
            director: common::clean(self.director),
            country: common::clean(self.country),
            kind_id: self.kind_id,
            status: self.status,
            rating: self.rating,
            finished_month: common::clean(self.finished_month),
            note: common::clean(self.note),
        };
        common::check_len(&f.director, "导演", 200)?;
        common::check_len(&f.country, "国家", 50)?;
        common::check_len(&f.note, "备注", 5000)?;
        common::check_rating(f.rating)?;
        common::check_month(&f.finished_month)?;
        Ok(f)
    }
}

const COLS: &str = "id, title, director, country, kind_id, status, rating, finished_month, note, created_at, updated_at";

fn from_row(r: &Row) -> rusqlite::Result<Film> {
    Ok(Film {
        id: r.get(0)?,
        title: r.get(1)?,
        director: r.get(2)?,
        country: r.get(3)?,
        kind_id: r.get(4)?,
        status: r.get(5)?,
        rating: r.get(6)?,
        finished_month: r.get(7)?,
        note: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<Film>, String> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM films ORDER BY id")).map_err(err)?;
    let rows = st.query_map([], from_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn get(conn: &Connection, id: i64) -> Result<Film, String> {
    conn.query_row(&format!("SELECT {COLS} FROM films WHERE id = ?1"), [id], from_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| NOT_FOUND.to_string())
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM films", [], |r| r.get(0)).map_err(err)
}

pub fn create(conn: &Connection, patch: FilmPatch) -> Result<Film, String> {
    let month_given = patch.finished_month.is_some();
    let mut f = Fields::default().apply(patch).validated()?;
    kinds::check(conn, KEY, f.kind_id)?;
    common::auto_month(conn, f.status, false, &mut f.finished_month, month_given)?;
    conn.execute(
        "INSERT INTO films (title, director, country, kind_id, status, rating, finished_month, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![f.title, f.director, f.country, f.kind_id, f.status, f.rating, f.finished_month, f.note],
    )
    .map_err(err)?;
    get(conn, conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, patch: FilmPatch) -> Result<Film, String> {
    let old = get(conn, id)?;
    let month_given = patch.finished_month.is_some();
    let mut f = Fields::from_film(&old).apply(patch).validated()?;
    kinds::check(conn, KEY, f.kind_id)?;
    common::auto_month(conn, f.status, old.status == Status::Done, &mut f.finished_month, month_given)?;
    conn.execute(
        "UPDATE films SET title = ?1, director = ?2, country = ?3, kind_id = ?4, status = ?5, rating = ?6,
           finished_month = ?7, note = ?8, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?9",
        params![f.title, f.director, f.country, f.kind_id, f.status, f.rating, f.finished_month, f.note, id],
    )
    .map_err(err)?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM films WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(NOT_FOUND.into()),
        _ => Ok(()),
    }
}

// ---------- import / export ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilmRecord {
    pub title: String,
    #[serde(default)]
    pub director: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    /// 类型 name (created if missing on import).
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub rating: Option<i64>,
    #[serde(default)]
    pub finished_month: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl FilmRecord {
    fn fields(&self) -> Result<Fields, String> {
        kinds::check_name(&self.kind)?;
        Fields {
            title: self.title.clone(),
            director: self.director.clone(),
            country: self.country.clone(),
            kind_id: None, // resolved from name in replace_all
            status: self.status,
            rating: self.rating,
            finished_month: self.finished_month.clone(),
            note: self.note.clone(),
        }
        .validated()
    }
}

fn record_err(i: usize, r: &FilmRecord, e: String) -> String {
    format!("{LABEL}第 {} 条（{}）：{e}", i + 1, r.title.trim())
}

pub fn validate_records(records: &[FilmRecord]) -> Result<(), String> {
    for (i, r) in records.iter().enumerate() {
        r.fields().map_err(|e| record_err(i, r, e))?;
    }
    Ok(())
}

/// All records in id order (kinds by name), for the export file.
pub fn export(conn: &Connection) -> Result<Vec<FilmRecord>, String> {
    let names = kinds::name_map(conn, KEY)?;
    Ok(list(conn)?
        .into_iter()
        .map(|x| FilmRecord {
            title: x.title,
            director: x.director,
            country: x.country,
            kind: x.kind_id.and_then(|id| names.get(&id).cloned()),
            status: x.status,
            rating: x.rating,
            finished_month: x.finished_month,
            note: x.note,
            created_at: Some(x.created_at),
            updated_at: Some(x.updated_at),
        })
        .collect())
}

/// Replaces all films with `records` (as-is, no auto-fill). Caller owns the transaction.
pub fn replace_all(conn: &Connection, records: &[FilmRecord]) -> Result<usize, String> {
    conn.execute("DELETE FROM films", []).map_err(err)?;
    let mut st = conn
        .prepare(
            "INSERT INTO films (title, director, country, kind_id, status, rating, finished_month, note, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
               coalesce(?9, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
               coalesce(?10, ?9, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
        )
        .map_err(err)?;
    for (i, r) in records.iter().enumerate() {
        let f = r.fields().map_err(|e| record_err(i, r, e))?;
        let kind_id = kinds::resolve(conn, KEY, r.kind.as_deref())?;
        st.execute(params![
            f.title, f.director, f.country, kind_id, f.status, f.rating, f.finished_month, f.note, r.created_at, r.updated_at
        ])
        .map_err(err)?;
    }
    Ok(records.len())
}

// ---------- commands ----------

#[tauri::command]
pub fn films_list(state: State<AppState>) -> Result<Vec<Film>, String> {
    state.with_db(|c| list(c))
}

#[tauri::command]
pub fn films_create(state: State<AppState>, patch: FilmPatch) -> Result<Film, String> {
    state.with_db(|c| create(c, patch))
}

#[tauri::command]
pub fn films_update(state: State<AppState>, id: i64, patch: FilmPatch) -> Result<Film, String> {
    state.with_db(|c| update(c, id, patch))
}

#[tauri::command]
pub fn films_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_db(|c| delete(c, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::modules::kinds::tests::kind_id;
    use serde_json::json;

    fn patch(v: serde_json::Value) -> FilmPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn create_defaults_and_kinds() {
        let c = memory_db();
        let f = create(&c, patch(json!({"title": " 千与千寻 ", "director": "宫崎骏"}))).unwrap();
        assert_eq!((f.title.as_str(), f.kind_id, f.status), ("千与千寻", None, Status::Unmarked));
        let series = kind_id(&c, "films", "剧集");
        assert_eq!(create(&c, patch(json!({"title": "请回答1988", "kindId": series}))).unwrap().kind_id, Some(series));
        assert!(create(&c, patch(json!({"title": "x", "kindId": kind_id(&c, "books", "虚构")}))).is_err());
        assert!(serde_json::from_value::<FilmPatch>(json!({"author": "x"})).is_err());
        assert!(create(&c, patch(json!({"title": " "}))).is_err());
    }

    #[test]
    fn update_and_auto_month() {
        let c = memory_db();
        let now = common::current_month(&c).unwrap();
        let anime = kind_id(&c, "films", "动漫");
        let f = create(&c, patch(json!({"title": "星际穿越", "director": "诺兰"}))).unwrap();
        let u = update(&c, f.id, patch(json!({"status": "done", "rating": 5}))).unwrap();
        assert_eq!((u.finished_month.as_deref(), u.rating), (Some(now.as_str()), Some(5)));
        let u = update(&c, f.id, patch(json!({"director": null, "kindId": anime}))).unwrap();
        assert_eq!((u.director, u.kind_id, u.status), (None, Some(anime), Status::Done));
        delete(&c, f.id).unwrap();
        assert!(get(&c, f.id).is_err());
    }

    #[test]
    fn replace_all_imports_as_is() {
        let c = memory_db();
        let recs: Vec<FilmRecord> = serde_json::from_value(json!([
            {"title": "A", "status": "done", "rating": 5, "kind": "电影"},
            {"title": "B", "kind": "series"}
        ]))
        .unwrap();
        assert_eq!(replace_all(&c, &recs).unwrap(), 2);
        let all = list(&c).unwrap();
        assert_eq!((all[0].finished_month.as_ref(), all[0].kind_id), (None, Some(kind_id(&c, "films", "电影"))));
        assert_eq!((all[1].kind_id, all[1].status), (Some(kind_id(&c, "films", "剧集")), Status::Unmarked));
    }
}
