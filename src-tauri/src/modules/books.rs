//! 摸鱼读读 (books). Core fns take `&Connection` (unit-testable); `#[tauri::command]`
//! wrappers at the bottom are the application-service API used by UI (and later AI tools).

use super::common::{self, err, some, Status};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tauri::State;

pub const KEY: &str = "books";
pub const LABEL: &str = "摸鱼读读";
const NOT_FOUND: &str = "找不到这本书，可能已经被删除了";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    pub id: i64,
    pub title: String,
    pub author: Option<String>,
    pub country: Option<String>,
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
pub struct BookPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "some")]
    pub author: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub country: Option<Option<String>>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default, deserialize_with = "some")]
    pub rating: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub finished_month: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
}

/// Editable fields; `validated()` normalizes + checks.
#[derive(Debug, Clone, Default, PartialEq)]
struct Fields {
    title: String,
    author: Option<String>,
    country: Option<String>,
    status: Status,
    rating: Option<i64>,
    finished_month: Option<String>,
    note: Option<String>,
}

impl Fields {
    fn from_book(b: &Book) -> Self {
        Self {
            title: b.title.clone(),
            author: b.author.clone(),
            country: b.country.clone(),
            status: b.status,
            rating: b.rating,
            finished_month: b.finished_month.clone(),
            note: b.note.clone(),
        }
    }

    fn apply(mut self, p: BookPatch) -> Self {
        if let Some(v) = p.title {
            self.title = v;
        }
        if let Some(v) = p.author {
            self.author = v;
        }
        if let Some(v) = p.country {
            self.country = v;
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
            title: common::require(&self.title, "书名", 200)?,
            author: common::clean(self.author),
            country: common::clean(self.country),
            status: self.status,
            rating: self.rating,
            finished_month: common::clean(self.finished_month),
            note: common::clean(self.note),
        };
        common::check_len(&f.author, "作者", 200)?;
        common::check_len(&f.country, "国家", 50)?;
        common::check_len(&f.note, "备注", 5000)?;
        common::check_rating(f.rating)?;
        common::check_month(&f.finished_month)?;
        Ok(f)
    }
}

/// Rule: becoming `done` with an empty month fills the current month,
/// unless the caller set the month explicitly (their value, even null, wins).
fn auto_month(conn: &Connection, f: &mut Fields, was_done: bool, month_given: bool) -> Result<(), String> {
    if f.status == Status::Done && !was_done && f.finished_month.is_none() && !month_given {
        f.finished_month = Some(common::current_month(conn)?);
    }
    Ok(())
}

const COLS: &str = "id, title, author, country, status, rating, finished_month, note, created_at, updated_at";

fn from_row(r: &Row) -> rusqlite::Result<Book> {
    Ok(Book {
        id: r.get(0)?,
        title: r.get(1)?,
        author: r.get(2)?,
        country: r.get(3)?,
        status: r.get(4)?,
        rating: r.get(5)?,
        finished_month: r.get(6)?,
        note: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<Book>, String> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM books ORDER BY id")).map_err(err)?;
    let rows = st.query_map([], from_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn get(conn: &Connection, id: i64) -> Result<Book, String> {
    conn.query_row(&format!("SELECT {COLS} FROM books WHERE id = ?1"), [id], from_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| NOT_FOUND.to_string())
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM books", [], |r| r.get(0)).map_err(err)
}

pub fn create(conn: &Connection, patch: BookPatch) -> Result<Book, String> {
    let month_given = patch.finished_month.is_some();
    let mut f = Fields::default().apply(patch).validated()?;
    auto_month(conn, &mut f, false, month_given)?;
    conn.execute(
        "INSERT INTO books (title, author, country, status, rating, finished_month, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![f.title, f.author, f.country, f.status, f.rating, f.finished_month, f.note],
    )
    .map_err(err)?;
    get(conn, conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, patch: BookPatch) -> Result<Book, String> {
    let old = get(conn, id)?;
    let month_given = patch.finished_month.is_some();
    let mut f = Fields::from_book(&old).apply(patch).validated()?;
    auto_month(conn, &mut f, old.status == Status::Done, month_given)?;
    conn.execute(
        "UPDATE books SET title = ?1, author = ?2, country = ?3, status = ?4, rating = ?5,
           finished_month = ?6, note = ?7, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?8",
        params![f.title, f.author, f.country, f.status, f.rating, f.finished_month, f.note, id],
    )
    .map_err(err)?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM books WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(NOT_FOUND.into()),
        _ => Ok(()),
    }
}

// ---------- import / export (Pomelo export format, no ids) ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookRecord {
    pub title: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
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

impl BookRecord {
    fn fields(&self) -> Result<Fields, String> {
        Fields {
            title: self.title.clone(),
            author: self.author.clone(),
            country: self.country.clone(),
            status: self.status,
            rating: self.rating,
            finished_month: self.finished_month.clone(),
            note: self.note.clone(),
        }
        .validated()
    }
}

fn record_err(i: usize, r: &BookRecord, e: String) -> String {
    format!("{LABEL}第 {} 条（{}）：{e}", i + 1, r.title.trim())
}

pub fn validate_records(records: &[BookRecord]) -> Result<(), String> {
    for (i, r) in records.iter().enumerate() {
        r.fields().map_err(|e| record_err(i, r, e))?;
    }
    Ok(())
}

/// Replaces all books with `records` (as-is, no auto-fill). Caller owns the transaction.
pub fn replace_all(conn: &Connection, records: &[BookRecord]) -> Result<usize, String> {
    conn.execute("DELETE FROM books", []).map_err(err)?;
    let mut st = conn
        .prepare(
            "INSERT INTO books (title, author, country, status, rating, finished_month, note, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
               coalesce(?8, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
               coalesce(?9, ?8, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
        )
        .map_err(err)?;
    for (i, r) in records.iter().enumerate() {
        let f = r.fields().map_err(|e| record_err(i, r, e))?;
        st.execute(params![
            f.title, f.author, f.country, f.status, f.rating, f.finished_month, f.note, r.created_at, r.updated_at
        ])
        .map_err(err)?;
    }
    Ok(records.len())
}

// ---------- commands ----------

#[tauri::command]
pub fn books_list(state: State<AppState>) -> Result<Vec<Book>, String> {
    state.with_db(|c| list(c))
}

#[tauri::command]
pub fn books_create(state: State<AppState>, patch: BookPatch) -> Result<Book, String> {
    state.with_db(|c| create(c, patch))
}

#[tauri::command]
pub fn books_update(state: State<AppState>, id: i64, patch: BookPatch) -> Result<Book, String> {
    state.with_db(|c| update(c, id, patch))
}

#[tauri::command]
pub fn books_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_db(|c| delete(c, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    fn patch(v: serde_json::Value) -> BookPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn patch_distinguishes_absent_and_null() {
        let p = patch(json!({"author": null, "title": "x"}));
        assert_eq!(p.author, Some(None));
        assert_eq!(p.country, None);
        assert!(serde_json::from_value::<BookPatch>(json!({"bogus": 1})).is_err());
        assert!(serde_json::from_value::<BookPatch>(json!({"status": "read"})).is_err());
    }

    #[test]
    fn create_defaults_and_normalizes() {
        let c = memory_db();
        let b = create(&c, patch(json!({"title": "  小王子 ", "author": " ", "country": "法国"}))).unwrap();
        assert_eq!(b.title, "小王子");
        assert_eq!(b.author, None);
        assert_eq!(b.country.as_deref(), Some("法国"));
        assert_eq!(b.status, Status::Unmarked);
        assert_eq!(b.finished_month, None);
        assert!(!b.created_at.is_empty());
    }

    #[test]
    fn create_rejects_invalid() {
        let c = memory_db();
        for bad in [
            json!({}),
            json!({"title": "   "}),
            json!({"title": "a", "rating": 6}),
            json!({"title": "a", "finishedMonth": "2026-13"}),
        ] {
            assert!(create(&c, patch(bad.clone())).is_err(), "{bad}");
        }
        assert_eq!(count(&c).unwrap(), 0);
    }

    #[test]
    fn done_auto_fills_month_unless_given() {
        let c = memory_db();
        let now = common::current_month(&c).unwrap();
        let a = create(&c, patch(json!({"title": "a", "status": "done"}))).unwrap();
        assert_eq!(a.finished_month.as_deref(), Some(now.as_str()));
        let b = create(&c, patch(json!({"title": "b", "status": "done", "finishedMonth": null}))).unwrap();
        assert_eq!(b.finished_month, None);
        let d = create(&c, patch(json!({"title": "d", "status": "done", "finishedMonth": "2019-03"}))).unwrap();
        assert_eq!(d.finished_month.as_deref(), Some("2019-03"));
    }

    #[test]
    fn update_patches_and_transitions() {
        let c = memory_db();
        let now = common::current_month(&c).unwrap();
        let b = create(&c, patch(json!({"title": "百年孤独", "author": "马尔克斯", "rating": 4}))).unwrap();

        let u = update(&c, b.id, patch(json!({"status": "doing"}))).unwrap();
        assert_eq!((u.status, u.rating, u.author.as_deref()), (Status::Doing, Some(4), Some("马尔克斯")));
        assert_eq!(u.finished_month, None);

        let u = update(&c, b.id, patch(json!({"status": "done"}))).unwrap();
        assert_eq!(u.finished_month.as_deref(), Some(now.as_str()));

        // already done: clearing the month sticks; re-saving done does not refill
        let u = update(&c, b.id, patch(json!({"finishedMonth": null}))).unwrap();
        assert_eq!(u.finished_month, None);
        let u = update(&c, b.id, patch(json!({"status": "done", "rating": null, "author": null}))).unwrap();
        assert_eq!((u.finished_month, u.rating, u.author), (None, None, None));

        assert!(update(&c, 999, patch(json!({"title": "x"}))).is_err());
        assert!(update(&c, b.id, patch(json!({"title": ""}))).is_err());
    }

    #[test]
    fn delete_works_once() {
        let c = memory_db();
        let b = create(&c, patch(json!({"title": "a"}))).unwrap();
        delete(&c, b.id).unwrap();
        assert!(delete(&c, b.id).is_err());
        assert!(list(&c).unwrap().is_empty());
    }

    #[test]
    fn replace_all_keeps_order_and_timestamps() {
        let c = memory_db();
        create(&c, patch(json!({"title": "old"}))).unwrap();
        let recs: Vec<BookRecord> = serde_json::from_value(json!([
            {"title": "A", "status": "done", "rating": 5, "createdAt": "2020-01-01T00:00:00.000Z"},
            {"title": "B"}
        ]))
        .unwrap();
        assert_eq!(replace_all(&c, &recs).unwrap(), 2);
        let all = list(&c).unwrap();
        assert_eq!(all.iter().map(|b| b.title.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(all[0].finished_month, None); // imported as-is, no auto-fill
        assert_eq!(all[0].created_at, "2020-01-01T00:00:00.000Z");
        assert_eq!(all[0].updated_at, "2020-01-01T00:00:00.000Z");
        assert_eq!(all[1].status, Status::Unmarked);

        let bad: Vec<BookRecord> = serde_json::from_value(json!([{"title": "ok"}, {"title": " "}])).unwrap();
        let e = validate_records(&bad).unwrap_err();
        assert!(e.contains("第 2 条"), "{e}");
    }
}
