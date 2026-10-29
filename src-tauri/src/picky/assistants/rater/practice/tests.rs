use super::*;
use crate::picky::assistants::seeker::{catalog, catalog::Record, Point};
use crate::picky::store::tests::memory_db;

fn record(id: &str, kind: &str, category: &str, title: &str, text: &str) -> Record {
    Record { id: id.into(), kind: kind.into(), category: category.into(), title: title.into(), text: text.into(), lang: "cn".into(), ..Default::default() }
}

fn q(id: &str, category: &str, question: &str, answer: &str) -> Question {
    Question { id: id.into(), category: category.into(), question: question.into(), answer: answer.into(), ..Default::default() }
}

fn job() -> JobSnapshot {
    JobSnapshot { id: 4, company: "柚子银行".into(), position: Some("数据分析师".into()), location: None, link: None, note: Some("负责信贷数据分析，要求 SQL。".into()), created_at: "2026-10-01T00:00:00.000Z".into() }
}

/// A made-up Seeker (one material with its 档案, rules) and bank: 简历题 n1 n2 (柚子笔记), 通用题 g1 g2,
/// 突击题 j1 (柚子银行) and j2 (another job).
fn seeded(c: &Connection) {
    let mut notes = record("m-notes", "material", "项目", "柚子笔记", "读书笔记整理工具");
    notes.points = vec![Point { id: "a".into(), text: "**检索改进**：按意思搜索，命中率从 54% 到 81%".into() }];
    let mut dossier = record("d", "fact", "经历档案", "柚子笔记 经历档案", "本人负责全部代码。");
    dossier.material = Some("m-notes".into());
    let mut shared = record("r1", "rule", "共用准则", "定位", "只做指定的事。");
    shared.lang = String::new();
    for r in [notes, dossier, shared, record("r2", "rule", "面试", "复盘", "事实和判断分开。"), record("r3", "decision", "面试", "语气", "少说套话。"), record("x", "rule", "素材润色", "别的", "不该读到"), record("f", "fact", "个人事实", "到岗时间", "2027 年 4 月后")] {
        catalog::put(c, &r).unwrap();
    }
    let bank_job = qbank::job_link(&job());
    for x in [
        Question { material: Some("m-notes".into()), ..q("n1", "简历题", "柚子笔记里你负责什么？", "全部代码。") },
        Question { material: Some("m-notes".into()), ..q("n2", "简历题", "命中率怎么算的？", "（待补）") },
        q("g1", "通用题", "自我介绍（1 分钟）", "我是林小柚。"),
        q("g2", "通用题", "为什么投这个岗位？", "因为……"),
        Question { job: Some(bank_job), ..q("j1", "突击题", "JD 要求 SQL，你怎么证明？", "我在……") },
        Question { job: Some(JobLink { id: 9, created_at: "x".into(), label: "别家".into() }), ..q("j2", "突击题", "别家的题", "") },
    ] {
        put(c, &x).unwrap();
    }
}

fn start(mode: &str, ids: &[&str]) -> Start {
    Start { mode: mode.into(), questions: ids.iter().map(|s| s.to_string()).collect(), ..Default::default() }
}

/// Plays a round: each question gets its mark (the cards come shuffled).
fn play(c: &Connection, ids: &[&str], marks: &[&str]) -> Round {
    let r = begin(c, &start("自己挑", ids), None).unwrap();
    let mut last = r.clone();
    for (i, card) in r.cards.iter().enumerate() {
        last = mark(c, &r.id, i, marks[ids.iter().position(|id| *id == card.question).unwrap()], "").unwrap();
    }
    last
}

fn ids(r: &Round) -> Vec<&str> {
    r.cards.iter().map(|c| c.question.as_str()).collect()
}

#[test]
fn practice_records_come_from_the_rounds_and_order_the_next_one() {
    let c = memory_db();
    seeded(&c);
    // g1: 会 twice → last; n1: 不熟 last time → first; g2: 会 once, n2 never → between
    play(&c, &["g1"], &[KNOW]);
    play(&c, &["g1"], &[KNOW]);
    play(&c, &["n1"], &[KNOW]);
    play(&c, &["n1"], &[UNSURE]);
    play(&c, &["g2"], &[KNOW]);
    let bank = read(&c).unwrap();
    let t = &bank.practice;
    assert_eq!((t["g1"].know, t["g1"].streak, t["g1"].rank()), (2, 2, 2));
    assert_eq!((t["n1"].know, t["n1"].unsure, t["n1"].last_mark.as_str(), t["n1"].rank()), (1, 1, UNSURE, 0));
    assert_eq!((t["g2"].rank(), t.contains_key("n2")), (1, false));
    assert!(t["g2"].last >= t["g1"].last);
    for _ in 0..5 {
        let r = begin(&c, &start("自己挑", &["g1", "g2", "n2", "n1"]), None).unwrap();
        assert_eq!((ids(&r)[0], ids(&r)[3]), ("n1", "g1"));
        stop(&c, &r.id).unwrap(); // nothing marked: dropped
        let r = begin(&c, &Start { mode: "随机".into(), count: 2, ..Default::default() }, None).unwrap();
        assert_eq!((ids(&r)[0], r.cards.len()), ("n1", 2));
        assert_ne!(ids(&r)[1], "g1");
        stop(&c, &r.id).unwrap();
    }
    // 按 JD: 通用 → 简历 → 突击, the model's order inside each
    let r = begin(&c, &Start { advice: "先自我介绍".into(), ..start("按 JD", &["j1", "n2", "g2", "n1", "g1"]) }, Some(qbank::job_link(&job()))).unwrap();
    assert_eq!(ids(&r), ["g2", "g1", "n2", "n1", "j1"]);
    assert_eq!((r.job.as_ref().map(|j| j.label.as_str()), r.advice.as_str()), (Some("柚子银行 · 数据分析师"), "先自我介绍"));
    // one running round at a time; 按 JD needs a job; unknown ids, nothing chosen
    assert!(begin(&c, &start("自己挑", &["g1"]), None).unwrap_err().contains("还没停下"));
    stop(&c, &r.id).unwrap();
    assert!(begin(&c, &start("按 JD", &["g1"]), None).is_err());
    assert!(begin(&c, &start("自己挑", &["gone"]), None).unwrap_err().contains("删除"));
    assert!(begin(&c, &start("自己挑", &[]), None).is_err());
    assert!(begin(&c, &start("挑着玩", &["g1"]), None).is_err());
    assert!(practice(&c).unwrap().running.is_none());
}

#[test]
fn a_round_is_marked_card_by_card_and_each_step_undoes() {
    let c = memory_db();
    seeded(&c);
    let mut h = crate::undo::History::default();
    let mut id = String::new();
    crate::undo::tests::step(&c, &mut h, "开始练习", |c| {
        id = begin(c, &start("自己挑", &["g1", "g2"]), None)?.id;
        Ok(())
    });
    let first = get(&c, &id).unwrap();
    // only the card on screen, only 会 / 不熟
    assert!(mark(&c, &id, 1, KNOW, "").unwrap_err().contains("对不上"));
    assert!(mark(&c, &id, 0, "还行", "").is_err());
    crate::undo::tests::step(&c, &mut h, "练习：不熟", |c| mark(c, &id, 0, UNSURE, "我是……  \n").map(|_| ()));
    let r = get(&c, &id).unwrap();
    assert_eq!((r.cards[0].typed.as_str(), r.current(), r.ended_at.is_none()), ("我是……", Some(1), true));
    assert_eq!(practice(&c).unwrap().running.map(|r| r.id), Some(id.clone()));
    // the last mark ends it
    crate::undo::tests::step(&c, &mut h, "练习：会", |c| mark(c, &id, 1, KNOW, "").map(|_| ()));
    let p = practice(&c).unwrap();
    assert!(p.running.is_none());
    assert_eq!((p.rounds.len(), p.rounds[0].know, p.rounds[0].unsure, p.rounds[0].left), (1, 1, 1, 0));
    assert!(mark(&c, &id, 1, KNOW, "").unwrap_err().contains("结束"));
    assert!(stop(&c, &id).is_err());
    // undo: the round open again on the second card, then the first, then gone
    crate::undo::undo(&c, &mut h).unwrap();
    assert_eq!(get(&c, &id).unwrap().current(), Some(1));
    crate::undo::undo(&c, &mut h).unwrap();
    assert_eq!(get(&c, &id).unwrap(), first);
    crate::undo::undo(&c, &mut h).unwrap();
    assert!(get(&c, &id).is_err() && read(&c).unwrap().practice.is_empty());

    // stopping halfway keeps what was done; deleting a round takes its marks off the record
    let r = begin(&c, &start("自己挑", &["g1", "g2"]), None).unwrap();
    mark(&c, &r.id, 0, KNOW, "").unwrap();
    stop(&c, &r.id).unwrap();
    let p = practice(&c).unwrap();
    assert_eq!((p.rounds[0].left, p.running.is_none()), (1, true));
    assert_eq!(read(&c).unwrap().practice.len(), 1);
    remove(&c, &r.id).unwrap();
    assert!(read(&c).unwrap().practice.is_empty() && remove(&c, &r.id).is_err());
}

#[test]
fn follow_ups_come_next_two_a_question_and_go_to_the_bank_once() {
    let c = memory_db();
    seeded(&c);
    let r = begin(&c, &Start { ..start("按 JD", &["n1", "j1"]) }, Some(qbank::job_link(&job()))).unwrap();
    // on the card on screen, once a card
    assert!(can_follow(&r, 1).is_err());
    let r = add_follow(&c, &r.id, 0, "我写了全部代码", "向量模型是你选的吗？", "是我选的（待补：为什么）。").unwrap();
    assert_eq!((r.cards.len(), r.cards[0].asked, r.cards[0].typed.as_str()), (3, true, "我写了全部代码"));
    let f = &r.cards[1];
    assert_eq!((f.follow, f.question.as_str(), f.category.as_str(), f.text.as_str()), (true, "n1", "简历题", "向量模型是你选的吗？"));
    assert!(can_follow(&r, 0).unwrap_err().contains("追问过"));
    mark(&c, &r.id, 0, KNOW, "").unwrap();
    // a follow-up's follow-up counts for the same question: two, then no more
    let r = add_follow(&c, &r.id, 1, "", "为什么不用云端？", "因为离线优先。").unwrap();
    mark(&c, &r.id, 1, UNSURE, "").unwrap();
    let r = get(&c, &r.id).unwrap();
    assert!(can_follow(&r, 2).unwrap_err().contains("2 次"));
    mark(&c, &r.id, 2, KNOW, "").unwrap();
    // the 突击题 has its own two
    assert!(can_follow(&get(&c, &r.id).unwrap(), 3).is_ok());
    let r = mark(&c, &r.id, 3, UNSURE, "").unwrap();
    assert!(r.ended_at.is_some());
    // follow-ups don't count in the record
    let t = read(&c).unwrap().practice;
    assert_eq!((t["n1"].know, t["n1"].unsure, t["j1"].unsure), (1, 0, 1));

    // 存进题库: once, with the question's category and material; one undo step
    let mut h = crate::undo::History::default();
    crate::undo::tests::step(&c, &mut h, "追问存进题库", |c| keep(c, &r.id, 1).map(|_| ()));
    let bank = read(&c).unwrap();
    let kept = bank.questions.iter().find(|x| x.question == "向量模型是你选的吗？").unwrap();
    assert_eq!((kept.category.as_str(), kept.material.as_deref(), kept.answer.as_str(), kept.revision), ("简历题", Some("m-notes"), "是我选的（待补：为什么）。", 1));
    assert_eq!(get(&c, &r.id).unwrap().cards[1].saved.as_deref(), Some(kept.id.as_str()));
    assert!(keep(&c, &r.id, 1).unwrap_err().contains("已经存"));
    assert!(keep(&c, &r.id, 0).is_err());
    crate::undo::undo(&c, &mut h).unwrap();
    assert_eq!((read(&c).unwrap().questions.len(), get(&c, &r.id).unwrap().cards[1].saved.clone()), (6, None));
    // a feedback only on an ended round; it wants one (follow-ups were asked)
    assert!(r.wants_comment());
    let r = set_feedback(&c, &r.id, Feedback { summary: "好".into(), ..Default::default() }).unwrap();
    assert_eq!(r.feedback.unwrap().summary, "好");
    let open = begin(&c, &start("自己挑", &["g1"]), None).unwrap();
    assert!(!open.wants_comment() && set_feedback(&c, &open.id, Feedback::default()).is_err());
}

#[test]
fn each_prompt_reads_what_it_needs() {
    let c = memory_db();
    seeded(&c);
    play(&c, &["n2"], &[UNSURE]);
    let (w, bank) = (seeker::read(&c).unwrap(), read(&c).unwrap());
    // the rules: 共用准则, 面试 (an older 个人约定 as a rule of one point), the skeleton; not other uses
    let rules = rules(&w);
    for part in ["# 共用准则\n\n## 定位\n1. 只做指定的事。", "# 面试\n\n## 复盘\n1. 事实和判断分开。\n\n## 语气\n1. 少说套话。", "# 答案\n答案是用户自己的口述稿"] {
        assert!(rules.contains(part), "{part}\n{rules}");
    }
    assert!(!rules.contains("不该读到"));
    // 按 JD: the JD, 通用 / 简历 and this job's 突击题 with their records
    let (system, user) = pick_prompt(&w, &bank, &job()).unwrap();
    assert!(system.contains("挑 8 张左右") && system.contains("自我介绍打头"), "{system}");
    for part in ["岗位：柚子银行 · 数据分析师\nJD：\n负责信贷数据分析", "题目 g1【通用题】自我介绍（1 分钟）（没练过）", "题目 n2【简历题】命中率怎么算的？（上次不熟（会 0 次、不熟 1 次）｜素材：柚子笔记）", "题目 j1【突击题】"] {
        assert!(user.contains(part), "{part}\n{user}");
    }
    assert!(!user.contains("别家的题"));
    // 追问 on a 简历题: the question, the 口述稿, what was typed, the material and its 档案, the JD
    let r = begin(&c, &start("自己挑", &["n1"]), None).unwrap();
    let (system, user) = follow_prompt(&w, &bank, &r, 0, "我负责写代码", Some(&job()));
    assert!(system.contains("扮演面试官") && system.contains("\"question\"") && system.contains("## 复盘"), "{system}");
    for part in ["【简历题】柚子笔记里你负责什么？\n用户题库里的口述稿：全部代码。", "用户这次说的：我负责写代码", "素材 m-notes【项目】柚子笔记", "经历档案《柚子笔记 经历档案》", "JD：\n负责信贷数据分析"] {
        assert!(user.contains(part), "{part}\n{user}");
    }
    // the second follow-up sees the first
    let r = add_follow(&c, &r.id, 0, "", "向量模型是你选的吗？", "是我选的。").unwrap();
    let (_, user) = follow_prompt(&w, &bank, &r, 1, "", None);
    assert!(user.contains("面试官追问：向量模型是你选的吗？\n参考答案：是我选的。\n用户这次说的：（没打字") && !user.contains("JD："), "{user}");
    // 通用题: 基本信息 / facts and the experiences
    let g = begin(&c, &start("自己挑", &["g2"]), None);
    assert!(g.is_err()); // one running at a time
    mark(&c, &r.id, 0, KNOW, "全部是我写的").unwrap();
    let r = mark(&c, &r.id, 1, UNSURE, "").unwrap();
    // feedback: each marked card with its 口述稿 / reference and what was typed
    let (system, user) = comment_prompt(&w, &bank, &r, None);
    assert!(system.contains("gaps") && system.contains("只看打了字的卡"), "{system}");
    for part in ["卡 1【简历题】柚子笔记里你负责什么？（素材：柚子笔记） —— 会\n  题库里的口述稿：全部代码。\n  这次打的字：全部是我写的", "卡 2【追问 · 接卡 1】向量模型是你选的吗？ —— 不熟\n  小拾的参考答案：是我选的。\n  （没打字）", "经历（简历素材"] {
        assert!(user.contains(part), "{part}\n{user}");
    }
    let r = begin(&c, &start("自己挑", &["g2"]), None).unwrap();
    let (_, user) = follow_prompt(&w, &bank, &r, 0, "", None);
    assert!(user.contains("已确认事实：\n- [个人事实] 到岗时间") && user.contains("【项目】柚子笔记"), "{user}");
}

#[test]
fn the_model_answers_are_checked() {
    let c = memory_db();
    seeded(&c);
    let bank = read(&c).unwrap();
    let (picked, advice) = parse_pick(&json!({ "advice": "先自我介绍", "questions": ["g1", "g1", "没有", "题目 n1", 3] }), &bank).unwrap();
    assert_eq!((picked, advice.as_str()), (vec!["g1".to_string(), "n1".to_string()], "先自我介绍"));
    assert!(parse_pick(&json!({ "questions": ["没有"] }), &bank).is_err());
    assert!(parse_pick(&json!({}), &bank).is_err());
    assert_eq!(parse_follow(&json!({ "question": " 为什么？ ", "answer": "因为" })).unwrap(), ("为什么？".to_string(), "因为".to_string()));
    assert!(parse_follow(&json!({ "answer": "因为" })).is_err());
    // notes: only on typed cards, numbered from 1
    let r = play(&c, &["g1", "g2"], &[KNOW, UNSURE]);
    let r = Round { cards: vec![Card { typed: "我是……".into(), ..r.cards[0].clone() }, r.cards[1].clone()], ..r };
    let fb = parse_comment(&json!({ "summary": "顺", "good": ["开头清楚", ""], "gaps": [], "notes": [{ "card": 1, "text": "再短一点" }, { "card": 2, "text": "没打字的" }, { "card": "卡 1", "text": "重复的" }, { "card": 9, "text": "x" }] }), &r).unwrap();
    assert_eq!((fb.good, fb.notes), (vec!["开头清楚".to_string()], vec![Note { card: 0, text: "再短一点".into() }]));
    assert!(parse_comment(&json!({ "summary": "" }), &r).is_err());
    // the mock model's answers pass the same checks
    assert!(parse_pick(&mock_pick(&bank, &job()), &bank).unwrap().0.len() >= 4);
    assert!(parse_follow(&mock_follow(&r, 0)).is_ok());
    assert_eq!(parse_comment(&mock_comment(&r), &r).unwrap().notes.len(), 1);
}

#[test]
fn rounds_go_through_the_data_file() {
    let a = memory_db();
    seeded(&a);
    let r = play(&a, &["g1", "n1"], &[KNOW, UNSURE]);
    set_feedback(&a, &r.id, Feedback { model: "DeepSeek Flash".into(), summary: "顺".into(), gaps: vec!["柚子笔记的指标".into()], notes: vec![Note { card: 0, text: "短".into() }], ..Default::default() }).unwrap();
    let open = begin(&a, &start("自己挑", &["g2"]), None).unwrap();
    add_follow(&a, &open.id, 0, "", "为什么？", "因为。").unwrap();
    let rec = crate::picky::transfer::export(&a).unwrap();
    let v = serde_json::to_value(&rec).unwrap();
    assert_eq!(v["rater"]["rounds"].as_array().unwrap().len(), 2);
    assert!(v["rater"].get("practice").is_none() && v["rater"].get("seconds").is_none());
    let b = memory_db();
    crate::picky::transfer::replace(&b, &serde_json::from_value(v.clone()).unwrap()).unwrap();
    assert_eq!(crate::picky::transfer::export(&b).unwrap(), rec);
    assert_eq!(read(&b).unwrap().practice["n1"].last_mark, UNSURE);
    // a file from before practice (format 23) leaves the rounds as they are
    let mut old = v;
    old["rater"].as_object_mut().unwrap().remove("rounds");
    crate::picky::transfer::replace(&b, &serde_json::from_value(old).unwrap()).unwrap();
    assert_eq!(rounds(&b).unwrap().len(), 2);
}

/// Richer made-up material for the real-model runs (still fictional): three experiences, facts, a
/// bank of eight answered questions, a JD.
fn seeded_rich(c: &Connection) -> JobSnapshot {
    seeded(c);
    let mut notes = catalog::load(c).unwrap().records.into_iter().find(|r| r.id == "m-notes").unwrap();
    (notes.role, notes.time, notes.stack) = ("独立开发".into(), "2026.03 - 至今".into(), "Rust、SQLite、本地向量模型".into());
    notes.points = vec![
        Point { id: "a".into(), text: "**检索改进**：用本地向量模型做按意思搜索，在 120 条测试问题上前 5 条命中率从 54% 提到 81%".into() },
        Point { id: "b".into(), text: "**数据整理**：把零散笔记按书合并成卷，去重后条数减少 30%".into() },
    ];
    catalog::put(c, &notes).unwrap();
    let mut orange = record("m-orange", "material", "项目", "橙子识别", "四类水果图像分类");
    (orange.role, orange.time, orange.stack) = ("项目组长".into(), "2025.12 - 2026.01".into(), "PyTorch、MobileNetV2".into());
    orange.points = vec![Point { id: "a".into(), text: "**模型迭代**：带 3 人小组两轮调参，测试集准确率 0.91".into() }, Point { id: "b".into(), text: "**类别均衡**：样本 20:75 失衡，加权采样后验证集 Macro-F1 从 0.88 到 0.92".into() }];
    catalog::put(c, &orange).unwrap();
    let mut d = record("d2", "fact", "经历档案", "橙子识别 经历档案", "本人负责第二版和最终版的调参与类别均衡；数据清洗是队友做的；测试集只有 60 张图，结果波动大。");
    d.material = Some("m-orange".into());
    catalog::put(c, &d).unwrap();
    let mut work = record("m-work", "material", "工作", "示例证券", "研究所数据实习");
    (work.role, work.time) = ("数据分析实习生".into(), "2025.07 - 2025.09".into());
    work.points = vec![Point { id: "a".into(), text: "**周报自动化**：用 Python 把行业周报的取数和作图做成脚本，每周省半天".into() }];
    catalog::put(c, &work).unwrap();
    catalog::put(c, &record("f2", "fact", "个人事实", "语言成绩", "CET-4 580，没考 CET-6")).unwrap();
    for x in [
        Question { material: Some("m-orange".into()), ..q("o1", "简历题", "橙子识别里你具体负责什么？哪些是队友做的？", "我是组长，负责第二版和最终版的调参和类别均衡；数据清洗是队友做的。最后测试集准确率 0.91，不过测试集只有 60 张图，波动比较大。") },
        Question { material: Some("m-orange".into()), ..q("o2", "简历题", "类别不均衡是怎么处理的？", "样本是 20:75，我用了加权采样，验证集 Macro-F1 从 0.88 到 0.92。如果重来，我会先补数据。") },
        Question { material: Some("m-work".into()), ..q("w1", "简历题", "示例证券实习做了什么？", "我把行业周报的取数和作图写成 Python 脚本，每周能省半天。") },
        Question { revision: 1, ..q("g1", "通用题", "自我介绍（1 分钟）", "我是林小柚，数据科学硕士在读。做过两个项目：柚子笔记，用本地向量模型做按意思搜索，命中率从 54% 提到 81%；橙子识别，带 3 人小组把准确率做到 0.91。还在示例证券做过数据实习。") },
        q("g3", "通用题", "你最大的缺点是什么？", "（待补：一个真实的缺点和改进的例子）"),
    ] {
        put(c, &x).unwrap();
    }
    JobSnapshot { note: Some("岗位职责：1. 负责信贷业务的数据分析和报表；2. 用 SQL、Python 做数据清洗和建模；3. 和业务部门沟通需求。任职要求：统计、计算机相关专业；熟悉 SQL；有机器学习项目经历优先。".into()), ..job() }
}

/// Opt-in, a real model: POMELO_REAL_LLM=1 [POMELO_MODEL=qwen3.5-9b] (the running local server), or
/// POMELO_CLOUD_KEY_FILE=… [POMELO_CLOUD_SERVICE=…] [POMELO_CLOUD_MODEL=…] [POMELO_CLOUD_THINK=always]
/// (chat.rs `cloud_target`). [POMELO_THINK=1] [POMELO_PRACTICE_CASE=挑卡|追问|反馈] [POMELO_PRACTICE_OUT=<report.md>]
///   cargo test --manifest-path src-tauri/Cargo.toml real_practice -- --ignored --nocapture
#[test]
#[ignore]
fn real_practice() {
    use crate::picky::provider::{self, Event, Msg, Target};
    let target = match crate::picky::chat::tests::cloud_target() {
        Some((t, _)) => t,
        None if std::env::var("POMELO_REAL_LLM").as_deref() == Ok("1") => Target::Local { model: std::env::var("POMELO_MODEL").unwrap_or_else(|_| "qwen3.5-9b".into()) },
        None => return eprintln!("needs POMELO_REAL_LLM=1 or POMELO_CLOUD_KEY_FILE"),
    };
    let name = match &target { Target::Cloud { model, .. } => model.api_model.clone(), Target::Local { model } => model.clone(), _ => "mock".into() };
    let think = std::env::var("POMELO_THINK").as_deref() == Ok("1");
    let only = std::env::var("POMELO_PRACTICE_CASE").ok();
    let wanted = |label: &str| only.as_deref().is_none_or(|o| label.starts_with(o));
    let c = memory_db();
    let jd = seeded_rich(&c);
    let w = seeker::read(&c).unwrap();
    // one call as the app makes it (room to think as Route::with_thinking gives it)
    let ask = |system: &str, user: &str, max: i64| -> (Result<Value, String>, String) {
        let room = match &target {
            Target::Cloud { model, .. } if model.thinking.thinks(think) => crate::picky::context::think_room(model.context_window),
            _ if think => crate::picky::context::think_room(16384),
            _ => 0,
        };
        let start = std::time::Instant::now();
        let (mut text, mut finish) = (String::new(), String::new());
        let call = provider::stream(target.clone(), vec![Msg::new("system", system), Msg::new("user", user)], provider::Options { max_tokens: Some(max + room), thinking: Some(think), one_off: true, ..Default::default() }, |e| match e {
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
    // numbers the answer has that the sources don't (maybe made up)
    let odd = |text: &str, source: &str| -> String {
        let mut out: Vec<&str> = text.split(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '%')).map(|s| s.trim_matches('.')).filter(|s| s.chars().any(|ch| ch.is_ascii_digit()) && s.len() > 1 && !source.contains(*s)).collect();
        out.dedup();
        if out.is_empty() { String::new() } else { format!("\n  - ⚠️ 资料里没有的数字：{}", out.join("、")) }
    };
    let mut report = vec![format!("# Rater 卡片练习真测：{name}{}\n", if think { "（思考）" } else { "" })];

    if wanted("挑卡") {
        let bank = read(&c).unwrap();
        let (system, user) = pick_prompt(&w, &bank, &jd).unwrap();
        let (out, took) = ask(&system, &user, 1024);
        report.push(format!("## 挑卡（按 JD：柚子银行 · 数据分析师）\n\n{took}"));
        match out.and_then(|v| parse_pick(&v, &bank)) {
            Err(e) => report.push(format!("- **没通过**：{e}\n")),
            Ok((ids, advice)) => {
                let picked: Vec<&Question> = ids.iter().filter_map(|id| bank.questions.iter().find(|q| &q.id == id)).collect();
                let ordered = order(MODES[1], picked.clone(), &bank.practice);
                report.push(format!("- 说明：{advice}\n- 模型挑了 {} 张（题库 {} 道，这个岗位能挑的 {} 道）；开练时的顺序：", picked.len(), bank.questions.len(), bank.questions.iter().filter(|q| q.category != "突击题" || q.job.as_ref().is_some_and(|j| j.id == jd.id)).count()));
                report.extend(ordered.iter().enumerate().map(|(i, q)| format!("  {}. 【{}】{}", i + 1, q.category, q.question)));
                report.push(format!("- 模型给的顺序：{}\n", picked.iter().map(|q| q.category.as_str()).collect::<Vec<_>>().join(" → ")));
            }
        }
    }

    if wanted("追问") {
        for (label, ids, typed) in [("追问 · 简历题（打了字，说错一个数）", ["o1"], "我是组长，调参是我做的，最后准确率 0.95。数据也是我清洗的。"), ("追问 · 通用题（没打字）", ["g1"], "")] {
            let r = begin(&c, &start("自己挑", &ids), None).unwrap();
            let bank = read(&c).unwrap();
            report.push(format!("## {label}\n\n题目：{}\n\n用户打的：{}", r.cards[0].text, if typed.is_empty() { "（没打字）" } else { typed }));
            let mut round = r.clone();
            for (step, said) in [(0usize, typed), (1, "这个……我不太记得了。")] {
                let (system, user) = follow_prompt(&w, &bank, &round, step, said, Some(&jd));
                let (out, took) = ask(&system, &user, 1536);
                match out.and_then(|v| parse_follow(&v)) {
                    Err(e) => {
                        report.push(format!("\n### 第 {} 次追问\n\n{took}\n- **没通过**：{e}", step + 1));
                        break;
                    }
                    Ok((question, answer)) => {
                        report.push(format!("\n### 第 {} 次追问\n\n{took}；参考答案约 {} 秒\n\n**问**：{question}\n\n**参考答案**：{answer}{}", step + 1, seconds(&answer), odd(&answer, &format!("{system}\n{user}"))));
                        round = add_follow(&c, &r.id, step, said, &question, &answer).unwrap();
                        mark(&c, &r.id, step, if step == 0 { KNOW } else { UNSURE }, said).unwrap();
                        round = get(&c, &round.id).unwrap();
                    }
                }
            }
            stop(&c, &r.id).ok();
            report.push(String::new());
        }
    }

    if wanted("反馈") {
        // a finished round: four cards, two typed (one with a wrong number and a wrong claim), a follow-up 不熟
        let r = begin(&c, &start("按 JD", &["g1", "o1", "o2", "w1"]), Some(qbank::job_link(&jd))).unwrap();
        let typed = ["我是林小柚，数据科学硕士在读，做过柚子笔记和橙子识别两个项目，还在示例证券实习过。", "", "我先看了样本比例，大概是 20 比 75，就用了加权采样，Macro-F1 提到了 0.95。", ""];
        let ids: Vec<String> = r.cards.iter().map(|k| k.question.clone()).collect();
        let mut i = 0;
        for (n, id) in ids.iter().enumerate() {
            let said = typed[["g1", "o1", "o2", "w1"].iter().position(|x| x == id).unwrap()];
            if id == "o1" {
                add_follow(&c, &r.id, i, said, "测试集只有 60 张图，你怎么判断 0.91 是可信的？", "（待补：有没有做交叉验证或多次划分）我承认测试集小，结果波动大……").unwrap();
                mark(&c, &r.id, i, KNOW, said).unwrap();
                mark(&c, &r.id, i + 1, UNSURE, "嗯……我们没有做交叉验证。").unwrap();
                i += 2;
            } else {
                mark(&c, &r.id, i, if n == 2 { UNSURE } else { KNOW }, said).unwrap();
                i += 1;
            }
        }
        let round = get(&c, &r.id).unwrap();
        let bank = read(&c).unwrap();
        let (system, user) = comment_prompt(&w, &bank, &round, Some(&jd));
        let (out, took) = ask(&system, &user, 2048);
        report.push(format!("## 反馈（4 张 + 1 张追问；打字的两张里一张把 0.92 说成 0.95）\n\n{took}"));
        match out.and_then(|v| parse_comment(&v, &round)) {
            Err(e) => report.push(format!("- **没通过**：{e}\n")),
            Ok(fb) => {
                report.push(format!("- 总评：{}", fb.summary));
                report.push(format!("- 说得好：\n{}", fb.good.iter().map(|g| format!("  - {g}")).collect::<Vec<_>>().join("\n")));
                report.push(format!("- 讲不清的：\n{}", fb.gaps.iter().map(|g| format!("  - {g}")).collect::<Vec<_>>().join("\n")));
                report.push(format!("- 打字的点评（{} 条）：", fb.notes.len()));
                report.extend(fb.notes.iter().map(|n| format!("  - 卡 {}「{}」：{}", n.card + 1, round.cards[n.card].text, n.text)));
                report.push(format!("- 抓到 0.95 了吗：{}\n", if fb.notes.iter().any(|n| n.text.contains("0.95") || n.text.contains("0.92")) || fb.gaps.iter().any(|g| g.contains("0.95")) { "抓到了" } else { "**没抓到**" }));
            }
        }
    }
    let text = report.join("\n");
    eprintln!("{text}");
    if let Ok(out) = std::env::var("POMELO_PRACTICE_OUT") {
        std::fs::write(out, text).unwrap();
    }
}

#[test]
fn questions_keep_the_users_order_and_go_several_at_once() {
    let c = memory_db();
    seeded(&c);
    let order = |c: &Connection| read(c).unwrap().questions.iter().filter(|q| q.category == "通用题").map(|q| q.id.clone()).collect::<Vec<_>>();
    assert_eq!(order(&c), ["g1", "g2"]);
    reorder(&c, &["g2".into(), "g1".into()]).unwrap();
    assert_eq!(order(&c), ["g2", "g1"]);
    // the rest kept their places
    assert_eq!(read(&c).unwrap().questions.iter().map(|q| q.id.as_str()).filter(|id| !id.starts_with('g')).collect::<Vec<_>>(), ["n1", "n2", "j1", "j2"]);
    let b = memory_db();
    crate::picky::transfer::replace(&b, &crate::picky::transfer::export(&c).unwrap()).unwrap();
    assert_eq!(order(&b), ["g2", "g1"]);
    // rounds: several deleted in one go
    let r1 = play(&c, &["g1"], &[KNOW]);
    let r2 = play(&c, &["g2"], &[UNSURE]);
    let tx = Tx::begin(&c).unwrap();
    for id in [&r1.id, &r2.id] {
        remove(&tx, id).unwrap();
    }
    tx.commit().unwrap();
    assert!(practice(&c).unwrap().rounds.is_empty());
}
