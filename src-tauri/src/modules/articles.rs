//! 见字如面 (articles, after Bear): longer writing in Markdown (CommonMark + Bear's own: ~underline~,
//! ==highlight== and its colours, callouts, math, Mermaid, folding). Rules:
//! - `content` is the only source of truth; `title` (its first line that isn't only tags, without the marks)
//!   and the tags (tags.rs, Markdown: code doesn't count; `article_tags`) are derived on every write. An
//!   empty article is fine (the page creates one with the first keystroke).
//! - Lists: 全部 = not archived, not in the trash; 置顶 / 没标签的 / a tag view are within that; 归档; 废纸篓
//!   (trashed_at); 加密的 (locked, not in the trash). Pinned first, then last edited first (the trash: last
//!   trashed first). Pinning, archiving, trashing and locking keep updated_at.
//! - 废纸篓: deleting moves an article there (restorable); deleting from there (or emptying it) is for good.
//!   Deleting a tag 「连笔记一起」 moves its articles to the trash.
//! - Locked (加密): the content sealed with 芝麻开门's key (vault.rs `seal_article`), content = ''; title and
//!   tags stay readable (the list shows the title, the tag tree counts it). Reading, writing, locking and
//!   unlocking need the vault open (Touch ID or the master password); renaming / deleting a tag a locked
//!   article uses too. Locked ones are not searched and 柚子 never sees them.
//! - Attachments (attachments.rs, owner 'article'): written into the text by name — `![](名字)` a picture,
//!   `[名字](名字)` a file, the name URL-encoded (space, parentheses, %, #, ?). The page sends the ids the
//!   text uses with each save. A data file carries { name, sha256 }; importing next to a same-bytes file of
//!   another name rewrites the text to that name.
//! - Links: `[[标题]]` (also `[[标题|别名]]`, `[[标题#小标题]]` / `[[标题/小标题]]`) by title, case-insensitive; links to
//!   notes and works and `![[…]]` are links.rs's. 柚子链接 (`backlinks`) = the readable articles not in the trash and the
//!   notes linking here (links.rs).
//! - created_at (UTC, ms) is an article's identity in data files: unique.

pub mod docx;
pub mod export;
pub mod import;

use super::attachments::{self, AttachmentRecord, FileInfo, Owner};
use super::common::{self, err};
use super::tags::{self, TagCount, TagPatch, TagRecord};
use super::vault::{self, Key};
use crate::AppState;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::State;

pub const KEY: &str = "articles";
pub const LABEL: &str = "见字如面";
pub const MAX_CHARS: usize = 500_000;
pub const TITLE_MAX: usize = 200;
const PREVIEW_CHARS: usize = 140;
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";
pub const LOCKED: &str = "这篇加密了：先用 Touch ID 打开芝麻开门";

pub static TABLES: tags::Tables = tags::Tables {
    items: "articles",
    tags: "article_tags",
    fk: "article_id",
    meta: "article_tag_meta",
    live: "n.archived = 0 AND n.trashed_at IS NULL",
    markdown: true,
};

fn gone() -> String {
    "找不到这篇笔记，可能已经被删除了".into()
}

// ---------- text ----------

fn clean_content(s: &str) -> Result<String, String> {
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    if s.chars().count() > MAX_CHARS {
        return Err(format!("笔记太长了（最多 {MAX_CHARS} 个字）"));
    }
    Ok(s)
}

/// A line without Markdown's marks (for titles, previews, 柚子's lists): `[文字](地址)` → 文字,
/// `[[标题|别名]]` → 别名, `[[摸鱼读读/名字]]` → 名字, pictures and icons dropped; **, __, ==, ~~, `, *, _, ~ removed.
pub fn strip_marks(line: &str) -> String {
    let mut s = super::notes::without_icons(line);
    // pictures, then links (text kept)
    while let Some(i) = s.find("![") {
        let Some(j) = s[i..].find("](").map(|j| i + j) else { break };
        let Some(k) = s[j..].find(')').map(|k| j + k) else { break };
        s.replace_range(i..=k, "");
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    loop {
        if let Some(i) = rest.find("[[") {
            if let Some(j) = rest[i..].find("]]").map(|j| i + j) {
                out.push_str(rest[..i].strip_suffix('!').unwrap_or(&rest[..i]));
                let inner = &rest[i + 2..j];
                // [[标题|别名]] → 别名; [[摸鱼读读/夜航西飞]] → 夜航西飞 (links.rs)
                match inner.split_once('|') {
                    Some((_, alias)) if !alias.trim().is_empty() => out.push_str(alias),
                    _ => out.push_str(&super::links::target_of(inner).1),
                }
                rest = &rest[j + 2..];
                continue;
            }
        }
        break;
    }
    out.push_str(rest);
    let mut s = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(i) = rest.find('[') {
        match (rest[i..].find("](").map(|j| i + j), rest[i..].find(']').map(|j| i + j)) {
            (Some(j), Some(b)) if b == j => match rest[j..].find(')').map(|k| j + k) {
                Some(k) => {
                    s.push_str(&rest[..i]);
                    s.push_str(&rest[i + 1..j]);
                    rest = &rest[k + 1..];
                }
                None => break,
            },
            _ => {
                s.push_str(&rest[..=i]);
                rest = &rest[i + 1..];
            }
        }
    }
    s.push_str(rest);
    let s = s.replace("**", "").replace("__", "").replace("==", "").replace("~~", "");
    // single marks only where they hug a word (a lone * or ~ in prose stays)
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        if matches!(c, '`' | '*' | '~') || (c == '_' && !(i > 0 && chars[i - 1].is_alphanumeric() && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric()))) {
            let before = i > 0 && !chars[i - 1].is_whitespace();
            let after = chars.get(i + 1).is_some_and(|n| !n.is_whitespace());
            if before || after {
                continue;
            }
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// The line without Bear's mark that it's folded (`<!-- {"fold":true} -->` at a heading's or list item's end). The
/// mark stays in the text (搬家 brings it as is); the page opens such a line folded; PDF and Word leave it out.
pub fn without_fold(line: &str) -> &str {
    let t = line.trim_end();
    if let Some(body) = t.strip_suffix("-->") {
        if let Some(i) = body.rfind("<!--") {
            if body[i + 4..].trim() == "{\"fold\":true}" {
                return t[..i].trim_end();
            }
        }
    }
    line
}

/// A callout's mark at the start of a quote's text (`[!NOTE]`, `[!TIP]-` …) taken off: the page draws it as an icon.
fn without_callout(t: &str) -> &str {
    let Some(rest) = t.strip_prefix("[!") else { return t };
    let Some(end) = rest.find(']') else { return t };
    if !["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"].iter().any(|k| rest[..end].eq_ignore_ascii_case(k)) {
        return t;
    }
    rest[end + 1..].trim_start_matches(['+', '-']).trim_start()
}

/// A line's own text: heading / list / quote / task markers (a callout's `[!NOTE]` too) and the tags taken off, marks
/// stripped.
fn line_text(line: &str) -> String {
    let mut t = without_fold(line).trim_start();
    let whole = t;
    loop {
        let before = t;
        t = t.trim_start_matches('>').trim_start();
        if before == t {
            break;
        }
    }
    if t.len() != whole.len() {
        t = without_callout(t);
    }
    let hashes = t.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && t[hashes..].starts_with([' ', '\t']) {
        t = t[hashes..].trim_start();
    }
    for m in ["- [ ] ", "- [x] ", "- [X] ", "* [ ] ", "* [x] ", "- ", "* ", "+ "] {
        if let Some(r) = t.strip_prefix(m) {
            t = r;
            break;
        }
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        if let Some(r) = t[digits..].strip_prefix(". ").or_else(|| t[digits..].strip_prefix(") ")) {
            t = r;
        }
    }
    let mut plain = String::new();
    let mut last = 0;
    for (a, b, _) in tags::tag_spans(t, true) {
        plain.push_str(&t[last..a - 1]);
        last = b;
    }
    plain.push_str(&t[last..]);
    strip_marks(&plain).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// The title: the first line with words of its own (not only tags), without the marks; at most 200 chars.
pub fn title_of(content: &str) -> String {
    let (_, title) = title_line(content);
    title
}

/// (index of the title line, title).
fn title_line(content: &str) -> (Option<usize>, String) {
    let mut fence = false;
    for (i, line) in content.lines().enumerate() {
        if is_fence(line) {
            fence = !fence;
            continue;
        }
        if fence || line.trim().is_empty() {
            continue;
        }
        let t = line_text(line);
        if !t.is_empty() {
            return (Some(i), t.chars().take(TITLE_MAX).collect());
        }
    }
    (None, String::new())
}

/// Two lines' worth of plain text after the title (code kept as text, fences and rules dropped).
pub fn preview_of(content: &str) -> String {
    let (at, _) = title_line(content);
    let mut words = Vec::new();
    let mut n = 0;
    for (i, line) in content.lines().enumerate() {
        if Some(i) == at || is_fence(line) || line.trim().trim_matches(['-', '*', '_', ' ']).is_empty() {
            continue;
        }
        let t = line_text(line);
        if t.is_empty() {
            continue;
        }
        n += t.chars().count() + 1;
        words.push(t);
        if n > PREVIEW_CHARS {
            break;
        }
    }
    let s = words.join(" ");
    if s.chars().count() > PREVIEW_CHARS {
        s.chars().take(PREVIEW_CHARS).collect::<String>() + "…"
    } else {
        s
    }
}

/// The whole text as plain lines (柚子 reads it; marks off, structure kept).
pub fn plain_text(content: &str) -> String {
    content.lines().map(|l| if is_fence(l) { String::new() } else { line_text(l) }).collect::<Vec<_>>().join("\n").trim().to_string()
}

/// A `[[link]]`'s target: up to the first `|` not escaped, Bear's `\x` escapes undone (`[[A\&B]]` → A&B).
fn wiki_target(inner: &str) -> String {
    let mut out = String::new();
    let mut it = inner.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' if it.peek().is_some_and(char::is_ascii_punctuation) => out.push(it.next().unwrap_or(c)),
            '|' => break,
            c => out.push(c),
        }
    }
    out
}

/// The targets of `[[…]]` links in the text (outside code), alias dropped, escapes undone.
pub fn links_in(content: &str) -> Vec<String> {
    let masked = tags::mask_code(content);
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = masked[at..].find("[[").map(|i| at + i) {
        let Some(j) = masked[i + 2..].find("]]").map(|j| i + 2 + j) else { break };
        let inner = &content[i + 2..j];
        if !inner.contains('\n') {
            let target = wiki_target(inner);
            let target = target.trim();
            if !target.is_empty() {
                out.push(target.to_string());
            }
        }
        at = j + 2;
    }
    out
}

/// A link target names this title (case-insensitive; `#小标题` or `/小标题` after it allowed).
pub(crate) fn names(target: &str, title: &str) -> bool {
    let t = target.trim().to_lowercase();
    let title = title.trim().to_lowercase();
    if title.is_empty() {
        return false;
    }
    t == title || t.rsplit_once('#').is_some_and(|(a, _)| a.trim() == title) || t.rsplit_once('/').is_some_and(|(a, _)| a.trim() == title)
}

/// An attachment's name as written in a link (what the page writes too).
pub fn encode_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            _ => out.push(c),
        }
    }
    out
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

// ---------- rows ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: i64,
    pub title: String,
    /// Plain text after the title (empty when locked).
    pub preview: String,
    pub pinned: bool,
    pub archived: bool,
    pub trashed: bool,
    pub locked: bool,
    pub tags: Vec<String>,
    /// The first picture the text shows (attachment id), for the list.
    pub thumb: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Article {
    pub id: i64,
    pub title: String,
    /// The Markdown ('' while locked and not opened).
    pub content: String,
    pub pinned: bool,
    pub archived: bool,
    pub trashed_at: Option<String>,
    pub locked: bool,
    /// The content is here (not locked, or opened with the vault key).
    pub open: bool,
    pub tags: Vec<String>,
    pub attachments: Vec<FileInfo>,
    /// 柚子链接: articles and notes linking here, newest first (links.rs).
    pub backlinks: Vec<super::links::Backlink>,
    pub created_at: String,
    pub updated_at: String,
}

struct Row {
    id: i64,
    title: String,
    content: String,
    sealed: Option<Vec<u8>>,
    pinned: bool,
    archived: bool,
    trashed_at: Option<String>,
    created_at: String,
    updated_at: String,
}

const COLS: &str = "a.id, a.title, a.content, a.sealed, a.pinned, a.archived, a.trashed_at, a.created_at, a.updated_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Row> {
    Ok(Row {
        id: r.get(0)?,
        title: r.get(1)?,
        content: r.get(2)?,
        sealed: r.get(3)?,
        pinned: r.get(4)?,
        archived: r.get(5)?,
        trashed_at: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

fn load(conn: &Connection, id: i64) -> Result<Row, String> {
    conn.query_row(&format!("SELECT {COLS} FROM articles a WHERE a.id = ?1"), [id], row).optional().map_err(err)?.ok_or_else(gone)
}

fn tags_of(conn: &Connection) -> Result<HashMap<i64, Vec<String>>, String> {
    let mut st = conn.prepare("SELECT article_id, tag FROM article_tags ORDER BY article_id, tag").map_err(err)?;
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for r in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(err)? {
        let (id, t) = r.map_err(err)?;
        out.entry(id).or_default().push(t);
    }
    Ok(out)
}

/// Tags in the order the text has them (a locked one: as stored).
fn ordered_tags(r: &Row, stored: Vec<String>) -> Vec<String> {
    if r.sealed.is_some() {
        stored
    } else {
        tags::tags_in(&r.content, true)
    }
}

/// The first picture the text shows, among the article's files.
fn thumb_of(content: &str, files: &[FileInfo]) -> Option<i64> {
    let mut best: Option<(usize, i64)> = None;
    for f in files.iter().filter(|f| f.kind == "image" && !f.missing) {
        if let Some(at) = content.find(&format!("]({})", encode_name(&f.name))) {
            if best.is_none_or(|(b, _)| at < b) {
                best = Some((at, f.id));
            }
        }
    }
    best.map(|(_, id)| id)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filter {
    /// all (default) | pinned | untagged | archived | trash | locked
    #[serde(default)]
    pub view: Option<String>,
    /// Includes sub-tags (within 全部).
    #[serde(default)]
    pub tag: Option<String>,
    /// Words (space-separated), all must appear in the title or the text.
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub id: Option<i64>,
    /// Local dates last edited, inclusive (柚子).
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
}

pub fn list(conn: &Connection, f: &Filter) -> Result<Vec<Summary>, String> {
    let mut conds: Vec<String> = Vec::new();
    let mut args: Vec<Value> = Vec::new();
    let mut arg = |v: Value| {
        args.push(v);
        format!("?{}", args.len())
    };
    let view = f.view.as_deref().unwrap_or("all");
    let live = "a.archived = 0 AND a.trashed_at IS NULL";
    match view {
        "all" => conds.push(live.into()),
        "pinned" => conds.push(format!("{live} AND a.pinned = 1")),
        "untagged" => conds.push(format!("{live} AND NOT EXISTS (SELECT 1 FROM article_tags t WHERE t.article_id = a.id)")),
        "archived" => conds.push("a.archived = 1 AND a.trashed_at IS NULL".into()),
        "trash" => conds.push("a.trashed_at IS NOT NULL".into()),
        "locked" => conds.push("a.sealed IS NOT NULL AND a.trashed_at IS NULL".into()),
        "any" => {}
        v => return Err(format!("不认识的列表：{v}")),
    }
    if let Some(t) = common::clean(f.tag.clone()) {
        let p = arg(Value::Text(t.trim_start_matches('#').to_string()));
        conds.push(format!("EXISTS (SELECT 1 FROM article_tags t WHERE t.article_id = a.id AND (t.tag = {p} OR substr(t.tag, 1, length({p}) + 1) = {p} || '/'))"));
    }
    for w in f.q.as_deref().unwrap_or("").split_whitespace() {
        let p = arg(Value::Text(w.to_string()));
        conds.push(format!("(instr(lower(a.title), lower({p})) > 0 OR instr(lower(a.content), lower({p})) > 0)"));
    }
    if let Some(id) = f.id {
        let p = arg(Value::Integer(id));
        conds.push(format!("a.id = {p}"));
    }
    if let Some(d) = common::clean(f.from.clone()) {
        let p = arg(Value::Text(d));
        conds.push(format!("date(a.updated_at, 'localtime') >= {p}"));
    }
    if let Some(d) = common::clean(f.to.clone()) {
        let p = arg(Value::Text(d));
        conds.push(format!("date(a.updated_at, 'localtime') <= {p}"));
    }
    let order = if view == "trash" { "a.trashed_at DESC, a.id DESC" } else { "a.pinned DESC, a.updated_at DESC, a.id DESC" };
    let filter = if conds.is_empty() { "1 = 1".to_string() } else { conds.join(" AND ") };
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM articles a WHERE {filter} ORDER BY {order}")).map_err(err)?;
    let rows = st.query_map(params_from_iter(args), row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let mut tags = tags_of(conn)?;
    let files = attachments::all_of(conn, Owner::Article)?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let stored = tags.remove(&r.id).unwrap_or_default();
            Summary {
                id: r.id,
                preview: if r.sealed.is_some() { String::new() } else { preview_of(&r.content) },
                thumb: if r.sealed.is_some() { None } else { thumb_of(&r.content, files.get(&r.id).map(Vec::as_slice).unwrap_or_default()) },
                tags: ordered_tags(&r, stored),
                title: r.title,
                pinned: r.pinned,
                archived: r.archived,
                trashed: r.trashed_at.is_some(),
                locked: r.sealed.is_some(),
                created_at: r.created_at,
                updated_at: r.updated_at,
            }
        })
        .collect())
}

/// Readable articles (not locked) not in the trash, with their text: for links, 柚子, the year's summary.
pub fn readable(conn: &Connection) -> Result<Vec<(i64, String, String)>, String> {
    let mut st = conn.prepare("SELECT id, title, content FROM articles WHERE sealed IS NULL AND trashed_at IS NULL ORDER BY updated_at DESC, id DESC").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    Ok(rows)
}


fn assemble(conn: &Connection, r: Row, content: Option<String>) -> Result<Article, String> {
    let stored = tags_of(conn)?.remove(&r.id).unwrap_or_default();
    let tags = match &content {
        Some(c) => tags::tags_in(c, true),
        None => stored,
    };
    let open = content.is_some();
    Ok(Article {
        id: r.id,
        backlinks: super::links::backlinks(conn, KEY, r.id)?,
        attachments: attachments::of(conn, Owner::Article, r.id)?,
        content: content.unwrap_or_default(),
        title: r.title,
        pinned: r.pinned,
        archived: r.archived,
        trashed_at: r.trashed_at,
        locked: r.sealed.is_some(),
        open,
        tags,
        created_at: r.created_at,
        updated_at: r.updated_at,
    })
}

/// An article as the page shows it; a locked one is opened when `key` is given (else its content is '').
pub fn get(conn: &Connection, id: i64, key: Option<&Key>) -> Result<Article, String> {
    let r = load(conn, id)?;
    let content = match (&r.sealed, key) {
        (None, _) => Some(r.content.clone()),
        (Some(blob), Some(k)) => Some(vault::unseal_article(k, blob)?),
        (Some(_), None) => None,
    };
    assemble(conn, r, content)
}

/// Titles of the articles not in the trash (for `[[` and opening a link), last edited first.
pub fn titles(conn: &Connection) -> Result<Vec<Link>, String> {
    let mut st = conn.prepare("SELECT id, title FROM articles WHERE trashed_at IS NULL AND title <> '' ORDER BY updated_at DESC, id DESC").map_err(err)?;
    let rows = st.query_map([], |r| Ok(Link { id: r.get(0)?, title: r.get(1)? })).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    Ok(rows)
}

// ---------- writing ----------

/// Now, nudged by 1 ms until no article has it (created_at is unique).
fn fresh_stamp(conn: &Connection) -> Result<String, String> {
    let s: String = conn.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0)).map_err(err)?;
    free_stamp(conn, s)
}

/// A time as articles keep it (UTC, ms: 2026-11-13T06:41:08.000Z) from a form SQLite reads (`…Z`, `…+08:00`).
pub(crate) fn stamp_of(conn: &Connection, s: &str) -> Option<String> {
    conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', ?1)", [s], |r| r.get::<_, Option<String>>(0)).ok().flatten()
}

/// `s`, nudged by 1 ms until no article has it.
fn free_stamp(conn: &Connection, mut s: String) -> Result<String, String> {
    for _ in 0..10_000 {
        let taken: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM articles WHERE created_at = ?1)", [&s], |r| r.get(0)).map_err(err)?;
        if !taken {
            return Ok(s);
        }
        s = conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', ?1, '+0.001 seconds')", [&s], |r| r.get(0)).map_err(err)?;
    }
    Err("时间冲突，请稍后再试".into())
}

/// A new text for a readable article (links.rs follows a renamed work).
pub(crate) fn rewrite(conn: &Connection, id: i64, text: &str) -> Result<(), String> {
    put_text(conn, id, text, None)
}

/// Writes a new text (title, tags, updated_at follow); a locked article is sealed again with `key`.
fn put_text(conn: &Connection, id: i64, text: &str, key: Option<&Key>) -> Result<(), String> {
    let text = clean_content(text)?;
    let r = load(conn, id)?;
    let title = title_of(&text);
    match &r.sealed {
        None => conn.execute(&format!("UPDATE articles SET content = ?1, title = ?2, updated_at = {NOW} WHERE id = ?3"), params![text, title, id]),
        Some(_) => {
            let k = key.ok_or_else(|| LOCKED.to_string())?;
            conn.execute(&format!("UPDATE articles SET sealed = ?1, title = ?2, updated_at = {NOW} WHERE id = ?3"), params![vault::seal_article(k, &text), title, id])
        }
    }
    .map_err(err)?;
    tags::sync(conn, &TABLES, id, &tags::tags_in(&text, true))
}

/// How a tag's rename / delete reads, rewrites and removes articles (`key`: the vault's, for locked ones).
struct ArticleItems<'a> {
    key: Option<&'a Key>,
}
impl tags::Items for ArticleItems<'_> {
    fn tables(&self) -> &'static tags::Tables {
        &TABLES
    }
    fn text(&self, conn: &Connection, id: i64) -> Result<String, String> {
        let r = load(conn, id)?;
        match (&r.sealed, self.key) {
            (None, _) => Ok(r.content),
            (Some(blob), Some(k)) => vault::unseal_article(k, blob),
            (Some(_), None) => Err(format!("「{}」加密了：先用 Touch ID 打开芝麻开门再改标签", r.title)),
        }
    }
    fn valid(&self, _: &str) -> bool {
        true
    }
    fn write(&self, conn: &Connection, id: i64, text: &str) -> Result<(), String> {
        put_text(conn, id, text, self.key)
    }
    fn remove(&self, conn: &Connection, ids: &[i64]) -> Result<(), String> {
        for id in ids {
            conn.execute(&format!("UPDATE articles SET trashed_at = {NOW} WHERE id = ?1 AND trashed_at IS NULL"), [id]).map_err(err)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Patch {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub archived: Option<bool>,
    /// Attachment file ids the text uses, in order (replaces the list).
    #[serde(default)]
    pub attachments: Option<Vec<i64>>,
}

pub fn create(conn: &Connection, p: Patch) -> Result<Article, String> {
    let content = clean_content(p.content.as_deref().unwrap_or(""))?;
    let files = attachments::clean_ids(conn, &p.attachments.unwrap_or_default())?;
    let stamp = fresh_stamp(conn)?;
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        "INSERT INTO articles (title, content, pinned, archived, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![title_of(&content), content, p.pinned.unwrap_or(false), p.archived.unwrap_or(false), stamp],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    tags::sync(&tx, &TABLES, id, &tags::tags_in(&content, true))?;
    attachments::set(&tx, Owner::Article, id, &files)?;
    tx.commit()?;
    get(conn, id, None)
}

/// An article written elsewhere (imports): its own times (missing or unreadable = now; a taken created_at is
/// nudged by 1 ms) and states, the files its text uses. The caller owns the transaction.
#[allow(clippy::too_many_arguments)]
pub(crate) fn insert(conn: &Connection, content: &str, created: Option<&str>, updated: Option<&str>, pinned: bool, archived: bool, trashed_at: Option<&str>, files: &[i64]) -> Result<i64, String> {
    let content = clean_content(content)?;
    let stamp = match created.and_then(|s| stamp_of(conn, s)) {
        Some(s) => free_stamp(conn, s)?,
        None => fresh_stamp(conn)?,
    };
    let updated = updated.and_then(|s| stamp_of(conn, s)).filter(|u| *u >= stamp).unwrap_or_else(|| stamp.clone());
    let trashed = trashed_at.and_then(|s| stamp_of(conn, s));
    conn.execute(
        "INSERT INTO articles (title, content, pinned, archived, trashed_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![title_of(&content), content, pinned, archived, trashed, stamp, updated],
    )
    .map_err(err)?;
    let id = conn.last_insert_rowid();
    tags::sync(conn, &TABLES, id, &tags::tags_in(&content, true))?;
    attachments::set(conn, Owner::Article, id, files)?;
    Ok(id)
}

/// Text and files are an edit (updated_at); pinning and archiving alone are not. A locked article's text needs `key`.
pub fn update(conn: &Connection, id: i64, p: Patch, key: Option<&Key>) -> Result<Article, String> {
    let r = load(conn, id)?;
    let tx = common::Tx::begin(conn)?;
    if let Some(c) = &p.content {
        let old = match (&r.sealed, key) {
            (None, _) => r.content.clone(),
            (Some(blob), Some(k)) => vault::unseal_article(k, blob)?,
            (Some(_), None) => return Err(LOCKED.into()),
        };
        if *c != old {
            put_text(&tx, id, c, key)?;
        }
    }
    if let Some(files) = &p.attachments {
        let files = attachments::clean_ids(&tx, files)?;
        if attachments::of(&tx, Owner::Article, id)?.iter().map(|f| f.id).collect::<Vec<_>>() != files {
            attachments::set(&tx, Owner::Article, id, &files)?;
            tx.execute(&format!("UPDATE articles SET updated_at = {NOW} WHERE id = ?1"), [id]).map_err(err)?;
        }
    }
    if let Some(v) = p.pinned {
        tx.execute("UPDATE articles SET pinned = ?1 WHERE id = ?2 AND pinned <> ?1", params![v, id]).map_err(err)?;
    }
    if let Some(v) = p.archived {
        tx.execute("UPDATE articles SET archived = ?1 WHERE id = ?2 AND archived <> ?1", params![v, id]).map_err(err)?;
    }
    tx.commit()?;
    get(conn, id, key)
}

/// To the trash (restorable).
pub fn trash(conn: &Connection, id: i64) -> Result<(), String> {
    load(conn, id)?;
    conn.execute(&format!("UPDATE articles SET trashed_at = {NOW} WHERE id = ?1 AND trashed_at IS NULL"), [id]).map_err(err)?;
    Ok(())
}

pub fn restore(conn: &Connection, id: i64) -> Result<(), String> {
    load(conn, id)?;
    conn.execute("UPDATE articles SET trashed_at = NULL WHERE id = ?1", [id]).map_err(err)?;
    Ok(())
}

/// For good (its links to files go too).
pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    if conn.execute("DELETE FROM articles WHERE id = ?1", [id]).map_err(err)? == 0 {
        return Err(gone());
    }
    attachments::prune(conn)
}

/// Empties the trash; returns how many went.
pub fn empty_trash(conn: &Connection) -> Result<usize, String> {
    let n = conn.execute("DELETE FROM articles WHERE trashed_at IS NOT NULL", []).map_err(err)?;
    attachments::prune(conn)?;
    Ok(n)
}

/// Locks (seals the text with the vault key) or unlocks (back to plain text) an article.
pub fn set_locked(conn: &Connection, id: i64, lock: bool, key: &Key) -> Result<Article, String> {
    let r = load(conn, id)?;
    match (&r.sealed, lock) {
        (None, true) => {
            conn.execute("UPDATE articles SET sealed = ?1, content = '' WHERE id = ?2", params![vault::seal_article(key, &r.content), id]).map_err(err)?;
        }
        (Some(blob), false) => {
            let text = vault::unseal_article(key, blob)?;
            conn.execute("UPDATE articles SET content = ?1, sealed = NULL WHERE id = ?2", params![text, id]).map_err(err)?;
        }
        _ => {}
    }
    get(conn, id, Some(key))
}

// ---------- tags (tags.rs) ----------

pub fn rename_tag(conn: &Connection, from: &str, to: &str, key: Option<&Key>) -> Result<usize, String> {
    tags::rename(conn, &ArticleItems { key }, from, to)
}
/// (articles changed or moved to the trash, 0).
pub fn delete_tag(conn: &Connection, tag: &str, with_articles: bool, key: Option<&Key>) -> Result<(usize, usize), String> {
    tags::delete(conn, &ArticleItems { key }, tag, with_articles)
}
pub fn place_tag(conn: &Connection, tag: &str, parent: Option<&str>, order: &[String], key: Option<&Key>) -> Result<usize, String> {
    tags::place(conn, &ArticleItems { key }, tag, parent, order)
}

// ---------- overview (left column) ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub all: i64,
    pub pinned: i64,
    pub untagged: i64,
    pub archived: i64,
    pub trash: i64,
    pub locked: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub counts: Counts,
    pub tags: Vec<TagCount>,
    /// Pinned (first-level) tags in use, in pin order.
    pub pinned: Vec<String>,
}

pub fn overview(conn: &Connection) -> Result<Overview, String> {
    let n = |cond: &str| -> Result<i64, String> { conn.query_row(&format!("SELECT count(*) FROM articles a WHERE {cond}"), [], |r| r.get(0)).map_err(err) };
    let live = "a.archived = 0 AND a.trashed_at IS NULL";
    let counts = Counts {
        all: n(live)?,
        pinned: n(&format!("{live} AND a.pinned = 1"))?,
        untagged: n(&format!("{live} AND NOT EXISTS (SELECT 1 FROM article_tags t WHERE t.article_id = a.id)"))?,
        archived: n("a.archived = 1 AND a.trashed_at IS NULL")?,
        trash: n("a.trashed_at IS NOT NULL")?,
        locked: n("a.sealed IS NOT NULL AND a.trashed_at IS NULL")?,
    };
    let (tags, pinned) = tags::tree(conn, &TABLES)?;
    Ok(Overview { counts, tags, pinned })
}

// ---------- import / export: { articles: [{ content, sealed?, title?, tags?, pinned?, archived?, trashedAt?,
//            attachments?: [{ name, sha256 }], createdAt, updatedAt }], tags?: [{ tag, pinned?, icon?, order? }] } ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArticleRecord {
    /// '' when locked.
    #[serde(default)]
    pub content: String,
    /// Locked: the sealed text (hex), and the title and tags it had.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trashed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentRecord>,
    /// Missing = now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArticlesRecord {
    #[serde(default)]
    pub articles: Vec<ArticleRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagRecord>,
}

impl ArticlesRecord {
    pub fn len(&self) -> usize {
        self.articles.len()
    }
}

pub fn validate_records(rec: &ArticlesRecord) -> Result<(), String> {
    let mut stamps = HashSet::new();
    for (i, a) in rec.articles.iter().enumerate() {
        let ctx = |e: String| format!("{LABEL}第 {} 篇：{e}", i + 1);
        clean_content(&a.content).map_err(ctx)?;
        attachments::check_records(&a.attachments).map_err(ctx)?;
        for s in [&a.created_at, &a.updated_at, &a.trashed_at].into_iter().flatten() {
            super::notes::check_stamp(s).map_err(ctx)?;
        }
        if let Some(s) = &a.created_at {
            if !stamps.insert(s.as_str()) {
                return Err(ctx(format!("和前面一篇的时间相同（{s}）")));
            }
        }
        match &a.sealed {
            Some(h) => {
                if !a.content.is_empty() || unhex(h).is_none_or(|b| b.len() < 25) {
                    return Err(ctx("加密的内容不对".into()));
                }
                if a.title.as_ref().is_some_and(|t| t.chars().count() > TITLE_MAX) {
                    return Err(ctx("标题太长了".into()));
                }
                for t in &a.tags {
                    tags::clean_tag(t).map_err(ctx)?;
                }
            }
            None if a.title.is_some() || !a.tags.is_empty() => return Err(ctx("没加密的不用写标题和标签（从正文里算）".into())),
            None => {}
        }
    }
    tags::check_records(LABEL, &rec.tags)
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM articles", [], |r| r.get(0)).map_err(err)
}

/// Replaces all articles. Caller owns the transaction.
pub fn replace_all(conn: &Connection, rec: &ArticlesRecord) -> Result<usize, String> {
    validate_records(rec)?;
    conn.execute("DELETE FROM articles", []).map_err(err)?;
    attachments::prune(conn)?;
    for a in &rec.articles {
        let stamp = match &a.created_at {
            Some(s) => s.clone(),
            None => fresh_stamp(conn)?,
        };
        let files = attachments::import_records(conn, &a.attachments)?;
        let mut content = a.content.clone();
        // a same-bytes file already here under another name: the text follows its name
        for (r, id) in a.attachments.iter().zip(&files) {
            let name: String = conn.query_row("SELECT name FROM attachment_files WHERE id = ?1", [id], |x| x.get(0)).map_err(err)?;
            if name != r.name {
                content = content.replace(&format!("]({})", encode_name(&r.name)), &format!("]({})", encode_name(&name)));
            }
        }
        let sealed = a.sealed.as_deref().and_then(unhex);
        let (title, tag_list) = match &sealed {
            Some(_) => (a.title.clone().unwrap_or_default(), a.tags.clone()),
            None => (title_of(&content), tags::tags_in(&content, true)),
        };
        conn.execute(
            "INSERT INTO articles (title, content, sealed, pinned, archived, trashed_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, coalesce(?8, ?7))",
            params![title, content, sealed, a.pinned, a.archived, a.trashed_at, stamp, a.updated_at],
        )
        .map_err(|e| format!("{LABEL}：{e}"))?;
        let id = conn.last_insert_rowid();
        tags::sync(conn, &TABLES, id, &tag_list)?;
        attachments::set(conn, Owner::Article, id, &files)?;
    }
    tags::import_records(conn, &TABLES, &rec.tags)?;
    Ok(rec.len())
}

/// Oldest first.
pub fn export(conn: &Connection) -> Result<ArticlesRecord, String> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM articles a ORDER BY a.created_at")).map_err(err)?;
    let rows = st.query_map([], row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let mut tags = tags_of(conn)?;
    let files = attachments::all_of(conn, Owner::Article)?;
    let articles = rows
        .into_iter()
        .map(|r| {
            let stored = tags.remove(&r.id).unwrap_or_default();
            let locked = r.sealed.is_some();
            Ok(ArticleRecord {
                attachments: attachments::records_of(files.get(&r.id).map(Vec::as_slice).unwrap_or_default(), conn)?,
                sealed: r.sealed.as_deref().map(hex),
                title: locked.then_some(r.title),
                tags: if locked { stored } else { vec![] },
                content: r.content,
                pinned: r.pinned,
                archived: r.archived,
                trashed_at: r.trashed_at,
                created_at: Some(r.created_at),
                updated_at: Some(r.updated_at),
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(ArticlesRecord { articles, tags: tags::export_records(conn, &TABLES)? })
}

// ---------- commands ----------

/// The vault key when 芝麻开门 is open (no prompt; counts as use).
fn open_key(state: &AppState) -> Option<Key> {
    state.vault_key().ok()
}

// The commands the page calls while I write (each save, and the lists and links it refreshes) are `async`: they run
// off the main thread, which hands the window its keys and wheel events — on it they held those up (a two-finger swipe
// right after typing came late and in lumps). The page sends its saves one after another (Editor.tsx).
#[tauri::command]
pub async fn articles_list(state: State<'_, AppState>, filter: Filter) -> Result<Vec<Summary>, String> {
    state.with_db(|c| list(c, &filter))
}
#[tauri::command]
pub async fn articles_overview(state: State<'_, AppState>) -> Result<Overview, String> {
    state.with_db(|c| overview(c))
}
#[tauri::command]
pub async fn articles_titles(state: State<'_, AppState>) -> Result<Vec<Link>, String> {
    state.with_db(|c| titles(c))
}
/// A locked one comes open only while 芝麻开门 is (else `open` is false and the page offers Touch ID).
#[tauri::command]
pub async fn article_get(state: State<'_, AppState>, id: i64) -> Result<Article, String> {
    let key = state.with_db(|c| Ok(load(c, id)?.sealed.is_some()))?.then(|| open_key(&state)).flatten();
    state.with_db(|c| get(c, id, key.as_ref()))
}
#[tauri::command]
pub async fn article_create(state: State<'_, AppState>, patch: Patch) -> Result<Article, String> {
    state.with_undo("新建笔记", |c| create(c, patch))
}
#[tauri::command]
pub async fn article_update(state: State<'_, AppState>, id: i64, patch: Patch) -> Result<Article, String> {
    let label = match (&patch.content, &patch.attachments, patch.pinned, patch.archived) {
        (None, None, Some(true), None) => "置顶笔记",
        (None, None, Some(false), None) => "取消置顶",
        (None, None, None, Some(true)) => "归档笔记",
        (None, None, None, Some(false)) => "取消归档",
        _ => "修改笔记",
    };
    let key = if patch.content.is_some() && state.with_db(|c| Ok(load(c, id)?.sealed.is_some()))? { Some(state.vault_key()?) } else { None };
    state.with_undo(label, |c| update(c, id, patch, key.as_ref()))
}
#[tauri::command]
pub fn article_trash(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除笔记", |c| trash(c, id))
}
#[tauri::command]
pub fn article_restore(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("捡回笔记", |c| restore(c, id))
}
#[tauri::command]
pub fn article_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("彻底删除笔记", |c| delete(c, id))
}
#[tauri::command]
pub fn articles_empty_trash(state: State<AppState>) -> Result<usize, String> {
    state.with_undo("清空废纸篓", empty_trash)
}
/// Needs 芝麻开门 open (the page asks for Touch ID first).
#[tauri::command]
pub fn article_lock(state: State<AppState>, id: i64, lock: bool) -> Result<Article, String> {
    let key = state.vault_key().map_err(|_| "先用 Touch ID 打开芝麻开门".to_string())?;
    state.with_undo(if lock { "加密笔记" } else { "取消加密" }, |c| set_locked(c, id, lock, &key))
}
#[tauri::command]
pub fn articles_tag_set(state: State<AppState>, tag: String, patch: TagPatch) -> Result<(), String> {
    let label = match (patch.pinned, &patch.icon) {
        (Some(true), _) => "置顶标签",
        (Some(false), _) => "取消置顶标签",
        _ => "标签图标",
    };
    state.with_undo(label, |c| tags::set(c, &TABLES, &tag, patch))
}
#[tauri::command]
pub fn articles_tag_rename(state: State<AppState>, from: String, to: String) -> Result<usize, String> {
    let key = open_key(&state);
    state.with_undo("标签改名", |c| rename_tag(c, &from, &to, key.as_ref()))
}
#[tauri::command]
pub fn articles_tag_place(state: State<AppState>, tag: String, parent: Option<String>, order: Vec<String>) -> Result<usize, String> {
    let moved = tags::parent_of(tag.trim().trim_start_matches('#')) != parent.as_deref().map(|p| p.trim().trim_start_matches('#')).filter(|p| !p.is_empty());
    let key = if moved { open_key(&state) } else { None };
    state.with_undo(if moved { "移动标签" } else { "标签排序" }, |c| place_tag(c, &tag, parent.as_deref(), &order, key.as_ref()))
}
#[tauri::command]
pub fn articles_tag_pins(state: State<AppState>, order: Vec<String>) -> Result<(), String> {
    state.with_undo("置顶标签排序", |c| tags::order_pins(c, &TABLES, &order))
}
/// (articles changed or moved to the trash, 0).
#[tauri::command]
pub fn articles_tag_delete(state: State<AppState>, tag: String, articles: bool) -> Result<(usize, usize), String> {
    let key = open_key(&state);
    state.with_undo(if articles { "删除标签和笔记" } else { "删除标签" }, |c| delete_tag(c, &tag, articles, key.as_ref()))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    pub fn add(c: &Connection, content: &str) -> i64 {
        create(c, Patch { content: Some(content.into()), ..Default::default() }).unwrap().id
    }
    fn ids(c: &Connection, view: &str) -> Vec<i64> {
        list(c, &Filter { view: Some(view.into()), ..Default::default() }).unwrap().iter().map(|s| s.id).collect()
    }

    #[test]
    fn titles_previews_and_marks() {
        assert_eq!(title_of("\n#读书 #旅行\n# **东京**的第三天 #日本\n早上被*香味*叫醒。"), "东京的第三天");
        assert_eq!(title_of("```\ncode\n```\n> - [ ] 买[柚子](https://a.b)"), "买柚子");
        assert_eq!(title_of(""), "");
        assert_eq!(preview_of("# 标题\n\n第一段 ==高亮== 和 [[别的笔记|那篇]]。\n\n---\n\n- 一条\n![](图%201.png)"), "第一段 高亮 和 那篇。 一条");
        assert_eq!(preview_of("# 标题\n\n> [!WARNING]\n> 注意\n\n> [!tip]- 带标题的\n> 内容\n\n[!NOTE] 不在引用里的照旧"), "注意 带标题的 内容 [!NOTE] 不在引用里的照旧");
        assert_eq!(strip_marks("a * b ~c~ snake_case `x`"), "a * b c snake_case x");
        assert_eq!(links_in("见 [[东京的第三天]] 和 [[Rust 笔记|这篇]]，`[[不算]]` [[A\\&B]] [[x \\| y|别名]]"), ["东京的第三天", "Rust 笔记", "A&B", "x | y"]);
        assert!(names("东京的第三天#早上", "东京的第三天") && names("rust 笔记", "Rust 笔记") && !names("东京", "东京的第三天"));
        assert_eq!(encode_name("截图 (1)#.png"), "截图%20%281%29%23.png");
    }

    #[test]
    fn views_trash_and_backlinks() {
        let c = memory_db();
        let a = add(&c, "# 东京的第三天\n#旅行/日本 早上");
        let b = add(&c, "# 计划\n看 [[东京的第三天#早上]]");
        let u = add(&c, "没有标签");
        update(&c, b, Patch { pinned: Some(true), ..Default::default() }, None).unwrap();
        assert_eq!(ids(&c, "all"), [b, u, a]);
        assert_eq!(ids(&c, "untagged"), [b, u]);
        assert_eq!(get(&c, a, None).unwrap().backlinks.iter().map(|l| (l.module, l.id, l.title.as_str())).collect::<Vec<_>>(), [("articles", b, "计划")]);
        update(&c, u, Patch { archived: Some(true), ..Default::default() }, None).unwrap();
        trash(&c, b).unwrap();
        assert_eq!((ids(&c, "all"), ids(&c, "archived"), ids(&c, "trash")), (vec![a], vec![u], vec![b]));
        assert!(get(&c, a, None).unwrap().backlinks.is_empty());
        let o = overview(&c).unwrap();
        assert_eq!((o.counts.all, o.counts.archived, o.counts.trash, o.tags.len()), (1, 1, 1, 2));
        restore(&c, b).unwrap();
        assert_eq!(ids(&c, "pinned"), [b]);
        trash(&c, u).unwrap();
        assert_eq!(empty_trash(&c).unwrap(), 1);
        assert_eq!(count(&c).unwrap(), 2);
        // searching: words in the title or the text
        assert_eq!(list(&c, &Filter { q: Some("东京 早上".into()), ..Default::default() }).unwrap().len(), 2);
    }

    #[test]
    fn tags_skip_code_and_deleting_with_articles_trashes() {
        let c = memory_db();
        let a = add(&c, "#读书 正文 `#不是`\n```\n#也不是\n```");
        let b = add(&c, "#读书/摘抄 b");
        assert_eq!(get(&c, a, None).unwrap().tags, ["读书"]);
        rename_tag(&c, "读书", "阅读", None).unwrap();
        assert_eq!(get(&c, a, None).unwrap().content, "#阅读 正文 `#不是`\n```\n#也不是\n```");
        assert_eq!(get(&c, b, None).unwrap().tags, ["阅读/摘抄"]);
        assert_eq!(delete_tag(&c, "阅读/摘抄", true, None).unwrap(), (1, 0));
        assert_eq!(ids(&c, "trash"), [b]);
        // only the tag: the article stays, now empty of it
        let d = add(&c, "#孤单");
        assert_eq!(delete_tag(&c, "孤单", false, None).unwrap(), (1, 0));
        assert_eq!(get(&c, d, None).unwrap().content, "");
    }

    #[test]
    fn locked_needs_the_key() {
        let c = memory_db();
        let k = vault::test_key();
        let a = add(&c, "# 给小林的信\n#私人 生日快乐");
        let locked = set_locked(&c, a, true, &k).unwrap();
        assert!(locked.locked && locked.open && locked.content.contains("生日快乐"));
        let raw: (String, Vec<u8>) = c.query_row("SELECT content, sealed FROM articles WHERE id = ?1", [a], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert!(raw.0.is_empty() && !String::from_utf8_lossy(&raw.1).contains("生日"));
        let shut = get(&c, a, None).unwrap();
        assert!(!shut.open && shut.content.is_empty() && shut.title == "给小林的信" && shut.tags == ["私人"]);
        assert_eq!(list(&c, &Filter::default()).unwrap()[0].preview, "");
        assert!(list(&c, &Filter { q: Some("生日".into()), ..Default::default() }).unwrap().is_empty());
        assert_eq!(update(&c, a, Patch { content: Some("x".into()), ..Default::default() }, None).unwrap_err(), LOCKED);
        update(&c, a, Patch { content: Some("# 给小林的信\n#私人 改了".into()), ..Default::default() }, Some(&k)).unwrap();
        assert!(rename_tag(&c, "私人", "秘密", None).is_err());
        rename_tag(&c, "私人", "秘密", Some(&k)).unwrap();
        assert_eq!(get(&c, a, Some(&k)).unwrap().content, "# 给小林的信\n#秘密 改了");
        assert!(vault::unseal_article(&vault::test_key(), &raw.1).is_err());
        let back = set_locked(&c, a, false, &k).unwrap();
        assert!(!back.locked && back.content.ends_with("改了"));
        assert!(readable(&c).unwrap().iter().any(|(id, _, _)| *id == a));
    }

    #[test]
    fn export_import_round_trip() {
        let a = memory_db();
        let dir = tempfile_dir();
        let pic = attachments::store(&a, &dir, "图 1.png", b"\x89PNG fake").unwrap();
        let x = create(&a, Patch { content: Some(format!("# 有图\n#读书 ![]({})", encode_name(&pic.name))), attachments: Some(vec![pic.id]), ..Default::default() }).unwrap();
        assert_eq!(list(&a, &Filter::default()).unwrap()[0].thumb, Some(pic.id));
        let y = add(&a, "# 锁着的\n#私人 秘密");
        set_locked(&a, y, true, &vault::test_key()).unwrap();
        let z = add(&a, "归档的");
        update(&a, z, Patch { archived: Some(true), pinned: Some(true), ..Default::default() }, None).unwrap();
        let w = add(&a, "扔掉的");
        trash(&a, w).unwrap();
        tags::set(&a, &TABLES, "读书", serde_json::from_value(json!({"pinned": true, "icon": "i:book-open"})).unwrap()).unwrap();
        let rec = export(&a).unwrap();
        let v = serde_json::to_value(&rec).unwrap();
        assert_eq!(v["articles"][1]["title"], "锁着的");
        assert_eq!(v["articles"][1]["tags"], json!(["私人"]));
        assert_eq!(v["articles"][1]["content"], "");
        let b = memory_db();
        replace_all(&b, &serde_json::from_value(v.clone()).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(export(&b).unwrap()).unwrap(), v);
        assert_eq!(overview(&b).unwrap(), overview(&a).unwrap());
        assert_eq!(get(&b, 1, None).unwrap().title, get(&a, x.id, None).unwrap().title);
        // the same bytes already here under another name: the text follows that name
        let c = memory_db();
        attachments::store(&c, &dir, "旧名字.png", b"\x89PNG fake").unwrap();
        replace_all(&c, &serde_json::from_value(v).unwrap()).unwrap();
        assert!(get(&c, 1, None).unwrap().content.contains("![](旧名字.png)"));
        assert!(list(&c, &Filter { view: Some("any".into()), ..Default::default() }).unwrap().iter().find(|s| s.title == "有图").unwrap().thumb.is_some());
        // a bad record
        let bad: ArticlesRecord = serde_json::from_value(json!({"articles": [{"content": "x", "title": "y"}]})).unwrap();
        assert!(validate_records(&bad).is_err());
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pomelo-articles-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
