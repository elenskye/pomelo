//! 「Seeker 资料」 (v0.7 阶段 ④): 小拾 reads Seeker and Rater, never writes them. One tool, views:
//! materials (基本信息 and 素材, by language), resume (the current drafts), exports (Word exports still
//! on disk and the jobs they went to; no copy of their text), questions (Rater's bank,
//! each with its practice record), practice (Rater's recent card rounds and their feedback),
//! interviews (Rater's 准备 and 复盘), files (cv/'s resumes and material/'s file names) and cv (one
//! resume's text from cv/; material/ is never read).
//! - Only what the user lets go to models: disabled records (「用于选材 / 发给模型」 off) aren't read.
//! - A cloud model only when Seeker's 「允许发给云端」 is on (Rater follows it too); a module hidden in
//!   the 模块坞 isn't read (Seeker: everything but questions / practice; Rater: those). data.rs `Access`.
//! - The text is cut to the room left (data.rs `Found`). Rules, 档案 and 网申 aren't offered.
use crate::picky::assistants::rater::{self, interview, practice};
use crate::picky::Mode;
use crate::picky::assistants::seeker;
use crate::picky::data::{self, Access, Answer, Found};
use rusqlite::Connection;
use serde_json::{json, Value};

pub const TOOL: &str = "pomelo_seeker";
const VIEWS: [&str; 8] = ["materials", "resume", "exports", "questions", "practice", "interviews", "files", "cv"];
/// Rounds the practice view shows (newest first).
const ROUNDS: usize = 10;

pub fn tool() -> Value {
    let s = |d: &str| json!({ "type": "string", "description": d });
    json!({ "type": "function", "function": { "name": TOOL, "description": "读用户的求职资料（Seeker 写简历、Rater 面试题库），只读。view=materials：简历素材和基本信息（lang 选中文 cn / 英文 en，不填两种都看；keyword 按字筛）；view=resume：现在的简历草稿；view=exports：导出过的 Word 和它们用在哪个岗位（正文用 view=cv 读）；view=questions：面试题库和练习记录（category 选类别，keyword 按字筛）；view=practice：最近几轮卡片练习；view=interviews：面试准备和复盘；view=files：cv/ 的简历和 material/ 的文件名；view=cv：读 cv/ 一份简历（keyword 写文件名）。", "parameters": { "type": "object", "properties": {
        "view": { "type": "string", "enum": VIEWS },
        "lang": { "type": "string", "enum": ["cn", "en"], "description": "materials / resume / exports：只看中文或英文" },
        "keyword": s("materials / questions：要有的字（空格隔开的几个都要有）"),
        "category": { "type": "string", "enum": rater::CATEGORIES }
    }, "required": ["view"] } } })
}

pub(crate) fn blocked(module: &str, label: &str, access: &Access) -> Answer {
    let (head, text) = if access.hidden.iter().any(|h| h == module) {
        ("已隐藏，没看", format!("没有查：{label}在模块坞里隐藏了，小拾不读它的数据。"))
    } else {
        ("不发给云端，没看", format!("没有查：Seeker 资料没允许发给云端模型（Seeker 的设置里能开）。请在回答开头写明「{label}不发给云端，这部分没看」，并提醒可以换本地模型再问。"))
    };
    Answer { module: Some(module.into()), label: format!("{label} · {head}"), state: "blocked", text, records: vec![] }
}

fn failed(label: &str, text: String) -> Answer {
    Answer { module: None, label: label.into(), state: "failed", text, records: vec![] }
}

fn lang_name(lang: &str) -> &'static str {
    if lang == "en" { "英文" } else { "中文" }
}

/// Every word of `keyword` is in `text` (case-insensitive).
fn has(text: &str, keyword: &str) -> bool {
    let t = text.to_lowercase();
    keyword.split_whitespace().all(|w| t.contains(&w.to_lowercase()))
}

/// A draft as plain text: its header, then each section with its entries and points.
pub fn draft_text(d: &seeker::Draft) -> String {
    let mut out = vec![];
    if d.lang == "en" {
        out.push(if d.form == "cv" { "（英文 CV）".to_string() } else { "（英文 Resume）".to_string() });
    }
    out.push(d.name.clone());
    out.extend(d.header.iter().filter(|h| h.visible && !h.value.trim().is_empty()).map(|h| format!("{}：{}", h.label, h.value.trim())));
    for s in &d.sections {
        out.push(format!("\n【{}】", s.title));
        for e in &s.entries {
            let head = [&e.time, &e.name, &e.role, &e.location].iter().map(|x| x.trim()).filter(|x| !x.is_empty()).collect::<Vec<_>>().join("｜");
            if !head.is_empty() && s.category != "技能" && s.category != "证书" && s.category != "摘要" {
                out.push(head);
            }
            if !e.note.trim().is_empty() {
                out.push(e.note.trim().into());
            }
            out.extend(e.points.iter().map(|p| format!("- {}", p.text.replace("**", ""))));
        }
    }
    out.join("\n")
}

fn material_text(r: &seeker::catalog::Record) -> String {
    let mut lines = vec![];
    if r.kind == "profile" {
        lines.push(format!("【基本信息 · {}】", lang_name(&r.lang)));
        lines.extend(r.fields.iter().filter(|f| !f.value.trim().is_empty()).map(|f| format!("- {}：{}", f.label, f.value.trim())));
        return lines.join("\n");
    }
    let head = [r.title.as_str(), &r.role, &r.time, &r.location].iter().map(|x| x.trim()).filter(|x| !x.is_empty()).collect::<Vec<_>>().join("｜");
    lines.push(format!("【{} · {}】{head}", lang_name(&r.lang), r.category));
    if !r.text.trim().is_empty() {
        lines.push(format!("  {}", r.text.trim()));
    }
    lines.extend(r.choosable().iter().map(|p| format!("  - {}", p.text.replace("**", ""))));
    lines.join("\n")
}

/// Runs one call. `budget`: tokens the result may take.
pub fn run(c: &Connection, mode: Mode, access: &Access, arguments: &str, budget: i64) -> Answer {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let view = args["view"].as_str().unwrap_or("");
    if !VIEWS.contains(&view) {
        return failed("Seeker 资料 · 没有这种查法", format!("view 只能是 {}。", VIEWS.join("、")));
    }
    let rater_view = ["questions", "practice", "interviews"].contains(&view);
    let (module, label) = if rater_view { (data::RATER, "Rater 题库") } else { (data::SEEKER, "Seeker 资料") };
    if !access.allows(module) {
        return blocked(module, if rater_view { "Rater" } else { "Seeker" }, access);
    }
    let lang = args["lang"].as_str().filter(|l| seeker::LANGS.contains(l));
    let keyword = args["keyword"].as_str().unwrap_or("").trim();
    let done = |found: Found| {
        let (text, records) = found.render(budget);
        Answer { module: Some(module.into()), label: found_label(view, label), state: "ok", text, records }
    };
    if view == "questions" {
        let bank = match rater::read(c) {
            Ok(b) => b,
            Err(e) => return failed("Rater 题库 · 没读成", format!("没读成：{e}")),
        };
        let category = args["category"].as_str().filter(|x| rater::CATEGORIES.contains(x));
        let rows: Vec<String> = bank
            .questions
            .iter()
            .filter(|q| category.is_none_or(|x| q.category == x) && has(&format!("{} {} {}", q.question, q.answer, q.tags.join(" ")), keyword))
            .map(|q| {
                let secs = bank.seconds.get(&q.id).copied().unwrap_or(0);
                let over = if secs > rater::LIMIT { "，超过 90 秒" } else { "" };
                let job = q.job.as_ref().map(|j| format!(" · 岗位：{}", j.label)).unwrap_or_default();
                let tags = if q.tags.is_empty() { String::new() } else { format!(" · 标签：{}", q.tags.join("、")) };
                let answer = if q.answer.trim().is_empty() { "（还没写答案）".to_string() } else { q.answer.trim().to_string() };
                let practiced = match bank.practice.get(&q.id) {
                    Some(t) => format!(" · 练过：会 {} 次、不熟 {} 次{}，最近 {}", t.know, t.unsure, if t.last_mark == practice::UNSURE { "（上次不熟）" } else { "" }, t.last.as_deref().map_or("", |d| &d[..10.min(d.len())])),
                    None => " · 还没练过".into(),
                };
                format!("【{}】{}（约 {secs} 秒{over}{job}{tags}{practiced}）\n  答：{answer}", q.category, q.question)
            })
            .collect();
        let head = vec![format!("Rater 题库一共 {} 道题，符合的 {} 道：", bank.questions.len(), rows.len())];
        return done(Found::new(String::new(), head).rows(rows.into_iter().map(data::row).collect()));
    }
    if view == "practice" {
        let rounds = match practice::rounds(c) {
            Ok(r) => r,
            Err(e) => return failed("Rater 练习 · 没读成", format!("没读成：{e}")),
        };
        let rows: Vec<String> = rounds.iter().rev().take(ROUNDS).map(round_text).collect();
        let head = vec![format!("Rater 卡片练习一共 {} 轮，最近的 {} 轮（新的在前）：", rounds.len(), rows.len())];
        return done(Found::new(String::new(), head).rows(rows.into_iter().map(data::row).collect()));
    }
    if view == "interviews" {
        let all = match interview::read_all(c) {
            Ok(a) => a,
            Err(e) => return failed("Rater 面试 · 没读成", format!("没读成：{e}")),
        };
        let mut rows: Vec<String> = all.preps.iter().filter(|p| has(&p.job.label, keyword)).map(prep_text).collect();
        rows.extend(all.reviews.iter().filter(|r| has(&r.job.label, keyword)).map(review_text));
        let head = vec![format!("Rater 面试记录：准备 {} 份、复盘 {} 次，符合的 {} 条：", all.preps.len(), all.reviews.len(), rows.len())];
        return done(Found::new(String::new(), head).rows(rows.into_iter().map(data::row).collect()));
    }
    if view == "files" || view == "cv" {
        let root = match seeker::files::root(c, mode) {
            Ok(Some(r)) => r,
            Ok(None) => return failed("Seeker 资料 · 没选文件夹", "Seeker 还没选文件夹，cv/ 和 material/ 没法看。".into()),
            Err(e) => return failed("Seeker 资料 · 没读成", format!("没读成：{e}")),
        };
        let files = seeker::files::scan(&root, &[], &[]);
        if view == "cv" {
            let hits: Vec<&seeker::files::CvFile> = files.cv.iter().filter(|f| !keyword.is_empty() && has(&f.name, keyword)).collect();
            let file = match hits.as_slice() {
                [one] => *one,
                [] => return failed("Seeker 资料 · 没有这份简历", format!("cv/ 里没有名字里有「{keyword}」的简历，先用 view=files 看文件名。")),
                many => return failed("Seeker 资料 · 对上了几份", format!("对上了 {} 份：{}。请写完整的文件名。", many.len(), many.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join("、"))),
            };
            return match seeker::files::text(&root, std::path::Path::new(&file.path)) {
                Ok(text) => Answer { module: Some(data::SEEKER.into()), label: format!("Seeker 资料 · {}", file.name), state: "ok", text: Found::new(String::new(), vec![format!("cv/{} 的正文：", file.name)]).rows(vec![data::row(text)]).render(budget).0, records: vec![] },
                Err(e) => failed("Seeker 资料 · 没读成", format!("没读成：{e}")),
            };
        }
        let mut rows: Vec<String> = files.cv.iter().map(|f| format!("- cv/{}（{}{}{}，{}）", f.name, lang_name(&f.lang), f.version.map(|v| format!("，第 {v} 版")).unwrap_or_default(), if f.template { "，模板" } else { "" }, day(f.modified))).collect();
        let mut groups: Vec<&str> = files.material.iter().map(|m| m.group.as_str()).collect();
        groups.dedup();
        rows.extend(groups.iter().map(|g| format!("- material/{}：{}", if g.is_empty() { "" } else { g }, files.material.iter().filter(|m| m.group == *g).map(|m| m.name.as_str()).collect::<Vec<_>>().join("、"))));
        let head = vec![format!("Seeker 文件夹里 cv/ 有 {} 份简历，material/ 有 {} 个证书原件（只给文件名，不读内容）：", files.cv.len(), files.material.len())];
        return done(Found::new(String::new(), head).rows(rows.into_iter().map(data::row).collect()));
    }
    let w = match seeker::read(c) {
        Ok(w) => w,
        Err(e) => return failed("Seeker 资料 · 没读成", format!("没读成：{e}")),
    };
    let langs: Vec<&str> = lang.map_or(seeker::LANGS.to_vec(), |l| vec![l]);
    match view {
        "materials" => {
            let rows: Vec<String> = w.catalog.records.iter().filter(|r| r.enabled && ["profile", "material"].contains(&r.kind.as_str()) && langs.contains(&r.lang.as_str())).map(material_text).filter(|t| has(t, keyword)).collect();
            done(Found::new(String::new(), vec![format!("Seeker 的简历素材（{}，只列启用的）：", langs.iter().map(|l| lang_name(l)).collect::<Vec<_>>().join("、"))]).rows(rows.into_iter().map(data::row).collect()))
        }
        "resume" => {
            let rows: Vec<String> = langs
                .iter()
                .map(|l| match w.draft(l) {
                    Some(d) => {
                        let jd: String = d.jd.trim().chars().take(400).collect();
                        format!("=== {}简历草稿 ===\n{}{}", lang_name(l), draft_text(d), if jd.is_empty() { String::new() } else { format!("\n\n目标 JD（开头）：{jd}") })
                    }
                    None => format!("=== {}简历：还没有草稿 ===", lang_name(l)),
                })
                .collect();
            done(Found::new(String::new(), vec![]).rows(rows.into_iter().map(data::row).collect()))
        }
        _ => {
            let rows = w
                .output
                .exports
                .iter()
                // a file deleted from disk isn't listed (its record stays)
                .filter(|e| langs.contains(&e.lang.as_str()) && std::path::Path::new(&e.path).is_file())
                .map(|e| {
                    let jobs: Vec<String> = w.output.links.iter().filter(|l| l.path == e.path).map(|l| format!("{}{}{}", l.job.company, l.job.position.as_deref().map(|p| format!(" · {p}")).unwrap_or_default(), if l.current { "（本次使用）" } else { "" })).collect();
                    format!("- {}（{}，{}）{}", e.label, lang_name(&e.lang), &e.created_at[..10.min(e.created_at.len())], if jobs.is_empty() { String::new() } else { format!("：用在 {}", jobs.join("、")) })
                })
                .collect::<Vec<_>>();
            done(Found::new(String::new(), vec![format!("导出过的 Word {} 份：", rows.len())]).rows(rows.into_iter().map(data::row).collect()))
        }
    }
}

/// A date (UTC) from seconds since 1970.
fn day(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    // civil from days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

fn prep_text(p: &interview::Prep) -> String {
    let sheet: String = p.sheet.chars().take(1500).collect();
    format!("【准备 · {} · {} · {}{}】简历：{}
{sheet}", &p.created_at[..10.min(p.created_at.len())], p.job.label, p.lang, if p.form.is_empty() { String::new() } else { format!(" · {}", p.form) }, p.resume)
}

fn review_text(r: &interview::Review) -> String {
    let mut lines = vec![format!("【复盘 · {} · {}{}{}】", r.date, r.job.label, if r.round.is_empty() { String::new() } else { format!(" · {}", r.round) }, if r.result.is_empty() { String::new() } else { format!(" · {}", r.result) })];
    for a in &r.items {
        lines.push(format!("- 问：{}{}{}", a.question, if a.answer.is_empty() { String::new() } else { format!("\n  我答：{}", a.answer) }, if a.feedback.is_empty() { String::new() } else { format!("\n  反应：{}", a.feedback) }));
    }
    if !r.feedback.is_empty() {
        lines.push(format!("面试官的反馈：{}", r.feedback));
    }
    if !r.thoughts.is_empty() {
        lines.push(format!("我的判断：{}", r.thoughts));
    }
    if let Some(a) = &r.analysis {
        lines.push(format!("对照：{}", a.summary));
        if !a.misses.is_empty() {
            lines.push(format!("遗漏：{}", a.misses.iter().map(|m| format!("{}（{}）", r.items.get(m.asked).map_or("", |x| x.question.as_str()), m.text)).collect::<Vec<_>>().join("；")));
        }
    }
    lines.join("\n")
}

/// One round: when, how, the counts, which cards were 不熟, the follow-ups and the feedback.
fn round_text(r: &practice::Round) -> String {
    let marked = |m: &str| r.cards.iter().filter(|c| c.mark.as_deref() == Some(m)).count();
    let mut lines = vec![format!("【{} · {}{}】{} 张：会 {}、不熟 {}{}{}", &r.started_at[..10.min(r.started_at.len())], r.mode, r.job.as_ref().map(|j| format!(" · {}", j.label)).unwrap_or_default(), r.cards.iter().filter(|c| !c.follow).count(), marked(practice::KNOW), marked(practice::UNSURE), match r.cards.iter().filter(|c| c.follow).count() { 0 => String::new(), n => format!("、追问 {n}") }, if r.ended_at.is_none() { "（还在练）" } else { "" })];
    let unsure: Vec<String> = r.cards.iter().filter(|c| c.mark.as_deref() == Some(practice::UNSURE)).map(|c| format!("{}{}", if c.follow { "追问：" } else { "" }, c.text)).collect();
    if !unsure.is_empty() {
        lines.push(format!("  不熟：{}", unsure.join("；")));
    }
    if let Some(f) = &r.feedback {
        lines.extend([(!f.summary.is_empty()).then(|| format!("  小拾的反馈：{}", f.summary)), (!f.good.is_empty()).then(|| format!("  说得好：{}", f.good.join("；"))), (!f.gaps.is_empty()).then(|| format!("  缺口：{}", f.gaps.join("；")))].into_iter().flatten());
    }
    lines.join("\n")
}

fn found_label(view: &str, label: &str) -> String {
    let what = match view {
        "materials" => "素材",
        "resume" => "简历草稿",
        "exports" => "导出的 Word",
        "practice" => "卡片练习",
        "interviews" => "面试",
        "files" => "cv/ 和 material/",
        _ => "题目",
    };
    format!("{label} · {what}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::assistants::seeker::catalog::{self, Record};
    use crate::picky::store::tests::memory_db;

    fn material(id: &str, lang: &str, title: &str, enabled: bool) -> Record {
        Record { id: id.into(), lang: lang.into(), category: "项目".into(), title: title.into(), text: "概览".into(), points: vec![seeker::Point { id: "a".into(), text: "**做事**：一条".into() }], enabled, ..Default::default() }
    }

    #[test]
    fn it_reads_what_may_go_to_the_model_and_nothing_hidden_or_not_for_the_cloud() {
        let c = memory_db();
        catalog::put(&c, &material("m1", "cn", "柚子笔记", true)).unwrap();
        catalog::put(&c, &material("m2", "en", "Pomelo Notes", true)).unwrap();
        catalog::put(&c, &material("m3", "cn", "停用的项目", false)).unwrap();
        rater::put(&c, &rater::Question { id: "q".into(), category: "通用题".into(), question: "自我介绍".into(), answer: "字".repeat(500), tags: vec!["必背".into()], ..Default::default() }).unwrap();
        let local = Access::default();
        let a = run(&c, Mode::Mock, &local, r#"{"view":"materials"}"#, 2000);
        assert_eq!((a.state, a.module.as_deref()), ("ok", Some("seeker")));
        assert!(a.text.contains("【中文 · 项目】柚子笔记") && a.text.contains("【英文 · 项目】Pomelo Notes") && a.text.contains("- 做事：一条") && !a.text.contains("停用"), "{}", a.text);
        let en = run(&c, Mode::Mock, &local, r#"{"view":"materials","lang":"en"}"#, 2000);
        assert!(!en.text.contains("柚子笔记"));
        let q = run(&c, Mode::Mock, &local, r#"{"view":"questions","category":"通用题"}"#, 2000);
        assert_eq!(q.module.as_deref(), Some("rater"));
        assert!(q.text.contains("【通用题】自我介绍（约 113 秒，超过 90 秒 · 标签：必背 · 还没练过）"), "{}", q.text);
        let r = practice::begin(&c, &practice::Start { mode: "自己挑".into(), questions: vec!["q".into()], ..Default::default() }, None).unwrap();
        practice::mark(&c, &r.id, 0, practice::UNSURE, "").unwrap();
        practice::set_feedback(&c, &r.id, practice::Feedback { summary: "开头太长".into(), gaps: vec!["没讲清在哪里实习".into()], ..Default::default() }).unwrap();
        assert!(run(&c, Mode::Mock, &local, r#"{"view":"questions"}"#, 2000).text.contains("必背 · 练过：会 0 次、不熟 1 次（上次不熟），最近 20"));
        let p = run(&c, Mode::Mock, &local, r#"{"view":"practice"}"#, 2000);
        assert_eq!((p.module.as_deref(), p.label.as_str()), (Some("rater"), "Rater 题库 · 卡片练习"));
        assert!(p.text.contains("自己挑】1 张：会 0、不熟 1\n  不熟：自我介绍\n  小拾的反馈：开头太长\n  缺口：没讲清在哪里实习"), "{}", p.text);
        assert!(run(&c, Mode::Mock, &Access { hidden: vec![data::RATER.into()], ..Default::default() }, r#"{"view":"practice"}"#, 2000).text.contains("隐藏"));
        assert!(run(&c, Mode::Mock, &local, r#"{"view":"resume"}"#, 2000).text.contains("中文简历：还没有草稿"));
        assert_eq!(run(&c, Mode::Mock, &local, r#"{"view":"rules"}"#, 2000).state, "failed");

        // a cloud model: only when Seeker allows it (Rater too); hidden modules never
        let cloud = Access { cloud: true, ..Default::default() };
        let refused = run(&c, Mode::Mock, &cloud, r#"{"view":"questions"}"#, 2000);
        assert_eq!(refused.state, "blocked");
        assert!(refused.text.contains("Rater不发给云端"), "{}", refused.text);
        let allowed = Access { cloud: true, cloud_allowed: vec![data::SEEKER.into()], ..Default::default() };
        assert_eq!(run(&c, Mode::Mock, &allowed, r#"{"view":"questions"}"#, 2000).state, "ok");
        let hidden = Access { hidden: vec![data::RATER.into()], ..Default::default() };
        assert!(run(&c, Mode::Mock, &hidden, r#"{"view":"questions"}"#, 2000).text.contains("隐藏"));
        assert_eq!(run(&c, Mode::Mock, &hidden, r#"{"view":"materials"}"#, 2000).state, "ok");
        assert!(!allowed.allows("seeker_other") && allowed.allows(data::SEEKER) && !cloud.allows(data::SEEKER));
    }
}
