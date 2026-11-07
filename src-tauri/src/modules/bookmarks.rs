//! 一网打尽 (网址收藏夹; requirements「一网打尽」): folders and links in one tree (table bookmarks, 0039).
//! - parent_id None = the top level, which holds folders only (0040: links left there went into 「未分类」);
//!   a link is always in a folder. Folders nest to any depth. sort_order = place among its siblings,
//!   folders and links mixed (as dragged); new ones go last.
//! - A link's url: http(s) only; a bare address gets https://. No name: the host.
//! - Placing (drag, 移到…): several at once, into a folder (or the top level, folders only) before one of its
//!   rows or last, in the order given; never a folder into itself or under it. One undo step.
//! - A data file from before 0040 with links at the top level: they go into a 「未分类」 folder, last.
//! - Deleting a folder deletes what's in it (FK cascade). Every edit is one undo step (commands).
//! - on_home: the link shows on 首页 (home.rs), in the tree's order.
//! - Site icons: icons.rs (a cache, not data). Search: search/sources.rs; 柚子: youzi/data.rs (read only).

use super::common::{self, err, Tx};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

pub const KEY: &str = "bookmarks";
pub const LABEL: &str = "一网打尽";
const NOT_FOUND: &str = "找不到这一条，可能已经被删除了";
const FOLDER: &str = "folder";
const LINK: &str = "link";
/// Where links left at the top level go (0040, old data files).
pub const LOOSE: &str = "未分类";
const NO_LOOSE: &str = "网址要放在文件夹里";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: i64,
    pub parent_id: Option<i64>,
    /// folder | link
    pub kind: String,
    pub title: String,
    /// '' for a folder.
    pub url: String,
    pub on_home: bool,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl Bookmark {
    pub fn is_folder(&self) -> bool {
        self.kind == FOLDER
    }
}

/// A new folder or link.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewBookmark {
    #[serde(default)]
    pub parent_id: Option<i64>,
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub on_home: bool,
}

/// Absent = keep.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookmarkPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub on_home: Option<bool>,
}

const COLS: &str = "id, parent_id, kind, title, url, on_home, sort_order, created_at, updated_at";

fn from_row(r: &Row) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark { id: r.get(0)?, parent_id: r.get(1)?, kind: r.get(2)?, title: r.get(3)?, url: r.get(4)?, on_home: r.get(5)?, sort_order: r.get(6)?, created_at: r.get(7)?, updated_at: r.get(8)? })
}

/// Everything, siblings in order (the page builds the tree).
pub fn list(conn: &Connection) -> Result<Vec<Bookmark>, String> {
    let mut st = conn.prepare_cached(&format!("SELECT {COLS} FROM bookmarks ORDER BY parent_id IS NOT NULL, parent_id, sort_order, id")).map_err(err)?;
    let rows = st.query_map([], from_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Bookmark, String> {
    conn.query_row(&format!("SELECT {COLS} FROM bookmarks WHERE id = ?1"), [id], from_row).optional().map_err(err)?.ok_or_else(|| NOT_FOUND.to_string())
}

/// Folders and links.
pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM bookmarks", [], |r| r.get(0)).map_err(err)
}

pub fn links(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM bookmarks WHERE kind = 'link'", [], |r| r.get(0)).map_err(err)
}

/// The tree's order (depth first: a folder, then what's in it): every row, with its folders' names.
pub fn in_order(all: &[Bookmark]) -> Vec<(&Bookmark, Vec<&str>)> {
    let mut kids: HashMap<Option<i64>, Vec<&Bookmark>> = HashMap::new();
    for b in all {
        kids.entry(b.parent_id).or_default().push(b);
    }
    for v in kids.values_mut() {
        v.sort_by_key(|b| (b.sort_order, b.id));
    }
    fn walk<'a>(at: Option<i64>, path: &mut Vec<&'a str>, kids: &HashMap<Option<i64>, Vec<&'a Bookmark>>, out: &mut Vec<(&'a Bookmark, Vec<&'a str>)>) {
        for b in kids.get(&at).map(Vec::as_slice).unwrap_or(&[]) {
            out.push((b, path.clone()));
            if b.is_folder() {
                path.push(&b.title);
                walk(Some(b.id), path, kids, out);
                path.pop();
            }
        }
    }
    let mut out = Vec::with_capacity(all.len());
    walk(None, &mut Vec::new(), &kids, &mut out);
    out
}

/// Links shown on 首页, in the tree's order.
pub fn home(conn: &Connection) -> Result<Vec<Bookmark>, String> {
    let all = list(conn)?;
    Ok(in_order(&all).into_iter().filter(|(b, _)| !b.is_folder() && b.on_home).map(|(b, _)| b.clone()).collect())
}

/// `https://…` / `http://…` kept as written (trimmed); a bare address (`www.x.com/a`) gets https://.
pub fn normalize_url(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("网址不能是空的".into());
    }
    if s.chars().any(char::is_whitespace) {
        return Err("网址里不能有空格".into());
    }
    let lower = s.to_ascii_lowercase();
    let url = if lower.starts_with("https://") || lower.starts_with("http://") {
        s.to_string()
    } else if s.contains("://") || lower.starts_with("javascript:") || lower.starts_with("data:") || lower.starts_with("file:") {
        return Err("只收 http 和 https 的网址".into());
    } else {
        format!("https://{s}")
    };
    if host_of(&url).is_empty() {
        return Err("网址不对：没有网站的地址".into());
    }
    if url.chars().count() > 4000 {
        return Err("网址太长了".into());
    }
    Ok(url)
}

/// The host of an http(s) address (lowercase, without user, port, path): `www.x.com`.
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host.starts_with('[') { host.split_inclusive(']').next().unwrap_or(host) } else { host.split(':').next().unwrap_or(host) };
    host.to_ascii_lowercase()
}

fn check_parent(conn: &Connection, parent: Option<i64>) -> Result<(), String> {
    if let Some(p) = parent {
        let kind: Option<String> = conn.query_row("SELECT kind FROM bookmarks WHERE id = ?1", [p], |r| r.get(0)).optional().map_err(err)?;
        match kind.as_deref() {
            Some(FOLDER) => {}
            Some(_) => return Err("只能放进文件夹".into()),
            None => return Err("这个文件夹已经不在了".into()),
        }
    }
    Ok(())
}

fn next_order(conn: &Connection, parent: Option<i64>) -> Result<i64, String> {
    conn.query_row("SELECT coalesce(max(sort_order), 0) + 1 FROM bookmarks WHERE parent_id IS ?1", [parent], |r| r.get(0)).map_err(err)
}

fn clean_title(title: &str, kind: &str, url: &str) -> Result<String, String> {
    let t = title.trim();
    if t.is_empty() {
        return if kind == LINK { Ok(host_of(url)) } else { Err("文件夹要有名字".into()) };
    }
    common::require(t, "名称", 300)
}

pub fn create(conn: &Connection, input: NewBookmark) -> Result<Bookmark, String> {
    let (kind, url) = match input.kind.as_str() {
        LINK => (LINK, normalize_url(&input.url)?),
        FOLDER => (FOLDER, String::new()),
        _ => return Err("只能是文件夹或网址".into()),
    };
    let title = clean_title(&input.title, kind, &url)?;
    if kind == LINK && input.parent_id.is_none() {
        return Err(NO_LOOSE.into());
    }
    check_parent(conn, input.parent_id)?;
    let order = next_order(conn, input.parent_id)?;
    conn.execute(
        "INSERT INTO bookmarks (parent_id, kind, title, url, on_home, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![input.parent_id, kind, title, url, input.on_home && kind == LINK, order],
    )
    .map_err(err)?;
    get(conn, conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, patch: BookmarkPatch) -> Result<Bookmark, String> {
    let old = get(conn, id)?;
    let url = match (&patch.url, old.is_folder()) {
        (Some(_), true) => return Err("文件夹没有网址".into()),
        (Some(u), false) => normalize_url(u)?,
        (None, _) => old.url.clone(),
    };
    let title = match &patch.title {
        Some(t) => clean_title(t, &old.kind, &url)?,
        None => old.title.clone(),
    };
    let on_home = patch.on_home.unwrap_or(old.on_home) && !old.is_folder();
    if (title.as_str(), url.as_str(), on_home) == (old.title.as_str(), old.url.as_str(), old.on_home) {
        return Ok(old);
    }
    conn.execute(
        "UPDATE bookmarks SET title = ?1, url = ?2, on_home = ?3, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?4",
        params![title, url, on_home, id],
    )
    .map_err(err)?;
    get(conn, id)
}

/// Puts `ids` (in this order) into `parent` (None: the top level), before `before` (one of its rows) or
/// last; the parent's rows are numbered again. A folder can't go into itself or under it.
pub fn place(conn: &Connection, ids: &[i64], parent: Option<i64>, before: Option<i64>) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    check_parent(conn, parent)?;
    let all = list(conn)?;
    let by_id: HashMap<i64, &Bookmark> = all.iter().map(|b| (b.id, b)).collect();
    let mut moving: Vec<i64> = Vec::new();
    for id in ids {
        match by_id.get(id) {
            None => return Err(NOT_FOUND.into()),
            Some(b) if parent.is_none() && !b.is_folder() => return Err(NO_LOOSE.into()),
            _ => {}
        }
        if !moving.contains(id) {
            moving.push(*id);
        }
    }
    // the parent and its ancestors: none of them may move
    let mut up = parent;
    while let Some(p) = up {
        if moving.contains(&p) {
            return Err("文件夹不能放进它自己或它里面的文件夹".into());
        }
        up = by_id.get(&p).and_then(|b| b.parent_id);
    }
    if let Some(b) = before {
        if by_id.get(&b).is_none_or(|x| x.parent_id != parent) {
            return Err("列表已经变了，请刷新后再拖".into());
        }
    }
    let mut siblings: Vec<&Bookmark> = all.iter().filter(|b| b.parent_id == parent && !moving.contains(&b.id)).collect();
    siblings.sort_by_key(|b| (b.sort_order, b.id));
    let mut order: Vec<i64> = siblings.iter().map(|b| b.id).collect();
    let at = before.filter(|b| !moving.contains(b)).and_then(|b| order.iter().position(|x| *x == b)).unwrap_or(order.len());
    order.splice(at..at, moving.iter().copied());
    let tx = Tx::begin(conn)?;
    for (i, id) in order.iter().enumerate() {
        let b = by_id[id];
        if b.parent_id != parent || b.sort_order != i as i64 + 1 {
            let moved = b.parent_id != parent;
            tx.execute(
                &format!("UPDATE bookmarks SET parent_id = ?1, sort_order = ?2{} WHERE id = ?3", if moved { ", updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')" } else { "" }),
                params![parent, i as i64 + 1, id],
            )
            .map_err(err)?;
        }
    }
    tx.commit()
}

/// Deletes `ids` (a folder with what's in it).
pub fn delete(conn: &Connection, ids: &[i64]) -> Result<usize, String> {
    let tx = Tx::begin(conn)?;
    let mut n = 0;
    for id in ids {
        n += tx.execute("DELETE FROM bookmarks WHERE id = ?1", [id]).map_err(err)?;
    }
    if n == 0 {
        return Err(NOT_FOUND.into());
    }
    tx.commit()?;
    Ok(n)
}

// ---------- import / export ----------

fn is_false(b: &bool) -> bool {
    !*b
}

/// One row of the tree, with what's in it (folders).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BookmarkRecord {
    /// folder | link
    pub kind: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub on_home: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<BookmarkRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

fn record_count(records: &[BookmarkRecord]) -> usize {
    records.iter().map(|r| 1 + record_count(&r.children)).sum()
}

/// Folders and links in the file.
pub fn records_len(records: &[BookmarkRecord]) -> usize {
    record_count(records)
}

fn check_record(r: &BookmarkRecord) -> Result<(String, String), String> {
    match r.kind.as_str() {
        LINK if r.children.is_empty() => {
            let url = normalize_url(&r.url)?;
            Ok((clean_title(&r.title, LINK, &url)?, url))
        }
        LINK => Err("网址里不能再放东西".into()),
        FOLDER if r.url.is_empty() && !r.on_home => Ok((clean_title(&r.title, FOLDER, "")?, String::new())),
        FOLDER => Err("文件夹没有网址，也不放首页".into()),
        _ => Err("只能是文件夹或网址".into()),
    }
}

pub fn validate_records(records: &[BookmarkRecord]) -> Result<(), String> {
    fn walk(rs: &[BookmarkRecord]) -> Result<(), String> {
        for r in rs {
            check_record(r).map_err(|e| format!("{LABEL}「{}」：{e}", r.title.trim()))?;
            walk(&r.children)?;
        }
        Ok(())
    }
    walk(records)
}

/// The whole tree, for the export file.
pub fn export(conn: &Connection) -> Result<Vec<BookmarkRecord>, String> {
    let all = list(conn)?;
    let mut kids: HashMap<Option<i64>, Vec<&Bookmark>> = HashMap::new();
    for b in &all {
        kids.entry(b.parent_id).or_default().push(b);
    }
    fn build(at: Option<i64>, kids: &HashMap<Option<i64>, Vec<&Bookmark>>) -> Vec<BookmarkRecord> {
        let mut rows = kids.get(&at).cloned().unwrap_or_default();
        rows.sort_by_key(|b| (b.sort_order, b.id));
        rows.into_iter()
            .map(|b| BookmarkRecord {
                kind: b.kind.clone(),
                title: b.title.clone(),
                url: b.url.clone(),
                on_home: b.on_home,
                children: if b.is_folder() { build(Some(b.id), kids) } else { vec![] },
                created_at: Some(b.created_at.clone()),
                updated_at: Some(b.updated_at.clone()),
            })
            .collect()
    }
    Ok(build(None, &kids))
}

/// Replaces the whole tree with `records` (as-is; links at the top level, from before 0040, into a 「未分类」
/// folder last). Caller owns the transaction.
pub fn replace_all(conn: &Connection, records: &[BookmarkRecord]) -> Result<usize, String> {
    conn.execute("DELETE FROM bookmarks", []).map_err(err)?;
    let (loose, mut top): (Vec<BookmarkRecord>, Vec<BookmarkRecord>) = records.iter().cloned().partition(|r| r.kind == LINK);
    if !loose.is_empty() {
        top.push(BookmarkRecord { kind: FOLDER.into(), title: LOOSE.into(), url: String::new(), on_home: false, children: loose, created_at: None, updated_at: None });
    }
    let records = top.as_slice();
    fn put(conn: &Connection, parent: Option<i64>, rs: &[BookmarkRecord]) -> Result<(), String> {
        for (i, r) in rs.iter().enumerate() {
            let (title, url) = check_record(r).map_err(|e| format!("{LABEL}「{}」：{e}", r.title.trim()))?;
            conn.execute(
                "INSERT INTO bookmarks (parent_id, kind, title, url, on_home, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, coalesce(?7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')), coalesce(?8, ?7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
                params![parent, r.kind, title, url, r.on_home, i as i64 + 1, r.created_at, r.updated_at],
            )
            .map_err(err)?;
            let id = conn.last_insert_rowid();
            put(conn, Some(id), &r.children)?;
        }
        Ok(())
    }
    put(conn, None, records)?;
    Ok(record_count(records))
}

// ---------- commands ----------

#[tauri::command]
pub fn bookmarks_list(state: State<AppState>) -> Result<Vec<Bookmark>, String> {
    state.with_db(|c| list(c))
}

#[tauri::command]
pub fn bookmarks_create(state: State<AppState>, input: NewBookmark) -> Result<Bookmark, String> {
    let label = if input.kind == FOLDER { "新建文件夹" } else { "添加网址" };
    state.with_undo(label, |c| create(c, input))
}

#[tauri::command]
pub fn bookmarks_update(state: State<AppState>, id: i64, patch: BookmarkPatch) -> Result<Bookmark, String> {
    state.with_undo("修改网址", |c| update(c, id, patch))
}

#[tauri::command]
pub fn bookmarks_place(state: State<AppState>, ids: Vec<i64>, parent_id: Option<i64>, before: Option<i64>) -> Result<(), String> {
    state.with_undo("移动网址", |c| place(c, &ids, parent_id, before))
}

#[tauri::command]
pub fn bookmarks_delete(state: State<AppState>, ids: Vec<i64>) -> Result<usize, String> {
    state.with_undo("删除网址", |c| delete(c, &ids))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    fn new(v: serde_json::Value) -> NewBookmark {
        serde_json::from_value(v).unwrap()
    }

    /// A small tree: 工作 { 邮箱 { Gmail }, GitHub }, 读书 { Google }; ids by title.
    pub(crate) fn seed(c: &Connection) -> HashMap<String, i64> {
        let mut ids = HashMap::new();
        let mut add = |parent: Option<&str>, kind: &str, title: &str, url: &str| {
            let parent_id = parent.map(|p| ids[p]);
            let b = create(c, new(json!({ "parentId": parent_id, "kind": kind, "title": title, "url": url }))).unwrap();
            ids.insert(title.to_string(), b.id);
        };
        add(None, FOLDER, "工作", "");
        add(Some("工作"), FOLDER, "邮箱", "");
        add(Some("邮箱"), LINK, "Gmail", "mail.google.com");
        add(Some("工作"), LINK, "GitHub", "https://github.com/");
        add(None, FOLDER, "读书", "");
        add(Some("读书"), LINK, "Google", "https://www.google.com/");
        ids
    }

    fn titles(c: &Connection, parent: Option<i64>) -> Vec<String> {
        list(c).unwrap().into_iter().filter(|b| b.parent_id == parent).map(|b| b.title).collect()
    }

    #[test]
    fn urls_and_hosts() {
        assert_eq!(normalize_url(" www.deepseek.com/zh ").unwrap(), "https://www.deepseek.com/zh");
        assert_eq!(normalize_url("http://127.0.0.1:8080/x").unwrap(), "http://127.0.0.1:8080/x");
        for bad in ["", "a b.com", "javascript:alert(1)", "ftp://x.com", "file:///etc", "https://"] {
            assert!(normalize_url(bad).is_err(), "{bad}");
        }
        assert_eq!(host_of("https://User@Platform.OpenAI.com:443/home?x=1#y"), "platform.openai.com");
        assert_eq!(host_of("https://bailian.console.aliyun.com/"), "bailian.console.aliyun.com");
    }

    #[test]
    fn creates_in_order_with_defaults() {
        let c = memory_db();
        let ids = seed(&c);
        assert_eq!(titles(&c, None), ["工作", "读书"]);
        let gmail = get(&c, ids["Gmail"]).unwrap();
        assert_eq!((gmail.url.as_str(), gmail.parent_id), ("https://mail.google.com", Some(ids["邮箱"])));
        assert_eq!(create(&c, new(json!({ "kind": "link", "url": "openrouter.ai/" }))).unwrap_err(), NO_LOOSE, "a link is always in a folder");
        let bare = create(&c, new(json!({ "kind": "link", "url": "openrouter.ai/", "parentId": ids["读书"] }))).unwrap();
        assert_eq!(bare.title, "openrouter.ai", "no name: the host");
        assert!(create(&c, new(json!({ "kind": "folder" }))).is_err());
        assert!(create(&c, new(json!({ "kind": "link", "url": "x.com", "parentId": ids["Google"] }))).is_err(), "only into a folder");
        assert!(create(&c, new(json!({ "kind": "tag", "title": "x" }))).is_err());
        let u = update(&c, bare.id, serde_json::from_value(json!({ "title": " OpenRouter ", "onHome": true })).unwrap()).unwrap();
        assert_eq!((u.title.as_str(), u.on_home), ("OpenRouter", true));
        assert!(update(&c, ids["工作"], serde_json::from_value(json!({ "url": "x.com" })).unwrap()).is_err());
        assert!(!update(&c, ids["工作"], serde_json::from_value(json!({ "onHome": true })).unwrap()).unwrap().on_home, "folders aren't on 首页");
        assert_eq!(home(&c).unwrap().iter().map(|b| b.title.as_str()).collect::<Vec<_>>(), ["OpenRouter"]);
    }

    #[test]
    fn placing_moves_and_orders() {
        let c = memory_db();
        let ids = seed(&c);
        // reorder at the top level (folders only there): 读书 first
        place(&c, &[ids["读书"]], None, Some(ids["工作"])).unwrap();
        assert_eq!(titles(&c, None), ["读书", "工作"]);
        assert_eq!(place(&c, &[ids["读书"], ids["Gmail"]], None, None).unwrap_err(), NO_LOOSE);
        assert_eq!(titles(&c, None), ["读书", "工作"]);
        place(&c, &[ids["工作"]], None, Some(ids["读书"])).unwrap();
        // several into a folder, last there, in the order given
        place(&c, &[ids["GitHub"], ids["Google"]], Some(ids["读书"]), None).unwrap();
        assert_eq!(titles(&c, Some(ids["读书"])), ["GitHub", "Google"]);
        place(&c, &[ids["Google"]], Some(ids["读书"]), Some(ids["GitHub"])).unwrap();
        assert_eq!(titles(&c, Some(ids["读书"])), ["Google", "GitHub"]);
        assert_eq!(titles(&c, Some(ids["工作"])), ["邮箱"]);
        // a folder never into itself or under it
        assert!(place(&c, &[ids["工作"]], Some(ids["邮箱"]), None).is_err());
        assert!(place(&c, &[ids["工作"]], Some(ids["工作"]), None).is_err());
        assert!(place(&c, &[ids["Gmail"]], Some(ids["Google"]), None).is_err(), "a link isn't a folder");
        assert!(place(&c, &[ids["Gmail"]], None, Some(ids["GitHub"])).is_err(), "before a row of another folder");
        // a folder moves with what's in it; deleting it deletes them
        place(&c, &[ids["邮箱"]], None, None).unwrap();
        assert_eq!(titles(&c, None), ["工作", "读书", "邮箱"]);
        assert_eq!(delete(&c, &[ids["邮箱"]]).unwrap(), 1);
        assert!(get(&c, ids["Gmail"]).is_err());
        assert_eq!(count(&c).unwrap(), 4);
        // the tree's order, with the folders' names
        let all = list(&c).unwrap();
        let order: Vec<String> = in_order(&all).into_iter().map(|(b, p)| format!("{}{}", p.join("/"), b.title)).collect();
        assert_eq!(order, ["工作", "读书", "读书Google", "读书GitHub"]);
    }

    #[test]
    fn records_round_trip() {
        let c = memory_db();
        let ids = seed(&c);
        update(&c, ids["GitHub"], serde_json::from_value(json!({ "onHome": true })).unwrap()).unwrap();
        let recs = export(&c).unwrap();
        assert_eq!(records_len(&recs), 6);
        let v = serde_json::to_value(&recs).unwrap();
        assert_eq!(v[0]["children"][0]["children"][0]["url"], "https://mail.google.com");
        assert_eq!(v[0]["children"][1]["onHome"], true);
        assert!(v[1]["children"][0].get("children").is_none() && v[1].get("url").is_none()); // a link has no children, a folder no url
        let d = memory_db();
        validate_records(&recs).unwrap();
        assert_eq!(replace_all(&d, &recs).unwrap(), 6);
        assert_eq!(export(&d).unwrap(), recs);
        let bad: Vec<BookmarkRecord> = serde_json::from_value(json!([{ "kind": "link", "title": "x", "url": "ftp://x" }])).unwrap();
        assert!(validate_records(&bad).is_err());
        let bad: Vec<BookmarkRecord> = serde_json::from_value(json!([{ "kind": "link", "title": "x", "url": "x.com", "children": [{ "kind": "folder", "title": "y" }] }])).unwrap();
        assert!(validate_records(&bad).is_err());
        // a file from before 0040: its top-level links go into 「未分类」, last
        let old: Vec<BookmarkRecord> = serde_json::from_value(json!([{ "kind": "link", "title": "A", "url": "a.com" }, { "kind": "folder", "title": "F" }, { "kind": "link", "title": "B", "url": "b.com" }])).unwrap();
        validate_records(&old).unwrap();
        let e = memory_db();
        replace_all(&e, &old).unwrap();
        let v = serde_json::to_value(export(&e).unwrap()).unwrap();
        assert_eq!((v[0]["title"].as_str(), v[1]["title"].as_str(), v[1]["children"][0]["title"].as_str(), v[1]["children"][1]["title"].as_str()), (Some("F"), Some(LOOSE), Some("A"), Some("B")));
    }

    #[test]
    fn migration_puts_top_level_links_in_a_folder() {
        let c = memory_db();
        let ids = seed(&c);
        // as before 0040: links straight at the top level
        c.execute("UPDATE bookmarks SET parent_id = NULL, sort_order = 10 WHERE id = ?1", [ids["Google"]]).unwrap();
        c.execute("UPDATE bookmarks SET parent_id = NULL, sort_order = 11 WHERE id = ?1", [ids["GitHub"]]).unwrap();
        let m = crate::db::MIGRATIONS.iter().find(|m| m.version == 40).unwrap();
        c.execute_batch(m.sql).unwrap();
        assert_eq!(titles(&c, None), ["工作", "读书", LOOSE]);
        let loose = list(&c).unwrap().into_iter().find(|b| b.title == LOOSE).unwrap().id;
        assert_eq!(titles(&c, Some(loose)), ["Google", "GitHub"]);
        // nothing loose: no folder
        c.execute_batch(m.sql).unwrap();
        assert_eq!(titles(&c, None), ["工作", "读书", LOOSE]);
    }
}
