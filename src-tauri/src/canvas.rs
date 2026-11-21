//! 有板有眼: visual-atelier's canvases, looked at inside Pomelo. Pomelo keeps none of it — the canvases are files in the
//! visual-atelier folder (`~/Documents/GitHub/visual-atelier`, or the folder in the setting `canvas.folder`), shown
//! by that repository's own viewer, the bench: `node bench/bin/dw.js serve`, a server on 127.0.0.1 whose page is the
//! canvas UI (pan, pages, one board full size, Tweaks, 批准). Pomelo starts it, asks it what there is, shows its page
//! in a frame and stops it, all through the interface the bench gives other programs (its README「给别的程序用」):
//!   `dw serve --port N --json` · `GET /api/status` (says `app: visual-atelier-bench`) · `GET /api/index` (four
//!   lists: Design System, Motion Deck, Explore Deck, Questionnaire; a bench from before it has only
//!   `GET /api/canvases`) · `GET /api/canvases/<name>` · `POST /api/stop` (JSON) · and since api 3, renaming and
//!   deleting (站主 2026-11-13): `POST /api/canvases/<name>/rename|delete|page-rename|page-delete`,
//!   `POST /api/trash/restore` (the undo); since api 4 (站主 2026-11-14) boards too (`board-rename|board-delete`) and a
//!   Design System's or Motion Deck's name in the bench only (`POST /api/names/<ds|motion>/<scheme>`, empty = its own
//!   name again; the list then carries `schemeTitle`). Deleting moves things into visual-atelier's own trash, never here.
//!   Since api 5 (站主 2026-11-15, visual-atelier's Phase 2): a fifth list, Checks (one row, `npm run all`, shown at its
//!   `route`; a bench without it has no such list here), and each board's 批准状态 (`GET /api/review/<name>`; 批准、退回
//!   are clicked only in the bench's UI, the side column just shows them and follows its `reviews` messages).
//! Running = a bench answers on the port. One started elsewhere (a terminal, an agent) is used as it is; 停止 asks
//! whichever is there to stop. Ours also stops when Pomelo quits. Something else on the port is a problem, said so.
//! Nothing here is data: no table, no backup, export, undo, search, and 柚子 doesn't read it.
//! Ports: the real app uses the bench's own 4321 (so it meets a `dw serve` from a terminal); the dev build, tests and a
//! scratch data dir use 4331, away from it.

use crate::site::server::{self, Server};
use crate::site::tools;
use crate::AppState;
use serde::Serialize;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

pub const FOLDER_KEY: &str = "canvas.folder";
const BENCH_APP: &str = "visual-atelier-bench";
const REAL_PORT: u16 = 4321;
const SCRATCH_PORT: u16 = 4331;
const START_WAIT: Duration = Duration::from_secs(30);

pub struct Canvas {
    home: PathBuf,
    port: u16,
    server: Server,
}

impl Canvas {
    /// `scratch`: the dev build, tests, or a data dir given by hand.
    pub fn new(home: &Path, scratch: bool) -> Self {
        Self { home: home.into(), port: if scratch || cfg!(debug_assertions) { SCRATCH_PORT } else { REAL_PORT }, server: Server::default() }
    }

    pub fn default_folder(&self) -> PathBuf {
        self.home.join("Documents/GitHub/visual-atelier")
    }

    /// Pomelo is quitting: the bench we started stops with it.
    pub fn on_exit(&self) {
        self.server.stop();
    }
}

// ---------- talking to the bench (plain HTTP/1.0 on 127.0.0.1: one request, the whole answer) ----------

fn http(port: u16, method: &str, path: &str, body: Option<&str>, timeout: Duration) -> Result<(u16, String), String> {
    let fail = |e: std::io::Error| format!("连不上画布服务：{e}");
    let mut s = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(400)).map_err(fail)?;
    s.set_read_timeout(Some(timeout)).map_err(fail)?;
    s.set_write_timeout(Some(timeout)).map_err(fail)?;
    let body = body.unwrap_or("");
    // head and body in one write: a short request arrives whole, nothing is left unread when the answer closes
    let req = format!("{method} {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    s.write_all(req.as_bytes()).map_err(fail)?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(fail)?;
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("画布服务的回答看不懂")?;
    let status = head.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).ok_or("画布服务的回答看不懂")?;
    Ok((status, body.to_string()))
}

fn get_json(port: u16, path: &str) -> Result<Value, String> {
    let (status, body) = http(port, "GET", path, None, Duration::from_secs(8))?;
    if status != 200 {
        return Err(format!("画布服务回了 {status}"));
    }
    serde_json::from_str(&body).map_err(|_| "画布服务的回答看不懂".to_string())
}

/// The bench answering on the port, if it is one.
fn bench_on(port: u16) -> Option<Value> {
    if !server::port_open(port) {
        return None;
    }
    let (status, body) = http(port, "GET", "/api/status", None, Duration::from_secs(2)).ok()?;
    let v: Value = (status == 200).then(|| serde_json::from_str(&body).ok()).flatten()?;
    (v["app"] == BENCH_APP).then_some(v)
}

/// Why the port can't be used: something that isn't a bench answers there — or a bench from before it could say who
/// it is (a `dw serve` left running since before Dawn Plan · Phase 8: it lists canvases but has no `/api/status`).
fn taken(port: u16) -> String {
    let old = get_json(port, "/api/canvases").is_ok_and(|v| v["canvases"].is_array());
    if old {
        format!("{port} 端口上是一个旧的画布服务（以前开的 dw serve）：先把它停掉，再点启动")
    } else {
        format!("{port} 端口被别的程序占着，画布服务起不来")
    }
}

// ---------- what the page shows ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub folder: String,
    pub is_default: bool,
    /// Why it can't be started (no folder, no bench in it, dependencies not installed, no Node, the port taken).
    pub problem: Option<String>,
    /// stopped | starting | running
    pub state: &'static str,
    /// Started by Pomelo.
    pub ours: bool,
    /// The bench's address, no slash at the end.
    pub url: String,
    /// Why the last start failed.
    pub error: Option<String>,
}

/// Why this folder can't run the bench.
pub fn check_folder(folder: &Path) -> Option<String> {
    if !folder.is_dir() {
        return Some(format!("找不到 visual-atelier 文件夹：{}", folder.display()));
    }
    if !folder.join("bench/bin/dw.js").is_file() {
        return Some(format!("这个文件夹里没有看画布的引擎（bench/bin/dw.js）：{}", folder.display()));
    }
    if !folder.join("node_modules").is_dir() {
        return Some("visual-atelier 还没装依赖（node_modules）：先在它的文件夹里跑一次 npm ci".into());
    }
    None
}

fn folder_of(app: &AppHandle) -> (PathBuf, bool) {
    let canvas = app.state::<Canvas>();
    let set = app.state::<AppState>().with_db(|c| crate::settings::get(c, FOLDER_KEY)).ok().flatten().and_then(|v| v.as_str().map(PathBuf::from)).filter(|p| !p.as_os_str().is_empty());
    match set {
        Some(p) => {
            let is_default = p == canvas.default_folder();
            (p, is_default)
        }
        None => (canvas.default_folder(), true),
    }
}

fn overview_of(canvas: &Canvas, folder: &Path, is_default: bool) -> Overview {
    let view = canvas.server.view(canvas.port);
    let bench = bench_on(canvas.port).is_some();
    let in_the_way = view.state == "running" && !bench;
    Overview {
        folder: folder.display().to_string(),
        is_default,
        problem: if bench {
            None
        } else if in_the_way {
            Some(taken(canvas.port))
        } else {
            check_folder(folder)
        },
        state: if bench {
            "running"
        } else if view.state == "starting" {
            "starting"
        } else {
            "stopped"
        },
        ours: view.ours,
        url: format!("http://127.0.0.1:{}", canvas.port),
        error: if bench { None } else { view.error },
    }
}

fn overview(app: &AppHandle) -> Overview {
    let (folder, is_default) = folder_of(app);
    overview_of(&app.state::<Canvas>(), &folder, is_default)
}

/// Starts the bench (unless one answers already) and waits until it does.
fn start(canvas: &Canvas, folder: &Path) -> Result<(), String> {
    if bench_on(canvas.port).is_some() {
        return Ok(());
    }
    if server::port_open(canvas.port) {
        return Err(taken(canvas.port));
    }
    if let Some(p) = check_folder(folder) {
        return Err(p);
    }
    let node_bin = tools::node_bin(folder, &canvas.home).map_err(|e| e.replace("网站", "visual-atelier "))?;
    let port = canvas.port.to_string();
    canvas.server.start_program(&node_bin.join("node"), folder, &["bench/bin/dw.js", "serve", "--port", &port, "--json"], &node_bin, canvas.port)?;
    canvas.server.wait_ready(canvas.port, START_WAIT)?;
    // the port answers: wait a moment more for it to say it is the bench
    let begun = Instant::now();
    while bench_on(canvas.port).is_none() {
        if begun.elapsed() > Duration::from_secs(5) {
            return Err("画布服务启动了，但回答不对".into());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Ok(())
}

/// Asks the bench on the port to stop (whoever started it), then makes sure ours is gone.
fn stop(canvas: &Canvas) {
    if bench_on(canvas.port).is_some() {
        let _ = http(canvas.port, "POST", "/api/stop", Some("{}"), Duration::from_secs(3));
    }
    let begun = Instant::now();
    while server::port_open(canvas.port) && begun.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(100));
    }
    canvas.server.stop();
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CanvasRow {
    pub name: String,
    pub title: String,
    /// ds (a Design System) | motion (a scheme's Motion Deck) | canvas (an Explore Deck: boards on pages) |
    /// page (a Questionnaire: one page of its own) | checks (`npm run all`'s results)
    pub kind: String,
    /// The line under the title, as the bench words it (「45 张板 · 12 页」「39 个零件」).
    pub sub: String,
    pub boards: usize,
    pub pages: Vec<Page>,
    /// A Design System or Motion Deck named in the bench: the scheme's own name (「回到原名」), else None.
    pub scheme_title: Option<String>,
    /// Where the bench's UI shows it (after `#`), as the bench says; Checks are opened by it.
    pub route: Option<String>,
}

/// One of the four lists, in the bench's order.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    /// ds | motion | explore | questionnaire | checks
    pub id: String,
    pub title: String,
    pub items: Vec<CanvasRow>,
}

const SECTIONS: [(&str, &str); 4] = [("ds", "Design System"), ("motion", "Motion Deck"), ("explore", "Explore Deck"), ("questionnaire", "Questionnaire")];
/// The fifth list (api 5), only when the bench sends it.
const CHECKS: (&str, &str) = ("checks", "Checks");

/// A board's 批准状态 as the bench keeps it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    /// proposed (待批) | approved (已批准) | rejected (退回)
    pub status: String,
    /// Why it was sent back (or a note).
    pub note: Option<String>,
    /// The board changed after it was marked.
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub file: String,
    pub title: String,
    /// The page it is on (the first page when the canvas has pages and the board names none).
    pub page: Option<String>,
    /// None: never marked (or a bench from before api 5).
    pub review: Option<Review>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    pub name: String,
    pub pages: Vec<Page>,
    pub boards: Vec<Board>,
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn pages_of(v: &Value) -> Vec<Page> {
    v.as_array().map(|a| a.iter().map(|p| Page { id: text(&p["id"]), name: text(&p["name"]) }).filter(|p| !p.id.is_empty()).collect()).unwrap_or_default()
}

fn row_of(c: &Value, kind: &str) -> CanvasRow {
    let boards = c["boards"].as_u64().unwrap_or(0) as usize;
    let pages = pages_of(&c["pages"]);
    let sub = c["sub"].as_str().map(str::to_string).unwrap_or_else(|| match kind {
        "page" => "问卷页".into(),
        "canvas" if pages.len() > 1 => format!("{boards} 张板 · {} 页", pages.len()),
        "canvas" => format!("{boards} 张板"),
        _ => String::new(),
    });
    let scheme_title = c["schemeTitle"].as_str().filter(|t| !t.is_empty()).map(str::to_string);
    let route = c["route"].as_str().filter(|r| r.starts_with('/')).map(str::to_string);
    CanvasRow { name: text(&c["name"]), title: text(&c["title"]), kind: kind.into(), sub, boards, pages, scheme_title, route }
}

/// `GET /api/index` as the four lists (one the bench doesn't send comes back empty, so the page always has four), and
/// Checks after them when the bench has it.
pub fn sections_of(v: &Value) -> Vec<Section> {
    let has_checks = v["sections"].as_array().is_some_and(|a| a.iter().any(|s| s["id"] == CHECKS.0));
    SECTIONS
        .iter()
        .chain(has_checks.then_some(&CHECKS))
        .map(|(id, title)| {
            let sec = v["sections"].as_array().and_then(|a| a.iter().find(|s| s["id"] == *id));
            let items = sec
                .and_then(|s| s["items"].as_array())
                .map(|list| {
                    list.iter()
                        .map(|c| {
                            let kind = match *id {
                                "ds" | "motion" | "checks" => (*id).to_string(),
                                "questionnaire" => c["kind"].as_str().unwrap_or("page").to_string(),
                                _ => c["kind"].as_str().unwrap_or("canvas").to_string(),
                            };
                            row_of(c, &kind)
                        })
                        .filter(|c| !c.name.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            Section { id: (*id).into(), title: sec.and_then(|s| s["title"].as_str()).unwrap_or(title).to_string(), items }
        })
        .collect()
}

/// A bench from before the four lists (api 1): `GET /api/canvases`, the canvases as Explore Decks and the pages as
/// Questionnaires.
pub fn sections_from_canvases(v: &Value) -> Vec<Section> {
    let rows: Vec<CanvasRow> = v["canvases"].as_array().map(|l| l.iter().map(|c| row_of(c, c["kind"].as_str().unwrap_or("canvas"))).filter(|c| !c.name.is_empty()).collect()).unwrap_or_default();
    SECTIONS
        .iter()
        .map(|(id, title)| Section {
            id: (*id).into(),
            title: (*title).into(),
            items: rows.iter().filter(|r| (*id == "explore" && r.kind == "canvas") || (*id == "questionnaire" && r.kind == "page")).cloned().collect(),
        })
        .collect()
}

fn list(port: u16) -> Result<Vec<Section>, String> {
    match get_json(port, "/api/index") {
        Ok(v) => Ok(sections_of(&v)),
        Err(_) => Ok(sections_from_canvases(&get_json(port, "/api/canvases")?)),
    }
}

/// `GET /api/canvases/<name>` as its pages and its boards in the canvas's own order.
pub fn detail_of(name: &str, v: &Value) -> Detail {
    let canvas = &v["canvas"];
    let pages = pages_of(&canvas["pages"]);
    let first = pages.first().map(|p| p.id.clone());
    let map = canvas["boards"].as_object();
    let mut files: Vec<String> = canvas["order"].as_array().map(|o| o.iter().map(text).filter(|f| map.is_some_and(|m| m.contains_key(f))).collect()).unwrap_or_default();
    if let Some(m) = map {
        for f in m.keys() {
            if !files.contains(f) {
                files.push(f.clone());
            }
        }
    }
    let boards = files
        .into_iter()
        .map(|f| {
            let b = &canvas["boards"][&f];
            let title = b["title"].as_str().filter(|t| !t.is_empty()).map(str::to_string).unwrap_or_else(|| f.trim_end_matches(".dc.html").to_string());
            Board { page: b["page"].as_str().map(str::to_string).or_else(|| first.clone()), title, file: f, review: None }
        })
        .collect();
    Detail { name: name.to_string(), pages, boards }
}

/// `GET /api/review/<name>`'s `reviews` (or a `reviews` message's): file → its 批准状态.
pub fn reviews_of(v: &Value) -> std::collections::HashMap<String, Review> {
    v.as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(file, r)| {
                    let status = r["status"].as_str().filter(|s| matches!(*s, "proposed" | "approved" | "rejected"))?;
                    Some((file.clone(), Review { status: status.into(), note: r["note"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string), changed: r["changed"].as_bool().unwrap_or(false) }))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What a rename, delete or undo came back with: the trash id a delete can be undone with, the canvas, the page
/// (when it was one, or the page a board is on) and the board (when it was one), and its title.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    pub id: Option<String>,
    pub canvas: String,
    pub page: Option<String>,
    pub file: Option<String>,
    pub title: String,
}

/// `POST` a JSON body; the bench answers JSON, and `{ message }` when it refuses (said as it says it).
fn post_json(port: u16, path: &str, body: &Value) -> Result<Value, String> {
    let (status, text) = http(port, "POST", path, Some(&body.to_string()), Duration::from_secs(15))?;
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    match status {
        200 => Ok(v),
        // no such address at all (not a JSON refusal): a bench from before it could rename and delete
        404 if v.is_null() => Err("画布服务是旧的，还不会改名和删除：点「停止」再「启动」一次".into()),
        _ => Err(v["message"].as_str().map(str::to_string).unwrap_or_else(|| format!("画布服务回了 {status}"))),
    }
}

fn changed_of(v: &Value, canvas: &str) -> Changed {
    let page = v["page"].as_str().map(str::to_string).or_else(|| v["page"]["id"].as_str().map(str::to_string));
    let canvas = v["canvas"].as_str().or_else(|| v["name"].as_str()).unwrap_or(canvas).to_string();
    Changed { id: v["id"].as_str().map(str::to_string), canvas, page, file: v["file"].as_str().map(str::to_string), title: text(&v["title"]) }
}

/// New title for a board of a canvas (its file and address stay).
pub fn rename_board(port: u16, name: &str, file: &str, title: &str) -> Result<Changed, String> {
    if !valid_name(name) {
        return Err("没有这块画布".into());
    }
    Ok(changed_of(&post_json(port, &format!("/api/canvases/{name}/board-rename"), &serde_json::json!({ "file": file, "title": title }))?, name))
}

/// Deletes a board of a canvas (into visual-atelier's trash with its 批准 record; `restore` with the id puts it back).
pub fn delete_board(port: u16, name: &str, file: &str) -> Result<Changed, String> {
    if !valid_name(name) {
        return Err("没有这块画布".into());
    }
    Ok(changed_of(&post_json(port, &format!("/api/canvases/{name}/board-delete"), &serde_json::json!({ "file": file }))?, name))
}

/// A Design System's (`ds`) or Motion Deck's (`motion`) name in the bench and here only (the scheme, its source and the
/// cloud keep theirs); None or empty = the scheme's own name again. Comes back with the name now shown.
pub fn name_scheme(port: u16, kind: &str, scheme: &str, title: Option<&str>) -> Result<Changed, String> {
    if !matches!(kind, "ds" | "motion") || !valid_name(scheme) {
        return Err("只能给 Design System 和 Motion Deck 改名".into());
    }
    let v = post_json(port, &format!("/api/names/{kind}/{scheme}"), &serde_json::json!({ "title": title.unwrap_or("") }))?;
    Ok(Changed { id: None, canvas: scheme.to_string(), page: None, file: None, title: text(&v["title"]) })
}

/// New title for a canvas, or for one of its pages.
pub fn rename(port: u16, name: &str, page: Option<&str>, title: &str) -> Result<Changed, String> {
    if !valid_name(name) {
        return Err("没有这块画布".into());
    }
    let v = match page {
        Some(p) => post_json(port, &format!("/api/canvases/{name}/page-rename"), &serde_json::json!({ "page": p, "name": title }))?,
        None => post_json(port, &format!("/api/canvases/{name}/rename"), &serde_json::json!({ "title": title }))?,
    };
    Ok(changed_of(&v, name))
}

/// Deletes a canvas, or one of its pages (into visual-atelier's trash; `restore` with the id puts it back).
pub fn delete(port: u16, name: &str, page: Option<&str>) -> Result<Changed, String> {
    if !valid_name(name) {
        return Err("没有这块画布".into());
    }
    let v = match page {
        Some(p) => post_json(port, &format!("/api/canvases/{name}/page-delete"), &serde_json::json!({ "page": p }))?,
        None => post_json(port, &format!("/api/canvases/{name}/delete"), &serde_json::json!({}))?,
    };
    Ok(changed_of(&v, name))
}

pub fn restore(port: u16, id: &str) -> Result<Changed, String> {
    let v = post_json(port, "/api/trash/restore", &serde_json::json!({ "id": id }))?;
    let mut c = changed_of(&v, "");
    c.id = None;
    Ok(c)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

// ---------- commands ----------

async fn blocking<T: Send + 'static>(app: AppHandle, f: impl FnOnce(&AppHandle) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || f(&app)).await.map_err(|e| format!("出错了：{e}"))?
}

#[tauri::command]
pub async fn canvas_overview(app: AppHandle) -> Result<Overview, String> {
    blocking(app, |a| Ok(overview(a))).await
}

#[tauri::command]
pub async fn canvas_start(app: AppHandle) -> Result<Overview, String> {
    blocking(app, |a| {
        let (folder, _) = folder_of(a);
        start(&a.state::<Canvas>(), &folder)?;
        Ok(overview(a))
    })
    .await
}

#[tauri::command]
pub async fn canvas_stop(app: AppHandle) -> Result<Overview, String> {
    blocking(app, |a| {
        stop(&a.state::<Canvas>());
        Ok(overview(a))
    })
    .await
}

#[tauri::command]
pub async fn canvas_list(app: AppHandle) -> Result<Vec<Section>, String> {
    blocking(app, |a| list(a.state::<Canvas>().port)).await
}

#[tauri::command]
pub async fn canvas_boards(app: AppHandle, name: String) -> Result<Detail, String> {
    blocking(app, move |a| {
        if !valid_name(&name) {
            return Err("没有这块画布".into());
        }
        let port = a.state::<Canvas>().port;
        let mut d = detail_of(&name, &get_json(port, &format!("/api/canvases/{name}"))?);
        // a bench from before api 5 has no 批准状态 to give: the boards just show none
        if let Ok(v) = get_json(port, &format!("/api/review/{name}")) {
            let mut rs = reviews_of(&v["reviews"]);
            d.boards.iter_mut().for_each(|b| b.review = rs.remove(&b.file));
        }
        Ok(d)
    })
    .await
}

/// A canvas, one of its pages (`page`) or one of its boards (`board`, its file).
#[tauri::command]
pub async fn canvas_rename(app: AppHandle, name: String, page: Option<String>, board: Option<String>, title: String) -> Result<Changed, String> {
    blocking(app, move |a| match board {
        Some(f) => rename_board(a.state::<Canvas>().port, &name, &f, &title),
        None => rename(a.state::<Canvas>().port, &name, page.as_deref(), &title),
    })
    .await
}

#[tauri::command]
pub async fn canvas_delete(app: AppHandle, name: String, page: Option<String>, board: Option<String>) -> Result<Changed, String> {
    blocking(app, move |a| match board {
        Some(f) => delete_board(a.state::<Canvas>().port, &name, &f),
        None => delete(a.state::<Canvas>().port, &name, page.as_deref()),
    })
    .await
}

/// A Design System's or Motion Deck's name in the bench (`kind` ds | motion); none = its own name again.
#[tauri::command]
pub async fn canvas_name(app: AppHandle, kind: String, name: String, title: Option<String>) -> Result<Changed, String> {
    blocking(app, move |a| name_scheme(a.state::<Canvas>().port, &kind, &name, title.as_deref())).await
}

#[tauri::command]
pub async fn canvas_restore(app: AppHandle, id: String) -> Result<Changed, String> {
    blocking(app, move |a| restore(a.state::<Canvas>().port, &id)).await
}

/// Another visual-atelier folder (empty = the default one). The bench we started stops first: it shows the old folder.
#[tauri::command]
pub async fn canvas_set_folder(app: AppHandle, folder: String) -> Result<Overview, String> {
    blocking(app, move |a| {
        let canvas = a.state::<Canvas>();
        let folder = folder.trim().to_string();
        if !folder.is_empty() {
            if let Some(p) = check_folder(Path::new(&folder)) {
                return Err(p);
            }
        }
        stop(&canvas);
        a.state::<AppState>().with_db(|c| crate::settings::set(c, FOLDER_KEY, &Value::String(folder)))?;
        Ok(overview(a))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use serde_json::json;
    use std::net::TcpListener;

    /// Something answering HTTP on a port of its own: `answer(method, path)` → (status, body). Counts what it was asked.
    fn fake(answer: fn(&str, &str) -> (u16, String)) -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = asked.clone();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let mut first = req.lines().next().unwrap_or("").split_whitespace();
                let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
                if method.is_empty() {
                    continue; // a probe that only connected
                }
                log.lock().unwrap().push(format!("{method} {path}{}", if req.contains("Content-Type: application/json") { " json" } else { "" }));
                let (status, body) = answer(method, path);
                let _ = s.write_all(format!("HTTP/1.0 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes());
            }
        });
        (port, asked)
    }

    fn bench(method: &str, path: &str) -> (u16, String) {
        match (method, path) {
            ("GET", "/api/status") => (200, json!({"app": BENCH_APP, "api": 1, "pid": 7, "stoppable": true}).to_string()),
            ("GET", "/api/canvases") => (200, json!({"canvases": [{"name": "pomelo-explore", "kind": "canvas", "title": "Pomelo Explore", "boards": 13, "pages": [{"id": "p2", "name": "Phase 2 · 最终设计"}]}, {"name": "survey", "kind": "page", "title": "问卷", "entry": "survey.html"}]}).to_string()),
            ("GET", "/api/index") => (
                200,
                json!({"sections": [
                    {"id": "ds", "title": "Design System", "items": [{"name": "pomelo", "title": "Pomelo 柚儿园", "sub": "schemes · pomelo", "route": "/ds/pomelo"}]},
                    {"id": "motion", "title": "Motion Deck", "items": [{"name": "pomelo", "title": "Pomelo 柚儿园", "parts": 39, "sub": "39 个零件"}]},
                    {"id": "explore", "title": "Explore Deck", "items": [{"name": "pomelo-explore", "kind": "canvas", "title": "Pomelo Explore", "boards": 13, "pages": [{"id": "p2", "name": "Phase 2 · 最终设计"}], "sub": "13 张板"}]},
                    {"id": "questionnaire", "title": "Questionnaire", "items": [{"name": "survey", "kind": "page", "title": "问卷", "sub": "问卷页"}]},
                    {"id": "checks", "title": "Checks", "items": [{"name": "all", "title": "npm run all", "sub": "全过 · 11-15 10:20", "route": "/qa/all"}]}
                ]})
                .to_string(),
            ),
            ("POST", "/api/stop") => (200, json!({"ok": true}).to_string()),
            ("POST", "/api/canvases/pomelo-explore/rename") => (200, json!({"name": "pomelo-explore", "kind": "canvas", "title": "新名字"}).to_string()),
            ("POST", "/api/canvases/pomelo-explore/page-delete") => (200, json!({"id": "20261113-1-pomelo-explore--x1", "what": "page", "canvas": "pomelo-explore", "page": "x1", "title": "探索 1"}).to_string()),
            ("POST", "/api/canvases/pomelo-explore/page-rename") => (404, json!({"code": "not_found", "message": "没有这一页：x9"}).to_string()),
            ("POST", "/api/trash/restore") => (200, json!({"id": "20261113-1-pomelo-explore--x1", "what": "page", "canvas": "pomelo-explore", "page": {"id": "x1", "name": "探索 1"}, "title": "探索 1"}).to_string()),
            ("POST", "/api/canvases/pomelo-explore/board-rename") => (200, json!({"name": "pomelo-explore", "file": "Frame-Main.dc.html", "title": "桌面 2"}).to_string()),
            ("POST", "/api/canvases/pomelo-explore/board-delete") => (200, json!({"id": "20261114-1-pomelo-explore--Frame-Main", "what": "board", "canvas": "pomelo-explore", "file": "Frame-Main.dc.html", "page": "p2", "title": "桌面"}).to_string()),
            ("POST", "/api/names/ds/pomelo") => (200, json!({"kind": "ds", "name": "pomelo", "title": "柚儿园", "reset": false}).to_string()),
            ("POST", "/api/names/motion/pomelo") => (200, json!({"kind": "motion", "name": "pomelo", "title": null, "reset": true}).to_string()),
            _ => (404, "Not Found".into()),
        }
    }

    fn canvas_on(port: u16) -> Canvas {
        Canvas { home: temp_dir("canvas-home").to_path_buf(), port, server: Server::default() }
    }

    #[test]
    fn a_bench_on_the_port_is_running_and_anything_else_is_in_the_way() {
        let (port, asked) = fake(bench);
        let c = canvas_on(port);
        let folder = temp_dir("canvas-none").join("missing");
        let o = overview_of(&c, &folder, true);
        // one started elsewhere: running, not ours, and the folder's problems don't matter while it runs
        assert_eq!((o.state, o.ours, o.problem.clone(), o.url.as_str()), ("running", false, None, format!("http://127.0.0.1:{port}").as_str()));
        start(&c, &folder).unwrap(); // already there: nothing to start
        let flat = |secs: Vec<Section>| secs.into_iter().flat_map(|s| s.items.into_iter().map(move |r| (s.id.clone(), r.name, r.kind, r.boards, r.pages.len(), r.sub))).collect::<Vec<_>>();
        let s = |x: &str| x.to_string();
        // the four lists, in the bench's order; the same Pomelo name can be a Design System and a Motion Deck
        assert_eq!(
            flat(list(port).unwrap()),
            [
                (s("ds"), s("pomelo"), s("ds"), 0, 0, s("schemes · pomelo")),
                (s("motion"), s("pomelo"), s("motion"), 0, 0, s("39 个零件")),
                (s("explore"), s("pomelo-explore"), s("canvas"), 13, 1, s("13 张板")),
                (s("questionnaire"), s("survey"), s("page"), 0, 0, s("问卷页")),
                (s("checks"), s("all"), s("checks"), 0, 0, s("全过 · 11-15 10:20"))
            ]
        );
        assert_eq!(list(port).unwrap()[4].items[0].route.as_deref(), Some("/qa/all"));
        // a bench from before Checks: the four lists only
        assert_eq!(sections_of(&json!({"sections": []})).len(), 4);
        // a bench from before /api/index: canvases and pages only, still under the four titles
        let secs = sections_from_canvases(&get_json(port, "/api/canvases").unwrap());
        assert_eq!(secs.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Design System", "Motion Deck", "Explore Deck", "Questionnaire"]);
        assert_eq!(
            flat(secs),
            [(s("explore"), s("pomelo-explore"), s("canvas"), 13, 1, s("13 张板")), (s("questionnaire"), s("survey"), s("page"), 0, 0, s("问卷页"))]
        );
        // renaming and deleting: JSON in, the bench's own words back when it refuses
        assert_eq!(rename(port, "pomelo-explore", None, "新名字").unwrap().title, "新名字");
        let gone = delete(port, "pomelo-explore", Some("x1")).unwrap();
        assert_eq!((gone.id.as_deref(), gone.page.as_deref(), gone.title.as_str()), (Some("20261113-1-pomelo-explore--x1"), Some("x1"), "探索 1"));
        assert_eq!(rename(port, "pomelo-explore", Some("x9"), "x").unwrap_err(), "没有这一页：x9");
        let back = restore(port, "20261113-1-pomelo-explore--x1").unwrap();
        assert_eq!((back.canvas.as_str(), back.page.as_deref(), back.id), ("pomelo-explore", Some("x1"), None));
        assert!(delete(port, "../x", None).is_err());
        // a bench from before it could: said so
        assert!(delete(port, "survey", None).unwrap_err().contains("画布服务是旧的"));
        assert!(asked.lock().unwrap().contains(&"POST /api/canvases/pomelo-explore/rename json".to_string()));
        // api 4: boards, and the bench's own names for a Design System or a Motion Deck
        assert_eq!(rename_board(port, "pomelo-explore", "Frame-Main.dc.html", "桌面 2").unwrap().title, "桌面 2");
        let b = delete_board(port, "pomelo-explore", "Frame-Main.dc.html").unwrap();
        assert_eq!((b.id.as_deref(), b.page.as_deref(), b.file.as_deref()), (Some("20261114-1-pomelo-explore--Frame-Main"), Some("p2"), Some("Frame-Main.dc.html")));
        assert_eq!(name_scheme(port, "ds", "pomelo", Some("柚儿园")).unwrap().title, "柚儿园");
        assert_eq!(name_scheme(port, "motion", "pomelo", None).unwrap().title, "");
        assert!(name_scheme(port, "canvas", "pomelo", None).is_err());
        assert!(name_scheme(port, "ds", "vermilion", None).unwrap_err().contains("画布服务是旧的"));
        // 停止 asks it to stop, as JSON (a form post is refused by the bench)
        stop(&c);
        assert!(asked.lock().unwrap().contains(&"POST /api/stop json".to_string()));

        // something that isn't a bench
        let (other, _) = fake(|_, _| (200, "{\"app\":\"something else\"}".into()));
        let c = canvas_on(other);
        let o = overview_of(&c, &folder, true);
        assert_eq!(o.state, "stopped");
        assert!(o.problem.unwrap().contains("端口被别的程序占着"));
        assert!(start(&c, &folder).unwrap_err().contains("端口被别的程序占着"));
        assert!(get_json(other, "/api/nothing").is_ok());

        // a bench from before it had /api/status: named as what it is
        let (old, _) = fake(|_, path| if path == "/api/canvases" { (200, "{\"canvases\":[]}".into()) } else { (404, "Not Found".into()) });
        let o = overview_of(&canvas_on(old), &folder, true);
        assert_eq!(o.state, "stopped");
        assert!(o.problem.unwrap().contains("旧的画布服务"));
    }

    #[test]
    fn folder_problems_are_said_before_starting() {
        let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let c = canvas_on(free);
        let dir = temp_dir("canvas-folder");
        let folder = dir.join("visual-atelier");
        assert!(check_folder(&folder).unwrap().contains("找不到 visual-atelier 文件夹"));
        std::fs::create_dir_all(folder.join("bench/bin")).unwrap();
        assert!(check_folder(&folder).unwrap().contains("bench/bin/dw.js"));
        std::fs::write(folder.join("bench/bin/dw.js"), "").unwrap();
        assert!(check_folder(&folder).unwrap().contains("npm ci"));
        let o = overview_of(&c, &folder, false);
        assert_eq!((o.state, o.ours, o.is_default), ("stopped", false, false));
        assert!(start(&c, &folder).unwrap_err().contains("npm ci"));
        std::fs::create_dir_all(folder.join("node_modules")).unwrap();
        assert_eq!(check_folder(&folder), None);
        // no .nvmrc: no Node to run it with, said in its own words
        assert!(start(&c, &folder).unwrap_err().contains("visual-atelier"));
    }

    #[test]
    fn boards_come_in_the_canvas_order_with_their_page() {
        let v = json!({"name": "x", "kind": "canvas", "canvas": {
            "pages": [{"id": "p2", "name": "定稿"}, {"id": "p3", "name": "方案"}],
            "order": ["B.dc.html", "gone.dc.html", "A.dc.html"],
            "boards": {"A.dc.html": {"title": "桌面", "page": "p3"}, "B.dc.html": {"title": ""}, "C.dc.html": {"title": "后加的", "page": "p2"}}
        }});
        let d = detail_of("x", &v);
        assert_eq!(d.pages.len(), 2);
        assert_eq!(
            d.boards.iter().map(|b| (b.file.as_str(), b.title.as_str(), b.page.as_deref())).collect::<Vec<_>>(),
            [("B.dc.html", "B", Some("p2")), ("A.dc.html", "桌面", Some("p3")), ("C.dc.html", "后加的", Some("p2"))]
        );
        // a canvas without pages
        let d = detail_of("y", &json!({"canvas": {"boards": {"Main.dc.html": {"title": "首页"}}}}));
        assert_eq!((d.pages.len(), d.boards[0].page.clone()), (0, None));
        assert!(valid_name("pomelo-explore") && !valid_name("../x") && !valid_name(""));
        // 批准状态: the three the bench knows, a note only when written
        let rs = reviews_of(&json!({
            "A.dc.html": {"status": "approved", "note": "", "at": "2026-11-15T10:00:00Z", "by": "owner", "changed": true},
            "B.dc.html": {"status": "rejected", "note": " 字太小 ", "by": "owner"},
            "C.dc.html": {"status": "odd"}
        }));
        assert_eq!(rs.len(), 2);
        assert_eq!(rs["A.dc.html"], Review { status: "approved".into(), note: None, changed: true });
        assert_eq!(rs["B.dc.html"].note.as_deref(), Some("字太小"));
        assert!(reviews_of(&Value::Null).is_empty());
    }

    /// The real bench of the real visual-atelier folder, on a port of its own: started, asked, stopped.
    /// `cargo test real_canvas_bench -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_canvas_bench() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let folder = home.join("Documents/GitHub/visual-atelier");
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let c = Canvas { home, port, server: Server::default() };
        let t = Instant::now();
        start(&c, &folder).unwrap();
        println!("画布服务 {:.1}s", t.elapsed().as_secs_f32());
        let o = overview_of(&c, &folder, true);
        assert_eq!((o.state, o.ours, o.problem), ("running", true, None));
        let secs = list(port).unwrap();
        assert_eq!(secs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["ds", "motion", "explore", "questionnaire", "checks"]);
        assert!(secs[2].items.iter().any(|r| r.name == "pomelo-explore"));
        let d = detail_of("pomelo-explore", &get_json(port, "/api/canvases/pomelo-explore").unwrap());
        println!("{}；pomelo-explore：{} 页 {} 张板", secs.iter().map(|s| format!("{} {}", s.title, s.items.len())).collect::<Vec<_>>().join("、"), d.pages.len(), d.boards.len());
        assert!(d.boards.len() > 5 && d.boards.iter().all(|b| b.page.is_some()));
        stop(&c);
        assert!(!server::port_open(port));
        assert_eq!(overview_of(&c, &folder, true).state, "stopped");
    }
}
