//! 面试 (Rater, v0.7 阶段 ⑦). picky.db `rater_interviews` (migration 0026): one row per 准备 (`Prep`)
//! or 复盘 (`Review`), a JSON body under a 投石问路 job (id + created_at, as Seeker keeps jobs).
//! - 准备 (`prep_prompt`): the resume the job uses (its 「本次使用」 under cv/: the file's text, and for a
//!   Pomelo export the materials it was made from; else the draft of the interview's language), the JD, its company's knowledge/, the bank (通用题, 简历题 of the resume's
//!   experiences, this job's 突击题, with their answers and practice records) and the experiences'
//!   经历档案. Rater's model picks the questions (ids it must not invent), adds likely follow-ups with
//!   reference answers and writes a one-page 速查卡 (Markdown, the user edits it; 另存 Word). Nothing
//!   goes back to the bank or the resume unless the user keeps a follow-up (`keep_follow`).
//!   Its framework is a rule (规则 · 面试, 「速查卡」, qbank.rs `PREP` until the user writes their own);
//!   the JSON shape stays here.
//! - 复盘: the user's record (date, round, form, result; each question asked with their answer and
//!   the interviewer's word; the overall feedback; their own judgement kept apart), filled by Rater's
//!   model in a conversation (record.rs, v0.7 R8) or edited by hand.
//! - 总结 (`Summary`, R8; kind `summary`): one or more reviews of a job (oldest first, their questions
//!   numbered on through them all) against a prep (or none: the bank only) → hits, misses, questions
//!   worth keeping, at most two long-term rule points (`summary_prompt`, framework 规则 · 复盘 「复盘卡」).
//!   Proposals only: 存进题库 and 加进规则 are the user's clicks (`keep_question`, `adopt_rule`; also on a
//!   review's own 对照 from before R8, kept as it was). One result doesn't make a resume good or bad.
//! - Every write is one undo step; the Word file is not undone (like Seeker's exports).
//! - Data file (format 25): with Rater's bank (`Bank::interviews`); a file without them leaves them.
//!   Format 32 adds the summaries.
use super::qbank::{self, Options};
use super::*;
use crate::picky::assistants::seeker::{self, catalog::Record, output::JobSnapshot, Workspace};
use crate::picky::store::{pref_get, pref_set, NOW};
use serde_json::{json, Value};
use std::path::Path;
use tauri::{AppHandle, Manager};

pub const LANGS: [&str; 2] = ["中文", "English"];
pub const RESULTS: [&str; 4] = ["", "待定", "通过", "没过"];
/// The groups a proposed rule point may go to.
pub const RULE_USES: [&str; 7] = [REVIEW, INTERVIEW, PRACTICE, QBANK, "简历题", "通用题", "突击题"];
/// The rule groups 准备面试 and 总结复盘 read.
pub const PREP_READS: [&str; 2] = [QBANK, INTERVIEW];
pub const SUMMARY_READS: [&str; 2] = [QBANK, REVIEW];
const PICK_MAX: usize = 20;
const FOLLOW_MAX: usize = 12;
const FOLDER: &str = "rater.word_folder";

/// A likely follow-up the prep added (or a question the review says to keep): kept when saved.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Follow {
    /// What it follows up: a question's text or a term on the resume.
    pub about: String,
    pub category: String,
    pub question: String,
    pub answer: String,
    pub saved: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Prep {
    pub id: String,
    pub job: JobLink,
    /// Which resume it was made from (an export's name, or 「中文草稿」…).
    pub resume: String,
    /// 中文 / English.
    pub lang: String,
    /// 形式 (「视频一面 · 技术面」).
    pub form: String,
    pub request: String,
    /// Bank questions, in interview order.
    pub questions: Vec<String>,
    pub follows: Vec<Follow>,
    /// The one-page 速查卡 (Markdown).
    pub sheet: String,
    pub advice: String,
    pub model: String,
    pub note: String,
    pub created_at: String,
}

/// One question as it was asked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Asked {
    pub question: String,
    pub answer: String,
    pub feedback: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Matched {
    /// A summary's: the review it was asked in (a review's own 对照: none).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub review: String,
    /// Index into the review's items.
    pub asked: usize,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RuleIdea {
    #[serde(rename = "use")]
    pub use_: String,
    pub title: String,
    pub text: String,
    pub why: String,
    /// The rule it went into.
    pub adopted: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Analysis {
    pub model: String,
    pub prep: Option<String>,
    pub summary: String,
    /// asked → what in the prep had it.
    pub hits: Vec<Matched>,
    /// asked → why it wasn't prepared or fell short.
    pub misses: Vec<Matched>,
    pub questions: Vec<Follow>,
    pub rules: Vec<RuleIdea>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Review {
    pub id: String,
    pub job: JobLink,
    /// YYYY-MM-DD.
    pub date: String,
    /// 一面 / 二面 / HR 面…
    pub round: String,
    pub form: String,
    pub result: String,
    pub items: Vec<Asked>,
    /// What the interviewer said overall.
    pub feedback: String,
    /// The user's own judgement (kept apart from what happened).
    pub thoughts: String,
    pub analysis: Option<Analysis>,
    pub created_at: String,
}

/// 总结复盘: some reviews of a job against a prep (none: the bank only).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Summary {
    pub id: String,
    pub job: JobLink,
    pub prep: Option<String>,
    /// Oldest first.
    pub reviews: Vec<String>,
    pub analysis: Analysis,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Interviews {
    /// Newest first.
    pub preps: Vec<Prep>,
    pub reviews: Vec<Review>,
    pub summaries: Vec<Summary>,
}

impl Interviews {
    pub fn len(&self) -> usize {
        self.preps.len() + self.reviews.len() + self.summaries.len()
    }
}

pub(super) fn now(c: &Connection) -> Result<String, String> {
    c.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0)).map_err(err)
}

pub fn read_all(c: &Connection) -> Result<Interviews, String> {
    let mut st = c.prepare("SELECT kind, body FROM rater_interviews ORDER BY created_at DESC, rowid DESC").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let mut out = Interviews::default();
    for (kind, body) in rows {
        match kind.as_str() {
            "prep" => out.preps.push(json(&body)?),
            "summary" => out.summaries.push(json(&body)?),
            _ => out.reviews.push(json(&body)?),
        }
    }
    Ok(out)
}

fn store(c: &Connection, kind: &str, id: &str, job: &JobLink, body: &str, created_at: &str) -> Result<(), String> {
    c.execute("INSERT INTO rater_interviews (id, kind, job_id, job_created_at, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET body = excluded.body", params![id, kind, job.id, job.created_at, body, created_at]).map_err(err)?;
    Ok(())
}

fn put_prep(c: &Connection, p: &Prep) -> Result<(), String> {
    store(c, "prep", &p.id, &p.job, &serde_json::to_string(p).map_err(err)?, &p.created_at)
}

fn put_review_row(c: &Connection, r: &Review) -> Result<(), String> {
    store(c, "review", &r.id, &r.job, &serde_json::to_string(r).map_err(err)?, &r.created_at)
}

fn put_summary(c: &Connection, s: &Summary) -> Result<(), String> {
    store(c, "summary", &s.id, &s.job, &serde_json::to_string(s).map_err(err)?, &s.created_at)
}

pub fn get_prep(c: &Connection, id: &str) -> Result<Prep, String> {
    let body: String = c.query_row("SELECT body FROM rater_interviews WHERE id = ?1 AND kind = 'prep'", [id], |r| r.get(0)).map_err(|_| "这份准备已经删除了".to_string())?;
    json(&body)
}

pub fn get_review(c: &Connection, id: &str) -> Result<Review, String> {
    let body: String = c.query_row("SELECT body FROM rater_interviews WHERE id = ?1 AND kind = 'review'", [id], |r| r.get(0)).map_err(|_| "这次复盘已经删除了".to_string())?;
    json(&body)
}

pub fn remove(c: &Connection, id: &str) -> Result<(), String> {
    if c.execute("DELETE FROM rater_interviews WHERE id = ?1", [id]).map_err(err)? == 0 {
        return Err("已经删除了".into());
    }
    Ok(())
}

/// A prep the user edited (速查卡, 形式…): the questions and follow-ups it keeps as they are.
pub fn save_prep(c: &Connection, p: &Prep) -> Result<(), String> {
    let old = get_prep(c, &p.id)?;
    if p.sheet.trim().is_empty() {
        return Err("速查卡不能是空的".into());
    }
    put_prep(c, &Prep { sheet: p.sheet.clone(), form: p.form.trim().into(), ..old })
}

/// A new review (no id yet: one is made) or an edit of one; its analysis stays.
pub fn save_review(c: &Connection, input: &Review, job: Option<JobLink>) -> Result<Review, String> {
    let mut r = input.clone();
    r.round = r.round.trim().into();
    r.form = r.form.trim().into();
    r.items = r.items.into_iter().map(|a| Asked { question: a.question.trim().into(), answer: a.answer.trim_end().into(), feedback: a.feedback.trim_end().into() }).filter(|a| !(a.question.is_empty() && a.answer.is_empty() && a.feedback.is_empty())).collect();
    if r.items.iter().any(|a| a.question.is_empty()) {
        return Err("每一条都要写问题".into());
    }
    if !RESULTS.contains(&r.result.as_str()) {
        return Err("未知的结果".into());
    }
    if r.round.chars().count() > 40 || r.form.chars().count() > 60 {
        return Err("轮次或形式太长了".into());
    }
    if r.date.trim().is_empty() {
        return Err("请填写面试日期".into());
    }
    if r.id.is_empty() {
        r.id = seeker::workflow::id()?;
        r.job = job.ok_or("复盘要挂在一个岗位下")?;
        r.created_at = now(c)?;
        r.analysis = None;
    } else {
        let old = get_review(c, &r.id)?;
        (r.job, r.created_at, r.analysis) = (old.job, old.created_at, old.analysis);
    }
    put_review_row(c, &r)?;
    Ok(r)
}

/// 存进题库: a prep's follow-up becomes a bank question (once).
pub fn keep_follow(c: &Connection, id: &str, index: usize) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    let mut p = get_prep(&tx, id)?;
    let f = p.follows.get(index).ok_or("没有这道追问")?;
    if f.saved.is_some() {
        return Err("已经存进题库了".into());
    }
    let qid = keep(&tx, f, &p.job)?;
    p.follows[index].saved = Some(qid);
    put_prep(&tx, &p)?;
    tx.commit()
}

/// An analysis to change: a review's own 对照 (before R8) or a summary's; saved after `f`.
fn with_analysis<T>(c: &Connection, id: &str, f: impl FnOnce(&Connection, &mut Analysis, &JobLink) -> Result<T, String>) -> Result<T, String> {
    let kind: String = c.query_row("SELECT kind FROM rater_interviews WHERE id = ?1", [id], |r| r.get(0)).map_err(|_| "这条已经删除了".to_string())?;
    if kind == "summary" {
        let body: String = c.query_row("SELECT body FROM rater_interviews WHERE id = ?1", [id], |r| r.get(0)).map_err(err)?;
        let mut s: Summary = json(&body)?;
        let out = f(c, &mut s.analysis, &s.job)?;
        put_summary(c, &s)?;
        return Ok(out);
    }
    let mut r = get_review(c, id)?;
    let job = r.job.clone();
    let out = f(c, r.analysis.as_mut().ok_or("还没有对照")?, &job)?;
    put_review_row(c, &r)?;
    Ok(out)
}

/// 存进题库: a question a summary (or a review's 对照) says to keep (once).
pub fn keep_question(c: &Connection, id: &str, index: usize) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    with_analysis(&tx, id, |c, a, job| {
        let f = a.questions.get(index).ok_or("没有这道题")?;
        if f.saved.is_some() {
            return Err("已经存进题库了".into());
        }
        let qid = keep(c, f, job)?;
        a.questions[index].saved = Some(qid);
        Ok(())
    })?;
    tx.commit()
}

fn keep(c: &Connection, f: &Follow, job: &JobLink) -> Result<String, String> {
    let category = if CATEGORIES.contains(&f.category.as_str()) { f.category.clone() } else { "突击题".into() };
    let q = Question { id: seeker::workflow::id()?, job: (category == "突击题").then(|| job.clone()), category, question: f.question.clone(), answer: f.answer.clone(), ..Default::default() };
    put(c, &q)?;
    Ok(q.id)
}

/// 加进规则: a proposed rule becomes a point of the rule with that title in the group it names (Rater's
/// rules page), else a new rule of one point.
pub fn adopt_rule(c: &Connection, id: &str, index: usize) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    with_analysis(&tx, id, |c, a, _| {
        let idea = a.rules.get(index).ok_or("没有这条")?;
        if idea.adopted.is_some() {
            return Err("已经加进规则了".into());
        }
        let point = seeker::Point { id: seeker::workflow::id()?, text: idea.text.trim().to_string() };
        let same = seeker::catalog::load(c)?.records.into_iter().find(|x| x.kind == "rule" && x.category == idea.use_ && x.title.trim() == idea.title.trim() && x.for_lang("cn"));
        let record = match same {
            Some(mut x) => {
                x.points.push(point);
                x
            }
            None => Record { id: seeker::workflow::id()?, kind: "rule".into(), category: idea.use_.clone(), title: idea.title.clone(), points: vec![point], lang: "cn".into(), ..Default::default() },
        };
        seeker::catalog::put(c, &record)?;
        a.rules[index].adopted = Some(record.id);
        Ok(())
    })?;
    tx.commit()
}

/// A new summary (it isn't edited: another one is made instead).
pub fn add_summary(c: &Connection, mut s: Summary) -> Result<Summary, String> {
    s.id = seeker::workflow::id()?;
    s.created_at = now(c)?;
    put_summary(c, &s)?;
    Ok(s)
}

/// The data-file import (inside its transaction): the file's preps and reviews replace these.
pub fn replace(c: &Connection, all: &Interviews) -> Result<(), String> {
    c.execute("DELETE FROM rater_interviews", []).map_err(err)?;
    for p in &all.preps {
        put_prep(c, p)?;
    }
    for r in &all.reviews {
        put_review_row(c, r)?;
    }
    for s in &all.summaries {
        put_summary(c, s)?;
    }
    Ok(())
}

// ---------- the model's part ----------

/// The resume an interview reads: its name, the materials it was made from (a Pomelo export, the draft;
/// none for other files: every 简历题) and its text.
#[derive(Debug, Clone, PartialEq)]
pub struct Used {
    pub name: String,
    pub materials: Vec<String>,
    pub text: String,
}

/// The job's résumé in use whose file is still there (`present`): the file's text (`text`, under cv/),
/// with an export's materials; no such file: the draft of the interview's language.
pub fn resume(w: &Workspace, job: &JobSnapshot, lang: &str, present: &dyn Fn(&str) -> bool, text: &dyn Fn(&str) -> Option<String>) -> Option<Used> {
    let code = if lang == LANGS[1] { "en" } else { "cn" };
    let linked: Vec<_> = w.output.links.iter().filter(|l| l.job.id == job.id && l.job.created_at == job.created_at && present(&l.path)).collect();
    if let Some(l) = linked.iter().find(|l| l.current).or(linked.first()) {
        if let Some(t) = text(&l.path).filter(|t| !t.trim().is_empty()) {
            let export = w.output.exports.iter().find(|e| e.path == l.path);
            let name = export.map(|e| e.label.clone()).unwrap_or_else(|| std::path::Path::new(&l.path).file_name().map_or(l.path.clone(), |n| n.to_string_lossy().into_owned()));
            return Some(Used { name, materials: export.map(|e| e.materials.clone()).unwrap_or_default(), text: t });
        }
    }
    w.draft(code).map(|d| Used { name: if code == "en" { "英文草稿".into() } else { "中文草稿".into() }, materials: seeker::output::materials_of(d), text: crate::picky::skills::seeker::draft_text(d) })
}

/// `resume` with the real files: present ones, the text of one under the Seeker folder's cv/.
pub fn resume_on_disk(state: &AppState, w: &Workspace, job: &JobSnapshot, lang: &str) -> Option<Used> {
    let root = state.picky.with_db(|c| seeker::files::root(c, state.picky.mode)).ok().flatten();
    resume(w, job, lang, &|p| std::path::Path::new(p).is_file(), &|p| root.as_ref().and_then(|r| seeker::files::text(r, std::path::Path::new(p)).ok()))
}

fn line(q: &Question, bank: &Bank) -> String {
    let practiced = match bank.practice.get(&q.id) {
        Some(t) if t.last_mark == super::practice::UNSURE => "，上次练习不熟",
        Some(_) => "，练过",
        None => "",
    };
    let answer: String = q.answer.trim().chars().take(240).collect();
    format!("题目 {}【{}】{}（约 {} 秒{practiced}）\n  答：{}", q.id, q.category, q.question, seconds(&q.answer), if answer.is_empty() { "（还没写）".into() } else { answer })
}

pub(super) fn jd_block(job: &JobSnapshot) -> String {
    let jd = job.note.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or("（这个岗位的备注里没有 JD）");
    format!("岗位：{}{}{}\nJD：\n{jd}", job.company, job.position.as_deref().map(|p| format!(" · {p}")).unwrap_or_default(), job.location.as_deref().map(|l| format!("（{l}）")).unwrap_or_default())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrepAsk {
    pub job: i64,
    pub lang: String,
    pub form: String,
    pub request: String,
}

/// 准备面试: the system message and the request (no model call); `employer`: what knowledge/ says.
pub fn prep_prompt(w: &Workspace, bank: &Bank, ask: &PrepAsk, job: &JobSnapshot, resume: Option<&Used>, employer: &str) -> Result<(String, String), String> {
    let lang = if ask.lang == LANGS[1] { LANGS[1] } else { LANGS[0] };
    // the resume's experiences: their 简历题 and 档案 (a file not made by Pomelo: every 简历题)
    let on_resume: Vec<&Record> = resume.map(|u| u.materials.iter().filter_map(|id| w.catalog.records.iter().find(|r| &r.id == id && r.enabled)).filter(|r| r.kind == "material" && !["技能", "摘要", "证书"].contains(&r.category.as_str())).collect()).unwrap_or_default();
    let ids: Vec<&str> = on_resume.iter().map(|r| r.id.as_str()).collect();
    let cn_of = |r: &&Record| -> Vec<String> {
        // an English material's 简历题 hang on its Chinese twin (the 档案 links both)
        let mut out = vec![r.id.clone()];
        for d in w.catalog.records.iter().filter(|d| d.is_dossier() && d.material_en.as_deref() == Some(&r.id)) {
            out.extend(d.material.clone());
        }
        out
    };
    let materials: Vec<String> = on_resume.iter().flat_map(cn_of).collect();
    let picked: Vec<&Question> = bank
        .questions
        .iter()
        .filter(|q| match q.category.as_str() {
            "通用题" => true,
            "简历题" => ids.is_empty() || q.material.as_ref().is_some_and(|m| materials.contains(m)),
            _ => q.job.as_ref().is_some_and(|j| j.id == job.id && j.created_at == job.created_at),
        })
        .collect();
    let mut sources = vec![jd_block(job)];
    match resume {
        Some(u) => sources.push(format!("投递的简历（{}）：\n{}", u.name, u.text)),
        None => sources.push("投递的简历：（还没有简历）".into()),
    }
    let dossiers: Vec<String> = on_resume.iter().flat_map(|r| w.catalog.records.iter().filter(|d| d.enabled && d.is_dossier() && (d.material.as_deref() == Some(&r.id) || d.material_en.as_deref() == Some(&r.id)))).map(|d| format!("经历档案《{}》：\n{}", d.title, d.text)).collect();
    sources.extend(dossiers);
    sources.extend(qbank::profile_and_facts(w));
    if !picked.is_empty() {
        sources.push(format!("题库里相关的题（复用这些答案，挑题只能挑这些编号）：\n{}", picked.iter().map(|q| line(q, bank)).collect::<Vec<_>>().join("\n")));
    }
    let english = lang == LANGS[1];
    let system = format!(
        "你是用户的中文面试准备助手，说明用中文。\n\n工作原则：\n{}\n\n任务：为这场面试做准备（面试语言：{lang}；形式：{}），照规则里的「速查卡」写出 questions（挑的题）、follows（可能的追问）和 sheet（速查卡）。{}\n仅输出 JSON：{{\"advice\":\"这次准备的重点、还缺什么（中文，一两句）\",\"questions\":[\"题目编号\"],\"follows\":[{{\"about\":\"…\",\"category\":\"简历题\",\"question\":\"…\",\"answer\":\"…\"}}],\"sheet\":\"Markdown\"}}\n\n规则：\n{}{employer}",
        seeker::model::principles(&[]),
        if ask.form.trim().is_empty() { "没说" } else { ask.form.trim() },
        if english { "\n面试语言是英文：自我介绍、答案要点和追问的应对用英文，标题和提醒用中文。" } else { "" },
        qbank::rules_of(w, &PREP_READS, true)
    );
    let user = format!("用户本次要求：{}\n\n资料：\n{}", if ask.request.trim().is_empty() { "（没有）" } else { ask.request.trim() }, sources.join("\n\n"));
    Ok((system, user))
}

pub fn parse_prep(v: &Value, bank: &Bank) -> Result<(Vec<String>, Vec<Follow>, String, String), String> {
    let ids: Vec<&str> = bank.questions.iter().map(|q| q.id.as_str()).collect();
    let mut picked: Vec<String> = vec![];
    for raw in v["questions"].as_array().into_iter().flatten() {
        let raw = raw.as_str().map(str::trim).unwrap_or("");
        let found = if ids.contains(&raw) { Some(raw) } else { ids.iter().filter(|id| raw.contains(**id)).max_by_key(|id| id.len()).copied() };
        if let Some(id) = found.filter(|id| !picked.iter().any(|x| x == id)) {
            picked.push(id.into());
        }
    }
    picked.truncate(PICK_MAX);
    let text = |x: &Value, k: &str| x[k].as_str().map(str::trim).unwrap_or("").to_string();
    let follows: Vec<Follow> = v["follows"].as_array().into_iter().flatten().map(|f| Follow { about: text(f, "about"), category: Some(text(f, "category")).filter(|c| CATEGORIES.contains(&c.as_str())).unwrap_or_else(|| "简历题".into()), question: text(f, "question"), answer: text(f, "answer"), saved: None }).filter(|f| !f.question.is_empty()).take(FOLLOW_MAX).collect();
    let sheet = text(v, "sheet");
    if sheet.is_empty() {
        return Err("模型没有写出速查卡，请重试".into());
    }
    Ok((picked, follows, sheet, text(v, "advice")))
}

/// 总结复盘: the reviews (oldest first; their questions numbered on through them all: `asked`), the
/// prep they are held against (none: the bank only), the job's bank questions, the JD.
pub fn summary_prompt(w: &Workspace, bank: &Bank, reviews: &[&Review], prep: Option<&Prep>, job: Option<&JobSnapshot>) -> (String, String) {
    let mut user = vec![];
    let mut n = 0;
    for (k, r) in reviews.iter().enumerate() {
        let head = [r.date.as_str(), r.round.as_str(), r.form.as_str()].iter().filter(|x| !x.trim().is_empty()).copied().collect::<Vec<_>>().join(" · ");
        let mut lines = vec![format!("{}（{head}{}）：", if reviews.len() > 1 { format!("第 {} 场面试", k + 1) } else { "这场面试".into() }, if r.result.is_empty() { String::new() } else { format!(" · 结果：{}", r.result) })];
        for a in &r.items {
            n += 1;
            lines.push(format!("问 {n}：{}\n  我答：{}\n  面试官当场的反应：{}", a.question, if a.answer.trim().is_empty() { "（没记）" } else { a.answer.trim() }, if a.feedback.trim().is_empty() { "（没记）" } else { a.feedback.trim() }));
        }
        if !r.feedback.trim().is_empty() {
            lines.push(format!("面试官的整体反馈（发生的事实）：{}", r.feedback.trim()));
        }
        if !r.thoughts.trim().is_empty() {
            lines.push(format!("用户自己的判断（个人看法，不是事实）：{}", r.thoughts.trim()));
        }
        user.push(lines.join("\n"));
    }
    match prep {
        Some(p) => {
            let picks: Vec<String> = p.questions.iter().filter_map(|id| bank.questions.iter().find(|q| &q.id == id)).map(|q| format!("- 【{}】{}", q.category, q.question)).collect();
            let follows: Vec<String> = p.follows.iter().map(|f| format!("- 追问：{}", f.question)).collect();
            user.push(format!("面试前的准备（{}）：\n挑的题：\n{}\n准备的追问：\n{}\n速查卡：\n{}", &p.created_at[..10.min(p.created_at.len())], picks.join("\n"), follows.join("\n"), p.sheet));
        }
        None => user.push("面试前的准备：（没有选速查卡，只对照题库）".into()),
    }
    let of_job = |q: &&Question| q.category != "突击题" || reviews.first().is_some_and(|r| q.job.as_ref().is_some_and(|j| j.id == r.job.id && j.created_at == r.job.created_at));
    let bank_lines: Vec<String> = bank.questions.iter().filter(of_job).map(|q| format!("- 【{}】{}", q.category, q.question)).collect();
    if !bank_lines.is_empty() {
        user.push(format!("题库里已有的题：\n{}", bank_lines.join("\n")));
    }
    if let Some(j) = job {
        user.push(jd_block(j));
    }
    user.extend(qbank::experiences(w));
    let many = if reviews.len() > 1 { "这几场面试（问的序号接着往下数）" } else { "这场面试" };
    let system = format!("你是用户的中文面试复盘助手，说明用中文。\n\n工作原则：\n{}\n\n任务：对照面试前的准备总结{many}，照规则里的「复盘卡」写出 summary、hits、misses、questions 和 rules。asked 写「问」的序号。\n仅输出 JSON：{{\"summary\":\"…\",\"hits\":[{{\"asked\":1,\"text\":\"…\"}}],\"misses\":[{{\"asked\":2,\"text\":\"…\"}}],\"questions\":[{{\"category\":\"简历题\",\"question\":\"…\",\"answer\":\"…\"}}],\"rules\":[{{\"use\":\"{}\",\"title\":\"…\",\"text\":\"…\",\"why\":\"…\"}}]}}\n\n规则：\n{}", seeker::model::principles(&[]), RULE_USES[0], qbank::rules_of(w, &SUMMARY_READS, true));
    (system, user.join("\n\n"))
}

pub fn parse_summary(v: &Value, reviews: &[&Review], bank: &Bank) -> Result<Analysis, String> {
    let asked: Vec<(String, usize)> = reviews.iter().flat_map(|r| (0..r.items.len()).map(|i| (r.id.clone(), i))).collect();
    let text = |x: &Value, k: &str| x[k].as_str().map(str::trim).unwrap_or("").to_string();
    let index = |x: &Value| x["asked"].as_u64().or_else(|| x["asked"].as_str().and_then(|s| s.trim_start_matches('问').trim().parse().ok())).map(|n| n as usize).filter(|n| *n >= 1 && *n <= asked.len()).map(|n| n - 1);
    let matched = |k: &str| -> Vec<Matched> {
        let mut out: Vec<Matched> = vec![];
        let mut seen = vec![];
        for m in v[k].as_array().into_iter().flatten() {
            if let Some(n) = index(m).filter(|n| !seen.contains(n)) {
                seen.push(n);
                out.push(Matched { review: asked[n].0.clone(), asked: asked[n].1, text: text(m, "text") });
            }
        }
        out
    };
    let have: Vec<&str> = bank.questions.iter().map(|q| q.question.trim()).collect();
    let questions: Vec<Follow> = v["questions"].as_array().into_iter().flatten().map(|f| Follow { about: String::new(), category: Some(text(f, "category")).filter(|c| CATEGORIES.contains(&c.as_str())).unwrap_or_else(|| "突击题".into()), question: text(f, "question"), answer: text(f, "answer"), saved: None }).filter(|f| !f.question.is_empty() && !have.contains(&f.question.as_str())).take(FOLLOW_MAX).collect();
    let rules: Vec<RuleIdea> = v["rules"].as_array().into_iter().flatten().map(|x| RuleIdea { use_: text(x, "use"), title: text(x, "title"), text: text(x, "text"), why: text(x, "why"), adopted: None }).filter(|x| RULE_USES.contains(&x.use_.as_str()) && !x.title.is_empty() && !x.text.is_empty()).take(2).collect();
    let a = Analysis { model: String::new(), prep: None, summary: text(v, "summary"), hits: matched("hits"), misses: matched("misses"), questions, rules };
    if a.summary.is_empty() && a.hits.is_empty() && a.misses.is_empty() {
        return Err("模型没有给出总结，请重试".into());
    }
    Ok(a)
}

fn mock_prep(bank: &Bank, job: &JobSnapshot) -> Value {
    let of = |cat: &'static str| bank.questions.iter().filter(|q| q.category == cat && (cat != "突击题" || q.job.as_ref().is_some_and(|j| j.id == job.id))).map(|q| q.id.as_str()).collect::<Vec<_>>();
    let ids = [of("通用题"), of("简历题"), of("突击题")].concat();
    let sheet = format!("# {} 速查卡（模拟）\n\n## 岗位要点\n- （模拟）JD 第一条 → 我的证据（待补）\n\n## 自我介绍\n- 一句话：我是谁、做过什么\n\n## 经历速记\n- 柚子笔记｜本人负责全部代码｜（待补：数据）\n\n## 高频题\n{}\n\n## 可能的追问\n- 指标怎么算的？ → （待补）\n\n## 反问\n- 团队现在最想解决的问题是什么？", jd_block(job).lines().next().unwrap_or(""), bank.questions.iter().filter(|q| ids.contains(&q.id.as_str())).take(4).map(|q| format!("- {}", q.question)).collect::<Vec<_>>().join("\n"));
    json!({ "advice": "（模拟模型）先把自我介绍和简历题背熟。", "questions": ids, "follows": [{ "about": "简历上的项目", "category": "简历题", "question": "（模拟追问）这个项目里哪一步是你本人做的？", "answer": "（模拟）我本人负责……（待补：具体步骤）" }], "sheet": sheet })
}

fn mock_summary(reviews: &[&Review]) -> Value {
    let n: usize = reviews.iter().map(|r| r.items.len()).sum();
    let hits: Vec<Value> = (0..n).step_by(2).map(|i| json!({ "asked": i + 1, "text": "（模拟）准备里有这道题" })).collect();
    let misses: Vec<Value> = (1..n).step_by(2).map(|i| json!({ "asked": i + 1, "text": "（模拟）没准备到：数据怎么来的讲不清" })).collect();
    json!({ "summary": "（模拟模型）大部分问题准备到了，追问细节时卡住。", "hits": hits, "misses": misses, "questions": [{ "category": "突击题", "question": "（模拟）你们的数据是怎么来的？", "answer": "（待补：数据来源）" }], "rules": [{ "use": "复盘", "title": "（模拟）先说数据口径", "text": "被问到数字时先说在什么数据上、怎么算的。", "why": "这次两次被追问口径" }] })
}

fn workspace(state: &AppState) -> Result<(Workspace, Bank, Options), String> {
    state.picky.with_db(|c| Ok((seeker::read(c)?, read(c)?, qbank::options(c)?)))
}

#[tauri::command]
pub async fn picky_rater_interviews(state: tauri::State<'_, AppState>) -> Result<Interviews, String> {
    state.picky.with_db(|c| read_all(c))
}

/// 准备面试: Rater's model, then the prep is kept (one undo step).
#[tauri::command]
pub async fn picky_rater_prep(app: AppHandle, ask: PrepAsk) -> Result<Interviews, String> {
    let state = app.state::<AppState>();
    let (w, bank, opts) = workspace(&state)?;
    let (model, level) = qbank::chosen_model(&w, &opts, "prep")?;
    if !LANGS.contains(&ask.lang.as_str()) {
        return Err("面试语言只能是中文或 English".into());
    }
    let (job, employer, found) = qbank::job_context(&app, Some(ask.job), model.starts_with("cloud:")).await?;
    let job = job.ok_or("岗位不存在，请重新选择")?;
    let used = resume_on_disk(&state, &w, &job, &ask.lang);
    let (system, user) = prep_prompt(&w, &bank, &ask, &job, used.as_ref(), &employer)?;
    let reply = qbank::call(&state, &model, &system, &user, 6144, &level).await?;
    let (questions, follows, sheet, advice) = parse_prep(&reply.json.unwrap_or_else(|| mock_prep(&bank, &job)), &bank)?;
    let resume = used.map(|u| u.name).unwrap_or_else(|| "（没有简历）".into());
    state.with_picky_undo("准备面试", |c| {
        let p = Prep { id: seeker::workflow::id()?, job: qbank::job_link(&job), resume, lang: ask.lang.clone(), form: ask.form.trim().into(), request: ask.request.trim().into(), questions, follows, sheet, advice, model: reply.model, note: [reply.note, found].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "), created_at: now(c)? };
        put_prep(c, &p)?;
        read_all(c)
    })
}

#[tauri::command]
pub async fn picky_rater_prep_save(state: tauri::State<'_, AppState>, prep: Prep) -> Result<Interviews, String> {
    state.with_picky_undo("改速查卡", |c| {
        save_prep(c, &prep)?;
        read_all(c)
    })
}

#[tauri::command]
pub async fn picky_rater_prep_keep(state: tauri::State<'_, AppState>, id: String, index: usize) -> Result<Interviews, String> {
    state.with_picky_undo("追问存进题库", |c| {
        keep_follow(c, &id, index)?;
        read_all(c)
    })
}

#[tauri::command]
pub async fn picky_rater_review_save(state: tauri::State<'_, AppState>, review: Review, job_id: Option<i64>) -> Result<Interviews, String> {
    let job = match (review.id.is_empty(), job_id) {
        (true, Some(id)) => Some(qbank::job_link(&seeker::output::job(&state, id)?)),
        _ => None,
    };
    state.with_picky_undo(if review.id.is_empty() { "记复盘" } else { "改复盘" }, |c| {
        save_review(c, &review, job)?;
        read_all(c)
    })
}

/// 总结复盘: Rater's model holds some reviews of one job (oldest first) against a prep (none: the bank
/// only); the summary is kept (one undo step).
#[tauri::command]
pub async fn picky_rater_summarize(app: AppHandle, reviews: Vec<String>, prep: Option<String>) -> Result<Interviews, String> {
    let state = app.state::<AppState>();
    let (w, bank, opts) = workspace(&state)?;
    let (model, level) = qbank::chosen_model(&w, &opts, "summary")?;
    let all = state.picky.with_db(|c| read_all(c))?;
    let mut chosen: Vec<&Review> = reviews.iter().map(|id| all.reviews.iter().find(|r| &r.id == id).ok_or_else(|| "有复盘已经删除了".to_string())).collect::<Result<_, _>>()?;
    let first = chosen.first().ok_or("至少选一次复盘")?.job.clone();
    if chosen.iter().any(|r| r.job.id != first.id || r.job.created_at != first.created_at) {
        return Err("只能总结同一个岗位的复盘".into());
    }
    if chosen.iter().all(|r| r.items.is_empty()) {
        return Err("选的复盘里还没记问了什么".into());
    }
    chosen.sort_by(|a, b| (&a.date, &a.created_at).cmp(&(&b.date, &b.created_at)));
    chosen.dedup_by(|a, b| a.id == b.id);
    let prep = match &prep {
        Some(pid) => Some(all.preps.iter().find(|p| &p.id == pid).ok_or("这份准备已经删除了")?),
        None => None,
    };
    let job = seeker::output::job(&state, first.id).ok().filter(|j| j.created_at == first.created_at);
    let (system, user) = summary_prompt(&w, &bank, &chosen, prep, job.as_ref());
    let reply = qbank::call(&state, &model, &system, &user, 4096, &level).await?;
    let analysis = Analysis { model: reply.model, prep: prep.map(|p| p.id.clone()), ..parse_summary(&reply.json.unwrap_or_else(|| mock_summary(&chosen)), &chosen, &bank)? };
    let summary = Summary { job: first, prep: prep.map(|p| p.id.clone()), reviews: chosen.iter().map(|r| r.id.clone()).collect(), analysis, ..Default::default() };
    state.with_picky_undo("总结复盘", |c| {
        add_summary(c, summary)?;
        read_all(c)
    })
}

#[tauri::command]
pub async fn picky_rater_review_keep(state: tauri::State<'_, AppState>, id: String, index: usize) -> Result<Interviews, String> {
    state.with_picky_undo("复盘的题存进题库", |c| {
        keep_question(c, &id, index)?;
        read_all(c)
    })
}

#[tauri::command]
pub async fn picky_rater_review_rule(state: tauri::State<'_, AppState>, id: String, index: usize) -> Result<Interviews, String> {
    state.with_picky_undo("复盘的约定加进规则", |c| {
        adopt_rule(c, &id, index)?;
        read_all(c)
    })
}

#[tauri::command]
pub async fn picky_rater_interview_delete(state: tauri::State<'_, AppState>, id: String) -> Result<Interviews, String> {
    state.with_picky_undo("删除面试记录", |c| {
        remove(c, &id)?;
        read_all(c)
    })
}

/// 另存 Word: the default name and folder (the last one used, else 文稿).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordPlace {
    pub folder: String,
    pub name: String,
}

#[tauri::command]
pub async fn picky_rater_prep_word_place(state: tauri::State<'_, AppState>, id: String) -> Result<WordPlace, String> {
    state.picky.with_db(|c| {
        let p = get_prep(c, &id)?;
        let folder = pref_get::<String>(c, FOLDER)?.filter(|f| Path::new(f).is_dir()).unwrap_or_else(|| std::env::var("HOME").map(|h| format!("{h}/Documents")).unwrap_or_default());
        let day: String = c.query_row("SELECT strftime('%Y-%m-%d', 'now', 'localtime')", [], |r| r.get(0)).map_err(err)?;
        Ok(WordPlace { folder, name: crate::picky::work::clean_name(&format!("速查卡 {} {day}", p.job.label.replace(" · ", " "))) })
    })
}

/// The 速查卡 as a new .docx (「 (2)」… rather than over a file); the folder is remembered (not undone).
#[tauri::command]
pub async fn picky_rater_prep_word(state: tauri::State<'_, AppState>, id: String, folder: String, name: String) -> Result<String, String> {
    let sheet = state.picky.with_db(|c| get_prep(c, &id))?.sheet;
    let bytes = crate::picky::word::markdown_to_docx(&sheet, &[])?;
    let path = write_new(Path::new(&folder), &name, &bytes)?;
    state.picky.with_db(|c| pref_set(c, FOLDER, &folder))?;
    Ok(path)
}

/// Bytes to `<dir>/<name>.docx`, never over a file. The path written.
pub fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> Result<String, String> {
    if !dir.is_dir() {
        return Err("找不到这个文件夹".into());
    }
    let base = crate::picky::work::clean_name(name.trim().trim_end_matches(".docx"));
    for k in 1..100 {
        let path = dir.join(if k == 1 { format!("{base}.docx") } else { format!("{base} ({k}).docx") });
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(bytes).map_err(|e| format!("没能保存：{e}"))?;
                return Ok(path.display().to_string());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("没能保存：{e}")),
        }
    }
    Err("同名的文件太多了，请换个名字".into())
}

#[cfg(test)]
mod tests;
