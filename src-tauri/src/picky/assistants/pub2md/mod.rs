//! Pub2MD (v0.6, the second 助手): PDF, Word and images → Mathpix Markdown (Mathpix), cleaned chunk by
//! chunk (整理), translated (翻译: 英文 → 简体中文, or 简繁互转); MD / MMD / TXT files are only translated.
//! Rules in requirements.md「Pub2MD」; the steps in job.rs.
//! - Two models: the Worker cleans and translates each chunk; the Supervisor reviews the cut, makes the term
//!   list (looking terms up on the web: search.rs), troubleshoots chunks that fail the program's checks,
//!   reviews chunks (精修, a switch) and reads the whole translation through (通读). Each has its own
//!   model (None = 小拾现在选的) and thinking level.
//! - No data folder of its own: each document gets a folder in the output folder (default 下载) holding the
//!   results, the work log and, while it isn't done, a hidden `.pub2md` with what 继续 needs.
//! - Settings and the last 10 documents live in picky.db `preferences` (`assistant.pub2md`); never undone,
//!   never exported. One document at a time; files added meanwhile wait in a queue (in memory).
//! - The Mathpix App ID and App Key live in the keychain (mathpix.rs), filled in on the page.
//! - 另存为 (Word, PDF, LaTeX, HTML): the results are sent to Mathpix's converter at the end (job.rs
//!   `convert`); their images by Mathpix's own links (kept 30 days after 提取).
//! - 预览: an output's text with its images inlined (data: links) for the page's mathpix-markdown-it.

pub mod check;
pub mod chunk;
pub mod job;
pub mod mathpix;
pub mod mmd;
pub mod prompts;
pub mod script;
pub mod search;
pub mod source;

use crate::picky::cloud::Effort;
use crate::picky::store::{pref_get, pref_set};
use crate::AppState;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

const KEY: &str = "assistant.pub2md";
const RECENT: usize = 10;
pub const MAX_PARALLEL: u8 = 8;

/// How hard a model thinks: 关 / 低 / 中 / 高.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl Level {
    pub fn thinking(self) -> bool {
        self != Level::Off
    }
    pub fn effort(self) -> Option<Effort> {
        match self {
            Level::Off => None,
            Level::Low => Some(Effort::Low),
            Level::Medium => Some(Effort::Medium),
            Level::High => Some(Effort::High),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Level::Off => "不思考",
            Level::Low => "思考 · 低",
            Level::Medium => "思考 · 中",
            Level::High => "思考 · 高",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Steps {
    /// 只提取 (Mathpix only)
    Extract,
    /// 提取 + 整理
    Clean,
    /// 提取 + 整理 + 翻译 (text files: 翻译 only)
    #[default]
    Translate,
}

/// Which way 翻译 goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lang {
    /// 英文 → 简体中文
    #[default]
    EnZh,
    /// 简体 → 繁体 (OpenCC, then the model)
    ZhHant,
    /// 繁体 → 简体
    HantZh,
}

impl Lang {
    /// (from, to) as the models are told.
    pub fn says(self) -> (&'static str, &'static str) {
        match self {
            Lang::EnZh => ("英文", "简体中文"),
            Lang::ZhHant => ("简体中文", "繁体中文"),
            Lang::HantZh => ("繁体中文", "简体中文"),
        }
    }
    /// 简繁互转: to traditional or not; None for a translation between languages.
    pub fn script(self) -> Option<bool> {
        match self {
            Lang::EnZh => None,
            Lang::ZhHant => Some(true),
            Lang::HantZh => Some(false),
        }
    }
}

/// (key, name, what the Worker is told) — Pub2MD's 文体 and 领域.
pub const STYLES: &[(&str, &str, &str)] = &[
    ("general", "通用", "用清楚、自然的中文，忠于作者的语气"),
    ("academic", "学术", "用严谨的学术写法，术语前后一致"),
    ("literary", "文学", "保留意象、语气、节奏和作者有意的含糊"),
    ("news", "新闻", "用简洁的新闻写法，保留消息来源和不确定的说法"),
];
pub const DOMAINS: &[(&str, &str, &str)] = &[
    ("general", "通用", "按上下文理解术语"),
    ("public", "公共管理", "用公共管理领域的术语，分清各种机构、职位和制度"),
    ("math", "数学", "用数学领域的术语，保留记号和逻辑上的限定"),
    ("cs", "计算机", "用计算机领域的术语，代码、标识符原样保留"),
];
pub const PROMPT_CHARS: usize = 2000;

/// What the results are also turned into (by Mathpix).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    Docx,
    Pdf,
    Latex,
    Html,
}

impl Format {
    /// Mathpix's key, also the file's ending (the zips hold the images).
    pub fn key(self) -> &'static str {
        match self {
            Format::Docx => "docx",
            Format::Pdf => "pdf",
            Format::Latex => "tex.zip",
            Format::Html => "html.zip",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Format::Docx => "Word",
            Format::Pdf => "PDF",
            Format::Latex => "LaTeX",
            Format::Html => "HTML",
        }
    }
}

/// The backup model at first: DeepSeek Flash (小拾's default cloud model).
pub const BACKUP: &str = "cloud:deepseek-flash";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub steps: Steps,
    /// Markdown out (else MMD).
    pub md: bool,
    /// Bilingual text: keep only the English.
    pub english_only: bool,
    /// local:<key> | cloud:<id>; None = 小拾现在选的.
    pub worker: Option<String>,
    pub worker_level: Level,
    pub supervisor: Option<String>,
    pub supervisor_level: Level,
    /// cloud:<id> for a request a content check turned down; "" = 不用.
    pub backup: String,
    /// Chunks at the same time.
    pub parallel: u8,
    /// None = 下载.
    pub folder: Option<String>,
    pub lang: Lang,
    /// STYLES / DOMAINS keys.
    pub style: String,
    pub domain: String,
    /// The user's own paragraph (both models see it).
    pub prompt: String,
    /// 联网查术语; None = on when the Supervisor's service has its own search (search.rs).
    pub web: Option<bool>,
    /// 精修: the Supervisor reviews each chunk after 整理 and after 翻译.
    pub refine: bool,
    /// Parts not translated (kept as they are; 前置信息: only the title is translated).
    pub skip: Vec<mmd::Part>,
    /// 另存为: also turned into these.
    pub formats: Vec<Format>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            steps: Steps::Translate,
            md: false,
            english_only: false,
            worker: None,
            worker_level: Level::Off,
            supervisor: None,
            supervisor_level: Level::High,
            backup: BACKUP.into(),
            parallel: 4,
            folder: None,
            lang: Lang::EnZh,
            style: "general".into(),
            domain: "general".into(),
            prompt: String::new(),
            web: None,
            refine: false,
            skip: vec![mmd::Part::Front, mmd::Part::Acknowledgments, mmd::Part::References, mmd::Part::Appendix],
            formats: vec![Format::Docx],
        }
    }
}

pub fn checked(mut s: Settings) -> Result<Settings, String> {
    for m in [&s.worker, &s.supervisor] {
        if m.as_deref().is_some_and(|m| !(m.starts_with("local:") || m.starts_with("cloud:"))) {
            return Err("不认识这个模型".into());
        }
    }
    if !(s.backup.is_empty() || s.backup.starts_with("cloud:")) {
        return Err("备用模型只能选云端模型".into());
    }
    s.parallel = s.parallel.clamp(1, MAX_PARALLEL);
    if !STYLES.iter().any(|x| x.0 == s.style) {
        s.style = "general".into();
    }
    if !DOMAINS.iter().any(|x| x.0 == s.domain) {
        s.domain = "general".into();
    }
    if s.prompt.chars().count() > PROMPT_CHARS {
        return Err(format!("自己写的要求最多 {PROMPT_CHARS} 字"));
    }
    s.skip.sort_by_key(|p| *p as u8);
    s.skip.dedup();
    s.formats.sort();
    s.formats.dedup();
    if let Some(f) = s.folder.as_deref() {
        if !Path::new(f).is_dir() {
            return Err("找不到这个文件夹".into());
        }
    }
    Ok(s)
}

pub fn downloads() -> PathBuf {
    std::env::var("HOME").map(|h| PathBuf::from(h).join("Downloads")).unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Where documents go now.
pub fn out_dir(s: &Settings) -> PathBuf {
    s.folder.as_deref().map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(downloads)
}

/// A document done, stopped or failed (the last 10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Recent {
    pub name: String,
    /// The document's folder (also its id).
    pub folder: String,
    /// "YYYY-MM-DD HH:MM", when it was started.
    pub at: String,
    /// running | done | stopped | failed
    pub state: String,
    /// Why it stopped or failed.
    pub message: Option<String>,
    /// The files written for the user (names in the folder).
    pub outputs: Vec<String>,
    /// Of the source files (an earlier Mathpix result can be used again).
    pub fingerprint: String,
    /// "YYYY-MM-DD", when Mathpix made the result; its images (assets/ name, Mathpix's link: kept 30 days).
    pub extracted: String,
    pub images: Vec<(String, String)>,
}

impl Default for Recent {
    fn default() -> Self {
        Recent { name: String::new(), folder: String::new(), at: String::new(), state: "running".into(), message: None, outputs: vec![], fingerprint: String::new(), extracted: String::new(), images: vec![] }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Saved {
    pub settings: Settings,
    pub recent: Vec<Recent>,
}

pub fn load(conn: &Connection) -> Saved {
    pref_get::<Saved>(conn, KEY).ok().flatten().unwrap_or_default()
}

fn store(conn: &Connection, s: &Saved) -> Result<(), String> {
    pref_set(conn, KEY, s)
}

/// Adds or changes the recent entry for `folder` (newest first, 10 kept).
pub fn set_recent(conn: &Connection, folder: &str, f: impl FnOnce(&mut Recent)) -> Result<(), String> {
    let mut s = load(conn);
    let mut r = match s.recent.iter().position(|r| r.folder == folder) {
        Some(i) => s.recent.remove(i),
        None => Recent { folder: folder.to_string(), ..Default::default() },
    };
    f(&mut r);
    s.recent.insert(0, r);
    s.recent.truncate(RECENT);
    store(conn, &s)
}

// ---------- the one at work ----------

/// What is waiting.
#[derive(Debug, Clone, PartialEq)]
pub enum Waiting {
    New(Vec<PathBuf>),
    /// 继续: the document's folder.
    Resume(PathBuf),
}

impl Waiting {
    fn name(&self) -> String {
        match self {
            Waiting::New(paths) => source::name_of(paths),
            Waiting::Resume(folder) => folder.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string(),
        }
    }
}

/// The page's progress line for the document at work.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub name: String,
    pub folder: String,
    /// 上传到 Mathpix / Mathpix 识别中 3/12 页 / Supervisor 审定切块 / 整理 …
    pub step: String,
    pub chunks: usize,
    pub done: usize,
    pub worker_requests: u32,
    pub supervisor_requests: u32,
    /// Seconds, earlier runs included.
    pub elapsed: u64,
    /// Which models (and a running local model used instead of the chosen one).
    pub models: Option<String>,
    #[serde(skip)]
    pub before: u64,
    #[serde(skip)]
    pub started: Option<Instant>,
}

#[derive(Default)]
struct Inner {
    queue: VecDeque<Waiting>,
    job: Option<Progress>,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    token: u64,
}

#[derive(Default)]
pub struct Live {
    inner: Mutex<Inner>,
}

impl Live {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Changes the progress of run `token` (if it's still the one at work).
    pub fn update(&self, token: u64, f: impl FnOnce(&mut Progress)) {
        let mut g = self.lock();
        if g.token == token {
            if let Some(p) = g.job.as_mut() {
                f(p);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub settings: Settings,
    pub recent: Vec<Recent>,
    pub queue: Vec<String>,
    pub current: Option<Progress>,
    /// The Mathpix keys are saved.
    pub mathpix: bool,
    /// Mock: no Mathpix keys needed.
    pub mock: bool,
    /// The output folder in use.
    pub folder: String,
    pub styles: Vec<(&'static str, &'static str)>,
    pub domains: Vec<(&'static str, &'static str)>,
    /// 联网查术语 when not set: the Supervisor's service has its own search.
    pub web_default: bool,
    /// A Tavily key is saved (for a Supervisor without its own search).
    pub tavily: bool,
}

fn view(st: &AppState) -> Result<View, String> {
    // what needs 小拾's database or the keychain first, then the queue's lock alone
    let saved = st.picky.with_db(|c| Ok(load(c)))?;
    let keys = &st.picky.keys;
    let web_default = search::service_of(&st.picky, saved.settings.supervisor.as_deref()).is_some_and(search::native);
    let (tavily, mathpix_saved) = (keys.has(crate::picky::skills::web::ACCOUNT), keys.has(mathpix::ID_ACCOUNT) && keys.has(mathpix::KEY_ACCOUNT));
    let (current, queue) = {
        let g = st.picky.assistants.pub2md.lock();
        (g.job.clone(), g.queue.iter().map(Waiting::name).collect::<Vec<_>>())
    };
    let current = current.map(|mut p| {
        p.elapsed = p.before + p.started.map_or(0, |s| s.elapsed().as_secs());
        p
    });
    let mut recent = saved.recent;
    // a run that didn't end (the app quit) reads as stopped (not the one being set up)
    let preparing = current.as_ref().is_some_and(|c| c.folder.is_empty());
    for r in recent.iter_mut().filter(|r| r.state == "running" && !preparing && current.as_ref().is_none_or(|c| c.folder != r.folder)) {
        r.state = "stopped".into();
        r.message.get_or_insert_with(|| "中途退出了 App".into());
    }
    Ok(View {
        styles: STYLES.iter().map(|x| (x.0, x.1)).collect(),
        domains: DOMAINS.iter().map(|x| (x.0, x.1)).collect(),
        web_default,
        tavily,
        folder: out_dir(&saved.settings).display().to_string(),
        settings: saved.settings,
        recent,
        queue,
        current,
        mathpix: mathpix_saved,
        mock: st.picky.mode == crate::picky::Mode::Mock,
    })
}

/// Starts the next waiting document if none is at work.
fn next(app: &AppHandle) {
    let st = app.state::<AppState>();
    let (token, what) = {
        let mut g = st.picky.assistants.pub2md.lock();
        if g.job.is_some() {
            return;
        }
        let Some(what) = g.queue.pop_front() else { return };
        g.token += 1;
        g.job = Some(Progress { name: what.name(), folder: String::new(), step: "正在准备…".into(), chunks: 0, done: 0, worker_requests: 0, supervisor_requests: 0, elapsed: 0, models: None, before: 0, started: Some(Instant::now()) });
        (g.token, what)
    };
    let app2 = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        job::run(&app2, token, what).await;
        {
            let st = app2.state::<AppState>();
            let mut g = st.picky.assistants.pub2md.lock();
            if g.token == token {
                g.job = None;
                g.task = None;
            }
        }
        next(&app2);
    });
    let mut g = st.picky.assistants.pub2md.lock();
    if g.token == token && g.job.is_some() {
        g.task = Some(task);
    }
}

// ---------- commands ----------

// The page's own reads and the settings run off the main thread (async): if one ever waits, the page
// waits, not the whole app.

#[tauri::command]
pub async fn picky_pub2md(app: AppHandle) -> Result<View, String> {
    view(&app.state::<AppState>())
}

#[tauri::command]
pub async fn picky_pub2md_settings(app: AppHandle, settings: Settings) -> Result<View, String> {
    let state = app.state::<AppState>();
    let settings = checked(settings)?;
    state.picky.with_db(|c| {
        let mut s = load(c);
        s.settings = settings;
        store(c, &s)
    })?;
    view(&state)
}

fn keys_ready(st: &AppState) -> Result<(), String> {
    let k = &st.picky.keys;
    if st.picky.mode == crate::picky::Mode::Real && !(k.has(mathpix::ID_ACCOUNT) && k.has(mathpix::KEY_ACCOUNT)) {
        return Err("先填上 Mathpix 的 App ID 和 App Key".into());
    }
    Ok(())
}

/// A document as the page shows it before 发送: its name and files.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Staged {
    pub name: String,
    pub files: Vec<String>,
}

/// Files picked or dropped, as the documents they will be (images together are one); nothing starts.
#[tauri::command]
pub async fn picky_pub2md_group(paths: Vec<String>) -> Result<Vec<Staged>, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    Ok(source::group(&paths)?.into_iter().map(|d| Staged { name: source::name_of(&d), files: d.iter().map(|p| p.display().to_string()).collect() }).collect())
}

/// 发送: each document waits its turn (images together are one document).
#[tauri::command]
pub fn picky_pub2md_add(app: AppHandle, paths: Vec<String>) -> Result<View, String> {
    let st = app.state::<AppState>();
    keys_ready(&st)?;
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let docs = source::group(&paths)?;
    if docs.is_empty() {
        return Err("没有能处理的文件".into());
    }
    st.picky.assistants.pub2md.lock().queue.extend(docs.into_iter().map(Waiting::New));
    next(&app);
    view(&st)
}

/// 继续 a stopped or failed document (from where it stopped).
#[tauri::command]
pub fn picky_pub2md_resume(app: AppHandle, folder: String) -> Result<View, String> {
    let st = app.state::<AppState>();
    keys_ready(&st)?;
    let known = st.picky.with_db(|c| Ok(load(c).recent.iter().any(|r| r.folder == folder)))?;
    let dir = PathBuf::from(&folder);
    if !known || !job::work_dir(&dir).join(job::JOB_FILE).is_file() {
        return Err("找不到这份文档的中间结果，没法接着做".into());
    }
    {
        let mut g = st.picky.assistants.pub2md.lock();
        if g.job.as_ref().is_some_and(|j| j.folder == folder) || g.queue.contains(&Waiting::Resume(dir.clone())) {
            return view(&st);
        }
        g.queue.push_front(Waiting::Resume(dir));
    }
    next(&app);
    view(&st)
}

#[tauri::command]
pub fn picky_pub2md_unqueue(state: State<AppState>, index: usize) -> Result<View, String> {
    {
        let mut g = state.picky.assistants.pub2md.lock();
        if index < g.queue.len() {
            g.queue.remove(index);
        }
    }
    view(&state)
}

/// Stops the document at work (what is done stays; 继续 goes on from there). The next one starts.
#[tauri::command]
pub fn picky_pub2md_stop(app: AppHandle) -> Result<View, String> {
    let st = app.state::<AppState>();
    let stopped = {
        let mut g = st.picky.assistants.pub2md.lock();
        if let Some(t) = g.task.take() {
            t.abort();
        }
        g.token += 1;
        g.job.take()
    };
    if let Some(p) = stopped.filter(|p| !p.folder.is_empty()) {
        let elapsed = p.before + p.started.map_or(0, |s| s.elapsed().as_secs());
        job::stopped(&app, Path::new(&p.folder), elapsed);
    }
    next(&app);
    view(&st)
}

/// 删除 a stopped or failed document: out of the recent list and the queue, its folder (results so far and
/// what 继续 needed) to the Trash. Only a folder Pub2MD made (it holds its work folder or its log).
#[tauri::command]
pub async fn picky_pub2md_delete(app: AppHandle, folder: String) -> Result<View, String> {
    delete(&app.state::<AppState>(), &folder)
}

fn delete(state: &AppState, folder: &str) -> Result<View, String> {
    let known = state.picky.with_db(|c| Ok(load(c).recent.into_iter().find(|r| r.folder == folder)))?;
    let Some(r) = known else { return Err("找不到这份文档".into()) };
    if r.state == "done" {
        return Err("做完的文档不在这里删".into());
    }
    let dir = PathBuf::from(folder);
    {
        let mut g = state.picky.assistants.pub2md.lock();
        if g.job.as_ref().is_some_and(|j| j.folder == folder) {
            return Err("这份文档正在处理，先停止".into());
        }
        g.queue.retain(|w| *w != Waiting::Resume(dir.clone()));
    }
    if dir.is_dir() {
        if !(job::work_dir(&dir).is_dir() || dir.join(job::LOG).is_file()) {
            return Err("这个文件夹不像 Pub2MD 建的，没有动它".into());
        }
        to_trash(&dir)?;
    }
    state.picky.with_db(|c| {
        let mut s = load(c);
        s.recent.retain(|r| r.folder != folder);
        store(c, &s)
    })?;
    view(state)
}

/// Moves a file or folder to the Trash (Finder can put it back). Tests delete it instead.
#[cfg(not(test))]
fn to_trash(path: &Path) -> Result<(), String> {
    move_to_trash(path)
}

fn move_to_trash(path: &Path) -> Result<(), String> {
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::msg_send;
    autoreleasepool(|_| unsafe {
        let fm_class = AnyClass::get(c"NSFileManager").ok_or("系统的 Foundation 不可用")?;
        let url_class = AnyClass::get(c"NSURL").ok_or("系统的 Foundation 不可用")?;
        let fm: Retained<AnyObject> = msg_send![fm_class, defaultManager];
        let p = objc2_foundation::NSString::from_str(&path.to_string_lossy());
        let url: Retained<AnyObject> = msg_send![url_class, fileURLWithPath: &*p];
        let none: *mut *mut AnyObject = std::ptr::null_mut();
        let ok: bool = msg_send![&*fm, trashItemAtURL: &*url, resultingItemURL: none, error: none];
        if ok { Ok(()) } else { Err("没能移到废纸篓".into()) }
    })
}

#[cfg(test)]
fn to_trash(path: &Path) -> Result<(), String> {
    std::fs::remove_dir_all(path).map_err(|e| e.to_string())
}

/// Opens a document's folder or one of its files (only folders in the recent list).
#[tauri::command]
pub fn picky_pub2md_open(app: AppHandle, folder: String, file: Option<String>) -> Result<(), String> {
    let st = app.state::<AppState>();
    let known = st.picky.with_db(|c| Ok(load(c).recent.iter().any(|r| r.folder == folder)))?;
    if !known {
        return Err("找不到这份文档".into());
    }
    let dir = PathBuf::from(&folder);
    let target = match file.as_deref() {
        Some(f) if !f.contains('/') && !f.starts_with('.') => dir.join(f),
        Some(_) => return Err("打不开这个文件".into()),
        None => dir,
    };
    if !target.exists() {
        return Err("文件已经不在了".into());
    }
    app.opener().open_path(target.display().to_string(), None::<&str>).map_err(|e| format!("打不开：{e}"))
}

/// An output to show on the page: its text, its images inlined (only .mmd / .md files of a recent document).
#[tauri::command]
pub async fn picky_pub2md_preview(app: AppHandle, folder: String, file: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    let known = state.picky.with_db(|c| Ok(load(c).recent.into_iter().find(|r| r.folder == folder)))?;
    let Some(r) = known else { return Err("找不到这份文档".into()) };
    if !r.outputs.contains(&file) || file.contains('/') || !(file.ends_with(".mmd") || file.ends_with(".md")) {
        return Err("这个文件不能预览".into());
    }
    let dir = PathBuf::from(&folder);
    let text = std::fs::read_to_string(dir.join(&file)).map_err(|_| "文件已经不在了".to_string())?;
    Ok(job::inline_images(&text, &dir))
}

/// What the two models are told with these settings (the page's 提示词).
#[tauri::command]
pub async fn picky_pub2md_prompts(app: AppHandle, settings: Settings) -> Result<Vec<(String, String)>, String> {
    let state = app.state::<AppState>();
    let settings = checked(settings)?;
    let web = settings.web.unwrap_or_else(|| search::service_of(&state.picky, settings.supervisor.as_deref()).is_some_and(search::native));
    Ok(job::prompts_shown(&settings, web))
}

#[tauri::command]
pub fn picky_pub2md_keys_save(state: State<AppState>, id: String, key: String) -> Result<View, String> {
    let k = &state.picky.keys;
    crate::picky::cloud::checked_key(&id).map_err(|_| "App ID 为空或格式不对".to_string())?;
    crate::picky::cloud::checked_key(&key).map_err(|_| "App Key 为空或格式不对".to_string())?;
    k.save(mathpix::ID_ACCOUNT, &id, "Pomelo 小拾 · Mathpix App ID")?;
    k.save(mathpix::KEY_ACCOUNT, &key, "Pomelo 小拾 · Mathpix App Key")?;
    view(&state)
}

#[tauri::command]
pub fn picky_pub2md_keys_remove(state: State<AppState>) -> Result<View, String> {
    state.picky.keys.remove(mathpix::ID_ACCOUNT)?;
    state.picky.keys.remove(mathpix::KEY_ACCOUNT)?;
    view(&state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::store::tests::memory_db;

    #[test]
    fn the_page_opens_with_the_supervisor_left_to_picky() {
        // it hung the app: the web default asked for 小拾's selection while holding its database lock
        let dir = crate::db::tests::temp_dir("pub2md-view");
        let st = std::sync::Arc::new(crate::AppState::init(crate::paths::DataPaths::new(dir.to_path_buf()), &dir, crate::picky::Mode::Mock));
        let (tx, rx) = std::sync::mpsc::channel();
        let st2 = st.clone();
        std::thread::spawn(move || tx.send(view(&st2).map(|v| (v.settings.supervisor.clone(), v.web_default))));
        let v = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("the page's view hung").unwrap();
        assert_eq!(v, (None, false));
        assert!(search::service_of(&st.picky, None).is_none() && search::service_of(&st.picky, Some("cloud:deepseek-flash")).is_some());
    }

    #[test]
    fn a_stopped_document_is_deleted_its_folder_too() {
        let dir = crate::db::tests::temp_dir("pub2md-delete");
        let st = crate::AppState::init(crate::paths::DataPaths::new(dir.to_path_buf()), &dir, crate::picky::Mode::Mock);
        let (stopped, done, other) = (dir.join("停下的"), dir.join("做完的"), dir.join("别的文件夹"));
        std::fs::create_dir_all(job::work_dir(&stopped)).unwrap();
        std::fs::create_dir_all(&done).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let key = |p: &Path| p.display().to_string();
        st.picky.with_db(|c| {
            set_recent(c, &key(&stopped), |r| r.state = "stopped".into())?;
            set_recent(c, &key(&done), |r| r.state = "done".into())?;
            set_recent(c, &key(&other), |r| r.state = "failed".into())
        })
        .unwrap();
        st.picky.assistants.pub2md.lock().queue.push_back(Waiting::Resume(stopped.clone()));
        let v = delete(&st, &key(&stopped)).unwrap();
        assert!(!stopped.exists() && v.recent.len() == 2 && v.queue.is_empty());
        assert!(delete(&st, &key(&done)).unwrap_err().contains("做完"));
        // a folder Pub2MD didn't make stays
        assert!(delete(&st, &key(&other)).unwrap_err().contains("不像") && other.exists());
        let staged = tauri::async_runtime::block_on(picky_pub2md_group(vec![])).unwrap();
        assert!(staged.is_empty());
    }

    /// The real Trash: one empty folder「Pomelo 废纸篓测试」goes there.
    ///   cargo test real_trash -- --ignored
    #[test]
    #[ignore]
    fn real_trash() {
        let dir = std::env::temp_dir().join("Pomelo 废纸篓测试");
        std::fs::create_dir_all(&dir).unwrap();
        move_to_trash(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn settings_and_recent() {
        let s = checked(Settings { parallel: 20, ..Default::default() }).unwrap();
        assert_eq!((s.parallel, s.supervisor_level, s.worker_level), (MAX_PARALLEL, Level::High, Level::Off));
        assert!(checked(Settings { worker: Some("x".into()), ..Default::default() }).is_err());
        assert!(checked(Settings { folder: Some("/no/such/folder".into()), ..Default::default() }).is_err());
        assert_eq!((Level::Medium.effort(), Level::Off.thinking()), (Some(Effort::Medium), false));
        let c = memory_db();
        for i in 0..12 {
            set_recent(&c, &format!("/f{i}"), |r| r.name = format!("文档 {i}")).unwrap();
        }
        set_recent(&c, "/f5", |r| r.state = "done".into()).unwrap();
        let r = load(&c).recent;
        assert_eq!((r.len(), r[0].folder.as_str(), r[0].state.as_str(), r[0].name.as_str()), (RECENT, "/f5", "done", "文档 5"));
        assert!(!r.iter().any(|x| x.folder == "/f0"));
    }
}
