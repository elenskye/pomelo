//! The reply's confirmation card in picky.db (`cards`, `card_items`): proposals in order; an item the
//! words fit several records keeps them all (`options`) until one is picked. Confirming writes the
//! ticked items in one undoable step and records what got written (and an edited record's
//! fingerprint before, to tell 已撤销 later); cancelling writes nothing. Each card is decided once.

use super::ops::{self, Op, Written};
use super::{act, label_of, Field, Proposal};
use crate::modules::common::{err, Tx};
use crate::modules::todo;
use crate::picky::data;
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

/// Adds a proposal to the reply's card; its 1-based number.
pub fn add(conn: &Connection, run_id: i64, p: &Proposal) -> Result<usize, String> {
    let (title, fields, input, options) = match p.choices.as_slice() {
        [one] => (one.title.clone(), json(&one.fields)?, json(&one.op)?, None),
        many => {
            let stored: Vec<Stored> = many.iter().map(|c| Stored { title: c.title.clone(), fields: c.fields.clone(), input: c.op.clone() }).collect();
            (format!("{} 条待选", many.len()), "[]".to_string(), json(&many[0].op)?, Some(json(&stored)?))
        }
    };
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
                (Some(kind), Some(id)) => Some(Written { module: module.clone(), kind, id, parent, day, gone: false, before }),
                _ => None,
            };
            let op: Option<Op> = serde_json::from_str(&input).ok();
            let options = options.and_then(|o| serde_json::from_str::<Vec<Stored>>(&o).ok()).map(|v| v.into_iter().map(|s| CardOption { title: s.title, fields: s.fields }).collect());
            CardItem { position, label: label_of(&tool, &module, op.as_ref()), tool, module, title, fields: serde_json::from_str(&fields).unwrap_or_default(), options, chosen, checked, written }
        })
        .collect();
    Ok(Some(Card { run_id, state, items }))
}

/// Marks written records that are gone, or (edits) back to how they were (pomelo.db).
pub fn mark_gone(pomelo: &Connection, card: &mut Card) {
    for w in card.items.iter_mut().filter_map(|it| it.written.as_mut()) {
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
    // (position, op, chosen option)
    let mut todo_list: Vec<(i64, Op, Option<(i64, Stored)>)> = vec![];
    for pick in picks {
        let (_, tool, module, input, options) = stored.iter().find(|x| x.0 == pick.position).ok_or("卡片里没有这一条")?;
        if hidden.contains(module) {
            return Err(format!("{}已在侧栏里隐藏，不能记", data::MODULES.iter().find(|m| m.key == module).map_or("这个模块", |m| m.label)));
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
    let written: Vec<Written> = st.with_undo(&label, |c| {
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
    })?;
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
