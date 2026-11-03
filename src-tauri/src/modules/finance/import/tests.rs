use super::*;
use crate::db::tests::memory_db;
use serde_json::json;

pub(crate) fn row(trade: &str, day: &str, amount: i64, treatment: &str) -> SourceRow {
    SourceRow { sheet: "账单".into(), row_number: 2, raw: vec![("交易单号".into(), trade.into())], trade_id: trade.into(), time: format!("{day} 12:00:00"), status: String::new(), draft: Draft { day: day.into(), amount: Some(amount), currency: Currency::Cny, treatment: treatment.into(), category: if treatment == "out" { Some("餐饮".into()) } else { None }, note: "测试交易".into(), refund_of: None }, issues: vec![], region: None }
}

fn parsed(source: &str, rows: Vec<SourceRow>) -> Parsed {
    Parsed { source: source.into(), rows }
}

#[test]
fn import_waits_for_me_and_keeps_neutral_and_sources() {
    let c = memory_db();
    let b = ingest(&c, "账单.xlsx", parsed("alipay", vec![row("a", "2026-09-30", 1000, "out"), row("b", "2026-10-01", 500, "neutral")]), None).unwrap();
    assert!(entries(&c, "2026-09").unwrap().is_empty());
    let result = commit(&c, b.summary.id, &b.rows.iter().map(|r| r.id).collect::<Vec<_>>()).unwrap();
    assert_eq!((result.posted, result.neutral), (1, 1));
    assert_eq!(entries(&c, "2026-09").unwrap().len(), 1);
    assert_eq!(c.query_row::<i64, _, _>("SELECT count(*) FROM fin_balances", [], |r| r.get(0)).unwrap(), 0);
    // the same file again is a new batch, and its rows are held back until decided
    let again = ingest(&c, "账单.xlsx", parsed("alipay", vec![row("a", "2026-09-30", 1000, "out")]), Some("模型")).unwrap();
    assert_eq!(again.rows[0].candidates.len(), 1);
    assert_eq!(again.rows[0].ai.as_ref().unwrap().model, "模型");
    assert!(commit(&c, again.summary.id, &[again.rows[0].id]).is_err());
}

#[test]
fn same_amount_trades_of_a_file_are_different_rows() {
    let c = memory_db();
    let b = ingest(&c, "1.xlsx", parsed("wechat", vec![row("", "2026-10-01", 1000, "out"), row("", "2026-10-01", 1000, "out")]), None).unwrap();
    assert!(b.rows.iter().all(|r| r.candidates.is_empty() && r.pending_candidates.is_empty()));
    commit(&c, b.summary.id, &b.rows.iter().map(|r| r.id).collect::<Vec<_>>()).unwrap();
    assert_eq!(entries(&c, "2026-10").unwrap().len(), 2);
}

#[test]
fn refund_changes_arrival_month_and_follows_original_category() {
    let c = memory_db();
    let original = create_entry(&c, serde_json::from_value(json!({"amount":1000,"currency":"CNY","day":"2026-09-30","kindId":kinds::tests::kind_id(&c, OUT_KINDS, "餐饮")})).unwrap()).unwrap();
    let refund = create_entry(&c, serde_json::from_value(json!({"amount":400,"currency":"CNY","day":"2026-10-02","isRefund":true,"refundOf":original.id})).unwrap()).unwrap();
    let r = range(&c, "2026-10-01", "2026-10-31", None, None, None).unwrap();
    assert_eq!(r.totals.iter().map(|t| t.amount).sum::<i64>(), -400);
    assert_eq!(totals(&c).unwrap().iter().find(|t| t.month == "2026-09").unwrap().amount, 1000);
    assert!(create_entry(&c, serde_json::from_value(json!({"amount":700,"currency":"CNY","day":"2026-10-02","isRefund":true,"refundOf":original.id})).unwrap()).is_err());
    assert!(update_entry(&c, original.id, serde_json::from_value(json!({"currency":"SGD"})).unwrap()).is_err());
    let shopping = kinds::tests::kind_id(&c, OUT_KINDS, "购物");
    update_entry(&c, original.id, serde_json::from_value(json!({"kindId": shopping})).unwrap()).unwrap();
    assert_eq!(entry(&c, refund.id).unwrap().kind_id, Some(shopping));
}

#[test]
fn export_round_trip_preserves_pending_sources_links_and_refunds() {
    let c = memory_db();
    c.execute("INSERT INTO fin_entries (id, flow, amount, currency, day) VALUES (50, 'out', 1, 'CNY', '2026-01-01')", []).unwrap();
    let b = ingest(&c, "账单.xlsx", parsed("alipay", vec![row("a", "2026-09-30", 1000, "out"), row("b", "2026-10-02", 200, "refund"), row("c", "2026-10-03", 10, "neutral")]), None).unwrap();
    commit(&c, b.summary.id, &[b.rows[0].id]).unwrap();
    let original = batch(&c, b.summary.id).unwrap().rows[0].entry_id.unwrap();
    let mut draft = b.rows[1].draft.clone(); draft.refund_of = Some(original);
    save_row(&c, b.rows[1].id, draft).unwrap();
    commit(&c, b.summary.id, &[b.rows[1].id]).unwrap();
    let a = super::super::export(&c).unwrap();
    let target = memory_db();
    super::super::replace_all(&target, &a).unwrap();
    assert_ne!(entries(&c, "2026-09").unwrap()[0].id, entries(&target, "2026-09").unwrap()[0].id);
    assert_eq!(serde_json::to_value(&a).unwrap(), serde_json::to_value(super::super::export(&target).unwrap()).unwrap());
}

#[test]
fn posting_and_row_state_share_one_undo_step() {
    let c = memory_db();
    let b = ingest(&c, "账单.xlsx", parsed("wechat", vec![row("a", "2026-10-01", 1000, "out")]), None).unwrap();
    let (_, changes) = crate::undo::record(&c, |c| commit(c, b.summary.id, &[b.rows[0].id])).unwrap();
    let mut inverse = vec![];
    rusqlite::session::invert_strm(&mut changes.unwrap().as_slice(), &mut inverse).unwrap();
    c.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    c.apply_strm(&mut inverse.as_slice(), None::<fn(&str)->bool>, |_, _| rusqlite::session::ConflictAction::SQLITE_CHANGESET_ABORT).unwrap();
    c.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    assert_eq!(entries(&c, "2026-10").unwrap().len(), 0);
    assert_eq!(batch(&c, b.summary.id).unwrap().rows[0].state, "pending");
}

#[test]
fn finance_upgrade_preserves_existing_entries_and_old_export() {
    let mut c = Connection::open_in_memory().unwrap();
    crate::db::configure(&c).unwrap();
    crate::db::migrate_with(&mut c, &crate::db::MIGRATIONS[..30], None).unwrap();
    c.execute("INSERT INTO fin_entries (flow, amount, currency, day) VALUES ('out', 1234, 'CNY', '2026-10-01')", []).unwrap();
    crate::db::migrate_with(&mut c, crate::db::MIGRATIONS, None).unwrap();
    let old = entries(&c, "2026-10").unwrap();
    assert_eq!((old[0].amount, old[0].is_refund, old[0].refund_of), (1234, false, None));
    let mut record = serde_json::to_value(super::super::export(&c).unwrap()).unwrap();
    record.as_object_mut().unwrap().remove("imports");
    for e in record["entries"].as_array_mut().unwrap() { e.as_object_mut().unwrap().remove("isRefund"); e.as_object_mut().unwrap().remove("refundOf"); }
    let record: FinanceRecord = serde_json::from_value(record).unwrap();
    super::super::replace_all(&c, &record).unwrap();
    assert_eq!(entries(&c, "2026-10").unwrap()[0].amount, 1234);
    let empty = memory_db();
    ingest(&empty, "待核对.xlsx", parsed("wechat", vec![row("pending", "2026-10-01", 100, "out")]), None).unwrap();
    assert_eq!(super::super::count(&empty).unwrap(), 1);
}

#[test]
fn duplicate_candidates_link_unlink_and_undo_keep_one_ledger_entry() {
    let c = memory_db();
    let a = ingest(&c, "微信.xlsx", parsed("wechat", vec![row("wx", "2026-10-01", 1000, "out")]), None).unwrap();
    commit(&c, a.summary.id, &[a.rows[0].id]).unwrap();
    let target = entries(&c, "2026-10").unwrap()[0].id;
    let b = ingest(&c, "DBS.pdf", parsed("dbs", vec![row("bank", "2026-10-03", 1000, "out")]), None).unwrap();
    let id = b.rows[0].id;
    assert_eq!(b.rows[0].candidates.len(), 1);
    assert!(commit(&c, b.summary.id, &[id]).err().unwrap().contains("可能已记过"));
    assert!(resolve_row(&c, id, Some(target + 100)).is_err());
    let (_, changes) = crate::undo::record(&c, |c| resolve_row(c, id, Some(target))).unwrap();
    assert_eq!(entries(&c, "2026-10").unwrap().len(), 1);
    assert_eq!(c.query_row::<i64,_,_>("SELECT count(*) FROM fin_import_rows WHERE entry_id = ?1", [target], |r| r.get(0)).unwrap(), 2);
    let exported = super::super::export(&c).unwrap();
    let other = memory_db();
    super::super::replace_all(&other, &exported).unwrap();
    assert_eq!(serde_json::to_value(exported).unwrap(), serde_json::to_value(super::super::export(&other).unwrap()).unwrap());
    let mut inverse = vec![];
    rusqlite::session::invert_strm(&mut changes.unwrap().as_slice(), &mut inverse).unwrap();
    c.apply_strm(&mut inverse.as_slice(), None::<fn(&str)->bool>, |_, _| rusqlite::session::ConflictAction::SQLITE_CHANGESET_ABORT).unwrap();
    assert_eq!(batch(&c, b.summary.id).unwrap().rows[0].state, "pending");
    resolve_row(&c, id, Some(target)).unwrap();
    unlink_row(&c, id).unwrap();
    assert_eq!(entries(&c, "2026-10").unwrap().len(), 1);
    assert_eq!(commit(&c, b.summary.id, &[id]).unwrap().posted, 1);
    assert_eq!(entries(&c, "2026-10").unwrap().len(), 2);
}

#[test]
fn candidates_respect_currency_refunds_ids_and_saved_edits() {
    let c = memory_db();
    let b = ingest(&c, "first.xlsx", parsed("wechat", vec![row("wx1", "2026-10-01", 1000, "out")]), None).unwrap();
    commit(&c, b.summary.id, &[b.rows[0].id]).unwrap();
    let b = ingest(&c, "different.xlsx", parsed("wechat", vec![row("wx2", "2026-10-01", 1000, "out")]), None).unwrap();
    assert!(b.rows[0].candidates.is_empty()); // the same source's other id
    let mut foreign = row("fx", "2026-10-01", 1000, "out"); foreign.draft.currency = Currency::Sgd;
    let b = ingest(&c, "bank.pdf", parsed("dbs", vec![foreign, row("refund", "2026-10-01", 1000, "refund"), row("far", "2026-10-20", 1000, "out"), row("close", "2026-10-02", 1000, "out")]), None).unwrap();
    assert!(b.rows[..3].iter().all(|r| r.candidates.is_empty()));
    let id = b.rows[3].id;
    assert_eq!(b.rows[3].candidates.len(), 1);
    resolve_row(&c, id, None).unwrap();
    save_row(&c, id, b.rows[3].draft.clone()).unwrap();
    assert!(batch(&c, b.summary.id).unwrap().rows[3].resolution.is_none());
    assert!(commit(&c, b.summary.id, &[b.rows[0].id, id]).is_err());
    assert_eq!(entries(&c, "2026-10").unwrap().len(), 1); // whole selected batch rolls back
    // an open row of an earlier batch counts too; a later one or one of its own batch doesn't
    let first = ingest(&c, "a.png", parsed("wise", vec![row("", "2026-09-12", 800, "out")]), None).unwrap();
    let next = ingest(&c, "b.png", parsed("wise", vec![row("", "2026-09-13", 800, "out")]), None).unwrap();
    assert_eq!(next.rows[0].pending_candidates.iter().map(|p| p.id).collect::<Vec<_>>(), [first.rows[0].id]);
    assert!(batch(&c, first.summary.id).unwrap().rows[0].pending_candidates.is_empty());
    assert!(commit(&c, next.summary.id, &[next.rows[0].id]).err().unwrap().contains("待入账"));
}

#[test]
fn set_category_marks_open_rows_only_and_is_undone_in_one_step() {
    let c = memory_db();
    let mut linked = row("r", "2026-10-02", 100, "refund"); linked.draft.refund_of = None;
    let b = ingest(&c, "账单.xlsx", parsed("alipay", vec![row("a", "2026-10-01", 100, "out"), row("b", "2026-10-01", 200, "neutral"), linked, row("c", "2026-10-03", 300, "out")]), Some("模型")).unwrap();
    commit(&c, b.summary.id, &[b.rows[3].id]).unwrap();
    let ids: Vec<i64> = b.rows.iter().map(|r| r.id).collect();
    let (n, changes) = crate::undo::record(&c, |c| set_category(c, &ids, Some("购物"))).unwrap();
    assert_eq!(n, 2); // not 不计收支, not the row that went in
    let rows = batch(&c, b.summary.id).unwrap().rows;
    assert_eq!(rows[0].draft.category.as_deref(), Some("购物"));
    assert_eq!(rows[2].draft.category.as_deref(), Some("购物"));
    assert_eq!(rows[1].draft.category, None);
    assert_eq!(rows[3].draft.category.as_deref(), Some("餐饮"));
    assert!(rows[0].ai.as_ref().unwrap().my_category);
    assert_eq!(rows[0].source.draft.category.as_deref(), Some("餐饮"));
    set_category(&c, &ids[..1], None).unwrap();
    assert_eq!(batch(&c, b.summary.id).unwrap().rows[0].draft.category, None);
    assert!(set_category(&c, &ids[..1], Some("工资")).is_err()); // 收入的分类
    let mut inverse = vec![];
    rusqlite::session::invert_strm(&mut changes.unwrap().as_slice(), &mut inverse).unwrap();
    assert!(!inverse.is_empty());
}

#[test]
fn add_ignore_restore_and_export() {
    let c = memory_db();
    let b = ingest(&c, "sample.png", parsed("icbc", vec![row("", "2026-09-12", 1250, "out")]), None).unwrap();
    let added = add_row(&c, b.summary.id).unwrap();
    ignore_row(&c, added, true).unwrap();
    let before = super::super::export(&c).unwrap();
    let other = memory_db(); super::super::replace_all(&other, &before).unwrap();
    assert_eq!(serde_json::to_value(before).unwrap(), serde_json::to_value(super::super::export(&other).unwrap()).unwrap());
    ignore_row(&c, added, false).unwrap();
    assert!(commit(&c, b.summary.id, &[added]).is_err()); // no date or amount yet
    let mut d = b.rows[0].draft.clone(); d.day = "2026-09-13".into(); d.amount = Some(500);
    save_row(&c, added, d).unwrap();
    assert_eq!(commit(&c, b.summary.id, &[b.rows[0].id, added]).unwrap().posted, 2);
    assert!(ignore_row(&c, added, true).is_err());
}

#[test]
fn old_rows_still_read_and_their_marks_keep_working() {
    // the shapes written before Importer: local extraction's fields and a review stamp
    let c = memory_db();
    let source = json!({"sheet":"截图","rowNumber":1,"raw":[["提取方式","本地 OCR"]],"tradeId":"","time":"","status":"待核对","draft":{"day":"2026-09-12","amount":800,"currency":"SGD","treatment":"out","category":null,"note":"Shop","refundOf":null},"issues":["本地 OCR 提取，请对照截图核对"],"region":[0.0,0.2,1.0,0.1]});
    let old_ai = r#"{"model":"deepseek","categoryDone":true,"noteDone":true,"settled":true,"why":"看得出","duplicate":"distinct","reason":"","review":{"status":"done","model":"m","changes":[],"reason":"一致"}}"#;
    c.execute("INSERT INTO fin_imports (id, name, source) VALUES (1, 'a.png', 'wise')", []).unwrap();
    c.execute("INSERT INTO fin_import_rows (id, batch_id, source_json, draft_json, ai_json, resolution) VALUES (1, 1, ?1, ?2, ?3, 'new')", params![source.to_string(), source["draft"].to_string(), old_ai]).unwrap();
    let b = batch(&c, 1).unwrap();
    assert_eq!(b.rows[0].source.region, Some([0.0, 0.2, 1.0, 0.1]));
    let ai = b.rows[0].ai.as_ref().unwrap();
    assert!(ai.settled && ai.duplicate == "distinct" && !ai.my_category);
    assert_eq!(commit(&c, 1, &[1]).unwrap().posted, 1);
    let exported = super::super::export(&c).unwrap();
    let other = memory_db();
    super::super::replace_all(&other, &exported).unwrap();
    assert_eq!(serde_json::to_value(exported).unwrap(), serde_json::to_value(super::super::export(&other).unwrap()).unwrap());
    // and a data file of format 34: its rows carry these fields too
    let row: RowRecord = serde_json::from_value(json!({"ai": serde_json::from_str::<serde_json::Value>(old_ai).unwrap(), "source": source, "draft": source["draft"], "state": "pending", "entry": null})).unwrap();
    assert_eq!(row.ai.unwrap().why.as_deref(), Some("看得出"));
}

#[test]
fn dbs_upgrade_preserves_phase_one_rows_and_older_export() {
    let mut c = Connection::open_in_memory().unwrap();
    crate::db::configure(&c).unwrap();
    crate::db::migrate_with(&mut c, &crate::db::MIGRATIONS[..31], None).unwrap();
    let source = row("prior", "2026-10-01", 1000, "out");
    c.execute("INSERT INTO fin_entries (id, flow, amount, currency, day) VALUES (42, 'out', 1000, 'CNY', '2026-10-01')", []).unwrap();
    c.execute("INSERT INTO fin_imports (id, name, source) VALUES (7, 'old.xlsx', 'wechat')", []).unwrap();
    c.execute("INSERT INTO fin_import_rows (id, batch_id, source_json, draft_json, state, entry_id) VALUES (9, 7, ?1, ?2, 'posted', 42)", params![serde_json::to_string(&source).unwrap(), serde_json::to_string(&source.draft).unwrap()]).unwrap();
    crate::db::migrate_with(&mut c, crate::db::MIGRATIONS, None).unwrap();
    let b = batch(&c, 7).unwrap();
    assert_eq!((b.rows[0].id, b.rows[0].entry_id, b.rows[0].source.clone()), (9, Some(42), source));
    assert!(b.rows[0].resolution.is_none());
    assert_eq!(c.query_row::<i64,_,_>("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).unwrap(), 0);
    let old = json!({"source": b.rows[0].source, "draft": b.rows[0].draft, "state":"posted", "entry":1});
    assert!(serde_json::from_value::<RowRecord>(old).unwrap().resolution.is_none());
}

#[test]
fn screenshot_migration_preserves_existing_manual_links() {
    let mut c = Connection::open_in_memory().unwrap();
    crate::db::configure(&c).unwrap();
    crate::db::migrate_with(&mut c, &crate::db::MIGRATIONS[..32], None).unwrap();
    let source = row("a", "2026-10-01", 100, "out");
    let entry = create_entry(&c, EntryPatch { flow:Some(Flow::Out),amount:Some(100),currency:Some(Currency::Cny),day:Some("2026-10-01".into()),..Default::default() }).unwrap();
    c.execute("INSERT INTO fin_imports(id,name,source) VALUES(1,'a.xlsx','wechat'),(2,'b.pdf','dbs')",[]).unwrap();
    for (id,state,resolution) in [(1,"posted",None),(2,"duplicate",Some("linked"))] {
        c.execute("INSERT INTO fin_import_rows(id,batch_id,source_json,draft_json,state,entry_id,resolution) VALUES(?1,?1,?2,?3,?4,?5,?6)",params![id,serde_json::to_string(&source).unwrap(),serde_json::to_string(&source.draft).unwrap(),state,entry.id,resolution]).unwrap();
    }
    crate::db::migrate_with(&mut c, crate::db::MIGRATIONS, None).unwrap();
    let linked=batch(&c,2).unwrap();assert_eq!(linked.rows[0].entry_id,Some(entry.id));assert_eq!(linked.rows[0].resolution.as_deref(),Some("linked"));assert!(linked.rows[0].ai.is_none());
    assert_eq!(c.query_row::<i64,_,_>("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).unwrap(), 0);
    let old: SourceRow = serde_json::from_value(json!({"sheet":"old","rowNumber":1,"raw":[],"tradeId":"","time":"","status":"","draft":source.draft,"issues":[]})).unwrap();
    assert!(old.region.is_none());
}

#[test]
fn batches_keep_their_order_and_name_through_export_and_deleting_one_keeps_its_entries() {
    let c = memory_db();
    let order = |c: &Connection| batches(c).unwrap().into_iter().map(|b| (b.name, b.title)).collect::<Vec<_>>();
    let a = ingest(&c, "a.xlsx", parsed("wechat", vec![row("a1", "2026-10-01", 100, "out")]), None).unwrap();
    let b = ingest(&c, "b.xlsx", parsed("wechat", vec![row("b1", "2026-10-02", 200, "out"), row("b2", "2026-10-03", 300, "out")]), None).unwrap();
    assert_eq!(order(&c), [("b.xlsx".into(), None), ("a.xlsx".into(), None)]); // newest first
    common::reorder(&c, "fin_imports", "1 = 1", &[], &[a.summary.id, b.summary.id]).unwrap();
    let mut odd = row("z1", "2026-10-04", 400, "out"); odd.draft.category = Some("不存在".into());
    let z = ingest(&c, "z.xlsx", parsed("alipay", vec![odd]), None).unwrap();
    assert_eq!((z.rows[0].draft.category.as_deref(), z.rows[0].source.draft.category.as_deref()), (None, Some("不存在")));
    rename_batch(&c, a.summary.id, " 十月微信 ").unwrap();
    rename_batch(&c, z.summary.id, "z.xlsx").unwrap();
    assert!(rename_batch(&c, a.summary.id, &"字".repeat(101)).is_err());
    let names = [("z.xlsx".into(), None), ("a.xlsx".into(), Some("十月微信".into())), ("b.xlsx".into(), None)];
    assert_eq!(order(&c), names);
    // a data file keeps both; an older one (format 33) lists newest first
    let exported = super::super::export(&c).unwrap();
    let other = memory_db();
    super::super::replace_all(&other, &exported).unwrap();
    assert_eq!(order(&other), names);
    let mut old = serde_json::to_value(&exported).unwrap();
    for b in old["imports"].as_array_mut().unwrap() { let b = b.as_object_mut().unwrap(); b.remove("title"); b.remove("sortOrder"); }
    let older = memory_db();
    super::super::replace_all(&older, &serde_json::from_value(old).unwrap()).unwrap();
    assert_eq!(order(&older).into_iter().map(|b| b.0).collect::<Vec<_>>(), ["z.xlsx", "b.xlsx", "a.xlsx"]);
    // deleting: its entries stay, a row merged into one of its open rows comes back
    commit(&c, b.summary.id, &[b.rows[0].id]).unwrap();
    let posted = batch(&c, b.summary.id).unwrap().rows[0].entry_id.unwrap();
    let ai = serde_json::to_string(&Record { model: "m".into(), duplicate: "duplicate".into(), duplicate_row: Some(b.rows[1].id), ..Default::default() }).unwrap();
    c.execute("UPDATE fin_import_rows SET state = 'duplicate', resolution = 'linked', ai_json = ?1 WHERE id = ?2", params![ai, a.rows[0].id]).unwrap();
    let (_, changes) = crate::undo::record(&c, |c| delete_batch(c, b.summary.id)).unwrap();
    assert_eq!(order(&c).len(), 2);
    assert!(entries(&c, "2026-10").unwrap().iter().any(|e| e.id == posted));
    let back = &batch(&c, a.summary.id).unwrap().rows[0];
    assert_eq!((back.state.as_str(), back.ai.is_none()), ("pending", true));
    assert_eq!(c.query_row::<i64, _, _>("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).unwrap(), 0);
    assert!(delete_batch(&c, b.summary.id).is_err());
    let mut inverse = vec![];
    rusqlite::session::invert_strm(&mut changes.unwrap().as_slice(), &mut inverse).unwrap();
    c.apply_strm(&mut inverse.as_slice(), None::<fn(&str) -> bool>, |_, _| rusqlite::session::ConflictAction::SQLITE_CHANGESET_ABORT).unwrap();
    assert_eq!(batch(&c, b.summary.id).unwrap().rows.len(), 2);
}

#[test]
fn migration_35_lists_old_batches_newest_first() {
    let mut c = Connection::open_in_memory().unwrap();
    crate::db::configure(&c).unwrap();
    crate::db::migrate_with(&mut c, &crate::db::MIGRATIONS[..34], None).unwrap();
    c.execute("INSERT INTO fin_imports (id, name, source) VALUES (1, 'old.xlsx', 'wechat'), (2, 'new.xlsx', 'alipay')", []).unwrap();
    crate::db::migrate_with(&mut c, crate::db::MIGRATIONS, None).unwrap();
    assert_eq!(batches(&c).unwrap().into_iter().map(|b| (b.name, b.title, b.sort_order)).collect::<Vec<_>>(), [("new.xlsx".into(), None, 1), ("old.xlsx".into(), None, 2)]);
    ingest(&c, "newest.xlsx", parsed("wechat", vec![row("n", "2026-10-01", 100, "out")]), None).unwrap();
    assert_eq!(batches(&c).unwrap()[0].name, "newest.xlsx");
}
