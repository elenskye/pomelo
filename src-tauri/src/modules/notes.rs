//! 念念不忘 (notes, after Flomo). A note is Markdown-lite text + its time; nothing else is typed in.
//! - Tags live inline: `#tag` / `#a/b` (nested). A tag starts with `#` at the start of the text or
//!   after whitespace and runs to the next whitespace or `#`; trailing `/` is dropped; valid when
//!   non-empty, ≤ 50 chars, no leading `/`, no `//` (the UI renderer mirrors this rule).
//!   `note_tags` is re-derived from content on every write; filtering by a tag includes sub-tags.
//!   Renaming a tag rewrites the notes' text (into an existing tag = merge).
//! - Refs (Flomo 批注): a note may quote others (`note_refs`, ordered); both sides list the link.
//! - created_at (UTC, ms) is a note's identity in data files: unique, refs are exported by it.
//! - Tag settings (`note_tag_meta`, by full path): pinned (first-level tags only; listed above the tag
//!   tree with their sub-tags, in pin order) and an icon (an emoji, or `i:<name>` for one of the UI's
//!   line icons). They outlive a tag's last note (it comes back with them) and move on rename.
//! Formatting is the UI's business: **bold**, ==highlight==, ++underline++, `- ` / `1. ` lists
//! (2 spaces per level), bare URLs.

use super::common::{self, err};
use crate::AppState;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use tauri::State;

pub const KEY: &str = "notes";
pub const LABEL: &str = "念念不忘";
pub const MAX_CHARS: usize = 50_000;
pub const TAG_MAX: usize = 50;
pub const MAX_REFS: usize = 50;
pub const ICON_MAX: usize = 16;
const EXCERPT_CHARS: usize = 60;
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

fn gone() -> String {
    "找不到这条笔记，可能已经被删除了".into()
}

// ---------- tags ----------

fn valid_tag(t: &str) -> bool {
    !t.is_empty() && !t.starts_with('/') && !t.contains("//") && t.chars().count() <= TAG_MAX
}

/// Tags in `text` as (byte start, byte end, tag) — the span covers the tag after `#`.
pub fn tag_spans(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c == '#' && prev.is_none_or(char::is_whitespace) {
            let start = i + 1;
            let mut end = start;
            while let Some(&(j, d)) = it.peek() {
                if d.is_whitespace() || d == '#' {
                    break;
                }
                end = j + d.len_utf8();
                it.next();
            }
            let tag = text[start..end].trim_end_matches('/');
            if valid_tag(tag) {
                out.push((start, start + tag.len(), tag.to_string()));
            }
            prev = text[..end].chars().next_back();
            continue;
        }
        prev = Some(c);
    }
    out
}

/// Distinct tags in order of appearance.
pub fn tags_in(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    tag_spans(text).into_iter().map(|(_, _, t)| t).filter(|t| seen.insert(t.clone())).collect()
}

/// A tag name typed by the user (rename target): optional leading `#`.
fn clean_tag(s: &str) -> Result<String, String> {
    let t = s.trim().trim_start_matches('#').trim_end_matches('/');
    if t.is_empty() {
        return Err("标签不能为空".into());
    }
    if t.chars().any(|c| c.is_whitespace() || c == '#') {
        return Err("标签里不能有空格或 #".into());
    }
    if t.chars().count() > TAG_MAX {
        return Err(format!("标签太长了（最多 {TAG_MAX} 个字）"));
    }
    if !valid_tag(t) {
        return Err(format!("标签的格式不对：{t}"));
    }
    Ok(t.to_string())
}

fn under(tag: &str, root: &str) -> bool {
    tag == root || tag.strip_prefix(root).is_some_and(|r| r.starts_with('/'))
}

/// `text` with every tag at or under `from` moved to `to` (sub-paths kept).
pub fn replace_tag(text: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (a, b, tag) in tag_spans(text) {
        if under(&tag, from) {
            out.push_str(&text[last..a]);
            out.push_str(to);
            out.push_str(&tag[from.len()..]);
            last = b;
        }
    }
    out.push_str(&text[last..]);
    out
}

// ---------- text ----------

fn clean_content(s: &str) -> Result<String, String> {
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    let s = s.trim();
    if s.is_empty() {
        return Err("笔记不能是空的".into());
    }
    if s.chars().count() > MAX_CHARS {
        return Err(format!("笔记太长了（最多 {MAX_CHARS} 个字）"));
    }
    Ok(s.to_string())
}

fn strip_list_marker(line: &str) -> &str {
    let t = line.trim_start();
    if let Some(r) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
        return r;
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        if let Some(r) = t[digits..].strip_prefix(". ").or_else(|| t[digits..].strip_prefix(") ")) {
            return r;
        }
    }
    t
}

/// One-line preview for links between notes: no tags, markers or list bullets.
pub fn excerpt(content: &str) -> String {
    let mut plain = String::new();
    let mut last = 0;
    for (a, b, _) in tag_spans(content) {
        plain.push_str(&content[last..a - 1]);
        last = b;
    }
    plain.push_str(&content[last..]);
    let plain = plain.replace("**", "").replace("==", "").replace("++", "");
    let words: Vec<&str> = plain.lines().map(strip_list_marker).flat_map(str::split_whitespace).collect();
    let mut s = words.join(" ");
    if s.is_empty() {
        s = tags_in(content).iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ");
    }
    if s.chars().count() > EXCERPT_CHARS {
        s = s.chars().take(EXCERPT_CHARS).collect::<String>() + "…";
    }
    s
}

/// `YYYY-MM-DDTHH:MM:SS.sssZ` (what SQLite's strftime writes).
fn check_stamp(s: &str) -> Result<(), String> {
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>, max: u32| s.get(r).and_then(|x| x.parse::<u32>().ok()).is_some_and(|n| n <= max);
    let ok = b.len() == 24
        && s.get(..10).is_some_and(|d| common::check_date(&Some(d.to_string()), "").is_ok())
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'.'
        && b[23] == b'Z'
        && num(11..13, 23)
        && num(14..16, 59)
        && num(17..19, 59)
        && num(20..23, 999);
    if ok {
        Ok(())
    } else {
        Err(format!("时间格式不对：{s}"))
    }
}

/// Now, nudged by 1 ms until no note has it (created_at is unique).
fn fresh_stamp(conn: &Connection) -> Result<String, String> {
    let mut s: String = conn.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0)).map_err(err)?;
    for _ in 0..10_000 {
        let taken: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM notes WHERE created_at = ?1)", [&s], |r| r.get(0)).map_err(err)?;
        if !taken {
            return Ok(s);
        }
        s = conn
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', ?1, '+0.001 seconds')", [&s], |r| r.get(0))
            .map_err(err)?;
    }
    Err("时间冲突，请稍后再试".into())
}

// ---------- notes ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Brief {
    pub id: i64,
    pub created_at: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: i64,
    pub content: String,
    pub pinned: bool,
    pub created_at: String,
    pub updated_at: String,
    /// Derived from content, in order of appearance.
    pub tags: Vec<String>,
    /// Notes this one quotes, in order.
    pub refs: Vec<Brief>,
    /// Notes quoting this one, oldest first.
    pub backrefs: Vec<Brief>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filter {
    /// Includes sub-tags.
    #[serde(default)]
    pub tag: Option<String>,
    /// Local date `YYYY-MM-DD` the note was written.
    #[serde(default)]
    pub day: Option<String>,
    /// Words (space-separated), all must appear.
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub id: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotePatch {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub pinned: Option<bool>,
    /// Ids of quoted notes, in order (replaces the list).
    #[serde(default)]
    pub refs: Option<Vec<i64>>,
}

type Links = HashMap<i64, Vec<Brief>>;

/// (refs by note, backrefs by note).
fn links(conn: &Connection) -> Result<(Links, Links), String> {
    let load = |sql: &str| -> Result<Links, String> {
        let mut st = conn.prepare(sql).map_err(err)?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, Brief { id: r.get(1)?, created_at: r.get(2)?, excerpt: excerpt(&r.get::<_, String>(3)?) })))
            .map_err(err)?;
        let mut m: Links = HashMap::new();
        for row in rows {
            let (k, b) = row.map_err(err)?;
            m.entry(k).or_default().push(b);
        }
        Ok(m)
    };
    Ok((
        load("SELECT r.note_id, n.id, n.created_at, n.content FROM note_refs r JOIN notes n ON n.id = r.ref_id ORDER BY r.note_id, r.sort_order")?,
        load("SELECT r.ref_id, n.id, n.created_at, n.content FROM note_refs r JOIN notes n ON n.id = r.note_id ORDER BY r.ref_id, n.created_at")?,
    ))
}

/// Matching notes, newest first (pinned on top when nothing is filtered).
pub fn list(conn: &Connection, f: &Filter) -> Result<Vec<Note>, String> {
    let mut conds: Vec<String> = Vec::new();
    let mut args: Vec<Value> = Vec::new();
    let mut arg = |v: Value| {
        args.push(v);
        format!("?{}", args.len())
    };
    if let Some(t) = common::clean(f.tag.clone()) {
        let t = t.trim_start_matches('#').to_string();
        let p = arg(Value::Text(t));
        conds.push(format!("EXISTS (SELECT 1 FROM note_tags t WHERE t.note_id = n.id AND (t.tag = {p} OR substr(t.tag, 1, length({p}) + 1) = {p} || '/'))"));
    }
    if let Some(d) = common::clean(f.day.clone()) {
        let p = arg(Value::Text(d));
        conds.push(format!("date(n.created_at, 'localtime') = {p}"));
    }
    for w in f.q.as_deref().unwrap_or("").split_whitespace() {
        let p = arg(Value::Text(w.to_string()));
        conds.push(format!("instr(lower(n.content), lower({p})) > 0"));
    }
    if let Some(id) = f.id {
        let p = arg(Value::Integer(id));
        conds.push(format!("n.id = {p}"));
    }
    let order = if conds.is_empty() { "n.pinned DESC, n.created_at DESC" } else { "n.created_at DESC" };
    let filter = if conds.is_empty() { "1 = 1".to_string() } else { conds.join(" AND ") };
    let mut st = conn
        .prepare(&format!("SELECT n.id, n.content, n.pinned, n.created_at, n.updated_at FROM notes n WHERE {filter} ORDER BY {order}"))
        .map_err(err)?;
    let rows = st
        .query_map(params_from_iter(args), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?)))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let (mut refs, mut backrefs) = links(conn)?;
    Ok(rows
        .into_iter()
        .map(|(id, content, pinned, created_at, updated_at)| Note {
            id,
            tags: tags_in(&content),
            content,
            pinned,
            created_at,
            updated_at,
            refs: refs.remove(&id).unwrap_or_default(),
            backrefs: backrefs.remove(&id).unwrap_or_default(),
        })
        .collect())
}

pub fn get(conn: &Connection, id: i64) -> Result<Note, String> {
    list(conn, &Filter { id: Some(id), ..Default::default() })?.pop().ok_or_else(gone)
}

fn exists(conn: &Connection, id: i64) -> Result<bool, String> {
    conn.query_row("SELECT EXISTS (SELECT 1 FROM notes WHERE id = ?1)", [id], |r| r.get(0)).map_err(err)
}

/// Deduplicated, existing, not `me`.
fn clean_refs(conn: &Connection, me: Option<i64>, refs: &[i64]) -> Result<Vec<i64>, String> {
    let mut seen = HashSet::new();
    let out: Vec<i64> = refs.iter().copied().filter(|r| seen.insert(*r)).collect();
    if out.len() > MAX_REFS {
        return Err(format!("一条笔记最多引用 {MAX_REFS} 条"));
    }
    for &r in &out {
        if Some(r) == me {
            return Err("笔记不能引用它自己".into());
        }
        if !exists(conn, r)? {
            return Err("引用的笔记找不到了，可能已经被删除了".into());
        }
    }
    Ok(out)
}

fn current_refs(conn: &Connection, id: i64) -> Result<Vec<i64>, String> {
    let mut st = conn.prepare("SELECT ref_id FROM note_refs WHERE note_id = ?1 ORDER BY sort_order").map_err(err)?;
    let ids = st.query_map([id], |r| r.get(0)).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    Ok(ids)
}

fn set_refs(conn: &Connection, id: i64, refs: &[i64]) -> Result<(), String> {
    if current_refs(conn, id)? == refs {
        return Ok(());
    }
    conn.execute("DELETE FROM note_refs WHERE note_id = ?1", [id]).map_err(err)?;
    for (i, r) in refs.iter().enumerate() {
        conn.execute("INSERT INTO note_refs (note_id, ref_id, sort_order) VALUES (?1, ?2, ?3)", params![id, r, i as i64 + 1])
            .map_err(err)?;
    }
    Ok(())
}

/// note_tags := tags in `content` (only the differences are written).
fn sync_tags(conn: &Connection, id: i64, content: &str) -> Result<(), String> {
    let want: HashSet<String> = tags_in(content).into_iter().collect();
    let mut st = conn.prepare("SELECT tag FROM note_tags WHERE note_id = ?1").map_err(err)?;
    let have: HashSet<String> = st.query_map([id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for t in have.difference(&want) {
        conn.execute("DELETE FROM note_tags WHERE note_id = ?1 AND tag = ?2", params![id, t]).map_err(err)?;
    }
    for t in want.difference(&have) {
        conn.execute("INSERT INTO note_tags (note_id, tag) VALUES (?1, ?2)", params![id, t]).map_err(err)?;
    }
    Ok(())
}

pub fn create(conn: &Connection, p: NotePatch) -> Result<Note, String> {
    let content = clean_content(p.content.as_deref().unwrap_or(""))?;
    let refs = clean_refs(conn, None, &p.refs.unwrap_or_default())?;
    let stamp = fresh_stamp(conn)?;
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        "INSERT INTO notes (content, pinned, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![content, p.pinned.unwrap_or(false), stamp],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    sync_tags(&tx, id, &content)?;
    set_refs(&tx, id, &refs)?;
    tx.commit()?;
    get(conn, id)
}

/// Pinning alone keeps updated_at (it is about the list, not the note).
pub fn update(conn: &Connection, id: i64, p: NotePatch) -> Result<Note, String> {
    let old = get(conn, id)?;
    let content = match &p.content {
        Some(c) => clean_content(c)?,
        None => old.content.clone(),
    };
    let old_refs: Vec<i64> = old.refs.iter().map(|b| b.id).collect();
    let refs = match &p.refs {
        Some(r) => clean_refs(conn, Some(id), r)?,
        None => old_refs.clone(),
    };
    let pinned = p.pinned.unwrap_or(old.pinned);
    let edited = content != old.content || refs != old_refs;
    if !edited && pinned == old.pinned {
        return Ok(old);
    }
    let tx = common::Tx::begin(conn)?;
    if edited {
        tx.execute(&format!("UPDATE notes SET content = ?1, pinned = ?2, updated_at = {NOW} WHERE id = ?3"), params![content, pinned, id])
            .map_err(err)?;
        sync_tags(&tx, id, &content)?;
        set_refs(&tx, id, &refs)?;
    } else {
        tx.execute("UPDATE notes SET pinned = ?1 WHERE id = ?2", params![pinned, id]).map_err(err)?;
    }
    tx.commit()?;
    get(conn, id)
}

/// Links to and from it go too; the other notes stay.
pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    if conn.execute("DELETE FROM notes WHERE id = ?1", [id]).map_err(err)? == 0 {
        return Err(gone());
    }
    Ok(())
}

/// Moves `from` (and its sub-tags) to `to` in every note using it; returns how many changed.
pub fn rename_tag(conn: &Connection, from: &str, to: &str) -> Result<usize, String> {
    let from = from.trim().trim_start_matches('#');
    let to = clean_tag(to)?;
    if to == from {
        return Ok(0);
    }
    if under(&to, from) {
        return Err("不能改成它自己下面的标签".into());
    }
    let mut st = conn
        .prepare("SELECT DISTINCT n.id, n.content FROM notes n JOIN note_tags t ON t.note_id = n.id WHERE t.tag = ?1 OR substr(t.tag, 1, length(?1) + 1) = ?1 || '/'")
        .map_err(err)?;
    let rows: Vec<(i64, String)> = st.query_map([from], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    if rows.is_empty() {
        return Err(format!("没有笔记用到「#{from}」"));
    }
    let tx = common::Tx::begin(conn)?;
    for (id, content) in &rows {
        let next = clean_content(&replace_tag(content, from, &to))?;
        tx.execute(&format!("UPDATE notes SET content = ?1, updated_at = {NOW} WHERE id = ?2"), params![next, id]).map_err(err)?;
        sync_tags(&tx, *id, &next)?;
    }
    move_meta(&tx, from, &to)?;
    tx.commit()?;
    Ok(rows.len())
}

// ---------- tag settings (pinned, icon) ----------

type Meta = (Option<i64>, Option<String>);

fn meta_of(conn: &Connection, tag: &str) -> Result<Option<Meta>, String> {
    conn.query_row("SELECT pin_order, icon FROM note_tag_meta WHERE tag = ?1", [tag], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .map_err(err)
}

/// Writes a tag's settings; a row with nothing set is removed.
fn put_meta(conn: &Connection, tag: &str, (pin, icon): Meta) -> Result<(), String> {
    if pin.is_none() && icon.is_none() {
        conn.execute("DELETE FROM note_tag_meta WHERE tag = ?1", [tag]).map_err(err)?;
    } else {
        conn.execute(
            &format!(
                "INSERT INTO note_tag_meta (tag, pin_order, icon) VALUES (?1, ?2, ?3)
                 ON CONFLICT(tag) DO UPDATE SET pin_order = excluded.pin_order, icon = excluded.icon, updated_at = {NOW}"
            ),
            params![tag, pin, icon],
        )
        .map_err(err)?;
    }
    Ok(())
}

/// Rename: settings of `from` and its sub-tags follow; merging into a tag that has its own keeps
/// the target's icon (else takes this one) and the earlier pin. A pin doesn't survive becoming a sub-tag.
fn move_meta(conn: &Connection, from: &str, to: &str) -> Result<(), String> {
    let mut st = conn.prepare("SELECT tag, pin_order, icon FROM note_tag_meta").map_err(err)?;
    let rows: Vec<(String, Option<i64>, Option<String>)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for (tag, pin, icon) in rows.into_iter().filter(|(t, _, _)| under(t, from)) {
        let target = format!("{to}{}", &tag[from.len()..]);
        let (tpin, ticon) = meta_of(conn, &target)?.unwrap_or((None, None));
        let pin = match (tpin, pin) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
        .filter(|_| !target.contains('/'));
        conn.execute("DELETE FROM note_tag_meta WHERE tag = ?1", [&tag]).map_err(err)?;
        put_meta(conn, &target, (pin, ticon.or(icon)))?;
    }
    Ok(())
}

fn clean_icon(icon: Option<String>) -> Result<Option<String>, String> {
    let icon = common::clean(icon);
    if let Some(i) = &icon {
        if i.chars().count() > ICON_MAX || i.chars().any(char::is_whitespace) {
            return Err("图标只能是一个 emoji 或自带的图标".into());
        }
    }
    Ok(icon)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TagPatch {
    #[serde(default)]
    pub pinned: Option<bool>,
    /// null = back to 「#」.
    #[serde(default, deserialize_with = "common::some")]
    pub icon: Option<Option<String>>,
}

/// Pins / unpins a first-level tag (a new pin goes last) or sets any tag's icon. Only tags some note uses.
pub fn set_tag(conn: &Connection, tag: &str, p: TagPatch) -> Result<(), String> {
    let tag = clean_tag(tag)?;
    if p.pinned == Some(true) && tag.contains('/') {
        return Err("只能置顶第一级标签".into());
    }
    let used: i64 = conn
        .query_row("SELECT count(*) FROM note_tags WHERE tag = ?1 OR substr(tag, 1, length(?1) + 1) = ?1 || '/'", [&tag], |r| r.get(0))
        .map_err(err)?;
    if used == 0 {
        return Err(format!("没有笔记用到「#{tag}」"));
    }
    let (pin, icon) = meta_of(conn, &tag)?.unwrap_or((None, None));
    let pin = match p.pinned {
        Some(true) if pin.is_none() => {
            Some(conn.query_row("SELECT coalesce(max(pin_order), 0) + 1 FROM note_tag_meta", [], |r| r.get::<_, i64>(0)).map_err(err)?)
        }
        Some(false) => None,
        _ => pin,
    };
    let icon = match p.icon {
        Some(i) => clean_icon(i)?,
        None => icon,
    };
    put_meta(conn, &tag, (pin, icon))
}

// ---------- overview (left column: counts, heatmap, tag tree) ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCount {
    pub day: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    /// Full path; every ancestor of a used tag is listed too.
    pub tag: String,
    /// Notes at or under this tag.
    pub count: i64,
    /// Emoji shown instead of 「#」.
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub total: i64,
    /// Local dates with notes, ascending.
    pub days: Vec<DayCount>,
    pub tags: Vec<TagCount>,
    /// Pinned (first-level) tags in use, in pin order.
    pub pinned: Vec<String>,
}

pub fn overview(conn: &Connection) -> Result<Overview, String> {
    let total = count(conn)?;
    let mut st = conn
        .prepare("SELECT date(created_at, 'localtime') AS d, count(*) FROM notes GROUP BY d ORDER BY d")
        .map_err(err)?;
    let days = st
        .query_map([], |r| Ok(DayCount { day: r.get(0)?, count: r.get(1)? }))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut st = conn.prepare("SELECT note_id, tag FROM note_tags").map_err(err)?;
    let pairs: Vec<(i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut under: BTreeMap<String, HashSet<i64>> = BTreeMap::new();
    for (id, tag) in pairs {
        let mut path = String::new();
        for seg in tag.split('/') {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(seg);
            under.entry(path.clone()).or_default().insert(id);
        }
    }
    let mut st = conn.prepare("SELECT tag, pin_order, icon FROM note_tag_meta ORDER BY pin_order, tag").map_err(err)?;
    let meta: Vec<(String, Option<i64>, Option<String>)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let pinned = meta.iter().filter(|(t, p, _)| p.is_some() && !t.contains('/') && under.contains_key(t)).map(|(t, _, _)| t.clone()).collect();
    let icons: HashMap<&str, &str> = meta.iter().filter_map(|(t, _, i)| i.as_deref().map(|i| (t.as_str(), i))).collect();
    let tags = under
        .iter()
        .map(|(tag, ids)| TagCount { tag: tag.clone(), count: ids.len() as i64, icon: icons.get(tag.as_str()).map(|i| i.to_string()) })
        .collect();
    Ok(Overview { total, days, tags, pinned })
}

// ---------- import / export: { notes: [{ content, pinned?, refs?: [createdAt], createdAt, updatedAt }],
//            tags?: [{ tag, pinned?, icon? }] (pinned ones in pin order) } ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoteRecord {
    pub content: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// createdAt of the quoted notes, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    /// Missing = now (then nothing can quote it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TagRecord {
    pub tag: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotesRecord {
    #[serde(default)]
    pub notes: Vec<NoteRecord>,
    /// v9 and older: none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagRecord>,
}

impl NotesRecord {
    pub fn len(&self) -> usize {
        self.notes.len()
    }
}

pub fn validate_records(rec: &NotesRecord) -> Result<(), String> {
    let mut stamps: HashMap<&str, usize> = HashMap::new();
    for (i, n) in rec.notes.iter().enumerate() {
        let ctx = |e: String| format!("{LABEL}第 {} 条：{e}", i + 1);
        clean_content(&n.content).map_err(ctx)?;
        if let Some(s) = &n.created_at {
            check_stamp(s).map_err(ctx)?;
            if let Some(j) = stamps.insert(s, i) {
                return Err(ctx(format!("和第 {} 条的时间相同（{s}）", j + 1)));
            }
        }
    }
    for (i, n) in rec.notes.iter().enumerate() {
        let ctx = |e: String| format!("{LABEL}第 {} 条：{e}", i + 1);
        if n.refs.len() > MAX_REFS {
            return Err(ctx(format!("最多引用 {MAX_REFS} 条")));
        }
        for r in &n.refs {
            match stamps.get(r.as_str()) {
                None => return Err(ctx(format!("找不到引用的笔记（{r}）"))),
                Some(&j) if j == i => return Err(ctx("不能引用它自己".into())),
                _ => {}
            }
        }
    }
    let mut seen = HashSet::new();
    for t in &rec.tags {
        let ctx = |e: String| format!("{LABEL}的标签「{}」：{e}", t.tag);
        let tag = clean_tag(&t.tag).map_err(ctx)?;
        clean_icon(t.icon.clone()).map_err(ctx)?;
        if !seen.insert(tag) {
            return Err(ctx("重复了".into()));
        }
    }
    Ok(())
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM notes", [], |r| r.get(0)).map_err(err)
}

/// Replaces all notes. Caller owns the transaction.
pub fn replace_all(conn: &Connection, rec: &NotesRecord) -> Result<usize, String> {
    validate_records(rec)?;
    conn.execute("DELETE FROM notes", []).map_err(err)?;
    let mut ids: HashMap<&str, i64> = HashMap::new();
    let mut made = Vec::with_capacity(rec.notes.len());
    for n in &rec.notes {
        let content = clean_content(&n.content)?;
        let stamp = match &n.created_at {
            Some(s) => s.clone(),
            None => fresh_stamp(conn)?,
        };
        conn.execute(
            "INSERT INTO notes (content, pinned, created_at, updated_at) VALUES (?1, ?2, ?3, coalesce(?4, ?3))",
            params![content, n.pinned, stamp, n.updated_at],
        )
        .map_err(|e| format!("{LABEL}：{e}"))?;
        let id = conn.last_insert_rowid();
        sync_tags(conn, id, &content)?;
        if let Some(s) = &n.created_at {
            ids.insert(s, id);
        }
        made.push(id);
    }
    for (n, id) in rec.notes.iter().zip(&made) {
        let refs: Vec<i64> = n.refs.iter().map(|r| ids[r.as_str()]).collect();
        set_refs(conn, *id, &refs)?;
    }
    conn.execute("DELETE FROM note_tag_meta", []).map_err(err)?;
    for (i, t) in rec.tags.iter().enumerate() {
        let tag = clean_tag(&t.tag)?;
        // v10 files may pin a sub-tag: dropped, as migration 22 did
        put_meta(conn, &tag, ((t.pinned && !tag.contains('/')).then_some(i as i64 + 1), clean_icon(t.icon.clone())?))?;
    }
    Ok(rec.len())
}

/// Oldest first.
pub fn export(conn: &Connection) -> Result<NotesRecord, String> {
    let mut st = conn.prepare("SELECT id, content, pinned, created_at, updated_at FROM notes ORDER BY created_at").map_err(err)?;
    let rows: Vec<(i64, String, bool, String, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let (refs, _) = links(conn)?;
    let notes = rows
        .into_iter()
        .map(|(id, content, pinned, created_at, updated_at)| NoteRecord {
            content,
            pinned,
            refs: refs.get(&id).map(|bs| bs.iter().map(|b| b.created_at.clone()).collect()).unwrap_or_default(),
            created_at: Some(created_at),
            updated_at: Some(updated_at),
        })
        .collect();
    let mut st = conn.prepare("SELECT tag, pin_order, icon FROM note_tag_meta ORDER BY pin_order IS NULL, pin_order, tag").map_err(err)?;
    let tags = st
        .query_map([], |r| Ok(TagRecord { tag: r.get(0)?, pinned: r.get::<_, Option<i64>>(1)?.is_some(), icon: r.get(2)? }))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    Ok(NotesRecord { notes, tags })
}

// ---------- commands ----------

#[tauri::command]
pub fn notes_list(state: State<AppState>, filter: Filter) -> Result<Vec<Note>, String> {
    state.with_db(|c| list(c, &filter))
}
#[tauri::command]
pub fn notes_overview(state: State<AppState>) -> Result<Overview, String> {
    state.with_db(|c| overview(c))
}
#[tauri::command]
pub fn note_create(state: State<AppState>, patch: NotePatch) -> Result<Note, String> {
    state.with_undo("添加笔记", |c| create(c, patch))
}
#[tauri::command]
pub fn note_update(state: State<AppState>, id: i64, patch: NotePatch) -> Result<Note, String> {
    let label = match (&patch.content, &patch.refs, patch.pinned) {
        (None, None, Some(true)) => "置顶笔记",
        (None, None, Some(false)) => "取消置顶",
        _ => "修改笔记",
    };
    state.with_undo(label, |c| update(c, id, patch))
}
#[tauri::command]
pub fn note_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除笔记", |c| delete(c, id))
}
#[tauri::command]
pub fn notes_tag_set(state: State<AppState>, tag: String, patch: TagPatch) -> Result<(), String> {
    let label = match (patch.pinned, &patch.icon) {
        (Some(true), _) => "置顶标签",
        (Some(false), _) => "取消置顶标签",
        _ => "标签图标",
    };
    state.with_undo(label, |c| set_tag(c, &tag, patch))
}
#[tauri::command]
pub fn notes_tag_rename(state: State<AppState>, from: String, to: String) -> Result<usize, String> {
    state.with_undo("标签改名", |c| rename_tag(c, &from, &to))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    fn np(v: serde_json::Value) -> NotePatch {
        serde_json::from_value(v).unwrap()
    }
    fn add(c: &Connection, text: &str) -> Note {
        create(c, np(json!({ "content": text }))).unwrap()
    }
    fn ids(notes: &[Note]) -> Vec<i64> {
        notes.iter().map(|n| n.id).collect()
    }
    fn filtered(c: &Connection, f: serde_json::Value) -> Vec<i64> {
        ids(&list(c, &serde_json::from_value(f).unwrap()).unwrap())
    }

    #[test]
    fn tag_rules() {
        assert_eq!(tags_in("#读书/摘抄 正文 #读书"), ["读书/摘抄", "读书"]);
        assert_eq!(tags_in("#&舒宇\n第二行 #a/b/ 结尾#不是 C# x"), ["&舒宇", "a/b"]);
        assert_eq!(tags_in("# 标题 ## x ##y #/z #a//b https://x.com/#frag"), Vec::<String>::new());
        assert_eq!(tags_in("全角　#空格后 #a#b"), ["空格后", "a"]);
        assert_eq!(tags_in(&format!("#{} #{}", "长".repeat(50), "长".repeat(51))), ["长".repeat(50)]);
        let t = "#a/b 和 #a 还有 #ab #a/b/c，以及 #x";
        assert_eq!(replace_tag(t, "a", "新/甲"), "#新/甲/b 和 #新/甲 还有 #ab #新/甲/b/c，以及 #x");
        assert_eq!(replace_tag("#a/ x", "a", "b"), "#b/ x");
        assert_eq!(excerpt("#百言集/春盗 \n\n- **重点** ==标==\n1. 第二 ++下++"), "重点 标 第二 下");
        assert_eq!(excerpt("#只有标签 #另一个"), "#只有标签 #另一个");
        assert_eq!(excerpt(&"字".repeat(70)).chars().count(), EXCERPT_CHARS + 1);
    }

    #[test]
    fn stamps() {
        assert!(check_stamp("2026-02-23T06:39:11.000Z").is_ok());
        for bad in ["2026-02-23T06:39:11Z", "2026-02-30T06:39:11.000Z", "2026-02-23 06:39:11.000Z", "2026-02-23T24:00:00.000Z"] {
            assert!(check_stamp(bad).is_err(), "{bad}");
        }
        let c = memory_db();
        let made: Vec<String> = (0..20).map(|i| add(&c, &i.to_string()).created_at).collect();
        assert_eq!(made.iter().collect::<HashSet<_>>().len(), 20); // same millisecond → nudged
        assert!(made.iter().all(|s| check_stamp(s).is_ok()));
        let s: String = c.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', '2026-12-31T23:59:59.999Z', '+0.001 seconds')", [], |r| r.get(0)).unwrap();
        assert_eq!(s, "2027-01-01T00:00:00.000Z");
    }

    #[test]
    fn create_update_filter_delete() {
        let c = memory_db();
        assert!(create(&c, np(json!({"content": " \n "}))).is_err());
        let a = add(&c, "  #读书/摘抄 第一条\r\n**重点**  ");
        assert_eq!((a.content.as_str(), a.tags.clone(), a.pinned), ("#读书/摘抄 第一条\n**重点**", vec!["读书/摘抄".to_string()], false));
        let b = add(&c, "#读书 第二条 Rust");
        let d = add(&c, "第三条 没有标签 rust 宏");
        assert_eq!(filtered(&c, json!({})), [d.id, b.id, a.id]);
        assert_eq!(filtered(&c, json!({"tag": "读书"})), [b.id, a.id]); // sub-tags included
        assert_eq!(filtered(&c, json!({"tag": "#读书/摘抄"})), [a.id]);
        assert_eq!(filtered(&c, json!({"tag": "读"})), Vec::<i64>::new());
        assert_eq!(filtered(&c, json!({"q": "RUST"})), [d.id, b.id]);
        assert_eq!(filtered(&c, json!({"q": "rust 宏"})), [d.id]);
        assert_eq!(filtered(&c, json!({"tag": "读书", "q": "rust"})), [b.id]);
        let today = crate::modules::todo::today(&c).unwrap();
        assert_eq!(filtered(&c, json!({"day": today})).len(), 3);
        assert!(filtered(&c, json!({"day": "2000-01-01"})).is_empty());

        // pinned: on top only when nothing is filtered; pinning keeps updated_at
        let p = update(&c, a.id, np(json!({"pinned": true}))).unwrap();
        assert_eq!((p.pinned, p.updated_at.as_str()), (true, a.updated_at.as_str()));
        assert_eq!(filtered(&c, json!({})), [a.id, d.id, b.id]);
        assert_eq!(filtered(&c, json!({"tag": "读书"})), [b.id, a.id]);

        let e = update(&c, b.id, np(json!({"content": "#写作 改过了"}))).unwrap();
        assert_eq!(e.tags, ["写作"]);
        assert_eq!(filtered(&c, json!({"tag": "读书"})), [a.id]);
        let tags: Vec<String> = c.prepare("SELECT tag FROM note_tags ORDER BY tag").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(tags, ["写作", "读书/摘抄"]);

        delete(&c, a.id).unwrap();
        assert!(delete(&c, a.id).is_err());
        assert!(get(&c, a.id).is_err());
        let n: i64 = c.query_row("SELECT count(*) FROM note_tags", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn refs_both_ways() {
        let c = memory_db();
        let a = add(&c, "#想法 原来的想法");
        let b = add(&c, "另一条");
        let q = create(&c, np(json!({"content": "批注", "refs": [a.id, b.id, a.id]}))).unwrap();
        assert_eq!(q.refs.iter().map(|r| (r.id, r.excerpt.as_str())).collect::<Vec<_>>(), [(a.id, "原来的想法"), (b.id, "另一条")]);
        assert_eq!(ids_of(&get(&c, a.id).unwrap().backrefs), [q.id]);
        assert!(update(&c, q.id, np(json!({"refs": [q.id]}))).is_err());
        assert!(create(&c, np(json!({"content": "x", "refs": [9999]}))).is_err());
        let q = update(&c, q.id, np(json!({"refs": [b.id]}))).unwrap();
        assert_eq!(ids_of(&q.refs), [b.id]);
        assert!(get(&c, a.id).unwrap().backrefs.is_empty());
        delete(&c, b.id).unwrap(); // the quoting note stays, the link goes
        assert!(get(&c, q.id).unwrap().refs.is_empty());
    }

    fn ids_of(bs: &[Brief]) -> Vec<i64> {
        bs.iter().map(|b| b.id).collect()
    }

    #[test]
    fn rename_and_merge_tags() {
        let c = memory_db();
        let a = add(&c, "#宏观叙事/商业 一 #宏观叙事");
        let b = add(&c, "#宏观叙事/商业/案例 二");
        let d = add(&c, "#商业 三");
        assert_eq!(rename_tag(&c, "宏观叙事/商业", "#商业").unwrap(), 2);
        assert_eq!(get(&c, a.id).unwrap().content, "#商业 一 #宏观叙事");
        assert_eq!(get(&c, b.id).unwrap().content, "#商业/案例 二");
        assert_eq!(filtered(&c, json!({"tag": "商业"})), [d.id, b.id, a.id]); // merged
        assert!(rename_tag(&c, "商业", "商业/子").is_err());
        assert!(rename_tag(&c, "商业", "有 空格").is_err());
        assert!(rename_tag(&c, "不存在", "x").is_err());
        assert_eq!(rename_tag(&c, "商业", "商业").unwrap(), 0);
        let o = overview(&c).unwrap();
        let tags: Vec<(&str, i64)> = o.tags.iter().map(|t| (t.tag.as_str(), t.count)).collect();
        assert_eq!(tags, [("商业", 3), ("商业/案例", 1), ("宏观叙事", 1)]);
        assert_eq!((o.total, o.days.len(), o.days[0].count), (3, 1, 3));
    }

    #[test]
    fn tag_settings_pin_icon_and_follow_renames() {
        let c = memory_db();
        add(&c, "#读书/摘抄 一");
        add(&c, "#读书 二 #电影");
        add(&c, "#舒宇 三");
        let tp = |v: serde_json::Value| -> TagPatch { serde_json::from_value(v).unwrap() };
        set_tag(&c, "#电影", tp(json!({"pinned": true}))).unwrap();
        set_tag(&c, "读书", tp(json!({"pinned": true, "icon": "i:book-open"}))).unwrap();
        assert!(set_tag(&c, "读书/摘抄", tp(json!({"pinned": true}))).unwrap_err().contains("第一级"));
        set_tag(&c, "读书/摘抄", tp(json!({"icon": "📖"}))).unwrap(); // icons: any level
        set_tag(&c, "舒宇", tp(json!({"icon": "🍊"}))).unwrap();
        assert!(set_tag(&c, "没有", tp(json!({"pinned": true}))).is_err());
        assert!(set_tag(&c, "电影", tp(json!({"icon": "太长了太长了太长了太长了太长了太长了"}))).is_err());
        let o = overview(&c).unwrap();
        assert_eq!(o.pinned, ["电影", "读书"]); // pin order
        let icon = |o: &Overview, t: &str| o.tags.iter().find(|x| x.tag == t).unwrap().icon.clone();
        assert_eq!((icon(&o, "读书").as_deref(), icon(&o, "读书/摘抄").as_deref()), (Some("i:book-open"), Some("📖")));
        // renaming a first-level tag keeps its pin; the sub-tag's settings move with it
        rename_tag(&c, "读书", "书").unwrap();
        let o = overview(&c).unwrap();
        assert_eq!((o.pinned.clone(), icon(&o, "书/摘抄").as_deref()), (vec!["电影".to_string(), "书".to_string()], Some("📖")));
        // becoming a sub-tag drops the pin, not the icon
        rename_tag(&c, "书", "爱好/书").unwrap();
        let o = overview(&c).unwrap();
        assert_eq!((o.pinned.clone(), icon(&o, "爱好/书").as_deref()), (vec!["电影".to_string()], Some("i:book-open")));
        // merging keeps the target's icon
        set_tag(&c, "电影", tp(json!({"icon": "🎬"}))).unwrap();
        rename_tag(&c, "舒宇", "电影").unwrap();
        assert_eq!(icon(&overview(&c).unwrap(), "电影").as_deref(), Some("🎬"));
        // unpin + clear icon → no settings left
        set_tag(&c, "电影", tp(json!({"pinned": false, "icon": null}))).unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM note_tag_meta WHERE tag = '电影'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        // a tag whose notes are gone keeps its settings (exported too)
        let only = list(&c, &serde_json::from_value(json!({"tag": "爱好/书/摘抄"})).unwrap()).unwrap()[0].id;
        delete(&c, only).unwrap();
        let out = export(&c).unwrap();
        assert_eq!(serde_json::to_value(&out.tags).unwrap(), json!([{"tag": "爱好/书", "icon": "i:book-open"}, {"tag": "爱好/书/摘抄", "icon": "📖"}]));
        let c2 = memory_db();
        replace_all(&c2, &out).unwrap();
        assert_eq!(serde_json::to_value(&out).unwrap(), serde_json::to_value(export(&c2).unwrap()).unwrap());
        // an old file pinning a sub-tag: the pin is dropped
        replace_all(&c2, &serde_json::from_value(json!({"notes": [{"content": "#a/b x"}], "tags": [{"tag": "a/b", "pinned": true}, {"tag": "a", "pinned": true}]})).unwrap()).unwrap();
        assert_eq!(overview(&c2).unwrap().pinned, ["a"]);
        assert!(validate_records(&serde_json::from_value(json!({"tags": [{"tag": "a"}, {"tag": "#a"}]})).unwrap()).is_err());
    }

    #[test]
    fn records_roundtrip() {
        let c = memory_db();
        let rec: NotesRecord = serde_json::from_value(json!({"notes": [
            {"content": "#读书 旧的", "createdAt": "2024-12-12T09:48:10.000Z"},
            {"content": "引用旧的", "pinned": true, "refs": ["2024-12-12T09:48:10.000Z"], "createdAt": "2025-01-01T00:00:00.000Z", "updatedAt": "2025-01-02T00:00:00.000Z"},
            {"content": "没时间的"}
        ]}))
        .unwrap();
        assert_eq!(replace_all(&c, &rec).unwrap(), 3);
        let all = list(&c, &Filter::default()).unwrap();
        assert_eq!(all[0].content, "引用旧的"); // pinned
        assert_eq!(all[0].refs[0].excerpt, "旧的");
        let out = export(&c).unwrap();
        assert_eq!(out.notes[1].updated_at.as_deref(), Some("2025-01-02T00:00:00.000Z"));
        assert_eq!(out.notes[0].updated_at.as_deref(), Some("2024-12-12T09:48:10.000Z"));
        let c2 = memory_db();
        replace_all(&c2, &out).unwrap();
        assert_eq!(serde_json::to_value(&out).unwrap(), serde_json::to_value(export(&c2).unwrap()).unwrap());
        assert_eq!(overview(&c2).unwrap().tags.len(), 1);

        let bad = |v: serde_json::Value| validate_records(&serde_json::from_value(v).unwrap()).is_err();
        assert!(bad(json!({"notes": [{"content": " "}]})));
        assert!(bad(json!({"notes": [{"content": "a", "createdAt": "2025-01-01"}]})));
        assert!(bad(json!({"notes": [{"content": "a", "createdAt": "2025-01-01T00:00:00.000Z"}, {"content": "b", "createdAt": "2025-01-01T00:00:00.000Z"}]})));
        assert!(bad(json!({"notes": [{"content": "a", "refs": ["2025-01-01T00:00:00.000Z"]}]})));
        assert!(bad(json!({"notes": [{"content": "a", "createdAt": "2025-01-01T00:00:00.000Z", "refs": ["2025-01-01T00:00:00.000Z"]}]})));
        assert!(serde_json::from_value::<NotesRecord>(json!({"notes": [{"content": "a", "tags": []}]})).is_err());
    }
}
