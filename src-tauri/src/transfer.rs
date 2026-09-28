//! Import of the Pomelo export format:
//! `{"format":"pomelo-export","formatVersion":1,"modules":{"books":[BookRecord...]}}`
//! Every record is validated before anything is written; all modules replace in one transaction.
//! Overwriting existing data requires `overwrite = true` and takes a snapshot backup first.
//! Adding a module: new `Payload` variant + arms below.

use crate::modules::{books, films};
use crate::modules::common::err;
use crate::{backup, AppState};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;
use tauri::State;

pub const FORMAT: &str = "pomelo-export";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportFile {
    format: String,
    format_version: u32,
    #[serde(default)]
    modules: Map<String, Value>,
}

enum Payload {
    Books(Vec<books::BookRecord>),
    Films(Vec<films::FilmRecord>),
}

impl Payload {
    fn key(&self) -> &'static str {
        match self {
            Payload::Books(_) => books::KEY,
            Payload::Films(_) => films::KEY,
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Payload::Books(_) => books::LABEL,
            Payload::Films(_) => films::LABEL,
        }
    }
    fn len(&self) -> usize {
        match self {
            Payload::Books(r) => r.len(),
            Payload::Films(r) => r.len(),
        }
    }
    fn existing(&self, conn: &Connection) -> Result<i64, String> {
        match self {
            Payload::Books(_) => books::count(conn),
            Payload::Films(_) => films::count(conn),
        }
    }
    fn replace(&self, conn: &Connection) -> Result<usize, String> {
        match self {
            Payload::Books(r) => books::replace_all(conn, r),
            Payload::Films(r) => films::replace_all(conn, r),
        }
    }
}

struct Parsed {
    payloads: Vec<Payload>,
    skipped: Vec<String>,
}

fn parse(text: &str) -> Result<Parsed, String> {
    let file: ExportFile =
        serde_json::from_str(text).map_err(|e| format!("这不是有效的 Pomelo 数据文件：{e}"))?;
    if file.format != FORMAT {
        return Err("这不是 Pomelo 的数据文件".into());
    }
    if file.format_version > FORMAT_VERSION {
        return Err(format!("数据文件的版本（{}）比这个 App 新，请先更新 App", file.format_version));
    }
    let mut payloads = Vec::new();
    let mut skipped = Vec::new();
    for (key, value) in file.modules {
        match key.as_str() {
            books::KEY => {
                let recs: Vec<books::BookRecord> = serde_json::from_value(value)
                    .map_err(|e| format!("{}的数据格式不对：{e}", books::LABEL))?;
                books::validate_records(&recs)?;
                payloads.push(Payload::Books(recs));
            }
            films::KEY => {
                let recs: Vec<films::FilmRecord> = serde_json::from_value(value)
                    .map_err(|e| format!("{}的数据格式不对：{e}", films::LABEL))?;
                films::validate_records(&recs)?;
                payloads.push(Payload::Films(recs));
            }
            _ => skipped.push(key),
        }
    }
    if payloads.is_empty() {
        return Err("文件里没有这个版本的 App 能导入的数据".into());
    }
    Ok(Parsed { payloads, skipped })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleSummary {
    pub key: String,
    pub label: String,
    pub incoming: usize,
    pub existing: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub modules: Vec<ModuleSummary>,
    /// Module keys present in the file but not supported yet.
    pub skipped: Vec<String>,
    /// Snapshot taken before overwriting (apply only).
    pub backup: Option<String>,
}

fn summarize(conn: &Connection, p: &Parsed) -> Result<ImportSummary, String> {
    let modules = p
        .payloads
        .iter()
        .map(|x| {
            Ok(ModuleSummary {
                key: x.key().into(),
                label: x.label().into(),
                incoming: x.len(),
                existing: x.existing(conn)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ImportSummary { modules, skipped: p.skipped.clone(), backup: None })
}

pub fn preview(conn: &Connection, text: &str) -> Result<ImportSummary, String> {
    summarize(conn, &parse(text)?)
}

pub fn apply(conn: &mut Connection, text: &str, overwrite: bool, backups_dir: &Path) -> Result<ImportSummary, String> {
    let parsed = parse(text)?;
    let mut summary = summarize(conn, &parsed)?;
    let occupied: Vec<&str> =
        summary.modules.iter().filter(|m| m.existing > 0).map(|m| m.label.as_str()).collect();
    if !occupied.is_empty() {
        if !overwrite {
            return Err(format!("{}已经有数据，需要确认覆盖后才能导入", occupied.join("、")));
        }
        let path = backup::snapshot(conn, backups_dir, "before-import")?;
        summary.backup = Some(path.display().to_string());
    }
    let tx = conn.transaction().map_err(err)?;
    for p in &parsed.payloads {
        p.replace(&tx)?;
    }
    tx.commit().map_err(err)?;
    Ok(summary)
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("读不了这个文件：{e}"))
}

#[tauri::command]
pub fn import_preview(state: State<AppState>, path: String) -> Result<ImportSummary, String> {
    let text = read(&path)?;
    state.with_db(|c| preview(c, &text))
}

#[tauri::command]
pub fn import_apply(state: State<AppState>, path: String, overwrite: bool) -> Result<ImportSummary, String> {
    let text = read(&path)?;
    let backups = state.paths.backups.clone();
    state.with_db(|c| apply(c, &text, overwrite, &backups))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::{memory_db, temp_dir};
    use crate::modules::common::Status;
    use serde_json::json;

    fn file(modules: Value) -> String {
        json!({"format": FORMAT, "formatVersion": 1, "modules": modules}).to_string()
    }

    #[test]
    fn preview_counts_and_skips_unknown() {
        let c = memory_db();
        let s = preview(&c, &file(json!({"books": [{"title": "A"}, {"title": "B"}], "films": [{"title": "F"}], "drawing": []})))
            .unwrap();
        assert_eq!(s.modules.len(), 2);
        assert_eq!((s.modules[0].key.as_str(), s.modules[0].incoming, s.modules[0].existing), ("books", 2, 0));
        assert_eq!((s.modules[1].key.as_str(), s.modules[1].incoming), ("films", 1));
        assert_eq!(s.skipped, ["drawing"]);
    }

    #[test]
    fn rejects_bad_files() {
        let c = memory_db();
        assert!(preview(&c, "not json").is_err());
        assert!(preview(&c, &json!({"format": "x", "formatVersion": 1}).to_string()).is_err());
        assert!(preview(&c, &json!({"format": FORMAT, "formatVersion": 99, "modules": {}}).to_string()).is_err());
        assert!(preview(&c, &file(json!({"drawing": []}))).is_err());
        assert!(preview(&c, &file(json!({"books": [{"title": "A", "extra": 1}]}))).is_err());
    }

    #[test]
    fn apply_empty_then_overwrite_with_backup() {
        let dir = temp_dir("import");
        let mut c = memory_db();
        let text = file(json!({"books": [{"title": "A", "status": "done"}, {"title": "B"}]}));
        let s = apply(&mut c, &text, false, &dir).unwrap();
        assert!(s.backup.is_none());
        assert_eq!(books::count(&c).unwrap(), 2);

        let e = apply(&mut c, &text, false, &dir).unwrap_err();
        assert!(e.contains("覆盖"), "{e}");

        let text2 = file(json!({"books": [{"title": "C"}]}));
        let s = apply(&mut c, &text2, true, &dir).unwrap();
        assert!(Path::new(s.backup.as_deref().unwrap()).exists());
        let all = books::list(&c).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title, "C");
    }

    #[test]
    fn invalid_record_writes_nothing() {
        let dir = temp_dir("import-invalid");
        let mut c = memory_db();
        let e = apply(&mut c, &file(json!({"books": [{"title": "A"}, {"title": ""}]})), true, &dir).unwrap_err();
        assert!(e.contains("第 2 条"), "{e}");
        assert_eq!(books::count(&c).unwrap(), 0);
    }

    /// Real prototype data (local only, gitignored). Run: cargo test -- --ignored
    #[test]
    #[ignore]
    fn real_prototype_import() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype/pomelo-import.json");
        let text = std::fs::read_to_string(&path).expect("run `pnpm import:xlsx` first");
        let mut c = memory_db();
        apply(&mut c, &text, false, &temp_dir("real")).unwrap();
        let all = books::list(&c).unwrap();
        let n = |s: Status| all.iter().filter(|b| b.status == s).count();
        assert_eq!(all.len(), 21);
        assert_eq!((n(Status::Done), n(Status::Doing), n(Status::Todo), n(Status::Unmarked)), (15, 1, 5, 0));
        assert_eq!(all[0].title, "献给阿尔吉侬的花束");
        assert_eq!(all.iter().filter(|b| b.rating == Some(5)).count(), 7);

        let fs = films::list(&c).unwrap();
        let m = |s: Status| fs.iter().filter(|f| f.status == s).count();
        assert_eq!(fs.len(), 50);
        assert_eq!((m(Status::Done), m(Status::Doing), m(Status::Todo), m(Status::Unmarked)), (12, 0, 0, 38));
        assert_eq!(fs[0].title, "肖申克的救赎");
        assert!(fs.iter().all(|f| f.kind == films::FilmKind::Movie));
        assert!(fs.iter().all(|f| !f.director.as_deref().unwrap_or("").contains('&')));
    }
}
