//! Rater (助手, v0.7 阶段 ④): the user's interview question bank (Chinese only). picky.db (migration
//! 0024): `rater_questions` (one row per question: JSON `Question`, revision) and
//! `rater_question_history` (every replaced or deleted version).
//! - A question: category 简历题 / 通用题 / 突击题, the question, the answer (the user's spoken draft),
//!   tags (「必背」…), and what it is about: the Seeker material (简历题), the 投石问路 job (突击题; id +
//!   created_at as Seeker keeps jobs, and the label it had), the question it was adapted from. No
//!   numbers, no A / B tiers; no duplicate or coverage checks (the user checks coverage).
//! - Length (`seconds`): the answer read aloud, 400 字 in 90 s; a CJK character is one 字, a Latin word
//!   or a number two. Over `LIMIT` seconds is flagged. Worked out on reading, never stored.
//! - Saving checks the revision the user started from (like Seeker's records); the replaced or
//!   deleted version goes to the history. Each save / delete is one undo step.
//! - Its rules (题库维护, 面试) are Seeker records (seeker/catalog.rs, `RULES`), imported with the rest of
//!   transfer/ (rules/qbank.md, rules/interview.md) and shown on Rater's 规则 page only.
//! - Data file (format 23): the whole bank replaces Rater's tables; an older file leaves it as it is.
//!   Format 24 adds the practice rounds (practice.rs, 阶段 ⑥), 25 the interviews (interview.rs, 阶段 ⑦);
//!   a file without them leaves them.
//! - 卡片练习 (阶段 ⑥): practice.rs. Each question's practice record (`Bank::practice`) is worked out
//!   from the rounds on reading.
//! - 小拾 reads it through the 「Seeker 资料」 skill (skills/seeker.rs) and adds to it through the three
//!   题库 SKILLs (阶段 ⑤, qbank.rs): proposals the user confirms.
pub mod interview;
pub mod practice;
pub mod qbank;
use crate::modules::common::{err, Tx};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tauri::State;

pub const CATEGORIES: [&str; 3] = ["简历题", "通用题", "突击题"];
/// Rater's rules: Seeker records of these categories (用途).
pub const RULES: [&str; 2] = ["题库维护", "面试"];
/// Answers longer than this (seconds) are flagged.
pub const LIMIT: i64 = 90;

/// A 投石问路 job as a question keeps it (the label it had when linked).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobLink {
    pub id: i64,
    pub created_at: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Question {
    pub id: String,
    pub category: String,
    pub question: String,
    pub answer: String,
    pub tags: Vec<String>,
    /// The Seeker material it is about (简历题).
    pub material: Option<String>,
    /// The job it is for (突击题).
    pub job: Option<JobLink>,
    /// The question it was adapted from (a 突击题 made from a 简历题 / 通用题).
    pub from: Option<String>,
    pub revision: i64,
}

impl Default for Question {
    fn default() -> Self {
        Self { id: String::new(), category: "通用题".into(), question: String::new(), answer: String::new(), tags: vec![], material: None, job: None, from: None, revision: 0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub id: i64,
    pub question_id: String,
    pub body: Question,
    pub saved_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bank {
    pub questions: Vec<Question>,
    pub revisions: Vec<Revision>,
    /// The practice rounds: in the data file only (the page reads them through practice.rs); a file
    /// without them (before format 24) leaves them as they are.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rounds: Option<Vec<practice::Round>>,
    /// 面试 (准备, 复盘): in the data file only, like the rounds (from format 25).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interviews: Option<interview::Interviews>,
    /// Each question's length in seconds (worked out on reading; not in the data file).
    #[serde(skip_deserializing, skip_serializing_if = "BTreeMap::is_empty")]
    pub seconds: BTreeMap<String, i64>,
    /// Each practiced question's record (worked out from the rounds on reading; not in the data file).
    #[serde(skip_deserializing, skip_serializing_if = "BTreeMap::is_empty")]
    pub practice: BTreeMap<String, practice::Tally>,
}

impl Bank {
    pub fn len(&self) -> usize {
        self.questions.len() + self.rounds.as_ref().map_or(0, Vec::len) + self.interviews.as_ref().map_or(0, interview::Interviews::len)
    }
}

/// How long `text` takes to say: 400 字 in 90 s (a CJK character one 字, a Latin word or number two).
pub fn seconds(text: &str) -> i64 {
    let mut units = 0i64;
    let mut in_word = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            if !in_word {
                units += 2;
                in_word = true;
            }
            continue;
        }
        // 「isn't」「state-of-the-art」「0.92」 are one word
        in_word = in_word && matches!(c, '\'' | '’' | '-' | '.');
        if is_cjk(c) {
            units += 1;
        }
    }
    (units * 90 + 399) / 400
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

fn json<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, String> {
    serde_json::from_str(s).map_err(err)
}

pub fn read(c: &Connection) -> Result<Bank, String> {
    let mut st = c.prepare("SELECT body FROM rater_questions ORDER BY rowid").map_err(err)?;
    let questions: Vec<Question> = st.query_map([], |r| r.get::<_, String>(0)).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?.iter().map(|b| json(b)).collect::<Result<_, _>>()?;
    let mut st = c.prepare("SELECT id, question_id, body, saved_at FROM rater_question_history ORDER BY id DESC").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?, r.get(3)?))).map_err(err)?.collect::<Result<Vec<(i64, String, String, String)>, _>>().map_err(err)?;
    let revisions = rows.into_iter().map(|(id, question_id, body, saved_at)| Ok(Revision { id, question_id, body: json(&body)?, saved_at })).collect::<Result<_, String>>()?;
    let seconds = questions.iter().map(|q| (q.id.clone(), seconds(&q.answer))).collect();
    let practice = practice::tallies(&practice::rounds(c)?);
    Ok(Bank { questions, revisions, rounds: None, interviews: None, seconds, practice })
}

/// The bank as the data file has it: with the rounds, without what is worked out on reading.
pub fn export(c: &Connection) -> Result<Bank, String> {
    Ok(Bank { rounds: Some(practice::rounds(c)?), interviews: Some(interview::read_all(c)?), seconds: BTreeMap::new(), practice: BTreeMap::new(), ..read(c)? })
}

pub fn validate(q: &Question) -> Result<(), String> {
    if q.id.is_empty() {
        return Err("题目没有编号".into());
    }
    if q.question.trim().is_empty() {
        return Err("请填写问题".into());
    }
    if !CATEGORIES.contains(&q.category.as_str()) {
        return Err("未知的题目类别".into());
    }
    if q.tags.iter().any(|t| t.trim().is_empty() || t.chars().count() > 20) {
        return Err("标签不能是空的，也不能超过 20 个字".into());
    }
    Ok(())
}

/// New (revision 0) or the next version of the one the user opened (same revision).
pub fn put(c: &Connection, input: &Question) -> Result<(), String> {
    let mut q = input.clone();
    q.question = q.question.trim().into();
    q.tags = q.tags.iter().map(|t| t.trim().to_string()).fold(vec![], |mut out, t| {
        if !out.contains(&t) {
            out.push(t);
        }
        out
    });
    validate(&q)?;
    if q.from.as_deref() == Some(q.id.as_str()) {
        return Err("题目不能改自它自己".into());
    }
    let old: Option<String> = c.query_row("SELECT body FROM rater_questions WHERE id = ?1", [&q.id], |r| r.get(0)).optional().map_err(err)?;
    match old {
        Some(body) => {
            let prior: Question = json(&body)?;
            if prior.revision != input.revision {
                return Err("这道题刚刚改过，请重新打开".into());
            }
            if prior == q {
                return Ok(());
            }
            c.execute("INSERT INTO rater_question_history (question_id, body) VALUES (?1, ?2)", params![prior.id, body]).map_err(err)?;
            q.revision = prior.revision + 1;
        }
        None if input.revision != 0 => return Err("这道题已经删除了".into()),
        None => q.revision = 1,
    }
    c.execute("INSERT INTO rater_questions (id, body, revision) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET body = excluded.body, revision = excluded.revision", params![q.id, serde_json::to_string(&q).map_err(err)?, q.revision]).map_err(err)?;
    Ok(())
}

/// Removes a question; its last version goes to the history (questions adapted from it keep the link
/// as text: the history still has it).
pub fn delete(c: &Connection, id: &str) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    let body: String = tx.query_row("SELECT body FROM rater_questions WHERE id = ?1", [id], |r| r.get(0)).map_err(|_| "这道题已经删除了".to_string())?;
    tx.execute("INSERT INTO rater_question_history (question_id, body) VALUES (?1, ?2)", params![id, body]).map_err(err)?;
    tx.execute("DELETE FROM rater_questions WHERE id = ?1", [id]).map_err(err)?;
    tx.commit()
}

/// The data-file import (inside its transaction): the file's bank replaces Rater's tables.
pub fn replace(c: &Connection, bank: &Bank) -> Result<(), String> {
    c.execute_batch("DELETE FROM rater_question_history; DELETE FROM rater_questions;").map_err(err)?;
    for q in &bank.questions {
        validate(q)?;
        c.execute("INSERT INTO rater_questions (id, body, revision) VALUES (?1, ?2, ?3)", params![q.id, serde_json::to_string(q).map_err(err)?, q.revision]).map_err(|_| "Rater：题目重复了".to_string())?;
    }
    for r in &bank.revisions {
        c.execute("INSERT INTO rater_question_history (id, question_id, body, saved_at) VALUES (?1, ?2, ?3, ?4)", params![r.id, r.question_id, serde_json::to_string(&r.body).map_err(err)?, r.saved_at]).map_err(err)?;
    }
    if let Some(rounds) = &bank.rounds {
        practice::replace(c, rounds)?;
    }
    if let Some(all) = &bank.interviews {
        interview::replace(c, all)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn picky_rater(state: State<'_, AppState>) -> Result<Bank, String> {
    state.picky.with_db(|c| read(c))
}

#[tauri::command]
pub async fn picky_rater_save(state: State<'_, AppState>, question: Question) -> Result<Bank, String> {
    state.with_picky_undo(if question.revision == 0 { "新增题目" } else { "保存题目" }, |c| {
        put(c, &question)?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_rater_delete(state: State<'_, AppState>, id: String) -> Result<Bank, String> {
    state.with_picky_undo("删除题目", |c| {
        delete(c, &id)?;
        read(c)
    })
}

/// The editor's live length (the same rule as the bank's).
#[tauri::command]
pub fn picky_rater_seconds(text: String) -> i64 {
    seconds(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::store::tests::memory_db;

    fn q(id: &str, question: &str, answer: &str) -> Question {
        Question { id: id.into(), category: "简历题".into(), question: question.into(), answer: answer.into(), ..Default::default() }
    }

    #[test]
    fn answers_are_timed_at_400_zi_in_90_seconds() {
        assert_eq!(seconds(""), 0);
        assert_eq!(seconds(&"字".repeat(400)), 90);
        assert_eq!(seconds(&"字".repeat(401)), 91);
        // a Latin word or a number counts two; punctuation and spaces nothing
        assert_eq!(seconds("用 LangGraph 做了 3 个节点，准确率 0.92。"), 4); // 9 字 + 3 words × 2 = 15 字
        assert_eq!(seconds("state-of-the-art isn't"), 1); // 2 words
        assert_eq!(seconds(&"word ".repeat(200)), 90);
    }

    #[test]
    fn a_question_saves_with_its_revision_keeps_history_and_undoes_in_one_step() {
        let c = memory_db();
        let first = q("a", " 你在项目里负责什么？ ", "我负责调参。");
        put(&c, &Question { tags: vec!["必背".into(), " 必背 ".into()], ..first.clone() }).unwrap();
        let bank = read(&c).unwrap();
        let saved = &bank.questions[0];
        assert_eq!((saved.question.as_str(), saved.tags.clone(), saved.revision, bank.seconds["a"]), ("你在项目里负责什么？", vec!["必背".to_string()], 1, 2));
        // the same again: nothing; an edit from the opened revision: the old one to the history
        put(&c, saved).unwrap();
        assert!(read(&c).unwrap().revisions.is_empty());
        put(&c, &Question { answer: "我负责调参和类别均衡。".into(), ..saved.clone() }).unwrap();
        let bank = read(&c).unwrap();
        assert_eq!((bank.questions[0].revision, bank.revisions.len(), bank.revisions[0].body.answer.as_str()), (2, 1, "我负责调参。"));
        // stale, unknown category, empty question, adapted from itself, deleted: refused
        assert!(put(&c, saved).unwrap_err().contains("改过"));
        assert!(put(&c, &Question { category: "技能题".into(), ..bank.questions[0].clone() }).is_err());
        assert!(put(&c, &Question { question: " ".into(), ..bank.questions[0].clone() }).is_err());
        assert!(put(&c, &Question { from: Some("a".into()), ..bank.questions[0].clone() }).is_err());
        assert!(put(&c, &Question { revision: 3, ..q("gone", "x", "") }).unwrap_err().contains("删除"));

        // a delete is one undo step; the history keeps its last version
        let mut h = crate::undo::History::default();
        crate::undo::tests::step(&c, &mut h, "删除题目", |c| delete(c, "a"));
        let bank = read(&c).unwrap();
        assert!(bank.questions.is_empty() && bank.revisions.len() == 2);
        assert_eq!(crate::undo::undo(&c, &mut h).unwrap().as_deref(), Some("删除题目"));
        assert_eq!(read(&c).unwrap().questions[0].revision, 2);
    }

    #[test]
    fn the_bank_goes_through_the_data_file_whole() {
        let a = memory_db();
        put(&a, &q("a", "自我介绍（1 分钟）", "我是……")).unwrap();
        let job = JobLink { id: 4, created_at: "2026-10-01T00:00:00.000Z".into(), label: "柚子银行 · 数据分析师".into() };
        put(&a, &Question { category: "突击题".into(), job: Some(job), from: Some("a".into()), tags: vec!["必背".into()], ..q("b", "为什么选柚子银行？", "因为……") }).unwrap();
        put(&a, &Question { answer: "我是林小柚。".into(), revision: 1, ..q("a", "自我介绍（1 分钟）", "") }).unwrap();
        let rec = crate::picky::transfer::export(&a).unwrap();
        let json = serde_json::to_value(&rec).unwrap();
        assert!(json["rater"].get("seconds").is_none());
        assert_eq!((json["rater"]["questions"].as_array().unwrap().len(), json["rater"]["revisions"].as_array().unwrap().len()), (2, 1));
        let b = memory_db();
        crate::picky::transfer::replace(&b, &serde_json::from_value(json).unwrap()).unwrap();
        assert_eq!(crate::picky::transfer::export(&b).unwrap(), rec);
        // a file from before Rater leaves the bank as it is
        crate::picky::transfer::replace(&b, &crate::picky::transfer::PickyRecord { rater: None, ..rec.clone() }).unwrap();
        assert_eq!(read(&b).unwrap().questions.len(), 2);
    }
}
