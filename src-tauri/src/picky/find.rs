//! Searching conversations (from Picky's ConversationSearch, but plain substring matching like
//! Pomelo's 全局搜索: every word must appear, case-insensitive; the data is small).
//! - The 小拾 search: titles and every finished message (all versions), filtered by the reply's model,
//!   a start time and whether archived ones count. One hit per session: title matches first, then
//!   the newest matching message; the hit carries that message (possibly an older version).
//! - ⌘K (search.rs) gets the same without archived sessions, at most PER_SOURCE.
//! - Opening a hit on an older version first shows that version (`reveal`).
//! - 意思相近 (v0.4): the search index's meaning hits among 小拾's messages and titles, with the
//!   same filters and one hit per conversation (`similar`); the page leaves out the ones words found.

use super::chat;
use crate::modules::common::err;
use crate::search::{Hit, PER_SOURCE};
use crate::AppState;
use rusqlite::{params_from_iter, types::Value, Connection, OptionalExtension};
use serde::Serialize;
use tauri::{Manager, State};

const MAX_WORDS: usize = 8;
const LIMIT: usize = 40;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatHit {
    pub session_id: i64,
    pub title: String,
    /// The matching message (None: only the title matched).
    pub message_id: Option<i64>,
    pub snippet: Option<String>,
    /// When the message was written (or the session's last activity).
    pub at: String,
    pub model: Option<String>,
    pub archived: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// A reply model's name, as shown.
    pub model: Option<String>,
    /// ISO time; only messages since then (sessions active since then for title hits).
    pub since: Option<String>,
    pub archived: bool,
}

fn words(q: &str) -> Vec<String> {
    q.split_whitespace().take(MAX_WORDS).map(str::to_lowercase).collect()
}

/// About `radius` characters either side of the first word, whitespace flattened.
pub fn snippet(content: &str, needle: &str, radius: usize) -> String {
    // Markdown emphasis / code marks read as noise in a one-line snippet
    let plain = content.replace("**", "").replace("__", "").replace('`', "");
    let flat: String = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = flat.chars().collect();
    let lower: Vec<char> = flat.to_lowercase().chars().collect();
    let n: Vec<char> = needle.chars().collect();
    let at = if lower.len() == chars.len() { (0..lower.len()).find(|i| lower[*i..].starts_with(&n)).unwrap_or(0) } else { 0 };
    let start = at.saturating_sub(radius);
    let end = (at + n.len() + radius).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    if start > 0 {
        s.insert(0, '…');
    }
    if end < chars.len() {
        s.push('…');
    }
    s
}

pub fn search(conn: &Connection, q: &str, f: &Filter, limit: usize) -> Result<Vec<ChatHit>, String> {
    let ws = words(q);
    if ws.is_empty() {
        return Ok(vec![]);
    }
    let mut args: Vec<Value> = ws.iter().map(|w| Value::Text(w.clone())).collect();
    let n = ws.len();
    let arg = |args: &mut Vec<Value>, v: Option<String>| {
        args.push(v.map_or(Value::Null, Value::Text));
        args.len()
    };
    let model_i = arg(&mut args, f.model.clone());
    let since_i = arg(&mut args, f.since.clone());
    let archived = if f.archived { "" } else { "AND s.archived_at IS NULL" };
    let cond = |col: &str| (1..=n).map(|i| format!("instr(lower({col}), ?{i}) > 0")).collect::<Vec<_>>().join(" AND ");

    // newest matching message per session
    let msg_sql = format!(
        "SELECT m.session_id, s.title, m.id, m.content, m.created_at, r.model_name, s.archived_at IS NOT NULL
         FROM messages m JOIN sessions s ON s.id = m.session_id LEFT JOIN runs r ON r.id = m.run_id
         WHERE m.state <> 'streaming' AND {} {archived}
           AND (?{model_i} IS NULL OR r.model_name = ?{model_i}) AND (?{since_i} IS NULL OR m.created_at >= ?{since_i})
         ORDER BY m.created_at DESC, m.id DESC",
        cond("m.content")
    );
    let mut st = conn.prepare(&msg_sql).map_err(err)?;
    let rows = st
        .query_map(params_from_iter(args.iter()), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, Option<String>>(5)?, r.get::<_, bool>(6)?))
        })
        .map_err(err)?;
    let mut by_message: Vec<ChatHit> = vec![];
    for row in rows {
        let (sid, title, mid, content, at, model, arch) = row.map_err(err)?;
        if by_message.iter().any(|h| h.session_id == sid) {
            continue;
        }
        by_message.push(ChatHit { session_id: sid, title, message_id: Some(mid), snippet: Some(snippet(&content, &ws[0], 28)), at, model, archived: arch });
    }

    let title_sql = format!(
        "SELECT s.id, s.title, s.updated_at, s.archived_at IS NOT NULL FROM sessions s
         WHERE {} {archived}
           AND (?{model_i} IS NULL OR EXISTS (SELECT 1 FROM runs r WHERE r.session_id = s.id AND r.model_name = ?{model_i}))
           AND (?{since_i} IS NULL OR s.updated_at >= ?{since_i})
         ORDER BY s.updated_at DESC, s.id DESC",
        cond("s.title")
    );
    let mut st = conn.prepare(&title_sql).map_err(err)?;
    let titles = st
        .query_map(params_from_iter(args.iter()), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, bool>(3)?)))
        .map_err(err)?;
    let mut out: Vec<ChatHit> = vec![];
    for t in titles {
        let (sid, title, at, arch) = t.map_err(err)?;
        // a title match also shows its best message, when there is one
        match by_message.iter().position(|h| h.session_id == sid) {
            Some(i) => out.push(by_message.remove(i)),
            None => out.push(ChatHit { session_id: sid, title, message_id: None, snippet: None, at, model: None, archived: arch }),
        }
    }
    out.extend(by_message);
    out.truncate(limit);
    Ok(out)
}

/// Meaning hits (search::query, messages and titles) as conversation hits, filtered like `search`.
pub fn similar(conn: &Connection, found: &[crate::search::query::Found], f: &Filter, limit: usize) -> Result<Vec<ChatHit>, String> {
    let mut out: Vec<ChatHit> = vec![];
    for h in found {
        let (session, message) = if h.kind == "message" { (h.parent.unwrap_or_default(), Some(h.id)) } else { (h.id, None) };
        if out.iter().any(|x| x.session_id == session) {
            continue;
        }
        let row = conn
            .query_row(
                "SELECT s.title, s.archived_at IS NOT NULL, coalesce(m.created_at, s.updated_at), r.model_name FROM sessions s
                 LEFT JOIN messages m ON m.id = ?2 AND m.session_id = s.id LEFT JOIN runs r ON r.id = m.run_id WHERE s.id = ?1",
                rusqlite::params![session, message],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?)),
            )
            .optional()
            .map_err(err)?;
        let Some((title, archived, at, model)) = row else { continue };
        if (archived && !f.archived) || f.since.as_ref().is_some_and(|s| &at < s) || f.model.as_ref().is_some_and(|m| model.as_ref() != Some(m)) {
            continue;
        }
        out.push(ChatHit { session_id: session, title, message_id: message, snippet: message.map(|_| h.snippet.clone()), at, model, archived });
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

/// Reply models that appear in history, for the filter.
pub fn models(conn: &Connection) -> Result<Vec<String>, String> {
    let mut st = conn.prepare("SELECT model_name FROM runs GROUP BY model_name ORDER BY max(started_at) DESC").map_err(err)?;
    let rows = st.query_map([], |r| r.get(0)).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// For ⌘K: id = session, parent = the matching message, detail = snippet, day = its date.
pub fn global(conn: &Connection, q: &str) -> Result<Vec<Hit>, String> {
    Ok(search(conn, q, &Filter::default(), PER_SOURCE)?
        .into_iter()
        .map(|h| Hit {
            module: "picky".into(),
            kind: "chat",
            id: h.session_id,
            title: h.title,
            detail: h.snippet,
            parent: h.message_id,
            day: Some(h.at.chars().take(10).collect()),
            amount: None,
            currency: None,
            flow: None,
        })
        .collect())
}

// ---------- commands ----------

#[tauri::command]
pub fn picky_search(state: State<AppState>, q: String, model: Option<String>, since: Option<String>, archived: bool) -> Result<Vec<ChatHit>, String> {
    state.picky.with_db(|c| search(c, &q, &Filter { model, since, archived }, LIMIT))
}

/// 意思相近 in 小拾's search (may start the embedding service).
#[tauri::command]
pub async fn picky_search_similar(app: tauri::AppHandle, q: String, model: Option<String>, since: Option<String>, archived: bool) -> Result<Vec<ChatHit>, String> {
    use crate::search::query::{self, Request, Way};
    let req = Request { text: q, way: Way::Meaning, modules: vec!["picky".into()], kinds: vec![], for_cloud: false, limit: Some(LIMIT * 2) };
    let found = query::search(&app, &req).await?.found;
    app.state::<AppState>().picky.with_db(|c| similar(c, &found, &Filter { model, since, archived }, LIMIT))
}

#[tauri::command]
pub fn picky_search_models(state: State<AppState>) -> Result<Vec<String>, String> {
    state.picky.with_db(|c| models(c))
}

/// Shows the version a hit is on (navigation, not undoable). Returns its session.
#[tauri::command]
pub fn picky_reveal(state: State<AppState>, message_id: i64) -> Result<i64, String> {
    state.picky.with_db(|c| {
        chat::show_path(c, message_id)?;
        c.query_row("SELECT session_id FROM messages WHERE id = ?1", [message_id], |r| r.get(0)).map_err(err)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::chat::{begin_run, begin_version, finish, path, Outcome};
    use crate::picky::library;
    use crate::picky::store::tests::memory_db;

    #[test]
    fn meaning_hits_follow_the_filters_one_per_conversation() {
        use crate::search::query::{Found, Way};
        let c = memory_db();
        c.execute_batch(
            "INSERT INTO sessions (id, title) VALUES (1, '找房'), (2, '旧的');
             UPDATE sessions SET archived_at = '2026-10-01T00:00:00Z' WHERE id = 2;
             INSERT INTO runs (id, session_id, model, model_name, state) VALUES (1, 1, 'local:a', 'A', 'completed');
             INSERT INTO messages (id, session_id, run_id, role, content, state, created_at) VALUES
               (10, 1, NULL, 'user', '租房要注意什么', 'complete', '2026-10-05T00:00:00Z'),
               (11, 1, 1, 'assistant', '押金和合同', 'complete', '2026-10-05T00:01:00Z'),
               (20, 2, NULL, 'user', '搬家', 'complete', '2026-09-01T00:00:00Z');",
        )
        .unwrap();
        let hit = |kind: &'static str, id: i64, parent: Option<i64>| Found { module: "picky".into(), kind, id, title: String::new(), detail: None, parent, day: None, snippet: "片段".into(), score: 1.0, by: Way::Meaning };
        let found = [hit("message", 11, Some(1)), hit("message", 10, Some(1)), hit("message", 20, Some(2)), hit("session", 1, None)];
        let all = similar(&c, &found, &Filter::default(), 10).unwrap();
        assert_eq!(all.iter().map(|h| (h.session_id, h.message_id)).collect::<Vec<_>>(), [(1, Some(11))]); // one per conversation; archived left out
        assert_eq!(all[0].model.as_deref(), Some("A"));
        assert_eq!(similar(&c, &found, &Filter { archived: true, ..Filter::default() }, 10).unwrap().len(), 2);
        assert!(similar(&c, &found, &Filter { model: Some("B".into()), ..Filter::default() }, 10).unwrap().is_empty());
        // since: the messages are older, but the title counts by its last activity (like word search)
        let recent = similar(&c, &found, &Filter { since: Some("2026-10-06".into()), ..Filter::default() }, 10).unwrap();
        assert_eq!(recent.iter().map(|h| (h.session_id, h.message_id)).collect::<Vec<_>>(), [(1, None)]);
    }

    fn done(c: &Connection, b: &chat::Begun, text: &str) {
        finish(c, b.run_id, b.reply_id, &Outcome { content: text.into(), state: "completed", ..Default::default() }).unwrap();
    }

    #[test]
    fn finds_titles_messages_and_older_versions_with_filters() {
        let c = memory_db();
        let a = begin_run(&c, None, None, "向量数据库怎么选", "local:q", "Qwen3 14B").unwrap();
        done(&c, &a, "可以看看 SQLite 的向量扩展。");
        let b = begin_run(&c, None, None, "周末去哪", "local:q", "Qwen3 14B").unwrap();
        done(&c, &b, "去公园，顺便想想向量");
        let v = begin_version(&c, b.reply_id, None, "cloud:d", "DeepSeek Flash", &[], &[]).unwrap();
        done(&c, &v, "去博物馆");

        let hits = search(&c, "向量", &Filter::default(), 40).unwrap();
        assert_eq!(hits.iter().map(|h| h.session_id).collect::<Vec<_>>(), [a.session_id, b.session_id]); // title match first
        assert_eq!(hits[0].message_id, Some(a.reply_id)); // its best message comes along
        // the hit in session b is on the older version
        assert_eq!(hits[1].message_id, Some(b.reply_id));
        assert!(hits[1].snippet.as_deref().unwrap().contains("向量"));
        assert_eq!(path(&c, b.session_id).unwrap()[1].id, v.reply_id);
        chat::show_path(&c, b.reply_id).unwrap();
        assert_eq!(path(&c, b.session_id).unwrap()[1].id, b.reply_id);

        // every word must match; model filter; archived only when asked
        assert_eq!(search(&c, "向量 SQLITE", &Filter::default(), 40).unwrap().len(), 1);
        let ds = Filter { model: Some("DeepSeek Flash".into()), ..Default::default() };
        assert_eq!(search(&c, "博物馆", &ds, 40).unwrap().len(), 1);
        assert!(search(&c, "SQLite", &ds, 40).unwrap().is_empty());
        library::set_archived(&c, &[a.session_id], true).unwrap();
        assert_eq!(search(&c, "SQLite", &Filter::default(), 40).unwrap().len(), 0);
        assert_eq!(search(&c, "SQLite", &Filter { archived: true, ..Default::default() }, 40).unwrap()[0].archived, true);
        let future = Filter { since: Some("2999-01-01".into()), archived: true, ..Default::default() };
        assert!(search(&c, "向量", &future, 40).unwrap().is_empty());
        assert_eq!(models(&c).unwrap().len(), 2);
        let g = global(&c, "博物馆").unwrap();
        assert_eq!((g[0].module.as_str(), g[0].kind, g[0].id, g[0].parent), ("picky", "chat", b.session_id, Some(v.reply_id)));
    }

    #[test]
    fn snippets() {
        assert_eq!(snippet("abc", "b", 28), "abc");
        let long = format!("{}关键{}", "前".repeat(40), "后".repeat(40));
        let s = snippet(&long, "关键", 5);
        assert_eq!(s, format!("…{}关键{}…", "前".repeat(5), "后".repeat(5)));
        assert_eq!(snippet("Hello  \n World", "world", 28), "Hello World");
        assert_eq!(snippet("看看 **SQLite** 的 `向量` 扩展", "sqlite", 28), "看看 SQLite 的 向量 扩展");
    }
}
