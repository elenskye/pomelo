//! ASK: a cloud model sends me multiple-choice or yes/no questions as a card and waits (`pomelo_ask_user`, offered in
//! conversations only: no one answers a 定时任务, and a local model's window is small). The reply's task simply stays
//! suspended on `Gate`: nothing runs and no request is open while it waits. My answer comes back as the tool's result and
//! the loop goes on (chat.rs `answer_with_lookups`, where an answered round doesn't count as a lookup round).
//! - While it waits the conversation is busy as ever: the composer can't send, only the card answers (or 停止).
//! - Quitting ends the reply as 中断 and the card stays: answered then, the answer goes as a new message, and the model
//!   carries on from the conversation (what it had looked up may have to be looked up again). A new question in that
//!   conversation closes such a card.
//! - Saved with the run (`run_asks`), in the reply's history for later questions, in exports.

use crate::modules::common::err;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Mutex;
use tokio::sync::oneshot;

pub const TOOL: &str = "pomelo_ask_user";
pub const MAX_PER_REPLY: i64 = 5;
const MAX_QUESTIONS: usize = 4;
const MAX_OPTIONS: usize = 6;
/// How my answer starts when it reaches the model.
pub const ANSWER_HEAD: &str = "我的回答：";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
    /// single | multi | judge
    pub kind: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    #[serde(default)]
    pub picked: Vec<String>,
    /// 其他: what I wrote instead.
    #[serde(default)]
    pub other: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ask {
    pub id: i64,
    pub run_id: i64,
    pub questions: Vec<Question>,
    pub answers: Option<Vec<Answer>>,
    /// waiting | answered | cancelled
    pub state: String,
}

pub fn tool() -> Value {
    json!({ "type": "function", "function": {
        "name": TOOL,
        "description": "把选择题或判断题发给我，显示成卡片，等我回答后再继续。只在缺了我的决定就做不下去、而且查不到时才用（比如几种做法要我选一个，要我确认一个判断）；能自己定的、查得到的不要问，闲聊时也不要问。一次最多 4 题。调用后你就暂停，我回答了你再接着做。",
        "parameters": { "type": "object", "properties": { "questions": { "type": "array", "minItems": 1, "maxItems": MAX_QUESTIONS, "items": { "type": "object", "properties": {
            "question": { "type": "string", "description": "要问的话，一句说清，不超过 200 字" },
            "kind": { "type": "string", "enum": ["single", "multi", "judge"], "description": "single 单选，multi 多选，judge 判断题（是 / 否，不用写 options）" },
            "options": { "type": "array", "items": { "type": "string" }, "description": "选项，2 到 6 个，每个不超过 60 字。不用写「其他」，卡片自带" },
        }, "required": ["question", "kind"] } } }, "required": ["questions"] },
    } })
}

/// The questions a call asks; what's wrong with them goes back to the model to fix.
pub fn parse(arguments: &str) -> Result<Vec<Question>, String> {
    let v: Value = serde_json::from_str(arguments).map_err(|_| "参数不是合法的 JSON，请重新问。".to_string())?;
    let list = v["questions"].as_array().filter(|l| !l.is_empty()).ok_or("至少要有一个问题（questions）。")?;
    if list.len() > MAX_QUESTIONS {
        return Err(format!("一次最多问 {MAX_QUESTIONS} 题，请精简。"));
    }
    list.iter().map(|q| {
        let question = q["question"].as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or("每题都要写 question。")?;
        if question.chars().count() > 200 {
            return Err("问题太长了，不超过 200 字。".to_string());
        }
        let given: Vec<String> = q["options"].as_array().map_or(vec![], |o| o.iter().filter_map(|x| x.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
        let kind = q["kind"].as_str().unwrap_or(if given.is_empty() { "judge" } else { "single" });
        let options = match kind {
            "judge" => vec!["是".to_string(), "否".to_string()],
            "single" | "multi" => {
                let mut seen = std::collections::HashSet::new();
                if given.len() < 2 || given.len() > MAX_OPTIONS || !given.iter().all(|o| seen.insert(o.clone())) {
                    return Err(format!("选择题要有 2 到 {MAX_OPTIONS} 个不重复的选项。"));
                }
                if given.iter().any(|o| o.chars().count() > 60) {
                    return Err("选项太长了，每个不超过 60 字。".to_string());
                }
                given
            }
            other => return Err(format!("kind 只能是 single、multi 或 judge，不是 {other}。")),
        };
        Ok(Question { question: question.to_string(), kind: kind.to_string(), options })
    }).collect()
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Ask> {
    let bad = |i, e: serde_json::Error| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e));
    let questions: String = r.get(2)?;
    let answers: Option<String> = r.get(3)?;
    Ok(Ask {
        id: r.get(0)?,
        run_id: r.get(1)?,
        questions: serde_json::from_str(&questions).map_err(|e| bad(2, e))?,
        answers: answers.map(|a| serde_json::from_str(&a).map_err(|e| bad(3, e))).transpose()?,
        state: r.get(4)?,
    })
}

const COLS: &str = "id, run_id, questions, answers, state";

/// A new card of the run (not more than `MAX_PER_REPLY`: then the model must decide itself).
pub fn save(conn: &Connection, run_id: i64, questions: &[Question]) -> Result<Ask, String> {
    let n: i64 = conn.query_row("SELECT count(*) FROM run_asks WHERE run_id = ?1", [run_id], |r| r.get(0)).map_err(err)?;
    if n >= MAX_PER_REPLY {
        return Err(format!("这条回答已经问了 {n} 次，不能再问了。请自己判断，把你的假设写明。"));
    }
    conn.execute("INSERT INTO run_asks (run_id, questions) VALUES (?1, ?2)", params![run_id, serde_json::to_string(questions).map_err(err)?]).map_err(err)?;
    get(conn, conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> Result<Ask, String> {
    conn.query_row(&format!("SELECT {COLS} FROM run_asks WHERE id = ?1"), [id], row).optional().map_err(err)?.ok_or_else(|| "这个问题已经不在了".to_string())
}

pub fn of_run(conn: &Connection, run_id: i64) -> Result<Vec<Ask>, String> {
    conn.prepare(&format!("SELECT {COLS} FROM run_asks WHERE run_id = ?1 ORDER BY id")).map_err(err)?.query_map([run_id], row).map_err(err)?.collect::<Result<_, _>>().map_err(err)
}

pub fn of_message(conn: &Connection, message_id: i64) -> Result<Vec<Ask>, String> {
    let run: Option<i64> = conn.query_row("SELECT run_id FROM messages WHERE id = ?1", [message_id], |r| r.get(0)).optional().map_err(err)?.flatten();
    run.map_or(Ok(vec![]), |r| of_run(conn, r))
}

/// My answers, as the model gets them; each question must be answered, with what it offered or in my own words.
pub fn answer_text(questions: &[Question], answers: &[Answer]) -> Result<String, String> {
    if answers.len() != questions.len() {
        return Err("每个问题都要回答".into());
    }
    let mut out = vec![];
    for (q, a) in questions.iter().zip(answers) {
        let other = a.other.trim();
        if other.chars().count() > 1000 {
            return Err("写的内容太长了，不超过 1000 字".into());
        }
        if a.picked.is_empty() && other.is_empty() {
            return Err("每个问题都要回答".into());
        }
        if a.picked.iter().any(|p| !q.options.contains(p)) {
            return Err("选的不是卡片上的选项".into());
        }
        if q.kind != "multi" && a.picked.len() > 1 {
            return Err("这一题只能选一个".into());
        }
        let said = a.picked.iter().map(String::as_str).chain((!other.is_empty()).then_some(other)).collect::<Vec<_>>().join("、");
        out.push(format!("{}\n→ {said}", q.question));
    }
    Ok(format!("{ANSWER_HEAD}\n{}", out.join("\n")))
}

/// Records my answer once; the run it belongs to and what to tell the model.
pub fn record_answer(conn: &Connection, id: i64, answers: &[Answer]) -> Result<(i64, String), String> {
    let ask = get(conn, id)?;
    if ask.state != "waiting" {
        return Err("这个问题已经结束了".into());
    }
    let text = answer_text(&ask.questions, answers)?;
    conn.execute("UPDATE run_asks SET answers = ?1, state = 'answered', answered_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2 AND state = 'waiting'", params![serde_json::to_string(answers).map_err(err)?, id]).map_err(err)?;
    Ok((ask.run_id, text))
}

/// 停止: the reply's open card closes.
pub fn cancel_run(conn: &Connection, run_id: i64) -> Result<(), String> {
    conn.execute("UPDATE run_asks SET state = 'cancelled' WHERE run_id = ?1 AND state = 'waiting'", [run_id]).map(|_| ()).map_err(err)
}

/// A new question in the conversation: cards left by a reply that ended (the app quit) close. Only called when no reply
/// holds the conversation's slot.
pub fn cancel_in_session(conn: &Connection, session_id: i64) -> Result<(), String> {
    conn.execute("UPDATE run_asks SET state = 'cancelled' WHERE state = 'waiting' AND run_id IN (SELECT id FROM runs WHERE session_id = ?1)", [session_id]).map(|_| ()).map_err(err)
}

/// A reply as later questions see it: what it asked and I answered goes with it.
pub fn in_history(content: &str, asks: &[Ask]) -> String {
    let said: Vec<String> = asks.iter().filter_map(|a| a.answers.as_ref().and_then(|ans| answer_text(&a.questions, ans).ok())).collect();
    if said.is_empty() {
        return content.to_string();
    }
    format!("（这条回答中途问过你，{}）\n\n{content}", said.join("\n"))
}

/// A reply's questions in a Markdown export.
pub fn block(asks: &[Ask]) -> String {
    asks.iter().flat_map(|a| a.questions.iter().enumerate().map(move |(i, q)| {
        let said = a.answers.as_ref().and_then(|ans| ans.get(i)).map_or_else(|| "（没回答）".to_string(), |x| x.picked.iter().map(String::as_str).chain((!x.other.trim().is_empty()).then_some(x.other.trim())).collect::<Vec<_>>().join("、"));
        format!("- 问我：{}\n  答：{said}", q.question)
    })).collect::<Vec<_>>().join("\n")
}

/// Where a reply's task waits for my answer.
#[derive(Default)]
pub struct Gate(Mutex<Option<(i64, oneshot::Sender<String>)>>);

impl Gate {
    pub fn open(&self, id: i64) -> oneshot::Receiver<String> {
        let (tx, rx) = oneshot::channel();
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, tx));
        rx
    }
    pub fn waiting(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }
    /// False: nothing waits for this one (the reply ended).
    pub fn answer(&self, id: i64, text: String) -> bool {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if g.as_ref().is_some_and(|(open, _)| *open == id) {
            if let Some((_, tx)) = g.take() {
                return tx.send(text).is_ok();
            }
        }
        false
    }
}

/// The mock model's question, when mine says 「问我测试」.
pub fn mock_call() -> super::provider::ToolCall {
    let arguments = json!({ "questions": [
        { "question": "模拟：这件事想怎么做？", "kind": "single", "options": ["简单做", "做完整"] },
        { "question": "模拟：要顺便写进笔记吗？", "kind": "judge" },
    ] }).to_string();
    super::provider::ToolCall { id: "ask1".into(), name: TOOL.into(), arguments }
}

/// The mock model, after my answer.
pub fn mock_reply(answer: &str) -> String {
    format!("这是开发版的**模拟回答**，收到了你的回答，接着做。\n\n{}", answer.replace('\n', "\n\n"))
}

#[cfg(test)]
mod tests;
