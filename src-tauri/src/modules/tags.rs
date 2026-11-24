//! Inline #tags, shared by 念念不忘 (notes.rs) and 见字如面 (articles.rs): each module has its own tables
//! (`Tables`) and its own way to read / rewrite / remove an item (`Items`); the rules are one.
//! - A tag starts with `#` at the start of the text or after whitespace and runs to the next whitespace
//!   or `#`; trailing `/` is dropped; valid when non-empty, ≤ 50 chars, no leading `/`, no `//` (the
//!   UI mirrors this). In Markdown (见字如面) code spans and fenced blocks don't count.
//! - `<tags>` (item, tag) is re-derived from the text on every write; filtering by a tag includes sub-tags.
//!   Renaming rewrites the items' text (into an existing tag = merge); dragging under another tag is a
//!   rename too. Deleting takes `#tag` (and sub-tags) out of the text — an item that would be left
//!   invalid (念念不忘: empty) stays as it was — or removes the items using it (the module decides how).
//! - Settings (`<meta>`, by full path): pinned (first level only), an icon (emoji or `i:<name>`) and a
//!   place among siblings (dragged; never-placed ones follow in pinyin order, the UI sorts). They outlive
//!   a tag's last item, move on rename; deleting a tag drops those no item uses any more.

use super::common::{self, err, NOW};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const TAG_MAX: usize = 50;

/// One module's tag tables. `items` has `id`; `tags` is (`fk`, tag); `live` is an SQL condition on
/// the item (alias `n`) for what the tag tree counts.
pub struct Tables {
    pub items: &'static str,
    pub tags: &'static str,
    pub fk: &'static str,
    pub meta: &'static str,
    pub live: &'static str,
    /// Code spans / fences hide tags.
    pub markdown: bool,
}

/// How a module reads, rewrites and removes its items (rename / delete of a tag go through it).
pub trait Items {
    fn tables(&self) -> &'static Tables;
    fn text(&self, conn: &Connection, id: i64) -> Result<String, String>;
    /// The text is valid for the module (else a strip leaves the item as it was).
    fn valid(&self, text: &str) -> bool;
    /// Writes a rewritten text: content, derived tags (`sync`), updated_at.
    fn write(&self, conn: &Connection, id: i64, text: &str) -> Result<(), String>;
    /// 「连笔记一起删」.
    fn remove(&self, conn: &Connection, ids: &[i64]) -> Result<(), String>;
}

// ---------- parsing ----------

fn valid_tag(t: &str) -> bool {
    !t.is_empty() && !t.starts_with('/') && !t.contains("//") && t.chars().count() <= TAG_MAX
}

/// A tag as written: `#` at `hash`, the token running to `end` (a trailing `/` included).
pub struct Token {
    pub hash: usize,
    pub end: usize,
    pub tag: String,
}

/// `text` with code (spans and fenced blocks) blanked out, byte for byte, so offsets still match.
pub fn mask_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let blank = |s: &str, out: &mut String| out.extend(s.chars().map(|c| if c == '\n' { "\n".to_string() } else { " ".repeat(c.len_utf8()) }));
    let mut fence: Option<String> = None;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        let marker: String = t.chars().take_while(|&c| c == '`' || c == '~').collect();
        if let Some(f) = &fence {
            blank(line, &mut out);
            if t.starts_with(f.as_str()) && t.trim_end().chars().all(|c| c == f.chars().next().unwrap()) {
                fence = None;
            }
            continue;
        }
        if marker.len() >= 3 && marker.chars().all(|c| c == marker.chars().next().unwrap()) {
            fence = Some(marker);
            blank(line, &mut out);
            continue;
        }
        // code spans: a run of backticks up to the same run
        let mut rest = line;
        while let Some(i) = rest.find('`') {
            out.push_str(&rest[..i]);
            let run = rest[i..].bytes().take_while(|&b| b == b'`').count();
            let after = &rest[i + run..];
            let close = after.match_indices(&"`".repeat(run)).find(|(j, _)| !after[j + run..].starts_with('`') && (*j == 0 || !after[..*j].ends_with('`')));
            match close {
                Some((j, _)) => {
                    blank(&rest[i..i + run + j + run], &mut out);
                    rest = &after[j + run..];
                }
                None => {
                    out.push_str(&rest[i..i + run]);
                    rest = after;
                }
            }
        }
        out.push_str(rest);
    }
    out
}

pub fn tokens(text: &str, markdown: bool) -> Vec<Token> {
    let masked;
    let text = if markdown {
        masked = mask_code(text);
        masked.as_str()
    } else {
        text
    };
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
                out.push(Token { hash: i, end, tag: tag.to_string() });
            }
            prev = text[..end].chars().next_back();
            continue;
        }
        prev = Some(c);
    }
    out
}

/// Tags in `text` as (byte start, byte end, tag) — the span covers the tag after `#`.
pub fn tag_spans(text: &str, markdown: bool) -> Vec<(usize, usize, String)> {
    tokens(text, markdown).into_iter().map(|t| (t.hash + 1, t.hash + 1 + t.tag.len(), t.tag)).collect()
}

/// Distinct tags in order of appearance.
pub fn tags_in(text: &str, markdown: bool) -> Vec<String> {
    let mut seen = HashSet::new();
    tag_spans(text, markdown).into_iter().map(|(_, _, t)| t).filter(|t| seen.insert(t.clone())).collect()
}

/// `text` without the tags at or under `root`: one space next to each goes with it, and a line (or
/// list item) left with nothing goes too.
pub fn strip_tag(text: &str, root: &str, markdown: bool) -> String {
    let mut lines = Vec::new();
    // code fences span lines: mask the whole text once, then cut it the same way
    let masked = if markdown { mask_code(text) } else { text.to_string() };
    let mut at = 0;
    for line in text.split('\n') {
        let shadow = &masked[at..at + line.len()];
        at += line.len() + 1;
        let toks: Vec<Token> = tokens(shadow, false).into_iter().filter(|t| under(&t.tag, root)).collect();
        if toks.is_empty() {
            lines.push(line.to_string());
            continue;
        }
        let mut out = String::with_capacity(line.len());
        let mut last = 0;
        for t in toks {
            let (mut a, mut b) = (t.hash, t.end);
            match line[b..].chars().next() {
                Some(c) if c.is_whitespace() => b += c.len_utf8(),
                _ => {
                    if let Some(c) = line[last..a].chars().next_back().filter(|c| c.is_whitespace()) {
                        a -= c.len_utf8();
                    }
                }
            }
            out.push_str(&line[last..a]);
            last = b;
        }
        out.push_str(&line[last..]);
        let left = out.trim();
        let bare = left.is_empty() || left == "-" || left == "*" || left.strip_suffix(['.', ')']).is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
        if !bare {
            lines.push(out.trim_end().to_string());
        }
    }
    lines.join("\n")
}

/// A tag name typed by the user (rename target): optional leading `#`.
pub fn clean_tag(s: &str) -> Result<String, String> {
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

pub fn under(tag: &str, root: &str) -> bool {
    tag == root || tag.strip_prefix(root).is_some_and(|r| r.starts_with('/'))
}

pub fn parent_of(tag: &str) -> Option<&str> {
    tag.rfind('/').map(|i| &tag[..i])
}

/// `text` with every tag at or under `from` moved to `to` (sub-paths kept).
pub fn replace_tag(text: &str, from: &str, to: &str, markdown: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (a, b, tag) in tag_spans(text, markdown) {
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

// ---------- derived rows ----------

/// `<tags>` of item `id` := `tags` (only the differences are written).
pub fn sync(conn: &Connection, t: &Tables, id: i64, tags: &[String]) -> Result<(), String> {
    let want: HashSet<&String> = tags.iter().collect();
    let mut st = conn.prepare(&format!("SELECT tag FROM {} WHERE {} = ?1", t.tags, t.fk)).map_err(err)?;
    let have: HashSet<String> = st.query_map([id], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    for x in have.iter().filter(|x| !want.contains(x)) {
        conn.execute(&format!("DELETE FROM {} WHERE {} = ?1 AND tag = ?2", t.tags, t.fk), params![id, x]).map_err(err)?;
    }
    for x in want.into_iter().filter(|x| !have.contains(*x)) {
        conn.execute(&format!("INSERT INTO {} ({}, tag) VALUES (?1, ?2)", t.tags, t.fk), params![id, x]).map_err(err)?;
    }
    Ok(())
}

/// Ids of the items at or under `tag`; none is an error.
fn using(conn: &Connection, t: &Tables, tag: &str) -> Result<Vec<i64>, String> {
    let mut st = conn
        .prepare(&format!("SELECT DISTINCT {fk} FROM {tags} WHERE tag = ?1 OR substr(tag, 1, length(?1) + 1) = ?1 || '/' ORDER BY {fk}", fk = t.fk, tags = t.tags))
        .map_err(err)?;
    let ids: Vec<i64> = st.query_map([tag], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    if ids.is_empty() {
        return Err(format!("没有笔记用到「#{tag}」"));
    }
    Ok(ids)
}

fn used(conn: &Connection, t: &Tables, tag: &str) -> Result<bool, String> {
    conn.query_row(&format!("SELECT EXISTS (SELECT 1 FROM {} WHERE tag = ?1 OR substr(tag, 1, length(?1) + 1) = ?1 || '/')", t.tags), [tag], |r| r.get(0))
        .map_err(err)
}

// ---------- rename / delete / place ----------

/// Moves `from` (and its sub-tags) to `to` in every item using it; returns how many changed.
pub fn rename(conn: &Connection, items: &dyn Items, from: &str, to: &str) -> Result<usize, String> {
    let t = items.tables();
    let from = from.trim().trim_start_matches('#');
    let to = clean_tag(to)?;
    if to == from {
        return Ok(0);
    }
    if under(&to, from) {
        return Err("不能改成它自己下面的标签".into());
    }
    let ids = using(conn, t, from)?;
    let tx = common::Tx::begin(conn)?;
    for &id in &ids {
        let text = items.text(&tx, id)?;
        items.write(&tx, id, &replace_tag(&text, from, &to, t.markdown))?;
    }
    move_meta(&tx, t, from, &to)?;
    tx.commit()?;
    Ok(ids.len())
}

/// Takes `#tag` (and its sub-tags) out of every item using it; with `items`, removes those items instead.
/// Returns (items changed or removed, items kept as they were because stripping would leave them invalid).
/// Settings of the tags no item uses any more go too.
pub fn delete(conn: &Connection, items: &dyn Items, tag: &str, with_items: bool) -> Result<(usize, usize), String> {
    let t = items.tables();
    let tag = tag.trim().trim_start_matches('#');
    let ids = using(conn, t, tag)?;
    let tx = common::Tx::begin(conn)?;
    let mut kept = 0;
    if with_items {
        items.remove(&tx, &ids)?;
    } else {
        for &id in &ids {
            let next = strip_tag(&items.text(&tx, id)?, tag, t.markdown);
            if items.valid(&next) {
                items.write(&tx, id, &next)?;
            } else {
                kept += 1;
            }
        }
    }
    tx.execute(
        &format!(
            "DELETE FROM {meta} WHERE (tag = ?1 OR substr(tag, 1, length(?1) + 1) = ?1 || '/')
               AND NOT EXISTS (SELECT 1 FROM {tags} t WHERE t.tag = {meta}.tag OR substr(t.tag, 1, length({meta}.tag) + 1) = {meta}.tag || '/')",
            meta = t.meta,
            tags = t.tags
        ),
        [tag],
    )
    .map_err(err)?;
    tx.commit()?;
    Ok((ids.len() - kept, kept))
}

/// Drag in the tag list: `tag` goes under `parent` (None = first level; a new parent is a rename),
/// then `order` (full paths of `parent`'s direct children, the moved one by its new path) is their
/// order from now on. Children left out keep theirs. Returns the items rewritten by the move.
pub fn place(conn: &Connection, items: &dyn Items, tag: &str, parent: Option<&str>, order: &[String]) -> Result<usize, String> {
    let t = items.tables();
    let tag = tag.trim().trim_start_matches('#');
    let parent = parent.map(|p| p.trim().trim_start_matches('#')).filter(|p| !p.is_empty());
    let leaf = tag.rsplit('/').next().unwrap_or(tag);
    let to = match parent {
        Some(p) => format!("{p}/{leaf}"),
        None => leaf.to_string(),
    };
    if parent.is_some_and(|p| under(p, tag)) {
        return Err("不能移到它自己下面".into());
    }
    let tx = common::Tx::begin(conn)?;
    let moved = if to == tag { 0 } else { rename(&tx, items, tag, &to)? };
    let mut seen = HashSet::new();
    let mut n = 0;
    for x in order {
        let x = x.trim().trim_start_matches('#');
        if parent_of(x) != parent {
            return Err(format!("「#{x}」不在同一层"));
        }
        if !seen.insert(x) {
            continue;
        }
        if !used(&tx, t, x)? {
            return Err(format!("没有笔记用到「#{x}」，请重新拖一次"));
        }
        n += 1;
        let mut m = meta_of(&tx, t, x)?.unwrap_or_default();
        m.order = Some(n);
        put_meta(&tx, t, x, m)?;
    }
    tx.commit()?;
    Ok(moved)
}

/// The pinned tags in `order` (all of them, as listed); pins of tags no item uses go after.
pub fn order_pins(conn: &Connection, t: &Tables, order: &[String]) -> Result<(), String> {
    let pinned: Vec<String> = tree(conn, t)?.1;
    let order: Vec<&str> = order.iter().map(|x| x.trim().trim_start_matches('#')).collect();
    if order.len() != pinned.len() || !pinned.iter().all(|p| order.contains(&p.as_str())) {
        return Err("置顶的标签已经变了，请重新拖一次".into());
    }
    let mut st = conn.prepare(&format!("SELECT tag FROM {} WHERE pin_order IS NOT NULL ORDER BY pin_order, tag", t.meta)).map_err(err)?;
    let rest: Vec<String> = st.query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<Vec<String>, _>>().map_err(err)?;
    let all = order.iter().map(|x| x.to_string()).chain(rest.into_iter().filter(|x| !order.contains(&x.as_str())));
    let tx = common::Tx::begin(conn)?;
    for (i, x) in all.enumerate() {
        tx.execute(&format!("UPDATE {} SET pin_order = ?1 WHERE tag = ?2", t.meta), params![i as i64 + 1, x]).map_err(err)?;
    }
    tx.commit()
}

// ---------- settings (pinned, icon, order) ----------

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Meta {
    pub pin: Option<i64>,
    pub icon: Option<String>,
    /// Place among its siblings (None = never placed).
    pub order: Option<i64>,
}

fn meta_of(conn: &Connection, t: &Tables, tag: &str) -> Result<Option<Meta>, String> {
    conn.query_row(&format!("SELECT pin_order, icon, sort_order FROM {} WHERE tag = ?1", t.meta), [tag], |r| Ok(Meta { pin: r.get(0)?, icon: r.get(1)?, order: r.get(2)? }))
        .optional()
        .map_err(err)
}

/// Writes a tag's settings; a row with nothing set is removed.
pub fn put_meta(conn: &Connection, t: &Tables, tag: &str, m: Meta) -> Result<(), String> {
    if m == Meta::default() {
        conn.execute(&format!("DELETE FROM {} WHERE tag = ?1", t.meta), [tag]).map_err(err)?;
    } else {
        conn.execute(
            &format!(
                "INSERT INTO {} (tag, pin_order, icon, sort_order) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(tag) DO UPDATE SET pin_order = excluded.pin_order, icon = excluded.icon, sort_order = excluded.sort_order, updated_at = {NOW}",
                t.meta
            ),
            params![tag, m.pin, m.icon, m.order],
        )
        .map_err(err)?;
    }
    Ok(())
}

/// Rename: settings of `from` and its sub-tags follow; merging into a tag that has its own keeps
/// the target's icon and place (else takes this one's) and the earlier pin. A pin doesn't survive
/// becoming a sub-tag; the place doesn't survive a new parent (it is a place among the old siblings).
fn move_meta(conn: &Connection, t: &Tables, from: &str, to: &str) -> Result<(), String> {
    let rows = all_meta(conn, t)?;
    let new_parent = parent_of(from) != parent_of(to);
    for (tag, m) in rows.into_iter().filter(|(x, _)| under(x, from)) {
        let target = format!("{to}{}", &tag[from.len()..]);
        let have = meta_of(conn, t, &target)?.unwrap_or_default();
        let pin = match (have.pin, m.pin) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
        .filter(|_| !target.contains('/'));
        let order = have.order.or(m.order.filter(|_| !(new_parent && tag == from)));
        conn.execute(&format!("DELETE FROM {} WHERE tag = ?1", t.meta), [&tag]).map_err(err)?;
        put_meta(conn, t, &target, Meta { pin, icon: have.icon.or(m.icon), order })?;
    }
    Ok(())
}

/// Every tag's settings, pinned ones first in pin order.
pub fn all_meta(conn: &Connection, t: &Tables) -> Result<Vec<(String, Meta)>, String> {
    let mut st = conn.prepare(&format!("SELECT tag, pin_order, icon, sort_order FROM {} ORDER BY pin_order IS NULL, pin_order, tag", t.meta)).map_err(err)?;
    let rows = st
        .query_map([], |r| Ok((r.get(0)?, Meta { pin: r.get(1)?, icon: r.get(2)?, order: r.get(3)? })))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    Ok(rows)
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

impl TagPatch {
    /// The undo label.
    pub fn label(&self) -> &'static str {
        match self.pinned {
            Some(true) => "置顶标签",
            Some(false) => "取消置顶标签",
            None => "标签图标",
        }
    }
}

/// Pins / unpins a first-level tag (a new pin goes last) or sets any tag's icon. Only tags some item uses.
pub fn set(conn: &Connection, t: &Tables, tag: &str, p: TagPatch) -> Result<(), String> {
    let tag = clean_tag(tag)?;
    if p.pinned == Some(true) && tag.contains('/') {
        return Err("只能置顶第一级标签".into());
    }
    if !used(conn, t, &tag)? {
        return Err(format!("没有笔记用到「#{tag}」"));
    }
    let mut m = meta_of(conn, t, &tag)?.unwrap_or_default();
    m.pin = match p.pinned {
        Some(true) if m.pin.is_none() => Some(conn.query_row(&format!("SELECT coalesce(max(pin_order), 0) + 1 FROM {}", t.meta), [], |r| r.get::<_, i64>(0)).map_err(err)?),
        Some(false) => None,
        _ => m.pin,
    };
    if let Some(i) = p.icon {
        m.icon = common::clean_icon(i)?;
    }
    put_meta(conn, t, &tag, m)
}

// ---------- the tag tree ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    /// Full path; every ancestor of a used tag is listed too.
    pub tag: String,
    /// Live items at or under this tag.
    pub count: i64,
    /// Emoji or `i:<name>` shown instead of 「#」.
    pub icon: Option<String>,
    /// Place among its siblings once dragged (None: after those, in pinyin order — the UI sorts).
    pub order: Option<i64>,
}

/// (tags with counts of live items, pinned first-level tags in use in pin order).
pub fn tree(conn: &Connection, t: &Tables) -> Result<(Vec<TagCount>, Vec<String>), String> {
    let mut st = conn.prepare(&format!("SELECT x.{fk}, x.tag FROM {tags} x JOIN {items} n ON n.id = x.{fk} WHERE {live}", fk = t.fk, tags = t.tags, items = t.items, live = t.live)).map_err(err)?;
    let pairs: Vec<(i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut by: BTreeMap<String, HashSet<i64>> = BTreeMap::new();
    for (id, tag) in pairs {
        let mut path = String::new();
        for seg in tag.split('/') {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(seg);
            by.entry(path.clone()).or_default().insert(id);
        }
    }
    let meta = all_meta(conn, t)?;
    let pinned = meta.iter().filter(|(x, m)| m.pin.is_some() && !x.contains('/') && by.contains_key(x)).map(|(x, _)| x.clone()).collect();
    let by_tag: HashMap<&str, &Meta> = meta.iter().map(|(x, m)| (x.as_str(), m)).collect();
    let tags = by
        .iter()
        .map(|(tag, ids)| {
            let m = by_tag.get(tag.as_str());
            TagCount { tag: tag.clone(), count: ids.len() as i64, icon: m.and_then(|m| m.icon.clone()), order: m.and_then(|m| m.order) }
        })
        .collect();
    Ok((tags, pinned))
}

// ---------- data files: { tag, pinned?, icon?, order? } (pinned ones in pin order) ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TagRecord {
    pub tag: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Place among its siblings (v36 and older: none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<i64>,
}

pub fn check_records(label: &str, recs: &[TagRecord]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for r in recs {
        let ctx = |e: String| format!("{label}的标签「{}」：{e}", r.tag);
        let tag = clean_tag(&r.tag).map_err(ctx)?;
        common::clean_icon(r.icon.clone()).map_err(ctx)?;
        if !seen.insert(tag) {
            return Err(ctx("重复了".into()));
        }
    }
    Ok(())
}

pub fn export_records(conn: &Connection, t: &Tables) -> Result<Vec<TagRecord>, String> {
    Ok(all_meta(conn, t)?.into_iter().map(|(tag, m)| TagRecord { tag, pinned: m.pin.is_some(), icon: m.icon, order: m.order }).collect())
}

/// Replaces every tag's settings (records already checked).
pub fn import_records(conn: &Connection, t: &Tables, recs: &[TagRecord]) -> Result<(), String> {
    conn.execute(&format!("DELETE FROM {}", t.meta), []).map_err(err)?;
    for (i, r) in recs.iter().enumerate() {
        let tag = clean_tag(&r.tag)?;
        // v10 files may pin a sub-tag: dropped, as migration 22 did
        put_meta(conn, t, &tag, Meta { pin: (r.pinned && !tag.contains('/')).then_some(i as i64 + 1), icon: common::clean_icon(r.icon.clone())?, order: r.order })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_code_hides_tags() {
        let t = "#读书 `#不是` 正文\n```\n#也不是\n```\n~~~js\nx = '#no'\n~~~\n#是/子 和 ``a`#b`` #c";
        assert_eq!(tags_in(t, true), ["读书", "是/子", "c"]);
        assert_eq!(tags_in(t, false), ["读书", "也不是", "是/子", "c"]);
        assert_eq!(mask_code(t).len(), t.len());
        // a heading is not a tag, nor a link's anchor
        assert!(tags_in("# 标题\n## 小标题\n[x](#a) http://a.b/#c", true).is_empty());
    }

    #[test]
    fn rename_and_strip_skip_code() {
        let t = "#a 正文 `#a` 后\n```\n#a\n```\n#a/b";
        assert_eq!(replace_tag(t, "a", "z", true), "#z 正文 `#a` 后\n```\n#a\n```\n#z/b");
        assert_eq!(strip_tag(t, "a", true), "正文 `#a` 后\n```\n#a\n```");
    }
}
