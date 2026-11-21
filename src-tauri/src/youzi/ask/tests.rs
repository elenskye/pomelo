use super::*;
use crate::youzi::chat::{begin_run, finish, path, Outcome};
use crate::youzi::store::tests::memory_db;

fn card(c: &Connection) -> (i64, Ask) {
    let b = begin_run(c, None, None, "问题", "cloud:x", "X").unwrap();
    let q = parse(&json!({ "questions": [{ "question": "选哪个？", "kind": "single", "options": ["甲", "乙"] }, { "question": "要写进笔记吗？", "kind": "judge" }] }).to_string()).unwrap();
    let a = save(c, b.run_id, &q).unwrap();
    (b.session_id, a)
}

#[test]
fn questions_are_checked_and_what_is_wrong_goes_back_to_the_model() {
    let q = parse(r#"{"questions":[{"question":" 选哪个？ ","kind":"multi","options":["甲"," 乙 ",""]},{"question":"对吗？","kind":"judge","options":["错的不算"]},{"question":"没写 kind 也行"}]}"#).unwrap();
    assert_eq!((q[0].question.as_str(), q[0].options.clone()), ("选哪个？", vec!["甲".to_string(), "乙".to_string()]));
    assert_eq!(q[1].options, ["是", "否"]);
    assert_eq!(q[2].kind, "judge");
    for bad in [
        "不是 JSON",
        r#"{"questions":[]}"#,
        r#"{"questions":[{"kind":"judge"}]}"#,
        r#"{"questions":[{"question":"只有一个选项","kind":"single","options":["甲"]}]}"#,
        r#"{"questions":[{"question":"重复","kind":"single","options":["甲","甲"]}]}"#,
        r#"{"questions":[{"question":"种类不对","kind":"essay"}]}"#,
        r#"{"questions":[{"question":"1","kind":"judge"},{"question":"2","kind":"judge"},{"question":"3","kind":"judge"},{"question":"4","kind":"judge"},{"question":"5","kind":"judge"}]}"#,
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
    let mut many: Vec<String> = (0..7).map(|i| format!("选项{i}")).collect();
    assert!(parse(&json!({ "questions": [{ "question": "多了", "kind": "single", "options": many }] }).to_string()).is_err());
    many.truncate(6);
    assert!(parse(&json!({ "questions": [{ "question": "刚好", "kind": "single", "options": many }] }).to_string()).is_ok());
}

#[test]
fn an_answer_is_what_the_card_offered_or_my_own_words_once() {
    let c = memory_db();
    let (_, a) = card(&c);
    let pick = |p: &[&str], o: &str| Answer { picked: p.iter().map(|s| s.to_string()).collect(), other: o.into() };
    for bad in [vec![pick(&["甲"], "")], vec![pick(&[], ""), pick(&["是"], "")], vec![pick(&["丙"], ""), pick(&["是"], "")], vec![pick(&["甲", "乙"], ""), pick(&["是"], "")], vec![pick(&["甲"], &"长".repeat(1001)), pick(&["是"], "")]] {
        assert!(record_answer(&c, a.id, &bad).is_err());
    }
    assert_eq!(get(&c, a.id).unwrap().state, "waiting");
    let (run, text) = record_answer(&c, a.id, &[pick(&["乙"], " 再便宜点 "), pick(&[], "先不用")]).unwrap();
    assert_eq!(run, a.run_id);
    assert_eq!(text, "我的回答：\n选哪个？\n→ 乙、再便宜点\n要写进笔记吗？\n→ 先不用");
    let now = get(&c, a.id).unwrap();
    assert_eq!((now.state.as_str(), now.answers.as_ref().unwrap()[1].other.as_str()), ("answered", "先不用"));
    assert!(record_answer(&c, a.id, &[pick(&["甲"], ""), pick(&["是"], "")]).is_err());
    // 多选 takes several
    let q = parse(r#"{"questions":[{"question":"多选","kind":"multi","options":["甲","乙"]}]}"#).unwrap();
    assert!(answer_text(&q, &[pick(&["甲", "乙"], "")]).unwrap().ends_with("→ 甲、乙"));
}

#[test]
fn the_card_stays_with_the_reply_goes_with_it_and_closes_when_the_talk_moves_on() {
    let c = memory_db();
    let (session, a) = card(&c);
    finish(&c, a.run_id, reply_of(&c, a.run_id), &Outcome { content: "前面写的".into(), state: "interrupted", ..Default::default() }).unwrap();
    // after a quit: still there, still waiting
    let shown = path(&c, session).unwrap();
    assert_eq!(shown.last().unwrap().asks[0].state, "waiting");
    // a new question closes it
    begin_run(&c, Some(session), None, "新问题", "cloud:x", "X").unwrap();
    assert_eq!(get(&c, a.id).unwrap().state, "cancelled");
    // answered: later questions see it with the reply
    let (session, a) = card(&c);
    let (_, _) = record_answer(&c, a.id, &[Answer { picked: vec!["甲".into()], other: String::new() }, Answer { picked: vec!["否".into()], other: String::new() }]).unwrap();
    finish(&c, a.run_id, reply_of(&c, a.run_id), &Outcome { content: "按甲做了".into(), state: "completed", ..Default::default() }).unwrap();
    let next = begin_run(&c, Some(session), None, "接着", "cloud:x", "X").unwrap();
    let history = next.request.history.iter().find(|m| m.role == "assistant").unwrap();
    assert!(history.content.contains("选哪个？\n→ 甲") && history.content.ends_with("按甲做了"));
    let out = block(&path(&c, session).unwrap().iter().flat_map(|m| m.asks.clone()).collect::<Vec<_>>());
    assert!(out.contains("- 问我：选哪个？\n  答：甲") && out.contains("- 问我：要写进笔记吗？\n  答：否"));
    // 停止 closes it; deleting the conversation takes it along
    let (session, a) = card(&c);
    cancel_run(&c, a.run_id).unwrap();
    assert!(record_answer(&c, a.id, &[Answer { picked: vec!["甲".into()], other: String::new() }, Answer { picked: vec!["是".into()], other: String::new() }]).is_err());
    c.execute("DELETE FROM sessions WHERE id = ?1", [session]).unwrap();
    assert_eq!(c.query_row::<i64, _, _>("SELECT count(*) FROM run_asks WHERE id = ?1", [a.id], |r| r.get(0)).unwrap(), 0);
}

fn reply_of(c: &Connection, run: i64) -> i64 {
    c.query_row("SELECT id FROM messages WHERE run_id = ?1 AND role = 'assistant'", [run], |r| r.get(0)).unwrap()
}

#[test]
fn only_so_many_asks_in_a_reply() {
    let c = memory_db();
    let b = begin_run(&c, None, None, "问题", "cloud:x", "X").unwrap();
    let q = parse(r#"{"questions":[{"question":"对吗？","kind":"judge"}]}"#).unwrap();
    for _ in 0..MAX_PER_REPLY { save(&c, b.run_id, &q).unwrap(); }
    assert!(save(&c, b.run_id, &q).unwrap_err().contains("不能再问了"));
}

#[test]
fn the_reply_waits_at_the_gate_until_the_answer_comes() {
    let gate = std::sync::Arc::new(Gate::default());
    assert!(!gate.waiting() && !gate.answer(1, "没人等".into()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let rx = gate.open(7);
        assert!(gate.waiting());
        assert!(!gate.answer(8, "别的卡片".into()));
        let g = gate.clone();
        tokio::spawn(async move { assert!(g.answer(7, "我的回答".into())); });
        assert_eq!(rx.await.unwrap(), "我的回答");
    });
    assert!(!gate.waiting());
    // 停止 ends the task: nothing is left to answer
    drop(gate.open(9));
    assert!(!gate.answer(9, "太晚了".into()));
}

/// With real cloud models, on made-up questions: asks when it must, not when it needn't, and carries on with the answer.
///   POMELO_CLOUD_KEY_FILE=… [POMELO_CLOUD_SERVICE=…] [POMELO_CLOUD_MODEL=…] [POMELO_CLOUD_THINK=always] [POMELO_ASK_OUT=<file>]
///   cargo test real_ask -- --ignored --nocapture
#[test]
#[ignore = "real cloud model: ask first"]
fn real_ask() {
    use crate::youzi::provider::{self, Event, Msg, Options};
    let Some((target, _)) = crate::youzi::chat::tests::cloud_target() else { return eprintln!("needs POMELO_CLOUD_KEY_FILE") };
    let cases = [
        ("该问", "帮我写一封请假邮件发给主管。开始写之前，先用工具问我：请几天、什么理由。"),
        ("该问", "我要订下周去东京的机票，帮我定个方案。我还没想好是省钱优先还是省时间优先，这得我来定。"),
        ("判断题", "我打算把项目周报从周五改到周一发。你判断不了我们团队的习惯，先问我一个是或否的问题确认团队接不接受，再给建议。"),
        ("不该问", "1 + 1 等于几？"),
        ("不该问", "把「今天天气很好」翻译成英文。"),
    ];
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let mut report = String::new();
    for (expect, q) in cases {
        let mut messages = vec![Msg::new("system", "你是柚子，用中文回答。"), Msg::new("user", q)];
        let (mut asked, mut said, mut bad) = (0, String::new(), String::new());
        for _ in 0..4 {
            let (mut text, mut calls) = (String::new(), vec![]);
            let opts = Options { max_tokens: Some(4096), thinking: Some(false), tools: vec![tool()], ..Default::default() };
            let result = rt.block_on(provider::stream(target.clone(), messages.clone(), opts, |e| match e {
                Event::Content(t) => text.push_str(&t),
                Event::ToolCalls(c) => calls = c,
                _ => {}
            }));
            if let Err(e) = result { bad = e; break; }
            said = text.clone();
            if calls.is_empty() { break; }
            messages.push(Msg { role: "assistant".into(), content: text, tool_calls: calls.clone(), ..Default::default() });
            for call in calls {
                asked += 1;
                let answer = match parse(&call.arguments) {
                    Ok(qs) => {
                        report += &format!("  问：{}\n", qs.iter().map(|x| format!("{}（{}：{}）", x.question, x.kind, x.options.join(" / "))).collect::<Vec<_>>().join("；"));
                        answer_text(&qs, &qs.iter().map(|x| Answer { picked: vec![x.options[0].clone()], other: String::new() }).collect::<Vec<_>>()).unwrap()
                    }
                    Err(e) => { report += &format!("  问题不合格：{e}\n"); e }
                };
                messages.push(Msg::tool(&call.id, &answer));
            }
        }
        report += &format!("【{expect}】{q}\n  问了 {asked} 次{}\n  最后：{}\n", if bad.is_empty() { String::new() } else { format!("，出错：{bad}") }, said.chars().take(120).collect::<String>().replace('\n', " "));
    }
    println!("{report}");
    if let Ok(out) = std::env::var("POMELO_ASK_OUT") { std::fs::write(out, report).unwrap(); }
}
