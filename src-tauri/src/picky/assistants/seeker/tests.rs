//! Seeker with a made-up Seeker folder shaped like the real transfer/ (all content fictional).
use super::catalog::{self, Record};
use super::{import, output, workflow, *};
use crate::db::tests::{temp_dir, TempDir};
use crate::picky::store::tests::memory_db;
use crate::picky::word;
use std::path::Path;

const FILES: &[(&str, &str)] = &[
    ("cn-modules/0 - 归档.md", "# 0 - 归档\n#职业/中文简历\n"),
    ("cn-modules/1 - 基本信息.md", "# 1 - 基本信息\n#职业/中文简历\n\n- 姓名：林小柚\n- 工作地点：上海\n- 工作邮箱：work@example.com\n- 个人邮箱：me@example.com\n- 联系电话：+86 100 0000 0000\n- GitHub：https://github.com/example\n- 个人网站：https://example.com\n"),
    ("cn-modules/2 - 教育背景.md", "# 2 - 教育背景\n#职业/中文简历\n\n**示例大学 · 信息学院**\n数据科学 · 硕士\n2025.07 - 至今\n- **主修课程**：机器学习、数据库\n\n---\n**示例学院 · 经济系**\n金融学 · 本科\n2021.09 - 2025.06\n- **主修课程**：计量经济学\n- **在校荣誉**：奖学金\n  \n"),
    ("cn-modules/3 - 技能证书.md", "# 3 - 技能证书\n#职业/中文简历\n\n- **编程语言**：Python、SQL\n- **语言能力**：英语六级\n"),
    ("cn-modules/4 - 项目经历.md", "# 4 - 项目经历\n#职业/中文简历\n\n**独立开发**\n柚子笔记：读书笔记整理工具\n2026.03 - 至今\n技术栈：Rust、SQLite\n- **数据整理**：把零散笔记合成卷\n- **检索改进**：按意思搜索\n\n---\n**课程项目**\n橙子识别：水果分类模型\n2025.10 - 2025.12　上海\n* **模型迭代**：两轮调参\n"),
    ("cn-modules/5 - 工作经历.md", "# 5 - 工作经历\n#职业/中文简历\n\n**示例证券 · 研究所**\n研究部实习生\n2024.01 - 2024.02 　杭州\n* **行业研究**：整理研报\n- **数据分析**：写脚本\n"),
    ("cn-modules/6 - 基本信息 - 网申.md", "# 6 - 基本信息 - 网申\n#职业/中文简历\n"),
    ("en-modules/1-SUMMARY.md", "# Summary\n\nData person.\n"),
    ("rules/cn-modules.md", "# 中文素材润色规则\n\n- 不编造事实\n"),
    ("rules/cn-resume.md", "# 中文简历整体审查规则\n\n- 只给建议\n"),
    ("rules/en-resume.md", "# English resume rules\n"),
    ("rules/qbank.md", "# 题库维护\n"),
    ("projects/project-orange.md", "# 橙子识别 经历档案\n\n本人负责调参，团队 3 人。\n"),
    ("FACTS.md", "# 已确认事实\n\n- 2027 年后到岗\n"),
    ("SEEKER.md", "# Seeker 工作准则\n\n只做指定的事。\n"),
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
    assert_eq!(skipped, ["TRANSFER.md：不是要导入的资料", "cn-modules/0 - 归档.md：归档的不导入", "en-modules/1-SUMMARY.md：英文资料以后再导入", "rules/en-resume.md：英文资料以后再导入", "rules/qbank.md：面试题库以后再导入", "问卷.docx：只导入 Markdown 和文本"]);
    let got: Vec<(&str, &str, &str)> = preview.items.iter().map(|i| (i.incoming.kind.as_str(), i.incoming.category.as_str(), i.incoming.title.as_str())).collect();
    assert_eq!(
        got,
        [
            ("fact", "已确认事实", "已确认事实"),
            ("rule", "共用准则", "Seeker 工作准则"),
            ("profile", "基本信息", "基本信息"),
            ("material", "教育", "示例大学 · 信息学院"),
            ("material", "教育", "示例学院 · 经济系"),
            ("material", "技能", "技能证书"),
            ("material", "项目", "柚子笔记"),
            ("material", "项目", "橙子识别"),
            ("material", "工作", "示例证券 · 研究所"),
            ("fact", "经历档案", "橙子识别 经历档案"),
            ("rule", "素材润色", "中文素材润色规则"),
            ("rule", "整体审查", "中文简历整体审查规则"),
        ]
    );
    let at = |title: &str| &preview.items.iter().find(|i| i.incoming.title == title).unwrap().incoming;
    let notes = at("柚子笔记");
    assert_eq!((notes.role.as_str(), notes.text.as_str(), notes.time.as_str(), notes.location.as_str()), ("独立开发", "读书笔记整理工具", "2026.03 - 至今", ""));
    assert_eq!(notes.points.iter().map(|p| (p.id.as_str(), p.text.as_str())).collect::<Vec<_>>(), [("技术栈", "技术栈：Rust、SQLite"), ("数据整理", "**数据整理**：把零散笔记合成卷"), ("检索改进", "**检索改进**：按意思搜索")]);
    assert_eq!(notes.source.as_deref(), Some("cn-modules/4 - 项目经历.md#柚子笔记:1"));
    assert_eq!((at("橙子识别").location.as_str(), at("橙子识别").role.as_str()), ("上海", "课程项目"));
    let work = at("示例证券 · 研究所");
    assert_eq!((work.role.as_str(), work.time.as_str(), work.location.as_str(), work.points.len()), ("研究部实习生", "2024.01 - 2024.02", "杭州", 2));
    let school = at("示例学院 · 经济系");
    assert_eq!((school.role.as_str(), school.points.len(), school.text.as_str()), ("金融学 · 本科", 2, ""));
    assert_eq!((at("技能证书").points.len(), at("技能证书").time.as_str()), (2, ""));
    assert_eq!(at("基本信息").fields.iter().map(|f| f.label.as_str()).collect::<Vec<_>>(), ["姓名", "工作地点", "工作邮箱", "个人邮箱", "联系电话", "GitHub", "个人网站"]);
    assert_eq!(at("中文素材润色规则").text, "- 不编造事实");

    let c = memory_db();
    let w = imported(&c, &dir);
    assert_eq!(w.catalog.records.len(), 12);
    // the same files again: nothing to import; a changed one: that item, against the current one
    assert!(import::preview(&dir.join("transfer"), &w.catalog.records).unwrap().items.is_empty());
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
    assert!(catalog::put(&c, &Record { revision: 1, text: "x".into(), ..orange }).unwrap_err().contains("改过"));
}

#[test]
fn a_draft_starts_from_the_profile_and_education_and_grows_by_adding() {
    let dir = seeker_folder("seeker-draft");
    let c = memory_db();
    let w = imported(&c, &dir);
    let d = workflow::initial(&w.catalog.records).unwrap();
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
    let d = workflow::assemble(&w.catalog.records, None, &picks, false).unwrap();
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
    let added = workflow::assemble(&w.catalog.records, Some(edited.clone()), &more, false).unwrap();
    assert_eq!(added.sections.len(), 4);
    assert_eq!(added.sections[1].entries.len(), 1);
    assert_eq!(added.sections[1].entries[0].points[0].text, "改过的要点");
    assert_eq!(added.sections[3].title, "专业技能");
    let again = workflow::assemble(&w.catalog.records, Some(edited), &[chosen.clone()], true).unwrap();
    assert_eq!((again.jd.as_str(), again.sections.len()), ("数据分析师", 2));
    assert_eq!(again.sections[1].entries[0].points[0].text, "**数据整理**：把零散笔记合成卷");
    // a choice made on points that are gone
    let stale = workflow::Selection { record_id: notes.id.clone(), point_ids: vec!["没有".into()] };
    assert!(workflow::assemble(&w.catalog.records, None, &[stale], false).is_err());

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
    save(&c, &workflow::assemble(&w.catalog.records, None, &[pick], false).unwrap()).unwrap();

    // the material gets a new version: the entry would take its new text, still one point
    let mut newer = notes.clone();
    newer.points[1].text = "**数据整理**：合成卷并去重".into();
    newer.text = "读书笔记整理与检索工具".into();
    catalog::put(&c, &newer).unwrap();
    let changes = workflow::source_changes(&read(&c).unwrap());
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].after.points.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["**数据整理**：合成卷并去重"]);
    assert_eq!(changes[0].after.note, "读书笔记整理与检索工具");
    assert_eq!(changes[0].after.source.as_ref().unwrap().revision, 2);

    // a batch delete is one undo step; undo brings the draft back as it was
    let before = read(&c).unwrap().draft.unwrap();
    let mut h = crate::undo::History::default();
    let ids: Vec<String> = before.sections.iter().map(|s| s.id.clone()).collect();
    crate::undo::tests::step(&c, &mut h, "删除简历内容", |c| {
        let mut d = read(c)?.draft.unwrap();
        delete_blocks(&mut d, &ids);
        save(c, &d)
    });
    assert!(read(&c).unwrap().draft.unwrap().sections.is_empty());
    assert_eq!(crate::undo::undo(&c, &mut h).unwrap().as_deref(), Some("删除简历内容"));
    assert_eq!(read(&c).unwrap().draft.unwrap(), before);
    assert_eq!(crate::undo::undo(&c, &mut h).unwrap(), None);
}

/// The draft as the tests' template slots expect it, with a confirmed version, an export and links.
fn full_workspace(c: &Connection, dir: &Path) -> Workspace {
    let w = imported(c, dir);
    let picks: Vec<workflow::Selection> = w.choices.clone();
    let mut d = workflow::assemble(&w.catalog.records, None, &picks, false).unwrap();
    d.jd = "数据分析".into();
    save(c, &d).unwrap();
    workflow::baseline(c, &d).unwrap();
    c.execute("INSERT INTO seeker_versions (label, body) VALUES ('第一版', ?1)", [serde_json::to_string(&d).unwrap()]).unwrap();
    super::set_settings(c, "folder", &dir.to_string_lossy()).unwrap();
    super::set_settings(c, "options", r#"{"selectionModel":"local:qwen","thinking":true}"#).unwrap();
    read(c).unwrap()
}

fn slots() -> output::Slots {
    output::Slots { name: 0, contact: 1, section: 2, education: Some(3), note: Some(4), point: 5, entry: 6 }
}

#[test]
fn the_resume_fills_the_template_and_export_never_overwrites() {
    let dir = seeker_folder("seeker-export");
    let c = memory_db();
    let w = full_workspace(&c, &dir);
    let draft = w.draft.clone().unwrap();
    let lines = output::lines(&draft, &slots());
    let text: Vec<String> = lines.iter().map(|(i, t)| format!("{i} {t}")).collect();
    assert_eq!(
        text[..8],
        ["0 林小柚", "1 work@example.com · +86 100 0000 0000 · https://example.com · 上海", "2 教育背景", "3 示例大学 · 信息学院\t2025.07 - 至今", "4 数据科学 · 硕士", "5 **主修课程**：机器学习、数据库", "3 示例学院 · 经济系\t2021.09 - 2025.06", "4 金融学 · 本科"]
    );
    // 技能: points only; 项目: heading in four columns, the note, the points
    let skills = text.iter().position(|l| l == "2 专业技能").unwrap();
    assert_eq!(text[skills + 1], "5 **编程语言**：Python、SQL");
    let project = text.iter().position(|l| l.starts_with("6 2026.03")).unwrap();
    assert_eq!(text[project..project + 3], ["6 2026.03 - 至今\t柚子笔记\t独立开发\t", "4 读书笔记整理工具", "5 **数据整理**：把零散笔记合成卷"]);
    let docx = word::resume_from_template(&word::tests::resume_template_docx(), &lines).unwrap();
    let items = word::read_docx(&docx).unwrap().items;
    assert_eq!(items[0].text, "**林小柚**");
    assert!(items.iter().any(|i| i.text == "**2026.03 - 至今\t柚子笔记\t**独立开发\t"), "{:?}", items.iter().map(|i| &i.text).collect::<Vec<_>>());
    assert!(items.iter().any(|i| i.text == "- **数据整理**：把零散笔记合成卷"));

    // 另存: the previewed file, once; same name again is refused and leaves no record
    let template = output::Template { path: "t.docx".into(), slots: slots(), paragraphs: vec![] };
    let stage = |ticket: &str, pages: usize, d: &Draft| {
        let s = dir.join(ticket);
        std::fs::create_dir_all(&s).unwrap();
        std::fs::write(s.join("resume.docx"), &docx).unwrap();
        std::fs::write(s.join("prepared.json"), serde_json::to_vec(&serde_json::json!({"draft": d, "template": template, "folder": "", "pages": pages})).unwrap()).unwrap();
        s
    };
    let out = dir.join("cv");
    std::fs::create_dir_all(&out).unwrap();
    let job = |id: i64| output::JobSnapshot { id, company: "示例公司".into(), position: Some("分析师".into()), location: None, link: None, note: None, created_at: "2026-10-01T00:00:00.000Z".into() };
    let first = output::save_export(&c, "a1", &stage("a1", 1, &draft), &out, "简历.docx", Some(job(7))).unwrap();
    assert_eq!(std::fs::read(out.join("简历.docx")).unwrap(), docx);
    assert_eq!(first.path, out.join("简历.docx").to_string_lossy());
    assert!(output::save_export(&c, "a2", &stage("a2", 1, &draft), &out, "简历.docx", None).unwrap_err().contains("同名"));
    assert!(output::save_export(&c, "a3", &stage("a3", 2, &draft), &out, "简历2.docx", None).unwrap_err().contains("2 页"));
    assert!(output::save_export(&c, "a4", &stage("a4", 1, &draft), &out, "../x.docx", None).is_err());
    let mut other = draft.clone();
    other.name = "别人".into();
    assert!(output::save_export(&c, "a5", &stage("a5", 1, &other), &out, "简历3.docx", None).unwrap_err().contains("改过"));
    assert_eq!(read(&c).unwrap().output.exports.len(), 1);
    assert!(!out.join("简历2.docx").exists() && !out.join("简历3.docx").exists());

    // a second export linked to the same job becomes the one used; the first link stays
    let second = output::save_export(&c, "b1", &stage("b1", 1, &draft), &out, "简历-2.docx", Some(job(7))).unwrap();
    let links = read(&c).unwrap().output.links;
    assert_eq!(links.iter().map(|l| (l.export_id.as_str(), l.current)).collect::<Vec<_>>(), [("b1", true), ("a1", false)]);
    output::associate(&c, &first, job(7)).unwrap();
    let links = read(&c).unwrap().output.links;
    assert_eq!(links.iter().map(|l| (l.export_id.as_str(), l.current)).collect::<Vec<_>>(), [("b1", false), ("a1", true)]);
    assert_eq!(links[1].jd, "数据分析");
    let _ = second;
}

#[test]
fn the_workspace_goes_through_the_data_file_whole() {
    let dir = seeker_folder("seeker-transfer");
    let a = memory_db();
    let w = full_workspace(&a, &dir);
    let draft = w.draft.clone().unwrap();
    let export = output::Export { id: "e1".into(), label: "简历.docx".into(), draft: draft.clone(), path: "/x/简历.docx".into(), pages: 1, renderer: "LibreOffice".into(), template: output::Template { path: "/x/t.docx".into(), slots: slots(), paragraphs: vec![Some("[姓名]".into()), None] }, created_at: "2026-10-02T00:00:00.000Z".into() };
    a.execute("INSERT INTO seeker_exports (id, body) VALUES ('e1', ?1)", [serde_json::to_string(&export).unwrap()]).unwrap();
    output::associate(&a, &export, output::JobSnapshot { id: 3, company: "示例".into(), position: None, location: None, link: None, note: None, created_at: "2026-09-01T00:00:00.000Z".into() }).unwrap();
    a.execute("UPDATE seeker_settings SET template = ?1", [serde_json::to_string(&export.template).unwrap()]).unwrap();
    let mut record = catalog::load(&a).unwrap().records[0].clone();
    record.text.push_str("\n补一句");
    catalog::put(&a, &record).unwrap();

    let rec = crate::picky::transfer::export(&a).unwrap();
    let json = serde_json::to_value(&rec).unwrap();
    assert!(json["seeker"].get("warnings").is_none() && json["seeker"].get("choices").is_none());
    let seeker = rec.seeker.as_ref().unwrap();
    assert_eq!((seeker.catalog.records.len(), seeker.catalog.revisions.len(), seeker.catalog.versions.len(), seeker.output.exports.len(), seeker.output.links.len()), (12, 1, 1, 1, 1));
    assert!(seeker.output.template.is_some() && seeker.catalog.settings.thinking && seeker.baseline.is_some());

    let b = memory_db();
    crate::picky::transfer::replace(&b, &serde_json::from_value(json).unwrap()).unwrap();
    assert_eq!(crate::picky::transfer::export(&b).unwrap(), rec);
    // a file from before Seeker leaves it as it is
    crate::picky::transfer::replace(&b, &crate::picky::transfer::PickyRecord { seeker: None, ..rec.clone() }).unwrap();
    assert_eq!(crate::picky::transfer::export(&b).unwrap(), rec);
    // a broken draft is refused before anything is written
    let mut bad = rec.clone();
    bad.seeker.as_mut().unwrap().draft.as_mut().unwrap().header[1].id = "header-0".into();
    assert!(crate::picky::transfer::validate(&bad).is_err());
}
