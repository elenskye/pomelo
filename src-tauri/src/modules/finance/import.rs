//! 导入账单's bookkeeping. Importer (youzi/assistants/importer.rs) reads a statement and leaves it here as a batch
//! of rows 待核对; only rows I confirm (入账) become ledger entries. `source` is what was read and stays as it was,
//! `draft` is what I can change. Possible duplicates (same amount within 7 days, or the row the model points at)
//! hold a row back until decided: the model's decision, 是这笔 or 不是重复.
use super::*;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub day: String,
    pub amount: Option<i64>,
    pub currency: Currency,
    /// out | in | refund | neutral. Neutral never enters fin_entries.
    pub treatment: String,
    pub category: Option<String>,
    pub note: String,
    pub refund_of: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceRow {
    pub sheet: String,
    pub row_number: usize,
    pub raw: Vec<(String, String)>,
    pub trade_id: String,
    pub time: String,
    pub status: String,
    pub draft: Draft,
    pub issues: Vec<String>,
    /// Only rows read before v0.8's Importer: where the Mac's OCR found the row on the screenshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<[f64; 4]>,
}

#[derive(Debug)]
pub struct Parsed {
    pub source: String,
    pub rows: Vec<SourceRow>,
}

/// What Importer did with a row. Older rows may carry fields no longer read (categoryDone, noteDone, review).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    #[serde(default)]
    pub model: String,
    /// 再整理 didn't finish for this row, or a value the model gave wasn't used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_error: Option<String>,
    /// The model judged the row's 疑点: nothing left for me (`settled`), and its reason or what to check (`why`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub settled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// "" | duplicate | distinct | review | error
    #[serde(default)]
    pub duplicate: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_row: Option<i64>,
    /// review: the ledger row it means (e:<entry> | r:<row>), offered with the 候选. Not in data files (ids change).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// I set the 分类 by hand (批量改分类): 再整理 leaves it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub my_category: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRow {
    pub file_name: Option<String>,
    pub ai: Option<Record>,
    pub id: i64,
    pub source: SourceRow,
    pub draft: Draft,
    pub state: String,
    pub entry_id: Option<i64>,
    pub resolution: Option<String>,
    pub candidates: Vec<MatchCandidate>,
    pub pending_candidates: Vec<PendingCandidate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchCandidate { pub entry: Entry, pub sources: Vec<String> }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingCandidate { pub id: i64, pub batch_id: i64, pub name: String, pub draft: Draft, pub source: SourceRow }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// `name` is the file's (选原图 checks it); `title` the one I gave it (改名), if any. Listed by `sort_order`.
pub struct BatchSummary { pub id: i64, pub name: String, pub title: Option<String>, pub sort_order: i64, pub source: String, pub created_at: String, pub count: i64, pub pending: i64 }

#[derive(Serialize)]
pub struct Batch { pub summary: BatchSummary, pub rows: Vec<ImportRow> }

pub fn batches(conn: &Connection) -> Result<Vec<BatchSummary>, String> {
    conn.prepare("SELECT b.id, b.name, b.title, b.sort_order, b.source, b.created_at, count(r.id), coalesce(sum(r.state IN ('pending', 'ready')), 0) FROM fin_imports b LEFT JOIN fin_import_rows r ON r.batch_id = b.id GROUP BY b.id ORDER BY b.sort_order, b.id DESC").map_err(err)?
        .query_map([], |r| Ok(BatchSummary { id: r.get(0)?, name: r.get(1)?, title: r.get(2)?, sort_order: r.get(3)?, source: r.get(4)?, created_at: r.get(5)?, count: r.get(6)?, pending: r.get(7)? })).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

pub fn rename_batch(conn: &Connection, id: i64, title: &str) -> Result<(), String> {
    let name: String = conn.query_row("SELECT name FROM fin_imports WHERE id = ?1", [id], |r| r.get(0)).map_err(|_| "这份账单已经删了".to_string())?;
    let title = title.trim();
    common::check_len(&Some(title.to_string()), "名称", 100)?;
    conn.execute("UPDATE fin_imports SET title = ?1 WHERE id = ?2", params![(!title.is_empty() && title != name).then_some(title), id]).map_err(err)?;
    Ok(())
}

/// The batch and its rows go; entries it posted stay in the ledger (they just lose this source). Rows of other
/// batches that merged into one of its rows before either was posted come back 待核对.
pub fn delete_batch(conn: &Connection, id: i64) -> Result<(), String> {
    let tx = common::Tx::begin(conn)?;
    let rows: Vec<i64> = conn.prepare("SELECT id FROM fin_import_rows WHERE batch_id = ?1").map_err(err)?.query_map([id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for row in &rows { release_dependents(conn, *row)?; }
    conn.execute("UPDATE fin_import_rows SET ai_json = json_remove(ai_json, '$.duplicateRow') WHERE batch_id != ?1 AND json_extract(ai_json, '$.duplicateRow') IN (SELECT id FROM fin_import_rows WHERE batch_id = ?1)", [id]).map_err(err)?;
    if conn.execute("DELETE FROM fin_imports WHERE id = ?1", [id]).map_err(err)? == 0 { return Err("这份账单已经删了".into()); }
    tx.commit().map_err(err)
}

fn read_row(r: &Row) -> rusqlite::Result<ImportRow> {
    let source: String = r.get(1)?;
    let draft: String = r.get(2)?;
    Ok(ImportRow { ai: None, file_name: None, id: r.get(0)?, source: serde_json::from_str(&source).map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e)))?, draft: serde_json::from_str(&draft).map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e)))?, state: r.get(3)?, entry_id: r.get(4)?, resolution: r.get(5)?, pending_candidates: vec![], candidates: vec![] })
}

/// The same source's own ids tell transactions apart.
fn other_trade(source: &str, id: &str, other_source: &str, other_id: &str) -> bool {
    source == other_source && !id.is_empty() && id != "/" && !other_id.is_empty() && other_id != "/" && id != other_id
}

/// Broad, explicit candidates only: equal account movement, same kind, within seven days.
fn candidates(conn: &Connection, source: &str, row: &ImportRow) -> Result<Vec<MatchCandidate>, String> {
    let d = &row.draft;
    if d.treatment == "neutral" || d.amount.is_none() || check_day(&d.day, "日期").is_err() { return Ok(vec![]); }
    let mut result = vec![];
    let list = conn.prepare(&format!("SELECT {ENTRY_COLS} FROM fin_entries WHERE amount = ?1 AND currency = ?2 AND flow = ?3 AND is_refund = ?4 AND end_day IS NULL AND abs(julianday(day) - julianday(?5)) <= 7 ORDER BY abs(julianday(day) - julianday(?5)), id DESC")).map_err(err)?
        .query_map(params![d.amount, d.currency, if d.treatment == "in" { "in" } else { "out" }, d.treatment == "refund", d.day], entry_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    for e in list {
        let sources = entry_sources(conn, e.id)?;
        if !sources.is_empty() && sources.iter().all(|s| other_trade(source, &row.source.trade_id, &s.0, &s.2)) { continue; }
        result.push(MatchCandidate { entry: e, sources: sources.into_iter().map(|s| s.1).collect() });
    }
    Ok(result)
}

/// (source, file, trade id) of the statement rows behind an entry.
fn entry_sources(conn: &Connection, id: i64) -> Result<Vec<(String, String, String)>, String> {
    conn.prepare("SELECT b.source, b.name, json_extract(r.source_json, '$.tradeId') FROM fin_import_rows r JOIN fin_imports b ON b.id = r.batch_id WHERE r.entry_id = ?1").map_err(err)?.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

/// The statement rows behind an entry, with their source.
pub(crate) fn entry_rows(conn: &Connection, id: i64) -> Result<Vec<(String, SourceRow)>, String> {
    let rows: Vec<(String, String)> = conn.prepare("SELECT b.source, r.source_json FROM fin_import_rows r JOIN fin_imports b ON b.id = r.batch_id WHERE r.entry_id = ?1").map_err(err)?.query_map([id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    rows.into_iter().map(|(source, row)| Ok((source, serde_json::from_str(&row).map_err(err)?))).collect()
}

/// The file names behind an entry (what Importer tells the model about a ledger row).
pub(crate) fn entry_files(conn: &Connection, id: i64) -> Result<Vec<String>, String> {
    Ok(entry_sources(conn, id)?.into_iter().map(|s| s.1).collect())
}

/// Same amount within 7 days in earlier batches' open rows. Rows of the same batch are different rows by construction.
fn pending_candidates(conn: &Connection, source: &str, batch_id: i64, row: &ImportRow) -> Result<Vec<PendingCandidate>, String> {
    let d = &row.draft;
    if d.amount.is_none() || d.treatment == "neutral" || check_day(&d.day, "日期").is_err() { return Ok(vec![]); }
    let mut stmt = conn.prepare("SELECT r.id, r.source_json, r.draft_json, r.state, r.entry_id, r.resolution, b.id, b.name, b.source FROM fin_import_rows r JOIN fin_imports b ON b.id = r.batch_id WHERE r.batch_id < ?1 AND r.state IN ('pending', 'ready') AND json_extract(r.draft_json, '$.amount') = ?2 AND json_extract(r.draft_json, '$.currency') = ?3 AND json_extract(r.draft_json, '$.treatment') = ?4 AND abs(julianday(json_extract(r.draft_json, '$.day')) - julianday(?5)) <= 7 ORDER BY r.id").map_err(err)?;
    let rows = stmt.query_map(params![batch_id, d.amount, d.currency, d.treatment, d.day], |r| Ok((read_row(r)?, r.get::<_, i64>(6)?, r.get::<_, String>(7)?, r.get::<_, String>(8)?))).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    Ok(rows.into_iter().filter(|(other, _, _, s)| !other_trade(source, &row.source.trade_id, s, &other.source.trade_id)).map(|(other, batch_id, name, _)| PendingCandidate { id: other.id, batch_id, name, draft: other.draft, source: other.source }).collect())
}

pub fn batch(conn: &Connection, id: i64) -> Result<Batch, String> {
    let summary = batches(conn)?.into_iter().find(|b| b.id == id).ok_or("导入记录不存在")?;
    let mut rows = conn.prepare("SELECT id, source_json, draft_json, state, entry_id, resolution FROM fin_import_rows WHERE batch_id = ?1 ORDER BY id").map_err(err)?.query_map([id], read_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    for row in &mut rows {
        row.ai = read_record(conn, row.id)?;
        if matches!(row.state.as_str(), "pending" | "ready") {
            row.candidates = candidates(conn, &summary.source, row)?;
            row.pending_candidates = pending_candidates(conn, &summary.source, id, row)?;
            offer_target(conn, row)?;
        }
    }
    Ok(Batch { summary, rows })
}

/// The row the model means by 拿不准, whatever its amount, goes with the 候选.
fn offer_target(conn: &Connection, row: &mut ImportRow) -> Result<(), String> {
    let Some(key) = row.ai.as_ref().and_then(|a| a.target.clone()) else { return Ok(()) };
    let Some((kind, target)) = key.split_once(':').and_then(|(k, t)| Some((k.to_string(), t.parse::<i64>().ok()?))) else { return Ok(()) };
    if kind == "e" && !row.candidates.iter().any(|c| c.entry.id == target) {
        if let Ok(e) = entry(conn, target) { row.candidates.insert(0, MatchCandidate { sources: entry_files(conn, e.id)?, entry: e }); }
    }
    if kind == "r" && !row.pending_candidates.iter().any(|c| c.id == target) {
        let other = conn.query_row("SELECT r.id, r.source_json, r.draft_json, r.state, r.entry_id, r.resolution, b.id, b.name FROM fin_import_rows r JOIN fin_imports b ON b.id = r.batch_id WHERE r.id = ?1 AND r.state IN ('pending', 'ready')", [target], |r| Ok((read_row(r)?, r.get::<_, i64>(6)?, r.get::<_, String>(7)?))).optional().map_err(err)?;
        if let Some((o, batch_id, name)) = other { row.pending_candidates.insert(0, PendingCandidate { id: o.id, batch_id, name, draft: o.draft, source: o.source }); }
    }
    Ok(())
}

pub fn ingest(conn: &Connection, name: &str, parsed: Parsed, model: Option<&str>) -> Result<Batch, String> {
    let tx = common::Tx::begin(conn)?;
    conn.execute("INSERT INTO fin_imports (name, source, sort_order) VALUES (?1, ?2, (SELECT coalesce(min(sort_order), 1) - 1 FROM fin_imports))", params![name, parsed.source]).map_err(err)?;
    let id = conn.last_insert_rowid();
    let (outgoing, incoming): (HashSet<String>, HashSet<String>) = (kinds::name_map(conn, Flow::Out.kinds_module())?.into_values().collect(), kinds::name_map(conn, Flow::In.kinds_module())?.into_values().collect());
    let record = model.map(|m| serde_json::to_string(&Record { model: m.into(), ..Default::default() })).transpose().map_err(err)?;
    for row in parsed.rows {
        // The source keeps what was read; the draft only a 分类 I have (the model's may not exist here).
        let mut draft = row.draft.clone();
        if draft.treatment == "neutral" || draft.category.as_ref().is_some_and(|c| !(if draft.treatment == "in" { &incoming } else { &outgoing }).contains(c)) { draft.category = None; }
        conn.execute("INSERT INTO fin_import_rows (batch_id, source_json, draft_json, ai_json) VALUES (?1, ?2, ?3, ?4)", params![id, serde_json::to_string(&row).map_err(err)?, serde_json::to_string(&draft).map_err(err)?, record]).map_err(err)?;
    }
    tx.commit().map_err(err)?;
    batch(conn, id)
}

fn check_draft(conn: &Connection, d: &Draft) -> Result<(), String> {
    check_day(&d.day, "交易日期")?;
    check_amount(d.amount.ok_or("请核对金额")?, "金额")?;
    if !["out", "in", "refund", "neutral"].contains(&d.treatment.as_str()) { return Err("请选择交易类型".into()); }
    common::check_len(&Some(d.note.clone()), "备注", 1000)?;
    if d.treatment != "neutral" {
        let flow = if d.treatment == "in" { Flow::In } else { Flow::Out };
        if let Some(name) = &d.category { if !kinds::name_map(conn, flow.kinds_module())?.values().any(|n| n == name) { return Err("分类已变化，请重新选择".into()); } }
    }
    if d.treatment != "refund" && d.refund_of.is_some() { return Err("只有退款可以关联原支出".into()); }
    Ok(())
}

pub fn save_row(conn: &Connection, id: i64, draft: Draft) -> Result<(), String> {
    let tx = common::Tx::begin(conn)?;
    check_draft(conn, &draft)?;
    release_dependents(conn, id)?;
    conn.execute("UPDATE fin_import_rows SET ai_json = NULL WHERE id = ?1", [id]).map_err(err)?;
    let changed = conn.execute("UPDATE fin_import_rows SET draft_json = ?1, state = 'ready', resolution = NULL WHERE id = ?2 AND state IN ('pending', 'ready')", params![serde_json::to_string(&draft).map_err(err)?, id]).map_err(err)?;
    if changed == 0 { return Err("该明细已处理，请在收支记录中编辑，或撤销上一次确认".into()); }
    tx.commit().map_err(err)?;
    Ok(())
}

/// 多选 · 改分类: open rows only; 不计收支 and a refund linked to its expense keep theirs. Marked so 再整理 leaves it.
pub fn set_category(conn: &Connection, ids: &[i64], category: Option<&str>) -> Result<usize, String> {
    let tx = common::Tx::begin(conn)?;
    let mut changed = 0;
    for id in ids {
        let row = conn.query_row("SELECT id, source_json, draft_json, state, entry_id, resolution FROM fin_import_rows WHERE id = ?1", [id], read_row).map_err(|_| "明细不存在".to_string())?;
        if !matches!(row.state.as_str(), "pending" | "ready") || row.draft.treatment == "neutral" || row.draft.refund_of.is_some() { continue; }
        let flow = if row.draft.treatment == "in" { Flow::In } else { Flow::Out };
        if let Some(name) = category { if !kinds::name_map(conn, flow.kinds_module())?.values().any(|n| n == name) { return Err(format!("「{name}」不是{}的分类", if flow == Flow::In { "收入" } else { "支出" })); } }
        let mut draft = row.draft;
        draft.category = category.map(str::to_string);
        conn.execute("UPDATE fin_import_rows SET draft_json = ?1 WHERE id = ?2", params![serde_json::to_string(&draft).map_err(err)?, id]).map_err(err)?;
        let mut record = read_record(conn, *id)?.unwrap_or_default();
        record.my_category = true;
        write_record(conn, *id, &record)?;
        changed += 1;
    }
    tx.commit().map_err(err)?;
    Ok(changed)
}

pub fn resolve_row(conn: &Connection, id: i64, target: Option<i64>) -> Result<(), String> {
    let batch_id: i64 = conn.query_row("SELECT batch_id FROM fin_import_rows WHERE id = ?1", [id], |r| r.get(0)).map_err(err)?;
    let b = batch(conn, batch_id)?;
    let row = b.rows.iter().find(|r| r.id == id).ok_or("明细不存在")?;
    if !matches!(row.state.as_str(), "pending" | "ready") { return Err("这笔来源已经处理".into()); }
    if let Some(target) = target {
        check_draft(conn, &row.draft)?;
        if !row.candidates.iter().any(|c| c.entry.id == target) { return Err("候选记录已变化，请重新核对".into()); }
        link(conn, id, &format!("e:{target}"), &mut Record::default())?;
    } else {
        conn.execute("UPDATE fin_import_rows SET resolution = 'new' WHERE id = ?1", [id]).map_err(err)?;
    }
    Ok(())
}

pub fn unlink_row(conn: &Connection, id: i64) -> Result<(), String> {
    let tx = common::Tx::begin(conn)?;
    conn.execute("UPDATE fin_import_rows SET ai_json = NULL WHERE id = ?1", [id]).map_err(err)?;
    let n = conn.execute("UPDATE fin_import_rows SET state = 'pending', entry_id = NULL, resolution = 'new' WHERE id = ?1 AND state = 'duplicate' AND resolution = 'linked'", [id]).map_err(err)?;
    if n == 0 { return Err("这笔来源没有人工关联，自动去重可撤销当次确认".into()); }
    tx.commit().map_err(err)?;
    Ok(())
}

pub(crate) fn read_record(conn: &Connection, id: i64) -> Result<Option<Record>, String> {
    let text: Option<String> = conn.query_row("SELECT ai_json FROM fin_import_rows WHERE id = ?1", [id], |r| r.get(0)).map_err(err)?;
    text.map(|s| serde_json::from_str(&s).map_err(err)).transpose()
}

pub(crate) fn write_record(conn: &Connection, id: i64, record: &Record) -> Result<(), String> {
    conn.execute("UPDATE fin_import_rows SET ai_json = ?1 WHERE id = ?2", params![serde_json::to_string(record).map_err(err)?, id]).map_err(err)?;
    Ok(())
}

fn release_dependents(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("UPDATE fin_import_rows SET state = 'pending', entry_id = NULL, resolution = NULL, ai_json = NULL WHERE state = 'duplicate' AND entry_id IS NULL AND json_extract(ai_json, '$.duplicateRow') = ?1", [id]).map_err(err)?;
    Ok(())
}

/// Merge row `id` into `key`: e:<entry> (the ledger) or r:<row> (a row not in yet, followed to the one it merged into).
pub(crate) fn link(conn: &Connection, id: i64, key: &str, record: &mut Record) -> Result<(), String> {
    let (kind, target) = key.split_once(':').ok_or("重复目标无效")?;
    let target: i64 = target.parse().map_err(err)?;
    let (entry_id, anchor) = if kind == "e" {
        entry(conn, target)?;
        (Some(target), None)
    } else {
        let mut anchor = target;
        while let Some(next) = read_record(conn, anchor)?.and_then(|a| a.duplicate_row) {
            if next == anchor || next == id { return Err("重复来源关联无效".into()); }
            anchor = next;
        }
        let (state, entry_id): (String, Option<i64>) = conn.query_row("SELECT state, entry_id FROM fin_import_rows WHERE id = ?1", [anchor], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
        if !["pending", "ready", "posted", "duplicate"].contains(&state.as_str()) { return Err("重复来源已经变化".into()); }
        (entry_id, Some(anchor))
    };
    conn.execute("UPDATE fin_import_rows SET state = 'duplicate', entry_id = ?1, resolution = 'linked' WHERE id = ?2", params![entry_id, id]).map_err(err)?;
    // rows that had merged into this one come along
    conn.execute("UPDATE fin_import_rows SET entry_id = ?1 WHERE state = 'duplicate' AND json_extract(ai_json, '$.duplicateRow') = ?2", params![entry_id, id]).map_err(err)?;
    if let Some(a) = anchor { conn.execute("UPDATE fin_import_rows SET ai_json = json_set(ai_json, '$.duplicateRow', ?1) WHERE state = 'duplicate' AND json_extract(ai_json, '$.duplicateRow') = ?2", params![a, id]).map_err(err)?; }
    record.duplicate_row = anchor;
    Ok(())
}

#[derive(Serialize)]
pub struct CommitResult { pub posted: usize, pub neutral: usize }

pub fn commit(conn: &Connection, id: i64, ids: &[i64]) -> Result<CommitResult, String> {
    if ids.is_empty() { return Err("请选择要确认的明细".into()); }
    let tx = common::Tx::begin(conn)?;
    let b = batch(conn, id)?;
    let mut result = CommitResult { posted: 0, neutral: 0 };
    let unique: HashSet<_> = ids.iter().copied().collect();
    if unique.len() != ids.len() || ids.iter().any(|id| !b.rows.iter().any(|r| r.id == *id)) { return Err("选择的明细不正确".into()); }
    for row in b.rows.iter().filter(|r| unique.contains(&r.id)) {
        if !matches!(row.state.as_str(), "pending" | "ready") { continue; }
        let d = &row.draft;
        check_draft(conn, d).map_err(|e| format!("第 {} 行：{e}", row.source.row_number))?;
        let (state, entry_id) = if d.treatment == "neutral" {
            result.neutral += 1;
            ("neutral", None)
        } else {
            if row.resolution.as_deref() != Some("new") {
                if !row.pending_candidates.is_empty() { return Err(format!("第 {} 行与待入账来源可能重复，请先复核", row.source.row_number)); }
                if row.ai.as_ref().is_some_and(|a| ["review", "error"].contains(&a.duplicate.as_str())) { return Err(format!("第 {} 行的重复交易尚未确定，请复核后保留为新交易或关联", row.source.row_number)); }
                if !row.candidates.is_empty() { return Err(format!("第 {} 行可能已记过，请先选择关联已有记录或保留为新交易", row.source.row_number)); }
            }
            let flow = if d.treatment == "in" { Flow::In } else { Flow::Out };
            let names = kinds::name_map(conn, flow.kinds_module())?;
            let kind_id = d.category.as_ref().and_then(|name| names.iter().find(|(_, n)| *n == name).map(|(id, _)| *id));
            let entry = create_entry(conn, EntryPatch { flow: Some(flow), amount: d.amount, currency: Some(d.currency), day: Some(d.day.clone()), end_day: Some(None), kind_id: Some(kind_id), note: Some(Some(d.note.clone())), is_refund: Some(d.treatment == "refund"), refund_of: Some(d.refund_of) })?;
            result.posted += 1;
            ("posted", Some(entry.id))
        };
        conn.execute("UPDATE fin_import_rows SET state = ?1, entry_id = ?2 WHERE id = ?3", params![state, entry_id, row.id]).map_err(err)?;
        conn.execute("UPDATE fin_import_rows SET entry_id = ?1 WHERE state = 'duplicate' AND json_extract(ai_json, '$.duplicateRow') = ?2", params![entry_id, row.id]).map_err(err)?;
    }
    tx.commit().map_err(err)?;
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RowRecord { #[serde(default, skip_serializing_if = "Option::is_none")] pub ai: Option<Record>, pub source: SourceRow, pub draft: Draft, pub state: String, pub entry: Option<usize>, #[serde(default, skip_serializing_if = "Option::is_none")] pub resolution: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchRecord {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Format 34 on; older files list newest first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i64>,
    pub source: String,
    pub created_at: String,
    pub rows: Vec<RowRecord>,
}

pub fn export_records(conn: &Connection, ids: &[i64]) -> Result<Vec<BatchRecord>, String> {
    let row_ids: Vec<i64> = conn.prepare("SELECT id FROM fin_import_rows ORDER BY batch_id, id").map_err(err)?.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_,_>>().map_err(err)?;
    let mut result = vec![];
    let mut list = batches(conn)?;
    list.sort_by_key(|b| b.id); // rows are numbered in this order (row_ids)
    for b in list {
        let rows = batch(conn, b.id)?.rows.into_iter().map(|r| {
            let mut draft = r.draft;
            draft.refund_of = draft.refund_of.and_then(|id| ids.iter().position(|n| *n == id).map(|i| (i + 1) as i64));
            let mut ai = r.ai;
            if let Some(a) = &mut ai {
                a.duplicate_row = a.duplicate_row.and_then(|id| row_ids.iter().position(|n| *n == id).map(|i| (i + 1) as i64));
                a.target = None;
            }
            RowRecord { ai, source: r.source, draft, state: r.state, resolution: r.resolution, entry: r.entry_id.and_then(|id| ids.iter().position(|n| *n == id).map(|i| i + 1)) }
        }).collect();
        result.push(BatchRecord { name: b.name, title: b.title, sort_order: Some(b.sort_order), source: b.source, created_at: b.created_at, rows });
    }
    Ok(result)
}

pub fn validate_records(records: &[BatchRecord], count: usize) -> Result<(), String> {
    let all:Vec<&RowRecord>=records.iter().flat_map(|b| &b.rows).collect();
    let mut position = 0;
    for b in records {
        if !["alipay", "wechat", "dbs", "wise", "icbc", "screenshot"].contains(&b.source.as_str()) { return Err("账单来源不正确".into()); }
        if b.title.as_ref().is_some_and(|t| t.trim().is_empty() || t.chars().count() > 100) { return Err("账单名称不正确".into()); }
        for r in &b.rows {
            position += 1;
            if r.ai.as_ref().and_then(|a| a.duplicate_row).is_some_and(|n| n <= 0 || n as usize > all.len() || n == position || r.state != "duplicate") { return Err("AI 来源关联无效".into()); }
            let mut seen=HashSet::new(); let mut next=r.ai.as_ref().and_then(|a|a.duplicate_row);
            while let Some(n)=next {if n<=0 || n as usize>all.len() || !seen.insert(n) {return Err("AI 来源关联循环或失效".into())} next=all[n as usize-1].ai.as_ref().and_then(|a|a.duplicate_row);}
            if r.resolution.as_deref().is_some_and(|s| !["new", "linked"].contains(&s)) || (r.resolution.as_deref() == Some("linked") && r.state != "duplicate") { return Err("账单去重决定不正确".into()); }
            if !["pending", "ready", "posted", "neutral", "duplicate", "ignored"].contains(&r.state.as_str()) { return Err("账单核对状态不正确".into()); }
            if r.entry.is_some_and(|n| n == 0 || n > count) || r.draft.refund_of.is_some_and(|n| n <= 0 || n as usize > count) { return Err("账单关联记录不存在".into()); }
        }
    }
    Ok(())
}

pub fn replace_records(conn: &Connection, records: &[BatchRecord], ids: &[i64]) -> Result<(), String> {
    let mut row_ids = vec![];
    let mut links = vec![];
    for (i, b) in records.iter().enumerate() {
        conn.execute("INSERT INTO fin_imports (name, title, sort_order, source, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![b.name, b.title, b.sort_order.unwrap_or((records.len() - i) as i64), b.source, b.created_at]).map_err(err)?;
        let batch_id = conn.last_insert_rowid();
        for r in &b.rows {
            let mut draft = r.draft.clone();
            draft.refund_of = draft.refund_of.map(|n| ids[n as usize - 1]);
            let mut ai = r.ai.clone();
            if let Some(a) = &mut ai { if let Some(n)=a.duplicate_row.take(){links.push((row_ids.len(),n as usize-1));} }
            conn.execute("INSERT INTO fin_import_rows (batch_id, source_json, draft_json, state, entry_id, resolution, ai_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![batch_id, serde_json::to_string(&r.source).map_err(err)?, serde_json::to_string(&draft).map_err(err)?, r.state, r.entry.map(|n| ids[n - 1]), r.resolution, ai.map(|a| serde_json::to_string(&a)).transpose().map_err(err)?]).map_err(err)?;
            row_ids.push(conn.last_insert_rowid());
        }
    }
    for (at,target) in links { conn.execute("UPDATE fin_import_rows SET ai_json=json_set(ai_json, '$.duplicateRow',?1) WHERE id=?2",params![row_ids[target],row_ids[at]]).map_err(err)?; }
    Ok(())
}

#[tauri::command]
pub fn fin_imports(state: State<AppState>) -> Result<Vec<BatchSummary>, String> { state.with_db(|c| batches(c)) }
#[tauri::command]
pub async fn fin_import_batch(state: State<'_, AppState>, id: i64) -> Result<Batch, String> { state.with_db(|c| batch(c, id)) }
#[tauri::command]
pub fn fin_import_save(state: State<AppState>, id: i64, draft: Draft) -> Result<(), String> { state.with_undo("核对账单明细", |c| save_row(c, id, draft)) }
#[tauri::command]
pub fn fin_import_set_category(state: State<AppState>, ids: Vec<i64>, category: Option<String>) -> Result<usize, String> { state.with_undo("批量改分类", |c| set_category(c, &ids, category.as_deref())) }
#[tauri::command]
pub async fn fin_import_commit(state: State<'_, AppState>, id: i64, ids: Vec<i64>) -> Result<CommitResult, String> { state.with_undo("确认账单", |c| commit(c, id, &ids)) }
#[tauri::command]
pub fn fin_entry_sources(state: State<AppState>, id: i64) -> Result<Vec<ImportRow>, String> {
    state.with_db(|c| c.prepare("SELECT r.id, r.source_json, r.draft_json, r.state, r.entry_id, r.resolution, b.name FROM fin_import_rows r JOIN fin_imports b ON b.id = r.batch_id WHERE r.entry_id = ?1 ORDER BY r.id").map_err(err)?.query_map([id], |r| { let mut row = read_row(r)?; row.file_name = Some(r.get(6)?); Ok(row) }).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err))
}
#[tauri::command]
pub fn fin_refund_candidates(state: State<AppState>, currency: Currency, day: String) -> Result<Vec<Entry>, String> {
    state.with_db(|c| Ok(entries_between(c, "1900-01-01", &day)?.into_iter().filter(|e| e.flow == Flow::Out && !e.is_refund && e.end_day.is_none() && e.currency == currency).collect()))
}

#[tauri::command]
pub async fn fin_import_resolve(state: State<'_, AppState>, id: i64, target: Option<i64>) -> Result<(), String> { state.with_undo("核对重复交易", |c| resolve_row(c, id, target)) }
#[tauri::command]
pub fn fin_import_unlink(state: State<AppState>, id: i64) -> Result<(), String> { state.with_undo("解除账单关联", |c| unlink_row(c, id)) }

#[tauri::command]
pub fn fin_import_link_pending(state: State<AppState>, id: i64, target: i64) -> Result<(), String> {
    state.with_undo("关联待入账来源", |c| {
        let batch_id: i64 = c.query_row("SELECT batch_id FROM fin_import_rows WHERE id = ?1", [id], |r| r.get(0)).map_err(err)?;
        let b = batch(c, batch_id)?;
        let row = b.rows.iter().find(|r| r.id == id).ok_or("来源不存在")?;
        if !row.pending_candidates.iter().any(|r| r.id == target) { return Err("候选来源已变化，请重新核对".into()); }
        check_draft(c, &row.draft)?;
        let mut record = row.ai.clone().unwrap_or_default();
        record.duplicate = "duplicate".into();
        record.reason = "人工确认同一交易".into();
        record.target = None;
        link(c, id, &format!("r:{target}"), &mut record)?;
        write_record(c, id, &record)
    })
}

/// Read once for the transient preview. No original path or bytes enter the database/export.
pub(crate) fn read_image(path: &str) -> Result<(Vec<u8>, String), String> {
    let ext = Path::new(path).extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let media = match ext.as_str() { "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", _ => return Err("请选择 PNG 或 JPEG 截图".into()) };
    let bytes = std::fs::read(path).map_err(|_| "无法读取截图".to_string())?;
    let preview = format!("data:{media};base64,{}", crate::youzi::office::base64(&bytes));
    Ok((bytes, preview))
}

#[tauri::command]
pub async fn fin_import_preview(state: State<'_, AppState>, id: i64, path: String) -> Result<String, String> {
    let name: String = state.with_db(|c| c.query_row("SELECT name FROM fin_imports WHERE id = ?1", [id], |r| r.get(0)).map_err(err))?;
    if Path::new(&path).file_name().and_then(|s| s.to_str()) != Some(name.as_str()) { return Err(format!("请选择原文件「{name}」")); }
    tauri::async_runtime::spawn_blocking(move || read_image(&path).map(|(_, preview)| preview)).await.map_err(err)?
}

pub fn add_row(conn: &Connection, id: i64) -> Result<i64, String> {
    let b = batch(conn, id)?;
    let currency = b.rows.first().map_or(Currency::Cny, |r| r.draft.currency);
    let draft = Draft { day: String::new(), amount: None, currency, treatment: "out".into(), category: None, note: String::new(), refund_of: None };
    let source = SourceRow { sheet: "手动补录".into(), row_number: b.rows.iter().map(|r| r.source.row_number).max().unwrap_or(0) + 1, raw: vec![], trade_id: String::new(), time: String::new(), status: String::new(), draft: draft.clone(), issues: vec!["对照原件补录，请填写日期与金额".into()], region: None };
    conn.execute("INSERT INTO fin_import_rows (batch_id, source_json, draft_json) VALUES (?1, ?2, ?3)", params![id, serde_json::to_string(&source).map_err(err)?, serde_json::to_string(&draft).map_err(err)?]).map_err(err)?;
    Ok(conn.last_insert_rowid())
}

pub fn ignore_row(conn: &Connection, id: i64, ignored: bool) -> Result<(), String> {
    release_dependents(conn, id)?;
    let n = if ignored {
        conn.execute("UPDATE fin_import_rows SET state = 'ignored', resolution = NULL, ai_json = NULL WHERE id = ?1 AND state IN ('pending', 'ready')", [id])
    } else { conn.execute("UPDATE fin_import_rows SET state = 'pending', ai_json = NULL WHERE id = ?1 AND state = 'ignored'", [id]) }.map_err(err)?;
    if n == 0 { return Err("这笔明细已处理，不能忽略；请先撤销或解除关联".into()); }
    Ok(())
}

#[tauri::command]
pub fn fin_import_add(state: State<AppState>, id: i64) -> Result<i64, String> { state.with_undo("补录账单明细", |c| add_row(c, id)) }
#[tauri::command]
pub fn fin_import_ignore(state: State<AppState>, id: i64, ignored: bool) -> Result<(), String> { state.with_undo(if ignored { "忽略账单误识别" } else { "恢复账单明细" }, |c| ignore_row(c, id, ignored)) }
#[tauri::command]
pub fn fin_import_rename(state: State<AppState>, id: i64, title: String) -> Result<(), String> { state.with_undo("账单改名", |c| rename_batch(c, id, &title)) }
#[tauri::command]
pub fn fin_import_delete(state: State<AppState>, id: i64) -> Result<(), String> { state.with_undo("删除账单", |c| delete_batch(c, id)) }
#[tauri::command]
pub fn fin_imports_reorder(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> { state.with_undo("账单排序", |c| common::reorder(c, "fin_imports", "1 = 1", &[], &ids)) }

#[cfg(test)]
pub(crate) mod tests;
