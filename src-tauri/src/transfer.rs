//! Pomelo data file (export + import), one JSON for all modules:
//! `{"format":"pomelo-export","formatVersion":13,"exportedAt":..,"appVersion":..,
//!   "kinds":{"books":["虚构",..],..},"modules":{"books":[BookRecord..],..}}`
//! v1 = no `kinds` and no packing timestamps (xlsx converter still writes v1); v2 = no todo task
//! types / logs; v3 = repeat occurrence in planDate, no doneBefore; v4 = check-in start / end in
//! planDate / repeatUntil, no finance (todo.rs adapts); v5 = no jobs; v6 = no notes; v7 = 书影 finishedMonth, finance
//! snapshots by month, holdings by value, entries with an account (books.rs / finance.rs adapt); v8 = jobs with one current
//! status, dates and 一步一步 links (jobs.rs adapts); v9 = no 书影 rereads, no notes tag settings; v10 = no 小拾
//! (module `picky`, picky/transfer.rs: it lives in picky.db, imported after pomelo.db); v11 = no 定时任务 (picky keeps its own); v12 = 定时任务 without 思考 (on); v15 = no 五花八门 / 循序渐进 (`custom`); v16 = tasks without the note they came from (`fromNote`); v17 = 五花八门 with 日期 fields (read as 年份), 循序渐进 without 类型; v18 = no Seeker (picky `seeker`).
//! All older versions are read.
//! Import: every record is validated before anything is written; all modules replace in one
//! transaction. A module's kinds lists (if present) replace its kinds first — only for modules
//! in the file (finance has two: fin_out / fin_in). Overwriting existing data requires `overwrite = true` and takes a snapshot first.
//! Export → import into an empty DB reproduces everything except ids (see roundtrip test).
//! Adding a module: new `Payload` variant + arms below + `export_payloads`.

use crate::modules::{books, custom, days, drawing, films, finance, jobs, kinds, notes, packing, todo};
use crate::modules::common::err;
use crate::{backup, AppState};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;
use tauri::State;

pub const FORMAT: &str = "pomelo-export";
pub const FORMAT_VERSION: u32 = 19;

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
    Finance(finance::FinanceRecord),
    Jobs(jobs::JobsRecord),
    Notes(notes::NotesRecord),
    Days(Vec<days::DayRecord>),
    Custom(Vec<custom::CustomRecord>),
}

impl Payload {
    fn key(&self) -> &'static str {
        match self {
            Payload::Books(_) => books::KEY,
            Payload::Films(_) => films::KEY,
            Payload::Drawing(_) => drawing::KEY,
            Payload::Packing(_) => packing::KEY,
            Payload::Todo(_) => todo::KEY,
            Payload::Finance(_) => finance::KEY,
            Payload::Jobs(_) => jobs::KEY,
            Payload::Notes(_) => notes::KEY,
            Payload::Days(_) => days::KEY,
            Payload::Custom(_) => custom::KEY,
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Payload::Books(_) => books::LABEL,
            Payload::Films(_) => films::LABEL,
            Payload::Drawing(_) => drawing::LABEL,
            Payload::Packing(_) => packing::LABEL,
            Payload::Todo(_) => todo::LABEL,
            Payload::Finance(_) => finance::LABEL,
            Payload::Jobs(_) => jobs::LABEL,
            Payload::Notes(_) => notes::LABEL,
            Payload::Days(_) => days::LABEL,
            Payload::Custom(_) => custom::LABEL,
        }
    }
    fn len(&self) -> usize {
        match self {
            Payload::Books(r) => r.len(),
            Payload::Films(r) => r.len(),
            Payload::Drawing(r) => r.len(),
            Payload::Packing(r) => r.len(),
            Payload::Todo(r) => r.len(),
            Payload::Finance(r) => r.len(),
            Payload::Jobs(r) => r.len(),
            Payload::Notes(r) => r.len(),
            Payload::Days(r) => r.len(),
            Payload::Custom(r) => r.len(),
        }
    }
    fn existing(&self, conn: &Connection) -> Result<i64, String> {
        match self {
            Payload::Books(_) => books::count(conn),
            Payload::Films(_) => films::count(conn),
            Payload::Drawing(_) => drawing::count(conn),
            Payload::Packing(_) => packing::count(conn),
            Payload::Todo(_) => todo::count(conn),
            Payload::Finance(_) => finance::count(conn),
            Payload::Jobs(_) => jobs::count(conn),
            Payload::Notes(_) => notes::count(conn),
            Payload::Days(_) => days::count(conn),
            Payload::Custom(_) => custom::count(conn),
        }
    }
    fn replace(&self, conn: &Connection) -> Result<usize, String> {
        match self {
            Payload::Books(r) => books::replace_all(conn, r),
            Payload::Films(r) => films::replace_all(conn, r),
            Payload::Drawing(r) => drawing::replace_all(conn, r),
            Payload::Packing(r) => packing::replace_all(conn, r),
            Payload::Todo(r) => todo::replace_all(conn, r),
            Payload::Finance(r) => finance::replace_all(conn, r),
            Payload::Jobs(r) => jobs::replace_all(conn, r),
            Payload::Notes(r) => notes::replace_all(conn, r),
            Payload::Days(r) => days::replace_all(conn, r),
            Payload::Custom(r) => custom::replace_all(conn, r),
        }
    }
    /// kinds.rs modules this payload's kinds lists live under.
    fn kind_modules(&self) -> &'static [&'static str] {
        match self {
            Payload::Books(_) => &[books::KEY],
            Payload::Films(_) => &[films::KEY],
            Payload::Drawing(_) => &[drawing::KEY],
            Payload::Packing(_) => &[packing::KEY],
            Payload::Todo(_) => &[],
            Payload::Finance(_) => &[finance::OUT_KINDS, finance::IN_KINDS],
            Payload::Jobs(_) => &[jobs::CHANNELS],
            Payload::Notes(_) | Payload::Days(_) | Payload::Custom(_) => &[],
        }
    }
    fn to_json(&self) -> Result<Value, String> {
        match self {
            Payload::Books(r) => serde_json::to_value(r),
            Payload::Films(r) => serde_json::to_value(r),
            Payload::Drawing(r) => serde_json::to_value(r),
            Payload::Packing(r) => serde_json::to_value(r),
            Payload::Todo(r) => serde_json::to_value(r),
            Payload::Finance(r) => serde_json::to_value(r),
            Payload::Jobs(r) => serde_json::to_value(r),
            Payload::Notes(r) => serde_json::to_value(r),
            Payload::Days(r) => serde_json::to_value(r),
            Payload::Custom(r) => serde_json::to_value(r),
        }
        .map_err(err)
    }
}

struct Parsed {
    payloads: Vec<Payload>,
    /// 小拾's part (picky.db).
    picky: Option<crate::picky::transfer::PickyRecord>,
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
    let mut picky = None;
    for (key, value) in file.modules {
        match key.as_str() {
            crate::picky::transfer::KEY => {
                let rec: crate::picky::transfer::PickyRecord =
                    serde_json::from_value(value).map_err(|e| format!("{}的数据格式不对：{e}", crate::picky::transfer::LABEL))?;
                crate::picky::transfer::validate(&rec)?;
                picky = Some(rec);
            }
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
            finance::KEY => {
                let rec: finance::FinanceRecord = serde_json::from_value(value)
                    .map_err(|e| format!("{}的数据格式不对：{e}", finance::LABEL))?;
                finance::validate_records(&rec)?;
                payloads.push(Payload::Finance(rec));
            }
            jobs::KEY => {
                let rec: jobs::JobsRecord = serde_json::from_value(value)
                    .map_err(|e| format!("{}的数据格式不对：{e}", jobs::LABEL))?;
                jobs::validate_records(&rec)?;
                payloads.push(Payload::Jobs(rec));
            }
            notes::KEY => {
                let rec: notes::NotesRecord = serde_json::from_value(value)
                    .map_err(|e| format!("{}的数据格式不对：{e}", notes::LABEL))?;
                notes::validate_records(&rec)?;
                payloads.push(Payload::Notes(rec));
            }
            days::KEY => {
                let rec: Vec<days::DayRecord> = serde_json::from_value(value).map_err(|e| format!("{}的数据格式不对：{e}", days::LABEL))?;
                days::validate_records(&rec)?;
                payloads.push(Payload::Days(rec));
            }
            custom::KEY => {
                let rec: Vec<custom::CustomRecord> = serde_json::from_value(value).map_err(|e| format!("{}的数据格式不对：{e}", custom::LABEL))?;
                custom::validate_records(&rec)?;
                payloads.push(Payload::Custom(rec));
            }
            _ => skipped.push(key),
        }
    }
    if payloads.is_empty() && picky.is_none() {
        return Err("文件里没有这个版本的 App 能导入的数据".into());
    }
    let mut kind_lists = Vec::new();
    for p in &payloads {
        for &module in p.kind_modules() {
            if let Some(names) = file.kinds.get(module) {
                kinds::validate_names(p.label(), names)?;
                kind_lists.push((module, names.clone()));
            }
        }
    }
    Ok(Parsed { payloads, picky, kinds: kind_lists, skipped })
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
    if parsed.payloads.is_empty() {
        return Ok(summary);
    }
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
    // notes before todo: tasks find the notes they came from (by createdAt)
    let mut order: Vec<&Payload> = parsed.payloads.iter().collect();
    order.sort_by_key(|p| !matches!(p, Payload::Notes(_)));
    for p in order {
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
        Payload::Finance(finance::export(conn)?),
        Payload::Jobs(jobs::export(conn)?),
        Payload::Notes(notes::export(conn)?),
        Payload::Days(days::export(conn)?),
        Payload::Custom(custom::export(conn)?),
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
#[cfg(test)]
pub fn export(conn: &mut Connection) -> Result<(String, Vec<ModuleSummary>), String> {
    export_with(conn, None)
}

/// With 小拾's part (read from picky.db by the caller).
pub fn export_with(conn: &mut Connection, picky: Option<&crate::picky::transfer::PickyRecord>) -> Result<(String, Vec<ModuleSummary>), String> {
    let tx = conn.transaction().map_err(err)?;
    let payloads = export_payloads(&tx)?;
    let mut modules = Map::new();
    for p in &payloads {
        modules.insert(p.key().into(), p.to_json()?);
    }
    if let Some(r) = picky {
        modules.insert(crate::picky::transfer::KEY.into(), serde_json::to_value(r).map_err(err)?);
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
    let mut counts: Vec<ModuleSummary> = payloads.iter().map(|p| ModuleSummary { key: p.key().into(), label: p.label().into(), incoming: p.len(), existing: 0 }).collect();
    if let Some(r) = picky {
        counts.push(ModuleSummary { key: crate::picky::transfer::KEY.into(), label: crate::picky::transfer::LABEL.into(), incoming: r.len(), existing: 0 });
    }
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
    let picky = if state.picky.is_open() { Some(state.picky.with_db(|c| crate::picky::transfer::export(c))?) } else { None };
    let (text, modules) = state.with_db(|c| export_with(c, picky.as_ref()))?;
    write_atomic(Path::new(&path), &text)?;
    Ok(ExportSummary { path, modules })
}

/// 小拾's line in a summary (what the file has, what picky.db has).
fn picky_summary(state: &AppState, rec: &crate::picky::transfer::PickyRecord) -> Result<ModuleSummary, String> {
    Ok(ModuleSummary {
        key: crate::picky::transfer::KEY.into(),
        label: crate::picky::transfer::LABEL.into(),
        incoming: rec.len(),
        existing: state.picky.with_db(|c| crate::picky::transfer::count(c))?,
    })
}

#[tauri::command]
pub fn import_preview(state: State<AppState>, path: String) -> Result<ImportSummary, String> {
    let text = read(&path)?;
    let mut out = state.with_db(|c| preview(c, &text))?;
    if let Some(r) = parse(&text)?.picky {
        out.modules.push(picky_summary(&state, &r)?);
    }
    Ok(out)
}

#[tauri::command]
pub fn import_apply(state: State<AppState>, path: String, overwrite: bool) -> Result<ImportSummary, String> {
    let text = read(&path)?;
    let backups = state.paths.backups.clone();
    let picky = parse(&text)?.picky;
    if let Some(r) = &picky {
        let line = picky_summary(&state, r)?;
        if line.existing > 0 && !overwrite {
            return Err(format!("{}已经有数据，需要确认覆盖后才能导入", line.label));
        }
    }
    let mut out = state.with_db(|c| apply(c, &text, overwrite, &backups))?;
    if let Some(r) = &picky {
        let line = picky_summary(&state, r)?;
        state.picky.with_db(|c| {
            if line.existing > 0 {
                crate::backup::snapshot_of(crate::backup::Store::Picky, c, &backups, "before-import")?;
            }
            crate::picky::transfer::replace(c, r)
        })?;
        out.modules.push(line);
    }
    state.clear_history();
    Ok(out)
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
            INSERT INTO books (title, author, country, kind_id, status, rating, finished_year, note, created_at, updated_at) VALUES
              ('小王子', '圣埃克苏佩里', '法国', (SELECT id FROM kinds WHERE module='books' AND name='漫画书'), 'done', 5, '2025', '第二遍', '2025-01-01T00:00:00.000Z', '2025-02-01T00:00:00.000Z'),
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
        // 附件: names and checksums travel (the files stay on the Mac they were added on)
        let att_dir = temp_dir("roundtrip-attachments");
        let pic = crate::modules::attachments::store(&a, &att_dir, "海报.png", b"poster").unwrap().id;
        let pdf = crate::modules::attachments::store(&a, &att_dir, "开题报告.pdf", b"%PDF-report").unwrap().id;
        let goal = todo::create_goal(&a, serde_json::from_value(json!({"groupId": tg, "name": "毕业", "status": "paused", "attachments": [pdf]})).unwrap()).unwrap().id;
        let root = todo::create_task(&a, serde_json::from_value(json!({"goalId": goal, "title": "答辩前提交", "collapsed": true, "attachments": [pic, pdf]})).unwrap()).unwrap().id;
        let kid = todo::create_task(&a, serde_json::from_value(json!({"parentId": root, "title": "IA Report", "dueDate": "2027-01-22"})).unwrap()).unwrap().id;
        todo::create_task(&a, serde_json::from_value(json!({"parentId": root, "title": "Poster"})).unwrap()).unwrap();
        todo::create_task(&a, serde_json::from_value(json!({"groupId": tg, "title": "窗口", "startDate": "2027-01-22", "dueDate": "2027-01-25"})).unwrap()).unwrap();
        todo::set_done(&a, kid, true).unwrap();
        // finance: a renamed 分类, an account with a balance, a 固定支出 that already posted once
        let fin_food = crate::modules::kinds::tests::kind_id(&a, finance::OUT_KINDS, "餐饮");
        crate::modules::kinds::rename(&a, fin_food, "吃饭").unwrap();
        let acc = finance::create_account(&a, serde_json::from_value(json!({"name": "DBS", "currency": "SGD"})).unwrap()).unwrap().id;
        finance::save_balances(&a, "2026-09-30", &[finance::BalanceInput { account_id: acc, amount: Some(123456) }], &[]).unwrap();
        finance::create_fixed(
            &a,
            serde_json::from_value(json!({"name": "饭卡", "amount": 5000, "currency": "SGD", "kindId": fin_food, "rule": "monthly:1", "nextDay": "2026-09-01"}))
                .unwrap(),
            "2026-09-15",
        )
        .unwrap();
        finance::create_entry(&a, serde_json::from_value(json!({"amount": 7000, "currency": "SGD", "day": "2026-09-28", "endDay": "2026-10-04"})).unwrap()).unwrap();
        // jobs: an application that skipped 笔试 and is at 面试
        let st = |n: &str| jobs::statuses(&a).unwrap().into_iter().find(|s| s.name == n).unwrap().id;
        let steps = json!([{"statusId": st("已投递"), "done": true, "doneOn": "2026-10-01"}, {"statusId": st("面试"), "done": true}, {"statusId": st("Offer")}]);
        jobs::create_job(&a, serde_json::from_value(json!({"company": "X", "steps": steps})).unwrap(), "2026-10-05")
            .unwrap();
        // notes: a pinned one quoting another (refs travel by createdAt)
        let first_note = notes::create(&a, serde_json::from_value(json!({"content": "#读书/摘抄 第一条"})).unwrap()).unwrap().id;
        notes::create(&a, serde_json::from_value(json!({"content": "批注 **加粗**", "pinned": true, "refs": [first_note], "attachments": [pic]})).unwrap()).unwrap();
        notes::set_tag(&a, "读书", serde_json::from_value(json!({"pinned": true, "icon": "i:book-open"})).unwrap()).unwrap();
        // 笔记转待办: a task made from the first note (linked by its createdAt)
        todo::create_from_note(&a, first_note, Some(tg), None, &["整理摘抄".into()], "2026-10-17").unwrap();
        notes::set_tag(&a, "读书/摘抄", serde_json::from_value(json!({"icon": "📖"})).unwrap()).unwrap();
        // 来日方长: a birthday with a gift, pinned
        days::create(&a, serde_json::from_value(json!({"kind": "birthday", "name": "妈妈", "date": "1966-11-02", "remind": [7], "pinned": true, "gifts": [{"year": "2025", "gift": "茶具"}]})).unwrap(), "2026-10-17").unwrap();
        // 五花八门 (a list with fields and an item) and 循序渐进 (called 季 / 集)
        let (_, _) = crate::modules::lists::tests::seed(&a);
        let games = custom::all(&a).unwrap()[0].id;
        let year = crate::modules::lists::create_field(&a, games, serde_json::from_value(json!({"name": "玩过", "type": "year"})).unwrap()).unwrap().id;
        crate::modules::lists::create_item(&a, games, serde_json::from_value(json!({"name": "塞尔达", "values": {"1": "Switch", "3": 5, year.to_string(): "2025"}})).unwrap()).unwrap();
        crate::modules::progress::tests::seed(&a);
        // 书影: a reread
        let little = books::list(&a).unwrap().into_iter().find(|b| b.title == "小王子").unwrap().id;
        books::update(&a, little, serde_json::from_value(json!({"rereads": [{"year": "2026", "rating": 5}]})).unwrap()).unwrap();

        let (text, counts) = export(&mut a).unwrap();
        assert_eq!(counts.iter().map(|m| (m.key.as_str(), m.incoming)).collect::<Vec<_>>(), [("books", 2), ("films", 1), ("drawing", 2), ("packing", 4), ("todo", 7), ("finance", 4), ("jobs", 1), ("notes", 2), ("days", 1), ("custom", 2)]);

        let dir = temp_dir("roundtrip");
        let mut b = memory_db();
        let s = apply(&mut b, &text, false, &dir).unwrap(); // defaults only → no overwrite needed
        assert!(s.backup.is_none());
        let (text2, _) = export(&mut b).unwrap();
        assert_eq!(without_time(&text), without_time(&text2));

        // spot checks on the imported side
        let v = without_time(&text2);
        assert_eq!(v["formatVersion"], FORMAT_VERSION);
        let made = v["modules"]["todo"]["groups"][0]["tasks"].as_array().unwrap().iter().find(|t| t["title"] == "整理摘抄").unwrap().clone();
        assert_eq!(made["fromNote"], v["modules"]["notes"]["notes"][0]["createdAt"]);
        assert_eq!(v["modules"]["custom"][0]["items"][0]["values"], json!({"平台": "Switch", "评分": 5, "玩过": "2025"}));
        assert_eq!((v["modules"]["custom"][1]["kinds"].clone(), v["modules"]["custom"][1]["courses"][0]["lessons"][0]["kind"].clone()), (json!(["正篇", "番外"]), json!("正篇")));
        assert_eq!((v["modules"]["custom"][1]["courseWord"].as_str(), v["modules"]["custom"][1]["courses"][0]["lessons"][2]["title"].as_str()), (Some("季"), Some("告别")));
        assert!(v["modules"]["custom"][0].get("courses").is_none());
        assert_eq!(v["modules"]["days"][0]["gifts"][0]["gift"], "茶具");
        assert_eq!(v["modules"]["notes"]["notes"][1]["attachments"][0]["name"], "海报.png");
        assert_eq!(v["modules"]["todo"]["groups"][0]["goals"][0]["attachments"][0]["name"], "开题报告.pdf");
        // on the other Mac they are names waiting for their files
        let missing: i64 = b.query_row("SELECT count(*) FROM attachment_files WHERE stored IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(missing, 2);
        assert_eq!(v["modules"]["notes"]["notes"][1]["refs"][0], v["modules"]["notes"]["notes"][0]["createdAt"]);
        assert_eq!(v["modules"]["notes"]["tags"], json!([{"tag": "读书", "pinned": true, "icon": "i:book-open"}, {"tag": "读书/摘抄", "icon": "📖"}]));
        assert_eq!(v["modules"]["books"][0]["rereads"], json!([{"year": "2026", "rating": 5}]));
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
    fn a_file_with_only_picky_reads_and_leaves_pomelo_alone() {
        let mut c = memory_db();
        let text = json!({"format": FORMAT, "formatVersion": 11, "modules": {"picky": {"globalInstructions": "用中文", "projects": [{"name": "论文"}], "memories": [{"content": "住在新加坡", "project": "论文", "notedOn": "2026-10-01"}]}}}).to_string();
        let p = parse(&text).unwrap();
        assert!(p.payloads.is_empty() && p.picky.as_ref().unwrap().len() == 2);
        assert!(apply(&mut c, &text, false, &temp_dir("picky-only")).unwrap().modules.is_empty());
        let bad = json!({"format": FORMAT, "formatVersion": 11, "modules": {"picky": {"memories": [{"content": "x", "project": "没有", "notedOn": "2026-10-01"}]}}}).to_string();
        assert!(parse(&bad).is_err());
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
        let text = std::fs::read_to_string(&path).expect("prototype file missing (local only)");
        let target = std::path::PathBuf::from(target);
        assert!(!target.to_string_lossy().contains("Application Support/Pomelo OS/"), "refusing real data dir");
        let mut c = crate::db::open(&target).unwrap();
        crate::db::migrate(&mut c, crate::db::MIGRATIONS, &temp_dir("seed")).unwrap();
        apply(&mut c, &text, true, &temp_dir("seed")).unwrap();
        for extra in ["todo-import.json", "jobs-import.json", "notes-import.json"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype").join(extra);
            if let Ok(text) = std::fs::read_to_string(&path) {
                apply(&mut c, &text, true, &temp_dir("seed")).unwrap();
            }
        }
    }

    /// Acceptance on real data (in memory): xlsx import → export → empty DB → import → same export.
    #[test]
    #[ignore]
    fn real_prototype_roundtrip() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype/pomelo-import.json");
        let text = std::fs::read_to_string(&path).expect("prototype file missing (local only)");
        let mut a = memory_db();
        apply(&mut a, &text, false, &temp_dir("real-rt")).unwrap();
        let (out, counts) = export(&mut a).unwrap();
        assert_eq!(counts.iter().map(|m| m.incoming).collect::<Vec<_>>(), [21, 50, 5, 3, 0, 0, 0, 0, 0, 0]); // … notes, days, custom
        let mut b = memory_db();
        apply(&mut b, &out, false, &temp_dir("real-rt")).unwrap();
        assert_eq!(without_time(&out), without_time(&export(&mut b).unwrap().0));
    }

    /// Seeker's applications (prototype, gitignored) import into the default statuses.
    #[test]
    #[ignore]
    fn real_jobs_import() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype/jobs-import.json");
        let text = std::fs::read_to_string(&path).expect("prototype file missing (local only)");
        let mut c = memory_db();
        apply(&mut c, &text, false, &temp_dir("real-jobs")).unwrap();
        let js = jobs::jobs(&c, "2026-10-05").unwrap();
        assert_eq!(js.len(), 3);
        assert!(js.iter().all(|j| j.resume.as_deref().is_some_and(|r| r.starts_with("~/Documents/Seeker/cv/"))));
        assert_eq!(jobs::statuses(&c).unwrap().len(), 7); // statuses kept
    }

    /// The converted Markdown todo list (prototype, gitignored) imports cleanly.
    #[test]
    #[ignore]
    fn real_todo_import() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype/todo-import.json");
        let text = std::fs::read_to_string(&path).expect("prototype file missing (local only)");
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

    /// The notes imported from Flomo (prototype/notes-import.json, gitignored) import cleanly and round-trip.
    #[test]
    #[ignore]
    fn real_notes_import() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype/notes-import.json");
        let text = std::fs::read_to_string(&path).expect("prototype/notes-import.json missing (local only)");
        let mut c = memory_db();
        apply(&mut c, &text, false, &temp_dir("real-notes")).unwrap();
        let o = notes::overview(&c).unwrap();
        assert_eq!((o.total, o.tags.len()), (179, 42));
        assert_eq!(o.days.first().unwrap().day, "2024-12-12");
        let all = notes::list(&c, &notes::Filter::default()).unwrap();
        assert!(all.iter().all(|n| !n.tags.is_empty()));
        let (out, _) = export(&mut c).unwrap();
        let mut b = memory_db();
        apply(&mut b, &out, false, &temp_dir("real-notes")).unwrap();
        assert_eq!(without_time(&out), without_time(&export(&mut b).unwrap().0));
    }

    /// Real prototype data (local only, gitignored). Run: cargo test -- --ignored
    #[test]
    #[ignore]
    fn real_prototype_import() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype/pomelo-import.json");
        let text = std::fs::read_to_string(&path).expect("prototype file missing (local only)");
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
