use super::card::{cancel, confirm, Pick};
use super::ops::Written;
use super::*;
use crate::modules::common::add_days;
use crate::modules::{books, drawing, jobs, notes, packing, todo};
use rusqlite::Connection;

fn seeded() -> Connection {
    let c = crate::db::tests::memory_db();
    c.execute_batch(
        "INSERT INTO todo_groups (name, sort_order) VALUES ('生活', 1), ('学业', 2);
         INSERT INTO fin_entries (flow, amount, currency, day) VALUES ('out', 100, 'CNY', date('now', 'localtime'));",
    )
    .unwrap();
    c
}

fn patch_of(p: &Proposal) -> &Value {
    match &p.choices[0].op {
        Op::AddEntry { patch } | Op::AddTask { patch } | Op::AddJob { patch } | Op::AddNote { patch } | Op::AddWork { patch, .. } | Op::AddTrip { patch, .. } => patch,
        Op::EditEntry { patch, .. } | Op::EditJob { patch, .. } | Op::EditWork { patch, .. } | Op::EditLesson { patch, .. } | Op::EditNote { patch, .. } => patch,
        other => panic!("no patch: {other:?}"),
    }
}

fn count(c: &Connection, table: &str) -> i64 {
    c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap()
}

/// Proposes and writes the (only) choice like confirming would.
fn write(c: &Connection, tool: &str, args: &str, question: &str) -> (Proposal, Written) {
    let today = todo::today(c).unwrap();
    let p = propose(c, tool, args, question, &today).unwrap_or_else(|e| panic!("{tool} {args}: {e}"));
    assert_eq!(p.choices.len(), 1, "{tool}: {:?}", p.choices.iter().map(|c| &c.title).collect::<Vec<_>>());
    let w = ops::apply(c, &p.choices[0].op, &today).unwrap();
    (p, w)
}

fn field<'a>(p: &'a Proposal, label: &str) -> &'a Field {
    p.choices[0].fields.iter().find(|f| f.label == label).unwrap_or_else(|| panic!("no {label}: {:?}", p.choices[0].fields))
}

#[test]
fn a_proposal_is_checked_but_never_written() {
    let c = seeded();
    let today = todo::today(&c).unwrap();
    let p = propose(&c, ADD_ENTRY, r#"{"amount": 35, "note": "午饭", "kind": "餐饮"}"#, "午饭 35", &today).unwrap();
    assert_eq!(patch_of(&p)["amount"], 3500);
    assert_eq!((patch_of(&p)["currency"].as_str(), patch_of(&p)["day"].as_str()), (Some("CNY"), Some(today.as_str()))); // usual currency, today
    assert!(p.choices[0].title.contains("午饭 ¥35"), "{}", p.choices[0].title);
    assert_eq!(p.label, "记一笔收支");
    assert_eq!(count(&c, "fin_entries"), 1); // the trial left nothing
    // the module's own rules refuse what it would refuse
    assert!(propose(&c, ADD_ENTRY, r#"{"note": "午饭"}"#, "午饭", &today).unwrap_err().contains("金额"));
    assert!(propose(&c, ADD_ENTRY, r#"{"amount": -5}"#, "x", &today).is_err());
    // a date word in the question wins over the model's arithmetic
    let y = add_days(&today, -1);
    let p = propose(&c, ADD_ENTRY, r#"{"amount": 20, "day": "2020-01-01"}"#, "昨天打车 20", &today).unwrap();
    assert_eq!(patch_of(&p)["day"].as_str(), Some(y.as_str()));
    // …but not where the model said today (「午饭 35，昨天打车 20」: the 午饭 is today's)
    let p = propose(&c, ADD_ENTRY, &format!(r#"{{"amount": 35, "day": "{today}"}}"#), "午饭 35，昨天打车 20", &today).unwrap();
    assert_eq!(patch_of(&p)["day"].as_str(), Some(today.as_str()));
    assert!(answer(ADD_ENTRY, Ok((1, &p))).text.contains("（收支 支出；金额 ¥35；"), "{}", answer(ADD_ENTRY, Ok((1, &p))).text);

    let t = propose(&c, ADD_TASK, r#"{"title": "交房租", "due_date": "2020-01-01"}"#, "提醒我明天交房租", &today).unwrap();
    assert_eq!(patch_of(&t)["groupId"], 1); // no group named: the first
    assert_eq!(patch_of(&t)["dueDate"].as_str(), Some(add_days(&today, 1).as_str()));
    let t = propose(&c, ADD_TASK, r#"{"title": "复习", "group": "学业"}"#, "学业里加个复习", &today).unwrap();
    assert_eq!(patch_of(&t)["groupId"], 2);
    assert!(t.choices[0].fields.iter().any(|f| f.label == "在" && f.after == "学业"));
    assert_eq!(count(&c, "todo_tasks"), 0);
    assert!(propose(&c, ADD_TASK, r#"{"due_date": "2026-10-20"}"#, "x", &today).is_err());
}

#[test]
fn cards_keep_their_items_and_the_rules_refuse() {
    let p = crate::picky::store::tests::memory_db();
    p.execute_batch("INSERT INTO sessions (title) VALUES ('t'); INSERT INTO runs (session_id, model, model_name, state) VALUES (1, 'local:x', 'X', 'completed');").unwrap();
    let choice = |t: &str| Choice { title: t.into(), fields: vec![Field { label: "金额".into(), before: None, after: "¥35".into(), long: false }], op: Op::AddEntry { patch: json!({"amount": 3500}) } };
    let prop = Proposal { tool: ADD_ENTRY.into(), module: "finance".into(), label: "记一笔收支".into(), choices: vec![choice("午饭 ¥35")], more: 0, note: None };
    assert_eq!(add(&p, 1, &prop).unwrap(), 1);
    let two = Proposal { tool: EDIT_ENTRY.into(), choices: vec![choice("10/13 午饭 ¥35"), choice("10/14 午饭 ¥30")], ..prop.clone() };
    assert_eq!(add(&p, 1, &two).unwrap(), 2);
    let card = card_of(&p, 1).unwrap().unwrap();
    assert_eq!((card.state.as_str(), card.items.len(), card.items[1].checked), ("pending", 2, true));
    assert_eq!(card.items[1].options.as_ref().map(|o| o.len()), Some(2));
    assert_eq!((card.items[1].title.as_str(), card.items[1].label.as_str()), ("2 条待选", "改一笔收支"));
    assert_eq!(cancel(&p, 1).unwrap().state, "cancelled");
    assert!(cancel(&p, 1).is_err());
    assert!(card_of(&p, 9).unwrap().is_none());

    let cloud = Access { cloud: true, cloud_allowed: vec!["todo".into(), "books".into()], ..Access::default() };
    let r = refused(&cloud, EDIT_ENTRY, "{}").unwrap();
    assert!(r.state == "blocked" && r.text.contains("「钱来钱往不发给云端，这次不能记」"));
    assert!(refused(&cloud, ADD_TASK, "{}").is_none());
    assert!(refused(&cloud, ADD_WORK, r#"{"kind": "book"}"#).is_none());
    assert!(refused(&cloud, ADD_WORK, r#"{"kind": "film"}"#).is_some()); // not allowed in this setting
    assert!(refused(&Access { hidden: vec!["todo".into()], ..Access::default() }, EDIT_TASK, "{}").is_some());
    let names = |a: &Access| tools(a).iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(names(&Access::default()).len(), ACTS.len());
    assert_eq!(names(&Access { hidden: vec!["finance".into()], ..Access::default() }).len(), ACTS.len() - 3);
    let no_books = tools(&Access { hidden: vec!["books".into()], ..Access::default() });
    let work = no_books.iter().find(|t| t["function"]["name"] == ADD_WORK).unwrap();
    assert_eq!(work["function"]["parameters"]["properties"]["kind"]["enum"], json!(["film"]));
    let pomelo = seeded();
    let text = instructions(&pomelo, &cloud, "2026-10-14").unwrap();
    assert!(text.contains("新任务") && !text.contains("记一笔收支") && text.contains("摸鱼读读：类型") && !text.contains("钱来钱往："), "{text}");
}

#[test]
fn confirming_writes_the_ticked_items_in_one_undo_step() {
    let dir = crate::db::tests::temp_dir("act-confirm");
    let st = crate::AppState::init(crate::paths::DataPaths::new(dir.to_path_buf()), &dir, crate::picky::Mode::Mock);
    st.with_db(|c| c.execute_batch("INSERT INTO todo_groups (name, sort_order) VALUES ('生活', 1); INSERT INTO todo_tasks (group_id, title, sort_order) VALUES (1, '交房租', 1);").map_err(crate::modules::common::err)).unwrap();
    st.picky
        .with_db(|c| c.execute_batch("INSERT INTO sessions (title) VALUES ('t'); INSERT INTO runs (session_id, model, model_name, state) VALUES (1, 'local:x', 'X', 'completed');").map_err(crate::modules::common::err))
        .unwrap();
    let today = st.with_db(|c| todo::today(c)).unwrap();
    for (tool, args, q) in [
        (ADD_ENTRY, r#"{"amount": 35, "note": "午饭"}"#, "午饭 35"),
        (EDIT_TASK, r#"{"task": "交房租", "done": true}"#, "交房租做完了"),
        (ADD_ENTRY, r#"{"amount": 20}"#, "打车 20"),
    ] {
        let p = st.with_db(|c| propose(c, tool, args, q, &today)).unwrap();
        st.picky.with_db(|c| add(c, 1, &p)).unwrap();
    }
    let n = |t: &str| st.with_db(|c| c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get::<_, i64>(0)).map_err(crate::modules::common::err)).unwrap();
    let done = || st.with_db(|c| Ok(todo::task(c, 1)?.done)).unwrap();
    assert!(confirm(&st, 1, &[]).is_err());
    let picks = [Pick { position: 0, option: None }, Pick { position: 1, option: None }]; // the third unticked
    let card = confirm(&st, 1, &picks).unwrap();
    assert_eq!(card.state, "confirmed");
    assert_eq!(card.items.iter().map(|i| (i.checked, i.written.is_some())).collect::<Vec<_>>(), [(true, true), (true, true), (false, false)]);
    assert_eq!((n("fin_entries"), done()), (1, true));
    assert!(confirm(&st, 1, &picks[..1]).is_err()); // once only
    // one ⌘Z takes both back; the card then shows them as gone (the edit: back to how it was)
    assert_eq!(crate::undo::step_back(&st, false).unwrap().label.as_deref(), Some("小拾：卡片里的 2 条"));
    assert_eq!((n("fin_entries"), done()), (0, false));
    let mut card = st.picky.with_db(|c| card_of(c, 1)).unwrap().unwrap();
    st.with_db(|c| Ok(mark_gone(c, &mut card))).unwrap();
    assert!(card.items[..2].iter().all(|i| i.written.as_ref().unwrap().gone));
    crate::undo::step_back(&st, true).unwrap(); // redo: there again
    st.with_db(|c| Ok(mark_gone(c, &mut card))).unwrap();
    assert!(card.items[..2].iter().all(|i| !i.written.as_ref().unwrap().gone));
}

#[test]
fn an_edit_with_several_fits_waits_for_a_pick() {
    let dir = crate::db::tests::temp_dir("act-pick");
    let st = crate::AppState::init(crate::paths::DataPaths::new(dir.to_path_buf()), &dir, crate::picky::Mode::Mock);
    st.with_db(|c| c.execute_batch("INSERT INTO fin_entries (flow, amount, currency, day, note) VALUES ('out', 3500, 'CNY', date('now', 'localtime'), '午饭'), ('out', 3000, 'CNY', date('now', 'localtime', '-1 day'), '午饭');").map_err(crate::modules::common::err)).unwrap();
    st.picky
        .with_db(|c| c.execute_batch("INSERT INTO sessions (title) VALUES ('t'); INSERT INTO runs (session_id, model, model_name, state) VALUES (1, 'local:x', 'X', 'completed');").map_err(crate::modules::common::err))
        .unwrap();
    let today = st.with_db(|c| todo::today(c)).unwrap();
    let p = st.with_db(|c| propose(c, EDIT_ENTRY, r#"{"find": "午饭", "amount": 40}"#, "午饭改成 40", &today)).unwrap();
    assert_eq!(p.choices.len(), 2);
    assert!(p.choices[0].title.contains("午饭 ¥35：金额 ¥35 → ¥40"), "{}", p.choices[0].title); // newest first
    assert!(answer(EDIT_ENTRY, Ok((1, &p))).text.contains("找到 2 条可能是的"));
    st.picky.with_db(|c| add(c, 1, &p)).unwrap();
    assert!(confirm(&st, 1, &[Pick { position: 0, option: None }]).unwrap_err().contains("还没选"));
    let card = confirm(&st, 1, &[Pick { position: 0, option: Some(1) }]).unwrap();
    assert_eq!(card.items[0].chosen, Some(1));
    assert!(card.items[0].title.contains("¥30 → ¥40"), "{}", card.items[0].title);
    let amounts: Vec<i64> = st.with_db(|c| Ok(c.prepare("SELECT amount FROM fin_entries ORDER BY day DESC").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect())).unwrap();
    assert_eq!(amounts, [3500, 4000]);
    // a date says which one; nothing fits: the model is told to ask
    let one = st.with_db(|c| propose(c, EDIT_ENTRY, r#"{"find": "午饭", "on": "x", "amount": 50}"#, "昨天的午饭改成 50", &today)).unwrap();
    assert_eq!(one.choices.len(), 1);
    let none = st.with_db(|c| propose(c, EDIT_ENTRY, r#"{"find": "晚饭", "amount": 50}"#, "晚饭改成 50", &today)).unwrap_err();
    assert!(none.contains("请问用户"), "{none}");
}

#[test]
fn every_module_adds_and_edits_with_its_own_rules() {
    let c = seeded();
    let today = todo::today(&c).unwrap();
    // 钱来钱往
    let (p, _) = write(&c, EDIT_ENTRY, r#"{"was": 1, "kind": "交通"}"#, "那笔 1 块的改成交通");
    assert_eq!((field(&p, "分类").before.as_deref(), field(&p, "分类").after.as_str()), (Some("未分类"), "交通"));
    c.execute("INSERT INTO fin_accounts (name, currency, sort_order) VALUES ('招行', 'CNY', 1)", []).unwrap();
    let (p, w) = write(&c, SET_BALANCE, r#"{"account": "招行", "amount": 1200.5}"#, "招行余额 1200.5");
    assert_eq!((field(&p, "余额").before.as_deref(), field(&p, "余额").after.as_str(), w.kind.as_str()), (Some("（空）"), "¥1,200.50", "account"));
    assert!(propose(&c, SET_BALANCE, r#"{"account": "花旗", "amount": 1}"#, "x", &today).unwrap_err().contains("招行"));

    // 一步一步: done, a progress amount, dates, groups, a subtask
    let (_, w) = write(&c, ADD_TASK, r#"{"title": "写论文"}"#, "新任务 写论文");
    let (p, _) = write(&c, ADD_TASK, r#"{"title": "列提纲", "parent": "论文"}"#, "给写论文加个子任务列提纲");
    assert_eq!((p.label.as_str(), field(&p, "在").after.as_str()), ("加子任务", "生活 / 写论文"));
    let (p, _) = write(&c, EDIT_TASK, r#"{"task": "写论文", "group": "学业", "due_date": "x"}"#, "写论文挪到学业，下周五截止");
    assert_eq!(field(&p, "在").after, "学业");
    assert!(field(&p, "时间").after.starts_with("截止"));
    let (p, _) = write(&c, EDIT_TASK, r#"{"task": "列提纲", "done": true}"#, "列提纲做完了");
    assert_eq!(field(&p, "状态").after, "已完成");
    assert!(todo::task(&c, w.id).unwrap().done); // its only subtask done: the parent too
    assert!(propose(&c, EDIT_TASK, r#"{"task": "列提纲", "done": true}"#, "x", &today).unwrap_err().contains("没找到")); // done ones aren't looked at
    let prog = todo::create_task(&c, serde_json::from_value(json!({"groupId": 1, "title": "背单词", "taskType": "progress", "target": 100, "unit": "个"})).unwrap()).unwrap();
    let (p, _) = write(&c, EDIT_TASK, r#"{"task": "背单词", "amount": 30}"#, "背单词记 30");
    assert_eq!((field(&p, "状态").before.as_deref(), field(&p, "状态").after.as_str()), (Some("0 / 100个"), "30 / 100个"));
    assert_eq!(todo::task_view(&c, prog.id).unwrap().count, 30);

    // 投石问路
    let (p, w) = write(&c, ADD_JOB, r#"{"company": "甲公司", "position": "产品经理", "status": "已投递"}"#, "投了甲公司的产品经理");
    assert!(field(&p, "进度").after.starts_with("已投递"));
    let (p, _) = write(&c, EDIT_JOB, r#"{"job": "甲公司", "status": "面试", "note": "一面在周五"}"#, "甲公司进面试了");
    assert!(field(&p, "进度").before.as_deref().unwrap().starts_with("已投递") && field(&p, "进度").after.starts_with("面试"));
    assert_eq!(jobs::job(&c, w.id, &today).unwrap().note.as_deref(), Some("一面在周五"));
    write(&c, EDIT_JOB, r#"{"job": "产品经理", "note": "二面"}"#, "x");
    assert_eq!(jobs::job(&c, w.id, &today).unwrap().note.as_deref(), Some("一面在周五\n二面")); // added below
    assert!(propose(&c, EDIT_JOB, r#"{"job": "甲公司", "status": "终面"}"#, "x", &today).unwrap_err().contains("没有「终面」"));
    let (p, _) = write(&c, ADD_JOB, r#"{"company": "戊公司", "status": "投了"}"#, "投了戊公司");
    assert!(field(&p, "进度").after.starts_with("已投递"), "{:?}", p.choices[0].fields);
    assert!(propose(&c, ADD_JOB, r#"{"company": "己公司", "status": "终面"}"#, "x", &today).unwrap_err().contains("没有「终面」"));
    let (p, _) = write(&c, ADD_JOB, r#"{"company": "庚公司"}"#, "投了庚公司"); // no step given, but 投了 said
    assert!(field(&p, "进度").after.starts_with("已投递"));
    // a date the model got wrong: the card uses Rust's, and the model is told
    let p = propose(&c, EDIT_JOB, r#"{"job": "甲公司", "status": "Offer", "day": "2020-01-01"}"#, "甲公司下周三拿到 offer", &today).unwrap();
    assert!(answer(EDIT_JOB, Ok((1, &p))).text.contains("问题里的日期是"));

    // 摸鱼读读 · 好奇看看
    let (_, w) = write(&c, ADD_WORK, r#"{"kind": "book", "title": "《三体》", "status": "todo"}"#, "想读《三体》");
    assert!(propose(&c, ADD_WORK, r#"{"kind": "book", "title": "三体"}"#, "x", &today).unwrap_err().contains("已经在摸鱼读读里了"));
    let (p, _) = write(&c, EDIT_WORK, r#"{"kind": "book", "title": "三体", "status": "done", "rating": 5}"#, "读完了《三体》五星");
    assert_eq!((field(&p, "状态").after.as_str(), field(&p, "星级").after.as_str()), ("读完了", "5 星"));
    assert_eq!(books::get(&c, w.id).unwrap().finished_year.as_deref(), Some(&today[..4])); // the module's own rule
    assert!(propose(&c, EDIT_WORK, r#"{"kind": "film", "title": "三体", "status": "done"}"#, "x", &today).is_err());

    // 随意画画: the next lesson not done, or one by its number
    let course = drawing::create_course(&c, serde_json::from_value(json!({"name": "素描入门"})).unwrap()).unwrap().id;
    for (code, title) in [("1", "线条"), ("2", "明暗"), ("3", "构图")] {
        drawing::create_lesson(&c, course, serde_json::from_value(json!({"code": code, "title": title})).unwrap()).unwrap();
    }
    let (p, _) = write(&c, FINISH_LESSON, r#"{"course": "素描"}"#, "画完了素描");
    assert_eq!(field(&p, "课节").after, "1 线条");
    let (p, _) = write(&c, FINISH_LESSON, r#"{"course": "素描入门", "lesson": "第3课"}"#, "x");
    assert_eq!((field(&p, "课节").after.as_str(), field(&p, "状态").after.as_str()), ("3 构图", "画完了"));

    // 出门走走
    let (_, w) = write(&c, ADD_TRIP, r#"{"name": "东京五天", "depart_on": "x"}"#, "新出行 东京五天，下周五出发");
    assert_eq!(packing::trip(&c, w.id).unwrap().depart_on, Some(assist::date_in("下周五", &today).unwrap()));
    let (p, _) = write(&c, EDIT_TRIP, r#"{"add": ["雨伞", "充电宝"]}"#, "加上雨伞、充电宝");
    assert_eq!(field(&p, "加上").after, "雨伞、充电宝");
    let (p, _) = write(&c, EDIT_TRIP, r#"{"trip": "东京", "pack": ["雨伞", "护照"], "add": ["雨伞"]}"#, "装好了雨伞");
    assert_eq!(field(&p, "装好").after, "雨伞");
    assert!(answer(EDIT_TRIP, Ok((1, &p))).text.contains("没找到（或已经装好了）：护照"));
    assert_eq!(packing::trip(&c, w.id).unwrap().packed, 1);

    // 念念不忘: the changes marked
    let (_, w) = write(&c, ADD_NOTE, r#"{"content": "周末去看了两套房 #租房"}"#, "记一下：周末去看了两套房 #租房");
    let (p, _) = write(&c, EDIT_NOTE, r#"{"find": "看了两套房", "old": "两套", "new": "三套"}"#, "笔记里的两套改成三套");
    let f = field(&p, "内容");
    assert!(f.long && f.before.as_deref() == Some("周末去看了两套房 #租房") && f.after == "周末去看了三套房 #租房");
    assert_eq!(notes::get(&c, w.id).unwrap().content, "周末去看了三套房 #租房");
    assert!(propose(&c, EDIT_NOTE, r#"{"find": "看了", "old": "四套", "new": "五套"}"#, "x", &today).is_err());
}

#[test]
fn words_find_their_records() {
    let items = vec!["交房租", "交房租提醒", "写论文初稿", "论文答辩"];
    let fit = |q: &str| make::fit(items.clone(), |s| s.to_string(), q);
    assert_eq!(fit("交房租"), ["交房租"]); // the whole name wins
    assert_eq!(fit("论文"), ["写论文初稿", "论文答辩"]);
    assert_eq!(fit("论文 初稿"), ["写论文初稿"]);
    assert_eq!(fit("写论文的初稿"), ["写论文初稿"]); // most character pairs
    assert!(fit("买菜").is_empty());
}

#[test]
fn the_mock_model_proposes_by_plain_patterns() {
    let today = "2026-10-14";
    let calls = |q: &str| mock_calls(q, "支出分类 餐饮、交通；收入分类 工资", today).into_iter().map(|(t, a)| format!("{t} {a}")).collect::<Vec<_>>();
    assert_eq!(calls("午饭 35，昨天打车 20").len(), 2);
    assert_eq!(calls("给写论文加个子任务列提纲"), [r#"pomelo_add_task {"parent":"写论文","title":"列提纲"}"#]);
    assert_eq!(calls("交房租做完了"), [r#"pomelo_edit_task {"done":true,"task":"交房租"}"#]);
    assert_eq!(calls("甲公司进面试了"), [r#"pomelo_edit_job {"job":"甲公司","status":"面试"}"#]);
    assert_eq!(calls("读完了《三体》五星"), [r#"pomelo_edit_work {"kind":"book","rating":5,"status":"done","title":"三体"}"#]);
    assert_eq!(calls("想看《沙丘》"), [r#"pomelo_add_work {"kind":"film","status":"todo","title":"沙丘"}"#]);
    assert_eq!(calls("把昨天的午饭改成 40"), [r#"pomelo_edit_entry {"amount":40.0,"find":"午饭","on":"2026-10-13"}"#]);
    assert_eq!(calls("招行余额 5000"), [r#"pomelo_set_balance {"account":"招行","amount":5000.0}"#]);
    assert_eq!(calls("装好了雨伞、护照"), [r#"pomelo_edit_trip {"pack":["雨伞","护照"]}"#]);
    assert_eq!(calls("笔记里的两套改成三套"), [r#"pomelo_edit_note {"find":"两套","new":"三套","old":"两套"}"#]);
    assert!(calls("推荐几本像《三体》的书").is_empty());
    assert!(calls("这周做完了什么").is_empty());
}

/// What the write tools take in the prompt (the estimate): cargo test tool_sizes -- --ignored --nocapture
#[test]
#[ignore]
fn tool_sizes() {
    let all = tools(&Access::default());
    let est = |v: &Value| crate::picky::context::estimate(&v.to_string());
    for t in &all {
        println!("{} {}", t["function"]["name"], est(t));
    }
    println!("total {} (read tools {})", est(&json!(all)), crate::picky::context::estimate(&serde_json::to_string(&crate::picky::data::tools(&Access::default())).unwrap()) - est(&json!(all)));
}
