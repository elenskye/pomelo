//! Import of the Pomelo export format:
//! `{"format":"pomelo-export","formatVersion":1,"modules":{"books":[BookRecord...]}}`
//! Every record is validated before anything is written; all modules replace in one transaction.
//! Overwriting existing data requires `overwrite = true` and takes a snapshot backup first.
//! Adding a module: new `Payload` variant + arms below.

use crate::modules::{books, drawing, films, packing};
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
    Drawing(Vec<drawing::CourseRecord>),
    Packing(packing::PackingRecord),
}

impl Payload {
    fn key(&self) -> &'static str {
        match self {
            Payload::Books(_) => books::KEY,
            Payload::Films(_) => films::KEY,
            Payload::Drawing(_) => drawing::KEY,
            Payload::Packing(_) => packing::KEY,
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Payload::Books(_) => books::LABEL,
            Payload::Films(_) => films::LABEL,
            Payload::Drawing(_) => drawing::LABEL,
            Payload::Packing(_) => packing::LABEL,
        }
    }
    fn len(&self) -> usize {
        match self {
            Payload::Books(r) => r.len(),
            Payload::Films(r) => r.len(),
            Payload::Drawing(r) => r.len(),
            Payload::Packing(r) => r.len(),
        }
    }
    fn existing(&self, conn: &Connection) -> Result<i64, String> {
        match self {
            Payload::Books(_) => books::count(conn),
            Payload::Films(_) => films::count(conn),
            Payload::Drawing(_) => drawing::count(conn),
            Payload::Packing(_) => packing::count(conn),
        }
    }
    fn replace(&self, conn: &Connection) -> Result<usize, String> {
        match self {
            Payload::Books(r) => books::replace_all(conn, r),
            Payload::Films(r) => films::replace_all(conn, r),
            Payload::Drawing(r) => drawing::replace_all(conn, r),
            Payload::Packing(r) => packing::replace_all(conn, r),
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
            drawing::KEY => {
                let recs: Vec<drawing::CourseRecord> = serde_json::from_value(value)
                    .map_err(|e| format!("{}的数据格式不对：{e}", drawing::LABEL))?;
                drawing::validate_records(&recs)?;
                payloads.push(Payload::Drawing(recs));
            }
            packing::KEY => {
                let rec: packing::PackingRecord = serde_json::from_value(value)
                    .map_err(|e| format!("{}的数据格式不对：{e}", packing::LABEL))?;
                packing::validate_records(&rec)?;
                payloads.push(Payload::Packing(rec));
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
        let s = preview(&c, &file(json!({"books": [{"title": "A"}, {"title": "B"}], "films": [{"title": "F"}], "future": []})))
            .unwrap();
        assert_eq!(s.modules.len(), 2);
        assert_eq!((s.modules[0].key.as_str(), s.modules[0].incoming, s.modules[0].existing), ("books", 2, 0));
        assert_eq!((s.modules[1].key.as_str(), s.modules[1].incoming), ("films", 1));
        assert_eq!(s.skipped, ["future"]);
    }

    #[test]
    fn rejects_bad_files() {
        let c = memory_db();
        assert!(preview(&c, "not json").is_err());
        assert!(preview(&c, &json!({"format": "x", "formatVersion": 1}).to_string()).is_err());
        assert!(preview(&c, &json!({"format": FORMAT, "formatVersion": 99, "modules": {}}).to_string()).is_err());
        assert!(preview(&c, &file(json!({"future": []}))).is_err());
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

    /// Agent tooling: build a scratch DB from the real import file for screenshots.
    /// `POMELO_SEED_DB=/tmp/x/pomelo.db cargo test seed_scratch_db -- --ignored` (never the real data dir).
    #[test]
    #[ignore]
    fn seed_scratch_db() {
        let Some(target) = std::env::var_os("POMELO_SEED_DB") else { return };
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype/pomelo-import.json");
        let text = std::fs::read_to_string(&path).expect("run `pnpm import:xlsx` first");
        let target = std::path::PathBuf::from(target);
        assert!(!target.to_string_lossy().contains("Application Support/Pomelo OS/"), "refusing real data dir");
        let mut c = crate::db::open(&target).unwrap();
        crate::db::migrate(&mut c, crate::db::MIGRATIONS, &temp_dir("seed")).unwrap();
        apply(&mut c, &text, true, &temp_dir("seed")).unwrap();
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
        let movie = crate::modules::kinds::tests::kind_id(&c, "films", "电影");
        assert!(fs.iter().all(|f| f.kind_id == Some(movie)));
        let fiction = crate::modules::kinds::tests::kind_id(&c, "books", "虚构");
        assert!(all.iter().all(|b| b.kind_id == Some(fiction)));

        let cs = drawing::courses(&c).unwrap();
        assert_eq!(
            cs.iter().map(|x| (x.name.as_str(), x.total)).collect::<Vec<_>>(),
            [("正确绘画前两年", 40), ("人体前传", 32), ("人体正传", 15), ("原创绘画全攻略", 93), ("通用板绘上手", 22)]
        );
        let ls = drawing::lessons(&c, cs[3].id).unwrap();
        assert!(ls.iter().all(|l| l.status == Status::Todo));
        assert_eq!(ls[3].code.as_deref(), Some("2B"));
        assert_eq!(ls[2].code, None); // DLC row: code folded into kind
        let names: Vec<String> = crate::modules::kinds::list(&c, "drawing").unwrap().into_iter().map(|k| k.name).collect();
        assert_eq!(names, ["正课", "DLC 跟画", "LIVE 辅导"]);

        let ts = packing::templates(&c).unwrap();
        assert_eq!(
            ts.iter().map(|t| (t.name.as_str(), t.item_count)).collect::<Vec<_>>(),
            [("国内游 3~7 天", 12), ("回家", 8), ("出国 5~10 天", 14)]
        );
        let abroad = packing::items(&c, packing::List::Template, ts[2].id).unwrap();
        let skin = abroad.iter().find(|i| i.name == "护肤品").unwrap();
        assert_eq!(skin.note.as_deref(), Some("随身最多携带 1L 液体，飞机托运无限制"));
        let cats: Vec<String> = crate::modules::kinds::list(&c, "packing").unwrap().into_iter().map(|k| k.name).collect();
        assert_eq!(cats, ["重要物品", "随身", "护理", "兴趣"]);
        assert!(packing::trips(&c).unwrap().is_empty());
        assert!(fs.iter().all(|f| !f.director.as_deref().unwrap_or("").contains('&')));
    }
}
