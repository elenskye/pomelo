//! 出处 (v0.4 phase 3): what a reply is given gets a number — the question's retrieved project file
//! passages first (reading::prompt shows them as 【[1] 资料 · 第 3 页】; then the 知识库's, v0.4.5, which
//! keep their source and file to open), then the records 小拾's search
//! finds — and the reply marks what it used with [n]. The list is kept per run (`run_sources`) as it
//! grows; the page shows the cited ones under the reply and opens them (records like a 全局搜索 hit,
//! passages as their text). Memories are not numbered (they keep their own list).
//! - pomelo_search: words and meaning together, reranked (search::query, Way::Both), within what
//!   this answer may read (hidden modules never; for a cloud model only modules allowed there).
//!   私密 conversations never (library.rs).
//!   A record found again keeps its number.
//! - pomelo_read: the whole of a numbered record (data::record_text; items open their trip /
//!   template; 小拾 messages from picky.db), checked again against the same rules.
//! - Earlier results re-sent with a later question lose their numbers (`unnumbered`): numbers
//!   belong to their own reply.
//! - Web pages (v0.4 phase 6, skills/: 联网搜索, 书单 / 影单检索) are numbered too (kind web, with
//!   their url; the same url keeps its number) and open in the browser.

use super::data::{self, Access, Answer, PageRef};
use super::reading::{self, Kind};
use crate::modules::common::err;
use crate::modules::custom::Template;
use crate::search::query::{self, Found, Way};
use crate::search::sources as index_sources;
use crate::AppState;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

pub const SEARCH: &str = "pomelo_search";
pub const READ: &str = "pomelo_read";
const RESULTS: usize = 12;
const SNIPPET_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub n: i64,
    /// record | passage | web
    pub kind: String,
    pub module: Option<String>,
    pub record_kind: Option<String>,
    pub id: Option<i64>,
    pub parent: Option<i64>,
    pub day: Option<String>,
    pub artifact_id: Option<i64>,
    pub segment: Option<i64>,
    /// A record's title, or a passage's 「资料名 · 第 3 页」.
    pub title: String,
    /// What kind of record and where (「笔记 · 2026/10/01」).
    pub detail: Option<String>,
    /// What the model was given.
    pub content: String,
    /// A web page (kind web).
    #[serde(default)]
    pub url: Option<String>,
    /// A 知识库 passage: its source and file (relative to the folder; "" for a web page).
    #[serde(default)]
    pub kb_source: Option<i64>,
    #[serde(default)]
    pub kb_path: Option<String>,
}

/// One reply's numbered list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sources(pub Vec<Source>);

/// 「笔记」「出行物品」「小拾对话」…
fn label(module: &str, kind: &str) -> &'static str {
    index_sources::source(module, kind).map_or("记录", |(_, s)| s.label)
}

/// 「笔记 · 2026/10/01」「任务 · 学业 · 已完成」
fn detail(f: &Found) -> String {
    let day = f.day.as_deref().map(|d| d.replace('-', "/"));
    [Some(label(&f.module, f.kind).to_string()), f.detail.clone(), day].into_iter().flatten().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ")
}

fn clip(s: &str, n: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > n {
        format!("{}…", flat.chars().take(n).collect::<String>())
    } else {
        flat
    }
}

impl Sources {
    /// The question's retrieved passages (project files', then the 知识库's), numbered in the order
    /// reading::prompt shows them.
    pub fn of_material(material: &[reading::Source]) -> Sources {
        Sources(
            material
                .iter()
                .filter(|s| s.kind == Kind::Passage)
                .chain(material.iter().filter(|s| s.kind == Kind::Knowledge))
                .chain(material.iter().filter(|s| s.kind == Kind::Excerpt))
                .enumerate()
                .map(|(i, s)| Source {
                    n: i as i64 + 1,
                    kind: "passage".into(),
                    module: None,
                    record_kind: None,
                    id: None,
                    parent: None,
                    day: None,
                    artifact_id: Some(s.artifact).filter(|a| *a > 0),
                    segment: Some(s.ordinal),
                    title: s.label.clone(),
                    detail: Some(match (&s.kb, s.kind) {
                        (Some(k), _) => format!("知识库 · {}", k.library),
                        (None, Kind::Excerpt) => "附件和资料".into(),
                        _ => "项目资料".into(),
                    }),
                    content: s.content.clone(),
                    url: None,
                    kb_source: s.kb.as_ref().map(|k| k.source),
                    kb_path: s.kb.as_ref().map(|k| k.path.clone()),
                })
                .collect(),
        )
    }

    fn known(&self, f: &Found) -> Option<i64> {
        self.0.iter().find(|s| s.kind == "record" && s.module.as_deref() == Some(f.module.as_str()) && s.record_kind.as_deref() == Some(f.kind) && s.id == Some(f.id)).map(|s| s.n)
    }

    /// The number a found record has or would get.
    fn peek(&self, f: &Found) -> i64 {
        self.known(f).unwrap_or(self.0.len() as i64 + 1)
    }

    /// A found record's number; `true` when it is new.
    fn number(&mut self, f: &Found) -> (i64, bool) {
        if let Some(n) = self.known(f) {
            return (n, false);
        }
        let n = self.0.len() as i64 + 1;
        self.0.push(Source {
            n,
            kind: "record".into(),
            module: Some(f.module.clone()),
            record_kind: Some(f.kind.into()),
            id: Some(f.id),
            parent: f.parent,
            day: f.day.clone(),
            artifact_id: None,
            segment: None,
            title: f.title.clone(),
            detail: Some(detail(f)),
            content: f.snippet.clone(),
            url: None,
            kb_source: None,
            kb_path: None,
        });
        (n, true)
    }

    /// A stored file's segment (读文件): its number (the same segment keeps its first one).
    pub fn passage(&mut self, artifact: i64, segment: i64, title: &str, detail: &str, content: &str) -> i64 {
        if let Some(s) = self.0.iter().find(|s| s.kind == "passage" && s.kb_source.is_none() && s.artifact_id == Some(artifact) && s.segment == Some(segment)) {
            return s.n;
        }
        let n = self.0.len() as i64 + 1;
        self.0.push(Source { n, kind: "passage".into(), module: None, record_kind: None, id: None, parent: None, day: None, artifact_id: Some(artifact), segment: Some(segment), title: title.into(), detail: Some(detail.into()), content: content.into(), url: None, kb_source: None, kb_path: None });
        n
    }

    /// A 知识库 passage (查知识库): its number (the same passage keeps its first one).
    pub fn knowledge(&mut self, p: &super::index::Passage) -> i64 {
        let Some(k) = &p.kb else { return self.passage(p.artifact_id, p.segment_ordinal, &p.label, "项目资料", &p.content) };
        if let Some(s) = self.0.iter().find(|s| s.kb_source == Some(k.source) && s.kb_path.as_deref() == Some(k.path.as_str()) && s.content == p.content) {
            return s.n;
        }
        let n = self.0.len() as i64 + 1;
        self.0.push(Source {
            n,
            kind: "passage".into(),
            module: None,
            record_kind: None,
            id: None,
            parent: None,
            day: None,
            artifact_id: Some(p.artifact_id).filter(|a| *a > 0),
            segment: Some(p.segment_ordinal),
            title: p.label.clone(),
            detail: Some(format!("知识库 · {}", k.library)),
            content: p.content.clone(),
            url: None,
            kb_source: Some(k.source),
            kb_path: Some(k.path.clone()),
        });
        n
    }

    /// The number a web page has or would get.
    pub fn peek_web(&self, url: &str) -> i64 {
        self.0.iter().find(|s| s.kind == "web" && s.url.as_deref() == Some(url)).map_or(self.0.len() as i64 + 1, |s| s.n)
    }

    /// A web page's number (the same url keeps its first one); `detail`: the site.
    pub fn web(&mut self, url: &str, title: &str, detail: &str, content: &str) -> i64 {
        if let Some(s) = self.0.iter().find(|s| s.kind == "web" && s.url.as_deref() == Some(url)) {
            return s.n;
        }
        let n = self.0.len() as i64 + 1;
        self.0.push(Source {
            n,
            kind: "web".into(),
            module: None,
            record_kind: None,
            id: None,
            parent: None,
            day: None,
            artifact_id: None,
            segment: None,
            title: title.into(),
            detail: Some(detail.into()),
            content: content.into(),
            url: Some(url.into()),
            kb_source: None,
            kb_path: None,
        });
        n
    }

    pub fn get(&self, n: i64) -> Option<&Source> {
        self.0.iter().find(|s| s.n == n)
    }
}

// ---------- tools ----------

/// The search and read tools (no user data in them).
pub fn tools(access: &Access) -> Vec<Value> {
    let mut modules: Vec<&str> = access.offered().iter().map(|m| m.key).collect();
    for t in [Template::List, Template::Progress] {
        if !access.made_offered(t).is_empty() {
            modules.push(t.as_str());
        }
    }
    modules.push("picky");
    vec![
        json!({ "type": "function", "function": {
            "name": SEARCH,
            "description": "按字和按意思一起，跨模块搜索用户在柚儿园里的记录（待办、书影、绘画、出行、记账、投递、笔记、重要的日子，自己建的清单 list 和进度 progress）和以前跟你的对话（picky）。适合「关于某件事记过什么」「有没有提到过某某」这类内容分散在各处的问题。结果带编号 [n]：回答里用到时在那句话后面标出编号；要看某条的全部内容用 pomelo_read。",
            "parameters": { "type": "object", "properties": {
                "query": { "type": "string", "description": "要找的事，用自然的说法（比如「租房」「找工作时的焦虑」）" },
                "modules": { "type": "array", "items": { "type": "string", "enum": modules }, "description": "只在这些模块里找；不填就是全部" },
            }, "required": ["query"] },
        } }),
        json!({ "type": "function", "function": {
            "name": READ,
            "description": "读某条搜索结果的全部内容（n 是结果前面的编号）。",
            "parameters": { "type": "object", "properties": { "n": { "type": "integer", "description": "结果的编号" } }, "required": ["n"] },
        } }),
    ]
}

/// The part of the system message about searching and citing.
pub const INSTRUCTIONS: &str = "- 要找「关于某件事记过什么」「有没有提到过」这类分散在各处的内容，用 pomelo_search（按意思也能找到，也能找到以前跟你的对话）；要看某条结果的全部内容，用 pomelo_read。\n- 回答里用到搜到的记录、或检索到的资料段落时，在那句话后面标出它的编号，比如 [2]；只标真的用到的，不要编编号。";

fn answer(label: String, state: &'static str, text: String) -> Answer {
    Answer { module: None, label, state, text, records: vec![] }
}

fn est(s: &str) -> i64 {
    super::context::estimate(s) + 1
}

/// pomelo_search's request (None + the answer when the arguments don't make one).
pub fn request(access: &Access, arguments: &str) -> Result<query::Request, Answer> {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let q = args["query"].as_str().unwrap_or("").trim().to_string();
    if q.is_empty() {
        return Err(answer("搜索 · 没有要找的内容".into(), "failed", "没搜：query 不能为空。".into()));
    }
    let modules: Vec<String> = args["modules"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).map(String::from).collect();
    Ok(query::Request { text: q, way: Way::Both, modules, kinds: vec![], for_cloud: access.cloud, limit: Some(RESULTS) })
}

/// Runs pomelo_search: new records join `sources`. 私密 conversations are left out.
pub async fn search(app: &AppHandle, access: &Access, sources: &mut Sources, arguments: &str, budget: i64) -> Answer {
    let mut req = match request(access, arguments) {
        Ok(r) => r,
        Err(a) => return a,
    };
    let private = app.state::<AppState>().picky.with_db(|c| super::library::private_ids(c)).unwrap_or_default();
    if !private.is_empty() {
        req.limit = Some(RESULTS * 2);
    }
    match query::search(app, &req).await {
        Ok(mut found) => {
            leave_private(&mut found.found, &private);
            found.found.truncate(RESULTS);
            results(&req.text, &found, sources, budget)
        }
        Err(e) => answer(format!("搜索「{}」· 没搜成", clip(&req.text, 20)), "failed", format!("没搜成：{e}")),
    }
}

/// Drops results from 私密 conversations (a message's parent is its conversation).
pub fn leave_private(found: &mut Vec<Found>, private: &std::collections::HashSet<i64>) {
    found.retain(|f| f.module != "picky" || !private.contains(&if f.kind == "session" { f.id } else { f.parent.unwrap_or_default() }));
}

/// What the model gets for `found`: numbered lines while they fit `budget`.
pub fn results(q: &str, found: &query::Answer, sources: &mut Sources, budget: i64) -> Answer {
    let budget = budget.min(data::MAX_RESULT);
    let head = if found.found.is_empty() { format!("没有找到和「{q}」有关的记录。") } else { format!("和「{q}」有关的记录（按相关程度排）：") };
    let mut lines = vec![head];
    let mut used = est(&lines[0]);
    let mut shown = 0;
    for f in &found.found {
        let n = sources.peek(f);
        let snippet = clip(&f.snippet, SNIPPET_CHARS);
        let line = if snippet.is_empty() || snippet == clip(&f.title, SNIPPET_CHARS) {
            format!("[{n}] {}「{}」", detail(f), clip(&f.title, 60))
        } else {
            format!("[{n}] {}「{}」：{snippet}", detail(f), clip(&f.title, 60))
        };
        if used + est(&line) > budget {
            break;
        }
        used += est(&line);
        sources.number(f);
        lines.push(line);
        shown += 1;
    }
    if shown < found.found.len() {
        lines.push(format!("……还有 {} 条没列出（可以换个更具体的说法再搜）", found.found.len() - shown));
    }
    if let Some(note) = &found.note {
        lines.push(format!("（{note}）"));
    }
    answer(format!("搜索「{}」· {shown} 条", clip(&q, 20)), "ok", lines.join("\n"))
}

/// Runs pomelo_read.
pub fn read(app: &AppHandle, access: &Access, sources: &Sources, arguments: &str, today: &str, budget: i64) -> Answer {
    let st = app.state::<AppState>();
    read_with(access, sources, arguments, budget, |module, kind, id| {
        if module == "picky" {
            st.picky.with_db(|c| conversation_text(c, kind, id))
        } else {
            st.with_db(|c| data::record_text(c, today, &PageRef { kind: kind.into(), id }).map(|(_, text, _)| text))
        }
    })
}

/// pomelo_read with `fetch` (module, kind, id → the whole text) doing the reading.
pub fn read_with(access: &Access, sources: &Sources, arguments: &str, budget: i64, fetch: impl FnOnce(&str, &str, i64) -> Result<String, String>) -> Answer {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let n = args["n"].as_i64().or_else(|| args["n"].as_str().and_then(|s| s.trim_matches(|c| c == '[' || c == ']').parse().ok()));
    let Some(s) = n.and_then(|n| sources.get(n)) else {
        return answer("读全文 · 没有这个编号".into(), "failed", "没有这个编号的结果：先用 pomelo_search 搜，再用结果前面的编号。".into());
    };
    let label = format!("读了 [{}] {}", s.n, clip(&s.title, 20));
    if s.kind == "passage" || s.kind == "web" {
        return answer(label, "ok", format!("[{}] {}\n{}", s.n, s.title, s.content));
    }
    let (module, kind, id) = (s.module.clone().unwrap_or_default(), s.record_kind.clone().unwrap_or_default(), s.id.unwrap_or_default());
    if !access.allows(&module) {
        return answer(label, "blocked", "没读：这个模块现在不能读（隐藏了，或不发给现在用的云端模型）。".into());
    }
    // an item opens its trip / template
    let (kind, id) = match kind.as_str() {
        "trip_item" => ("trip".to_string(), s.parent.unwrap_or(id)),
        "template_item" => ("template".to_string(), s.parent.unwrap_or(id)),
        _ => (kind, id),
    };
    let full = fetch(&module, &kind, id);
    match full {
        Ok(text) => {
            let text = format!("[{}] {}\n{}", s.n, s.title, text);
            let cut: String = if est(&text) > budget.min(data::MAX_RESULT) { text.chars().take((budget.min(data::MAX_RESULT) as usize).max(200)).collect::<String>() + "……（后面略）" } else { text };
            answer(label, "ok", cut)
        }
        Err(e) => answer(label, "failed", format!("没读成：{e}")),
    }
}

/// A 小拾 conversation (title) or one of its messages, as text.
fn conversation_text(c: &Connection, kind: &str, id: i64) -> Result<String, String> {
    let private: Option<bool> = if kind == "session" {
        c.query_row("SELECT private FROM sessions WHERE id = ?1", [id], |r| r.get(0)).ok()
    } else {
        c.query_row("SELECT s.private FROM messages m JOIN sessions s ON s.id = m.session_id WHERE m.id = ?1", [id], |r| r.get(0)).ok()
    };
    if private == Some(true) {
        return Err("这个对话是私密的".into());
    }
    if kind == "session" {
        return c.query_row("SELECT title FROM sessions WHERE id = ?1", [id], |r| r.get::<_, String>(0)).map(|t| format!("对话：{t}")).map_err(|_| "这个对话已经删了".to_string());
    }
    c.query_row("SELECT s.title, m.role, m.content, m.created_at FROM messages m JOIN sessions s ON s.id = m.session_id WHERE m.id = ?1", [id], |r| {
        Ok(format!("对话：{}\n{}（{}）：\n{}", r.get::<_, String>(0)?, if r.get::<_, String>(1)? == "user" { "用户问" } else { "你答" }, r.get::<_, String>(3)?.get(..10).unwrap_or_default(), r.get::<_, String>(2)?))
    })
    .map_err(|_| "这条消息已经删了".to_string())
}

// ---------- picky.db ----------

pub fn save(conn: &Connection, run_id: i64, s: &Source) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO run_sources (run_id, n, kind, module, record_kind, ref_id, parent, day, artifact_id, segment_ordinal, title, detail, content, url, kb_source, kb_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![run_id, s.n, s.kind, s.module, s.record_kind, s.id, s.parent, s.day, s.artifact_id, s.segment, s.title, s.detail, s.content, s.url, s.kb_source, s.kb_path],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn of_run(conn: &Connection, run_id: i64) -> Result<Vec<Source>, String> {
    let mut st = conn
        .prepare("SELECT n, kind, module, record_kind, ref_id, parent, day, artifact_id, segment_ordinal, title, detail, content, url, kb_source, kb_path FROM run_sources WHERE run_id = ?1 ORDER BY n")
        .map_err(err)?;
    let rows = st
        .query_map([run_id], |r| {
            Ok(Source {
                n: r.get(0)?,
                kind: r.get(1)?,
                module: r.get(2)?,
                record_kind: r.get(3)?,
                id: r.get(4)?,
                parent: r.get(5)?,
                day: r.get(6)?,
                artifact_id: r.get(7)?,
                segment: r.get(8)?,
                title: r.get(9)?,
                detail: r.get(10)?,
                content: r.get(11)?,
                url: r.get(12)?,
                kb_source: r.get(13)?,
                kb_path: r.get(14)?,
            })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// The numbers [n] a reply's text cites, in order of first use.
pub fn cited(text: &str, sources: &[Source]) -> Vec<i64> {
    let mut out = vec![];
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            let digits: String = chars[i + 1..].iter().take_while(|c| c.is_ascii_digit()).collect();
            let close = i + 1 + digits.len();
            if !digits.is_empty() && chars.get(close) == Some(&']') && chars.get(close + 1) != Some(&'(') {
                if let Ok(n) = digits.parse::<i64>() {
                    if sources.iter().any(|s| s.n == n) && !out.contains(&n) {
                        out.push(n);
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// Earlier results for a later question: "[3] 笔记…" lines become "- 笔记…".
pub fn unnumbered(text: &str) -> String {
    text.lines()
        .map(|l| match l.strip_prefix('[').and_then(|r| r.split_once("] ")).filter(|(d, _)| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())) {
            Some((_, rest)) => format!("- {rest}"),
            None => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(module: &'static str, kind: &'static str, id: i64, title: &str) -> Found {
        Found { module: module.into(), kind, id, title: title.into(), detail: None, parent: None, day: Some("2026-10-01".into()), snippet: format!("{title}的片段"), score: 1.0, by: Way::Both }
    }

    #[test]
    fn passages_first_then_records_keep_their_numbers() {
        let material = vec![
            reading::Source { kind: Kind::Attachment, artifact: 9, ordinal: 0, label: "附件".into(), content: "x".into(), kb: None, earlier: false },
            reading::Source { kind: Kind::Knowledge, artifact: 0, ordinal: 1, label: "合同.md · 第 2 段".into(), content: "合同".into(), kb: Some(super::super::knowledge::KbRef { source: 4, path: "合同.md".into(), library: "生活".into() }), earlier: false },
            reading::Source { kind: Kind::Passage, artifact: 3, ordinal: 2, label: "租房.md · 押金".into(), content: "押金怎么退".into(), kb: None, earlier: false },
        ];
        let mut s = Sources::of_material(&material);
        assert_eq!((s.0.len(), s.0[0].n, s.0[0].kind.as_str(), s.0[0].segment), (2, 1, "passage", Some(2)));
        // the 知识库 passage comes after the project file's, with what opens its file
        let k = &s.0[1];
        assert_eq!((k.n, k.artifact_id, k.kb_source, k.kb_path.as_deref(), k.detail.as_deref()), (2, None, Some(4), Some("合同.md"), Some("知识库 · 生活")));
        assert_eq!(s.number(&found("notes", "note", 7, "看房")), (3, true));
        assert_eq!(s.number(&found("books", "book", 7, "小王子")), (4, true));
        assert_eq!(s.number(&found("notes", "note", 7, "看房")), (3, false)); // found again: same number
        assert_eq!(s.get(3).unwrap().detail.as_deref(), Some("笔记 · 2026/10/01"));
    }

    #[test]
    fn cited_numbers_and_unnumbered_results() {
        let s = Sources(vec![Source { n: 1, kind: "record".into(), module: None, record_kind: None, id: None, parent: None, day: None, artifact_id: None, segment: None, title: "t".into(), detail: None, content: String::new(), url: None, kb_source: None, kb_path: None }; 2]
            .into_iter()
            .enumerate()
            .map(|(i, mut x)| {
                x.n = i as i64 + 1;
                x
            })
            .collect());
        assert_eq!(cited("租房要注意押金 [2]，看房记录见 [1][2]。[9] 不算，[链接](x) 和 [3](y) 也不算", &s.0), [2, 1]);
        assert_eq!(unnumbered("和「租房」有关：\n[1] 笔记「看房」\n[12] 书「x」\n[a] 不是"), "和「租房」有关：\n- 笔记「看房」\n- 书「x」\n[a] 不是");
    }

    #[test]
    fn private_conversations_are_not_found_or_read() {
        let c = super::super::store::tests::memory_db();
        c.execute_batch(
            "INSERT INTO sessions (title) VALUES ('租房'), ('秘密');
             INSERT INTO messages (session_id, role, content, state) VALUES (1, 'user', '哪里便宜', 'complete'), (2, 'user', '不能说', 'complete');",
        )
        .unwrap();
        super::super::library::set_private(&c, &[2], true).unwrap();
        let private = super::super::library::private_ids(&c).unwrap();
        let mut list = vec![found("picky", "session", 1, "租房"), found("picky", "session", 2, "秘密"), Found { parent: Some(2), ..found("picky", "message", 2, "秘密") }, found("notes", "note", 2, "笔记")];
        leave_private(&mut list, &private);
        assert_eq!(list.iter().map(|f| (f.kind, f.id)).collect::<Vec<_>>(), [("session", 1), ("note", 2)]);
        assert!(conversation_text(&c, "message", 1).unwrap().contains("哪里便宜"));
        assert_eq!(conversation_text(&c, "message", 2), Err("这个对话是私密的".into()));
        assert_eq!(conversation_text(&c, "session", 2), Err("这个对话是私密的".into()));
    }

    #[test]
    fn stored_with_the_run() {
        let c = super::super::store::tests::memory_db();
        c.execute_batch("INSERT INTO sessions (title) VALUES ('t'); INSERT INTO runs (session_id, model, model_name, state) VALUES (1, 'local:x', 'X', 'completed');").unwrap();
        let mut s = Sources::default();
        s.number(&found("notes", "note", 7, "看房"));
        save(&c, 1, &s.0[0]).unwrap();
        save(&c, 1, &s.0[0]).unwrap(); // saved again as it grows: no duplicate
        assert_eq!(of_run(&c, 1).unwrap(), s.0);
    }
}
