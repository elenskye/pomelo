//! Pomelo data file (export + import), one JSON for all modules:
//! `{"format":"pomelo-export","formatVersion":2,"exportedAt":..,"appVersion":..,
//!   "kinds":{"books":["虚构",..],..},"modules":{"books":[BookRecord..],..}}`
//! v1 = no `kinds` and no packing timestamps (xlsx converter still writes v1); both are read.
//! Import: every record is validated before anything is written; all modules replace in one
//! transaction. A module's kinds list (if present) replaces its kinds first — only for modules
//! in the file. Overwriting existing data requires `overwrite = true` and takes a snapshot first.
//! Export → import into an empty DB reproduces everything except ids (see roundtrip test).
//! Adding a module: new `Payload` variant + arms below + `export_payloads`.

use crate::modules::{books, drawing, films, kinds, packing, todo};
use crate::modules::common::err;
use crate::{backup, AppState};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;
use tauri::State;

pub const FORMAT: &str = "pomelo-export";
pub const FORMAT_VERSION: u32 = 2;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportFile {
    format: String,
    format_version: u32,
    #[serde(default)]
    kinds: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    modules: Map<String, Value>,
}

enum Payload {
    Books(Vec<books::BookRecord>),
    Films(Vec<films::FilmRecord>),
    Drawing(Vec<drawing::CourseRecord>),
    Packing(packing::PackingRecord),
    Todo(todo::TodoRecord),
}

impl Payload {
    fn key(&self) -> &'static str {
        match self {
            Payload::Books(_) => books::KEY,
            Payload::Films(_) => films::KEY,
            Payload::Drawing(_) => drawing::KEY,
            Payload::Packing(_) => packing::KEY,
            Payload::Todo(_) => todo::KEY,
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Payload::Books(_) => books::LABEL,
            Payload::Films(_) => films::LABEL,
            Payload::Drawing(_) => drawing::LABEL,
            Payload::Packing(_) => packing::LABEL,
            Payload::Todo(_) => todo::LABEL,
        }
    }
    fn len(&self) -> usize {
        match self {
            Payload::Books(r) => r.len(),
            Payload::Films(r) => r.len(),
            Payload::Drawing(r) => r.len(),
            Payload::Packing(r) => r.len(),
            Payload::Todo(r) => r.len(),
        }
    }
    fn existing(&self, conn: &Connection) -> Result<i64, String> {
        match self {
            Payload::Books(_) => books::count(conn),
            Payload::Films(_) => films::count(conn),
            Payload::Drawing(_) => drawing::count(conn),
            Payload::Packing(_) => packing::count(conn),
            Payload::Todo(_) => todo::count(conn),
        }
    }
    fn replace(&self, conn: &Connection) -> Result<usize, String> {
        match self {
            Payload::Books(r) => books::replace_all(conn, r),
            Payload::Films(r) => films::replace_all(conn, r),
            Payload::Drawing(r) => drawing::replace_all(conn, r),
            Payload::Packing(r) => packing::replace_all(conn, r),
            Payload::Todo(r) => todo::replace_all(conn, r),
        }
    }
    fn to_json(&self) -> Result<Value, String> {
        match self {
            Payload::Books(r) => serde_json::to_value(r),
            Payload::Films(r) => serde_json::to_value(r),
            Payload::Drawing(r) => serde_json::to_value(r),
            Payload::Packing(r) => serde_json::to_value(r),
            Payload::Todo(r) => serde_json::to_value(r),
        }
        .map_err(err)
    }
}

struct Parsed {
    payloads: Vec<Payload>,
    /// (module key, kind names) for modules in `payloads` only.
    kinds: Vec<(&'static str, Vec<String>)>,
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
            todo::KEY => {
                let rec: todo::TodoRecord = serde_json::from_value(value)
                    .map_err(|e| format!("{}的数据格式不对：{e}", todo::LABEL))?;
                todo::validate_records(&rec)?;
                payloads.push(Payload::Todo(rec));
            }
            _ => skipped.push(key),
        }
    }
    if payloads.is_empty() {
        return Err("文件里没有这个版本的 App 能导入的数据".into());
    }
    let mut kind_lists = Vec::new();
    for p in &payloads {
        if let Some(names) = file.kinds.get(p.key()) {
            kinds::validate_names(p.label(), names)?;
            kind_lists.push((p.key(), names.clone()));
        }
    }
    Ok(Parsed { payloads, kinds: kind_lists, skipped })
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
    for (module, names) in &parsed.kinds {
        kinds::replace_names(&tx, module, names)?;
    }
    for p in &parsed.payloads {
        p.replace(&tx)?;
    }
    tx.commit().map_err(err)?;
    Ok(summary)
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("读不了这个文件：{e}"))
}

// ---------- export ----------

fn export_payloads(conn: &Connection) -> Result<Vec<Payload>, String> {
    Ok(vec![
        Payload::Books(books::export(conn)?),
        Payload::Films(films::export(conn)?),
        Payload::Drawing(drawing::export(conn)?),
        Payload::Packing(packing::export(conn)?),
        Payload::Todo(todo::export(conn)?),
    ])
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportOut {
    format: &'static str,
    format_version: u32,
    exported_at: String,
    app_version: &'static str,
    kinds: BTreeMap<String, Vec<String>>,
    modules: Map<String, Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub path: String,
    /// `incoming` = records written; `existing` unused (0).
    pub modules: Vec<ModuleSummary>,
}

/// Whole database → pretty JSON text + per-module counts. One read transaction = consistent.
pub fn export(conn: &mut Connection) -> Result<(String, Vec<ModuleSummary>), String> {
    let tx = conn.transaction().map_err(err)?;
    let payloads = export_payloads(&tx)?;
    let mut modules = Map::new();
    for p in &payloads {
        modules.insert(p.key().into(), p.to_json()?);
    }
    let out = ExportOut {
        format: FORMAT,
        format_version: FORMAT_VERSION,
        exported_at: tx
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |r| r.get(0))
            .map_err(err)?,
        app_version: env!("CARGO_PKG_VERSION"),
        kinds: kinds::export(&tx)?,
        modules,
    };
    let text = serde_json::to_string_pretty(&out).map_err(err)?;
    let counts = payloads
        .iter()
        .map(|p| ModuleSummary { key: p.key().into(), label: p.label().into(), incoming: p.len(), existing: 0 })
        .collect();
    Ok((text, counts))
}

/// Writes via a temp file + rename so an existing file is never left half-written.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.part");
    std::fs::write(&tmp, text)
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("导出文件没能写入：{e}")
        })
}

#[tauri::command]
pub fn export_to(state: State<AppState>, path: String) -> Result<ExportSummary, String> {
    let (text, modules) = state.with_db(export)?;
    write_atomic(Path::new(&path), &text)?;
    Ok(ExportSummary { path, modules })
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

    fn without_time(text: &str) -> Value {
        let mut v: Value = serde_json::from_str(text).unwrap();
        v.as_object_mut().unwrap().remove("exportedAt");
        v
    }

    fn sql(c: &Connection, q: &str) {
        c.execute_batch(q).unwrap();
    }

    /// Acceptance: export → empty DB → import → export gives the same file (ids aside).
    #[test]
    fn export_import_roundtrip_is_lossless() {
        let mut a = memory_db();
        // customized kinds: renamed, reordered, deleted, unused extra
        let comic = crate::modules::kinds::tests::kind_id(&a, "books", "漫画");
        crate::modules::kinds::rename(&a, comic, "漫画书").unwrap();
        crate::modules::kinds::delete(&a, crate::modules::kinds::tests::kind_id(&a, "books", "其他")).unwrap();
        crate::modules::kinds::create(&a, "books", "没用上的类型").unwrap();
        let ids: Vec<i64> = crate::modules::kinds::list(&a, "films").unwrap().iter().rev().map(|k| k.id).collect();
        crate::modules::kinds::reorder(&a, "films", &ids).unwrap();
        crate::modules::kinds::create(&a, "packing", "证件").unwrap();
        sql(&a, "
            INSERT INTO books (title, author, country, kind_id, status, rating, finished_month, note, created_at, updated_at) VALUES
              ('小王子', '圣埃克苏佩里', '法国', (SELECT id FROM kinds WHERE module='books' AND name='漫画书'), 'done', 5, '2025-11', '第二遍', '2025-01-01T00:00:00.000Z', '2025-02-01T00:00:00.000Z'),
              ('无类型', NULL, NULL, NULL, 'unmarked', NULL, NULL, NULL, '2025-01-02T00:00:00.000Z', '2025-01-02T00:00:00.000Z');
            INSERT INTO films (title, director, kind_id, status, rating) VALUES
              ('千与千寻', '宫崎骏', (SELECT id FROM kinds WHERE module='films' AND name='动漫'), 'doing', 4);
            INSERT INTO drawing_courses (name, note, sort_order) VALUES ('B 课', '备注', 5), ('A 课', NULL, 9);
            INSERT INTO drawing_lessons (course_id, code, kind_id, title, status, finished_month, sort_order) VALUES
              ((SELECT id FROM drawing_courses WHERE name='B 课'), '2B', (SELECT id FROM kinds WHERE module='drawing' AND name='正课'), '第二课', 'done', '2026-08', 7),
              ((SELECT id FROM drawing_courses WHERE name='B 课'), NULL, NULL, '第一课', 'todo', NULL, 3);
            INSERT INTO packing_templates (name, note, sort_order) VALUES ('回家', NULL, 2), ('出国', '护照', 1), ('要删的', NULL, 3);
            INSERT INTO packing_template_items (template_id, kind_id, name, note, sort_order) VALUES
              ((SELECT id FROM packing_templates WHERE name='回家'), (SELECT id FROM kinds WHERE module='packing' AND name='证件'), '身份证', NULL, 1),
              ((SELECT id FROM packing_templates WHERE name='回家'), NULL, '家钥匙 🔑', '别忘了', 2);
        ");
        let home = a.query_row("SELECT id FROM packing_templates WHERE name='回家'", [], |r| r.get(0)).unwrap();
        let gone = a.query_row("SELECT id FROM packing_templates WHERE name='要删的'", [], |r| r.get(0)).unwrap();
        let t1 = packing::create_trip(&a, serde_json::from_value(json!({"name": "国庆回家", "departOn": "2026-10-01"})).unwrap(), Some(home)).unwrap();
        packing::create_trip(&a, serde_json::from_value(json!({"name": "孤儿出行"})).unwrap(), Some(gone)).unwrap();
        packing::delete_template(&a, gone).unwrap();
        let first = packing::items(&a, packing::List::Trip, t1.id).unwrap()[0].id;
        packing::update_item(&a, packing::List::Trip, first, serde_json::from_value(json!({"packed": true})).unwrap()).unwrap();
        let tg = todo::create_group(&a, "🖥️ Diploma").unwrap().id;
        let goal = todo::create_goal(&a, serde_json::from_value(json!({"groupId": tg, "name": "毕业", "status": "paused"})).unwrap()).unwrap().id;
        let root = todo::create_task(&a, serde_json::from_value(json!({"goalId": goal, "title": "答辩前提交", "collapsed": true})).unwrap()).unwrap().id;
        let kid = todo::create_task(&a, serde_json::from_value(json!({"parentId": root, "title": "IA Report", "dueDate": "2027-01-22"})).unwrap()).unwrap().id;
        todo::create_task(&a, serde_json::from_value(json!({"parentId": root, "title": "Poster"})).unwrap()).unwrap();
        todo::create_task(&a, serde_json::from_value(json!({"groupId": tg, "title": "窗口", "startDate": "2027-01-22", "dueDate": "2027-01-25"})).unwrap()).unwrap();
        todo::set_done(&a, kid, true).unwrap();

        let (text, counts) = export(&mut a).unwrap();
        assert_eq!(counts.iter().map(|m| (m.key.as_str(), m.incoming)).collect::<Vec<_>>(), [("books", 2), ("films", 1), ("drawing", 2), ("packing", 4), ("todo", 6)]);

        let dir = temp_dir("roundtrip");
        let mut b = memory_db();
        let s = apply(&mut b, &text, false, &dir).unwrap(); // defaults only → no overwrite needed
        assert!(s.backup.is_none());
        let (text2, _) = export(&mut b).unwrap();
        assert_eq!(without_time(&text), without_time(&text2));

        // spot checks on the imported side
        let v = without_time(&text2);
        assert_eq!(v["formatVersion"], 2);
        assert_eq!(v["kinds"]["books"], json!(["虚构", "非虚构", "漫画书", "没用上的类型"]));
        assert_eq!(v["kinds"]["films"], json!(["动漫", "剧集", "电影"]));
        assert_eq!(v["modules"]["drawing"][0]["name"], "B 课");
        assert_eq!(v["modules"]["drawing"][0]["lessons"][0]["title"], "第一课");
        assert_eq!(v["modules"]["packing"]["templates"][0]["name"], "出国");
        let trips = &v["modules"]["packing"]["trips"];
        assert_eq!((trips[0]["name"].as_str(), trips[0]["template"].as_str()), (Some("国庆回家"), Some("回家")));
        assert_eq!(trips[1]["template"], Value::Null);
        assert_eq!(trips[0]["items"][0]["packed"], true);
        assert!(v["modules"]["packing"]["templates"][1]["items"][0].get("packed").is_none());
        assert_eq!(v["modules"]["books"][0]["createdAt"], "2025-01-01T00:00:00.000Z");
        let bt = packing::trips(&b).unwrap();
        assert_eq!(bt.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["孤儿出行", "国庆回家"]); // newest first, as before
    }

    #[test]
    fn kinds_in_file_apply_only_to_imported_modules() {
        let dir = temp_dir("import-kinds");
        let mut c = memory_db();
        let text = json!({
            "format": FORMAT, "formatVersion": 2,
            "kinds": {"books": ["绘本", "虚构"], "films": ["只有类型"], "future": ["x"]},
            "modules": {"books": [{"title": "A", "kind": "虚构"}]}
        })
        .to_string();
        apply(&mut c, &text, false, &dir).unwrap();
        let names = |m| crate::modules::kinds::list(&c, m).unwrap().into_iter().map(|k| k.name).collect::<Vec<_>>();
        assert_eq!(names("books"), ["绘本", "虚构"]);
        assert_eq!(names("films"), ["电影", "剧集", "动漫"]); // films not in file → untouched
        let bad = json!({"format": FORMAT, "formatVersion": 2, "kinds": {"books": ["a", "a"]}, "modules": {"books": []}}).to_string();
        assert!(preview(&c, &bad).unwrap_err().contains("重复"));
    }

    #[test]
    fn export_file_is_written_atomically() {
        let dir = temp_dir("export-write");
        let path = dir.join("pomelo-export-2026-09-29.json");
        std::fs::write(&path, "old").unwrap();
        let mut c = memory_db();
        let (text, _) = export(&mut c).unwrap();
        write_atomic(&path, &text).unwrap();
        assert!(preview(&c, &std::fs::read_to_string(&path).unwrap()).is_ok());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1); // no leftover .part
        assert!(write_atomic(&dir.join("missing/x.json"), &text).is_err());
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
        let todo_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype/todo-import.json");
        if let Ok(todo_text) = std::fs::read_to_string(&todo_path) {
            apply(&mut c, &todo_text, true, &temp_dir("seed")).unwrap();
        }
    }

    /// Acceptance on real data (in memory): xlsx import → export → empty DB → import → same export.
    #[test]
    #[ignore]
    fn real_prototype_roundtrip() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype/pomelo-import.json");
        let text = std::fs::read_to_string(&path).expect("run `pnpm import:xlsx` first");
        let mut a = memory_db();
        apply(&mut a, &text, false, &temp_dir("real-rt")).unwrap();
        let (out, counts) = export(&mut a).unwrap();
        assert_eq!(counts.iter().map(|m| m.incoming).collect::<Vec<_>>(), [21, 50, 5, 3, 0]);
        let mut b = memory_db();
        apply(&mut b, &out, false, &temp_dir("real-rt")).unwrap();
        assert_eq!(without_time(&out), without_time(&export(&mut b).unwrap().0));
    }

    /// The converted Markdown todo list (`pnpm import:todo`, gitignored) imports cleanly.
    #[test]
    #[ignore]
    fn real_todo_import() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype/todo-import.json");
        let text = std::fs::read_to_string(&path).expect("run `pnpm import:todo` first");
        let mut c = memory_db();
        apply(&mut c, &text, false, &temp_dir("real-todo")).unwrap();
        let gs = todo::groups(&c).unwrap();
        assert_eq!(gs.len(), 3);
        assert_eq!(todo::goals(&c).unwrap().len(), 4);
        let n: i64 = c.query_row("SELECT count(*) FROM todo_tasks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 21);
        let all: String = c.query_row("SELECT group_concat(coalesce(title, '') || coalesce(note, ''), '|') FROM todo_tasks", [], |r| r.get(0)).unwrap();
        assert!(!all.contains("密码"));
        let (out, _) = export(&mut c).unwrap();
        let mut b = memory_db();
        apply(&mut b, &out, false, &temp_dir("real-todo")).unwrap();
        assert_eq!(without_time(&out), without_time(&export(&mut b).unwrap().0));
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
