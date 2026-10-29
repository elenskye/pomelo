//! The reply's confirmation card in picky.db (`cards`, `card_items`): proposals in order; an item the
//! words fit several records keeps them all (`options`) until one is picked. Confirming writes the
//! ticked items in one undoable step and records what got written (and an edited record's
//! fingerprint before, to tell 已撤销 later); cancelling writes nothing. Each card is decided once.
//! Rater's questions (the 题库 SKILLs, skills/qbank.rs; v0.7 阶段 ⑤) are items too: their input is the
//! question; confirming writes them into picky.db as their own undo step (a card with both writes
//! Pomelo's first, then Rater's); 已撤销 = the question is gone.
//! - Changing a pending item (v0.7 refine R4): each item has a 编号 for the model (`12-1`: run 12's
//!   first item). A write tool called with `replace` = that 编号 checks its new arguments like a new
//!   proposal and puts the result in the item's place (`replace`, `replace_rater`) — this reply's
//!   card or an earlier reply's on the conversation's shown path, while the user hasn't confirmed or
//!   cancelled it. The earlier ones are listed for the model with their 编号 (`pending_text`).

use super::ops::{self, Op, Written};
use super::{act, label_of, Field, Proposal};
use crate::modules::common::{err, Tx};
use crate::modules::todo;
use crate::picky::assistants::rater::{self, Question};
use crate::picky::data;
use crate::picky::skills::qbank;
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

/// A record the words fit (shown to pick from).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardOption {
    pub title: String,
    pub fields: Vec<Field>,
}

/// As stored: the option and what confirming it runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    title: String,
    fields: Vec<Field>,
    input: Op,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardItem {
    pub position: i64,
    pub tool: String,
    pub module: String,
    /// 「记一笔收支」「改任务」
    pub label: String,
    pub title: String,
    pub fields: Vec<Field>,
    /// Several records fit: pick one (after confirming: `chosen`, with its title and fields above).
    pub options: Option<Vec<CardOption>>,
    pub chosen: Option<i64>,
    /// Ticked when confirmed (pending: true).
    pub checked: bool,
    pub written: Option<Written>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub run_id: i64,
    /// pending | confirmed | cancelled
    pub state: String,
    pub items: Vec<CardItem>,
}

/// Confirming: a ticked item, and for one with options which record.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    pub position: i64,
    #[serde(default)]
    pub option: Option<i64>,
}

/// A proposal as an item's columns: title, fields, input, options.
fn columns(p: &Proposal) -> Result<(String, String, String, Option<String>), String> {
    Ok(match p.choices.as_slice() {
        [one] => (one.title.clone(), json(&one.fields)?, json(&one.op)?, None),
        many => {
            let stored: Vec<Stored> = many.iter().map(|c| Stored { title: c.title.clone(), fields: c.fields.clone(), input: c.op.clone() }).collect();
            (format!("{} 条待选", many.len()), "[]".to_string(), json(&many[0].op)?, Some(json(&stored)?))
        }
    })
}

/// An item's 编号 for the model: `12-1` = run 12's first item.
pub fn reference(run_id: i64, position: i64) -> String {
    format!("{run_id}-{}", position + 1)
}

/// A 编号 the model gave (`12-1`; a bare `2` = this reply's second): (run, position, the item's tool),
/// when it is an item of a pending card among `runs` (this conversation's shown path and this reply).
pub fn find_ref(conn: &Connection, runs: &[i64], this: i64, r: &str) -> Result<(i64, i64, String), String> {
    let r = r.trim().trim_start_matches("编号").trim();
    let parsed = match r.split_once('-') {
        Some((a, b)) => a.trim().parse::<i64>().ok().zip(b.trim().parse::<i64>().ok()),
        None => r.parse::<i64>().ok().map(|n| (this, n)),
    };
    let Some((run, n)) = parsed.filter(|(_, n)| *n >= 1) else { return Err(format!("编号「{r}」不对：照卡片列表或工具结果里的写，如 12-1")) };
    let state: Option<String> = if run == this || runs.contains(&run) { conn.query_row("SELECT state FROM cards WHERE run_id = ?1", [run], |x| x.get(0)).optional().map_err(err)? } else { None };
    match state.as_deref() {
        None => return Err(format!("这个对话里没有编号 {r} 的卡片")),
        Some("pending") => {}
        Some(_) => return Err(format!("编号 {r} 那张卡片用户已经确认或取消了，不能再改：要改就不写 replace，新提议一条")),
    }
    let tool: String = conn.query_row("SELECT tool FROM card_items WHERE run_id = ?1 AND position = ?2", params![run, n - 1], |x| x.get(0)).optional().map_err(err)?.ok_or_else(|| format!("卡片里没有编号 {r} 这一条"))?;
    Ok((run, n - 1, tool))
}

/// Puts a checked proposal in place of a pending card's item (nothing is written: the user confirms).
pub fn replace(conn: &Connection, run_id: i64, position: i64, p: &Proposal) -> Result<(), String> {
    let (title, fields, input, options) = columns(p)?;
    let n = conn
        .execute(
            "UPDATE card_items SET tool = ?3, module = ?4, title = ?5, fields = ?6, input = ?7, options = ?8, chosen = NULL WHERE run_id = ?1 AND position = ?2 AND (SELECT state FROM cards WHERE run_id = ?1) = 'pending'",
            params![run_id, position, p.tool, p.module, title, fields, input, options],
        )
        .map_err(err)?;
    if n == 0 { Err("那一条已经不能改了".into()) } else { Ok(()) }
}

/// A pending item's Rater question (the input of a 题库 item).
pub fn rater_item(conn: &Connection, run_id: i64, position: i64) -> Result<Question, String> {
    let input: String = conn.query_row("SELECT input FROM card_items WHERE run_id = ?1 AND position = ?2", params![run_id, position], |r| r.get(0)).optional().map_err(err)?.ok_or("卡片里没有这一条")?;
    serde_json::from_str(&input).map_err(err)
}

/// Puts one of Rater's questions in place of a pending card's 题库 item.
pub fn replace_rater(conn: &Connection, run_id: i64, position: i64, tool: &str, title: &str, fields: &[Field], question: &Question) -> Result<(), String> {
    qbank::category(tool).ok_or("没有这个工具")?;
    let n = conn
        .execute(
            "UPDATE card_items SET tool = ?3, module = ?4, title = ?5, fields = ?6, input = ?7, options = NULL, chosen = NULL WHERE run_id = ?1 AND position = ?2 AND (SELECT state FROM cards WHERE run_id = ?1) = 'pending'",
            params![run_id, position, tool, data::RATER, title, json(&fields)?, json(question)?],
        )
        .map_err(err)?;
    if n == 0 { Err("那一条已经不能改了".into()) } else { Ok(()) }
}

/// One line of an item for the model: `作者 甲 → 乙`, a long text clipped.
pub fn field_text(f: &Field) -> String {
    match &f.before {
        Some(b) if !f.long => format!("{} {b} → {}", f.label, f.after),
        _ if f.long => format!("{}：{}", f.label, f.after.chars().take(80).collect::<String>()),
        _ => format!("{} {}", f.label, f.after),
    }
}

/// The pending cards of `runs` (earlier replies on the conversation's shown path) for the system
/// message: each item this model may see (`allows`: hidden and cloud-refused modules aren't) with its
/// 编号 ("" when none).
pub fn pending_text(conn: &Connection, runs: &[i64], allows: impl Fn(&str) -> bool) -> Result<String, String> {
    let mut lines = vec![];
    for &run in runs {
        let Some(card) = card_of(conn, run)?.filter(|c| c.state == "pending") else { continue };
        for it in card.items.iter().filter(|it| allows(&it.module)) {
            let what = match &it.options {
                Some(o) => format!("找到 {} 条，等用户在卡片里选", o.len()),
                None => it.fields.iter().map(field_text).collect::<Vec<_>>().join("；"),
            };
            lines.push(format!("- 编号 {} · {} · {}：{what}", reference(run, it.position), it.label, it.title));
        }
    }
    if lines.is_empty() {
        return Ok(String::new());
    }
    Ok(format!("【还没确认的卡片】前面的回答放进卡片、用户还没点确认的：\n{}\n用户要改其中一条时，用原来的工具重新调用、加上 replace 写那一条的编号（参数写全，没变的照抄），柚儿园检查后把卡片里那一条换掉，不多出一条；题库的题写 replace 和 request，会按 request 重写那道题的答案。编号只用来改卡片，回答里不用提。", lines.join("\n")))
}

/// Adds a proposal to the reply's card; its 1-based number.
pub fn add(conn: &Connection, run_id: i64, p: &Proposal) -> Result<usize, String> {
    let (title, fields, input, options) = columns(p)?;
    let tx = Tx::begin(conn)?;
    tx.execute("INSERT OR IGNORE INTO cards (run_id) VALUES (?1)", [run_id]).map_err(err)?;
    let n: i64 = tx.query_row("SELECT count(*) FROM card_items WHERE run_id = ?1", [run_id], |r| r.get(0)).map_err(err)?;
    tx.execute(
        "INSERT INTO card_items (run_id, position, tool, module, title, fields, input, options) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![run_id, n, p.tool, p.module, title, fields, input, options],
    )
    .map_err(err)?;
    tx.commit()?;
    Ok(n as usize + 1)
}

/// Adds one of Rater's questions (new, or with its new answer) to the reply's card; its 1-based number.
pub fn add_rater(conn: &Connection, run_id: i64, tool: &str, title: &str, fields: &[Field], question: &Question) -> Result<usize, String> {
    qbank::category(tool).ok_or("没有这个工具")?;
    let tx = Tx::begin(conn)?;
    tx.execute("INSERT OR IGNORE INTO cards (run_id) VALUES (?1)", [run_id]).map_err(err)?;
    let n: i64 = tx.query_row("SELECT count(*) FROM card_items WHERE run_id = ?1", [run_id], |r| r.get(0)).map_err(err)?;
    tx.execute("INSERT INTO card_items (run_id, position, tool, module, title, fields, input) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![run_id, n, tool, data::RATER, title, json(&fields)?, json(question)?]).map_err(err)?;
    tx.commit()?;
    Ok(n as usize + 1)
}

/// 「新简历题」 / 「改答案」
fn rater_label(input: &str) -> String {
    match serde_json::from_str::<Question>(input) {
        Ok(q) if q.revision > 0 => "改答案".into(),
        Ok(q) => format!("新{}", q.category),
        Err(_) => "题目".into(),
    }
}

fn json<T: Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string(v).map_err(err)
}

type Row = (i64, String, String, String, String, bool, Option<String>, Option<i64>, Option<i64>, Option<String>, String, Option<String>, Option<i64>, Option<String>);

pub fn card_of(conn: &Connection, run_id: i64) -> Result<Option<Card>, String> {
    let Some(state) = conn.query_row("SELECT state FROM cards WHERE run_id = ?1", [run_id], |r| r.get::<_, String>(0)).optional().map_err(err)? else { return Ok(None) };
    let mut st = conn
        .prepare(
            "SELECT position, tool, module, title, fields, checked, record_kind, record_id, record_parent, record_day, input, options, chosen, record_before
             FROM card_items WHERE run_id = ?1 ORDER BY position",
        )
        .map_err(err)?;
    let rows: Vec<Row> = st
        .query_map([run_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let items = rows
        .into_iter()
        .map(|(position, tool, module, title, fields, checked, kind, id, parent, day, input, options, chosen, before)| {
            let written = match (kind, id) {
                // a question of Rater's: gone when it isn't in the bank any more (picky.db, this conn)
                (Some(kind), Some(id)) if kind == RATER_KIND => Some(Written { module: module.clone(), gone: !conn.query_row("SELECT 1 FROM rater_questions WHERE rowid = ?1", [id], |_| Ok(())).is_ok(), kind, id, parent, day, before }),
                (Some(kind), Some(id)) => Some(Written { module: module.clone(), kind, id, parent, day, gone: false, before }),
                _ => None,
            };
            let label = if qbank::category(&tool).is_some() { rater_label(&input) } else { label_of(&tool, &module, serde_json::from_str::<Op>(&input).ok().as_ref()) };
            let options = options.and_then(|o| serde_json::from_str::<Vec<Stored>>(&o).ok()).map(|v| v.into_iter().map(|s| CardOption { title: s.title, fields: s.fields }).collect());
            CardItem { position, label, tool, module, title, fields: serde_json::from_str(&fields).unwrap_or_default(), options, chosen, checked, written }
        })
        .collect();
    Ok(Some(Card { run_id, state, items }))
}

/// What a written Rater question is (its `record_id` is the question's rowid in picky.db).
pub const RATER_KIND: &str = "rater";

/// Marks written records that are gone, or (edits) back to how they were (pomelo.db; Rater's
/// questions are worked out in `card_of`).
pub fn mark_gone(pomelo: &Connection, card: &mut Card) {
    for w in card.items.iter_mut().filter_map(|it| it.written.as_mut()).filter(|w| w.kind != RATER_KIND) {
        let now = ops::fingerprint(pomelo, &w.kind, w.id);
        w.gone = now.is_none() || (w.before.is_some() && now == w.before);
    }
}

/// Confirms the ticked items (`picks`): written in one undoable step, then the card records it.
pub fn confirm(st: &AppState, run_id: i64, picks: &[Pick]) -> Result<Card, String> {
    let card = st.picky.with_db(|c| card_of(c, run_id))?.ok_or("找不到这张卡片")?;
    if card.state != "pending" {
        return Err("这张卡片已经处理过了".into());
    }
    if picks.is_empty() {
        return Err("至少勾上一条".into());
    }
    let stored: Vec<(i64, String, String, String, Option<String>)> = st.picky.with_db(|c| {
        let mut s = c.prepare("SELECT position, tool, module, input, options FROM card_items WHERE run_id = ?1 ORDER BY position").map_err(err)?;
        let rows = s.query_map([run_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(err)
    })?;
    let hidden = st.with_db(|c| Ok(data::hidden(c)))?;
    // (position, op, chosen option); Rater's questions apart
    let mut todo_list: Vec<(i64, Op, Option<(i64, Stored)>)> = vec![];
    let mut questions: Vec<(i64, Question)> = vec![];
    for pick in picks {
        let (_, tool, module, input, options) = stored.iter().find(|x| x.0 == pick.position).ok_or("卡片里没有这一条")?;
        if hidden.contains(module) {
            return Err(format!("{}已在模块坞里隐藏，不能记", data::MODULES.iter().find(|m| m.key == module).map_or(if module == data::RATER { "Rater" } else { "这个模块" }, |m| m.label)));
        }
        if qbank::category(tool).is_some() {
            questions.push((pick.position, serde_json::from_str(input).map_err(err)?));
            continue;
        }
        act(tool).ok_or("没有这个工具")?;
        match options {
            Some(o) => {
                let list: Vec<Stored> = serde_json::from_str(o).map_err(err)?;
                let k = pick.option.ok_or_else(|| format!("第 {} 条还没选是哪一条", pick.position + 1))?;
                let s = list.into_iter().nth(k as usize).ok_or("选的那条不在卡片里")?;
                todo_list.push((pick.position, s.input.clone(), Some((k, s))));
            }
            None => todo_list.push((pick.position, serde_json::from_str(input).map_err(err)?, None)),
        }
    }
    let label = match todo_list.as_slice() {
        [(pos, op, _)] => {
            let (tool, module) = stored.iter().find(|x| x.0 == *pos).map(|x| (x.1.as_str(), x.2.as_str())).unwrap_or_default();
            format!("小拾：{}", label_of(tool, module, Some(op)))
        }
        many => format!("小拾：卡片里的 {} 条", many.len()),
    };
    let written: Vec<Written> = if todo_list.is_empty() { vec![] } else { st.with_undo(&label, |c| {
        let today = todo::today(c)?;
        let tx = Tx::begin(c)?;
        let mut out = vec![];
        for (_, op, _) in &todo_list {
            let before = op.target().and_then(|(k, id)| ops::fingerprint(&tx, k, id));
            let mut w = ops::apply(&tx, op, &today)?;
            w.before = before;
            out.push(w);
        }
        tx.commit()?;
        Ok(out)
    })? };
    let rater_written: Vec<(i64, i64)> = if questions.is_empty() {
        vec![]
    } else {
        let label = match questions.as_slice() {
            [(_, q)] if q.revision > 0 => "小拾：改答案".to_string(),
            [(_, q)] => format!("小拾：新{}", q.category),
            many => format!("小拾：{} 道题", many.len()),
        };
        st.with_picky_undo(&label, |c| {
            let tx = Tx::begin(c)?;
            let mut out = vec![];
            for (pos, q) in &questions {
                rater::put(&tx, q)?;
                out.push((*pos, tx.query_row("SELECT rowid FROM rater_questions WHERE id = ?1", [&q.id], |r| r.get(0)).map_err(err)?));
            }
            tx.commit()?;
            Ok(out)
        })?
    };
    st.picky.with_db(|c| {
        let tx = Tx::begin(c)?;
        tx.execute(&format!("UPDATE cards SET state = 'confirmed', decided_at = {} WHERE run_id = ?1", crate::picky::store::NOW), [run_id]).map_err(err)?;
        tx.execute("UPDATE card_items SET checked = 0 WHERE run_id = ?1", [run_id]).map_err(err)?;
        for ((pos, _, chosen), w) in todo_list.iter().zip(&written) {
            tx.execute(
                "UPDATE card_items SET checked = 1, record_kind = ?3, record_id = ?4, record_parent = ?5, record_day = ?6, record_before = ?7 WHERE run_id = ?1 AND position = ?2",
                params![run_id, pos, w.kind, w.id, w.parent, w.day, w.before],
            )
            .map_err(err)?;
            if let Some((k, s)) = chosen {
                tx.execute(
                    "UPDATE card_items SET chosen = ?3, title = ?4, fields = ?5 WHERE run_id = ?1 AND position = ?2",
                    params![run_id, pos, k, s.title, serde_json::to_string(&s.fields).map_err(err)?],
                )
                .map_err(err)?;
            }
        }
        for (pos, rowid) in &rater_written {
            tx.execute("UPDATE card_items SET checked = 1, record_kind = ?3, record_id = ?4 WHERE run_id = ?1 AND position = ?2", params![run_id, pos, RATER_KIND, rowid]).map_err(err)?;
        }
        tx.commit()?;
        card_of(c, run_id)?.ok_or_else(|| "找不到这张卡片".to_string())
    })
}

pub fn cancel(conn: &Connection, run_id: i64) -> Result<Card, String> {
    let n = conn.execute(&format!("UPDATE cards SET state = 'cancelled', decided_at = {} WHERE run_id = ?1 AND state = 'pending'", crate::picky::store::NOW), [run_id]).map_err(err)?;
    if n == 0 {
        return Err("这张卡片已经处理过了".into());
    }
    card_of(conn, run_id)?.ok_or_else(|| "找不到这张卡片".into())
}

// ---------- commands ----------

#[tauri::command]
pub fn picky_card_confirm(app: AppHandle, run_id: i64, picks: Vec<Pick>) -> Result<Card, String> {
    let st = app.state::<AppState>();
    let mut card = confirm(&st, run_id, &picks)?;
    let _ = st.with_db(|c| Ok(mark_gone(c, &mut card)));
    Ok(card)
}

#[tauri::command]
pub fn picky_card_cancel(state: State<AppState>, run_id: i64) -> Result<Card, String> {
    state.picky.with_db(|c| cancel(c, run_id))
}
