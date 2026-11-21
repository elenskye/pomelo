//! 柚子链接 (v1.0 Phase 5): links between 见字如面, 念念不忘 and the works (摸鱼读读, 好奇看看, 玩物有志). All the rules
//! are here; src/shared/links/links.ts mirrors the parsing. A new kind of thing to link to is one row in `KINDS`
//! plus its arm in `Names` (load, resolve, suggest).
//! - Written in a note's or an article's text (an article's code doesn't count):
//!   `[[目标]]` the thing's name with its module's sticker; `[[目标|字]]` those words as a link (柚子链接色, no sticker);
//!   `![[目标]]` (articles) the thing set in: a note as a small card (its whole text and time), anything else as a link.
//! - 目标 = `<模块名>/<名字>`: 念念不忘/<记下的时间，本机时间 YYYY-MM-DD HH:MM；同一分钟有两条时加 :SS，还一样再加毫秒>,
//!   摸鱼读读 / 好奇看看 / 玩物有志/<名称>, 见字如面/<标题>. A bare 标题 is an article (Bear's links; `#小标题` or `/小标题`
//!   after it allowed). Names match case-insensitively; two with the same name → the first added.
//! - Bear's escapes: `\x` before punctuation is x; a name written into a link escapes `\`, `|`, `[`, `]`.
//! - 柚子链接 of a thing (backlinks): the articles (readable, not in the trash) and notes whose text links to or sets
//!   it in, newest first; a note's also has the notes quoting it (`note_refs`). A locked article's links can't be read;
//!   linked to, it shows only its title.
//! - A link to something gone stays as written (the page draws 找不到). Renaming a book / film / game rewrites the
//!   links to it in notes and readable articles, in the same step (unless another one still has the old name).
//!   An article's title and a note's time are not followed (Bear doesn't either).
//! - Hidden modules: `[[` offers nothing of theirs; links to them still show.

use super::common::err;
use super::{articles, notes, tags};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// The kinds of thing a link names: module key, its name in a link.
pub const KINDS: &[(&str, &str)] = &[("articles", "见字如面"), ("notes", "念念不忘"), ("books", "摸鱼读读"), ("films", "好奇看看"), ("games", "玩物有志")];

pub fn label(module: &str) -> &'static str {
    KINDS.iter().find(|(k, _)| *k == module).map(|(_, l)| *l).unwrap_or("")
}

/// A link's target, escapes undone: (module, name). A bare name is an article.
pub fn target_of(raw: &str) -> (&'static str, String) {
    let t = raw.trim();
    for (k, l) in KINDS {
        if let Some(rest) = t.strip_prefix(l).and_then(|r| r.strip_prefix('/')) {
            if !rest.trim().is_empty() {
                return (k, rest.trim().to_string());
            }
        }
    }
    ("articles", t.to_string())
}

/// A name as written in a link.
pub fn escape(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if matches!(c, '\\' | '|' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// What a link to this thing is written as (`[[` puts it in).
pub fn written(module: &str, name: &str) -> String {
    if module == "articles" { escape(name) } else { format!("{}/{}", label(module), escape(name)) }
}

/// One `[[…]]` in a text.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// `![[…]]`.
    pub embed: bool,
    /// The target, escapes undone.
    pub target: String,
    /// The words after `|`.
    pub alias: Option<String>,
    /// Byte span of the target as written (between `[[` and `|` / `]]`).
    pub span: (usize, usize),
}

/// The links in a text, in order (`markdown`: an article's, where code doesn't count).
pub fn found_in(text: &str, markdown: bool) -> Vec<Found> {
    let masked = if markdown { tags::mask_code(text) } else { text.to_string() };
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = masked[at..].find("[[").map(|i| at + i) {
        let Some(j) = masked[i + 2..].find("]]").map(|j| i + 2 + j) else { break };
        let inner = &text[i + 2..j];
        if !inner.contains('\n') {
            // up to the first `|` not escaped; escapes undone
            let (mut target, mut cut, mut k) = (String::new(), inner.len(), 0);
            let b = inner.as_bytes();
            while k < b.len() {
                let c = inner[k..].chars().next().unwrap_or(' ');
                if c == '\\' && inner[k + 1..].chars().next().is_some_and(|n| n.is_ascii_punctuation()) {
                    let n = inner[k + 1..].chars().next().unwrap_or(' ');
                    target.push(n);
                    k += 1 + n.len_utf8();
                    continue;
                }
                if c == '|' {
                    cut = k;
                    break;
                }
                target.push(c);
                k += c.len_utf8();
            }
            let alias = (cut < inner.len()).then(|| inner[cut + 1..].trim().to_string()).filter(|a| !a.is_empty());
            if !target.trim().is_empty() {
                out.push(Found { embed: text[..i].ends_with('!'), target: target.trim().to_string(), alias, span: (i + 2, i + 2 + cut) });
            }
        }
        at = j + 2;
    }
    out
}

/// A text with its links as they read: `[[模块/名字]]` → 名字, `[[目标|字]]` → 字 (excerpts, previews).
pub fn shown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for f in found_in(text, false) {
        let start = f.span.0 - 2 - usize::from(text[..f.span.0 - 2].ends_with('!'));
        let Some(end) = text[f.span.0..].find("]]").map(|j| f.span.0 + j + 2) else { continue };
        if start < last {
            continue;
        }
        out.push_str(&text[last..start]);
        out.push_str(&f.alias.clone().unwrap_or_else(|| target_of(&f.target).1));
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

// ---------- names → things ----------

/// A thing a link names, as the page draws it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thing {
    pub module: &'static str,
    pub id: i64,
    /// An article's title, a note's opening words, a work's name.
    pub title: String,
    /// 作者 / 导演 / 平台 · 状态.
    pub meta: Option<String>,
    /// Local `YYYY-MM-DD HH:MM` (notes: written; articles: last edited).
    pub date: Option<String>,
    /// A note's whole text (a set-in note's card).
    pub text: Option<String>,
    /// A locked article (only its title shows).
    pub locked: bool,
    /// What a link to it is written as.
    pub target: String,
    #[serde(skip)]
    updated: String,
}

/// Everything linkable, loaded once: names → ids.
pub struct Names {
    things: Vec<Thing>,
    /// (module, lowercase name) → index, first added wins.
    by_name: HashMap<(&'static str, String), usize>,
}

const GAME_STATUS: [(&str, &str); 3] = [("todo", "想玩"), ("doing", "在玩"), ("done", "通关")];

impl Names {
    pub fn load(conn: &Connection) -> Result<Self, String> {
        let mut things: Vec<Thing> = Vec::new();
        let mut by_name = HashMap::new();
        let mut put = |things: &mut Vec<Thing>, names: &[String], t: Thing| {
            for n in names {
                by_name.entry((t.module, n.to_lowercase())).or_insert(things.len());
            }
            things.push(t);
        };
        let thing = |module, id, title: String, meta, date, text, locked, target, updated| Thing { module, id, title, meta, date, text, locked, target, updated };
        // articles not in the trash (the first one with a title wins, as articles.rs `names`)
        let mut st = conn
            .prepare("SELECT id, title, sealed IS NOT NULL, strftime('%Y-%m-%d %H:%M', updated_at, 'localtime'), updated_at FROM articles WHERE trashed_at IS NULL ORDER BY created_at, id")
            .map_err(err)?;
        let rows: Vec<(i64, String, bool, String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
        for (id, title, locked, date, updated) in rows {
            let t = thing("articles", id, title.clone(), None, Some(date), None, locked, written("articles", &title), updated);
            put(&mut things, &[title], t);
        }
        // notes: by minute, second and millisecond (local); the first one written owns a shared minute / second
        let mut st = conn
            .prepare(
                "SELECT id, content, strftime('%Y-%m-%d %H:%M', created_at, 'localtime'), strftime('%Y-%m-%d %H:%M:%S', created_at, 'localtime'),
                        strftime('%Y-%m-%d %H:%M:%f', created_at, 'localtime'), created_at FROM notes ORDER BY created_at, id",
            )
            .map_err(err)?;
        let rows: Vec<(i64, String, String, String, String, String)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
        let mut minutes: HashMap<&str, usize> = HashMap::new();
        let mut seconds: HashMap<&str, usize> = HashMap::new();
        for (_, _, m, s, _, _) in &rows {
            *minutes.entry(m.as_str()).or_default() += 1;
            *seconds.entry(s.as_str()).or_default() += 1;
        }
        for (id, content, m, s, ms, created) in &rows {
            let stamp = if minutes[m.as_str()] == 1 { m } else if seconds[s.as_str()] == 1 { s } else { ms };
            let t = thing("notes", *id, notes::excerpt(content), None, Some(m.clone()), Some(content.clone()), false, written("notes", stamp), created.clone());
            put(&mut things, &[m.clone(), s.clone(), ms.clone()], t);
        }
        // works
        for (module, sql) in [
            ("books", "SELECT id, title, author, updated_at FROM books ORDER BY id"),
            ("films", "SELECT id, title, director, updated_at FROM films ORDER BY id"),
            ("games", "SELECT g.id, g.title, trim(coalesce(k.name, '') || ' · ' || g.status, ' ·'), g.updated_at FROM games g LEFT JOIN kinds k ON k.id = g.platform_id ORDER BY g.id"),
        ] {
            let mut st = conn.prepare(sql).map_err(err)?;
            let rows: Vec<(i64, String, Option<String>, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
            for (id, title, meta, updated) in rows {
                let meta = match module {
                    "games" => meta.map(|m| GAME_STATUS.iter().fold(m, |m, (a, b)| if m.ends_with(a) { format!("{}{b}", &m[..m.len() - a.len()]) } else { m })),
                    _ => meta.filter(|m| !m.trim().is_empty()),
                };
                let t = thing(module, id, title.clone(), meta, None, None, false, written(module, &title), updated);
                put(&mut things, &[title], t);
            }
        }
        Ok(Self { things, by_name })
    }

    /// The thing a target names.
    pub fn resolve(&self, target: &str) -> Option<&Thing> {
        let (module, name) = target_of(target);
        let key = |n: &str| self.by_name.get(&(module, n.trim().to_lowercase())).map(|&i| &self.things[i]);
        key(&name).or_else(|| {
            // an article's #小标题 / /小标题
            (module == "articles").then(|| name.rsplit_once('#').and_then(|(a, _)| key(a)).or_else(|| name.rsplit_once('/').and_then(|(a, _)| key(a)))).flatten()
        })
    }

    fn of(&self, module: &str, id: i64) -> Option<&Thing> {
        self.things.iter().find(|t| t.module == module && t.id == id)
    }
}

/// Each target → the thing it names (None: 找不到).
pub fn lookup(conn: &Connection, targets: &[String]) -> Result<Vec<Option<Thing>>, String> {
    let names = Names::load(conn)?;
    Ok(targets.iter().map(|t| names.resolve(t).cloned()).collect())
}

/// A thing (module, id) as a link draws it.
pub fn thing(conn: &Connection, module: &str, id: i64) -> Result<Option<Thing>, String> {
    Ok(Names::load(conn)?.of(module, id).cloned())
}

// ---------- 柚子链接 (backlinks) ----------

/// An article or note linking to a thing.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backlink {
    pub module: &'static str,
    pub id: i64,
    /// An article's title, a note's opening words.
    pub title: String,
    /// Local `YYYY-MM-DD` (articles: last edited; notes: written).
    pub date: String,
}

/// Who links to what: (module, id) → the articles and notes linking to it, newest first.
pub struct Index(HashMap<(&'static str, i64), Vec<(String, Backlink)>>);

impl Index {
    pub fn build(conn: &Connection) -> Result<Self, String> {
        let names = Names::load(conn)?;
        let mut map: HashMap<(&'static str, i64), Vec<(String, Backlink)>> = HashMap::new();
        let mut add = |from: Backlink, sort: String, text: &str, markdown: bool| {
            let mut seen = HashSet::new();
            for f in found_in(text, markdown) {
                let Some(t) = names.resolve(&f.target) else { continue };
                if (t.module, t.id) == (from.module, from.id) || !seen.insert((t.module, t.id)) {
                    continue;
                }
                map.entry((t.module, t.id)).or_default().push((sort.clone(), from.clone()));
            }
        };
        let mut st = conn
            .prepare("SELECT id, title, content, strftime('%Y-%m-%d', updated_at, 'localtime'), updated_at FROM articles WHERE sealed IS NULL AND trashed_at IS NULL")
            .map_err(err)?;
        let rows: Vec<(i64, String, String, String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
        for (id, title, content, date, at) in rows {
            add(Backlink { module: "articles", id, title, date }, at, &content, true);
        }
        let mut st = conn.prepare("SELECT id, content, strftime('%Y-%m-%d', created_at, 'localtime'), created_at FROM notes WHERE content LIKE '%[[%'").map_err(err)?;
        let rows: Vec<(i64, String, String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
        for (id, content, date, at) in rows {
            add(Backlink { module: "notes", id, title: notes::excerpt(&content), date }, at, &content, false);
        }
        // a note quoting another (念念不忘's 引用)
        let mut st = conn
            .prepare("SELECT r.ref_id, n.id, n.content, strftime('%Y-%m-%d', n.created_at, 'localtime'), n.created_at FROM note_refs r JOIN notes n ON n.id = r.note_id")
            .map_err(err)?;
        let rows: Vec<(i64, i64, String, String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
        for (to, id, content, date, at) in rows {
            let v = map.entry(("notes", to)).or_default();
            if !v.iter().any(|(_, b)| b.module == "notes" && b.id == id) {
                v.push((at, Backlink { module: "notes", id, title: notes::excerpt(&content), date }));
            }
        }
        for v in map.values_mut() {
            v.sort_by(|a, b| b.0.cmp(&a.0));
        }
        Ok(Self(map))
    }

    pub fn of(&self, module: &str, id: i64) -> Vec<Backlink> {
        KINDS
            .iter()
            .find(|(k, _)| *k == module)
            .and_then(|(k, _)| self.0.get(&(*k, id)))
            .map(|v| v.iter().map(|(_, b)| b.clone()).collect())
            .unwrap_or_default()
    }
}

pub fn backlinks(conn: &Connection, module: &str, id: i64) -> Result<Vec<Backlink>, String> {
    Ok(Index::build(conn)?.of(module, id))
}

// ---------- [[ : what to offer ----------

/// Up to `limit` things for what's typed after `[[` (all kinds mixed, best match first: the name is it, starts with
/// it, has it; then a note's text or a work's 作者 / 导演 has it; ties: latest first). Nothing typed: the latest.
/// `hidden` modules are left out, and so is `except` (the article or note being written).
pub fn suggest(conn: &Connection, q: &str, hidden: &[String], except: Option<(&str, i64)>, limit: usize) -> Result<Vec<Thing>, String> {
    let names = Names::load(conn)?;
    let words: Vec<String> = q.split_whitespace().map(str::to_lowercase).collect();
    let mut hits: Vec<(u8, &Thing)> = names
        .things
        .iter()
        .filter(|t| !hidden.iter().any(|h| h == t.module))
        .filter(|t| except.is_none_or(|(m, id)| !(t.module == m && t.id == id)))
        .filter_map(|t| {
            if words.is_empty() {
                return Some((0, t));
            }
            let name = t.title.to_lowercase();
            let all = |s: &str| words.iter().all(|w| s.contains(w.as_str()));
            let rank = if name == words.join(" ") {
                0
            } else if name.starts_with(&words[0]) && all(&name) {
                1
            } else if all(&name) {
                2
            } else if all(&format!("{} {} {}", name, t.meta.as_deref().unwrap_or("").to_lowercase(), t.text.as_deref().map(notes::excerpt).unwrap_or_default().to_lowercase())) {
                3
            } else {
                return None;
            };
            Some((rank, t))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.updated.cmp(&a.1.updated)));
    Ok(hits.into_iter().take(limit).map(|(_, t)| t.clone()).collect())
}

// ---------- renaming a work ----------

/// After a book / film / game named `old` became `new`: the links to it in notes and readable articles are rewritten
/// (unless something else of that module still has the old name — the links name that one now).
pub fn follow_rename(conn: &Connection, module: &str, old: &str, new: &str) -> Result<usize, String> {
    if old.trim().eq_ignore_ascii_case(new.trim()) && old.trim() == new.trim() {
        return Ok(0);
    }
    let table = match module {
        "books" | "films" | "games" => module,
        _ => return Ok(0),
    };
    let still: i64 = conn.query_row(&format!("SELECT count(*) FROM {table} WHERE lower(trim(title)) = lower(trim(?1))"), [old], |r| r.get(0)).map_err(err)?;
    if still > 0 {
        return Ok(0);
    }
    let rewrite = |text: &str, markdown: bool| -> Option<String> {
        let mut out = text.to_string();
        let mut changed = false;
        for f in found_in(text, markdown).into_iter().rev() {
            let (m, name) = target_of(&f.target);
            if m == module && name.to_lowercase() == old.trim().to_lowercase() {
                out.replace_range(f.span.0..f.span.1, &written(module, new.trim()));
                changed = true;
            }
        }
        changed.then_some(out)
    };
    let pattern = format!("%[[{}/%", label(module));
    let mut n = 0;
    let mut st = conn.prepare("SELECT id, content FROM notes WHERE content LIKE ?1").map_err(err)?;
    let rows: Vec<(i64, String)> = st.query_map([&pattern], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for (id, content) in rows {
        if let Some(next) = rewrite(&content, false) {
            notes::rewrite(conn, id, &next)?;
            n += 1;
        }
    }
    let mut st = conn.prepare("SELECT id, content FROM articles WHERE sealed IS NULL AND content LIKE ?1").map_err(err)?;
    let rows: Vec<(i64, String)> = st.query_map([&pattern], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for (id, content) in rows {
        if let Some(next) = rewrite(&content, true) {
            articles::rewrite(conn, id, &next)?;
            n += 1;
        }
    }
    Ok(n)
}

// ---------- exports: a set-in note as a quote ----------

/// An article's text with each `![[念念不忘/…]]` line made a quote of that note (its text, then its time), for the
/// Markdown, PDF and Word exports. A note that's gone stays as written.
pub fn quote_embeds(conn: &Connection, text: &str) -> Result<String, String> {
    let found: Vec<Found> = found_in(text, true).into_iter().filter(|f| f.embed && target_of(&f.target).0 == "notes").collect();
    if found.is_empty() {
        return Ok(text.to_string());
    }
    let names = Names::load(conn)?;
    let mut out = text.to_string();
    for f in found.into_iter().rev() {
        let Some(t) = names.resolve(&f.target) else { continue };
        // the whole `![[…]]`, alone on its line
        let (a, b) = (f.span.0 - 3, text[f.span.0..].find("]]").map(|j| f.span.0 + j + 2).unwrap_or(f.span.1));
        let line_start = text[..a].rfind('\n').map_or(0, |i| i + 1);
        let line_end = text[b..].find('\n').map_or(text.len(), |i| b + i);
        if !text[line_start..a].trim().is_empty() || !text[b..line_end].trim().is_empty() {
            continue;
        }
        let mut q: Vec<String> = t.text.as_deref().unwrap_or("").lines().map(|l| if l.trim().is_empty() { ">".to_string() } else { format!("> {l}") }).collect();
        q.push(">".into());
        q.push(format!("> —— 念念不忘 · {}", t.date.as_deref().unwrap_or("")));
        out.replace_range(line_start..line_end, &q.join("\n"));
    }
    Ok(out)
}

// ---------- commands ----------

use crate::AppState;
use tauri::State;

// `async`: asked at every save and at every letter after `[[`, each reading all notes and articles — not on the main
// thread (articles.rs).
#[tauri::command]
pub async fn links_lookup(state: State<'_, AppState>, targets: Vec<String>) -> Result<Vec<Option<Thing>>, String> {
    state.with_db(|c| lookup(c, &targets))
}

#[tauri::command]
pub async fn links_suggest(state: State<'_, AppState>, q: String, hidden: Vec<String>, except: Option<(String, i64)>) -> Result<Vec<Thing>, String> {
    state.with_db(|c| suggest(c, &q, &hidden, except.as_ref().map(|(m, id)| (m.as_str(), *id)), 6))
}

#[tauri::command]
pub async fn links_backlinks(state: State<'_, AppState>, module: String, id: i64) -> Result<Vec<Backlink>, String> {
    state.with_db(|c| backlinks(c, &module, id))
}

/// A note's own target (書架 drags it into an article, 柚子链接 opens it).
#[tauri::command]
pub async fn links_thing(state: State<'_, AppState>, module: String, id: i64) -> Result<Option<Thing>, String> {
    state.with_db(|c| thing(c, &module, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::modules::{books, films, games};
    use serde_json::json;

    fn note(c: &Connection, text: &str) -> notes::Note {
        notes::create(c, serde_json::from_value(json!({"content": text})).unwrap()).unwrap()
    }
    fn article(c: &Connection, text: &str) -> i64 {
        articles::create(c, serde_json::from_value(json!({"content": text})).unwrap()).unwrap().id
    }

    #[test]
    fn parsing() {
        let f = found_in("读 [[摸鱼读读/夜航西飞]] 和 ![[念念不忘/2026-11-02 21:14]]，[[A\\|B|那篇]] `[[不算]]` [[ ]]", true);
        assert_eq!(f.iter().map(|x| (x.embed, x.target.as_str(), x.alias.as_deref())).collect::<Vec<_>>(), [(false, "摸鱼读读/夜航西飞", None), (true, "念念不忘/2026-11-02 21:14", None), (false, "A|B", Some("那篇"))]);
        assert_eq!(found_in("`[[算]]`", false).len(), 1); // a note has no code
        assert_eq!(target_of("玩物有志/风之旅人"), ("games", "风之旅人".into()));
        assert_eq!(target_of("关于慢慢来"), ("articles", "关于慢慢来".into()));
        assert_eq!(target_of("见字如面/关于慢慢来"), ("articles", "关于慢慢来".into()));
        assert_eq!(written("books", "A|B [x]"), "摸鱼读读/A\\|B \\[x\\]");
        assert_eq!(shown("读 [[摸鱼读读/夜航西飞]]，见 ![[念念不忘/2026-11-02 21:14]] 和 [[关于慢慢来|那篇]]"), "读 夜航西飞，见 2026-11-02 21:14 和 那篇");
        let s = "x [[摸鱼读读/旧|别名]] y";
        let f = &found_in(s, false)[0];
        assert_eq!(&s[f.span.0..f.span.1], "摸鱼读读/旧");
    }

    #[test]
    fn resolve_backlinks_and_suggest() {
        let c = memory_db();
        let book = books::create(&c, serde_json::from_value(json!({"title": "夜航西飞", "author": "柏瑞尔·马卡姆"})).unwrap()).unwrap();
        let film = films::create(&c, serde_json::from_value(json!({"title": "夜航西飞", "director": "某人"})).unwrap()).unwrap();
        let game = games::create(&c, serde_json::from_value(json!({"title": "风之旅人", "status": "done"})).unwrap()).unwrap();
        let n1 = note(&c, "地铁上读 [[摸鱼读读/夜航西飞]]：「如果必须离开一个地方」 #读书/摘抄");
        let names = Names::load(&c).unwrap();
        let stamp = names.of("notes", n1.id).unwrap().target.clone();
        assert!(stamp.starts_with("念念不忘/") && stamp.len() == "念念不忘/".len() + 16, "{stamp}");
        let a = article(&c, &format!("# 读《夜航西飞》\n\n![[{stamp}]]\n\n和 [[玩物有志/风之旅人|那段沙漠]] 有点像，[[好奇看看/夜航西飞]]。\n\n```\n[[摸鱼读读/不算]]\n```"));
        let b = article(&c, "# 十月读了什么\n\n见 [[读《夜航西飞》#读到一半]]");
        let n2 = note(&c, "把 [[读《夜航西飞》]] 写完了");

        let names = Names::load(&c).unwrap();
        assert_eq!(names.resolve("摸鱼读读/夜航西飞").map(|t| t.id), Some(book.id));
        assert_eq!(names.resolve("好奇看看/夜航西飞").map(|t| t.id), Some(film.id));
        assert_eq!(names.resolve(&stamp).map(|t| t.id), Some(n1.id));
        assert!(names.resolve("摸鱼读读/没有").is_none());
        assert_eq!(names.resolve("玩物有志/风之旅人").and_then(|t| t.meta.clone()).as_deref(), Some("通关"));

        let ix = Index::build(&c).unwrap();
        let ids = |m: &str, id: i64| ix.of(m, id).iter().map(|b| (b.module, b.id)).collect::<HashSet<_>>();
        assert_eq!(ids("books", book.id), HashSet::from([("notes", n1.id)]));
        assert_eq!(ids("notes", n1.id), HashSet::from([("articles", a)]));
        assert_eq!(ids("games", game.id), HashSet::from([("articles", a)]));
        assert_eq!(ids("articles", a), HashSet::from([("articles", b), ("notes", n2.id)]));
        assert_eq!(ids("films", film.id), HashSet::from([("articles", a)]));

        let s = suggest(&c, "夜航", &[], None, 6).unwrap();
        assert_eq!(s[..2].iter().map(|t| t.module).collect::<HashSet<_>>(), HashSet::from(["books", "films"])); // the name starts with it
        assert!(s.iter().any(|t| t.module == "articles" && t.id == a) && s.iter().any(|t| t.module == "notes" && t.id == n2.id));
        assert!(suggest(&c, "夜航", &["books".to_string()], None, 6).unwrap().iter().all(|t| t.module != "books"));
        assert!(suggest(&c, "", &[], Some(("articles", a)), 6).unwrap().iter().all(|t| !(t.module == "articles" && t.id == a)));
        assert!(suggest(&c, "柏瑞尔", &[], None, 6).unwrap().iter().any(|t| t.id == book.id && t.module == "books")); // by 作者

        // the export quotes a set-in note
        let text = articles::get(&c, a, None).unwrap().content;
        let q = quote_embeds(&c, &text).unwrap();
        assert!(q.contains("> 地铁上读 [[摸鱼读读/夜航西飞]]") && q.contains("> —— 念念不忘 · "), "{q}");
    }

    #[test]
    fn renaming_a_work_follows_it() {
        let c = memory_db();
        let g = games::create(&c, serde_json::from_value(json!({"title": "风之旅人"})).unwrap()).unwrap();
        let n = note(&c, "玩 [[玩物有志/风之旅人]] 和 [[玩物有志/风之旅人|那段]]");
        let a = article(&c, "# 冬天\n\n![[玩物有志/风之旅人]] 和 `[[玩物有志/风之旅人]]`");
        games::update(&c, g.id, serde_json::from_value(json!({"title": "Journey"})).unwrap()).unwrap();
        assert_eq!(notes::get(&c, n.id).unwrap().content, "玩 [[玩物有志/Journey]] 和 [[玩物有志/Journey|那段]]");
        assert_eq!(articles::get(&c, a, None).unwrap().content, "# 冬天\n\n![[玩物有志/Journey]] 和 `[[玩物有志/风之旅人]]`");
        // another one still has the old name: the links are its now
        let g2 = games::create(&c, serde_json::from_value(json!({"title": "Journey"})).unwrap()).unwrap();
        games::update(&c, g2.id, serde_json::from_value(json!({"title": "Journey 2"})).unwrap()).unwrap();
        assert!(notes::get(&c, n.id).unwrap().content.contains("[[玩物有志/Journey]]"));
    }

    #[test]
    fn notes_in_the_same_minute_get_seconds() {
        let c = memory_db();
        let a = note(&c, "一");
        let b = note(&c, "二");
        c.execute("UPDATE notes SET created_at = '2026-11-02T13:14:05.000Z' WHERE id = ?1", [a.id]).unwrap();
        c.execute("UPDATE notes SET created_at = '2026-11-02T13:14:40.000Z' WHERE id = ?1", [b.id]).unwrap();
        let names = Names::load(&c).unwrap();
        let (ta, tb) = (names.of("notes", a.id).unwrap().target.clone(), names.of("notes", b.id).unwrap().target.clone());
        assert!(ta.ends_with(":05") && tb.ends_with(":40"), "{ta} {tb}");
        // the minute alone: the first one written
        let minute = &ta[..ta.len() - 3];
        assert_eq!(names.resolve(minute).map(|t| t.id), Some(a.id));
    }
}
