//! 书单检索 / 影单检索 (插件库): a title → who made it, where from, what kind — only what Pomelo keeps.
//! - Wikidata first: its search knows Chinese titles and translated names (Open Library mostly
//!   doesn't: 「地下室手记」 finds nothing there, 「Notes from Underground」 does — tried), and an item
//!   says what it is (novel / film / TV series / anime), who made it (P50 author, P57 director, else
//!   P170 creator), where from (a book: its author's P27 first — 《百年孤独》 was first published in
//!   Argentina, but it's 哥伦比亚's; a film: P495), when (P577) and its ids elsewhere
//!   (P648 Open Library, P4947 / P4983 TMDb, P4086 MyAnimeList).
//! - Then the source confirms: the Open Library work's author, TMDb's director (with the user's Read
//!   Access Token), the Jikan anime entry. Where Wikidata finds nothing, the source's own search
//!   answers and the note says Wikidata didn't confirm it. A source that can't be reached (Jikan is
//!   often slow) only weakens the note.
//! - 类型: 漫画 / 非虚构 / 虚构 for books, 动漫 / 剧集 / 电影 for films, matched to the user's 类型 by
//!   name (`kind_id`); a name the user doesn't have stays unmatched.
//! - Requests to Wikidata are paced (it throttles bursts; a 429 waits and tries once more) and
//!   answers cached for the session. Jikan is given little time (it was often unreachable).
//! - Mock (dev / tests): a few made-up entries; 「检索失败测试」 fails like the services being down.

use crate::modules::common::err;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const TMDB_ACCOUNT: &str = "tmdb";
pub const MOCK_FAILURE: &str = "检索失败测试";
const UA: &str = "PomeloOS/0.4 (personal desktop app) reqwest";

/// What a lookup may use: the TMDb token, and the contact the user set in the 插件库. Wikimedia allows
/// 200 requests a minute to a User-Agent with contact details (10 without); it goes to Wikidata and
/// Open Library only.
#[derive(Debug, Clone, Default)]
pub struct Reach {
    pub tmdb: Option<String>,
    pub contact: Option<String>,
}

/// The User-Agent with the contact (Wikidata, Open Library) or without.
fn agent(contact: Option<&str>) -> String {
    match contact {
        Some(c) => format!("PomeloOS/0.4 (personal desktop app; {c}) reqwest"),
        None => UA.into(),
    }
}
const WIKIDATA: &str = "https://www.wikidata.org/w/api.php";
const LANGS: &str = "zh-cn|zh-hans|zh|en|ja";
/// Wikidata asks clients not to burst.
const WIKIDATA_GAP: Duration = Duration::from_millis(350);
const CACHE_FOR: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Which {
    Books,
    Films,
}

/// What a lookup found.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    pub title: String,
    /// The original / English title, when it differs.
    pub original: Option<String>,
    /// Author / director (the main one).
    pub creator: Option<String>,
    pub country: Option<String>,
    /// 虚构 / 非虚构 / 漫画 · 电影 / 剧集 / 动漫
    pub kind: Option<String>,
    pub year: Option<String>,
    /// The page to open (the source's, else Wikidata's).
    pub url: String,
    /// 「Wikidata · Open Library」
    pub site: String,
    /// 「Wikidata 和 Open Library 一致」
    pub note: String,
}

// ---------- types ----------

const COMICS: &[&str] = &["Q1004", "Q8274", "Q21198342", "Q14406742", "Q725377"];
const FICTION: &[&str] = &["Q8261", "Q1667921", "Q49084", "Q25379", "Q5185279", "Q1279564", "Q7725634"];
const NONFICTION: &[&str] = &["Q35760", "Q213051", "Q36279", "Q4184", "Q5292", "Q193955"];
const BOOKISH: &[&str] = &["Q47461344", "Q571", "Q277759", "Q732577"];
const FILM: &[&str] = &["Q11424", "Q24869", "Q506240", "Q93204", "Q226730", "Q24862"];
const SERIES: &[&str] = &["Q5398426", "Q1259759", "Q526877", "Q15416"];
const ANIMATED: &[&str] = &["Q202866", "Q20650540", "Q63952888", "Q581714", "Q1107", "Q117467246"];

fn has(ids: &[String], set: &[&str]) -> bool {
    ids.iter().any(|i| set.contains(&i.as_str()))
}

/// What the item is, as the hint for Pomelo's 类型; None = not this kind of work.
fn kind_of(which: Which, types: &[String], genres: &[String], mal: bool) -> Option<&'static str> {
    match which {
        Which::Books => {
            if has(types, COMICS) {
                Some("漫画")
            } else if has(types, NONFICTION) || has(genres, NONFICTION) {
                Some("非虚构")
            } else if has(types, FICTION) {
                Some("虚构")
            } else if has(types, BOOKISH) {
                Some("")
            } else {
                None
            }
        }
        Which::Films => {
            if mal || has(types, ANIMATED) {
                Some("动漫")
            } else if has(types, SERIES) {
                Some("剧集")
            } else if has(types, FILM) {
                Some("电影")
            } else {
                None
            }
        }
    }
}

/// The user's 类型 that has the hint's name.
pub fn kind_id(names: &[(i64, String)], kind: Option<&str>) -> Option<i64> {
    let k = kind.filter(|k| !k.is_empty())?;
    names.iter().find(|(_, n)| n == k).map(|(id, _)| *id)
}

fn country(label: &str) -> String {
    match label {
        "中华人民共和国" => "中国",
        "美利坚合众国" => "美国",
        "大不列颠及北爱尔兰联合王国" | "大不列颠及北爱尔兰联合王国 (英国)" => "英国",
        "大韩民国" => "韩国",
        "俄罗斯帝国" | "俄罗斯联邦" => "俄罗斯",
        "日本国" => "日本",
        other => other,
    }
    .to_string()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn cjk(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x9FFF | 0xAC00..=0xD7AF))
}

fn edits(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (prev + usize::from(a[i - 1] != b[j - 1])).min(row[j] + 1).min(row[j - 1] + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// Two names of one person: same letters, one inside the other, or (spelt differently: Dostoyevsky /
/// Dostoevsky) last names a couple of letters apart.
fn same_person(a: &str, b: &str) -> bool {
    let (x, y) = (norm(a), norm(b));
    if x.is_empty() || y.is_empty() {
        return false;
    }
    if x == y || x.contains(&y) || y.contains(&x) {
        return true;
    }
    let last = |s: &str| s.split([' ', '·', '.', '-', ',']).filter(|w| w.chars().count() > 2).next_back().map(norm);
    match (last(a), last(b)) {
        (Some(p), Some(q)) if !p.is_empty() && !q.is_empty() => y.contains(&p) || x.contains(&q) || (p.chars().count() >= 6 && edits(&p, &q) <= 2),
        _ => false,
    }
}

// ---------- http ----------

fn client(contact: Option<&str>) -> Result<reqwest::Client, String> {
    reqwest::Client::builder().user_agent(agent(contact)).timeout(Duration::from_secs(10)).build().map_err(err)
}

fn last_wikidata() -> &'static tokio::sync::Mutex<Option<Instant>> {
    static L: OnceLock<tokio::sync::Mutex<Option<Instant>>> = OnceLock::new();
    L.get_or_init(|| tokio::sync::Mutex::new(None))
}

async fn get_json(c: &reqwest::Client, url: &str, query: &[(&str, &str)], bearer: Option<&str>) -> Result<Value, String> {
    let url = reqwest::Url::parse_with_params(url, query).map_err(|_| "地址不对".to_string())?;
    let mut req = c.get(url);
    if let Some(t) = bearer {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.map_err(|e| if e.is_timeout() { "超时了".to_string() } else { "连不上".to_string() })?;
    match resp.status().as_u16() {
        200 => resp.json().await.map_err(|_| "返回的内容看不懂".to_string()),
        401 => Err("Token 不对".into()),
        404 => Err("没有这一条".into()),
        429 => Err("请求太频繁".into()),
        // Wikimedia's throttle answers 200 with a plain-text page sometimes; caught as unreadable
        s => Err(format!("返回了错误（{s}）")),
    }
}

async fn wikidata(c: &reqwest::Client, query: &[(&str, &str)]) -> Result<Value, String> {
    let mut last = last_wikidata().lock().await;
    if let Some(t) = *last {
        let wait = WIKIDATA_GAP.saturating_sub(t.elapsed());
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
    let mut q = vec![("format", "json")];
    q.extend_from_slice(query);
    let mut out = get_json(c, WIKIDATA, &q, None).await;
    if out.as_ref().is_err_and(|e| e == "请求太频繁") {
        tokio::time::sleep(Duration::from_secs(3)).await;
        out = get_json(c, WIKIDATA, &q, None).await;
    }
    *last = Some(Instant::now());
    out.map_err(|e| format!("Wikidata {e}"))
}

// ---------- Wikidata items ----------

fn label(e: &Value) -> Option<String> {
    ["zh-cn", "zh-hans", "zh", "en"].iter().find_map(|l| e["labels"][l]["value"].as_str().map(String::from))
}

fn claim_ids(e: &Value, p: &str) -> Vec<String> {
    e["claims"][p].as_array().into_iter().flatten().filter_map(|c| c["mainsnak"]["datavalue"]["value"]["id"].as_str().map(String::from)).collect()
}

fn claim_strings(e: &Value, p: &str) -> Vec<String> {
    e["claims"][p].as_array().into_iter().flatten().filter_map(|c| c["mainsnak"]["datavalue"]["value"].as_str().map(String::from)).collect()
}

fn claim_year(e: &Value, p: &str) -> Option<String> {
    e["claims"][p].as_array().into_iter().flatten().filter_map(|c| c["mainsnak"]["datavalue"]["value"]["time"].as_str()).filter_map(|t| t.get(1..5).map(String::from)).min()
}

async fn entities(c: &reqwest::Client, ids: &[String], props: &str) -> Result<serde_json::Map<String, Value>, String> {
    if ids.is_empty() {
        return Ok(Default::default());
    }
    let joined = ids.iter().take(50).cloned().collect::<Vec<_>>().join("|");
    let v = wikidata(c, &[("action", "wbgetentities"), ("ids", &joined), ("props", props), ("languages", LANGS)]).await?;
    Ok(v["entities"].as_object().cloned().unwrap_or_default())
}

/// The best item for the title (and the creator hint), with the labels its claims point to.
struct Item {
    id: String,
    e: Value,
    kind: &'static str,
    refs: serde_json::Map<String, Value>,
}

async fn find_item(c: &reqwest::Client, which: Which, title: &str, hint: Option<&str>) -> Result<Option<Item>, String> {
    let lang = if cjk(title) { "zh" } else { "en" };
    let s = wikidata(c, &[("action", "wbsearchentities"), ("search", title), ("language", lang), ("uselang", lang), ("type", "item"), ("limit", "8")]).await?;
    let ids: Vec<String> = s["search"].as_array().into_iter().flatten().filter_map(|x| x["id"].as_str().map(String::from)).collect();
    let found = entities(c, &ids, "labels|claims").await?;
    let mut items: Vec<(usize, String, Value, &'static str)> = vec![];
    for (rank, id) in ids.iter().enumerate() {
        let Some(e) = found.get(id) else { continue };
        let kind = kind_of(which, &claim_ids(e, "P31"), &claim_ids(e, "P136"), !claim_strings(e, "P4086").is_empty());
        if let Some(k) = kind {
            items.push((rank, id.clone(), e.clone(), k));
        }
    }
    if items.is_empty() {
        return Ok(None);
    }
    let people = |e: &Value| [claim_ids(e, "P50"), claim_ids(e, "P57"), claim_ids(e, "P170")].concat();
    let mut refs_ids: Vec<String> = items.iter().take(4).flat_map(|(_, _, e, _)| [people(e), claim_ids(e, "P495")].concat()).collect();
    refs_ids.sort();
    refs_ids.dedup();
    let refs = entities(c, &refs_ids, "labels|claims").await?;
    // one whose maker fits the hint, then a whole-title match of the same kind as Wikidata's first
    // (「人类简史」: not the graphic-novel adaptation that happens to have that exact label), then its order
    let q = norm(title);
    let labels_of = |e: &Value| -> Vec<String> { e["labels"].as_object().map(|m| m.values().filter_map(|l| l["value"].as_str().map(norm)).collect()).unwrap_or_default() };
    let first_kind = items[0].3;
    items.sort_by_key(|(rank, _, e, kind)| {
        let exact = *kind == first_kind && labels_of(e).contains(&q);
        let maker = hint.is_some_and(|h| people(e).iter().filter_map(|p| refs.get(p)).any(|p| p["labels"].as_object().is_some_and(|m| m.values().any(|l| same_person(l["value"].as_str().unwrap_or(""), h)))));
        (!maker, !exact, *rank)
    });
    let (_, id, e, kind) = items.remove(0);
    Ok(Some(Item { id, e, kind, refs }))
}

impl Item {
    fn names(&self, p: &str) -> Vec<(String, Option<String>)> {
        claim_ids(&self.e, p).iter().filter_map(|id| self.refs.get(id)).map(|r| (label(r).unwrap_or_default(), r["labels"]["en"]["value"].as_str().map(String::from))).filter(|(l, _)| !l.is_empty()).collect()
    }
    fn maker(&self) -> Option<(String, Option<String>)> {
        ["P50", "P57", "P170"].iter().find_map(|p| self.names(p).into_iter().next())
    }
    /// Every label of the maker (Open Library may write 村上春樹).
    fn maker_labels(&self) -> Vec<String> {
        let id = ["P50", "P57", "P170"].iter().find_map(|p| claim_ids(&self.e, p).into_iter().next());
        id.and_then(|i| self.refs.get(&i)).and_then(|r| r["labels"].as_object()).map(|m| m.values().filter_map(|l| l["value"].as_str().map(String::from)).collect()).unwrap_or_default()
    }
    /// Where it is from: a book's author's citizenship (one more fetch for its name), else P495.
    async fn country(&self, c: &reqwest::Client, which: Which) -> Option<String> {
        if which == Which::Books {
            let place = claim_ids(&self.e, "P50").into_iter().next().and_then(|a| self.refs.get(&a).and_then(|r| claim_ids(r, "P27").into_iter().next_back()));
            if let Some(place) = place {
                if let Some(n) = entities(c, std::slice::from_ref(&place), "labels").await.ok().and_then(|got| got.get(&place).and_then(label)) {
                    return Some(country(&n));
                }
            }
        }
        self.names("P495").into_iter().next().map(|(n, _)| country(&n))
    }
}

// ---------- the sources ----------

/// (url, author names, title). A work lists its authors with translators and editors: up to three
/// names are fetched (「地下室手记」's work is a French edition whose first name isn't Dostoevsky).
async fn open_library(c: &reqwest::Client, work: Option<&str>, title: &str, author: Option<&str>) -> Result<(String, Vec<String>, Option<String>), String> {
    let ol = |e: String| format!("Open Library {e}");
    if let Some(w) = work.filter(|w| w.ends_with('W')) {
        let v = get_json(c, &format!("https://openlibrary.org/works/{w}.json"), &[], None).await.map_err(ol)?;
        let mut names = vec![];
        for k in v["authors"].as_array().into_iter().flatten().filter_map(|a| a["author"]["key"].as_str()).take(3) {
            if let Some(n) = get_json(c, &format!("https://openlibrary.org{k}.json"), &[], None).await.ok().and_then(|a| a["name"].as_str().map(String::from)) {
                names.push(n);
            }
        }
        return Ok((format!("https://openlibrary.org/works/{w}"), names, v["title"].as_str().map(String::from)));
    }
    let mut q = vec![("title", title), ("fields", "key,title,author_name,first_publish_year"), ("limit", "3")];
    if let Some(a) = author {
        q.push(("author", a));
    }
    let v = get_json(c, "https://openlibrary.org/search.json", &q, None).await.map_err(ol)?;
    let d = v["docs"].as_array().and_then(|d| d.first()).ok_or_else(|| ol("没找到".into()))?;
    let names = d["author_name"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(String::from)).take(3).collect();
    Ok((format!("https://openlibrary.org{}", d["key"].as_str().unwrap_or("")), names, d["title"].as_str().map(String::from)))
}

/// TMDb details: (url, director, title, countries, year, media).
async fn tmdb(c: &reqwest::Client, token: &str, media: &str, id: &str) -> Result<(String, Option<String>, Option<String>, Vec<String>, Option<String>), String> {
    let v = get_json(c, &format!("https://api.themoviedb.org/3/{media}/{id}"), &[("language", "zh-CN"), ("append_to_response", "credits")], Some(token)).await.map_err(|e| format!("TMDb {e}"))?;
    let director = v["credits"]["crew"].as_array().into_iter().flatten().find(|p| p["job"] == "Director").and_then(|p| p["name"].as_str()).or_else(|| v["created_by"][0]["name"].as_str()).map(String::from);
    let countries: Vec<String> = v["production_countries"].as_array().into_iter().flatten().filter_map(|x| x["iso_3166_1"].as_str().map(String::from)).chain(v["origin_country"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from))).collect();
    let title = v["title"].as_str().or_else(|| v["name"].as_str()).map(String::from);
    let year = v["release_date"].as_str().or_else(|| v["first_air_date"].as_str()).and_then(|d| d.get(..4)).map(String::from);
    Ok((format!("https://www.themoviedb.org/{media}/{id}"), director, title, countries, year))
}

async fn tmdb_search(c: &reqwest::Client, token: &str, title: &str) -> Result<Option<(String, String)>, String> {
    let v = get_json(c, "https://api.themoviedb.org/3/search/multi", &[("query", title), ("language", "zh-CN")], Some(token)).await.map_err(|e| format!("TMDb {e}"))?;
    Ok(v["results"].as_array().into_iter().flatten().find(|r| matches!(r["media_type"].as_str(), Some("movie" | "tv"))).map(|r| (r["media_type"].as_str().unwrap_or("movie").to_string(), r["id"].to_string())))
}

/// Jikan (MyAnimeList): (url, title, year).
async fn jikan(mal: Option<&str>, title: &str) -> Result<(String, Option<String>, Option<String>), String> {
    let j = |e: String| format!("Jikan {e}");
    let c = reqwest::Client::builder().user_agent(UA).timeout(Duration::from_secs(5)).build().map_err(err)?;
    let a = match mal {
        Some(id) => get_json(&c, &format!("https://api.jikan.moe/v4/anime/{id}"), &[], None).await.map_err(j)?["data"].clone(),
        None => get_json(&c, "https://api.jikan.moe/v4/anime", &[("q", title), ("limit", "1")], None).await.map_err(j)?["data"][0].clone(),
    };
    let url = a["url"].as_str().ok_or_else(|| j("没找到".into()))?.to_string();
    Ok((url, a["title"].as_str().map(String::from), a["year"].as_i64().map(|y| y.to_string())))
}

// ---------- the lookup ----------

fn cache() -> &'static Mutex<HashMap<(Which, String), (Instant, Option<Work>)>> {
    static C: OnceLock<Mutex<HashMap<(Which, String), (Instant, Option<Work>)>>> = OnceLock::new();
    C.get_or_init(Mutex::default)
}

/// The work `title` names (`hint`: its author / director when said); None = not found anywhere.
pub async fn find(which: Which, title: &str, hint: Option<&str>, reach: Reach, mock: bool) -> Result<Option<Work>, String> {
    let title = title.trim().trim_matches(|c| c == '《' || c == '》').trim();
    if title.is_empty() {
        return Err("没说名称".into());
    }
    if mock {
        return mock_find(which, title);
    }
    let key = (which, format!("{}|{}", norm(title), hint.map(norm).unwrap_or_default()));
    if let Some((at, w)) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        if at.elapsed() < CACHE_FOR {
            return Ok(w.clone());
        }
    }
    let found = lookup(which, title, hint, reach).await?;
    cache().lock().unwrap_or_else(|e| e.into_inner()).insert(key, (Instant::now(), found.clone()));
    Ok(found)
}

async fn lookup(which: Which, title: &str, hint: Option<&str>, reach: Reach) -> Result<Option<Work>, String> {
    // the contact only to Wikidata and Open Library
    let c = client(reach.contact.as_deref())?;
    let plain = client(None)?;
    let tmdb_token = reach.tmdb;
    let item = match find_item(&c, which, title, hint).await {
        Ok(i) => i,
        Err(e) => return fallback(&c, &plain, which, title, hint, tmdb_token.as_deref(), Some(e)).await,
    };
    let Some(item) = item else { return fallback(&c, &plain, which, title, hint, tmdb_token.as_deref(), None).await };
    let wd_url = format!("https://www.wikidata.org/wiki/{}", item.id);
    let maker = item.maker();
    let mut w = Work {
        title: label(&item.e).unwrap_or_else(|| title.to_string()),
        original: item.e["labels"]["en"]["value"].as_str().map(String::from),
        creator: maker.as_ref().map(|m| m.0.clone()),
        country: item.country(&c, which).await,
        kind: Some(item.kind.to_string()).filter(|k| !k.is_empty()),
        year: claim_year(&item.e, "P577"),
        url: wd_url,
        site: "Wikidata".into(),
        note: "Wikidata 找到".into(),
    };
    if w.original.as_deref().is_some_and(|o| norm(o) == norm(&w.title)) {
        w.original = None;
    }
    let maker_names: Vec<String> = item.maker_labels();
    let agrees = |name: &Option<String>| name.as_deref().is_some_and(|n| maker_names.iter().any(|m| same_person(m, n)));
    // the source confirms: (its page, its name, agrees, its maker)
    let confirmed: Result<(String, &str, bool, Option<String>), String> = match which {
        Which::Books => {
            let ol = claim_strings(&item.e, "P648").into_iter().find(|x| x.ends_with('W'));
            let en_author = maker.as_ref().and_then(|m| m.1.clone());
            let search_title = w.original.clone().unwrap_or_else(|| w.title.clone());
            let fits = |authors: &[String]| maker_names.is_empty() || authors.iter().any(|a| agrees(&Some(a.clone())));
            let mut got = open_library(&c, ol.as_deref(), &search_title, en_author.as_deref()).await;
            // the linked work may be an edition listing its translator: the search by title and author
            if ol.is_some() && got.as_ref().is_ok_and(|(_, a, _)| !fits(a)) {
                if let Some(other) = open_library(&c, None, &search_title, en_author.as_deref()).await.ok().filter(|(_, a, _)| fits(a)) {
                    got = Ok(other);
                }
            }
            got.map(|(url, authors, _)| (url, "Open Library", fits(&authors), authors.into_iter().next()))
        }
        Which::Films => {
            let mal = claim_strings(&item.e, "P4086").into_iter().next();
            let tm = claim_strings(&item.e, "P4947").into_iter().next().map(|id| ("movie", id)).or_else(|| claim_strings(&item.e, "P4983").into_iter().next().map(|id| ("tv", id)));
            match (tmdb_token.as_deref(), tm, mal) {
                (Some(t), Some((media, id)), _) => tmdb(&plain, t, media, &id).await.map(|(url, director, ..)| (url, "TMDb", maker_names.is_empty() || director.is_none() || agrees(&director), director)),
                (_, _, Some(id)) => jikan(Some(&id), &w.title).await.map(|(url, ..)| (url, "Jikan", true, None)),
                (None, Some(_), None) => Err("没填 TMDb Token，没核对".into()),
                _ => Err("别处没有这一部的编号，没核对".into()),
            }
        }
    };
    match confirmed {
        Ok((url, src, true, theirs)) => {
            w.url = url;
            w.site = format!("Wikidata · {src}");
            w.note = format!("Wikidata 和 {src} 一致");
            // Wikidata names no maker (TV series often): the source's
            if w.creator.is_none() {
                w.creator = theirs;
            }
        }
        Ok((_, src, false, _)) => {
            w.site = format!("Wikidata · {src}");
            w.note = format!("{src} 上的{}和 Wikidata 不一样，以 Wikidata 为准", if which == Which::Books { "作者" } else { "导演" });
        }
        Err(e) => w.note = format!("Wikidata 找到（{e}）"),
    }
    Ok(Some(w))
}

/// Wikidata found nothing (or couldn't be reached): the source's own search, marked as such.
/// `c`: for Open Library (with the contact); `plain`: for TMDb.
async fn fallback(c: &reqwest::Client, plain: &reqwest::Client, which: Which, title: &str, hint: Option<&str>, token: Option<&str>, wd_error: Option<String>) -> Result<Option<Work>, String> {
    let why = wd_error.map_or("Wikidata 没有".to_string(), |e| format!("Wikidata 没查成：{e}"));
    let bare = |url: String, site: &str, title: String, creator: Option<String>, year: Option<String>, kind: Option<&str>| Work {
        title,
        original: None,
        creator,
        country: None,
        kind: kind.map(String::from),
        year,
        url,
        site: site.into(),
        note: format!("只在 {site} 找到（{why}），没有交叉核对"),
    };
    match which {
        Which::Books => match open_library(c, None, title, hint).await {
            Ok((url, authors, t)) => Ok(Some(bare(url, "Open Library", t.unwrap_or_else(|| title.into()), authors.into_iter().next(), None, None))),
            Err(_) => Ok(None),
        },
        Which::Films => {
            if let Some(t) = token {
                if let Ok(Some((media, id))) = tmdb_search(plain, t, title).await {
                    if let Ok((url, director, name, _, year)) = tmdb(plain, t, &media, &id).await {
                        return Ok(Some(bare(url, "TMDb", name.unwrap_or_else(|| title.into()), director, year, Some(if media == "tv" { "剧集" } else { "电影" }))));
                    }
                }
            }
            match jikan(None, title).await {
                Ok((url, name, year)) => Ok(Some(bare(url, "Jikan", name.unwrap_or_else(|| title.into()), None, year, Some("动漫")))),
                Err(_) => Ok(None),
            }
        }
    }
}

// ---------- mock ----------

fn mock_find(which: Which, title: &str) -> Result<Option<Work>, String> {
    if title.contains(MOCK_FAILURE) {
        return Err("模拟：Wikidata 连不上。".into());
    }
    let w = |t: &str, o: &str, c: &str, n: &str, k: &str, y: &str, site: &str, url: &str| Work {
        title: t.into(),
        original: Some(o.into()).filter(|o: &String| !o.is_empty()),
        creator: Some(c.into()),
        country: Some(n.into()),
        kind: Some(k.into()),
        year: Some(y.into()),
        url: url.into(),
        site: site.into(),
        note: format!("{} 一致（模拟）", site.replace(" · ", " 和 ")),
    };
    let found = match (which, norm(title).as_str()) {
        (Which::Books, "三体") => Some(w("三体", "The Three-Body Problem", "刘慈欣", "中国", "虚构", "2006", "Wikidata · Open Library", "https://openlibrary.org/works/OL17267881W")),
        (Which::Books, "地下室手记") => Some(w("地下室手记", "Notes from Underground", "费奥多尔·陀思妥耶夫斯基", "俄罗斯", "虚构", "1864", "Wikidata · Open Library", "https://openlibrary.org/works/OL24767320W")),
        (Which::Books, "人类简史") => Some(w("人类简史", "Sapiens", "尤瓦尔·赫拉利", "以色列", "非虚构", "2011", "Wikidata · Open Library", "https://openlibrary.org/works/OL17075811W")),
        (Which::Films, "霸王别姬") => Some(w("霸王别姬", "Farewell My Concubine", "陈凯歌", "中国", "电影", "1993", "Wikidata · TMDb", "https://www.themoviedb.org/movie/10997")),
        (Which::Films, "请回答1988") => Some(w("请回答1988", "Reply 1988", "申源浩", "韩国", "剧集", "2015", "Wikidata · TMDb", "https://www.themoviedb.org/tv/64010")),
        (Which::Films, "进击的巨人") => Some(w("进击的巨人", "Attack on Titan", "荒木哲郎", "日本", "动漫", "2013", "Wikidata · Jikan", "https://myanimelist.net/anime/16498")),
        _ => None,
    };
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_people_and_the_mock() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(kind_of(Which::Books, &ids(&["Q8274"]), &[], false), Some("漫画"));
        assert_eq!(kind_of(Which::Books, &ids(&["Q7725634"]), &ids(&["Q213051"]), false), Some("非虚构"));
        assert_eq!(kind_of(Which::Books, &ids(&["Q7725634"]), &[], false), Some("虚构"));
        assert_eq!(kind_of(Which::Books, &ids(&["Q11424"]), &[], false), None); // a film isn't a book
        assert_eq!(kind_of(Which::Films, &ids(&["Q5398426"]), &[], true), Some("动漫"));
        assert_eq!(kind_of(Which::Films, &ids(&["Q5398426"]), &[], false), Some("剧集"));
        assert_eq!(kind_of(Which::Films, &ids(&["Q11424"]), &[], false), Some("电影"));
        assert!(same_person("Fyodor Dostoevsky", "Fyodor Mikhailovich Dostoevsky"));
        assert!(same_person("Liu Cixin", "Cixin Liu") || same_person("Liu Cixin", "Liu Cixin"));
        assert!(!same_person("Stephen King", "Haruki Murakami"));
        assert!(same_person("Fyodor Dostoyevsky", "Fyodor Mikhailovich Dostoevsky"));
        assert!(!same_person("Ivan Turgenev", "Fyodor Dostoevsky"));
        assert_eq!(country("中华人民共和国"), "中国");
        let names = vec![(1, "虚构".to_string()), (2, "漫画".to_string())];
        assert_eq!((kind_id(&names, Some("漫画")), kind_id(&names, Some("非虚构")), kind_id(&names, Some(""))), (Some(2), None, None));
        let w = tauri::async_runtime::block_on(find(Which::Books, "《三体》", None, Reach::default(), true)).unwrap().unwrap();
        assert_eq!((w.creator.as_deref(), w.country.as_deref(), w.kind.as_deref()), (Some("刘慈欣"), Some("中国"), Some("虚构")));
        assert!(tauri::async_runtime::block_on(find(Which::Films, "没有这部", None, Reach::default(), true)).unwrap().is_none());
        assert!(tauri::async_runtime::block_on(find(Which::Films, MOCK_FAILURE, None, Reach::default(), true)).is_err());
        assert_eq!(agent(Some("me@example.com")), "PomeloOS/0.4 (personal desktop app; me@example.com) reqwest");
    }

    /// Against the real Wikidata / Open Library / Jikan (and TMDb with POMELO_TMDB_TOKEN):
    /// POMELO_REAL_WEB=1 [POMELO_CONTACT=邮箱] [POMELO_ASK='书:三体|影:霸王别姬'] cargo test real_works -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_works() {
        if std::env::var("POMELO_REAL_WEB").as_deref() != Ok("1") {
            return;
        }
        let reach = Reach { tmdb: std::env::var("POMELO_TMDB_TOKEN").ok(), contact: std::env::var("POMELO_CONTACT").ok() };
        let asks = std::env::var("POMELO_ASK").unwrap_or_else(|_| "书:三体|书:地下室手记|书:百年孤独|书:人类简史|书:Norwegian Wood|影:霸王别姬|影:肖申克的救赎|影:请回答1988|影:进击的巨人|影:千与千寻".into());
        for a in asks.split('|') {
            let (which, title) = match a.split_once(':') {
                Some(("书", t)) => (Which::Books, t),
                Some((_, t)) => (Which::Films, t),
                None => (Which::Books, a),
            };
            let t = Instant::now();
            let r = tauri::async_runtime::block_on(find(which, title, None, reach.clone(), false));
            println!("{a} ({:.1}s) → {r:?}", t.elapsed().as_secs_f32());
        }
    }
}
