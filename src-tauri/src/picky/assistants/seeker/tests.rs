//! Seeker with a made-up Seeker folder shaped like the real transfer/ (all content fictional).
use super::catalog::{self, Record};
use super::{company, import, model, output, workflow, *};
use crate::db::tests::{temp_dir, TempDir};
use crate::picky::store::tests::memory_db;
use crate::picky::word;
use std::path::Path;

const FILES: &[(&str, &str)] = &[
    ("cn-modules/0 - 归档.md", "# 0 - 归档\n#职业/中文简历\n"),
    ("cn-modules/1 - 基本信息.md", "# 1 - 基本信息\n#职业/中文简历\n\n- 姓名：林小柚\n- 工作地点：上海\n- 工作邮箱：work@example.com\n- 个人邮箱：me@example.com\n- 联系电话：+86 100 0000 0000\n- GitHub：https://github.com/example\n- 个人网站：https://example.com\n"),
    ("cn-modules/2 - 教育背景.md", "# 2 - 教育背景\n#职业/中文简历\n\n## 示例大学 · 信息学院\n- 专业：数据科学 · 硕士\n- 时间：2025.07 - 至今\n\n- **主修课程**：机器学习、数据库\n\n## 示例学院 · 经济系\n- 专业：金融学 · 本科\n- 时间：2021.09 - 2025.06\n\n- **主修课程**：计量经济学\n- **在校荣誉**：奖学金\n  \n"),
    ("cn-modules/3 - 技能证书.md", "# 3 - 技能证书\n#职业/中文简历\n\n- **编程语言**：Python、SQL\n- **语言能力**：英语六级\n"),
    ("cn-modules/4 - 项目经历.md", "# 4 - 项目经历\n#职业/中文简历\n\n## 柚子笔记\n- 角色：独立开发\n- 时间：2026.03 - 至今\n- 概览：读书笔记整理工具\n- 技术栈：Rust、SQLite\n\n- **数据整理**：把零散笔记合成卷\n- **检索改进**：按意思搜索\n\n## 橙子识别\n- 角色：课程项目\n- 时间：2025.10 - 2025.12\n- 地点：上海\n- 概览：水果分类模型\n\n* **模型迭代**：两轮调参\n"),
    ("cn-modules/5 - 工作经历.md", "# 5 - 工作经历\n#职业/中文简历\n\n## 示例证券 · 研究所\n- 职位：研究部实习生\n- 时间：2024.01 - 2024.02\n- 地点：杭州\n\n* **行业研究**：整理研报\n- **数据分析**：写脚本\n"),
    ("cn-modules/6 - 基本信息 - 网申.md", "# 6 - 基本信息 - 网申\n#职业/中文简历\n\n## 为什么选择我们\n示例答案。\n"),
    ("rules/cn-modules.md", "# 中文素材润色规则\n\n## 事实与职责\n\n- 不编造事实\n\n## 有效个人约定\n\n- **自维护内容**：基本信息由用户自维护\n- 不催稿\n"),
    ("rules/cn-resume.md", "# 中文简历整体审查规则\n\n给 AI 建议用（文件说明，不导入）。\n\n## 审查边界\n\n- 只给建议\n"),
    ("rules/qbank.md", "# 题库维护\n"),
    ("rules/interview.md", "# 准备面试与复盘\n"),
    ("projects/project-orange.md", "# 橙子识别 经历档案\n\n- 素材：橙子识别\n- 素材：Orange Classifier: Fruit CNN\n\n## 概览\n\n本人负责调参，团队 3 人。\n"),
    ("FACTS.md", "# 已确认事实\n\n说明（不导入）。\n\n## 个人事实\n\n- **到岗时间**：2027 年后到岗\n- **语言成绩**：CET-4\n"),
    ("SEEKER.md", "# Seeker 工作准则\n\n文件说明。\n\n## 定位\n\n只做指定的事。\n\n## 维护简历素材\n\n用于：素材润色\n\n先读素材。\n"),
    ("TRANSFER.md", "# 迁移安排\n"),
    ("问卷.docx", "not really a docx"),
    (".DS_Store", ""),
];

fn seeker_folder(name: &str) -> TempDir {
    let dir = temp_dir(name);
    for (path, text) in FILES {
        let p = dir.join("transfer").join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

fn imported(c: &Connection, root: &Path) -> Workspace {
    let preview = import::preview(&root.join("transfer"), &read(c).unwrap().catalog.records).unwrap();
    for item in &preview.items {
        catalog::put(c, &item.incoming).unwrap();
    }
    read(c).unwrap()
}

fn record<'a>(w: &'a Workspace, title: &str) -> &'a Record {
    w.catalog.records.iter().find(|r| r.title == title).unwrap_or_else(|| panic!("{title}"))
}

#[test]
fn transfer_files_become_records_and_a_second_import_shows_only_what_changed() {
    let dir = seeker_folder("seeker-import");
    let preview = import::preview(&dir.join("transfer"), &[]).unwrap();
    let mut skipped = preview.skipped.clone();
    skipped.sort();
    assert_eq!(skipped, ["TRANSFER.md：不是要导入的资料".to_string(), "cn-modules/0 - 归档.md：归档的不导入".into(), "问卷.docx：只导入 Markdown 和文本".into()]);
    let got: Vec<(&str, &str, &str)> = preview.items.iter().map(|i| (i.incoming.kind.as_str(), i.incoming.category.as_str(), i.incoming.title.as_str())).collect();
    assert_eq!(
        got,
        [
            ("fact", "个人事实", "到岗时间"),
            ("fact", "个人事实", "语言成绩"),
            ("rule", "共用准则", "定位"),
            ("rule", "素材润色", "维护简历素材"),
            ("profile", "基本信息", "基本信息"),
            ("material", "教育", "示例大学 · 信息学院"),
            ("material", "教育", "示例学院 · 经济系"),
            ("material", "技能", "技能证书"),
            ("material", "项目", "柚子笔记"),
            ("material", "项目", "橙子识别"),
            ("material", "工作", "示例证券 · 研究所"),
            ("application", "网申", "为什么选择我们"),
            ("fact", "经历档案", "橙子识别 经历档案"),
            ("rule", "素材润色", "事实与职责"),
            ("rule", "素材润色", "有效个人约定"),
            ("rule", "整体审查", "审查边界"),
        ]
    );
    let at = |title: &str| &preview.items.iter().find(|i| i.incoming.title == title).unwrap().incoming;
    let notes = at("柚子笔记");
    assert_eq!((notes.role.as_str(), notes.text.as_str(), notes.time.as_str(), notes.location.as_str(), notes.stack.as_str()), ("独立开发", "读书笔记整理工具", "2026.03 - 至今", "", "Rust、SQLite"));
    assert_eq!(notes.points.iter().map(|p| (p.id.as_str(), p.text.as_str())).collect::<Vec<_>>(), [("数据整理", "**数据整理**：把零散笔记合成卷"), ("检索改进", "**检索改进**：按意思搜索")]);
    assert_eq!(notes.choosable().iter().map(|p| p.text.as_str()).collect::<Vec<_>>()[0], "技术栈：Rust、SQLite");
    assert_eq!(notes.source.as_deref(), Some("cn-modules/4 - 项目经历.md#柚子笔记:1"));
    assert_eq!((at("橙子识别").location.as_str(), at("橙子识别").role.as_str()), ("上海", "课程项目"));
    let work = at("示例证券 · 研究所");
    assert_eq!((work.role.as_str(), work.time.as_str(), work.location.as_str(), work.points.len()), ("研究部实习生", "2024.01 - 2024.02", "杭州", 2));
    let school = at("示例学院 · 经济系");
    assert_eq!((school.role.as_str(), school.points.len(), school.text.as_str()), ("金融学 · 本科", 2, ""));
    assert_eq!((at("技能证书").points.len(), at("技能证书").time.as_str()), (2, ""));
    assert_eq!(at("基本信息").fields.iter().map(|f| f.label.as_str()).collect::<Vec<_>>(), ["姓名", "工作地点", "工作邮箱", "个人邮箱", "联系电话", "GitHub", "个人网站"]);
    // rules come in as points; a 有效个人约定 section is one rule, a point per bullet
    let texts = |t: &str| at(t).points.iter().map(|p| p.text.clone()).collect::<Vec<_>>();
    assert_eq!((texts("事实与职责"), texts("维护简历素材"), texts("有效个人约定")), (vec!["不编造事实".to_string()], vec!["先读素材。".to_string()], vec!["自维护内容：基本信息由用户自维护".to_string(), "不催稿".to_string()]));
    assert!(at("事实与职责").text.is_empty());
    assert_eq!((at("到岗时间").text.as_str(), at("为什么选择我们").text.as_str()), ("2027 年后到岗", "示例答案。"));
    let dossier = at("橙子识别 经历档案");
    assert_eq!((dossier.material.as_ref(), dossier.text.as_str()), (Some(&at("橙子识别").id), "## 概览\n\n本人负责调参，团队 3 人。"));

    let c = memory_db();
    let w = imported(&c, &dir);
    assert_eq!(w.catalog.records.len(), 16);
    // the same files again: nothing to do; a changed one: that item, against the current one
    let same = import::preview(&dir.join("transfer"), &w.catalog.records).unwrap();
    assert!(same.items.is_empty() && same.gone.is_empty());
    std::fs::write(dir.join("transfer/cn-modules/4 - 项目经历.md"), FILES[4].1.replace("两轮调参", "三轮调参")).unwrap();
    let again = import::preview(&dir.join("transfer"), &w.catalog.records).unwrap();
    assert_eq!(again.items.len(), 1);
    let item = &again.items[0];
    assert_eq!(item.current.as_ref().unwrap().id, item.incoming.id);
    catalog::put(&c, &item.incoming).unwrap();
    let orange = record(&read(&c).unwrap(), "橙子识别").clone();
    assert_eq!((orange.revision, orange.points[0].text.as_str()), (2, "**模型迭代**：三轮调参"));
    assert_eq!(read(&c).unwrap().catalog.revisions.len(), 1);
    // saving from an old revision is refused
    assert!(catalog::put(&c, &Record { revision: 1, text: "x".into(), ..orange.clone() }).unwrap_err().contains("改过"));
    // gone from the files: offered for removal (into the history); the link stays only as text
    std::fs::write(dir.join("transfer/cn-modules/4 - 项目经历.md"), FILES[4].1.split("## 橙子识别").next().unwrap()).unwrap();
    let gone = import::preview(&dir.join("transfer"), &read(&c).unwrap().catalog.records).unwrap().gone;
    assert_eq!(gone.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["橙子识别"]);
    import::confirm(&c, &[], &[orange.id.clone()]).unwrap();
    let w = read(&c).unwrap();
    assert!(!w.catalog.records.iter().any(|r| r.id == orange.id) && w.catalog.revisions.iter().any(|r| r.record_id == orange.id));
}

#[test]
fn the_model_reads_the_rules_for_its_task_and_the_linked_dossier() {
    let dir = seeker_folder("seeker-context");
    let c = memory_db();
    let w = imported(&c, &dir);
    let orange = record(&w, "橙子识别");
    let polish = model::context(&w.catalog, "素材润色", &[("别的名字", Some(orange.id.as_str()))], "cn");
    // Markdown: # 组, ## 标题, points numbered (共用准则 first, then 写作规则 and the task's group)
    assert!(polish.starts_with("# 共用准则\n\n## 定位\n1. "), "{polish}");
    for part in ["# 素材润色\n\n## 维护简历素材\n1. 先读素材。", "## 事实与职责\n1. 不编造事实", "## 有效个人约定\n1. 自维护内容：基本信息由用户自维护\n2. 不催稿", "# 已确认事实\n\n## 个人事实\n- 到岗时间：2027 年后到岗\n- 语言成绩：CET-4", "# 经历档案\n\n## 橙子识别 经历档案"] {
        assert!(polish.contains(part), "{part}\n{polish}");
    }
    assert!(!polish.contains("审查边界") && !polish.contains("【"));
    let review = model::context(&w.catalog, "整体审查", &[("柚子笔记", Some("x"))], "cn");
    assert!(review.contains("# 整体审查\n\n## 审查边界") && !review.contains("事实与职责") && !review.contains("有效个人约定") && !review.contains("经历档案"), "{review}");
}

#[test]
fn a_draft_starts_from_the_profile_and_education_and_grows_by_adding() {
    let dir = seeker_folder("seeker-draft");
    let c = memory_db();
    let w = imported(&c, &dir);
    let d = workflow::initial(&w.catalog.records, "cn").unwrap();
    assert_eq!(d.name, "林小柚");
    assert_eq!(d.header.iter().map(|h| (h.label.as_str(), h.visible)).collect::<Vec<_>>(), [("工作邮箱", true), ("联系电话", true), ("个人网站", true), ("工作地点", true), ("个人邮箱", false), ("GitHub", false)]);
    assert_eq!(d.sections.len(), 1);
    assert_eq!((d.sections[0].title.as_str(), d.sections[0].entries.len()), ("教育背景", 2));

    // default choices leave out 技术栈 unless 纯开发岗
    let notes = record(&w, "柚子笔记").clone();
    let chosen = w.choices.iter().find(|s| s.record_id == notes.id).unwrap().clone();
    assert_eq!(chosen.point_ids, ["数据整理", "检索改进"]);
    let work = record(&w, "示例证券 · 研究所").clone();
    let picks = vec![chosen.clone(), workflow::Selection { record_id: work.id.clone(), point_ids: vec!["行业研究".into()] }];
    let d = workflow::assemble(&w.catalog.records, None, &picks, false, "cn").unwrap();
    assert_eq!(d.sections.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["教育背景", "项目经历", "工作经历"]);
    let entry = &d.sections[1].entries[0];
    assert_eq!((entry.name.as_str(), entry.role.as_str(), entry.note.as_str(), entry.points.len()), ("柚子笔记", "独立开发", "读书笔记整理工具", 2));
    save(&c, &d).unwrap();

    // 增加 keeps the draft and skips what is in it already; 重新组装 starts over (keeping the JD)
    let mut edited = d.clone();
    edited.jd = "数据分析师".into();
    edited.sections[1].entries[0].points[0].text = "改过的要点".into();
    let skills = record(&w, "技能证书").clone();
    let more = vec![chosen.clone(), workflow::Selection { record_id: skills.id.clone(), point_ids: vec!["编程语言".into()] }];
    let added = workflow::assemble(&w.catalog.records, Some(edited.clone()), &more, false, "cn").unwrap();
    assert_eq!(added.sections.len(), 4);
    assert_eq!(added.sections[1].entries.len(), 1);
    assert_eq!(added.sections[1].entries[0].points[0].text, "改过的要点");
    assert_eq!(added.sections[3].title, "专业技能");
    let again = workflow::assemble(&w.catalog.records, Some(edited), &[chosen.clone()], true, "cn").unwrap();
    assert_eq!((again.jd.as_str(), again.sections.len()), ("数据分析师", 2));
    assert_eq!(again.sections[1].entries[0].points[0].text, "**数据整理**：把零散笔记合成卷");
    // a choice made on points that are gone
    let stale = workflow::Selection { record_id: notes.id.clone(), point_ids: vec!["没有".into()] };
    assert!(workflow::assemble(&w.catalog.records, None, &[stale], false, "cn").is_err());

    // sorting within one group only; points stay in their entry
    let mut d = added.clone();
    let ids: Vec<String> = d.sections.iter().rev().map(|s| s.id.clone()).collect();
    workflow::reorder(&mut d, "section", None, &ids).unwrap();
    assert_eq!(d.sections[0].title, "专业技能");
    let e = d.sections.iter().find(|s| s.title == "项目经历").unwrap().entries[0].clone();
    let points: Vec<String> = e.points.iter().rev().map(|p| p.id.clone()).collect();
    workflow::reorder(&mut d, "point", Some(&e.id), &points).unwrap();
    assert!(workflow::reorder(&mut d, "point", Some(&e.id), &points[..1]).is_err());
    assert!(workflow::reorder(&mut d, "entry", Some(&e.id), &points).is_err());

    // deleting a section takes its entries; a point goes alone
    let work_section = d.sections.iter().find(|s| s.title == "工作经历").unwrap().id.clone();
    let edu_point = d.sections.iter().find(|s| s.title == "教育背景").unwrap().entries[1].points[1].id.clone();
    delete_blocks(&mut d, &[work_section, edu_point, "header-4".into()]);
    assert_eq!(d.sections.len(), 3);
    assert_eq!(d.sections.iter().find(|s| s.title == "教育背景").unwrap().entries[1].points.len(), 1);
    assert_eq!(d.header.len(), 5);
    validate(&d).unwrap();
}

#[test]
fn source_changes_keep_the_chosen_points_and_edits_undo_in_one_step() {
    let dir = seeker_folder("seeker-undo");
    let c = memory_db();
    let w = imported(&c, &dir);
    let notes = record(&w, "柚子笔记").clone();
    let pick = workflow::Selection { record_id: notes.id.clone(), point_ids: vec!["数据整理".into()] };
    save(&c, &workflow::assemble(&w.catalog.records, None, &[pick], false, "cn").unwrap()).unwrap();

    // the material gets a new version: the entry would take its new text, still one point
    let mut newer = notes.clone();
    newer.points[0].text = "**数据整理**：合成卷并去重".into();
    newer.text = "读书笔记整理与检索工具".into();
    catalog::put(&c, &newer).unwrap();
    let changes = workflow::source_changes(&read(&c).unwrap(), "cn");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].after.points.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["**数据整理**：合成卷并去重"]);
    assert_eq!(changes[0].after.note, "读书笔记整理与检索工具");
    assert_eq!(changes[0].after.source.as_ref().unwrap().revision, 2);

    // a batch delete is one undo step; undo brings the draft back as it was
    let before = read(&c).unwrap().cn.draft.unwrap();
    let mut h = crate::undo::History::default();
    let ids: Vec<String> = before.sections.iter().map(|s| s.id.clone()).collect();
    crate::undo::tests::step(&c, &mut h, "删除简历内容", |c| {
        let mut d = read(c)?.cn.draft.unwrap();
        delete_blocks(&mut d, &ids);
        save(c, &d)
    });
    assert!(read(&c).unwrap().cn.draft.unwrap().sections.is_empty());
    assert_eq!(crate::undo::undo(&c, &mut h).unwrap().as_deref(), Some("删除简历内容"));
    assert_eq!(read(&c).unwrap().cn.draft.unwrap(), before);
    assert_eq!(crate::undo::undo(&c, &mut h).unwrap(), None);
}

/// The draft as the tests' template slots expect it.
fn full_workspace(c: &Connection, dir: &Path) -> Workspace {
    let w = imported(c, dir);
    let picks: Vec<workflow::Selection> = w.choices.iter().filter(|s| w.catalog.records.iter().any(|r| r.id == s.record_id && r.lang == "cn")).cloned().collect();
    let mut d = workflow::assemble(&w.catalog.records, None, &picks, false, "cn").unwrap();
    d.jd = "数据分析".into();
    save(c, &d).unwrap();
    workflow::baseline(c, &d).unwrap();
    super::set_settings(c, "folder", &dir.to_string_lossy()).unwrap();
    super::set_settings(c, "options", r#"{"selectionModel":"local:qwen","thinking":true}"#).unwrap();
    read(c).unwrap()
}

/// The test template's paragraphs, chosen like the user's (word.rs `resume_template_docx`).
fn template() -> output::Template {
    let slots = output::Slots { name: 0, contact: 1, contact2: Some(2), section: 3, education: Some(4), note: Some(5), point: 6, entry: 7, summary: None, blank: None };
    output::Template { path: "t.docx".into(), slots, paragraphs: word::resume_template_items(&word::tests::resume_template_docx()).unwrap() }
}

fn bank() -> output::JobSnapshot {
    output::JobSnapshot { id: 4, company: "柚子银行股份有限公司".into(), position: Some("数据分析师".into()), location: None, link: None, note: Some("  岗位职责：分析信贷数据  ".into()), created_at: "2026-10-01T00:00:00.000Z".into() }
}

#[test]
fn the_resume_fills_the_template_and_export_never_overwrites() {
    let dir = seeker_folder("seeker-export");
    let c = memory_db();
    let w = full_workspace(&c, &dir);
    let mut draft = w.cn.draft.clone().unwrap();
    draft.header.iter_mut().find(|h| h.label == "GitHub").unwrap().visible = true;
    save(&c, &draft).unwrap();
    let t = template();
    let lines = output::lines(&draft, &t);
    let text: Vec<String> = lines.iter().map(|(i, t)| format!("{i} {t}")).collect();
    // links go on the second contact line by name; the template's own separator
    assert_eq!(
        text[..9],
        ["0 林小柚", "1 work@example.com  ·  +86 100 0000 0000  ·  上海", "2 个人网站：https://example.com  ·  GitHub：https://github.com/example", "3 教育背景", "4 示例大学 · 信息学院\t2025.07 - 至今", "5 数据科学 · 硕士", "6 **主修课程**：机器学习、数据库", "4 示例学院 · 经济系\t2021.09 - 2025.06", "5 金融学 · 本科"]
    );
    // without a second contact line: everything on the first, in order
    let mut one = t.clone();
    one.slots.contact2 = None;
    assert_eq!(output::lines(&draft, &one)[1].1, "work@example.com  ·  +86 100 0000 0000  ·  https://example.com  ·  上海  ·  https://github.com/example");
    assert!(["https://a.com", "github.com/x", "www.example.cn", "example.com"].iter().all(|v| output::is_link(v)));
    assert!(["me@example.com", "+86 100 0000 0000", "上海", "v1.2", "2025.07"].iter().all(|v| !output::is_link(v)));
    assert_eq!(output::separator(Some("[电话]｜[邮箱]")), "｜");
    assert_eq!(output::separator(Some("[电话] [邮箱]")), " · ");
    // 技能: points only; 项目: heading 名称 ⇥ 角色 ⇥ 时间 (no 地点), the note, the points
    let skills = text.iter().position(|l| l == "3 专业技能").unwrap();
    assert_eq!(text[skills + 1], "6 **编程语言**：Python、SQL");
    let project = text.iter().position(|l| l.starts_with("7 柚子笔记")).unwrap();
    assert_eq!(text[project..project + 3], ["7 柚子笔记\t独立开发\t2026.03 - 至今", "5 读书笔记整理工具", "6 **数据整理**：把零散笔记合成卷"]);
    let docx = word::resume_from_template(&word::tests::resume_template_docx(), &lines).unwrap();
    let items = word::read_docx(&docx).unwrap().items;
    assert_eq!(items[0].text, "**林小柚**");
    assert!(items.iter().any(|i| i.text == "**柚子笔记\t独立开发\t**2026.03 - 至今"), "{:?}", items.iter().map(|i| &i.text).collect::<Vec<_>>());
    assert!(items.iter().any(|i| i.text == "- **数据整理**：把零散笔记合成卷"));

    // 另存: once; the same name again is refused and leaves no record; no page count
    let out = dir.join("cv");
    std::fs::create_dir_all(&out).unwrap();
    let job = |id: i64| output::JobSnapshot { id, company: "示例公司".into(), position: Some("分析师".into()), location: None, link: None, note: None, created_at: "2026-10-01T00:00:00.000Z".into() };
    let first = output::save_export(&c, &draft, &docx, &out, "简历.docx", Some(job(7))).unwrap();
    assert_eq!(std::fs::read(out.join("简历.docx")).unwrap(), docx);
    assert_eq!(first.path, out.join("简历.docx").to_string_lossy());
    assert!(serde_json::to_value(&first).unwrap().get("pages").is_none());
    assert!(output::save_export(&c, &draft, &docx, &out, "简历.docx", None).unwrap_err().contains("同名"));
    assert!(output::save_export(&c, &draft, &docx, &out, "../x.docx", None).is_err());
    let mut other = draft.clone();
    other.name = "别人".into();
    assert!(output::save_export(&c, &other, &docx, &out, "简历3.docx", None).unwrap_err().contains("改过"));
    assert_eq!(read(&c).unwrap().output.exports.len(), 1);
    assert!(!out.join("简历3.docx").exists());

    // a second export linked to the same job becomes the one used; the first link stays
    let second = output::save_export(&c, &draft, &docx, &out, "简历-2.docx", Some(job(7))).unwrap();
    let links = read(&c).unwrap().output.links;
    assert_eq!(links.iter().map(|l| (l.path.clone(), l.current)).collect::<Vec<_>>(), [(second.path.clone(), true), (first.path.clone(), false)]);
    output::associate(&c, &first.path, &draft.jd, job(7), true).unwrap();
    let links = read(&c).unwrap().output.links;
    assert_eq!(links.iter().map(|l| (l.path.clone(), l.current)).collect::<Vec<_>>(), [(second.path.clone(), false), (first.path.clone(), true)]);
    assert_eq!(links[1].jd, "数据分析");

    // an export from before (with its page count) still reads
    let mut old = serde_json::to_value(&first).unwrap();
    old["id"] = "old".into();
    old["pages"] = 1.into();
    old["renderer"] = "LibreOffice".into();
    c.execute("INSERT INTO seeker_exports (id, body) VALUES ('old', ?1)", [old.to_string()]).unwrap();
    assert_eq!(read(&c).unwrap().output.exports.len(), 3);
}

#[test]
fn a_job_brings_its_jd_and_reassembling_keeps_it() {
    let dir = seeker_folder("seeker-job");
    let c = memory_db();
    imported(&c, &dir);
    // no draft yet: a new one for the job, its notes as the JD
    use_job(&c, Some(&bank()), "cn").unwrap();
    let w = read(&c).unwrap();
    let d = w.cn.draft.clone().unwrap();
    assert_eq!((d.jd.as_str(), d.job.as_ref().map(|j| (j.id, j.created_at.as_str()))), ("岗位职责：分析信贷数据", Some((4, "2026-10-01T00:00:00.000Z"))));
    assert_eq!((d.sections.len(), d.sections[0].category.as_str()), (1, "教育"));
    assert_eq!(w.cn.baseline.as_ref(), Some(&d));
    // a job without notes leaves the JD; the baseline stays
    use_job(&c, Some(&output::JobSnapshot { id: 5, note: None, ..bank() }), "cn").unwrap();
    let w = read(&c).unwrap();
    let d = w.cn.draft.clone().unwrap();
    assert_eq!((d.jd.as_str(), d.job.as_ref().map(|j| j.id)), ("岗位职责：分析信贷数据", Some(5)));
    assert_ne!(w.cn.baseline.as_ref(), Some(&d));
    // 重新组装 keeps the JD and the job
    let again = workflow::assemble(&w.catalog.records, Some(d.clone()), &w.choices, true, "cn").unwrap();
    assert_eq!((&again.jd, &again.job), (&d.jd, &d.job));
    // for no job: the JD stays; a draft for none doesn't write the field
    use_job(&c, None, "cn").unwrap();
    let d = read(&c).unwrap().cn.draft.unwrap();
    assert!(d.job.is_none() && d.jd == "岗位职责：分析信贷数据");
    assert!(!serde_json::to_string(&d).unwrap().contains("\"job\""));
}

const KNOWLEDGE: &[(&str, &str)] = &[
    ("招聘索引.md", "# 招聘资料索引\n\n| 公司 | 岗位与资料 | 地点 |\n|---|---|---|\n| 柚子银行 | [数据分析师](seek-pomelo/北京-数据分析-001/JD.md) | 北京 |\n| 橙子科技 | [软件开发工程师（校招）](seek-orange/北京-软件开发-002/JD.md) | 北京 |\n| **橙子科技** | [测试开发](seek-orange/天津-测试开发-003/JD.md) | 天津 |\n| 排除 | [别的岗位](https://example.com/jobs/1) | 上海 |\n\n## 公司资料\n\n- [柚子银行](seek-pomelo/公司与招聘.md)：校招时间。\n- [Orange Tech](seek-orange/公司与招聘.md)：英文名。\n"),
    ("seek-pomelo/公司与招聘.md", "# 柚子银行\n\n柚子银行的校招每年九月开始，看重数据分析能力。\n"),
    ("seek-pomelo/北京-数据分析-001/JD.md", "# 数据分析师\n\n负责信贷数据分析。\n"),
    ("seek-orange/公司与招聘.md", "# 橙子科技\n\n做水果识别的软件公司。\n"),
    ("seek-orange/北京-软件开发-002/JD.md", "# 软件开发工程师\n"),
    ("seek-orange/天津-测试开发-003/JD.md", "# 测试开发\n"),
    ("seek-keyreply/seek-keyreply.md", "# 一个项目\n"),
    (".hidden/x.md", "x"),
    ("resume-cn-001.webp", "img"),
];

#[test]
fn jobs_find_their_company_in_knowledge_by_name() {
    let dir = seeker_folder("seeker-company");
    for (path, text) in KNOWLEDGE {
        let p = dir.join("knowledge").join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let list = company::companies(&dir.join("knowledge")).unwrap();
    let shown: Vec<(&str, &str)> = list.iter().map(|c| (c.folder.as_str(), c.name.as_str())).collect();
    assert_eq!(shown, [("seek-keyreply", "seek-keyreply"), ("seek-orange", "橙子科技"), ("seek-pomelo", "柚子银行")]);
    assert_eq!(list[1].names, ["orange", "橙子科技", "Orange Tech"]);
    let m = |name: &str| company::matched(name, &list);
    assert_eq!(m("柚子银行股份有限公司").as_deref(), Some("seek-pomelo"));
    assert_eq!(m("橙子").as_deref(), Some("seek-orange"));
    assert_eq!(m("Orange Tech (Beijing) Ltd.").as_deref(), Some("seek-orange"));
    assert_eq!(m("Key Reply").as_deref(), Some("seek-keyreply"));
    assert_eq!(m("Keyreplyx"), None);
    assert_eq!(m("Orangery Tech"), None);
    assert_eq!(m("柚"), None);
    assert_eq!(m("苹果公司"), None);
    assert_eq!(m("柚子银行和橙子科技"), None);

    // a name the user gives (Seeker 设置): shown and matched; blank goes back to the found one
    let folders: Vec<String> = list.iter().map(|c| c.folder.clone()).collect();
    let given = |pairs: &[(&str, &str)]| pairs.iter().map(|(f, n)| (f.to_string(), n.to_string())).collect::<std::collections::BTreeMap<_, _>>();
    let names = company::clean_names(&given(&[("seek-keyreply", " 钥匙回复 "), ("seek-orange", ""), ("seek-gone", "已删的")]), &folders).unwrap();
    assert_eq!(names, given(&[("seek-keyreply", "钥匙回复")]));
    let renamed = company::named(list.clone(), &names);
    assert_eq!(renamed.iter().map(|c| (c.name.as_str(), c.auto.as_str())).collect::<Vec<_>>(), [("钥匙回复", "seek-keyreply"), ("橙子科技", "橙子科技"), ("柚子银行", "柚子银行")]);
    assert_eq!(company::matched("钥匙回复新加坡", &renamed).as_deref(), Some("seek-keyreply"));
    assert_eq!(company::matched("Key Reply", &renamed).as_deref(), Some("seek-keyreply"));
    assert!(company::clean_names(&given(&[("seek-keyreply", "柚子"), ("seek-pomelo", "柚 子")]), &folders).unwrap_err().contains("都叫"));
    // kept by the settings command, through the data file
    let s = memory_db();
    set_settings(&s, "options", &serde_json::to_string(&catalog::Settings { company_names: names.clone(), ..Default::default() }).unwrap()).unwrap();
    assert_eq!(settings_of(&s).unwrap().company_names, names);
    let b = memory_db();
    replace(&b, &read(&s).unwrap()).unwrap();
    assert_eq!(settings_of(&b).unwrap().company_names, names);

    // the user's choice wins over the name and goes back to it when cleared; undone with the rest
    let c = memory_db();
    company::choose(&c, &bank(), Some("seek-orange")).unwrap();
    company::choose(&c, &bank(), Some("")).unwrap();
    assert_eq!(company::load(&c).unwrap(), [company::Choice { job_id: 4, job_created_at: "2026-10-01T00:00:00.000Z".into(), folder: "".into() }]);
    let found = company::JobCompany { companies: list.clone(), auto: m(&bank().company), chosen: Some("".into()) };
    assert_eq!(found.folder(), None);
    company::choose(&c, &bank(), None).unwrap();
    assert!(company::load(&c).unwrap().is_empty());
    assert_eq!(company::JobCompany { chosen: None, ..found }.folder(), Some("seek-pomelo"));
}

#[test]
fn the_workspace_goes_through_the_data_file_whole() {
    let dir = seeker_folder("seeker-transfer");
    let a = memory_db();
    let w = full_workspace(&a, &dir);
    let draft = w.cn.draft.clone().unwrap();
    let export = output::Export { id: "e1".into(), label: "简历.docx".into(), path: "/x/简历.docx".into(), lang: "cn".into(), materials: output::materials_of(&draft), created_at: "2026-10-02T00:00:00.000Z".into(), draft: None };
    let old_template = output::Template { path: "/x/t.docx".into(), slots: template().slots, paragraphs: vec![Some("[姓名]".into()), None] };
    a.execute("INSERT INTO seeker_exports (id, body) VALUES ('e1', ?1)", [serde_json::to_string(&export).unwrap()]).unwrap();
    output::associate(&a, &export.path, &draft.jd, output::JobSnapshot { id: 3, company: "示例".into(), position: None, location: None, link: None, note: None, created_at: "2026-09-01T00:00:00.000Z".into() }, true).unwrap();
    a.execute("UPDATE seeker_settings SET template = ?1", [serde_json::to_string(&old_template).unwrap()]).unwrap();
    rename_group_for_test(&a);
    let mut record = catalog::load(&a).unwrap().records[0].clone();
    record.text.push_str("\n补一句");
    catalog::put(&a, &record).unwrap();
    // the draft is for a job, whose company the user chose
    use_job(&a, Some(&bank()), "cn").unwrap();
    company::choose(&a, &bank(), Some("seek-pomelo")).unwrap();

    let rec = crate::picky::transfer::export(&a).unwrap();
    let json = serde_json::to_value(&rec).unwrap();
    assert!(json["seeker"].get("warnings").is_none() && json["seeker"].get("choices").is_none());
    let seeker = rec.seeker.as_ref().unwrap();
    assert_eq!((seeker.catalog.records.len(), seeker.catalog.revisions.len(), seeker.catalog.groups.len(), seeker.output.exports.len(), seeker.output.links.len()), (16, 1, 1, 1, 1)); // the two 个人约定 are one rule now
    assert!(!seeker.output.exports[0].materials.is_empty());
    assert!(seeker.output.template.is_some() && seeker.catalog.settings.thinking && seeker.cn.baseline.is_some());
    assert_eq!((seeker.job_companies.len(), seeker.cn.draft.as_ref().unwrap().job.as_ref().map(|j| j.id)), (1, Some(4)));

    let b = memory_db();
    crate::picky::transfer::replace(&b, &serde_json::from_value(json).unwrap()).unwrap();
    assert_eq!(crate::picky::transfer::export(&b).unwrap(), rec);
    // a file from before Seeker leaves it as it is
    crate::picky::transfer::replace(&b, &crate::picky::transfer::PickyRecord { seeker: None, ..rec.clone() }).unwrap();
    assert_eq!(crate::picky::transfer::export(&b).unwrap(), rec);
    // a broken draft is refused before anything is written
    let mut bad = rec.clone();
    bad.seeker.as_mut().unwrap().cn.draft.as_mut().unwrap().header[1].id = "header-0".into();
    assert!(crate::picky::transfer::validate(&bad).is_err());
}

#[test]
fn records_saved_before_format_21_still_read() {
    let c = memory_db();
    let old = |id: &str, kind: &str, category: &str, title: &str, text: &str, points: &str| format!(r#"{{"id":"{id}","kind":"{kind}","category":"{category}","title":"{title}","text":"{text}","time":"","role":"","location":"","points":{points},"fields":[],"source":null,"revision":1,"enabled":true}}"#);
    for (id, body) in [
        ("m", old("m", "material", "项目", "旧项目", "", r#"[{"id":"技术栈","text":"技术栈：Rust"},{"id":"a","text":"**做事**：一条"}]"#)),
        ("r", old("r", "rule", "素材润色", "中文素材润色规则", "## 事实\\n- 不编造", "[]")),
        ("d", old("d", "fact", "经历档案", "旧项目 经历档案", "本人负责。", "[]")),
        ("f", old("f", "fact", "已确认事实", "已确认事实", "| 事项 | 口径 |", "[]")),
    ] {
        c.execute("INSERT INTO seeker_records (id, source, body, revision) VALUES (?1, NULL, ?2, 1)", params![id, body]).unwrap();
    }
    let w = read(&c).unwrap();
    let m = record(&w, "旧项目");
    assert_eq!((m.stack.as_str(), m.material.as_ref()), ("", None));
    assert_eq!(m.choosable().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["技术栈", "a"]);
    // the old 技术栈 point is still left out unless 纯开发岗
    assert_eq!(w.choices.iter().find(|s| s.record_id == "m").unwrap().point_ids, ["a"]);
    let ctx = model::context(&w.catalog, "素材润色", &[("旧项目", Some("m"))], "cn");
    // an old rule's text read as points (its sub-heading too)
    for part in ["## 中文素材润色规则\n1. 事实\n2. 不编造", "# 已确认事实\n\n## 已确认事实\n- 已确认事实：| 事项 | 口径 |", "# 经历档案\n\n## 旧项目 经历档案\n本人负责。"] {
        assert!(ctx.contains(part), "{part}\n{ctx}");
    }
}

/// English files shaped like the real transfer/en-modules after 阶段 ③'s tidy-up (all fictional).
const EN_FILES: &[(&str, &str)] = &[
    ("en-modules/0-Archived.md", "# 0-Archived\n#职业/英文简历\n\n**Old Role**\n"),
    ("en-modules/1-SUMMARY.md", "# 1 - Summary\n#职业/英文简历\n\n## Data Summary (Default)\n\nData student who builds tools.\n\n## Agent Summary\n\nAgent builder with a live deployment.\n"),
    ("en-modules/2-CONTACT.md", "# 2 - Contact\n#职业/英文简历\n\n- Name: LIN XIAOYOU\n- Location: Shanghai\n- Email: work@example.com\n- Personal Email: me@example.com\n- Phone: +86 100 0000 0000\n- GitHub: https://github.com/example\n- LinkedIn: https://linkedin.com/in/example\n"),
    ("en-modules/3-SKILLS.md", "# 3 - Skills\n#职业/英文简历\n\n- **Programming**: Python, SQL\n- **Languages**: English, Chinese\n"),
    ("en-modules/4-EDUCATION.md", "# 4 - Education\n#职业/英文简历\n\n## Example University\n- 专业：MSc in Data Science\n- 时间：Jul 2025 – Present\n\n- Coursework: Machine Learning, Databases\n"),
    ("en-modules/5-PROJECTS.md", "# 5 - Projects\n#职业/英文简历\n\n说明（不导入）。\n\n## Pomelo Notes: Reading Notes Tool\n- 角色：Sole Developer\n- 时间：Mar 2026 – Present\n- 技术栈：Rust, SQLite\n\n- Built a notes merger\n- Added meaning search\n- Shipped it\n- Wrote docs\n\n## Orange Classifier: Fruit CNN\n- 角色：Team Leader\n- 时间：Oct 2025 – Dec 2025\n\n- Tuned the model twice\n\n## Third Project\n- 时间：2024\n\n- Helped\n"),
    ("en-modules/6-EXPERIENCE.md", "# 6 - Experience\n#职业/英文简历\n\n## Example Securities, Research\n- 职位：Research Intern\n- 时间：Jan 2024 – Feb 2024\n- 地点：Hangzhou\n\n- Summarised reports\n"),
    ("en-modules/7-CERTIFICATIONS.md", "# 7 - Certifications\n#职业/英文简历\n\n- **IELTS Academic – Overall 7.5** · May 2025\n- **Example Certificate** Example Org · Jan 2026\n"),
    ("rules/en-modules.md", "# 英文素材润色规则\n\n## 适用范围\n\n正文保持英文。\n\n## 有效个人约定\n\n- **自维护与冻结**：CONTACT 由用户维护\n"),
    ("rules/en-resume.md", "# 英文简历整体审查规则\n\n## 产物\n\n- Resume 一页\n"),
];

fn english_folder(name: &str) -> TempDir {
    let dir = seeker_folder(name);
    for (path, text) in EN_FILES {
        let p = dir.join("transfer").join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

#[test]
fn english_files_come_in_as_english_and_dossiers_hang_on_both_languages() {
    let dir = english_folder("seeker-en-import");
    let preview = import::preview(&dir.join("transfer"), &[]).unwrap();
    assert!(preview.skipped.contains(&"en-modules/0-Archived.md：归档的不导入".to_string()), "{:?}", preview.skipped);
    let en: Vec<(&str, &str, &str)> = preview.items.iter().filter(|i| i.incoming.lang == "en").map(|i| (i.incoming.kind.as_str(), i.incoming.category.as_str(), i.incoming.title.as_str())).collect();
    assert_eq!(
        en,
        [
            ("material", "摘要", "Data Summary (Default)"),
            ("material", "摘要", "Agent Summary"),
            ("profile", "基本信息", "Contact"),
            ("material", "技能", "Skills"),
            ("material", "教育", "Example University"),
            ("material", "项目", "Pomelo Notes: Reading Notes Tool"),
            ("material", "项目", "Orange Classifier: Fruit CNN"),
            ("material", "项目", "Third Project"),
            ("material", "工作", "Example Securities, Research"),
            ("material", "证书", "Certifications"),
            ("rule", "素材润色", "适用范围"),
            ("rule", "素材润色", "有效个人约定"),
            ("rule", "整体审查", "产物"),
        ]
    );
    let at = |title: &str| &preview.items.iter().find(|i| i.incoming.title == title).unwrap().incoming;
    assert_eq!(at("Agent Summary").text, "Agent builder with a live deployment.");
    assert_eq!(at("Contact").fields.iter().map(|f| f.label.as_str()).collect::<Vec<_>>(), ["Name", "Location", "Email", "Personal Email", "Phone", "GitHub", "LinkedIn"]);
    assert_eq!(at("Contact").fields[5].value, "https://github.com/example");
    let school = at("Example University");
    assert_eq!((school.role.as_str(), school.time.as_str(), school.points[0].text.as_str()), ("MSc in Data Science", "Jul 2025 – Present", "Coursework: Machine Learning, Databases"));
    let notes = at("Pomelo Notes: Reading Notes Tool");
    assert_eq!((notes.role.as_str(), notes.points.len(), notes.points[0].id.as_str()), ("Sole Developer", 4, "1"));
    assert_eq!(notes.choosable()[0].text, "Tech Stack: Rust, SQLite");
    assert_eq!(at("Example Securities, Research").location, "Hangzhou");
    assert_eq!(at("Certifications").points.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["IELTS Academic – Overall 7.5", "Example Certificate"]);
    // SEEKER.md, FACTS.md and projects/ are both languages'; Chinese files stay Chinese
    assert_eq!((at("定位").lang.as_str(), at("维护简历素材").lang.as_str(), at("到岗时间").lang.as_str(), at("事实与职责").lang.as_str(), at("柚子笔记").lang.as_str()), ("", "", "", "cn", "cn"));
    let dossier = at("橙子识别 经历档案");
    assert_eq!((dossier.material.as_ref(), dossier.material_en.as_ref()), (Some(&at("橙子识别").id), Some(&at("Orange Classifier: Fruit CNN").id)));

    // the English rules and the dossier reach English tasks only (and the shared ones both)
    let c = memory_db();
    let w = imported(&c, &dir);
    let orange_en = record(&w, "Orange Classifier: Fruit CNN").id.clone();
    let polish = model::context(&w.catalog, "素材润色", &[("Orange Classifier: Fruit CNN", Some(orange_en.as_str()))], "en");
    assert!(polish.starts_with("# 共用准则\n\n## 定位"), "{polish}");
    for part in ["## 维护简历素材", "## 适用范围\n1. 正文保持英文。", "## 有效个人约定\n1. 自维护与冻结：CONTACT 由用户维护", "# 已确认事实", "# 经历档案\n\n## 橙子识别 经历档案"] {
        assert!(polish.contains(part), "{part}\n{polish}");
    }
    assert!(!polish.contains("事实与职责") && !polish.contains("不催稿"), "{polish}");
    let cn = model::context(&w.catalog, "素材润色", &[], "cn");
    assert!(cn.contains("事实与职责") && !cn.contains("适用范围") && !cn.contains("自维护与冻结"), "{cn}");
    // a second import of the same files has nothing to do
    let same = import::preview(&dir.join("transfer"), &w.catalog.records).unwrap();
    assert!(same.items.is_empty() && same.gone.is_empty(), "{:?}", same.items.iter().map(|i| &i.incoming.title).collect::<Vec<_>>());
}

#[test]
fn an_english_resume_has_a_summary_its_own_order_and_reminders_apart_from_the_chinese_one() {
    let dir = english_folder("seeker-en-draft");
    let c = memory_db();
    let w = imported(&c, &dir);
    let d = workflow::initial(&w.catalog.records, "en").unwrap();
    assert_eq!((d.lang.as_str(), d.form.as_str(), d.name.as_str()), ("en", "resume", "LIN XIAOYOU"));
    assert_eq!(d.header.iter().map(|h| (h.label.as_str(), h.visible)).collect::<Vec<_>>(), [("Location", true), ("Email", true), ("Phone", true), ("GitHub", true), ("Personal Email", false), ("LinkedIn", false)]);
    assert_eq!(d.sections.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Profile Summary", "Education"]);
    assert_eq!(d.sections[0].entries[0].note, "Data student who builds tools.");
    assert_eq!(d.sections[1].entries[0].name, "Example University");

    // Chinese materials can't go into an English draft; two Summaries can't go in at once
    let pick = |title: &str| workflow::Selection { record_id: record(&w, title).id.clone(), point_ids: w.choices.iter().find(|s| s.record_id == record(&w, title).id).unwrap().point_ids.clone() };
    assert!(workflow::assemble(&w.catalog.records, Some(d.clone()), &[pick("柚子笔记")], false, "en").is_err());
    assert!(workflow::assemble(&w.catalog.records, Some(d.clone()), &[pick("Agent Summary"), pick("Data Summary (Default)")], false, "en").unwrap_err().contains("Summary"));
    // sections go in the usual order; another Summary replaces the one there
    let picks = [pick("Example Securities, Research"), pick("Pomelo Notes: Reading Notes Tool"), pick("Orange Classifier: Fruit CNN"), pick("Third Project"), pick("Skills"), pick("Agent Summary"), pick("Certifications")];
    let mut d = workflow::assemble(&w.catalog.records, Some(d), &picks, false, "en").unwrap();
    assert_eq!(d.sections.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Profile Summary", "Skills", "Education", "Projects", "Experience", "Certifications"]);
    assert_eq!(d.sections[0].entries.iter().map(|e| e.note.as_str()).collect::<Vec<_>>(), ["Agent builder with a live deployment."]);
    save(&c, &d).unwrap();
    // reminders: a Resume's projects and every entry's points; a CV only the points
    let w = read(&c).unwrap();
    assert_eq!(w.en.warnings, ["Resume 建议 1–2 个项目，现在 3 个", "「Pomelo Notes: Reading Notes Tool」4 条，每段最多 3 条"]);
    assert!(w.cn.draft.is_none() && w.cn.warnings.is_empty());
    d.form = "cv".into();
    save(&c, &d).unwrap();
    assert_eq!(read(&c).unwrap().en.warnings, ["「Pomelo Notes: Reading Notes Tool」4 条，每段最多 3 条"]);
    // 重新组装 keeps Resume / CV, the JD and the job
    let again = workflow::assemble(&w.catalog.records, Some(Draft { jd: "Data analyst".into(), ..d.clone() }), &[pick("Skills")], true, "en").unwrap();
    assert_eq!((again.form.as_str(), again.jd.as_str(), again.sections.len()), ("cv", "Data analyst", 3));

    // the Chinese draft is its own: making one, a job for it, deleting in it leave the English one
    use_job(&c, Some(&bank()), "cn").unwrap();
    let w = read(&c).unwrap();
    assert_eq!((w.cn.draft.as_ref().unwrap().lang.as_str(), w.cn.draft.as_ref().unwrap().jd.as_str()), ("cn", "岗位职责：分析信贷数据"));
    assert_eq!(w.en.draft.as_ref(), Some(&d));
    let mut cn = w.cn.draft.clone().unwrap();
    let first = cn.sections[0].id.clone();
    delete_blocks(&mut cn, &[first]);
    save(&c, &cn).unwrap();
    assert_eq!(read(&c).unwrap().en.draft.as_ref(), Some(&d));
    // a draft must be one language with a form only in English
    assert!(validate(&Draft { form: String::new(), ..d.clone() }).is_err());
    assert!(validate(&Draft { lang: "jp".into(), ..d.clone() }).is_err());
    assert!(validate(&Draft { lang: "cn".into(), ..d.clone() }).is_err());
    // the export of an English draft checks the English one
    let t = output::Template { path: "t.docx".into(), slots: en_slots(), paragraphs: word::resume_template_items(&word::tests::resume_template_en_docx()).unwrap() };
    let docx = word::resume_from_template(&word::tests::resume_template_en_docx(), &output::lines(&d, &t)).unwrap();
    let out = dir.join("cv-en");
    std::fs::create_dir_all(&out).unwrap();
    let export = output::save_export(&c, &d, &docx, &out, "resume.docx", None).unwrap();
    assert_eq!(export.lang, "en");
    // the record keeps no copy of the résumé
    let body: String = c.query_row("SELECT body FROM seeker_exports WHERE id = ?1", [&export.id], |r| r.get(0)).unwrap();
    assert!(!body.contains("sections") && !body.contains("template") && body.contains("\"materials\""), "{body}");
}

fn en_slots() -> output::Slots {
    output::Slots { name: 0, contact: 1, contact2: None, section: 2, summary: Some(3), blank: Some(4), point: 5, entry: 6, note: Some(7), education: None }
}

#[test]
fn an_english_resume_writes_role_then_name_and_blank_lines_between() {
    let dir = english_folder("seeker-en-lines");
    let c = memory_db();
    let w = imported(&c, &dir);
    let pick = |title: &str| workflow::Selection { record_id: record(&w, title).id.clone(), point_ids: w.choices.iter().find(|s| s.record_id == record(&w, title).id).unwrap().point_ids.clone() };
    let d = workflow::assemble(&w.catalog.records, Some(workflow::initial(&w.catalog.records, "en").unwrap()), &[pick("Skills"), pick("Orange Classifier: Fruit CNN"), pick("Third Project"), pick("Example Securities, Research")], false, "en").unwrap();
    let t = output::Template { path: "t.docx".into(), slots: en_slots(), paragraphs: word::resume_template_items(&word::tests::resume_template_en_docx()).unwrap() };
    output::check_slots(&t.slots, &t.paragraphs).unwrap();
    let text: Vec<String> = output::lines(&d, &t).iter().map(|(i, t)| format!("{i} {t}")).collect();
    assert_eq!(
        text,
        [
            "0 LIN XIAOYOU",
            "1 Shanghai · work@example.com · +86 100 0000 0000 · https://github.com/example",
            "2 Profile Summary",
            "3 Data student who builds tools.",
            "4 ",
            "2 Skills",
            "5 **Programming**: Python, SQL",
            "5 **Languages**: English, Chinese",
            "4 ",
            "2 Education",
            "6 Example University\tJul 2025 – Present",
            "7 MSc in Data Science",
            "5 Coursework: Machine Learning, Databases",
            "4 ",
            "2 Projects",
            "6 Team Leader\tOct 2025 – Dec 2025",
            "7 Orange Classifier: Fruit CNN",
            "5 Tuned the model twice",
            "4 ",
            "6 Third Project\t2024",
            "5 Helped",
            "4 ",
            "2 Experience",
            "6 Research Intern\tJan 2024 – Feb 2024",
            "7 Example Securities, Research, Hangzhou",
            "5 Summarised reports",
        ]
    );
    let docx = word::resume_from_template(&word::tests::resume_template_en_docx(), &output::lines(&d, &t)).unwrap();
    let items = word::read_docx(&docx).unwrap().items;
    assert!(items.iter().any(|i| i.text == "**Team Leader\tOct 2025 – Dec 2025**"), "{:?}", items.iter().map(|i| &i.text).collect::<Vec<_>>());
    // the blank paragraph only as the blank line; a text slot on a blank paragraph is refused
    assert!(output::check_slots(&output::Slots { blank: Some(3), ..en_slots() }, &t.paragraphs).is_err());
    assert!(output::check_slots(&output::Slots { summary: Some(4), ..en_slots() }, &t.paragraphs).is_err());
    // the Chinese layout doesn't change with the English one's slots unset
    assert!(template().slots.summary.is_none() && !serde_json::to_string(&template().slots).unwrap().contains("blank"));
}

#[test]
fn both_languages_go_through_the_data_file_and_old_ones_share_the_seeker_rules() {
    let dir = english_folder("seeker-en-transfer");
    let a = memory_db();
    let w = full_workspace(&a, &dir);
    save(&a, &workflow::initial(&w.catalog.records, "en").unwrap()).unwrap();
    workflow::baseline(&a, &workflow::initial(&w.catalog.records, "en").unwrap()).unwrap();
    let t = output::Template { path: "/x/en.docx".into(), slots: en_slots(), paragraphs: word::resume_template_items(&word::tests::resume_template_en_docx()).unwrap() };
    a.execute("UPDATE seeker_settings SET template_en = ?1", [serde_json::to_string(&t).unwrap()]).unwrap();
    let rec = crate::picky::transfer::export(&a).unwrap();
    let json = serde_json::to_value(&rec).unwrap();
    assert_eq!((json["seeker"]["draft"]["lang"].as_str(), json["seeker"]["en"]["draft"]["lang"].as_str()), (Some("cn"), Some("en")));
    assert!(json["seeker"]["output"]["templateEn"].is_object() && json["seeker"]["en"].get("warnings").is_none());
    let b = memory_db();
    crate::picky::transfer::replace(&b, &serde_json::from_value(json.clone()).unwrap()).unwrap();
    assert_eq!(crate::picky::transfer::export(&b).unwrap(), rec);
    // a draft in the wrong language's place is refused
    let mut wrong = rec.clone();
    let s = wrong.seeker.as_mut().unwrap();
    s.en.draft = s.cn.draft.clone();
    assert!(crate::picky::transfer::replace(&memory_db(), &wrong).is_err());

    // a file from before English (format 21): its draft and records are Chinese, SEEKER.md's rules both
    let mut old = json["seeker"].clone();
    old.as_object_mut().unwrap().remove("en");
    for r in old["catalog"]["records"].as_array_mut().unwrap() {
        r.as_object_mut().unwrap().remove("lang");
    }
    let mut ws: Workspace = serde_json::from_value(old).unwrap();
    assert!(ws.catalog.records.iter().all(|r| r.lang == "cn") && ws.en.draft.is_none());
    from_before_english(&mut ws);
    let lang = |title: &str| ws.catalog.records.iter().find(|r| r.title == title).unwrap().lang.clone();
    assert_eq!((lang("定位"), lang("维护简历素材"), lang("事实与职责"), lang("柚子笔记")), ("".into(), "".into(), "cn".into(), "cn".into()));
}

#[test]
fn migration_23_keeps_the_chinese_draft_and_shares_the_seeker_rules() {
    let mut c = rusqlite::Connection::open_in_memory().unwrap();
    crate::db::configure(&c).unwrap();
    let before = &crate::picky::store::MIGRATIONS[..22];
    crate::db::migrate_as(&mut c, before, None, crate::backup::Store::Picky).unwrap();
    let draft = r#"{"name":"林小柚","header":[],"sections":[],"jd":"分析"}"#;
    c.execute("INSERT INTO seeker_draft (id, body, baseline) VALUES (1, ?1, ?1)", [draft]).unwrap();
    c.execute("INSERT INTO seeker_settings (id, folder) VALUES (1, '/x')", []).unwrap();
    let rule = |id: &str, category: &str| format!(r#"{{"id":"{id}","kind":"rule","category":"{category}","title":"{id}","text":"x","revision":1,"enabled":true}}"#);
    for (id, source, body) in [("a", Some("SEEKER.md#定位:1"), rule("a", "共用准则")), ("b", Some("SEEKER.md#维护简历素材:1"), rule("b", "素材润色")), ("c", Some("rules/cn-modules.md#事实:1"), rule("c", "素材润色")), ("d", None, rule("d", "共用准则"))] {
        c.execute("INSERT INTO seeker_records (id, source, body) VALUES (?1, ?2, ?3)", params![id, source, body]).unwrap();
    }
    crate::db::migrate_as(&mut c, crate::picky::store::MIGRATIONS, None, crate::backup::Store::Picky).unwrap();
    let w = read(&c).unwrap();
    let d = w.cn.draft.as_ref().unwrap();
    assert_eq!((d.lang.as_str(), d.jd.as_str(), w.cn.baseline.as_ref().map(|b| b.name.as_str())), ("cn", "分析", Some("林小柚")));
    assert!(w.en.draft.is_none() && w.output.template_en.is_none());
    let langs: Vec<(&str, &str)> = w.catalog.records.iter().map(|r| (r.id.as_str(), r.lang.as_str())).collect();
    assert_eq!(langs, [("a", ""), ("b", ""), ("c", "cn"), ("d", "")]);
}

#[test]
fn rater_rules_come_from_qbank_and_interview_and_stay_out_of_seeker_tasks() {
    let dir = seeker_folder("seeker-rater-rules");
    std::fs::write(dir.join("transfer/rules/qbank.md"), "# 面试题库维护规则\n\n## 适用范围\n\n只补我指定的。\n\n## 答案要求\n\n- 能口述\n").unwrap();
    std::fs::write(dir.join("transfer/rules/interview.md"), "# 准备面试与复盘\n\n给 Rater 用（文件说明，不导入）。\n\n## 准备面试\n\n先确认语言。\n\n## 面试复盘\n\n记下实际问题。\n").unwrap();
    let c = memory_db();
    let w = imported(&c, &dir);
    let rater: Vec<(&str, &str, &str)> = w.catalog.records.iter().filter(|r| crate::picky::assistants::rater::RULES.contains(&r.category.as_str())).map(|r| (r.category.as_str(), r.title.as_str(), r.lang.as_str())).collect();
    assert_eq!(rater, [("面试", "准备面试", "cn"), ("面试", "面试复盘", "cn"), ("题库维护", "适用范围", "cn"), ("题库维护", "答案要求", "cn")]);
    for task in ["素材润色", "整体审查"] {
        let ctx = model::context(&w.catalog, task, &[], "cn");
        assert!(!ctx.contains("准备面试") && !ctx.contains("答案要求"), "{ctx}");
    }
}

#[test]
fn cv_resumes_are_told_apart_by_series_and_version_and_material_is_names_only() {
    use super::files;
    assert_eq!(files::series("seek-it-keyreply-resume-v5"), ("seek-it-keyreply-resume".to_string(), Some(5)));
    assert_eq!(files::series("林小柚-Resume-2026-10-28"), ("林小柚-Resume".to_string(), None));
    assert_eq!(files::series("简历 V2"), ("简历".to_string(), Some(2)));
    assert_eq!(files::series("Resume-20261028-v3"), ("Resume".to_string(), Some(3)));
    assert_eq!(files::series("vivo"), ("vivo".to_string(), None));
    let dir = seeker_folder("seeker-files");
    let docx = word::tests::sample_docx();
    for (p, bytes) in [("cv/en/seek-it-keyreply-resume-v4.docx", docx.clone()), ("cv/en/seek-it-keyreply-resume-v5.docx", docx.clone()), ("cv/en/seek-it-iss-cv-v1.docx", docx.clone()), ("cv/cn/林小柚-简历-2026-10-20.docx", docx.clone()), ("cv/cn/模板.docx", docx.clone()), ("cv/cn/seek-it-cn-template-v1.docx", docx.clone()), ("cv/cn/seek-it-cn-template-v2.docx", docx.clone()), ("cv/en/notes.txt", vec![]), ("cv/en/~$seek-it.docx", vec![]), ("material/certificates/四级-林小柚.pdf", vec![]), ("material/证件照.jpg", vec![]), ("material/.DS_Store", vec![])] {
        let path = dir.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let export = dir.join("cv/cn/林小柚-简历-2026-10-20.docx").to_string_lossy().into_owned();
    let jobs = vec![("KeyReply · 软件开发实习".to_string(), "KeyReply".to_string()), ("NUS-ISS 学院代投 · 实习".to_string(), "NUS-ISS 学院代投".to_string()), ("腾讯 · 后台开发".to_string(), "腾讯".to_string())];
    let f = files::scan(&dir, &[("e1".to_string(), export)], &jobs);
    let got: Vec<(&str, &str, &str, Option<u32>, bool, bool, Vec<String>)> = f.cv.iter().map(|x| (x.name.as_str(), x.lang.as_str(), x.series.as_str(), x.version, x.template, x.export, x.jobs.clone())).collect();
    // 模板: only the newest version of a template series (v1 is an old one)
    assert_eq!(got, [
        ("cn/seek-it-cn-template-v2.docx", "cn", "seek-it-cn-template", Some(2), true, false, vec![]),
        ("cn/seek-it-cn-template-v1.docx", "cn", "seek-it-cn-template", Some(1), false, false, vec![]),
        ("cn/林小柚-简历-2026-10-20.docx", "cn", "林小柚-简历", None, false, true, vec![]),
        ("cn/模板.docx", "cn", "模板", None, true, false, vec![]),
        ("en/seek-it-iss-cv-v1.docx", "en", "seek-it-iss-cv", Some(1), false, false, vec!["NUS-ISS 学院代投 · 实习".to_string()]),
        ("en/seek-it-keyreply-resume-v5.docx", "en", "seek-it-keyreply-resume", Some(5), false, false, vec!["KeyReply · 软件开发实习".to_string()]),
        ("en/seek-it-keyreply-resume-v4.docx", "en", "seek-it-keyreply-resume", Some(4), false, false, vec!["KeyReply · 软件开发实习".to_string()]),
    ]);
    assert_eq!(f.material.iter().map(|m| (m.group.as_str(), m.name.as_str())).collect::<Vec<_>>(), [("certificates", "四级-林小柚.pdf"), ("", "证件照.jpg")]);
    assert!(f.transfer && f.cv[2].size > 0 && f.cv[2].modified > 0 && f.cv[2].export_id.as_deref() == Some("e1"));
    // a resume's text: only inside cv/
    let text = files::text(&dir, &dir.join("cv/en/seek-it-keyreply-resume-v5.docx")).unwrap();
    assert!(!text.is_empty());
    assert!(files::text(&dir, &dir.join("material/证件照.jpg")).unwrap_err().contains("只能读 cv/"));
    assert!(files::text(&dir, &dir.join("cv/en/notes.txt")).is_err());

    // 小拾 through 「Seeker 资料」: the list (material by name only) and one resume's text
    let c = memory_db();
    c.execute("INSERT INTO seeker_settings (id, folder) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET folder = excluded.folder", [dir.to_string_lossy()]).unwrap();
    let local = crate::picky::data::Access::default();
    let run = |args: &str| crate::picky::skills::seeker::run(&c, crate::picky::Mode::Mock, &local, args, 4000);
    let list = run(r#"{"view":"files"}"#);
    assert!(list.text.contains("cv/ 有 7 份简历，material/ 有 2 个证书原件") && list.text.contains("- cv/en/seek-it-keyreply-resume-v5.docx（英文，第 5 版") && list.text.contains("- material/certificates：四级-林小柚.pdf"), "{}", list.text);
    let one = run(r#"{"view":"cv","keyword":"keyreply-resume-v5"}"#);
    assert_eq!((one.state, one.module.as_deref()), ("ok", Some("seeker")));
    assert!(run(r#"{"view":"cv","keyword":"keyreply"}"#).text.contains("对上了 2 份"));
    assert!(run(r#"{"view":"cv","keyword":"证件照"}"#).text.contains("没有名字里有"));
    // a cloud model only when Seeker allows it
    let cloud = crate::picky::data::Access { cloud: true, ..Default::default() };
    assert_eq!(crate::picky::skills::seeker::run(&c, crate::picky::Mode::Mock, &cloud, r#"{"view":"files"}"#, 4000).state, "blocked");
}

/// Opt-in, a real model on the made-up Seeker (林小柚, Chinese and English): 选材, 审查, 润色要点, 素材润色,
/// and in English 选材, Summary, 润色. POMELO_CLOUD_KEY_FILE=… [POMELO_CLOUD_SERVICE=…]
/// [POMELO_CLOUD_MODEL=…] [POMELO_CLOUD_THINK=always] (chat.rs `cloud_target`), or POMELO_REAL_LLM=1.
/// [POMELO_SEEKER_CASE=<label start>] [POMELO_SEEKER_OUT=<report.md>]
///   cargo test --manifest-path src-tauri/Cargo.toml real_seeker -- --ignored --nocapture
#[test]
#[ignore]
fn real_seeker() {
    use crate::picky::provider::{self, Event, Msg, Options, Target};
    let target = match crate::picky::chat::tests::cloud_target() {
        Some((t, _)) => t,
        None if std::env::var("POMELO_REAL_LLM").as_deref() == Ok("1") => Target::Local { model: std::env::var("POMELO_MODEL").unwrap_or_else(|_| "qwen3.5-9b".into()) },
        None => return eprintln!("needs POMELO_REAL_LLM=1 or POMELO_CLOUD_KEY_FILE"),
    };
    let name = match &target { Target::Cloud { model, .. } => model.api_model.clone(), Target::Local { model } => model.clone(), _ => "mock".into() };
    let c = memory_db();
    let dir = english_folder("seeker-real");
    let w = imported(&c, &dir);
    let jd = "岗位：柚子银行 · 数据分析实习生。职责：用 SQL、Python 做数据清洗和报表；整理研究资料。要求：统计或计算机相关专业，熟悉 SQL，有机器学习项目经历优先。";
    let jd_en = "Data Analyst Intern at Pomelo Bank. Clean data and build reports with SQL and Python; support research. Statistics or CS background; SQL required; machine-learning projects a plus.";
    let pick = |lang: &str, cats: &[&str]| -> Vec<workflow::Selection> { w.catalog.records.iter().filter(|r| r.kind == "material" && r.lang == lang && cats.contains(&r.category.as_str())).map(|r| workflow::Selection { record_id: r.id.clone(), point_ids: r.choosable().iter().map(|p| p.id.clone()).collect() }).collect() };
    let mut cn = workflow::assemble(&w.catalog.records, None, &pick("cn", &["教育", "项目", "工作"]), true, "cn").unwrap();
    cn.jd = jd.into();
    save(&c, &cn).unwrap();
    let mut en = workflow::assemble(&w.catalog.records, None, &pick("en", &["摘要", "教育", "项目", "工作"]).into_iter().filter(|s| w.catalog.records.iter().any(|r| r.id == s.record_id && r.title != "Agent Summary")).collect::<Vec<_>>(), true, "en").unwrap();
    (en.jd, en.form) = (jd_en.into(), "resume".into());
    save(&c, &en).unwrap();
    let view = read(&c).unwrap();
    let material = view.catalog.records.iter().find(|r| r.kind == "material" && r.lang == "cn" && r.title == "柚子笔记").unwrap().id.clone();
    let think = std::env::var("POMELO_THINK").as_deref() == Ok("1");
    let only = std::env::var("POMELO_SEEKER_CASE").ok();
    let cases: Vec<(&str, &str, Option<&str>, &str)> = vec![("中文 · 选材", "select", None, "cn"), ("中文 · 整份审查", "review", None, "cn"), ("中文 · 润色要点", "polish", None, "cn"), ("中文 · 润色一条素材（柚子笔记）", "material", Some(material.as_str()), "cn"), ("英文 · 选材", "select", None, "en"), ("英文 · 按 JD 改写 Summary", "summary", None, "en"), ("英文 · 润色要点", "polish", None, "en")];
    let mut report = vec![format!("# Seeker 真测：{name}{}\n", if think { "（思考）" } else { "" })];
    for (label, kind, record, lang) in cases {
        if only.as_deref().is_some_and(|o| !label.starts_with(o)) {
            continue;
        }
        let (system, user, result) = model::prompt(&view, kind, record, "", lang, "").unwrap();
        let room = match &target {
            Target::Cloud { model, .. } if model.thinking.thinks(think) => crate::picky::context::think_room(model.context_window),
            _ if think => crate::picky::context::think_room(16384),
            _ => 0,
        };
        let start = std::time::Instant::now();
        let (mut answer, mut finish) = (String::new(), String::new());
        let call = provider::stream(target.clone(), vec![Msg::new("system", &system), Msg::new("user", &user)], Options { max_tokens: Some(4096 + room), thinking: Some(think), one_off: true, ..Default::default() }, |e| match e {
            Event::Content(t) => answer.push_str(&t),
            Event::Finished(r) => finish = r,
            _ => (),
        });
        let res = tauri::async_runtime::block_on(async { tokio::time::timeout(std::time::Duration::from_secs(600), call).await });
        report.push(format!("## {label}\n\n用时 {} 秒；结束原因 {finish}", start.elapsed().as_secs()));
        if let Err(e) = res.map_err(|_| "超时".to_string()).and_then(|r| r) {
            report.push(format!("- **失败**：{e}\n"));
            continue;
        }
        let source = format!("{system}\n{user}");
        let odd = |text: &str| -> String {
            let mut out: Vec<&str> = text.split(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '%')).map(|s| s.trim_matches('.')).filter(|s| s.chars().any(|ch| ch.is_ascii_digit()) && s.len() > 1 && !source.contains(*s)).collect();
            out.dedup();
            if out.is_empty() { String::new() } else { format!("（⚠️ 资料里没有的数字：{}）", out.join("、")) }
        };
        match model::parse(result, &answer, &view.catalog, lang) {
            Err(e) => report.push(format!("- **没通过检查**：{e}\n\n```\n{answer}\n```\n")),
            Ok(s) => {
                report.push(format!("- 说明：{}", s.advice.trim()));
                if kind == "select" {
                    for sel in &s.selections {
                        let r = view.catalog.records.iter().find(|r| r.id == sel.record_id).unwrap();
                        report.push(format!("  - 选了【{}】{}（{} 条要点）", r.category, r.title, sel.point_ids.len()));
                    }
                }
                let points = |d: &Draft| d.sections.iter().flat_map(|s| &s.entries).flat_map(|e| e.points.iter().map(|p| p.text.clone()).chain(std::iter::once(e.note.clone()).filter(|n| !n.is_empty()))).collect::<Vec<_>>();
                if let (Some(b), Some(a)) = (&s.before_draft, &s.after_draft) {
                    for (x, y) in points(b).iter().zip(points(a).iter()).filter(|(x, y)| x != y) {
                        report.push(format!("  - 原：{x}\n    新：{y}{}", odd(y)));
                    }
                }
                if let (Some(b), Some(a)) = (&s.before_record, &s.after_record) {
                    if b.text != a.text {
                        report.push(format!("  - 正文 原：{}\n    新：{}{}", b.text, a.text, odd(&a.text)));
                    }
                    for (x, y) in b.points.iter().zip(&a.points).filter(|(x, y)| x.text != y.text) {
                        report.push(format!("  - 原：{}\n    新：{}{}", x.text, y.text, odd(&y.text)));
                    }
                }
                report.push(String::new());
            }
        }
    }
    let text = report.join("\n");
    eprintln!("{text}");
    if let Ok(out) = std::env::var("POMELO_SEEKER_OUT") {
        std::fs::write(out, text).unwrap();
    }
}

#[test]
fn records_keep_the_users_order_and_change_several_at_once() {
    let c = memory_db();
    let dir = seeker_folder("seeker-order");
    let w = imported(&c, &dir);
    let projects: Vec<String> = w.catalog.records.iter().filter(|r| r.kind == "material" && r.category == "项目").map(|r| r.id.clone()).collect();
    let titles = |c: &Connection| read(c).unwrap().catalog.records.iter().filter(|r| r.kind == "material" && r.category == "项目").map(|r| r.title.clone()).collect::<Vec<_>>();
    assert_eq!(titles(&c), ["柚子笔记", "橙子识别"]);
    // a group reordered: its places swap, everything else stays; one undo step
    let before = read(&c).unwrap().catalog.records.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
    let mut h = crate::undo::History::default();
    crate::undo::tests::step(&c, &mut h, "调整顺序", |c| catalog::reorder(c, &[projects[1].clone(), projects[0].clone()]));
    assert_eq!(titles(&c), ["橙子识别", "柚子笔记"]);
    let after = read(&c).unwrap().catalog.records.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
    assert_eq!(after.iter().filter(|id| !projects.contains(id)).collect::<Vec<_>>(), before.iter().filter(|id| !projects.contains(id)).collect::<Vec<_>>());
    // a new one goes last; the order goes through the data file
    catalog::put(&c, &Record { id: "new".into(), kind: "material".into(), category: "项目".into(), title: "新项目".into(), ..Default::default() }).unwrap();
    assert_eq!(titles(&c), ["橙子识别", "柚子笔记", "新项目"]);
    let b = memory_db();
    replace(&b, &read(&c).unwrap()).unwrap();
    assert_eq!(titles(&b), ["橙子识别", "柚子笔记", "新项目"]);
    crate::undo::undo(&c, &mut h).unwrap(); // the reorder alone goes back
    assert_eq!(titles(&c), ["柚子笔记", "橙子识别", "新项目"]);
    assert!(catalog::reorder(&c, &["gone".into()]).is_err());
    // 停用 / 启用 several: each a new version, one step; unchanged ones untouched
    let ids = vec![projects[0].clone(), projects[1].clone()];
    catalog::set_enabled(&c, &ids, false).unwrap();
    let w = read(&c).unwrap();
    assert!(w.catalog.records.iter().filter(|r| ids.contains(&r.id)).all(|r| !r.enabled && r.revision == 2));
    catalog::set_enabled(&c, &ids, false).unwrap();
    assert!(read(&c).unwrap().catalog.records.iter().filter(|r| ids.contains(&r.id)).all(|r| r.revision == 2));
    assert!(catalog::set_enabled(&c, &["gone".into()], true).is_err());
}

#[test]
fn a_development_jd_chooses_the_tech_stack_by_default() {
    assert!(workflow::dev_jd("后端开发工程师，熟悉 Go") && workflow::dev_jd("Software Engineer Intern") && !workflow::dev_jd("数据分析实习生：SQL、报表") && !workflow::dev_jd(""));
    let c = memory_db();
    let dir = seeker_folder("seeker-dev-jd");
    let w = imported(&c, &dir);
    let notes = record(&w, "柚子笔记").clone();
    let chosen = |c: &Connection| read(c).unwrap().choices.iter().find(|s| s.record_id == notes.id).unwrap().point_ids.clone();
    assert!(!chosen(&c).contains(&"技术栈".to_string()));
    let mut d = workflow::initial(&w.catalog.records, "cn").unwrap();
    d.jd = "后端开发工程师".into();
    save(&c, &d).unwrap();
    assert_eq!(chosen(&c).first().map(String::as_str), Some("技术栈"));
}

/// A fuller made-up resume (林小柚, fictional) in `lang`, every material of the fake Seeker folder
/// with a few more points, as the export would take it.
fn sample_resume(c: &Connection, dir: &Path, lang: &str) -> Draft {
    let w = imported(c, dir);
    let more: &[(&str, &[&str])] = if lang == "en" {
        &[("Pomelo Notes: Reading Notes Tool", &["Built semantic search over 3,000 reading notes with a local embedding model; top-5 hit rate rose from 54% to 81% on 120 test questions", "Merged scattered notes into per-book volumes, cutting duplicates by 30%", "Kept every byte offline in one SQLite file with one-click backup"]), ("Orange Classifier: Fruit CNN", &["Led a 3-person team through two tuning rounds; test accuracy reached 0.91 on a 60-image set", "Rebalanced a 20:75 class split with weighted sampling; validation Macro-F1 rose from 0.88 to 0.92"])]
    } else {
        &[("柚子笔记", &["**检索改进**：用本地向量模型做按意思搜索，在 120 条测试问题上前 5 条命中率从 54% 提到 81%", "**数据整理**：把零散笔记按书合并成卷，去重后条数减少 30%", "**离线优先**：全部数据存在本机 SQLite，单文件备份"]), ("橙子识别", &["**模型迭代**：带 3 人小组两轮调参，测试集准确率 0.91", "**类别均衡**：样本 20:75 失衡，加权采样后验证集 Macro-F1 从 0.88 到 0.92"])]
    };
    for (title, points) in more {
        let mut r = record(&w, title).clone();
        r.points = points.iter().enumerate().map(|(i, t)| Point { id: format!("p{i}"), text: t.to_string() }).collect();
        catalog::put(c, &r).unwrap();
    }
    let w = read(c).unwrap();
    let picks: Vec<workflow::Selection> = w.catalog.records.iter().filter(|r| r.kind == "material" && r.lang == lang && r.title != "Agent Summary" && r.title != "Third Project").map(|r| workflow::Selection { record_id: r.id.clone(), point_ids: r.choosable().iter().filter(|p| !workflow::is_stack(p)).map(|p| p.id.clone()).collect() }).collect();
    let mut d = workflow::assemble(&w.catalog.records, Some(workflow::initial(&w.catalog.records, lang).unwrap()), &picks, true, lang).unwrap();
    if lang == "en" {
        d.form = "resume".into();
    }
    for h in d.header.iter_mut() {
        h.visible = h.visible || ["GitHub", "LinkedIn"].contains(&h.label.as_str());
    }
    d
}

#[test]
fn the_built_in_templates_take_every_part_of_a_resume() {
    for lang in LANGS {
        let (t, bytes) = output::builtin(lang).unwrap();
        output::check_slots(&t.slots, &t.paragraphs).unwrap();
        let dir = english_folder(&format!("seeker-builtin-{lang}"));
        let c = memory_db();
        let d = sample_resume(&c, &dir, lang);
        let docx = word::resume_from_template(bytes, &output::lines(&d, &t)).unwrap();
        let text = word::tests::textutil_text(&dir, &docx);
        let expect: &[&str] = if lang == "en" { &["LIN XIAOYOU", "top-5 hit rate rose", "Example University", "IELTS Academic"] } else { &["林小柚", "检索改进", "示例大学", "项目经历"] };
        for part in expect {
            assert!(text.contains(part), "{lang}: {part}\n{text}");
        }
        // no placeholder left behind
        assert!(!text.contains("YOUR NAME") && !text.contains("[姓名]"), "{text}");
        // 中文经历抬头: 名称 ⇥ 角色 ⇥ 时间, no 地点
        if lang == "cn" {
            assert!(text.lines().any(|l| l == "橙子识别\t课程项目\t2025.10 - 2025.12"), "{text}");
        }
        // opt-in: write the sample for a look in Word (POMELO_RESUME_PREVIEW=<folder>)
        if let Ok(out) = std::env::var("POMELO_RESUME_PREVIEW") {
            std::fs::write(Path::new(&out).join(format!("样例-{}.docx", if lang == "en" { "英文" } else { "中文" })), &docx).unwrap();
        }
    }
}

#[test]
fn migration_28_keeps_the_job_links_by_their_exports_path() {
    let mut c = rusqlite::Connection::open_in_memory().unwrap();
    crate::db::configure(&c).unwrap();
    crate::db::migrate_as(&mut c, &crate::picky::store::MIGRATIONS[..27], None, crate::backup::Store::Picky).unwrap();
    let export = |id: &str, path: &str| format!(r#"{{"id":"{id}","label":"{id}.docx","draft":{{"name":"林小柚","header":[],"sections":[],"jd":"分析"}},"path":"{path}","template":{{"path":"","slots":{{"name":0,"contact":1,"section":2,"entry":3,"point":4}},"paragraphs":[]}},"createdAt":"2026-10-01T00:00:00.000Z"}}"#);
    let link = |id: &str, export: &str| format!(r#"{{"id":"{id}","exportId":"{export}","job":{{"id":3,"company":"示例","position":null,"location":null,"link":null,"note":null,"createdAt":"2026-09-01T00:00:00.000Z"}},"jd":"分析","createdAt":"2026-10-01T00:00:00.000Z","current":false}}"#);
    for (id, path) in [("e1", "/cv/cn/a.docx"), ("e2", "/cv/cn/b.docx")] {
        c.execute("INSERT INTO seeker_exports (id, body) VALUES (?1, ?2)", params![id, export(id, path)]).unwrap();
    }
    for (id, e, current) in [("l1", "e1", 0), ("l2", "e2", 1)] {
        c.execute("INSERT INTO seeker_job_exports (id, export_id, job_id, job_created_at, body, current) VALUES (?1, ?2, 3, '2026-09-01T00:00:00.000Z', ?3, ?4)", params![id, e, link(id, e), current]).unwrap();
    }
    crate::db::migrate_as(&mut c, crate::picky::store::MIGRATIONS, None, crate::backup::Store::Picky).unwrap();
    let links = read(&c).unwrap().output.links;
    assert_eq!(links.iter().map(|l| (l.id.as_str(), l.path.as_str(), l.current, l.jd.as_str())).collect::<Vec<_>>(), [("l2", "/cv/cn/b.docx", true, "分析"), ("l1", "/cv/cn/a.docx", false, "分析")]);
    let body: String = c.query_row("SELECT body FROM seeker_job_resumes WHERE id = 'l1'", [], |r| r.get(0)).unwrap();
    assert!(!body.contains("exportId") && body.contains("\"path\":\"/cv/cn/a.docx\""), "{body}");
}

#[test]
fn a_job_uses_any_resume_under_cv_and_the_old_field_moves_over_once() {
    let dir = seeker_folder("seeker-job-resumes");
    let docx = word::tests::sample_docx();
    for p in ["cv/en/seek-it-keyreply-resume-v4.docx", "cv/en/seek-it-keyreply-resume-v5.docx", "cv/cn/林小柚-简历.docx", "material/a.pdf"] {
        std::fs::create_dir_all(dir.join(p).parent().unwrap()).unwrap();
        std::fs::write(dir.join(p), &docx).unwrap();
    }
    let real = |p: &str| std::fs::canonicalize(dir.join(p)).unwrap().to_string_lossy().into_owned();
    // only a résumé file under cv/
    assert_eq!(files::in_cv(&dir, &dir.join("cv/en/seek-it-keyreply-resume-v5.docx")).unwrap(), real("cv/en/seek-it-keyreply-resume-v5.docx"));
    assert!(files::in_cv(&dir, &dir.join("material/a.pdf")).is_err() && files::in_cv(&dir, &dir.join("cv/en/none.docx")).is_err());

    let home = crate::db::tests::temp_dir("seeker-job-resumes-app");
    let st = crate::AppState::init(crate::paths::DataPaths::new(home.to_path_buf()), &home, crate::picky::Mode::Mock);
    // no Seeker folder yet: nothing moves, tried again later
    let job = |company: &str, resume: Option<String>| st.with_db(|c| { c.execute("INSERT INTO jobs (company, resume) VALUES (?1, ?2)", params![company, resume]).map_err(err)?; Ok(c.last_insert_rowid()) }).unwrap();
    let keyreply = job("KeyReply", Some(dir.join("cv/en/seek-it-keyreply-resume-v4.docx").to_string_lossy().into_owned()));
    let iss = job("NUS-ISS", Some("~/Desktop/别处的简历.docx".into()));
    let tencent = job("腾讯", Some(dir.join("cv/cn/林小柚-简历.docx").to_string_lossy().into_owned()));
    assert_eq!(output::adopt_job_resumes(&st).unwrap(), 0);
    st.picky.with_db(|c| c.execute("INSERT INTO seeker_settings (id, folder) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET folder = excluded.folder", [dir.to_string_lossy()]).map_err(err)).unwrap();
    // 腾讯 already has a résumé in use: the old one goes beside it
    let snap = |id: i64| output::job(&st, id).unwrap();
    st.picky.with_db(|c| output::associate(c, &real("cv/en/seek-it-keyreply-resume-v5.docx"), "", snap(tencent), true)).unwrap();
    assert_eq!(output::adopt_job_resumes(&st).unwrap(), 2);
    assert_eq!(output::adopt_job_resumes(&st).unwrap(), 0);
    let links = st.picky.with_db(|c| Ok(read(c)?.output.links)).unwrap();
    let of = |id: i64| links.iter().filter(|l| l.job.id == id).map(|l| (l.path.clone(), l.current)).collect::<Vec<_>>();
    assert_eq!(of(keyreply), [(real("cv/en/seek-it-keyreply-resume-v4.docx"), true)]);
    assert!(of(iss).is_empty());
    assert_eq!(of(tencent), [(real("cv/cn/林小柚-简历.docx"), false), (real("cv/en/seek-it-keyreply-resume-v5.docx"), true)]);

    // the panel: linked files still under cv/ (newest first), and every résumé to pick from
    let cv = files::scan(&dir, &[], &[]).cv;
    let shown = output::job_resumes(&links, &snap(tencent), cv.clone());
    assert_eq!(shown.iter().map(|l| (l.name.as_str(), l.current)).collect::<Vec<_>>(), [("cn/林小柚-简历.docx", false), ("en/seek-it-keyreply-resume-v5.docx", true)]);
    std::fs::remove_file(dir.join("cv/cn/林小柚-简历.docx")).unwrap();
    let cv = files::scan(&dir, &[], &[]).cv;
    assert_eq!(output::job_resumes(&links, &snap(tencent), cv).len(), 1);
    assert_eq!(st.picky.with_db(|c| Ok(read(c)?.output.links.len())).unwrap(), 3); // the record stays
}

#[test]
fn an_old_data_file_links_jobs_by_export() {
    let c = memory_db();
    // an export of format 26 carried its draft: its language and materials are kept, the draft isn't
    let export: output::Export = serde_json::from_str(r#"{"id":"e1","label":"a.docx","path":"/cv/cn/a.docx","createdAt":"2026-10-01T00:00:00.000Z","template":{"path":""},"draft":{"lang":"en","name":"林小柚","header":[],"jd":"分析","sections":[{"id":"s","title":"Projects","category":"项目","entries":[{"id":"e","time":"","name":"Pomelo","role":"","location":"","note":"","points":[],"source":{"recordId":"m1","revision":1,"pointIds":[]}}]}]}}"#).unwrap();
    let link: output::JobResume = serde_json::from_str(r#"{"id":"l1","exportId":"e1","job":{"id":3,"company":"示例","position":null,"location":null,"link":null,"note":null,"createdAt":"2026-09-01T00:00:00.000Z"},"jd":"分析","createdAt":"2026-10-01T00:00:00.000Z","current":true}"#).unwrap();
    output::replace(&c, &output::Output { exports: vec![export], links: vec![link], ..Default::default() }).unwrap();
    let w = read(&c).unwrap();
    assert_eq!((w.output.exports[0].lang.as_str(), w.output.exports[0].materials.clone(), w.output.exports[0].draft.is_none()), ("en", vec!["m1".to_string()], true));
    let links = w.output.links;
    assert_eq!((links[0].path.as_str(), links[0].current), ("/cv/cn/a.docx", true));
    assert!(!serde_json::to_string(&links[0]).unwrap().contains("exportId"));
}

fn rename_group_for_test(c: &Connection) {
    catalog::rename_group(c, "material", "cn", "教育", "教育背景").unwrap();
}

#[test]
fn groups_rename_by_their_shown_name_or_by_category() {
    let dir = seeker_folder("seeker-groups");
    let c = memory_db();
    let w = imported(&c, &dir);
    let before: Vec<(String, String)> = w.catalog.records.iter().map(|r| (r.id.clone(), r.category.clone())).collect();
    // a built-in group only shows another name; the program still knows it as 教育 / 素材润色
    catalog::rename_group(&c, "material", "cn", "教育", "教育背景").unwrap();
    catalog::rename_group(&c, "rule", "en", "素材润色", "润色要点和素材").unwrap();
    let w = read(&c).unwrap();
    assert_eq!(w.catalog.records.iter().map(|r| (r.id.clone(), r.category.clone())).collect::<Vec<_>>(), before);
    assert_eq!(w.catalog.groups.iter().map(|g| (g.kind.as_str(), g.lang.as_str(), g.category.as_str(), g.label.as_str())).collect::<Vec<_>>(), [("material", "cn", "教育", "教育背景"), ("rule", "", "素材润色", "润色要点和素材")]);
    // its own name again: back to plain
    catalog::rename_group(&c, "material", "cn", "教育", "教育").unwrap();
    assert_eq!(read(&c).unwrap().catalog.groups.len(), 1);
    // a group of the user's own: its records move (a new version each; this language only)
    let mut prize = catalog::Record { id: "m-prize".into(), category: "竞赛".into(), title: "数学建模".into(), ..Default::default() };
    catalog::put(&c, &prize).unwrap();
    prize = catalog::Record { id: "m-prize-en".into(), lang: "en".into(), ..prize };
    catalog::put(&c, &prize).unwrap();
    catalog::rename_group(&c, "material", "cn", "竞赛", "奖项").unwrap();
    let w = read(&c).unwrap();
    let of = |id: &str| w.catalog.records.iter().find(|r| r.id == id).map(|r| (r.category.clone(), r.revision)).unwrap();
    assert_eq!((of("m-prize"), of("m-prize-en")), (("奖项".to_string(), 2), ("竞赛".to_string(), 1)));
    assert!(catalog::rename_group(&c, "material", "cn", "奖项", " ").is_err());
    assert!(catalog::rename_group(&c, "fact", "cn", "个人事实", "x").is_err());
}

#[test]
fn migration_29_drops_versions_and_the_copies_in_exports() {
    let mut c = rusqlite::Connection::open_in_memory().unwrap();
    crate::db::configure(&c).unwrap();
    crate::db::migrate_as(&mut c, &crate::picky::store::MIGRATIONS[..28], None, crate::backup::Store::Picky).unwrap();
    c.execute("INSERT INTO seeker_versions (label, body) VALUES ('第一版', '{}')", []).unwrap();
    let body = r#"{"id":"e1","label":"a.docx","path":"/cv/en/a.docx","createdAt":"2026-10-01T00:00:00.000Z","template":{"path":"t"},"draft":{"lang":"en","sections":[{"entries":[{"source":{"recordId":"m1"}},{"source":null},{"source":{"recordId":"m2"}}]},{"entries":[{"source":{"recordId":"m1"}}]}]}}"#;
    c.execute("INSERT INTO seeker_exports (id, body) VALUES ('e1', ?1)", [body]).unwrap();
    crate::db::migrate_as(&mut c, crate::picky::store::MIGRATIONS, None, crate::backup::Store::Picky).unwrap();
    let gone: i64 = c.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'seeker_versions'", [], |r| r.get(0)).unwrap();
    assert_eq!(gone, 0);
    let e = &read(&c).unwrap().output.exports[0];
    assert_eq!((e.lang.as_str(), e.materials.clone(), e.label.as_str()), ("en", vec!["m1".to_string(), "m2".to_string()], "a.docx"));
    let body: String = c.query_row("SELECT body FROM seeker_exports", [], |r| r.get(0)).unwrap();
    assert!(!body.contains("draft") && !body.contains("template"), "{body}");
}

#[test]
fn each_function_shows_the_groups_it_reads_and_its_system_message() {
    let dir = seeker_folder("seeker-prompts");
    let c = memory_db();
    imported(&c, &dir);
    for (category, title, text) in [("素材润色", "润色口径", "只润色不加事实 POLISH-ONLY"), ("整体审查", "审查口径", "先看匹配 REVIEW-ONLY")] {
        catalog::put(&c, &catalog::Record { id: title.into(), kind: "rule".into(), category: category.into(), title: title.into(), text: text.into(), lang: "".into(), ..Default::default() }).unwrap();
    }
    let w = read(&c).unwrap();
    let cn = model::previews(&w, "cn");
    assert_eq!(cn.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), ["select", "review", "polish", "material"]);
    let of = |k: &str| cn.iter().find(|p| p.key == k).unwrap();
    assert!(of("select").system.contains("REVIEW-ONLY") && !of("select").system.contains("POLISH-ONLY"), "{}", of("select").system);
    assert!(of("polish").system.contains("POLISH-ONLY") && !of("polish").system.contains("REVIEW-ONLY"));
    assert_eq!(of("material").groups, ["共用准则", "写作规则", "素材润色", "个人范例"]);
    // no draft yet: a fresh one with a placeholder JD; English has the Summary too
    assert!(cn.iter().all(|p| !p.system.starts_with("（现在看不了")), "{cn:?}");
    assert_eq!(model::previews(&w, "en").len(), 5);
    // Rater: every function, from the same builders as the calls
    let bank = crate::picky::assistants::rater::read(&c).unwrap();
    let rater = crate::picky::assistants::rater::previews(&w, &bank);
    assert_eq!(rater.len(), 9);
    // 润色答案 and 挑卡 need a question in the bank: they say so
    assert!(rater.iter().all(|p| ["polish", "pick"].contains(&p.key.as_str()) == p.system.starts_with("（现在看不了")), "{rater:?}");
    assert_eq!(rater.iter().find(|p| p.key == "突击题").unwrap().groups, ["共用准则", "题库维护", "突击题"]);
}

/// Refine R5: a rule is a title and points; older text (lists, tables, paragraphs) and 个人约定 are read
/// as points, and a rule without one isn't saved.
#[test]
fn rules_are_points_and_older_text_is_split() {
    let text = "以下为依据。素材润色仅可参考。\n\n- 第一条\n2. 第二条\n\n| 维度 | 检查重点 |\n|---|---|\n| 内容 | 是否对应 |\n| 形式 | 是否重复 |\n\n### 用词\n避免空话！";
    let points: Vec<String> = catalog::split_points(text).into_iter().map(|p| p.text).collect();
    assert_eq!(points, ["以下为依据。", "素材润色仅可参考。", "第一条", "第二条", "内容：是否对应", "形式：是否重复", "用词", "避免空话！"]);
    assert_eq!(catalog::split_points(text).iter().map(|p| p.id.as_str()).take(3).collect::<Vec<_>>(), ["p1", "p2", "p3"]);
    let mut old = Record { id: "d".into(), kind: "decision".into(), category: "素材润色".into(), title: "按步骤".into(), text: "按用户指定步骤处理".into(), lang: "cn".into(), ..Default::default() };
    catalog::normalize(&mut old);
    assert_eq!((old.kind.as_str(), old.text.as_str(), old.points.iter().map(|p| p.text.as_str()).collect::<Vec<_>>()), ("rule", "", vec!["按用户指定步骤处理"]));
    assert_eq!(catalog::rule_markdown(&old), "## 按步骤\n1. 按用户指定步骤处理");
    // saving: empty points dropped; none left → refused; an old-shaped rule is taken as points
    let c = memory_db();
    let empty = Record { id: "r".into(), kind: "rule".into(), category: "共用准则".into(), title: "空的".into(), points: vec![Point { id: "p1".into(), text: "  ".into() }], lang: String::new(), ..Default::default() };
    assert!(catalog::put(&c, &empty).unwrap_err().contains("至少一个要点"));
    let kept = Record { points: vec![Point { id: "p1".into(), text: " 一句 ".into() }, Point { id: "p2".into(), text: "".into() }], ..empty.clone() };
    catalog::put(&c, &kept).unwrap();
    assert_eq!(read(&c).unwrap().catalog.records[0].points, [Point { id: "p1".into(), text: "一句".into() }]);
    catalog::put(&c, &Record { id: "t".into(), text: "甲。乙。".into(), points: vec![], ..empty }).unwrap();
    let t = read(&c).unwrap().catalog.records.into_iter().find(|r| r.id == "t").unwrap();
    assert_eq!((t.text.as_str(), t.points.len()), ("", 2));
}

/// Opt-in: every Seeker and Rater function's 发给模型的说明 from a copy of a picky.db (never the real one:
/// copy it first). POMELO_PROMPTS_DB=<copy> [POMELO_PROMPTS_OUT=<file.md>]
#[test]
#[ignore]
fn prompts_of_a_copy() {
    let path = std::env::var("POMELO_PROMPTS_DB").expect("POMELO_PROMPTS_DB");
    assert!(!path.contains("Application Support/Pomelo OS/"), "copy the database first");
    let c = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let w = read(&c).unwrap();
    let bank = crate::picky::assistants::rater::read(&c).unwrap();
    let mut out = vec![];
    for (who, list) in [("Seeker · 中文", model::previews(&w, "cn")), ("Seeker · English", model::previews(&w, "en")), ("Rater", crate::picky::assistants::rater::previews(&w, &bank))] {
        for p in list {
            out.push(format!("## {who} · {}\n\n读的规则：{}\n\n```\n{}\n```\n", p.label, p.groups.join("、"), p.system));
        }
    }
    let text = out.join("\n");
    match std::env::var("POMELO_PROMPTS_OUT") {
        Ok(f) => std::fs::write(f, text).unwrap(),
        Err(_) => println!("{text}"),
    }
}
