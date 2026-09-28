//! Key/value app settings (JSON values) stored in the `settings` table.

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

pub fn get(conn: &Connection, key: &str) -> Result<Option<Value>, String> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    raw.map(|s| serde_json::from_str(&s).map_err(|e| e.to_string())).transpose()
}

pub fn set(conn: &Connection, key: &str, value: &Value) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
           updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        (key, value.to_string()),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    #[test]
    fn roundtrip() {
        let c = memory_db();
        assert_eq!(get(&c, "k").unwrap(), None);
        set(&c, "k", &json!("/books")).unwrap();
        set(&c, "k", &json!({"a": 1})).unwrap();
        assert_eq!(get(&c, "k").unwrap(), Some(json!({"a": 1})));
    }
}
