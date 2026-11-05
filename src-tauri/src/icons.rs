//! Site icons for 一网打尽 (requirements「一网打尽」): fetched once per host from the site itself (its home
//! page's <link rel="icon">, else /favicon.ico, .svg, .png; a subdomain with none: its main site's; no
//! third-party service), kept in <data>/icons/ (one file per
//! host, `failed.json` for hosts that gave none: tried again after 7 days). A cache, like search.db: not
//! backed up, exported or undone; deleting the folder fetches again.
//! - Only the real app goes online (`online`): dev builds, tests and scratch data dirs never do, and the
//!   page shows the host's first letter instead.
//! - The page asks for what's cached (`bookmarks_icons`); that also starts fetching the missing ones in the
//!   background, a few at a time, and says `bookmark-icons` once some arrive.

use crate::modules::bookmarks;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

pub const EVENT: &str = "bookmark-icons";
const FAILED: &str = "failed.json";
const RETRY_DAYS: i64 = 7;
const MAX_PAGE: usize = 512 * 1024;
const MAX_ICON: usize = 300 * 1024;
const AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

pub struct Icons {
    dir: PathBuf,
    online: bool,
    /// Hosts being fetched now.
    busy: Mutex<HashSet<String>>,
}

impl Icons {
    pub fn new(dir: PathBuf, online: bool) -> Self {
        Self { dir, online, busy: Mutex::new(HashSet::new()) }
    }
}

/// A file name for a host (letters, digits, `.` and `-` kept).
fn file_stem(host: &str) -> String {
    host.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

const KINDS: [(&str, &str); 6] = [("png", "image/png"), ("ico", "image/x-icon"), ("svg", "image/svg+xml"), ("jpg", "image/jpeg"), ("gif", "image/gif"), ("webp", "image/webp")];

/// What the bytes are (by their first bytes): file extension.
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG") {
        Some("png")
    } else if b.starts_with(&[0, 0, 1, 0]) || b.starts_with(&[0, 0, 2, 0]) {
        Some("ico")
    } else if b.starts_with(&[0xFF, 0xD8]) {
        Some("jpg")
    } else if b.starts_with(b"GIF8") {
        Some("gif")
    } else if b.len() > 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        Some("webp")
    } else {
        let head = String::from_utf8_lossy(&b[..b.len().min(1024)]).to_ascii_lowercase();
        (head.contains("<svg")).then_some("svg")
    }
}

/// Every cached icon: host → data URL.
pub fn cached(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        let (Some(stem), Some(ext)) = (p.file_stem().and_then(|s| s.to_str()), p.extension().and_then(|s| s.to_str())) else { continue };
        let Some((_, mime)) = KINDS.iter().find(|(x, _)| *x == ext) else { continue };
        if let Ok(bytes) = std::fs::read(&p) {
            out.insert(stem.to_string(), format!("data:{mime};base64,{}", crate::youzi::office::base64(&bytes)));
        }
    }
    out
}

fn failed(dir: &Path) -> HashMap<String, i64> {
    std::fs::read_to_string(dir.join(FAILED)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The hosts still without an icon (and not given up on lately), with an address of each to start from.
pub fn missing(dir: &Path, links: &[(String, String)]) -> Vec<(String, String)> {
    let have = cached(dir);
    let gave_up = failed(dir);
    let mut seen = HashSet::new();
    links
        .iter()
        .filter(|(host, _)| !host.is_empty() && seen.insert(host.clone()))
        .filter(|(host, _)| !have.contains_key(&file_stem(host)) && gave_up.get(&file_stem(host)).is_none_or(|t| now() - t > RETRY_DAYS * 86400))
        .cloned()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub href: String,
    pub score: i64,
}

/// The value of attribute `name` in one tag's text (`<link rel="icon" href=…>`).
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        // a whole attribute name: after a space, followed by =
        if at == 0 || !lower.as_bytes()[at - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = tag[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        return Some(value.replace("&amp;", "&"));
    }
    None
}

/// The icons a page names, best first: an SVG, then a PNG near 64 px, an apple-touch-icon, any other.
pub fn candidates(html: &str) -> Vec<Candidate> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<link") {
        let start = from + i;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        from = end;
        let tag = &html[start..end];
        let rel = attr(tag, "rel").unwrap_or_default().to_ascii_lowercase();
        let Some(href) = attr(tag, "href").filter(|h| !h.trim().is_empty()) else { continue };
        let rels: Vec<&str> = rel.split_whitespace().collect();
        if !rels.iter().any(|r| *r == "icon" || r.starts_with("apple-touch-icon")) || rels.contains(&"mask-icon") {
            continue;
        }
        let ty = attr(tag, "type").unwrap_or_default().to_ascii_lowercase();
        let sizes = attr(tag, "sizes").unwrap_or_default().to_ascii_lowercase();
        let px: i64 = sizes.split_whitespace().filter_map(|s| s.split('x').next()?.parse().ok()).max().unwrap_or(0);
        let svg = ty.contains("svg") || href.to_ascii_lowercase().split(['?', '#']).next().unwrap_or("").ends_with(".svg");
        let score = if svg {
            100
        } else if rels.iter().any(|r| r.starts_with("apple-touch-icon")) {
            60
        } else if px > 0 {
            90 - (px - 64).abs().min(80) / 4
        } else {
            50
        };
        out.push(Candidate { href: href.trim().to_string(), score });
    }
    out.sort_by(|a, b| b.score.cmp(&a.score));
    out
}

/// `href` against the page it came from (`base`, a full http(s) address).
pub fn resolve(base: &str, href: &str) -> String {
    let (scheme, rest) = base.split_once("://").unwrap_or(("https", base));
    let origin_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let origin = format!("{scheme}://{}", &rest[..origin_end]);
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("data:") {
        href.to_string()
    } else if let Some(h) = href.strip_prefix("//") {
        format!("{scheme}://{h}")
    } else if href.starts_with('/') {
        format!("{origin}{href}")
    } else {
        let path = &rest[origin_end..];
        let path = path.split(['?', '#']).next().unwrap_or("");
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
        format!("{origin}{dir}/{href}")
    }
}

/// Standard base64 (padding and whitespace allowed).
fn unbase64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let clean: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// A data: URL's bytes (base64 or plain).
fn data_url(href: &str) -> Option<Vec<u8>> {
    let (head, body) = href.strip_prefix("data:")?.split_once(',')?;
    if head.ends_with(";base64") {
        unbase64(body)
    } else {
        Some(crate::modules::attachments::percent_decode(body).into_bytes())
    }
}

async fn get(client: &reqwest::Client, url: &str, max: usize) -> Option<(String, Vec<u8>)> {
    let mut resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let final_url = resp.url().to_string();
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = resp.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() > max {
            return None;
        }
    }
    Some((final_url, body))
}

/// The main site of a subdomain (`bailian.console.aliyun.com` → `aliyun.com`, `wx.mail.qq.com` → `qq.com`,
/// `a.fishc.com.cn` → `fishc.com.cn`); None for a main site or an address by number.
pub fn main_site(host: &str) -> Option<String> {
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return None;
    }
    let labels: Vec<&str> = host.split('.').collect();
    let short_tld = labels.last().is_some_and(|t| t.len() == 2);
    let keep = if short_tld && labels.len() >= 3 && ["com", "net", "org", "edu", "gov", "co", "ac"].contains(&labels[labels.len() - 2]) { 3 } else { 2 };
    (labels.len() > keep).then(|| labels[labels.len() - keep..].join("."))
}

/// One host's icon: its bytes and kind, if the site (or its main site) has one.
async fn fetch(client: &reqwest::Client, start: &str) -> Option<(Vec<u8>, &'static str)> {
    let (scheme, rest) = start.split_once("://")?;
    let host_part = rest.split(['/', '?', '#']).next()?;
    if let Some(found) = fetch_from(client, &format!("{scheme}://{host_part}/"), start).await {
        return Some(found);
    }
    let main = main_site(host_part.split(':').next().unwrap_or(host_part))?;
    let home = format!("https://{main}/");
    Box::pin(fetch_from(client, &home, &home)).await
}

async fn fetch_from(client: &reqwest::Client, home: &str, start: &str) -> Option<(Vec<u8>, &'static str)> {
    let home = home.to_string();
    let mut tried: Vec<String> = Vec::new();
    let mut list: Vec<String> = Vec::new();
    for page in [home.clone(), start.to_string()] {
        if tried.contains(&page) {
            continue;
        }
        tried.push(page.clone());
        if let Some((base, html)) = get(client, &page, MAX_PAGE).await {
            let html = String::from_utf8_lossy(&html);
            list.extend(candidates(&html).into_iter().map(|c| resolve(&base, &c.href)));
            list.push(resolve(&base, "/favicon.ico"));
            if !list.is_empty() {
                break;
            }
        }
    }
    for f in ["favicon.ico", "favicon.svg", "favicon.png", "apple-touch-icon.png"] {
        list.push(format!("{home}{f}"));
    }
    let mut seen = HashSet::new();
    for url in list.into_iter().filter(|u| seen.insert(u.clone())) {
        let bytes = if url.starts_with("data:") { data_url(&url) } else { get(client, &url, MAX_ICON).await.map(|(_, b)| b) };
        if let Some(b) = bytes.filter(|b| !b.is_empty() && b.len() <= MAX_ICON) {
            if let Some(kind) = sniff(&b) {
                return Some((b, kind));
            }
        }
    }
    None
}

/// Fetches the missing hosts' icons (four at a time), then says so to the page.
fn start(app: &AppHandle) {
    let icons = app.state::<Icons>();
    if !icons.online {
        return;
    }
    let links: Vec<(String, String)> = app
        .state::<AppState>()
        .with_db(|c| Ok(bookmarks::list(c)?.into_iter().filter(|b| !b.is_folder()).map(|b| (bookmarks::host_of(&b.url), b.url)).collect()))
        .unwrap_or_default();
    let todo: Vec<(String, String)> = {
        let mut busy = icons.busy.lock().unwrap_or_else(|e| e.into_inner());
        missing(&icons.dir, &links).into_iter().filter(|(h, _)| busy.insert(h.clone())).collect()
    };
    if todo.is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok(client) = reqwest::Client::builder().user_agent(AGENT).timeout(Duration::from_secs(10)).connect_timeout(Duration::from_secs(6)).build() else { return };
        let dir = app.state::<Icons>().dir.clone();
        let _ = std::fs::create_dir_all(&dir);
        let mut got = 0;
        for chunk in todo.chunks(4) {
            let jobs: Vec<_> = chunk
                .iter()
                .map(|(host, url)| {
                    let client = client.clone();
                    let (host, url) = (host.clone(), url.clone());
                    tauri::async_runtime::spawn(async move { (host, fetch(&client, &url).await) })
                })
                .collect();
            let mut done = Vec::new();
            for j in jobs {
                if let Ok(r) = j.await {
                    done.push(r);
                }
            }
            let mut gave_up = failed(&dir);
            for (host, icon) in done {
                let stem = file_stem(&host);
                match icon {
                    Some((bytes, ext)) if std::fs::write(dir.join(format!("{stem}.{ext}")), &bytes).is_ok() => {
                        gave_up.remove(&stem);
                        got += 1;
                    }
                    _ => {
                        gave_up.insert(stem, now());
                    }
                }
                app.state::<Icons>().busy.lock().unwrap_or_else(|e| e.into_inner()).remove(&host);
            }
            let _ = std::fs::write(dir.join(FAILED), serde_json::to_string(&gave_up).unwrap_or_default());
        }
        if got > 0 {
            let _ = app.emit(EVENT, got);
        }
    });
}

/// The icons there are (host → data URL); the missing ones start arriving in the background.
#[tauri::command]
pub fn bookmarks_icons(app: AppHandle, icons: State<'_, Icons>) -> BTreeMap<String, String> {
    let have = cached(&icons.dir);
    start(&app);
    have
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_best_icon_on_a_page() {
        let html = r##"<html><head>
            <link rel="stylesheet" href="/a.css">
            <link rel="shortcut icon" href="/favicon.ico">
            <LINK REL="icon" type="image/png" sizes="16x16" href="/f16.png">
            <link rel="icon" sizes="64x64" href='/f64.png?v=2&amp;x=1'>
            <link rel="apple-touch-icon" sizes="180x180" href="https://cdn.x.com/touch.png">
            <link rel="mask-icon" href="/mask.svg" color="#000">
            <link rel=icon href=/logo.svg>
        </head></html>"##;
        let c: Vec<String> = candidates(html).into_iter().map(|c| c.href).collect();
        assert_eq!(c, ["/logo.svg", "/f64.png?v=2&x=1", "/f16.png", "https://cdn.x.com/touch.png", "/favicon.ico"]);
    }

    #[test]
    fn resolves_relative_addresses() {
        let base = "https://platform.deepseek.com/zh/usage?x=1";
        assert_eq!(resolve(base, "/favicon.ico"), "https://platform.deepseek.com/favicon.ico");
        assert_eq!(resolve(base, "//cdn.x.com/i.png"), "https://cdn.x.com/i.png");
        assert_eq!(resolve(base, "img/i.png"), "https://platform.deepseek.com/zh/img/i.png");
        assert_eq!(resolve("https://x.com", "i.png"), "https://x.com/i.png");
        assert_eq!(resolve(base, "https://y.com/i.png"), "https://y.com/i.png");
    }

    #[test]
    fn knows_images_and_skips_what_it_has() {
        assert_eq!(sniff(b"\x89PNG\r\n"), Some("png"));
        assert_eq!(sniff(&[0, 0, 1, 0, 1]), Some("ico"));
        assert_eq!(sniff(b"<?xml version='1.0'?><svg xmlns='x'/>"), Some("svg"));
        assert_eq!(sniff(b"<!doctype html><html>"), None);
        assert_eq!(main_site("bailian.console.aliyun.com").as_deref(), Some("aliyun.com"));
        assert_eq!(main_site("wx.mail.qq.com").as_deref(), Some("qq.com"));
        assert_eq!(main_site("a.fishc.com.cn").as_deref(), Some("fishc.com.cn"));
        assert_eq!((main_site("github.com"), main_site("fishc.com.cn"), main_site("127.0.0.1")), (None, None, None));
        assert_eq!(data_url("data:image/svg+xml,%3Csvg%3E").map(|b| String::from_utf8(b).unwrap()).as_deref(), Some("<svg>"));
        assert_eq!(unbase64(&crate::youzi::office::base64(b"\x89PNG\r\n\x1a")).unwrap(), b"\x89PNG\r\n\x1a");
        assert_eq!(data_url("data:image/png;base64,iVBORw==").unwrap(), b"\x89PNG");
        let dir = crate::db::tests::temp_dir("icons");
        std::fs::write(dir.join("github.com.png"), b"\x89PNG").unwrap();
        std::fs::write(dir.join(FAILED), serde_json::json!({ "gone.example": now() }).to_string()).unwrap();
        let links = |hs: &[&str]| hs.iter().map(|h| (h.to_string(), format!("https://{h}/"))).collect::<Vec<_>>();
        let m = missing(&dir, &links(&["github.com", "gone.example", "openrouter.ai", "openrouter.ai", ""]));
        assert_eq!(m.iter().map(|(h, _)| h.as_str()).collect::<Vec<_>>(), ["openrouter.ai"]);
        assert!(cached(&dir)["github.com"].starts_with("data:image/png;base64,"));
    }

    /// Real sites: `cargo test real_site_icons -- --ignored --nocapture` (goes online).
    #[test]
    #[ignore]
    fn real_site_icons() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let client = reqwest::Client::builder().user_agent(AGENT).timeout(Duration::from_secs(10)).build().unwrap();
        for url in ["https://github.com/settings/education/benefits", "https://platform.deepseek.com/transactions", "https://openrouter.ai/", "https://www.bilibili.com/account/history", "https://bailian.console.aliyun.com/", "https://kns.cnki.net/kns8s/"] {
            let got = rt.block_on(fetch(&client, url));
            println!("{url}: {:?}", got.map(|(b, k)| (k, b.len())));
        }
    }
}
