use super::*;
use crate::db::tests::memory_db;
use crate::modules::finance::import::tests::row;
use crate::modules::finance::{create_entry, EntryPatch};
use crate::modules::finance::import::{batch, commit, ingest, resolve_row, save_row, set_category, Record};
use std::io::Write;

fn workbook(rows: &[Vec<&str>]) -> Vec<u8> {
    let mut sheet = String::from("<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>");
    for (i, r) in rows.iter().enumerate() {
        sheet.push_str(&format!("<row r=\"{}\">", i + 1));
        for (j, value) in r.iter().enumerate() {
            let col = (b'A' + j as u8) as char;
            if let Some(serial) = value.strip_prefix("date:") { sheet.push_str(&format!("<c r=\"{col}{}\" s=\"1\"><v>{serial}</v></c>", i + 1)); }
            else if !value.is_empty() { sheet.push_str(&format!("<c r=\"{col}{}\" t=\"inlineStr\"><is><t>{value}</t></is></c>", i + 1)); }
        }
        sheet.push_str("</row>");
    }
    sheet.push_str("</sheetData></worksheet>");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let files = [
        ("_rels/.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#),
        ("[Content_Types].xml", "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>"),
        ("xl/workbook.xml", "<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"账单\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>"),
        ("xl/_rels/workbook.xml.rels", "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/></Relationships>"),
        ("xl/styles.xml", "<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><cellXfs count=\"2\"><xf numFmtId=\"0\"/><xf numFmtId=\"22\"/></cellXfs></styleSheet>"),
        ("xl/worksheets/sheet1.xml", sheet.as_str()),
    ];
    for (name, bytes) in files { zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap(); zip.write_all(bytes.as_bytes()).unwrap(); }
    zip.finish().unwrap().into_inner()
}

const HEADER: [&str; 5] = ["交易时间", "商户", "收/支", "金额", "单号"];

fn statement_rows(n: usize) -> Vec<Vec<&'static str>> {
    let mut rows = vec![vec!["支付宝账单"], vec!["共 N 笔"], vec![], HEADER.to_vec()];
    rows.extend((0..n).map(|_| vec!["date:46305.5", "店", "支出", "12.30", "'2026A"]));
    rows.push(vec!["------ 以上是全部 ------"]);
    rows
}

const TODAY: &str = "2026-11-02";

fn kinds() -> Value { json!({"in": ["工资"], "out": ["餐饮", "购物"]}) }

#[test]
fn excel_goes_as_numbered_lines_in_parts_each_with_the_header() {
    let s = read_excel(workbook(&statement_rows(130))).unwrap();
    assert_eq!(s.parts.iter().map(|p| p.lines.len()).collect::<Vec<_>>(), [60, 60, 11]);
    assert!(s.parts.iter().all(|p| p.head.contains("交易时间") && p.head.contains("支付宝账单") && p.label.contains("分了 3 块")));
    // numbered by place, with the date read and the leading ' gone; the sheet's row number is kept
    let first = &s.lines[s.parts[0].lines[0] - 1];
    assert!(first.text.starts_with("4: [") && first.text.contains("2026-10-10 12:00:00") && first.text.contains("\"2026A\""));
    assert_eq!((first.number, first.sheet.as_str(), first.full), (5, "账单", true));
    assert_eq!(first.raw[1], ("商户".to_string(), "店".to_string()));
    // a table's own Column1… above the statement's lines isn't the header
    let mut framed = vec![vec!["Column1", "Column2", "Column3", "Column4", "Column5", "Column6"]];
    framed.extend(statement_rows(2));
    let s2 = read_excel(workbook(&framed)).unwrap();
    assert_eq!(s2.parts[0].lines.len(), 3); // the two and the footer
    assert!(s2.parts[0].head.contains("交易时间") && s2.parts[0].head.contains("Column1"));
    assert_eq!(s2.lines[s2.parts[0].lines[0] - 1].raw[1].0, "商户");
    let prompt: Value = serde_json::from_str(&part_prompt("a.xlsx", "2026-11-02", &kinds(), &s.parts[1], &s.lines)).unwrap();
    let text = prompt["source"].as_str().unwrap();
    assert!(text.contains("交易时间") && text.contains(&s.lines[s.parts[1].lines[0] - 1].text));
    assert!(!text.contains(&format!("\n{}: ", s.parts[0].lines[0])));
    // the footer isn't a row that must be read
    assert!(!s.lines.last().unwrap().full);
    assert!(read_excel(workbook(&[vec!["只有一行"]])).is_err());
    assert!(read_excel(b"not a workbook".to_vec()).is_err());
}

#[test]
fn the_calendar_says_the_weekday_of_each_first() {
    let c = calendar("2026-11-02");
    assert_eq!(c.len(), 6);
    assert_eq!((c[0].as_str(), c[2].as_str(), c[5].as_str()), ("2026-11-01 周日", "2026-09-01 周二", "2026-06-01 周一"));
    assert_eq!(calendar("2027-01-15")[1], "2026-12-01 周二");
    assert!(calendar("").is_empty());
    let prompt: Value = serde_json::from_str(&part_prompt("a.png", "2026-11-02", &kinds(), &Part::default(), &[])).unwrap();
    assert_eq!(prompt["calendar"][2], "2026-09-01 周二");
    assert!(prompt.get("source").is_none());
}

#[test]
fn rows_nobody_read_are_asked_again() {
    let source = read_excel(workbook(&statement_rows(70))).unwrap();
    let (a, b) = (source.parts[0].clone(), source.parts[1].clone());
    let named = json!({"rows": a.lines[1..].iter().map(|k| json!({"line": k})).collect::<Vec<_>>()});
    let again = unread(&[(a.clone(), Ok(named)), (b.clone(), Err("断了".to_string()))], &source);
    assert_eq!(again.len(), 2);
    assert_eq!(again[0].lines, [a.lines[0]]);
    assert_eq!(again[1].lines, b.lines[..b.lines.len() - 1]); // not the footer
    assert!(again[0].head.contains("交易时间") && again[0].label.contains("没读到"));
    let prompt: Value = serde_json::from_str(&part_prompt("a.xlsx", "2026-11-02", &kinds(), &again[0], &source.lines)).unwrap();
    assert!(prompt["source"].as_str().unwrap().contains(&source.lines[a.lines[0] - 1].text));
    // nothing to ask of a screenshot
    assert!(unread(&[(Part::default(), Ok(json!({"rows": []})))], &Source::default()).is_empty());
}

#[test]
fn the_mock_model_reads_a_file_end_to_end() {
    let c = memory_db();
    let kinds = categories(&c).unwrap();
    let dir = std::env::temp_dir().join(format!("pomelo-importer-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("账单.xlsx");
    std::fs::write(&path, workbook(&statement_rows(3))).unwrap();
    let source = read_source(path.to_str().unwrap()).unwrap();
    assert!(source.preview.is_none());
    let m = Model { target: Target::Mock { thinking: false }, name: "模拟".into(), level: Level::off(), max_tokens: OUTPUT };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (parsed, errors) = rt.block_on(read_statement(&m, "账单.xlsx", &source, &[], &kinds, TODAY)).unwrap();
    assert!(errors.is_empty());
    let b = ingest(&c, "账单.xlsx", parsed, Some("模拟")).unwrap();
    assert_eq!(b.rows.len(), 3);
    assert_eq!(b.rows[2].source.issues, ["模拟：要你看一下"]);
    assert_eq!(b.rows[0].ai.as_ref().unwrap().model, "模拟");
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(read_source("/没有/这个.txt").is_err());
}

#[test]
fn channels_are_read_from_the_statement_or_its_text() {
    assert_eq!(channel_of("alipay", "财付通 也写着"), Some("支付宝")); // its own statement wins
    assert_eq!(channel_of("wechat", ""), Some("微信"));
    assert_eq!(channel_of("icbc", "财付通-JJ DRINK"), Some("微信"));
    assert_eq!(channel_of("icbc", "支付宝-JJ DRINK"), Some("支付宝"));
    assert_eq!(channel_of("dbs", "PayNow to KOI"), None);
    assert_eq!(channel_of("icbc", "支付宝 转 微信"), None); // both named: can't tell
    assert_eq!(channel_of("", "ALIPAY+ Voyager"), Some("支付宝"));
}

#[test]
fn another_channel_never_merges_without_asking() {
    let c = memory_db();
    // 支付宝's statement, and an entry that came from WeChat's
    let raw = |text: &str, day: &str, amount: i64| { let mut r = row("", day, amount, "out"); r.raw = vec![("原文".into(), text.to_string())]; r.draft.note = text.to_string(); r };
    let alipay = ingest(&c, "支付宝.xlsx", Parsed { source: "alipay".into(), rows: vec![row("a1", "2026-09-03", 531, "out"), row("a2", "2026-09-05", 532, "out")] }, None).unwrap();
    let wechat = ingest(&c, "微信.xlsx", Parsed { source: "wechat".into(), rows: vec![row("w1", "2026-09-06", 700, "out")] }, None).unwrap();
    commit(&c, wechat.summary.id, &[wechat.rows[0].id]).unwrap();
    let posted = batch(&c, wechat.summary.id).unwrap().rows[0].entry_id.unwrap();
    let by_hand = entry_on(&c, "2026-09-07", 800, "午饭，微信付的");
    let plain = entry_on(&c, "2026-09-07", 900, "午饭");
    let bank = ingest(&c, "工行.jpg", Parsed { source: "icbc".into(), rows: vec![raw("财付通-JJ DRINK", "2026-09-04", 531), raw("支付宝-JJ DRINK", "2026-09-05", 532), raw("支付宝-某店", "2026-09-06", 700), raw("支付宝-某店", "2026-09-07", 800), raw("支付宝-某店", "2026-09-07", 900), raw("柜台取款", "2026-09-03", 531)] }, Some("模型")).unwrap();
    let rows = rows_of(&c, &[bank.summary.id]);
    let jobs = match_jobs(&c, &rows).unwrap();
    // the model is told, too
    let ledger = jobs[0].prompt["ledger"].as_array().unwrap();
    let of = |key: String| ledger.iter().find(|v| v["key"] == key).unwrap()["channel"].clone();
    assert_eq!((of(format!("r:{}", alipay.rows[0].id)), of(format!("e:{posted}")), of(format!("e:{by_hand}")), of(format!("e:{plain}"))), (json!("支付宝"), json!("微信"), json!("微信"), Value::Null));
    assert_eq!(jobs[0].prompt["rows"][0]["channel"], "微信");
    let id = |i: usize| bank.rows[i].id;
    let (r0, r1) = (format!("r:{}", alipay.rows[0].id), format!("r:{}", alipay.rows[1].id));
    let answers = vec![
        json!({"id": id(0), "decision": "duplicate", "target": r0, "reason": "同商户同额"}), // 财付通 → 支付宝's statement: the mistake this stops
        json!({"id": id(1), "decision": "duplicate", "target": r1, "reason": "同一笔"}), // 支付宝 → 支付宝's statement: fine
        json!({"id": id(2), "decision": "duplicate", "target": format!("e:{posted}"), "reason": "同额"}), // 支付宝 → an entry from WeChat's
        json!({"id": id(3), "decision": "duplicate", "target": format!("e:{by_hand}"), "reason": "同额"}), // 支付宝 → I said WeChat
        json!({"id": id(4), "decision": "duplicate", "target": format!("e:{plain}"), "reason": "同额"}), // nothing said: fine
        json!({"id": id(5), "decision": "duplicate", "target": r0, "reason": "同额"}), // no channel on this side: fine
    ];
    let report = apply_match(&c, &rows, "模型", vec![job_answers(&jobs[0], answers)]).unwrap();
    assert_eq!((report.linked, report.review), (3, 3));
    let got = batch(&c, bank.summary.id).unwrap().rows;
    for i in [0, 2, 3] {
        assert_eq!((got[i].state.as_str(), got[i].ai.as_ref().unwrap().duplicate.as_str()), ("pending", "review"), "第 {i} 行");
        assert!(got[i].ai.as_ref().unwrap().reason.starts_with("付款渠道不同，没有直接合并"));
    }
    assert_eq!(got[0].pending_candidates.first().map(|p| p.id), Some(alipay.rows[0].id)); // offered, for one click
    for i in [1, 4, 5] { assert_eq!(got[i].state, "duplicate", "第 {i} 行"); }
}

#[test]
fn a_merge_into_what_i_wrote_needs_the_same_category() {
    let c = memory_db();
    let traffic = kinds::name_map(&c, Flow::Out.kinds_module()).unwrap().into_iter().find(|(_, n)| n == "交通").unwrap().0;
    let by_hand = create_entry(&c, EntryPatch { flow: Some(Flow::Out), amount: Some(729), currency: Some(Currency::Cny), day: Some("2026-09-22".into()), kind_id: Some(Some(traffic)), note: Some(Some("打车".into())), ..Default::default() }).unwrap().id;
    let new = ingest(&c, "这次.xlsx", Parsed { source: "alipay".into(), rows: vec![row("a", "2026-09-23", 729, "out")] }, Some("模型")).unwrap();
    let rows = rows_of(&c, &[new.summary.id]);
    let jobs = match_jobs(&c, &rows).unwrap();
    apply_match(&c, &rows, "模型", vec![job_answers(&jobs[0], vec![json!({"id": new.rows[0].id, "decision": "duplicate", "target": format!("e:{by_hand}"), "reason": "同额"})])]).unwrap();
    let got = &batch(&c, new.summary.id).unwrap().rows[0];
    assert_eq!((got.state.as_str(), got.ai.as_ref().unwrap().duplicate.as_str()), ("pending", "review"));
}

#[test]
fn later_page_groups_bring_page_one_for_the_period() {
    assert_eq!(page_groups(2).iter().map(|g| g.0.clone()).collect::<Vec<_>>(), [vec![0, 1]]);
    assert!(page_groups(2)[0].1.is_empty());
    let g = page_groups(9);
    assert_eq!(g.iter().map(|g| g.0.clone()).collect::<Vec<_>>(), [vec![0, 1, 2, 3], vec![0, 4, 5, 6, 7], vec![0, 8]]);
    assert!(g[1].1.contains("第 5～8 页") && g[2].1.contains("第一张图"));
}

#[test]
fn checklist_is_a_markdown_list_of_the_points_in_force() {
    let s = checked(Settings { model: None, level: Level::off(), points: vec![
        Point { text: "  账单中出现 Steam 的明细，\n统一把备注写成“Steam 充值” ".into(), on: true },
        Point { text: "不生效的".into(), on: false },
        Point { text: "   ".into(), on: true },
        Point { text: "Wise 是新币".into(), on: true },
    ] }).unwrap();
    assert_eq!(s.points.len(), 3);
    assert_eq!(checklist(&s.points), "- 账单中出现 Steam 的明细， 统一把备注写成“Steam 充值”\n- Wise 是新币");
    let sent = system(READ, &s.points);
    assert!(sent.ends_with("## 我的要求（优先于上面的做法）\n\n- 账单中出现 Steam 的明细， 统一把备注写成“Steam 充值”\n- Wise 是新币"));
    assert_eq!(system(READ, &[Point { text: "关着".into(), on: false }]), READ);
    assert!(checked(Settings { model: None, level: Level::off(), points: vec![Point { text: "长".repeat(501), on: true }] }).is_err());
    // kept with the model in youzi.db
    let c = crate::youzi::store::tests::memory_db();
    pref_set(&c, KEY, &s).unwrap();
    assert_eq!(load(&c), s);
}

#[test]
fn the_models_rows_are_kept_and_only_bad_values_dropped() {
    let source = read_excel(workbook(&statement_rows(3))).unwrap();
    let keys = &source.parts[0].lines;
    let v = json!({"source": "alipay", "rows": [
        {"line": keys[0], "day": "2026-10-10", "amount": 1230, "currency": "SGD", "treatment": "out", "category": "餐饮", "note": "店：午饭", "tradeId": "'2026A", "doubt": ""},
        {"line": keys[1], "day": "2026-13-01", "amount": -1, "currency": "EUR", "treatment": "gift", "category": "发明的", "note": "Shop", "tradeId": "/", "doubt": "看不清是不是退款"},
        {"line": keys[1], "day": "2026-10-10", "amount": 999, "currency": "CNY", "treatment": "out", "note": "同一行又来一次"},
    ]});
    let (p, errors) = statement(vec![(source.parts[0].clone(), Ok(v))], &kinds(), &source, TODAY).unwrap();
    assert!(errors.is_empty());
    assert_eq!(p.source, "alipay");
    assert_eq!(p.rows.len(), 3); // two read, one this model skipped
    let (a, b, gap) = (&p.rows[0], &p.rows[1], &p.rows[2]);
    assert_eq!((a.draft.category.as_deref(), a.draft.amount, a.draft.currency, a.issues.len(), a.trade_id.as_str(), a.row_number), (Some("餐饮"), Some(1230), Currency::Sgd, 0, "2026A", 5));
    assert_eq!(a.raw[1], ("商户".to_string(), "店".to_string()));
    assert_eq!((b.draft.day.as_str(), b.draft.amount, b.draft.currency, b.draft.treatment.as_str(), b.draft.category.as_deref(), b.trade_id.as_str()), ("", None, Currency::Cny, "out", None, ""));
    assert_eq!(b.issues.len(), 5);
    assert!(b.issues.contains(&"看不清是不是退款".to_string()));
    assert_eq!((gap.draft.amount, gap.row_number, gap.issues[0].as_str()), (None, 7, "模型漏了这一行，请核对"));
    assert_eq!(gap.raw[1].1, "店");
    // nothing read at all: no statement
    assert!(statement(vec![(source.parts[0].clone(), Ok(json!({"rows": []})))], &kinds(), &read_excel(workbook(&statement_rows(0))).unwrap_or(Source::default()), TODAY).is_err());
    assert!(statement(vec![(source.parts[0].clone(), Err("网络断了".to_string()))], &kinds(), &source, TODAY).is_err());
}

#[test]
fn a_part_that_failed_keeps_its_rows_to_check_and_images_have_their_own_text() {
    let source = read_excel(workbook(&statement_rows(70))).unwrap();
    let (a, b) = (source.parts[0].clone(), source.parts[1].clone());
    let answer = json!({"source": "wechat", "rows": a.lines.iter().map(|k| json!({"line": k, "day": "2026-10-10", "amount": 100, "currency": "CNY", "treatment": "out", "note": "x"})).collect::<Vec<_>>()});
    let (p, errors) = statement(vec![(a, Ok(answer)), (b.clone(), Err("模型没有正常答完".into()))], &kinds(), &source, TODAY).unwrap();
    assert_eq!(errors, ["模型没有正常答完"]);
    assert_eq!(p.rows.len(), 70);
    assert!(p.rows.iter().skip(60).all(|r| r.draft.amount.is_none() && r.issues[0].contains("模型没有正常答完")));
    // rows stay in the file's order
    assert!(p.rows.windows(2).all(|w| w[0].row_number < w[1].row_number));
    let shot = Source { sheet: "截图".into(), parts: vec![Part::default()], ..Default::default() };
    let v = json!({"source": "别的", "rows": [{"day": "2026-07-29", "amount": 10924, "currency": "SGD", "treatment": "out", "category": "购物", "note": "Don Don Donki", "raw": "Don Don Donki - 109.24 SGD", "tradeId": ""}, {"day": "", "amount": 95.0, "currency": "SGD", "treatment": "neutral", "category": "购物", "note": "x", "raw": ""}]});
    let (p, _) = statement(vec![(Part::default(), Ok(v))], &kinds(), &shot, TODAY).unwrap();
    assert_eq!(p.source, OTHER_SOURCE);
    assert_eq!((p.rows[0].sheet.as_str(), p.rows[0].row_number, p.rows[0].raw[0].1.as_str()), ("截图", 1, "Don Don Donki - 109.24 SGD"));
    assert_eq!((p.rows[1].draft.amount, p.rows[1].draft.category.as_deref(), p.rows[1].row_number), (Some(95), None, 2));
    // a date after today stays, marked
    let (p, _) = statement(vec![(Part::default(), Ok(json!({"rows": [{"day": "2026-11-09", "amount": 100, "currency": "CNY", "treatment": "out", "note": "x"}]})))], &kinds(), &shot, TODAY).unwrap();
    assert_eq!((p.rows[0].draft.day.as_str(), p.rows[0].issues[0].as_str()), ("2026-11-09", "日期在今天之后，请核对"));
}

fn rows_of(c: &Connection, ids: &[i64]) -> Vec<Open> { open_rows(c, ids, None).unwrap() }

fn job_answers(job: &Job, answers: Vec<Value>) -> (Job, Result<Vec<Value>, String>) { (job.clone(), Ok(answers)) }

fn entry_on(c: &Connection, day: &str, amount: i64, note: &str) -> i64 {
    create_entry(c, EntryPatch { flow: Some(Flow::Out), amount: Some(amount), currency: Some(Currency::Cny), day: Some(day.into()), note: Some(Some(note.into())), ..Default::default() }).unwrap().id
}

#[test]
fn the_ledger_around_a_batch_is_what_the_model_sees() {
    let c = memory_db();
    let by_hand = entry_on(&c, "2026-10-03", 1500, "和朋友吃饭");
    let far = entry_on(&c, "2026-08-01", 1000, "太早了");
    let period = create_entry(&c, EntryPatch { flow: Some(Flow::Out), amount: Some(30000), currency: Some(Currency::Cny), day: Some("2026-09-28".into()), end_day: Some(Some("2026-10-04".into())), ..Default::default() }).unwrap().id;
    let older = ingest(&c, "上次.xlsx", Parsed { source: "alipay".into(), rows: vec![row("old1", "2026-10-02", 800, "out"), row("old2", "2026-10-02", 900, "neutral"), row("posted", "2026-10-02", 700, "out")] }, None).unwrap();
    commit(&c, older.summary.id, &[older.rows[2].id]).unwrap();
    let new = ingest(&c, "这次.xlsx", Parsed { source: "wechat".into(), rows: vec![row("a", "2026-10-01", 1000, "out"), row("n", "2026-10-01", 50, "neutral"), row("b", "2026-10-02", 1500, "out")] }, Some("模型")).unwrap();
    let later = ingest(&c, "之后.xlsx", Parsed { source: "dbs".into(), rows: vec![row("z", "2026-10-02", 1100, "out")] }, None).unwrap();
    let rows = rows_of(&c, &[new.summary.id]);
    let jobs = match_jobs(&c, &rows).unwrap();
    assert_eq!(jobs.len(), 1);
    let job = &jobs[0];
    assert_eq!(job.rows, [new.rows[0].id, new.rows[2].id]); // not 不计收支
    let ledger = job.prompt["ledger"].as_array().unwrap();
    let keys: Vec<&str> = ledger.iter().map(|v| v["key"].as_str().unwrap()).collect();
    assert!(keys.contains(&format!("e:{by_hand}").as_str()) && keys.contains(&format!("e:{period}").as_str()));
    assert!(!keys.contains(&format!("e:{far}").as_str()));
    assert!(keys.contains(&format!("r:{}", older.rows[0].id).as_str()));
    assert!(!keys.contains(&format!("r:{}", older.rows[1].id).as_str())); // 不计收支
    assert!(!keys.iter().any(|k| *k == format!("r:{}", later.rows[0].id) || *k == format!("r:{}", new.rows[0].id)));
    let by = |key: String| ledger.iter().find(|v| v["key"] == key).unwrap();
    assert_eq!(by(format!("e:{by_hand}"))["from"], json!(["手记"]));
    assert_eq!(by(format!("e:{period}"))["until"], "2026-10-04");
    let posted = batch(&c, older.summary.id).unwrap().rows[2].entry_id.unwrap();
    assert_eq!(by(format!("e:{posted}"))["from"], json!(["上次.xlsx"]));
}

#[test]
fn match_merges_says_new_or_asks_and_only_keys_it_was_given() {
    let c = memory_db();
    let by_hand = entry_on(&c, "2026-10-03", 1400, "和朋友吃饭，大概 14");
    let exact = entry_on(&c, "2026-10-03", 1000, "午饭");
    let older = ingest(&c, "上次.png", Parsed { source: "wise".into(), rows: vec![row("", "2026-10-02", 1100, "out"), row("", "2026-09-27", 1600, "out")] }, None).unwrap();
    let new = ingest(&c, "这次.xlsx", Parsed { source: "wechat".into(), rows: (0..8).map(|i| row(&format!("t{i}"), "2026-10-02", 1000 + i * 100, "out")).collect() }, Some("模型")).unwrap();
    let id = |i: usize| new.rows[i].id;
    let rows = rows_of(&c, &[new.summary.id]);
    let jobs = match_jobs(&c, &rows).unwrap();
    let answers = vec![
        json!({"id": id(0), "decision": "duplicate", "target": format!("e:{exact}"), "reason": "就是那顿饭"}),
        json!({"id": id(1), "decision": "duplicate", "target": format!("r:{}", older.rows[0].id), "reason": "同一笔"}),
        json!({"id": id(2), "decision": "distinct", "reason": ""}),
        json!({"id": id(3), "decision": "review", "target": format!("e:{by_hand}"), "reason": "可能是这笔"}),
        json!({"id": id(4), "decision": "duplicate", "target": "e:999999", "reason": "没给过的"}),
        json!({"id": id(5), "decision": "duplicate", "target": format!("e:{by_hand}"), "reason": "大概是这笔"}), // the amount differs: asks
        json!({"id": id(6), "decision": "duplicate", "target": format!("r:{}", older.rows[1].id), "reason": "同额"}), // 5 days between statements: asks
        json!({"id": 777777, "decision": "duplicate", "target": format!("e:{by_hand}")}), // not asked about
    ];
    let (report, changes) = crate::undo::record(&c, |c| apply_match(c, &rows, "模型", vec![job_answers(&jobs[0], answers)])).unwrap();
    assert_eq!((report.linked, report.review), (2, 5));
    let got = batch(&c, new.summary.id).unwrap().rows;
    assert_eq!((got[0].state.as_str(), got[0].entry_id, got[0].resolution.as_deref()), ("duplicate", Some(exact), Some("linked")));
    assert_eq!(got[1].state, "duplicate");
    assert_eq!(got[1].ai.as_ref().unwrap().duplicate_row, Some(older.rows[0].id));
    assert_eq!((got[2].state.as_str(), got[2].resolution.as_deref()), ("pending", Some("new")));
    assert_eq!((got[3].resolution.as_deref(), got[3].ai.as_ref().unwrap().duplicate.as_str()), (None, "review"));
    // what it means is offered, and 是这笔 works though the amount differs
    assert_eq!(got[3].candidates.first().map(|c| c.entry.id), Some(by_hand));
    assert_eq!(got[4].ai.as_ref().unwrap().duplicate, "review");
    assert!(got[4].ai.as_ref().unwrap().reason.contains("对不上"));
    assert_eq!((got[5].state.as_str(), got[5].ai.as_ref().unwrap().duplicate.as_str(), got[5].ai.as_ref().unwrap().target.clone()), ("pending", "review", Some(format!("e:{by_hand}"))));
    assert_eq!((got[6].state.as_str(), got[6].pending_candidates.first().map(|c| c.id)), ("pending", Some(older.rows[1].id)));
    assert_eq!((got[7].ai.as_ref().unwrap().duplicate.as_str(), got[7].ai.as_ref().unwrap().reason.as_str()), ("error", "模型漏了这一笔"));
    assert!(commit(&c, new.summary.id, &[got[3].id]).is_err());
    assert!(commit(&c, new.summary.id, &[got[7].id]).is_err());
    assert_eq!(commit(&c, new.summary.id, &[got[2].id]).unwrap().posted, 1);
    resolve_row(&c, got[3].id, Some(by_hand)).unwrap();
    assert_eq!(batch(&c, new.summary.id).unwrap().rows[3].entry_id, Some(by_hand));
    // a data file doesn't keep the row it meant; it is the same when read back
    let exported = crate::modules::finance::export(&c).unwrap();
    let other = memory_db();
    crate::modules::finance::replace_all(&other, &exported).unwrap();
    assert_eq!(serde_json::to_value(&exported).unwrap(), serde_json::to_value(crate::modules::finance::export(&other).unwrap()).unwrap());
    assert!(!changes.unwrap().is_empty());
}

#[test]
fn match_leaves_what_i_decided_and_what_changed_meanwhile() {
    let c = memory_db();
    let by_hand = entry_on(&c, "2026-10-03", 1000, "午饭");
    let new = ingest(&c, "这次.xlsx", Parsed { source: "wechat".into(), rows: vec![row("a", "2026-10-02", 1000, "out"), row("b", "2026-10-02", 1000, "out"), row("c", "2026-10-02", 1000, "out")] }, Some("模型")).unwrap();
    // 不是重复 is mine; a row saved still gets checked
    resolve_row(&c, new.rows[0].id, None).unwrap();
    let rows = rows_of(&c, &[new.summary.id]);
    let jobs = match_jobs(&c, &rows).unwrap();
    assert_eq!(jobs[0].rows, [new.rows[1].id, new.rows[2].id]);
    // the model's own 不是重复 may be redone
    let answers = vec![json!({"id": new.rows[1].id, "decision": "distinct"}), json!({"id": new.rows[2].id, "decision": "duplicate", "target": format!("e:{by_hand}")})];
    let mut d = new.rows[2].draft.clone(); d.note = "我改的".into();
    save_row(&c, new.rows[2].id, d).unwrap();
    apply_match(&c, &rows, "模型", vec![job_answers(&jobs[0], answers)]).unwrap();
    let got = batch(&c, new.summary.id).unwrap().rows;
    assert_eq!(got[1].ai.as_ref().unwrap().duplicate, "distinct");
    assert_eq!((got[2].state.as_str(), got[2].draft.note.as_str()), ("ready", "我改的")); // changed while it ran
    let rows = rows_of(&c, &[new.summary.id]);
    assert!(match_jobs(&c, &rows).unwrap()[0].rows.contains(&new.rows[1].id));
}

#[test]
fn tidy_follows_the_model_but_keeps_my_rows_and_my_category() {
    let c = memory_db();
    let kinds = categories(&c).unwrap();
    let mut no_day = row("", "", 1200, "out"); no_day.issues = vec!["没有日期，请补上".into()];
    let new = ingest(&c, "这次.png", Parsed { source: "wise".into(), rows: vec![no_day, row("b", "2026-10-02", 1000, "out"), row("c", "2026-10-02", 1000, "out"), row("d", "2026-10-02", 1000, "out"), row("e", "2026-10-02", 1000, "out")] }, Some("模型")).unwrap();
    let id = |i: usize| new.rows[i].id;
    set_category(&c, &[id(2)], Some("购物")).unwrap();
    let mut d = new.rows[3].draft.clone(); d.note = "我存过的".into();
    save_row(&c, id(3), d).unwrap();
    let rows = rows_of(&c, &[new.summary.id]);
    let pending: Vec<&ImportRow> = rows.iter().filter(|o| o.row.state == "pending").map(|o| &o.row).collect();
    assert_eq!(pending.len(), 4);
    let job = Job { batch: 0, rows: pending.iter().map(|r| r.id).collect(), prompt: json!({"rows": pending.iter().map(|r| tidy_row(r)).collect::<Vec<_>>(), "categories": kinds}) };
    assert_eq!(job.prompt["rows"][0]["issues"][0], "没有日期，请补上");
    let answers = vec![
        json!({"id": id(0), "day": "2026-09-23", "amount": 1200, "currency": "CNY", "treatment": "out", "category": "餐饮", "note": "Steam 充值", "settled": true, "why": "我的要求写了是九月"}),
        json!({"id": id(1), "day": "bad", "amount": -5, "currency": "EUR", "treatment": "neutral", "category": "餐饮", "note": "转给自己", "settled": false, "why": "看不出是不是本人账户"}),
        json!({"id": id(2), "treatment": "out", "category": "餐饮", "note": "x".repeat(1200), "settled": true, "why": ""}),
    ];
    let r = apply_tidy(&c, &rows, "模型", &kinds, vec![(job, Ok(answers))]).unwrap();
    assert_eq!((r.tidied, r.review), (3, 1));
    let got = batch(&c, new.summary.id).unwrap().rows;
    assert_eq!((got[0].draft.day.as_str(), got[0].draft.note.as_str(), got[0].draft.category.as_deref()), ("2026-09-23", "Steam 充值", Some("餐饮")));
    assert_eq!(got[0].source.draft.day, ""); // what was read stays
    assert!(got[0].ai.as_ref().unwrap().settled);
    // bad values dropped for that row alone; 不计收支 has no 分类
    assert_eq!((got[1].draft.day.as_str(), got[1].draft.amount, got[1].draft.currency, got[1].draft.treatment.as_str(), got[1].draft.category.as_deref()), ("2026-10-02", Some(1000), Currency::Cny, "neutral", None));
    let ai = got[1].ai.as_ref().unwrap();
    assert!(ai.note_error.as_deref().unwrap().contains("日期、金额、币种") && !ai.settled);
    assert_eq!(ai.why.as_deref(), Some("看不出是不是本人账户"));
    // the 分类 I set stays; the note is cut, not refused
    assert_eq!((got[2].draft.category.as_deref(), got[2].draft.note.chars().count()), (Some("购物"), 1000));
    assert!(got[2].ai.as_ref().unwrap().my_category);
    // saved: not asked about, not touched
    assert_eq!((got[3].draft.note.as_str(), got[3].ai.is_none()), ("我存过的", true));
    // skipped: said so, nothing changed
    assert_eq!(got[4].ai.as_ref().unwrap().note_error.as_deref(), Some("模型漏了这一笔"));
    assert_eq!(got[4].draft.note, "测试交易");
}

#[test]
fn a_tidy_that_moves_a_row_asks_about_its_duplicate_again() {
    let c = memory_db();
    let kinds = categories(&c).unwrap();
    let new = ingest(&c, "这次.png", Parsed { source: "wise".into(), rows: vec![row("a", "2026-10-02", 1000, "out")] }, Some("模型")).unwrap();
    let mut record = Record { model: "模型".into(), duplicate: "distinct".into(), ..Default::default() };
    record.reason = "".into();
    write_record(&c, new.rows[0].id, &record).unwrap();
    c.execute("UPDATE fin_import_rows SET resolution = 'new' WHERE id = ?1", [new.rows[0].id]).unwrap();
    let rows = rows_of(&c, &[new.summary.id]);
    let job = Job { batch: 0, rows: vec![new.rows[0].id], prompt: Value::Null };
    apply_tidy(&c, &rows, "模型", &kinds, vec![(job, Ok(vec![json!({"id": new.rows[0].id, "day": "2026-10-03", "treatment": "out", "category": null, "note": "n", "settled": true, "why": ""})]))]).unwrap();
    assert_eq!(batch(&c, new.summary.id).unwrap().rows[0].resolution, None);
}

fn batch_named(c: &Connection, name: &str) -> i64 {
    import::batches(c).unwrap().into_iter().find(|b| b.name == name).unwrap().id
}

/// The real thing, on a folder of statements, with a cloud model (never the keychain; see chat.rs `cloud_target`):
///   POMELO_CLOUD_KEY_FILE=… POMELO_CLOUD_SERVICE=… POMELO_CLOUD_MODEL=… [POMELO_CLOUD_THINK=always]
///   POMELO_IMPORTER_DIR=<statements> POMELO_IMPORTER_OUT=<result.json> [POMELO_IMPORTER_POINTS=<one point a line>]
///   [POMELO_IMPORTER_LEDGER=<JSON of entries: day, amount, currency, flow, kind, note>] [POMELO_IMPORTER_ONLY=a.jpg,b.xlsx]
///   [POMELO_TODAY=2026-11-02] [POMELO_IMPORTER_AGAIN=<a file of the folder>: posted, then read and checked once more]
///   [POMELO_IMPORTER_TIDY=<a point>: one more point, every open row of POMELO_IMPORTER_TIDY_FILE redone under it]
///   cargo test real_importer -- --ignored --nocapture
/// Everything stays in memory but the result file.
#[test]
#[ignore = "real cloud model, real statements: ask first"]
fn real_importer() {
    let Some((target, _)) = crate::youzi::chat::tests::cloud_target() else { return eprintln!("needs POMELO_CLOUD_KEY_FILE") };
    let dir = std::env::var("POMELO_IMPORTER_DIR").expect("POMELO_IMPORTER_DIR");
    let out = std::env::var("POMELO_IMPORTER_OUT").expect("POMELO_IMPORTER_OUT");
    let points: Vec<Point> = std::env::var("POMELO_IMPORTER_POINTS").ok().map_or(vec![], |f| std::fs::read_to_string(f).unwrap().lines().map(|l| Point { text: l.into(), on: true }).collect());
    let only = std::env::var("POMELO_IMPORTER_ONLY").ok().map(|s| s.split(',').map(str::to_string).collect::<Vec<_>>());
    let c = memory_db();
    let kinds = categories(&c).unwrap();
    let today = std::env::var("POMELO_TODAY").unwrap_or_else(|_| finance::today(&c).unwrap());
    if let Ok(f) = std::env::var("POMELO_IMPORTER_LEDGER") {
        let names = kinds::name_map(&c, Flow::Out.kinds_module()).unwrap();
        for e in serde_json::from_str::<Vec<Value>>(&std::fs::read_to_string(f).unwrap()).unwrap() {
            let kind = names.iter().find(|(_, n)| e["kind"] == n.as_str()).map(|(k, _)| *k);
            create_entry(&c, EntryPatch { flow: Some(Flow::Out), amount: e["amount"].as_i64(), currency: serde_json::from_value(e["currency"].clone()).ok(), day: e["day"].as_str().map(str::to_string), end_day: e["until"].as_str().map(|u| Some(u.to_string())), kind_id: Some(kind), note: Some(e["note"].as_str().map(str::to_string)), ..Default::default() }).unwrap();
        }
    }
    let level = Level(std::env::var("POMELO_IMPORTER_LEVEL").unwrap_or_else(|_| "off".into()));
    let m = Model { max_tokens: OUTPUT, name: std::env::var("POMELO_CLOUD_MODEL").unwrap_or_default(), level, target };
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| Some(e.ok()?.path())).filter(|p| p.is_file() && p.extension().is_some_and(|e| ["xlsx", "pdf", "jpg", "png"].contains(&e.to_string_lossy().to_lowercase().as_str()))).collect();
    files.sort();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let mut report = json!({"model": m.name, "files": {}, "errors": []});
    let mut ids = vec![];
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if only.as_ref().is_some_and(|o| !o.contains(&name)) { continue; }
        let started = std::time::Instant::now();
        let before = *USAGE.lock().unwrap();
        let source = read_source(path.to_str().unwrap()).unwrap();
        match rt.block_on(read_statement(&m, &name, &source, &points, &kinds, &today)) {
            Ok((parsed, errors)) => {
                let batch = ingest(&c, &name, parsed, Some(&m.name)).unwrap();
                ids.push(batch.summary.id);
                let after = *USAGE.lock().unwrap();
                println!("{name}: {} 行，{:.1} 秒，{} 个请求，词元 {} + {}{}", batch.rows.len(), started.elapsed().as_secs_f64(), after.2 - before.2, after.0 - before.0, after.1 - before.1, if errors.is_empty() { String::new() } else { format!("，出错 {errors:?}") });
                report["files"][&name] = json!({"seconds": started.elapsed().as_secs_f64(), "requests": after.2 - before.2, "prompt": after.0 - before.0, "completion": after.1 - before.1, "source": batch.summary.source, "errors": errors});
            }
            Err(e) => { println!("{name}: 失败：{e}"); report["errors"].as_array_mut().unwrap().push(json!({"file": name, "error": e})); }
        }
    }
    // 查重: batch by batch, as the app does
    let started = std::time::Instant::now();
    let before = *USAGE.lock().unwrap();
    let rows = open_rows(&c, &ids, None).unwrap();
    let jobs = match_jobs(&c, &rows).unwrap();
    println!("查重：{} 块，账本最多 {} 条", jobs.len(), jobs.iter().map(|j| j.prompt["ledger"].as_array().map_or(0, Vec::len)).max().unwrap_or(0));
    let results = rt.block_on(run_jobs(&m, &system(MATCH, &points), jobs, |job, part| Job { batch: job.batch, rows: part.to_vec(), prompt: json!({"rows": job.prompt["rows"].as_array().unwrap().iter().filter(|r| r["id"].as_i64().is_some_and(|i| part.contains(&i))).cloned().collect::<Vec<_>>(), "ledger": job.prompt["ledger"]}) }, |_| vec![]));
    let summary = apply_match(&c, &rows, &m.name, results).unwrap();
    let after = *USAGE.lock().unwrap();
    println!("查重：合并 {}，待看 {}，出错 {:?}，{:.1} 秒，{} 个请求，词元 {} + {}", summary.linked, summary.review, summary.errors, started.elapsed().as_secs_f64(), after.2 - before.2, after.0 - before.0, after.1 - before.1);
    report["match"] = json!({"linked": summary.linked, "review": summary.review, "errors": summary.errors, "seconds": started.elapsed().as_secs_f64(), "requests": after.2 - before.2, "prompt": after.0 - before.0, "completion": after.1 - before.1});
    if let Ok(name) = std::env::var("POMELO_IMPORTER_AGAIN") {
        let id = batch_named(&c, &name);
        let open: Vec<i64> = batch(&c, id).unwrap().rows.iter().filter(|r| r.state == "pending").map(|r| r.id).collect();
        for r in &open { c.execute("UPDATE fin_import_rows SET resolution = 'new' WHERE id = ?1", [r]).unwrap(); }
        commit(&c, id, &open).unwrap();
        let source = read_source(&format!("{dir}/{name}")).unwrap();
        let (parsed, _) = rt.block_on(read_statement(&m, &name, &source, &points, &kinds, &today)).unwrap();
        let again = ingest(&c, &format!("再一次 {name}"), parsed, Some(&m.name)).unwrap();
        ids.push(again.summary.id);
        let rows = open_rows(&c, &[again.summary.id], None).unwrap();
        let jobs = match_jobs(&c, &rows).unwrap();
        let results = rt.block_on(run_jobs(&m, &system(MATCH, &points), jobs, |job, _| job.clone(), |_| vec![]));
        let s = apply_match(&c, &rows, &m.name, results).unwrap();
        println!("再导入 {name}：{} 行，合并 {}，待看 {}", rows.len(), s.linked, s.review);
        report["again"] = json!({"rows": rows.len(), "linked": s.linked, "review": s.review});
    }
    if let Ok(point) = std::env::var("POMELO_IMPORTER_TIDY") {
        let id = batch_named(&c, &std::env::var("POMELO_IMPORTER_TIDY_FILE").unwrap());
        let mut more = points.clone();
        more.push(Point { text: point, on: true });
        let before = open_rows(&c, &[id], None).unwrap();
        let pending: Vec<&ImportRow> = before.iter().filter(|o| o.row.state == "pending").map(|o| &o.row).collect();
        let job = Job { batch: 0, rows: pending.iter().map(|r| r.id).collect(), prompt: json!({"rows": pending.iter().map(|r| tidy_row(r)).collect::<Vec<_>>(), "categories": kinds}) };
        let results = rt.block_on(run_jobs(&m, &system(TIDY, &more), vec![job], |job, _| job.clone(), |_| vec![]));
        let s = apply_tidy(&c, &before, &m.name, &kinds, results).unwrap();
        println!("再整理：{} 笔，待看 {}，出错 {:?}", s.tidied, s.review, s.errors);
        report["tidy"] = json!({"tidied": s.tidied, "review": s.review, "errors": s.errors, "rows": batch(&c, id).unwrap().rows.iter().map(|r| json!({"day": r.draft.day, "amount": r.draft.amount, "treatment": r.draft.treatment, "category": r.draft.category, "note": r.draft.note, "settled": r.ai.as_ref().map(|a| a.settled), "why": r.ai.as_ref().and_then(|a| a.why.clone()), "state": r.state})).collect::<Vec<_>>()});
    }
    let mut all = serde_json::Map::new();
    for id in &ids {
        let b = batch(&c, *id).unwrap();
        all.insert(b.summary.name.clone(), serde_json::to_value(&b.rows).unwrap());
    }
    report["rows"] = Value::Object(all);
    report["total"] = json!({"prompt": after.0, "completion": after.1, "requests": after.2});
    std::fs::write(out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
}
