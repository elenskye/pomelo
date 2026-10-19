//! The one search entry (⌘K, conversation search, 念念不忘 and 小拾 use it from v0.4 phase 3):
//! a question, a scope and a way.
//! - Scope: the modules / kinds asked for (all when empty), minus modules hidden in 设置 → 侧栏,
//!   minus — for an answer going to a cloud model — modules not allowed there (picky/data.rs).
//!   芝麻开门 is never in the index.
//! - Words: every word must appear (text.rs); best FTS5 rank first, then the newest.
//! - Meaning: the question's vector against every allowed passage (vectors.rs), each record's
//!   best passage, cosine ≥ MIN_MEANING. Records changed since their vectors were made are filled
//!   first (one round); the rest are left out and `note` says so.
//! - Both: the two lists merged by rank (reciprocal rank fusion); of the first RERANK, those the
//!   rerank model is sure about (score ≥ SURE) go first by its score, the rest keep the merged
//!   order — for a short, indirect query (「找房子」) it scores everything near 0 and sorting by that
//!   noise would bury what meaning found (tried on the real model). Without it, the merged order.
//! Service problems never fail a search that can still answer by words: `note` explains.

use super::indexer::Search;
use super::sources::{self, SOURCES};
use super::text;
use crate::modules::common::err;
use crate::picky::embed::{self, Input};
use crate::picky::runtime::Svc;
use crate::picky::{data, rerank};
use crate::AppState;
use rusqlite::{params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{AppHandle, Manager};

pub const MIN_MEANING: f32 = 0.35;
const POOL: usize = 40;
const RERANK: usize = 30;
/// A rerank score it means (related records score ~0.9, unrelated ~0.001; indirect ones ~0.01).
const SURE: f32 = 0.1;
const RRF_K: f32 = 60.0;
const DEFAULT_LIMIT: usize = 20;
const SNIPPET: usize = 30;
const PASSAGE_CHARS: usize = 1200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Way {
    Words,
    Meaning,
    Both,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub text: String,
    pub way: Way,
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default)]
    pub kinds: Vec<String>,
    /// The results go to a cloud model.
    #[serde(default)]
    pub for_cloud: bool,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub module: String,
    pub kind: &'static str,
    pub id: i64,
    pub title: String,
    pub detail: Option<String>,
    pub parent: Option<i64>,
    pub day: Option<String>,
    pub snippet: String,
    pub score: f32,
    /// Found by words, by meaning or both.
    pub by: Way,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Answer {
    pub found: Vec<Found>,
    /// Why meaning search or reranking was left out / partial.
    pub note: Option<String>,
}

/// A doc as read back for a hit.
#[derive(Debug, Clone)]
struct Doc {
    id: i64,
    source: usize,
    module: String,
    ref_id: i64,
    title: String,
    detail: Option<String>,
    parent: Option<i64>,
    day: Option<String>,
    body: String,
}

/// Bits of the sources a search may return. A source of made modules is in when any could be; which
/// of them are left out is `excluded`.
pub fn allowed(modules: &[String], kinds: &[String], hidden: &[String], cloud_allowed: Option<&[String]>) -> u32 {
    let mut mask = 0u32;
    for (i, s) in SOURCES.iter().enumerate() {
        let module = s.module.to_string();
        let ok = (s.dynamic() || !hidden.contains(&module))
            && (s.dynamic() || cloud_allowed.is_none_or(|a| s.module == "picky" || a.contains(&module)))
            && (modules.is_empty() || modules.iter().any(|m| s.owns(m) || s.module.strip_suffix('/') == Some(m.as_str())))
            && (kinds.is_empty() || kinds.iter().any(|k| k == s.kind));
        if ok {
            mask |= 1 << i;
        }
    }
    mask
}

/// Made modules (keys) a search leaves out: hidden ones, ones this cloud model may not get, ones not
/// asked for (asking for "list" / "progress" means all made from it).
pub fn excluded(customs: &[String], modules: &[String], hidden: &[String], cloud_allowed: Option<&[String]>) -> Vec<String> {
    let asked = |k: &String| modules.is_empty() || modules.iter().any(|m| m == k || k.split('/').next() == Some(m.as_str()));
    customs.iter().filter(|k| hidden.contains(k) || cloud_allowed.is_some_and(|a| !a.contains(k)) || !asked(k)).cloned().collect()
}

fn source_filter(mask: u32, exclude: &[String]) -> String {
    // keys are "list/<id>": nothing to escape
    let out = if exclude.is_empty() { String::new() } else { format!(" AND d.module NOT IN ({})", exclude.iter().map(|k| format!("'{}'", k.replace('\'', ""))).collect::<Vec<_>>().join(",")) };
    if mask == (1u32 << SOURCES.len()) - 1 {
        return out;
    }
    // kinds are unique among sources
    let pairs: Vec<String> = SOURCES.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, s)| format!("'{}'", s.kind)).collect();
    if pairs.is_empty() {
        " AND 0".into()
    } else {
        format!(" AND d.kind IN ({}){out}", pairs.join(","))
    }
}

const DOC_COLS: &str = "d.id, d.module, d.kind, d.ref_id, d.title, d.detail, d.parent, d.day, d.body";

fn doc(r: &rusqlite::Row) -> rusqlite::Result<Option<Doc>> {
    let (module, kind): (String, String) = (r.get(1)?, r.get(2)?);
    let Some((source, _)) = sources::source(&module, &kind) else { return Ok(None) };
    Ok(Some(Doc { id: r.get(0)?, source, module, ref_id: r.get(3)?, title: r.get(4)?, detail: r.get(5)?, parent: r.get(6)?, day: r.get(7)?, body: r.get(8)? }))
}

/// Docs matching every word, best first: (doc, rank score; higher is better).
fn by_words(c: &Connection, q: &str, mask: u32, exclude: &[String], limit: usize) -> Result<Vec<(Doc, f32)>, String> {
    let Some(m) = text::fts_query(q) else { return Ok(vec![]) };
    let sql = format!(
        "SELECT {DOC_COLS}, bm25(docs_fts) FROM docs_fts JOIN docs d ON d.id = docs_fts.rowid
         WHERE docs_fts MATCH ?1{} ORDER BY bm25(docs_fts), d.at DESC LIMIT {limit}",
        source_filter(mask, exclude)
    );
    let mut st = c.prepare(&sql).map_err(err)?;
    let rows = st.query_map([m], |r| Ok((doc(r)?, -(r.get::<_, f64>(9)? as f32)))).map_err(err)?;
    let mut out = vec![];
    for row in rows {
        if let (Some(d), score) = row.map_err(err)? {
            out.push((d, score));
        }
    }
    Ok(out)
}

/// Word hits as results (also what tests drive 小拾's search with).
pub fn found_by_words(c: &Connection, q: &str, mask: u32, exclude: &[String], limit: usize) -> Result<Vec<Found>, String> {
    let first = text::first_word(q);
    Ok(by_words(c, q, mask, exclude, limit)?.into_iter().map(|(d, score)| found(&d, crate::picky::find::snippet(&d.body, &first, SNIPPET), score, Way::Words)).collect())
}

fn docs_by_id(c: &Connection, ids: &[i64]) -> Result<HashMap<i64, Doc>, String> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!("SELECT {DOC_COLS} FROM docs d WHERE d.id IN ({})", vec!["?"; ids.len()].join(","));
    let mut st = c.prepare(&sql).map_err(err)?;
    let rows = st.query_map(params_from_iter(ids.iter()), doc).map_err(err)?;
    let mut out = HashMap::new();
    for r in rows {
        if let Some(d) = r.map_err(err)? {
            out.insert(d.id, d);
        }
    }
    Ok(out)
}

/// The passage a meaning hit matched (or the one holding the first word), for snippets and reranking.
fn passage(d: &Doc, ordinal: Option<u16>, first_word: &str) -> String {
    let list = text::passages(&d.title, &d.body);
    let pick = match ordinal {
        Some(o) => list.get(o as usize),
        None => list.iter().find(|p| p.to_lowercase().contains(first_word)).or(list.first()),
    };
    pick.cloned().unwrap_or_default().chars().take(PASSAGE_CHARS).collect()
}

fn found(d: &Doc, snippet: String, score: f32, by: Way) -> Found {
    let s = &SOURCES[d.source];
    Found { module: d.module.clone(), kind: s.kind, id: d.ref_id, title: d.title.clone(), detail: d.detail.clone(), parent: d.parent, day: d.day.clone(), snippet, score, by }
}

fn flat_snippet(p: &str) -> String {
    let flat: String = p.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s: String = flat.chars().take(SNIPPET * 2 + 10).collect();
    if flat.chars().count() > SNIPPET * 2 + 10 {
        s.push('…');
    }
    s
}

/// Meaning hits: (doc, best passage ordinal, cosine). Errors when the embedding service can't help.
async fn by_meaning(app: &AppHandle, q: &str, mask: u32, exclude: &[String], limit: usize) -> Result<(Vec<(Doc, u16, f32)>, Option<String>), String> {
    let st = app.state::<AppState>();
    let s: &Search = &st.search;
    let model = st.picky.runtime.ensure(Svc::Embedding).await?;
    let before = s.status();
    // the indexer keeps the counts; with nothing missing the (full-scan) pending query is skipped
    let filled = if before.done < before.records { s.fill(app, &model).await } else { Ok(0) };
    Search::ensure_loaded(app, &model.key).await?;
    let qv = embed::embed(&st.picky.runtime, &model, &[q.to_string()], Input::RecordQuery).await.map_err(String::from)?.pop().ok_or("向量服务没有返回问题的向量。")?;
    let hits = s.with_vectors(|v| v.map(|v| v.search(&qv, mask, limit, MIN_MEANING)).unwrap_or_default());
    let docs = s.with_db(|c| docs_by_id(c, &hits.iter().map(|h| h.doc).collect::<Vec<_>>()))?;
    let status = s.status();
    let missing = status.records - status.done;
    let note = match filled {
        Err(e) => Some(format!("有些记录的向量没算好，这次按意思只找了算好的：{e}")),
        Ok(_) if missing > 0 => Some(format!("还有 {missing} 条记录的向量没算好，这次按意思没找它们。")),
        Ok(_) => None,
    };
    Ok((hits.into_iter().filter_map(|h| docs.get(&h.doc).filter(|d| !exclude.contains(&d.module)).map(|d| (d.clone(), h.ordinal, h.score))).collect(), note))
}

/// A doc in the merged list.
struct Merged {
    doc: Doc,
    /// The best passage when found by meaning.
    ordinal: Option<u16>,
    rrf: f32,
    by: Way,
}

/// Reciprocal rank fusion of the two lists (a doc in both adds up), best first, at most `keep`.
fn merge(words: Vec<(Doc, f32)>, meaning: Vec<(Doc, u16, f32)>, keep: usize) -> Vec<Merged> {
    let rrf = |rank: usize| 1.0 / (RRF_K + rank as f32 + 1.0);
    let mut merged: Vec<Merged> = words.into_iter().enumerate().map(|(r, (doc, _))| Merged { doc, ordinal: None, rrf: rrf(r), by: Way::Words }).collect();
    for (r, (doc, o, _)) in meaning.into_iter().enumerate() {
        match merged.iter_mut().find(|m| m.doc.id == doc.id) {
            Some(m) => (m.ordinal, m.rrf, m.by) = (Some(o), m.rrf + rrf(r), Way::Both),
            None => merged.push(Merged { doc, ordinal: Some(o), rrf: rrf(r), by: Way::Meaning }),
        }
    }
    merged.sort_by(|a, b| b.rrf.total_cmp(&a.rrf));
    merged.truncate(keep);
    merged
}

/// Final order: what the rerank model is sure about first (by its scores), the rest as merged.
fn order(merged: &[Merged], scores: Option<Vec<f32>>) -> Vec<(usize, f32)> {
    let s = scores.unwrap_or_default();
    let mut out: Vec<(usize, f32)> = s.iter().copied().enumerate().filter(|(_, x)| *x >= SURE).collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out.extend((0..merged.len()).filter(|i| s.get(*i).is_none_or(|x| *x < SURE)).map(|i| (i, merged[i].rrf)));
    out
}

pub async fn search(app: &AppHandle, req: &Request) -> Result<Answer, String> {
    let st = app.state::<AppState>();
    let q = req.text.trim();
    if q.is_empty() {
        return Ok(Answer::default());
    }
    let limit = req.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 200);
    let (hidden, customs) = st.with_db(|c| Ok((data::hidden(c), data::custom_keys(c)))).unwrap_or_default();
    let cloud = if req.for_cloud { Some(st.picky.with_db(|c| Ok(data::cloud_allowed(c, &customs))).unwrap_or_default()) } else { None };
    let mask = allowed(&req.modules, &req.kinds, &hidden, cloud.as_deref());
    let exclude = excluded(&customs, &req.modules, &hidden, cloud.as_deref());
    let first = text::first_word(q);
    let words = |n: usize| st.search.with_db(|c| by_words(c, q, mask, &exclude, n));

    if req.way == Way::Words {
        let found = st.search.with_db(|c| found_by_words(c, q, mask, &exclude, limit))?;
        return Ok(Answer { found, note: None });
    }
    let meaning = by_meaning(app, q, mask, &exclude, if req.way == Way::Meaning { limit } else { POOL }).await;
    if req.way == Way::Meaning {
        let (hits, note) = meaning?;
        let found = hits.into_iter().map(|(d, o, score)| found(&d, flat_snippet(&passage(&d, Some(o), &first)), score, Way::Meaning)).collect();
        return Ok(Answer { found, note });
    }

    // both: reciprocal rank fusion, then the rerank model on the first RERANK
    let word_hits = words(POOL)?;
    let (meaning_hits, mut note) = match meaning {
        Ok(x) => x,
        Err(e) => (vec![], Some(format!("向量服务用不了，只按字找了：{e}"))),
    };
    let merged = merge(word_hits, meaning_hits, RERANK.max(limit));
    let passages: Vec<String> = merged.iter().map(|m| passage(&m.doc, m.ordinal, &first)).collect();
    let rerank_n = RERANK.min(merged.len());
    let reranked = match rerank::prepare(app).await {
        Ok(model) => rerank::scores(&st.picky.runtime, &model, q, &passages[..rerank_n]).await,
        Err(e) => Err(e),
    };
    let scores = match reranked {
        Ok(scores) => Some(scores),
        Err(e) => {
            note.get_or_insert(format!("重排服务用不了，按合并后的顺序排：{e}"));
            None
        }
    };
    let found = order(&merged, scores)
        .into_iter()
        .take(limit)
        .map(|(i, score)| {
            let m = &merged[i];
            let snippet = if m.by != Way::Meaning { crate::picky::find::snippet(&m.doc.body, &first, SNIPPET) } else { flat_snippet(&passages[i]) };
            found(&m.doc, snippet, score, m.by)
        })
        .collect();
    Ok(Answer { found, note })
}

#[tauri::command]
pub async fn search_query(app: AppHandle, request: Request) -> Result<Answer, String> {
    search(&app, &request).await
}

/// 意思相近 in ⌘K: meaning hits shaped like ⌘K's (小拾's open their conversation).
#[tauri::command]
pub async fn search_similar(app: AppHandle, q: String) -> Result<Vec<super::Hit>, String> {
    let req = Request { text: q, way: Way::Meaning, modules: vec![], kinds: vec![], for_cloud: false, limit: Some(8) };
    let found = search(&app, &req).await?.found;
    let st = app.state::<AppState>();
    let money: HashMap<i64, (i64, String, String)> = {
        let ids: Vec<i64> = found.iter().filter(|f| f.kind == "entry").map(|f| f.id).collect();
        st.with_db(|c| {
            let mut out = HashMap::new();
            for id in ids {
                if let Ok(row) = c.query_row("SELECT amount, currency, flow FROM fin_entries WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))) {
                    out.insert(id, row);
                }
            }
            Ok(out)
        })
        .unwrap_or_default()
    };
    Ok(found
        .into_iter()
        .map(|f| {
            let (kind, id, parent) = match f.kind {
                "message" => ("chat", f.parent.unwrap_or_default(), Some(f.id)),
                "session" => ("chat", f.id, None),
                k => (k, f.id, f.parent),
            };
            let m = money.get(&f.id).filter(|_| f.kind == "entry");
            super::Hit {
                module: f.module,
                kind,
                id,
                title: f.title,
                detail: if kind == "chat" { Some(f.snippet) } else { f.detail },
                parent,
                day: f.day,
                amount: m.map(|x| x.0),
                currency: m.map(|x| x.1.clone()),
                flow: m.map(|x| x.2.clone()),
            }
        })
        .collect())
}

/// 意思相近 on 念念不忘's page: notes by meaning within the page's other filters (tag, day).
#[tauri::command]
pub async fn search_notes_similar(app: AppHandle, filter: crate::modules::notes::Filter) -> Result<Vec<crate::modules::notes::Note>, String> {
    let q = filter.q.clone().unwrap_or_default();
    let req = Request { text: q, way: Way::Meaning, modules: vec!["notes".into()], kinds: vec!["note".into()], for_cloud: false, limit: Some(20) };
    let found = search(&app, &req).await?.found;
    let rest = crate::modules::notes::Filter { q: None, ..filter };
    let mut notes = app.state::<AppState>().with_db(|c| crate::modules::notes::list(c, &rest))?;
    let rank: HashMap<i64, usize> = found.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
    notes.retain(|n| rank.contains_key(&n.id));
    notes.sort_by_key(|n| rank[&n.id]);
    Ok(notes)
}

#[tauri::command]
pub fn search_index_status(state: tauri::State<AppState>) -> super::indexer::IndexStatus {
    state.search.status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::indexer::{apply, load, pending, save_vectors};
    use crate::search::store::tests::memory_db;
    use crate::search::vectors::{quantize, VectorIndex};

    fn ids(v: &[(Doc, f32)]) -> Vec<i64> {
        v.iter().map(|(d, _)| d.ref_id).collect()
    }

    /// Notes in a fresh index, with mock vectors.
    fn notes(texts: &[&str]) -> Connection {
        let s = memory_db();
        let p = crate::db::tests::memory_db();
        for (i, t) in texts.iter().enumerate() {
            p.execute("INSERT INTO notes (content, created_at) VALUES (?1, ?2)", rusqlite::params![t, format!("2026-10-{:02}T01:00:00.000Z", i + 1)]).unwrap();
        }
        let (i, src) = sources::source("notes", "note").unwrap();
        apply(&s, i, sources::read(&p, src).unwrap()).unwrap();
        for d in pending(&s, "mock", 100).unwrap() {
            let qs: Vec<_> = text::passages(&d.title, &d.body).iter().map(|x| quantize(&embed::mock_vector(&format!("笔记：{x}")))).collect();
            save_vectors(&s, "mock", &d, &qs).unwrap();
        }
        s
    }

    #[test]
    fn scope_leaves_out_hidden_cloud_refused_and_unasked() {
        let all = allowed(&[], &[], &[], None);
        assert_eq!(all.count_ones() as usize, SOURCES.len());
        let note = 1 << sources::source("notes", "note").unwrap().0;
        assert_eq!(allowed(&["notes".into()], &[], &[], None), note);
        assert_eq!(allowed(&[], &[], &["notes".into()], None) & note, 0);
        let cloud = allowed(&[], &[], &[], Some(&["notes".to_string()]));
        assert_eq!(cloud & (1 << sources::source("finance", "entry").unwrap().0), 0); // not allowed for the cloud
        assert_ne!(cloud & (1 << sources::source("picky", "message").unwrap().0), 0); // 小拾's own conversations
        assert_eq!(cloud & note, note);
        assert_eq!(source_filter(all, &[]), "");
        assert_eq!(source_filter(0, &[]), " AND 0");
        // made modules: their source is in for any of them; the ones left out go by key
        let items = 1 << sources::source("list/3", "list_item").unwrap().0;
        assert_eq!(allowed(&["list/3".into()], &[], &["list/3".into()], None), items);
        let customs = ["list/3".to_string(), "list/4".into(), "progress/5".into()];
        assert_eq!(excluded(&customs, &[], &["list/4".into()], Some(&["list/3".to_string()])), ["list/4", "progress/5"]);
        assert_eq!(excluded(&customs, &["list/3".into()], &[], None), ["list/4", "progress/5"]);
        assert_eq!(excluded(&customs, &["list".into()], &[], None), ["progress/5"]);
        assert_eq!(allowed(&["progress".into()], &[], &[], None).count_ones(), 2);
        assert_eq!(source_filter(all, &["list/4".into()]), " AND d.module NOT IN ('list/4')");
    }

    #[test]
    fn words_and_meaning_find_their_own() {
        let s = notes(&["周末去看了两套出租的公寓，押一付三", "整理了租房要注意的事：合同、押金", "读完了《百年孤独》", "午饭 35 元"]);
        let all = allowed(&[], &[], &[], None);
        assert_eq!(ids(&by_words(&s, "租房", all, &[], 10).unwrap()), [2]);
        assert_eq!(ids(&by_words(&s, "押", all, &[], 10).unwrap()).len(), 2);
        assert!(by_words(&s, "租房", 0, &[], 10).unwrap().is_empty()); // nothing allowed
        assert!(by_words(&s, "。，", all, &[], 10).unwrap().is_empty());

        let flat = load(&s, "mock").unwrap();
        let hits = flat.search(&embed::mock_vector("租房押金合同"), all, 10, 0.2);
        assert_eq!(hits[0].doc, s.query_row("SELECT id FROM docs WHERE ref_id = 2", [], |r| r.get::<_, i64>(0)).unwrap());
        let d = docs_by_id(&s, &[hits[0].doc]).unwrap().remove(&hits[0].doc).unwrap();
        assert!(passage(&d, Some(hits[0].ordinal), "").contains("押金"));
        assert_eq!(flat_snippet("一 二\n三"), "一 二 三");
    }

    #[test]
    fn both_merges_by_rank_and_reranks_the_top() {
        let d = |id: i64| Doc { id, source: 0, module: "todo".into(), ref_id: id, title: format!("t{id}"), detail: None, parent: None, day: None, body: String::new() };
        let m = merge(vec![(d(1), 9.0), (d(2), 5.0)], vec![(d(2), 0, 0.8), (d(3), 1, 0.6)], 10);
        assert_eq!(m.iter().map(|x| (x.doc.id, x.by)).collect::<Vec<_>>(), [(2, Way::Both), (1, Way::Words), (3, Way::Meaning)]);
        assert_eq!(m[0].ordinal, Some(0));
        assert_eq!(merge(vec![(d(1), 1.0)], vec![(d(3), 0, 0.5)], 1).len(), 1);
        // what the reranker is sure about by its scores, the rest after it as merged
        let o = order(&m, Some(vec![0.1, 0.9]));
        assert_eq!(o.iter().map(|x| x.0).collect::<Vec<_>>(), [1, 0, 2]);
        let o = order(&m, Some(vec![0.003, 0.9, 0.012]));
        assert_eq!(o.iter().map(|x| x.0).collect::<Vec<_>>(), [1, 0, 2]); // noise doesn't reorder
        assert_eq!(order(&m, Some(vec![0.003, 0.012])).iter().map(|x| x.0).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(order(&m, None).iter().map(|x| x.0).collect::<Vec<_>>(), [0, 1, 2]); // no reranker: merged order
    }

    /// 200 000 records: words within 0.1 s, meaning (the scan, the question already embedded)
    /// within 0.3 s. Run in release: cargo test --release search_speed -- --ignored --nocapture
    #[test]
    #[ignore]
    fn search_speed() {
        use std::time::Instant;
        const N: usize = 200_000;
        const DIMS: usize = 1024;
        let dir = crate::db::tests::temp_dir("search-speed");
        let s = crate::search::store::open(&dir.join("search.db")).unwrap();
        // made-up notes from a word list with a long tail (some words common, most rare)
        let vocab: Vec<String> = {
            let base = ["租房", "合同", "押金", "周末", "公园", "读书", "电影", "面试", "简历", "旅行", "机票", "酒店", "午饭", "咖啡", "跑步", "画画", "速写", "预算", "工资", "家人", "朋友", "项目", "会议", "论文", "复习", "考试", "医院", "体检", "搬家", "装修"];
            let mut v: Vec<String> = base.iter().map(|x| x.to_string()).collect();
            let chars: Vec<char> = "天地人和春夏秋冬山水风云日月星辰花草树木鸟鱼虫龙虎马牛羊猫狗金木水火土东南西北红黄蓝绿白黑".chars().collect();
            for i in 0..4000 {
                v.push(format!("{}{}{}", chars[i % chars.len()], chars[(i / chars.len()) % chars.len()], chars[(i * 7 + 3) % chars.len()]));
            }
            v
        };
        let mut seed: u64 = 42;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let t = Instant::now();
        let (bi, _) = sources::source("notes", "note").unwrap();
        let mut rows = Vec::with_capacity(N);
        for i in 0..N {
            let words: Vec<&str> = (0..12).map(|_| {
                let r = rnd();
                // Zipf-ish: small indexes far more often
                let k = ((r % 1000) as f64 / 1000.0).powi(3) * vocab.len() as f64;
                vocab[k as usize].as_str()
            }).collect();
            rows.push(sources::Row { ref_id: i as i64 + 1, title: words[0].into(), detail: None, parent: None, day: None, at: format!("{:08}", i), body: words.join("，"), module: None });
        }
        apply(&s, bi, rows).unwrap();
        println!("indexed {N} records in {:.1} s", t.elapsed().as_secs_f32());

        let all = allowed(&[], &[], &[], None);
        for q in ["租房", "租房 押金", "咖啡", "天地人", "合 同", "不存在的词"] {
            let t = Instant::now();
            let hits = by_words(&s, q, all, &[], 40).unwrap();
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            println!("words {q:<8} {:>5} hits shown  {ms:6.1} ms", hits.len());
            assert!(ms < 100.0, "{q}: {ms} ms");
        }
        let note = allowed(&["notes".into()], &[], &[], None);
        let t = Instant::now();
        let hits = by_words(&s, "租房", note, &[], 40).unwrap();
        println!("words in one module: {} hits {:.1} ms", hits.len(), t.elapsed().as_secs_f64() * 1000.0);

        // vectors: random unit vectors (the scan doesn't care what they mean)
        let mut flat = crate::search::vectors::Flat::new("speed");
        let ids: Vec<i64> = s.prepare("SELECT id FROM docs").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        let mut vec_of = |_: usize| -> Vec<f32> { (0..DIMS).map(|_| (rnd() % 2001) as f32 / 1000.0 - 1.0).collect() };
        for id in &ids {
            flat.add(crate::search::vectors::Entry { doc: *id, ordinal: 0, source: bi as u8 }, &quantize(&vec_of(0)));
        }
        let question = vec_of(1);
        for (label, mask) in [("all", all), ("notes", note)] {
            let t = Instant::now();
            let hits = flat.search(&question, mask, 40, -1.0);
            let docs = docs_by_id(&s, &hits.iter().map(|h| h.doc).collect::<Vec<_>>()).unwrap();
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            println!("meaning ({label}) over {} passages: {} hits {ms:.1} ms", flat.len(), docs.len());
            assert!(ms < 300.0, "{ms} ms");
        }
    }

    /// The real 8081 / 8082 services on the real import files (local only, in memory) plus two
    /// made-up notes: what words, meaning and the rerank model find. Starts and stops the services.
    /// POMELO_REAL_LLM=1 [POMELO_ASK='找房子|…'] cargo test real_meaning_search -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_meaning_search() {
        if std::env::var("POMELO_REAL_LLM").as_deref() != Ok("1") {
            return;
        }
        use crate::picky::embed::EmbedError;
        use crate::picky::runtime::{LlmPaths, Runtime};
        let home = std::env::var("HOME").unwrap();
        let rt = Runtime::new(crate::picky::Mode::Real, LlmPaths::new(std::path::Path::new(&home)));
        let mut pomelo = crate::db::tests::memory_db();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../prototype");
        let scratch = crate::db::tests::temp_dir("real-search");
        for f in ["todo-import.json", "notes-import.json", "jobs-import.json"] {
            if let Ok(text) = std::fs::read_to_string(dir.join(f)) {
                crate::transfer::apply(&mut pomelo, &text, true, &scratch).unwrap();
            }
        }
        pomelo
            .execute_batch("INSERT INTO notes (content, created_at) VALUES ('整理了一下租房要注意的事：合同、押金、水电燃气的交接。', '2026-10-01T01:00:00.000Z'), ('周末去公园散步，银杏黄了。', '2026-10-02T01:00:00.000Z');")
            .unwrap();
        let s = memory_db();
        for (i, src) in SOURCES.iter().enumerate().filter(|(_, x)| x.db == sources::Db::Pomelo) {
            apply(&s, i, sources::read(&pomelo, src).unwrap()).unwrap();
        }
        tauri::async_runtime::block_on(async {
            let model = rt.ensure(Svc::Embedding).await.unwrap();
            let t = std::time::Instant::now();
            let mut n = 0;
            loop {
                let docs = pending(&s, &model.key, 64).unwrap();
                if docs.is_empty() {
                    break;
                }
                let (mut texts, mut owners) = (vec![], vec![]);
                for (i, d) in docs.iter().enumerate() {
                    for p in text::passages(&d.title, &d.body) {
                        texts.push(format!("{}：{p}", SOURCES[d.source].label));
                        owners.push(i);
                    }
                }
                let vecs = match embed::embed(&rt, &model, &texts, Input::Document).await {
                    Ok(v) => v,
                    Err(EmbedError::TooLong) => {
                        let mut v = vec![];
                        for t in &texts {
                            v.push(embed::embed(&rt, &model, &[t.chars().take(400).collect()], Input::Document).await.map_err(String::from).unwrap().remove(0));
                        }
                        v
                    }
                    Err(e) => panic!("{}", String::from(e)),
                };
                let mut per: Vec<Vec<_>> = vec![vec![]; docs.len()];
                for (i, v) in owners.into_iter().zip(vecs) {
                    per[i].push(quantize(&v));
                }
                for (d, qs) in docs.iter().zip(per) {
                    save_vectors(&s, &model.key, d, &qs).unwrap();
                }
                n += docs.len();
            }
            println!("vectors for {n} records in {:.1} s", t.elapsed().as_secs_f32());
            let flat = load(&s, &model.key).unwrap();
            let rerank_model = rt.ensure(Svc::Rerank).await.unwrap();
            let asks = std::env::var("POMELO_ASK").unwrap_or_else(|_| "找房子|最近在读什么书|想换工作".into());
            for q in asks.split('|') {
                let t = std::time::Instant::now();
                let mask = allowed(&[], &[], &[], None);
                let qv = embed::embed(&rt, &model, &[q.to_string()], Input::RecordQuery).await.map_err(String::from).unwrap().remove(0);
                let hits = flat.search(&qv, mask, POOL, MIN_MEANING);
                if std::env::var("POMELO_SHOW").as_deref() == Ok("1") {
                    let all = flat.search(&qv, mask, 400, -1.0);
                    let bodies = docs_by_id(&s, &all.iter().map(|h| h.doc).collect::<Vec<_>>()).unwrap();
                    for (rank, h) in all.iter().enumerate().filter(|(r, h)| *r < 5 || bodies.get(&h.doc).is_some_and(|d| d.body.contains("租房"))) {
                        println!("  raw #{rank} {:.3} {}", h.score, bodies.get(&h.doc).map(|d| d.body.chars().take(40).collect::<String>()).unwrap_or_default());
                    }
                }
                let docs = docs_by_id(&s, &hits.iter().map(|h| h.doc).collect::<Vec<_>>()).unwrap();
                let meaning: Vec<(Doc, u16, f32)> = hits.into_iter().filter_map(|h| docs.get(&h.doc).map(|d| (d.clone(), h.ordinal, h.score))).collect();
                let words = by_words(&s, q, mask, &[], POOL).unwrap();
                let first = text::first_word(q);
                let merged = merge(words, meaning, RERANK);
                let passages: Vec<String> = merged.iter().map(|m| passage(&m.doc, m.ordinal, &first)).collect();
                let k = RERANK.min(merged.len());
                let scores = rerank::scores(&rt, &rerank_model, q, &passages[..k]).await.unwrap();
                if std::env::var("POMELO_SHOW").as_deref() == Ok("1") {
                    for (i, p) in passages.iter().enumerate().take(5) {
                        println!("  merged #{i} rrf {:.4} rerank {:.4} {:?}", merged[i].rrf, scores.get(i).copied().unwrap_or(f32::NAN), p.chars().take(60).collect::<String>());
                    }
                }
                println!("\n==== {q}  ({:.1} s, {} merged)", t.elapsed().as_secs_f32(), merged.len());
                for (i, score) in order(&merged, Some(scores)).into_iter().take(6) {
                    println!("  {score:.3} {:?} {}", merged[i].by, flat_snippet(&passages[i]).chars().take(70).collect::<String>());
                }
            }
            rt.stop_ours_on_exit();
        });
    }
}
