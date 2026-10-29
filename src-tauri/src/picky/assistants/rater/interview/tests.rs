use super::*;
use crate::picky::assistants::seeker::{catalog, output::{Export, JobResume}, workflow, Entry, Point, Section};
use crate::picky::store::tests::memory_db;

fn record(id: &str, kind: &str, category: &str, title: &str, text: &str) -> Record {
    Record { id: id.into(), kind: kind.into(), category: category.into(), title: title.into(), text: text.into(), lang: "cn".into(), ..Default::default() }
}

fn q(id: &str, category: &str, question: &str, answer: &str) -> Question {
    Question { id: id.into(), category: category.into(), question: question.into(), answer: answer.into(), ..Default::default() }
}

fn job() -> JobSnapshot {
    JobSnapshot { id: 4, company: "柚子银行".into(), position: Some("数据分析师".into()), location: Some("上海".into()), link: None, note: Some("负责信贷数据分析，要求 SQL。".into()), created_at: "2026-10-01T00:00:00.000Z".into() }
}

fn draft(name: &str, ids: &[&str]) -> seeker::Draft {
    let title = |id: &str| match id { "m-notes" => "柚子笔记".to_string(), "m-orange" => "橙子识别".to_string(), _ => format!("经历 {id}") };
    let entries = ids.iter().map(|id| Entry { id: format!("e-{id}"), time: String::new(), name: title(id), role: String::new(), location: String::new(), note: String::new(), points: vec![Point { id: format!("p-{id}"), text: format!("{id} 的要点") }], source: Some(workflow::Source { record_id: id.to_string(), revision: 1, point_ids: vec![] }) }).collect();
    seeker::Draft { lang: "cn".into(), form: String::new(), name: name.into(), header: vec![], sections: vec![Section { id: "s".into(), title: "项目经历".into(), entries, category: "项目".into() }], jd: String::new(), job: None }
}

/// Two materials (柚子笔记 with its 档案, 橙子识别), the 面试 rule, a bank: 通用题 g1, 简历题 n1 (柚子笔记)
/// and o1 (橙子识别), 突击题 j1 (柚子银行) and j2 (another job).
fn seeded(c: &Connection) {
    let mut dossier = record("d", "fact", "经历档案", "柚子笔记 经历档案", "本人负责全部代码。");
    dossier.material = Some("m-notes".into());
    for r in [record("m-notes", "material", "项目", "柚子笔记", "读书笔记工具"), record("m-orange", "material", "项目", "橙子识别", "水果分类"), dossier, record("r", "rule", "面试", "复盘", "事实和判断分开。"), record("f", "fact", "个人事实", "到岗时间", "2027 年 4 月后")] {
        catalog::put(c, &r).unwrap();
    }
    for x in [
        q("g1", "通用题", "自我介绍（1 分钟）", "我是林小柚。"),
        Question { material: Some("m-notes".into()), ..q("n1", "简历题", "柚子笔记里你负责什么？", "全部代码。") },
        Question { material: Some("m-orange".into()), ..q("o1", "简历题", "橙子识别怎么调参？", "两轮。") },
        Question { job: Some(qbank::job_link(&job())), ..q("j1", "突击题", "JD 要求 SQL，你怎么证明？", "我在……") },
        Question { job: Some(JobLink { id: 9, created_at: "x".into(), label: "别家".into() }), ..q("j2", "突击题", "别家的题", "") },
    ] {
        put(c, &x).unwrap();
    }
    seeker::save(c, &draft("林小柚", &["m-orange"])).unwrap();
}

#[test]
fn a_prep_reads_the_resume_the_job_uses_its_jd_and_the_relevant_questions() {
    let c = memory_db();
    seeded(&c);
    let (mut w, bank) = (seeker::read(&c).unwrap(), read(&c).unwrap());
    let ask = PrepAsk { job: 4, lang: "中文".into(), form: "视频一面".into(), request: "多准备 SQL".into() };
    // no export for the job: the Chinese draft (橙子识别) → its 简历题, not 柚子笔记's
    // the export's Word (a.docx) reads as its text; the rest have none
    let txt = |p: &str| (p == "/x/a.docx").then(|| "林小柚\n柚子笔记 · 独立开发".to_string());
    let used = |w: &Workspace| resume(w, &job(), "中文", &|_| true, &txt);
    let (system, user) = prep_prompt(&w, &bank, &ask, &job(), used(&w).as_ref(), "").unwrap();
    assert!(system.contains("面试语言：中文；形式：视频一面") && system.contains("# 面试\n\n## 复盘\n1. 事实和判断分开。") && system.contains("\"sheet\""), "{system}");
    for part in ["用户本次要求：多准备 SQL", "岗位：柚子银行 · 数据分析师（上海）\nJD：\n负责信贷数据分析", "投递的简历（中文草稿）：", "橙子识别\n", "题目 g1【通用题】", "题目 o1【简历题】", "题目 j1【突击题】", "[个人事实] 到岗时间"] {
        assert!(user.contains(part), "{part}\n{user}");
    }
    assert!(!user.contains("题目 n1") && !user.contains("别家的题") && !user.contains("本人负责全部代码"), "{user}");
    // the job's export in use wins: 柚子笔记, with its 档案
    let export = Export { id: "e1".into(), label: "林小柚-简历.docx".into(), path: "/x/a.docx".into(), lang: "cn".into(), materials: vec!["m-notes".into()], created_at: "2026-10-02T00:00:00.000Z".into(), draft: None };
    w.output.links.push(JobResume { id: "l".into(), path: "/x/a.docx".into(), job: job(), jd: String::new(), created_at: "2026-10-02T00:00:00.000Z".into(), current: true, export_id: None });
    w.output.exports.push(export);
    let (_, user) = prep_prompt(&w, &bank, &ask, &job(), used(&w).as_ref(), "\n\n公司资料（柚子银行）…").unwrap();
    assert!(user.contains("投递的简历（林小柚-简历.docx）") && user.contains("题目 n1") && !user.contains("题目 o1") && user.contains("经历档案《柚子笔记 经历档案》"), "{user}");
    // English interview: said so
    let (system, _) = prep_prompt(&w, &bank, &PrepAsk { lang: "English".into(), ..ask.clone() }, &job(), used(&w).as_ref(), "").unwrap();
    assert!(system.contains("面试语言是英文"));
    // a résumé from cv/ that Pomelo didn't make (now in use): its text, and every 简历题
    w.output.links[0].current = false;
    w.output.links.insert(0, JobResume { id: "l2".into(), path: "/cv/cn/林小柚-简历-v3.pdf".into(), job: job(), jd: String::new(), created_at: "2026-10-03T00:00:00.000Z".into(), current: true, export_id: None });
    let pdf = resume(&w, &job(), "中文", &|_| true, &|p| p.ends_with(".pdf").then(|| "林小柚\n柚子银行实习 · 写 SQL 报表".to_string())).unwrap();
    assert_eq!((pdf.name.as_str(), pdf.materials.is_empty()), ("林小柚-简历-v3.pdf", true));
    let (_, user) = prep_prompt(&w, &bank, &ask, &job(), Some(&pdf), "").unwrap();
    assert!(user.contains("投递的简历（林小柚-简历-v3.pdf）：\n林小柚\n柚子银行实习") && user.contains("题目 n1") && user.contains("题目 o1"), "{user}");
    // its file deleted: the export before it isn't used either when gone; the draft then
    let gone = resume(&w, &job(), "中文", &|p| !p.ends_with(".pdf"), &txt).unwrap();
    assert_eq!(gone.name, "林小柚-简历.docx");
    assert_eq!(resume(&w, &job(), "中文", &|_| false, &|_| None).unwrap().name, "中文草稿");
}

#[test]
fn the_model_answers_are_checked() {
    let c = memory_db();
    seeded(&c);
    let bank = read(&c).unwrap();
    let (picked, follows, sheet, advice) = parse_prep(&json!({ "advice": "背熟", "questions": ["g1", "g1", "没有", "题目 n1"], "follows": [{ "about": "SQL", "category": "突击题", "question": "窗口函数是什么？", "answer": "（待补）" }, { "question": " " }, { "question": "怎么算的？", "category": "乱写" }], "sheet": "# 速查卡" }), &bank).unwrap();
    assert_eq!((picked, sheet.as_str(), advice.as_str()), (vec!["g1".to_string(), "n1".to_string()], "# 速查卡", "背熟"));
    assert_eq!(follows.iter().map(|f| (f.question.as_str(), f.category.as_str())).collect::<Vec<_>>(), [("窗口函数是什么？", "突击题"), ("怎么算的？", "简历题")]);
    assert!(parse_prep(&json!({ "questions": ["g1"] }), &bank).is_err());
    // the review's comparison: numbers checked, known questions and odd uses dropped, two rules at most
    let r = Review { items: vec![Asked { question: "自我介绍".into(), ..Default::default() }, Asked { question: "数据从哪来？".into(), ..Default::default() }], ..Default::default() };
    let a = parse_compare(&json!({ "summary": "还行", "hits": [{ "asked": 1, "text": "g1" }, { "asked": 1, "text": "重复" }, { "asked": 5 }], "misses": [{ "asked": "问 2", "text": "讲不清" }], "questions": [{ "category": "突击题", "question": "数据从哪来？", "answer": "（待补）" }, { "question": "自我介绍（1 分钟）" }], "rules": [{ "use": "面试", "title": "先说口径", "text": "数字先说口径", "why": "被追问" }, { "use": "素材润色", "title": "x", "text": "y" }, { "use": "面试", "title": "", "text": "y" }] }), &r, &bank).unwrap();
    assert_eq!((a.hits.len(), a.misses[0].asked, a.questions.len(), a.rules.len()), (1, 1, 1, 1));
    assert!(parse_compare(&json!({}), &r, &bank).is_err());
    // the mock model's answers pass the same checks
    assert!(parse_prep(&mock_prep(&bank, &job()), &bank).unwrap().0.contains(&"j1".to_string()));
    assert!(parse_compare(&mock_compare(&r), &r, &bank).is_ok());
}

#[test]
fn reviews_save_compare_and_turn_into_questions_and_rules_one_step_each() {
    let c = memory_db();
    seeded(&c);
    let link = qbank::job_link(&job());
    let mut h = crate::undo::History::default();
    // a new review: rows without a question refused, empty rows dropped; needs a job and a date
    let input = Review { date: "2026-10-28".into(), round: " 一面 ".into(), result: "待定".into(), items: vec![Asked { question: "自我介绍".into(), answer: "我是……".into(), ..Default::default() }, Asked::default(), Asked { question: "数据从哪来？".into(), feedback: "追问了两次".into(), ..Default::default() }], feedback: "基础扎实".into(), thoughts: "感觉一般".into(), ..Default::default() };
    assert!(save_review(&c, &Review { items: vec![Asked { answer: "没问题的答".into(), ..Default::default() }], ..input.clone() }, Some(link.clone())).is_err());
    assert!(save_review(&c, &input, None).is_err());
    assert!(save_review(&c, &Review { result: "挂了".into(), ..input.clone() }, Some(link.clone())).is_err());
    let mut id = String::new();
    crate::undo::tests::step(&c, &mut h, "记复盘", |c| {
        id = save_review(c, &input, Some(link.clone()))?.id;
        Ok(())
    });
    let r = get_review(&c, &id).unwrap();
    assert_eq!((r.round.as_str(), r.items.len(), r.job.label.as_str()), ("一面", 2, "柚子银行 · 数据分析师"));
    // the comparison reads the prep and keeps facts apart from the user's judgement
    let prep = Prep { id: "p1".into(), job: link.clone(), questions: vec!["g1".into()], follows: vec![Follow { question: "SQL 怎么学的？".into(), category: "突击题".into(), ..Default::default() }], sheet: "# 速查卡\n## 高频题".into(), created_at: "2026-10-27T00:00:00.000Z".into(), ..Default::default() };
    put_prep(&c, &prep).unwrap();
    let (w, bank) = (seeker::read(&c).unwrap(), read(&c).unwrap());
    let (system, user) = compare_prompt(&w, &bank, &r, Some(&prep), Some(&job()));
    assert!(system.contains("一次录取或拒绝不说明简历好坏") && system.contains("面试 / 题库维护"), "{system}");
    for part in ["问 2：数据从哪来？\n  我答：（没记）\n  面试官当场的反应：追问了两次", "面试官的整体反馈（发生的事实）：\n基础扎实", "用户自己的判断（个人看法，不是事实）：\n感觉一般", "挑的题：\n- 【通用题】自我介绍（1 分钟）", "- 追问：SQL 怎么学的？", "## 高频题", "JD：\n负责信贷"] {
        assert!(user.contains(part), "{part}\n{user}");
    }
    assert!(!user.contains("别家的题"));
    let a = Analysis { prep: Some("p1".into()), ..parse_compare(&mock_compare(&r), &r, &bank).unwrap() };
    crate::undo::tests::step(&c, &mut h, "复盘对照", |c| set_analysis(c, &id, a).map(|_| ()));
    // an edit keeps the analysis; the questions and the rule go in once each
    let edited = save_review(&c, &Review { result: "通过".into(), ..get_review(&c, &id).unwrap() }, None).unwrap();
    assert!(edited.analysis.is_some() && edited.result == "通过");
    crate::undo::tests::step(&c, &mut h, "复盘的题存进题库", |c| keep_question(c, &id, 0));
    let bank = read(&c).unwrap();
    let kept = bank.questions.iter().find(|x| x.question == "（模拟）你们的数据是怎么来的？").unwrap();
    assert_eq!((kept.category.as_str(), kept.job.as_ref().map(|j| j.id)), ("突击题", Some(4)));
    assert!(keep_question(&c, &id, 0).unwrap_err().contains("已经"));
    crate::undo::tests::step(&c, &mut h, "加进规则", |c| adopt_rule(c, &id, 0));
    let rule = seeker::read(&c).unwrap().catalog.records.into_iter().find(|x| x.title == "（模拟）先说数据口径").unwrap();
    assert_eq!((rule.kind.as_str(), rule.category.as_str(), rule.points.len()), ("rule", "面试", 1));
    assert!(adopt_rule(&c, &id, 0).is_err());
    assert!(super::super::practice::rules(&seeker::read(&c).unwrap()).contains("## （模拟）先说数据口径\n1. 被问到数字时"), "{}", super::super::practice::rules(&seeker::read(&c).unwrap()));
    // each was one step
    crate::undo::undo(&c, &mut h).unwrap();
    assert!(seeker::read(&c).unwrap().catalog.records.iter().all(|x| x.title != "（模拟）先说数据口径"));
    assert!(get_review(&c, &id).unwrap().analysis.unwrap().rules[0].adopted.is_none());
    crate::undo::undo(&c, &mut h).unwrap();
    assert!(read(&c).unwrap().questions.iter().all(|x| x.question != "（模拟）你们的数据是怎么来的？"));
    // a prep's follow-up goes to the bank once; the 速查卡 is edited, not emptied
    keep_follow(&c, "p1", 0).unwrap();
    assert!(keep_follow(&c, "p1", 0).is_err());
    assert_eq!(read(&c).unwrap().questions.iter().find(|x| x.question == "SQL 怎么学的？").and_then(|x| x.job.as_ref()).map(|j| j.id), Some(4));
    save_prep(&c, &Prep { sheet: "# 改过的".into(), questions: vec![], ..prep.clone() }).unwrap();
    let p = get_prep(&c, "p1").unwrap();
    assert_eq!((p.sheet.as_str(), p.questions.len(), p.follows[0].saved.is_some()), ("# 改过的", 1, true));
    assert!(save_prep(&c, &Prep { sheet: " ".into(), ..prep }).is_err());
    remove(&c, "p1").unwrap();
    assert!(get_prep(&c, "p1").is_err() && remove(&c, "p1").is_err());
}

#[test]
fn interviews_go_through_the_data_file_and_word_never_overwrites() {
    let a = memory_db();
    seeded(&a);
    put_prep(&a, &Prep { id: "p1".into(), job: qbank::job_link(&job()), sheet: "# 速查卡".into(), created_at: "2026-10-27T00:00:00.000Z".into(), ..Default::default() }).unwrap();
    save_review(&a, &Review { date: "2026-10-28".into(), items: vec![Asked { question: "自我介绍".into(), ..Default::default() }], ..Default::default() }, Some(qbank::job_link(&job()))).unwrap();
    let rec = crate::picky::transfer::export(&a).unwrap();
    let v = serde_json::to_value(&rec).unwrap();
    assert_eq!((v["rater"]["interviews"]["preps"].as_array().unwrap().len(), v["rater"]["interviews"]["reviews"].as_array().unwrap().len()), (1, 1));
    let b = memory_db();
    crate::picky::transfer::replace(&b, &serde_json::from_value(v.clone()).unwrap()).unwrap();
    assert_eq!(crate::picky::transfer::export(&b).unwrap(), rec);
    // a file from before 面试 (format 24) leaves them
    let mut old = v;
    old["rater"].as_object_mut().unwrap().remove("interviews");
    crate::picky::transfer::replace(&b, &serde_json::from_value(old).unwrap()).unwrap();
    assert_eq!(read_all(&b).unwrap().len(), 2);
    // 另存 Word: a new file each time
    let dir = crate::db::tests::temp_dir("rater-word");
    let bytes = crate::picky::word::markdown_to_docx("# 速查卡\n\n- 一条", &[]).unwrap();
    let first = write_new(&dir, "速查卡 柚子银行", &bytes).unwrap();
    let second = write_new(&dir, "速查卡 柚子银行.docx", &bytes).unwrap();
    assert!(first.ends_with("速查卡 柚子银行.docx") && second.ends_with("速查卡 柚子银行 (2).docx"));
    assert!(write_new(&dir.join("没有"), "x", &bytes).is_err());
}

/// Opt-in, a real model: 准备面试 (made-up data: two projects, a draft, a bank of six, a JD) and 对照 a
/// made-up review against it. POMELO_CLOUD_KEY_FILE=… [POMELO_CLOUD_SERVICE=…] [POMELO_CLOUD_MODEL=…]
/// [POMELO_CLOUD_THINK=always] (chat.rs `cloud_target`), or POMELO_REAL_LLM=1. [POMELO_INTERVIEW_OUT=<report.md>]
///   cargo test --manifest-path src-tauri/Cargo.toml real_interview -- --ignored --nocapture
#[test]
#[ignore]
fn real_interview() {
    use crate::picky::provider::{self, Event, Msg, Options, Target};
    let target = match crate::picky::chat::tests::cloud_target() {
        Some((t, _)) => t,
        None if std::env::var("POMELO_REAL_LLM").as_deref() == Ok("1") => Target::Local { model: std::env::var("POMELO_MODEL").unwrap_or_else(|_| "qwen3.5-9b".into()) },
        None => return eprintln!("needs POMELO_REAL_LLM=1 or POMELO_CLOUD_KEY_FILE"),
    };
    let name = match &target { Target::Cloud { model, .. } => model.api_model.clone(), Target::Local { model } => model.clone(), _ => "mock".into() };
    let c = memory_db();
    seeded(&c);
    let mut notes = catalog::load(&c).unwrap().records.into_iter().find(|r| r.id == "m-notes").unwrap();
    (notes.role, notes.time, notes.stack) = ("独立开发".into(), "2026.03 - 至今".into(), "Rust、SQLite、本地向量模型".into());
    notes.points = vec![Point { id: "a".into(), text: "**检索改进**：用本地向量模型做按意思搜索，在 120 条测试问题上前 5 条命中率从 54% 提到 81%".into() }, Point { id: "b".into(), text: "**离线优先**：全部数据存在本机 SQLite".into() }];
    catalog::put(&c, &notes).unwrap();
    let mut orange = catalog::load(&c).unwrap().records.into_iter().find(|r| r.id == "m-orange").unwrap();
    (orange.role, orange.stack) = ("项目组长".into(), "PyTorch、MobileNetV2".into());
    orange.points = vec![Point { id: "a".into(), text: "**模型迭代**：带 3 人小组两轮调参，测试集准确率 0.91".into() }, Point { id: "b".into(), text: "**类别均衡**：样本 20:75 失衡，加权采样后验证集 Macro-F1 从 0.88 到 0.92".into() }];
    catalog::put(&c, &orange).unwrap();
    let mut d2 = record("d2", "fact", "经历档案", "橙子识别 经历档案", "本人负责第二版和最终版的调参与类别均衡；数据清洗是队友做的；测试集只有 60 张图，结果波动大。");
    d2.material = Some("m-orange".into());
    catalog::put(&c, &d2).unwrap();
    seeker::save(&c, &draft("林小柚", &["m-notes", "m-orange"])).unwrap();
    put(&c, &Question { material: Some("m-notes".into()), ..q("n2", "简历题", "81% 是怎么算的？", "在我自己攒的 120 条测试问题上，看前 5 条结果里有没有要找的笔记。") }).unwrap();
    let jd = JobSnapshot { note: Some("岗位职责：1. 负责信贷业务的数据分析和报表；2. 用 SQL、Python 做数据清洗和建模；3. 和业务部门沟通需求。任职要求：统计、计算机相关专业；熟悉 SQL；有机器学习项目经历优先。".into()), ..job() };
    let (w, bank) = (seeker::read(&c).unwrap(), read(&c).unwrap());
    let ask = |system: &str, user: &str, max: i64| -> (Result<Value, String>, String) {
        let room = match &target {
            Target::Cloud { model, .. } if model.thinking.thinks(false) => crate::picky::context::think_room(model.context_window),
            _ => 0,
        };
        let start = std::time::Instant::now();
        let (mut text, mut finish) = (String::new(), String::new());
        let call = provider::stream(target.clone(), vec![Msg::new("system", system), Msg::new("user", user)], Options { max_tokens: Some(max + room), thinking: Some(false), one_off: true, ..Default::default() }, |e| match e {
            Event::Content(t) => text.push_str(&t),
            Event::Finished(r) => finish = r,
            _ => (),
        });
        let res = tauri::async_runtime::block_on(async { tokio::time::timeout(std::time::Duration::from_secs(600), call).await });
        let took = format!("用时 {} 秒；结束原因 {finish}", start.elapsed().as_secs());
        let out = match res.map_err(|_| "超时".to_string()).and_then(|r| r) {
            Err(e) => Err(e),
            Ok(()) if finish == "length" => Err(format!("被截断：\n{text}")),
            Ok(()) => qbank::answer_json(&text).ok_or_else(|| format!("回答里没有 JSON：\n{text}")),
        };
        (out, took)
    };
    let mut report = vec![format!("# Rater 面试真测：{name}\n")];
    // 准备
    let (system, user) = prep_prompt(&w, &bank, &PrepAsk { job: 4, lang: "中文".into(), form: "视频一面 · 业务面".into(), request: String::new() }, &jd, resume(&w, &jd, "中文", &|_| true, &|_| None).as_ref(), "").unwrap();
    let source = format!("{system}\n{user}");
    let odd = |text: &str| -> String {
        let mut out: Vec<&str> = text.split(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '%')).map(|s| s.trim_matches('.')).filter(|s| s.chars().any(|ch| ch.is_ascii_digit()) && s.len() > 1 && !source.contains(*s)).collect();
        out.sort();
        out.dedup();
        if out.is_empty() { "没有".into() } else { out.join("、") }
    };
    let (out, took) = ask(&system, &user, 6144);
    report.push(format!("## 准备面试（柚子银行 · 数据分析师，中文，视频一面）\n\n{took}"));
    let prep = match out.and_then(|v| parse_prep(&v, &bank)) {
        Err(e) => {
            report.push(format!("- **没通过**：{e}\n"));
            None
        }
        Ok((questions, follows, sheet, advice)) => {
            report.push(format!("- 说明：{advice}\n- 挑了 {} 道：{}\n- 速查卡 {} 字；资料里没有的数字：{}\n- 追问 {} 个：", questions.len(), questions.iter().filter_map(|id| bank.questions.iter().find(|q| &q.id == id)).map(|q| format!("【{}】{}", q.category, q.question)).collect::<Vec<_>>().join("；"), sheet.chars().count(), odd(&sheet), follows.len()));
            report.extend(follows.iter().map(|f| format!("  - （{}）{} → {}", f.about, f.question, f.answer)));
            report.push(format!("\n### 速查卡\n\n{sheet}\n"));
            Some(Prep { id: "p".into(), job: qbank::job_link(&jd), questions, follows, sheet, created_at: "2026-10-28T00:00:00.000Z".into(), ..Default::default() })
        }
    };
    // 对照: two asked were prepared, two weren't
    let review = Review { date: "2026-10-29".into(), round: "一面".into(), result: "待定".into(), job: qbank::job_link(&jd), items: vec![
        Asked { question: "先做个一分钟自我介绍".into(), answer: "我是林小柚，数据科学硕士在读……".into(), feedback: String::new() },
        Asked { question: "81% 是怎么测出来的？测试集是谁标的？".into(), answer: "120 条问题是我自己攒的……标注……没想过".into(), feedback: "追问了两次标注方法".into() },
        Asked { question: "SQL 里窗口函数用过吗？举个例子".into(), answer: "用得不多……".into(), feedback: "面试官说基础要补".into() },
        Asked { question: "信贷业务里你觉得最重要的指标是什么？".into(), answer: "逾期率？不太确定".into(), feedback: String::new() },
    ], feedback: "项目讲得清楚，SQL 和业务理解偏弱".into(), thoughts: "感觉挂了，SQL 没答好".into(), ..Default::default() };
    let (system, user) = compare_prompt(&w, &bank, &review, prep.as_ref(), Some(&jd));
    let (out, took) = ask(&system, &user, 4096);
    report.push(format!("## 复盘对照（4 个问题：前两个准备到了，后两个没有）\n\n{took}"));
    match out.and_then(|v| parse_compare(&v, &review, &bank)) {
        Err(e) => report.push(format!("- **没通过**：{e}\n")),
        Ok(a) => {
            report.push(format!("- 总评：{}", a.summary));
            report.push(format!("- 准备到了：{}", a.hits.iter().map(|m| format!("问 {}（{}）", m.asked + 1, m.text)).collect::<Vec<_>>().join("；")));
            report.push(format!("- 没准备到：{}", a.misses.iter().map(|m| format!("问 {}（{}）", m.asked + 1, m.text)).collect::<Vec<_>>().join("；")));
            report.push(format!("- 建议加进题库 {} 道：", a.questions.len()));
            report.extend(a.questions.iter().map(|f| format!("  - 【{}】{} → {}", f.category, f.question, f.answer)));
            report.push(format!("- 提议的规则 {} 条：", a.rules.len()));
            report.extend(a.rules.iter().map(|r| format!("  - 【{}】{}：{}（{}）", r.use_, r.title, r.text, r.why)));
            report.push(String::new());
        }
    }
    let text = report.join("\n");
    eprintln!("{text}");
    if let Ok(out) = std::env::var("POMELO_INTERVIEW_OUT") {
        std::fs::write(out, text).unwrap();
    }
}
