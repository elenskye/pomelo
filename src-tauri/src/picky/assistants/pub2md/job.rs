//! One document through Pub2MD. Each step is saved in `<folder>/.pub2md/job.json` as it ends (and after
//! every chunk's step), so 继续 goes on from where a stop, an error or a quit left it:
//! 1. 提取: an earlier Mathpix result of the same files is used again (its 「Mathpix 原始结果.zip」 still
//!    there); else the file goes to Mathpix once (the pdf_id is saved at once), then wait and download.
//!    Written for the user: 原文.mmd (images pointed at assets/), assets/, Mathpix 原始结果.zip. A text file
//!    (MD / MMD / TXT) is taken as it is (copied into .pub2md) and only translated.
//! 2. 只提取 ends here. Else: the models (schedule.rs `route`), 原页图 (PDF pages, or the images packed into
//!    one PDF, as JPEGs; Word and text have none), the term list started beside the cut (英文 → 中文: the
//!    Supervisor, looking terms up per search.rs), the cut (draft → the Supervisor's review, chunk.rs).
//! 3. Each chunk goes 整理 → (精修) → 翻译 → (精修), `parallel` chunk steps at a time, the earliest chunk
//!    first; a chunk is translated once it is cleaned and the term list is there (简繁: OpenCC, then the
//!    Worker looks the draft over). Parts not translated are copied as they are; 前置信息 gets only its
//!    title translated. Each step: the Worker (with its pages' images in 整理, when it's a cloud model) →
//!    the program's checks; a failed request is asked once more; a failed check goes to the Supervisor
//!    (retry with a hint / split (整理 only) / think / do it itself / accept / stop), at most twice a chunk
//!    a step and six times a document; then it stops, keeping what's done. 精修: the Supervisor's fixes
//!    (find → replace, applied when the words are there exactly once).
//! 4. 通读: the Supervisor reads the whole translation (in windows when it doesn't fit) for what spans
//!    chunks; its fixes applied the same way. Then the results in order (an article a file when the
//!    Supervisor found several): 译文, 整理稿, 术语表, the work log (工作日志.md); `.pub2md` removed. A stop
//!    or a failure keeps `.pub2md` and writes the log so far.
//! 5. 另存为 (the formats picked): 译文 and 整理稿 (只提取: 原文) go to Mathpix's converter, their assets/
//!    images given back Mathpix's links (kept 30 days after 提取; images without one are left out and
//!    noted). Not fatal: a failure is noted in the log, the MMD results stay.
//! Steps 2–4 report progress through `Report` (the page's line; nothing in tests).

use super::chunk::{self, Article, Budget, Chunk, Plan};
use super::mmd::{self, Block, Part};
use super::prompts::{self, TranslateAsk, WorkerAsk};
use super::search::{self, Lookup};
use super::source::{self, Kind};
use super::{check, mathpix, script, set_recent, Format, Lang, Level, Settings, Steps, Waiting};
use crate::picky::provider::{self, Event, Image, Msg, Options, Target};
use crate::picky::{context, office, pdf, schedule, skills, Mode, Route};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::task::JoinSet;

pub const JOB_FILE: &str = "job.json";
const WORK: &str = ".pub2md";
pub const RAW_ZIP: &str = "Mathpix 原始结果.zip";
pub const ORIGINAL: &str = "原文.mmd";
pub const LOG: &str = "工作日志.md";
pub const TERMS_FILE: &str = "术语表.md";
const ASSETS: &str = "assets";
/// Tokens a chunk (less for a small window).
const TARGET: i64 = 2400;
const PAGE_SIDE: f64 = 1568.0;
const IMAGES_A_CHUNK: usize = 6;
const FIXES_PER_CHUNK: u32 = 2;
const FIXES_PER_DOC: u32 = 6;

/// The Supervisor's troubleshooting a document may have: 6, or one for every three chunks of a long one
/// (Pargent's 26 chunks used all 6).
fn fixes_cap(chunks: usize) -> u32 {
    FIXES_PER_DOC.max(chunks as u32 / 3)
}
const BEFORE_CHARS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    #[default]
    Pending,
    Done,
    /// The check didn't pass, the Supervisor accepted it.
    Accepted,
    /// Not translated (its part is kept as it is).
    Skipped,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChunkJob {
    pub chunk: Chunk,
    pub pages: (usize, usize),
    pub tokens: i64,
    /// 整理, and its 精修.
    pub state: State,
    pub reviewed: bool,
    /// 翻译, and its 精修.
    pub tr: State,
    pub tr_reviewed: bool,
    pub requests: u32,
    /// The Supervisor's troubleshooting, in 整理 and in 翻译.
    pub fixes: u32,
    pub tr_fixes: u32,
    pub seconds: u64,
    /// What happened (checks, the Supervisor's choices), for the log.
    pub notes: Vec<String>,
    pub by_supervisor: bool,
}

/// A line of the term list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Term {
    pub source: String,
    pub target: String,
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Job {
    pub name: String,
    pub sources: Vec<String>,
    pub kind: Option<Kind>,
    pub fingerprint: String,
    // what was asked for (kept for 继续; the models, levels and 同时几块 are the page's at each run)
    pub steps: Steps,
    pub md: bool,
    pub english_only: bool,
    pub lang: Lang,
    pub style: String,
    #[serde(alias = "domain", deserialize_with = "super::domains_of")]
    pub domains: Vec<String>,
    pub prompt: String,
    pub web: Option<bool>,
    pub refine: bool,
    pub skip: Vec<Part>,
    pub formats: Vec<Format>,
    /// "YYYY-MM-DD HH:MM"
    pub started: String,
    pub pdf_id: Option<String>,
    pub reused: bool,
    /// "YYYY-MM-DD" of Mathpix's result, and its images: (assets/ name, Mathpix's link).
    pub extracted_on: String,
    pub images: Vec<(String, String)>,
    /// 另存为 done, and what didn't go.
    pub converted: bool,
    pub convert_notes: Vec<String>,
    pub extracted: bool,
    pub pages: usize,
    pub rendered: usize,
    pub planned: bool,
    pub chunks: Vec<ChunkJob>,
    pub articles: Vec<Article>,
    /// What of the Supervisor's cut wasn't taken.
    pub plan_notes: Vec<String>,
    /// None until the Supervisor's term list is there (an empty list when it couldn't be made).
    pub terms: Option<Vec<Term>>,
    pub terms_note: String,
    pub read_through: bool,
    pub read_notes: Vec<String>,
    pub fixes: u32,
    pub worker_requests: u32,
    pub supervisor_requests: u32,
    /// Seconds of work so far (stops not counted).
    pub elapsed: u64,
    pub models: String,
    /// "HH:MM:SS what happened"
    pub events: Vec<String>,
}

pub fn work_dir(folder: &Path) -> PathBuf {
    folder.join(WORK)
}

fn save(folder: &Path, job: &Job) -> Result<(), String> {
    let dir = work_dir(folder);
    std::fs::create_dir_all(&dir).map_err(|e| format!("没能写中间结果：{e}"))?;
    let tmp = dir.join("job.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(job).map_err(|e| e.to_string())?).map_err(|e| format!("没能写中间结果：{e}"))?;
    std::fs::rename(&tmp, dir.join(JOB_FILE)).map_err(|e| format!("没能写中间结果：{e}"))
}

pub fn load_job(folder: &Path) -> Result<Job, String> {
    let data = std::fs::read(work_dir(folder).join(JOB_FILE)).map_err(|_| "找不到这份文档的中间结果".to_string())?;
    serde_json::from_slice(&data).map_err(|_| "这份文档的中间结果读不懂了".into())
}

/// Local time by SQLite's clock (`fmt`: strftime).
fn clock(fmt: &str) -> String {
    rusqlite::Connection::open_in_memory().and_then(|c| c.query_row("SELECT strftime(?1, 'now', 'localtime')", [fmt], |r| r.get(0))).unwrap_or_default()
}

fn event(job: &mut Job, what: impl Into<String>) {
    job.events.push(format!("{} {}", clock("%H:%M:%S"), what.into()));
}

/// A file or folder name from a title.
fn safe_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if "/:\\".contains(c) || c.is_control() { '-' } else { c }).collect();
    let n: String = n.trim().trim_start_matches('.').chars().take(80).collect();
    if n.trim().is_empty() { "文档".into() } else { n.trim().to_string() }
}

fn unique_dir(base: &Path, name: &str) -> PathBuf {
    let stem = format!("{} · {}", safe_name(name), clock("%Y-%m-%d"));
    let mut dir = base.join(&stem);
    let mut n = 2;
    while dir.exists() {
        dir = base.join(format!("{stem} ({n})"));
        n += 1;
    }
    dir
}

// ---------- progress ----------

pub enum Change {
    Step(String),
    Counts { chunks: usize, done: usize, worker: u32, supervisor: u32 },
    Models(String),
}

/// Where progress goes: the page's line for the document at work (nothing in tests).
#[derive(Clone)]
pub struct Report(Arc<dyn Fn(Change) + Send + Sync>);

impl Report {
    #[cfg(test)]
    pub fn none() -> Report {
        Report(Arc::new(|_| {}))
    }
    fn of(app: &AppHandle, token: u64) -> Report {
        let app = app.clone();
        Report(Arc::new(move |c| {
            app.state::<AppState>().picky.assistants.pub2md.update(token, |p| match c {
                Change::Step(s) => p.step = s,
                Change::Counts { chunks, done, worker, supervisor } => (p.chunks, p.done, p.worker_requests, p.supervisor_requests) = (chunks, done, worker, supervisor),
                Change::Models(m) => p.models = Some(m),
            })
        }))
    }
    fn step(&self, s: &str) {
        (self.0)(Change::Step(s.to_string()));
    }
    fn counts(&self, job: &Job) {
        let done = job.chunks.iter().filter(|c| finished(job, c)).count();
        (self.0)(Change::Counts { chunks: job.chunks.len(), done, worker: job.worker_requests, supervisor: job.supervisor_requests });
    }
}

// ---------- a run ----------

/// The document's folder and job (new, or the saved one for 继续).
fn prepare(st: &AppState, settings: &Settings, what: &Waiting) -> Result<(PathBuf, Job), String> {
    match what {
        Waiting::New(paths) => {
            let name = source::name_of(paths);
            let fingerprint = source::fingerprint(paths)?;
            let folder = unique_dir(&super::out_dir(settings), &name);
            std::fs::create_dir_all(work_dir(&folder)).map_err(|e| format!("没能在输出文件夹里建文件夹：{e}"))?;
            let kind = source::kind_of(paths);
            let mut job = Job {
                name: name.clone(),
                sources: paths.iter().map(|p| p.display().to_string()).collect(),
                kind: Some(kind),
                fingerprint: fingerprint.clone(),
                // a text file is only translated
                steps: if kind == Kind::Text { Steps::Translate } else { settings.steps },
                md: settings.md,
                english_only: settings.english_only,
                lang: settings.lang,
                style: settings.style.clone(),
                domains: settings.domains.clone(),
                prompt: settings.prompt.clone(),
                web: settings.web,
                refine: settings.refine,
                skip: settings.skip.clone(),
                formats: settings.formats.clone(),
                started: clock("%Y-%m-%d %H:%M"),
                ..Default::default()
            };
            event(&mut job, format!("开始：{}", paths.iter().map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("")).collect::<Vec<_>>().join("、")));
            save(&folder, &job)?;
            let at = job.started.clone();
            st.picky.with_db(|c| set_recent(c, &folder.display().to_string(), |r| (r.name, r.at, r.state, r.message, r.fingerprint) = (name, at, "running".into(), None, fingerprint)))?;
            Ok((folder, job))
        }
        Waiting::Resume(folder) => {
            let mut job = load_job(folder)?;
            event(&mut job, "继续");
            save(folder, &job)?;
            st.picky.with_db(|c| set_recent(c, &folder.display().to_string(), |r| (r.state, r.message) = ("running".into(), None)))?;
            Ok((folder.clone(), job))
        }
    }
}

pub async fn run(app: &AppHandle, token: u64, what: Waiting) {
    let st = app.state::<AppState>();
    let report = Report::of(app, token);
    let settings = st.picky.with_db(|c| Ok(super::load(c).settings)).unwrap_or_default();
    let (folder, mut job) = match prepare(&st, &settings, &what) {
        Ok(x) => x,
        Err(e) => {
            report.step(&format!("没能开始：{e}"));
            tokio::time::sleep(Duration::from_secs(4)).await;
            return;
        }
    };
    let (name, before) = (job.name.clone(), job.elapsed);
    st.picky.assistants.pub2md.update(token, |p| (p.folder, p.name, p.before) = (folder.display().to_string(), name, before));
    report.counts(&job);
    let started = Instant::now();
    let result = steps(app, &report, &folder, &mut job, &settings).await;
    job.elapsed += started.elapsed().as_secs();
    finish(&st, &folder, &mut job, result);
}

/// After 停止: the saved job gets the time and a line, the log what's done.
pub fn stopped(app: &AppHandle, folder: &Path, elapsed: u64) {
    let st = app.state::<AppState>();
    if let Ok(mut job) = load_job(folder) {
        job.elapsed = elapsed.max(job.elapsed);
        event(&mut job, "停止");
        let _ = save(folder, &job);
        let _ = std::fs::write(folder.join(LOG), log(&job, "停下了，点「继续」从停下的地方接着做"));
    }
    let _ = st.picky.with_db(|c| set_recent(c, &folder.display().to_string(), |r| (r.state, r.message) = ("stopped".into(), Some("已停止".into()))));
}

fn finish(st: &AppState, folder: &Path, job: &mut Job, result: Result<Vec<String>, String>) {
    let key = folder.display().to_string();
    match result {
        Ok(mut outputs) => {
            event(job, "完成");
            let _ = std::fs::write(folder.join(LOG), log(job, "完成"));
            outputs.push(LOG.into());
            let _ = std::fs::remove_dir_all(work_dir(folder));
            let _ = st.picky.with_db(|c| set_recent(c, &key, |r| (r.state, r.message, r.outputs) = ("done".into(), None, outputs)));
        }
        Err(e) => {
            event(job, format!("出错停下：{e}"));
            let _ = save(folder, job);
            let _ = std::fs::write(folder.join(LOG), log(job, &format!("出错停下：{e}。点「继续」从停下的地方接着做")));
            let _ = st.picky.with_db(|c| set_recent(c, &key, |r| (r.state, r.message) = ("failed".into(), Some(e))));
        }
    }
}

async fn steps(app: &AppHandle, report: &Report, folder: &Path, job: &mut Job, settings: &Settings) -> Result<Vec<String>, String> {
    let st = app.state::<AppState>();
    let text_input = job.kind == Some(Kind::Text);
    if !job.extracted {
        if text_input {
            take_text(folder, job)?;
        } else {
            let mock = st.picky.mode == Mode::Mock;
            let keys = if mock { None } else { Some(mathpix::Keys { id: st.picky.keys.read(mathpix::ID_ACCOUNT)?, key: st.picky.keys.read(mathpix::KEY_ACCOUNT)? }) };
            let earlier = earlier(&st, &job.fingerprint, folder);
            extract(report, folder, job, Extraction { keys, earlier }).await?;
            let key = folder.display().to_string();
            st.picky.with_db(|c| set_recent(c, &key, |r| (r.extracted, r.images) = (job.extracted_on.clone(), job.images.clone())))?;
        }
    }
    let mut outputs = vec![];
    if !text_input {
        outputs.push(ORIGINAL.to_string());
        if folder.join(ASSETS).is_dir() {
            outputs.push(ASSETS.into());
        }
        outputs.push(RAW_ZIP.into());
    }
    if job.steps == Steps::Extract {
        outputs.extend(another_format(&st, report, folder, job, &[ORIGINAL.to_string()]).await);
        return Ok(outputs);
    }
    report.step("准备模型…");
    let (worker, wn, _) = schedule::route(&st.picky, settings.worker.as_deref()).await?;
    let (sup, sn, _) = schedule::route(&st.picky, settings.supervisor.as_deref()).await?;
    // the backup: a cloud model with its key (none when it's gone or has no key)
    let backup = settings.backup.strip_prefix("cloud:").and_then(|id| st.picky.route_with(&crate::picky::Selection::Cloud(id.into()), true).ok());
    let spare = backup.as_ref().map_or(String::new(), |b| format!("　备用：{}", b.name));
    job.models = format!("Worker：{}（{}）　Supervisor：{}（{}）{spare}{wn}{sn}", worker.name, settings.worker_level.name(), sup.name, settings.supervisor_level.name());
    (report.0)(Change::Models(job.models.clone()));
    if job.rendered == 0 && !matches!(job.kind, Some(Kind::Word) | Some(Kind::Text)) {
        report.step("画原页图…");
        match render(folder, job).await {
            Ok(n) => {
                job.rendered = n;
                event(job, format!("画了 {n} 页原页图"));
            }
            Err(e) => event(job, format!("没有原页图：{e}")),
        }
        save(folder, job)?;
    }
    let text = std::fs::read_to_string(original(folder, job)).map_err(|_| "找不到原文".to_string())?;
    let (blocks, _) = mmd::blocks(&text);
    if blocks.is_empty() {
        return Err("没有识别出文字".into());
    }
    let blocks = Arc::new(blocks);
    let models = Models { worker, sup, wl: settings.worker_level, sl: settings.supervisor_level, parallel: settings.parallel as usize, backup };
    // the term list starts now, beside the cut and 整理
    let mut set = JoinSet::new();
    if needs_terms(job) && job.terms.is_none() {
        let tavily = st.picky.keys.read(skills::web::ACCOUNT).ok();
        let lookup = search::lookup(job.web, &models.sup, tavily);
        set.spawn(terms_task(job, &text, &models, lookup));
        job.supervisor_requests += 1;
        event(job, "开始定术语表");
    }
    if !job.planned {
        plan(report, folder, job, &blocks, &models).await?;
    }
    work(report, folder, job, &blocks, &models, set).await?;
    if job.steps == Steps::Translate && !job.read_through {
        read_through(report, folder, job, &models).await?;
    }
    let mut written = assemble(folder, job)?;
    let results: Vec<String> = written.iter().filter(|f| *f != TERMS_FILE && (f.ends_with(".mmd") || f.ends_with(".md"))).cloned().collect();
    written.extend(another_format(&st, report, folder, job, &results).await);
    written.extend(outputs);
    Ok(written)
}

/// 另存为, with the keys (none needed in the mock).
async fn another_format(st: &AppState, report: &Report, folder: &Path, job: &mut Job, files: &[String]) -> Vec<String> {
    if job.formats.is_empty() || job.converted {
        return vec![];
    }
    let keys = if st.picky.mode == Mode::Mock {
        None
    } else {
        match (st.picky.keys.read(mathpix::ID_ACCOUNT), st.picky.keys.read(mathpix::KEY_ACCOUNT)) {
            (Ok(id), Ok(key)) => Some(mathpix::Keys { id, key }),
            _ => {
                job.convert_notes.push("没有 Mathpix 的 Key，没能另存".into());
                job.converted = true;
                return vec![];
            }
        }
    };
    let made = convert(report, folder, job, keys.as_ref(), files).await;
    let _ = save(folder, job);
    made
}

/// Where the document's text is: 原文.mmd, or a text file's copy.
fn original(folder: &Path, job: &Job) -> PathBuf {
    if job.kind == Some(Kind::Text) { work_dir(folder).join("source.md") } else { folder.join(ORIGINAL) }
}

/// A text file, taken as it is.
pub fn take_text(folder: &Path, job: &mut Job) -> Result<(), String> {
    let src = job.sources.first().ok_or("没有文件")?;
    let text = std::fs::read_to_string(src).map_err(|_| "读不了这个文本文件（要 UTF-8 的 MD、MMD 或 TXT）".to_string())?;
    if text.trim().is_empty() {
        return Err("这个文件是空的".into());
    }
    std::fs::write(work_dir(folder).join("source.md"), &text).map_err(|e| format!("没能写中间结果：{e}"))?;
    job.pages = 1;
    job.extracted = true;
    event(job, "读了文本文件");
    save(folder, job)
}

// ---------- 1. 提取 ----------

/// An earlier result of the same files (its zip still there).
fn earlier(st: &AppState, fingerprint: &str, folder: &Path) -> Option<Earlier> {
    let recent = st.picky.with_db(|c| Ok(super::load(c).recent)).ok()?;
    let me = folder.display().to_string();
    recent.into_iter().filter(|r| r.fingerprint == fingerprint && r.folder != me).map(|r| Earlier { zip: Path::new(&r.folder).join(RAW_ZIP), extracted: r.extracted, images: r.images }).find(|e| e.zip.is_file())
}

/// An earlier Mathpix result: its zip, when it was made and its images' links.
pub struct Earlier {
    pub zip: PathBuf,
    pub extracted: String,
    pub images: Vec<(String, String)>,
}

/// How to extract: Mathpix with these keys (None = the mock), an earlier result of the same files.
pub struct Extraction {
    pub keys: Option<mathpix::Keys>,
    pub earlier: Option<Earlier>,
}

pub async fn extract(report: &Report, folder: &Path, job: &mut Job, how: Extraction) -> Result<(), String> {
    let mdir = work_dir(folder).join("mathpix");
    std::fs::create_dir_all(&mdir).map_err(|e| format!("没能写中间结果：{e}"))?;
    let zip_path = mdir.join("extraction.zip");
    let paths: Vec<PathBuf> = job.sources.iter().map(PathBuf::from).collect();
    let kind = job.kind.unwrap_or(Kind::Pdf);
    if !zip_path.is_file() && job.pdf_id.is_none() && !job.reused {
        if let Some(prev) = how.earlier {
            std::fs::copy(&prev.zip, &zip_path).map_err(|e| format!("没能用上次的 Mathpix 结果：{e}"))?;
            job.reused = true;
            (job.extracted_on, job.images) = (prev.extracted, prev.images);
            event(job, format!("文件和上次的一样，用了上次的 Mathpix 结果（{}）", prev.zip.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("")));
        }
    }
    if !zip_path.is_file() {
        if paths.iter().any(|p| !p.is_file()) {
            return Err("找不到原来的文件了".into());
        }
        report.step("准备上传…");
        let (k, p) = (kind, paths.clone());
        let (name, mime, data) = tauri::async_runtime::spawn_blocking(move || source::upload(k, &p)).await.map_err(|e| e.to_string())??;
        if kind == Kind::Images {
            std::fs::write(work_dir(folder).join("input.pdf"), &data).map_err(|e| format!("没能写中间结果：{e}"))?;
        }
        let text = match &how.keys {
            None => mock_extract(report, folder, kind, &paths, &data).await?,
            Some(keys) => mathpix_extract(keys, report, folder, job, &name, mime, &data).await?,
        };
        std::fs::write(mdir.join("extraction.mmd"), &text).map_err(|e| format!("没能写中间结果：{e}"))?;
        job.extracted_on = clock("%Y-%m-%d");
    }
    let zip = std::fs::read(&zip_path).map_err(|_| "Mathpix 的结果不见了".to_string())?;
    let downloaded = std::fs::read_to_string(mdir.join("extraction.mmd")).ok();
    let (text, images, links) = unpack(&zip, downloaded)?;
    if !links.is_empty() {
        job.images = links;
    }
    if !images.is_empty() {
        let assets = folder.join(ASSETS);
        std::fs::create_dir_all(&assets).map_err(|e| format!("没能写附件：{e}"))?;
        for (name, data) in &images {
            std::fs::write(assets.join(name), data).map_err(|e| format!("没能写附件：{e}"))?;
        }
    }
    std::fs::write(folder.join(ORIGINAL), &text).map_err(|e| format!("没能写 原文.mmd：{e}"))?;
    std::fs::copy(&zip_path, folder.join(RAW_ZIP)).map_err(|e| format!("没能写 Mathpix 原始结果：{e}"))?;
    job.pages = mmd::blocks(&text).1;
    job.extracted = true;
    event(job, format!("提取完成：{} 页，{} 张图", job.pages, images.len()));
    save(folder, job)
}

async fn mock_extract(report: &Report, folder: &Path, kind: Kind, paths: &[PathBuf], data: &[u8]) -> Result<String, String> {
    let pages = if kind == Kind::Word { 1 } else { pdf::pages(data).map(|p| p.len()).unwrap_or(1) };
    for i in 1..=pages.min(3) {
        report.step(&format!("Mathpix 识别中（模拟）{i}/{pages} 页"));
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    let text = mathpix::mock_mmd(kind, paths, (kind != Kind::Word).then_some(data))?;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file("extraction/extraction.mmd", zip::write::SimpleFileOptions::default()).map_err(|e| e.to_string())?;
    w.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    let zip = w.finish().map_err(|e| e.to_string())?.into_inner();
    std::fs::write(work_dir(folder).join("mathpix/extraction.zip"), zip).map_err(|e| format!("没能写中间结果：{e}"))?;
    Ok(text)
}

async fn mathpix_extract(keys: &mathpix::Keys, report: &Report, folder: &Path, job: &mut Job, name: &str, mime: &str, data: &[u8]) -> Result<String, String> {
    let id = match job.pdf_id.clone() {
        Some(id) => id,
        None => {
            report.step("上传到 Mathpix…");
            let id = mathpix::submit(keys, name, mime, data).await?;
            job.pdf_id = Some(id.clone());
            event(job, "上传到了 Mathpix");
            save(folder, job)?;
            id
        }
    };
    let deadline = Instant::now() + Duration::from_secs(3600);
    loop {
        let s = mathpix::status(keys, &id).await?;
        let pages = match (s.done, s.pages) {
            (Some(d), Some(n)) => format!(" {d}/{n} 页"),
            _ => String::new(),
        };
        report.step(&format!("Mathpix 识别中{pages}"));
        if s.state == "completed" {
            break;
        }
        if Instant::now() > deadline {
            return Err("Mathpix 一个小时还没识别完，过一会儿点「继续」再看".into());
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    report.step("从 Mathpix 下载结果…");
    let text = String::from_utf8(download(keys, &id, "mmd").await?).map_err(|_| "Mathpix 的结果读不懂".to_string())?;
    let zip = download(keys, &id, "mmd.zip").await?;
    std::fs::write(work_dir(folder).join("mathpix/extraction.zip"), &zip).map_err(|e| format!("没能写中间结果：{e}"))?;
    event(job, "从 Mathpix 下载了结果");
    Ok(text)
}

/// A result, asked again while it isn't assembled yet.
async fn download(keys: &mathpix::Keys, id: &str, ext: &str) -> Result<Vec<u8>, String> {
    for _ in 0..100 {
        if let Some(b) = mathpix::download(keys, id, ext).await? {
            return Ok(b);
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    Err("Mathpix 的结果一直没准备好，过一会儿点「继续」再试".into())
}

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "tif", "tiff"];

/// The text (`downloaded`, else the zip's) with its images pointed at assets/, the images (name, bytes) and
/// Mathpix's links of those that had one (name, link).
pub fn unpack(zip: &[u8], downloaded: Option<String>) -> Result<(String, Vec<(String, Vec<u8>)>, Vec<(String, String)>), String> {
    let bad = || "Mathpix 的结果（zip）打不开".to_string();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|_| bad())?;
    let mut images: Vec<(String, String, Vec<u8>)> = vec![];
    let mut texts: Vec<(String, String)> = vec![];
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|_| bad())?;
        if f.is_dir() {
            continue;
        }
        total += f.size();
        if total > 2_000_000_000 {
            return Err("Mathpix 的结果太大了".into());
        }
        let path = f.name().map_err(|_| bad())?.to_string();
        let base = path.rsplit('/').next().unwrap_or("").to_string();
        let ext = base.rsplit('.').next().unwrap_or("").to_lowercase();
        let mut data = vec![];
        f.read_to_end(&mut data).map_err(|_| bad())?;
        if IMAGE_EXTS.contains(&ext.as_str()) && !base.starts_with('.') {
            let mut name = base.clone();
            let mut n = 2;
            while images.iter().any(|(_, x, _)| *x == name) {
                name = format!("{n}-{base}");
                n += 1;
            }
            images.push((path, name, data));
        } else if ext == "mmd" || ext == "md" {
            texts.push((ext, String::from_utf8_lossy(&data).into_owned()));
        }
    }
    texts.sort_by_key(|(e, _)| e != "mmd");
    let text = downloaded.filter(|t| !t.trim().is_empty()).or_else(|| texts.into_iter().next().map(|(_, t)| t)).ok_or("Mathpix 的结果里没有文字")?;
    let (text, links) = relink(&text, &images);
    Ok((text, images.into_iter().map(|(_, n, d)| (n, d)).collect(), links))
}

/// Image links (Mathpix's CDN, paths inside the zip) pointed at assets/ when the image is there; the CDN
/// links taken (assets/ name, link).
fn relink(text: &str, images: &[(String, String, Vec<u8>)]) -> (String, Vec<(String, String)>) {
    let mut out = String::with_capacity(text.len());
    let mut links: Vec<(String, String)> = vec![];
    let mut rest = text;
    while let Some(i) = rest.find("https://cdn.mathpix.com/") {
        out.push_str(&rest[..i]);
        let end = rest[i..].find(|c: char| c.is_whitespace() || ")}\"'<>]".contains(c)).map_or(rest.len(), |e| i + e);
        let url = &rest[i..end];
        let base = url.split('?').next().unwrap_or(url).rsplit('/').next().unwrap_or("");
        let cropped = cropped_name(url);
        match images.iter().find(|(p, _, _)| p.rsplit('/').next().is_some_and(|b| b == base || Some(b) == cropped.as_deref())) {
            Some((_, name, _)) => {
                out.push_str(&format!("{ASSETS}/{name}"));
                if !links.iter().any(|(n, _)| n == name) {
                    links.push((name.clone(), url.to_string()));
                }
            }
            None => out.push_str(url),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    for (path, name, _) in images {
        let inner = path.split_once('/').map_or(path.as_str(), |(_, r)| r);
        // the zip's own text says ./images/<name>
        for p in [path.to_string(), inner.to_string(), format!("./{path}"), format!("./{inner}")] {
            if p != format!("{ASSETS}/{name}") {
                out = out.replace(&format!("({p})"), &format!("({ASSETS}/{name})")).replace(&format!("{{{p}}}"), &format!("{{{ASSETS}/{name}}}"));
            }
        }
    }
    (out, links)
}

/// The zip's name for a cropped image: the link `<id>-1.jpg?height=396&width=423&top_left_y=172&top_left_x=56`
/// is `images/<id>-1_396_423_172_56.jpg` there.
fn cropped_name(url: &str) -> Option<String> {
    let (path, query) = url.split_once('?')?;
    let (stem, ext) = path.rsplit('/').next()?.rsplit_once('.')?;
    let q = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(k)?.strip_prefix('='));
    Some(format!("{stem}_{}_{}_{}_{}.{ext}", q("height")?, q("width")?, q("top_left_y")?, q("top_left_x")?))
}

// ---------- 2. 原页图, the cut ----------

pub async fn render(folder: &Path, job: &Job) -> Result<usize, String> {
    let data = match job.kind {
        Some(Kind::Images) => {
            let packed = work_dir(folder).join("input.pdf");
            match std::fs::read(&packed) {
                Ok(d) => d,
                Err(_) => {
                    let paths: Vec<PathBuf> = job.sources.iter().map(PathBuf::from).collect();
                    tauri::async_runtime::spawn_blocking(move || pdf::images_to_pdf(&paths)).await.map_err(|e| e.to_string())??
                }
            }
        }
        _ => std::fs::read(job.sources.first().ok_or("没有原文件")?).map_err(|_| "找不到原来的 PDF".to_string())?,
    };
    let dir = work_dir(folder).join("pages");
    tauri::async_runtime::spawn_blocking(move || pdf::render_pages(&data, &dir, PAGE_SIDE)).await.map_err(|e| e.to_string())?
}

/// One request; the reply and whether it hit its length.
/// A reply's room plus its thinking. A model told to think 中 or 高 (the Supervisor's default) may think
/// far longer than chat's room before it answers, and a cut-off JSON answer is lost; the service caps
/// it at the model's own limit, and only what's used is paid.
fn reply_room(route: &Route, need: i64, level: Level) -> i64 {
    let think = match level {
        // a model that always thinks (GLM-5.3) thinks at 关 too
        Level::Off if route.think == crate::picky::cloud::Think::Always => 8_192,
        Level::Off => 0,
        Level::Low => 8_192,
        Level::Medium => 16_384,
        Level::High => 32_768,
    };
    if route.think.thinks(level.thinking()) { need + think } else { need }
}

async fn ask(route: &Route, level: Level, system: String, user: String, images: Vec<Image>, max: i64) -> Result<(String, bool), String> {
    let mut msg = Msg::new("user", &user);
    msg.images = images;
    let (mut out, mut cut) = (String::new(), false);
    // each chunk's request is its own: no cache to write
    let opts = Options { max_tokens: Some(max), thinking: Some(level.thinking()), effort: level.effort(), one_off: true, ..Default::default() };
    provider::stream(route.target.clone(), vec![Msg::new("system", &system), msg], opts, |e| match e {
        Event::Content(t) => out.push_str(&t),
        Event::Finished(r) => cut = r == "length",
        #[cfg(test)]
        Event::Usage { prompt, completion } => tests::used(&route.name, prompt, completion),
        _ => {}
    })
    .await?;
    Ok((out, cut))
}

/// A Supervisor request. When Alibaba's content check turns it down (Qwen through OpenRouter or Aliyun, on
/// news of wars, history with racism in it), the backup model answers it once instead, at the Worker's
/// level. The flag: the backup answered.
async fn ask_sup(m: &Models, level: Level, system: String, user: String, images: Vec<Image>, need: i64) -> Result<(String, bool, bool), String> {
    match ask(&m.sup, level, system.clone(), user.clone(), images.clone(), reply_room(&m.sup, need, level)).await {
        Err(e) if e == provider::failure::INSPECTED && m.backup_for(&m.sup).is_some() => {
            let b = m.backup_for(&m.sup).unwrap();
            ask(b, m.wl, system, user, images, reply_room(b, need, m.wl)).await.map(|(t, cut)| (t, cut, true))
        }
        r => r.map(|(t, cut)| (t, cut, false)),
    }
}

/// For the log: a step the backup model did (`what`: 审定切块, 定术语表…).
fn by_backup(what: &str, m: &Models) -> String {
    format!("{what}被阿里云的内容审核拦下，改用备用模型 {}", m.backup.as_ref().map_or("", |b| b.name.as_str()))
}

/// The draft, reviewed by the Supervisor when the text (or its outline) fits its window.
pub async fn plan(report: &Report, folder: &Path, job: &mut Job, blocks: &[Block], m: &Models) -> Result<(), String> {
    let (worker, sup, level) = (&m.worker, &m.sup, m.sl);
    let parts = mmd::parts(blocks);
    let budget = Budget::for_window(worker.window, TARGET);
    let draft = chunk::draft(blocks, &parts, budget);
    let room = sup.window * 6 / 10 - 4000;
    let full = chunk::outline(blocks, &parts, &draft, true);
    let shown = if context::estimate(&full) <= room {
        Some((full, true))
    } else {
        let short = chunk::outline(blocks, &parts, &draft, false);
        (context::estimate(&short) <= room).then_some((short, false))
    };
    let mut chunks = draft.clone();
    match shown {
        None => event(job, "全文太长，Supervisor 的模型放不下，照程序的草案切"),
        Some((outline, full)) => {
            report.step("Supervisor 审定切块…");
            job.supervisor_requests += 1;
            report.counts(job);
            let asked = ask_sup(m, level, prompts::plan_system(budget.target, budget.max), prompts::plan_message(&job.name, job.pages, &outline, full), vec![], 1500 + 150 * draft.len() as i64).await;
            if let Ok((_, _, true)) = asked {
                job.supervisor_requests += 1;
                event(job, by_backup("审定切块", m));
            }
            match asked.map(|(reply, cut, _)| (reply, cut)) {
                Ok((reply, _)) => match chunk::json_in(&reply).and_then(|v| serde_json::from_value::<Plan>(v).ok()).filter(|p| !p.chunks.is_empty()) {
                    Some(p) => {
                        let (c, said) = chunk::apply(blocks, &parts, &p, budget);
                        chunks = c;
                        job.plan_notes = said;
                        job.articles = p.articles.into_iter().filter(|a| a.article >= 1 && !a.title.trim().is_empty()).collect();
                        event(job, "Supervisor 审定了切块");
                    }
                    None => event(job, "Supervisor 的回答读不懂，照程序的草案切"),
                },
                Err(e) => event(job, format!("Supervisor 审定没成功（{e}），照程序的草案切")),
            }
        }
    }
    job.chunks = chunks.into_iter().map(|c| new_chunk(job, blocks, c)).collect();
    // a text file isn't cleaned: its chunks are taken as they are
    if job.kind == Some(Kind::Text) {
        std::fs::create_dir_all(work_dir(folder).join("chunks")).map_err(|e| format!("没能写中间结果：{e}"))?;
        for cj in job.chunks.iter_mut() {
            std::fs::write(chunk_file(folder, cj.chunk.start, "out"), chunk::source_text(blocks, &cj.chunk, false)).map_err(|e| format!("没能写中间结果：{e}"))?;
            (cj.state, cj.reviewed) = (State::Done, true);
        }
    }
    job.planned = true;
    event(job, format!("切成 {} 块", job.chunks.len()));
    report.counts(job);
    save(folder, job)
}

/// A chunk as planned (or split): not translated when its part is skipped (前置信息: its title is).
fn new_chunk(job: &Job, blocks: &[Block], c: Chunk) -> ChunkJob {
    let skipped = job.steps == Steps::Translate && c.part != Part::Front && job.skip.contains(&c.part);
    ChunkJob { pages: chunk::pages(blocks, &c), tokens: chunk::tokens(blocks, c.start, c.end), tr: if skipped { State::Skipped } else { State::Pending }, chunk: c, ..Default::default() }
}

// ---------- 3. 整理, 翻译 ----------

#[derive(Clone)]
pub struct Models {
    pub worker: Route,
    pub sup: Route,
    pub wl: Level,
    pub sl: Level,
    pub parallel: usize,
    /// For a request a content check turned down (a cloud model; None: 不用, or it can't be reached).
    pub backup: Option<Route>,
}

impl Models {
    /// The backup for a request `blocked` sent to (not the same model).
    fn backup_for(&self, blocked: &Route) -> Option<&Route> {
        self.backup.as_ref().filter(|b| b.model != blocked.model)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Clean,
    CleanReview,
    Translate,
    TranslateReview,
}

impl Stage {
    /// The chunk file it writes.
    fn file(self) -> &'static str {
        match self {
            Stage::Clean | Stage::CleanReview => "out",
            Stage::Translate | Stage::TranslateReview => "tr",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Stage::Clean => "整理",
            Stage::CleanReview => "整理的精修",
            Stage::Translate => "翻译",
            Stage::TranslateReview => "翻译的精修",
        }
    }
}

pub fn needs_terms(job: &Job) -> bool {
    job.steps == Steps::Translate && job.lang == Lang::EnZh
}

/// What a chunk does next (None: nothing now: it's done, or it waits for the term list).
fn next_stage(job: &Job, cj: &ChunkJob, terms_ready: bool) -> Option<Stage> {
    if cj.state == State::Pending {
        return Some(Stage::Clean);
    }
    if job.refine && !cj.reviewed {
        return Some(Stage::CleanReview);
    }
    if job.steps == Steps::Translate {
        if cj.tr == State::Pending {
            return terms_ready.then_some(Stage::Translate);
        }
        if job.refine && cj.tr != State::Skipped && !cj.tr_reviewed {
            return Some(Stage::TranslateReview);
        }
    }
    None
}

/// All its steps done.
fn finished(job: &Job, cj: &ChunkJob) -> bool {
    next_stage(job, cj, true).is_none()
}

fn terms_ready(job: &Job) -> bool {
    job.terms.is_some() || !needs_terms(job)
}

struct Ctx {
    models: Models,
    md: bool,
    english: bool,
    lang: Lang,
    wishes: String,
    /// 前置信息 is skipped: only its title is translated.
    title_only: bool,
    blocks: Arc<Vec<Block>>,
    pages_dir: PathBuf,
    rendered: usize,
    fixes: AtomicU32,
    fixes_cap: u32,
    worker_requests: AtomicU32,
    supervisor_requests: AtomicU32,
}

impl Ctx {
    fn count(&self, sup: bool) {
        if sup { &self.supervisor_requests } else { &self.worker_requests }.fetch_add(1, Ordering::SeqCst);
    }
}

enum Res {
    Done(String),
    Accepted(String, String),
    Split(usize),
    Stop(String),
}

struct Outcome {
    start: usize,
    stage: Stage,
    res: Res,
    requests: u32,
    fixes: u32,
    seconds: u64,
    notes: Vec<String>,
    by_supervisor: bool,
}

impl Outcome {
    fn new(start: usize, stage: Stage, fixes: u32) -> Outcome {
        Outcome { start, stage, res: Res::Stop(String::new()), requests: 0, fixes, seconds: 0, notes: vec![], by_supervisor: false }
    }
}

/// What a task under way ends with.
enum Finished {
    Chunk(Outcome),
    /// The list, or why not; and whether the backup model made it.
    Terms(Result<(Vec<Term>, String), String>, bool),
}

fn chunk_file(folder: &Path, start: usize, ext: &str) -> PathBuf {
    work_dir(folder).join("chunks").join(format!("{start:06}.{ext}"))
}

fn tail(text: &str) -> String {
    let n = text.chars().count();
    text.chars().skip(n.saturating_sub(BEFORE_CHARS)).collect()
}

fn wishes(job: &Job) -> String {
    prompts::wishes(&job.style, &job.domains, &job.prompt)
}

/// The text cut to about `room` tokens (and whether it was cut).
fn fit(text: &str, room: i64) -> (String, bool) {
    let est = context::estimate(text);
    if est <= room {
        return (text.to_string(), false);
    }
    let keep = (text.chars().count() as f64 * room as f64 / est as f64) as usize;
    (text.chars().take(keep).collect(), true)
}

/// The term list (the Supervisor, with its lookup), as a task beside the rest.
fn terms_task(job: &Job, text: &str, m: &Models, lookup: Lookup) -> impl std::future::Future<Output = Finished> + Send + 'static {
    let (from, to) = job.lang.says();
    let room = (m.sup.window / 2 - 6000).max(2000);
    let (body, cut) = fit(text, room);
    let system = prompts::terms_system(from, to, &wishes(job), lookup != Lookup::Off);
    let user = prompts::terms_message(&job.name, &body, cut);
    let m = m.clone();
    async move {
        let level = terms_level(m.sl);
        let mut asked = search::ask(&m.sup, level, &system, &user, reply_room(&m.sup, 8000, level), &lookup).await;
        // blocked by Alibaba's content check: the backup model, without looking up
        let backup = if matches!(&asked, Err(e) if e == provider::failure::INSPECTED) { m.backup_for(&m.sup).cloned() } else { None };
        if let Some(b) = &backup {
            let level = terms_level(m.wl);
            asked = search::ask(b, level, &system, &user, reply_room(b, 8000, level), &Lookup::Off).await.map(|(t, _)| (t, by_backup("定术语表", &m)));
        }
        let r = asked.and_then(|(reply, how)| {
            let (terms, whole) = terms_in(&reply).ok_or("术语表的回答读不懂")?;
            Ok((terms, if whole { how } else { format!("{how}，回答不完整，用了读得出的部分") }))
        });
        Finished::Terms(r, backup.is_some())
    }
}

/// Picking terms and their usual renderings needs knowledge more than long thought: at most 低.
fn terms_level(level: Level) -> Level {
    if level.thinking() { Level::Low } else { Level::Off }
}

/// The term list in a reply, and whether the reply was whole (a cut-off one gives its complete entries).
fn terms_in(reply: &str) -> Option<(Vec<Term>, bool)> {
    let keep = |terms: Vec<Term>| terms.into_iter().filter(|t| !t.source.trim().is_empty() && !t.target.trim().is_empty()).take(120).collect::<Vec<_>>();
    if let Some(terms) = chunk::json_in(reply).and_then(|v| serde_json::from_value::<Vec<Term>>(v["terms"].clone()).ok()) {
        return Some((keep(terms), true));
    }
    let terms: Vec<Term> = chunk::json_objects(reply).into_iter().filter(|v| v.get("source").is_some() && v.get("target").is_some()).filter_map(|v| serde_json::from_value(v).ok()).collect();
    (!terms.is_empty()).then(|| (keep(terms), false))
}

/// The terms a text has (all of them when `text` is None), as lines.
fn terms_lines(terms: &[Term], text: Option<&str>) -> String {
    let lower = text.map(str::to_lowercase);
    terms
        .iter()
        .filter(|t| lower.as_deref().is_none_or(|l| l.contains(&t.source.to_lowercase())))
        .take(80)
        .map(|t| if t.note.trim().is_empty() { format!("- {} → {}", t.source, t.target) } else { format!("- {} → {}（{}）", t.source, t.target, t.note.trim()) })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The chunks' steps, `parallel` at a time, the earliest chunk first; `set` may hold the term list.
/// Stops scheduling at the first step that has to stop (the ones under way finish and are kept).
async fn work(report: &Report, folder: &Path, job: &mut Job, blocks: &Arc<Vec<Block>>, models: &Models, mut set: JoinSet<Finished>) -> Result<(), String> {
    std::fs::create_dir_all(work_dir(folder).join("chunks")).map_err(|e| format!("没能写中间结果：{e}"))?;
    let parallel = models.parallel.max(1);
    let ctx = Arc::new(Ctx {
        models: Models { parallel, ..models.clone() },
        md: job.md,
        english: job.english_only,
        lang: job.lang,
        wishes: wishes(job),
        title_only: job.skip.contains(&Part::Front),
        blocks: blocks.clone(),
        pages_dir: work_dir(folder).join("pages"),
        rendered: job.rendered,
        fixes: AtomicU32::new(job.fixes),
        fixes_cap: fixes_cap(job.chunks.len()),
        worker_requests: AtomicU32::new(job.worker_requests),
        supervisor_requests: AtomicU32::new(job.supervisor_requests),
    });
    let mut busy: HashSet<usize> = HashSet::new();
    let mut failure: Option<String> = None;
    let progress = |report: &Report, job: &Job| {
        let n = job.chunks.len();
        let cleaned = job.chunks.iter().filter(|c| c.state != State::Pending).count();
        let translated = job.chunks.iter().filter(|c| c.tr != State::Pending).count();
        report.step(&if job.steps == Steps::Translate { format!("整理 {cleaned}/{n} · 翻译 {translated}/{n}") } else { format!("整理 {cleaned}/{n}") });
        report.counts(job);
    };
    progress(report, job);
    loop {
        if failure.is_none() {
            let ready = terms_ready(job);
            while busy.len() < parallel {
                let pick = job.chunks.iter().enumerate().find_map(|(k, cj)| if busy.contains(&cj.chunk.start) { None } else { next_stage(job, cj, ready).map(|s| (k, s)) });
                let Some((k, stage)) = pick else { break };
                let cj = &job.chunks[k];
                let c = cj.chunk.clone();
                let n = job.chunks.len();
                busy.insert(c.start);
                let read = |ext: &str, start: usize| std::fs::read_to_string(chunk_file(folder, start, ext)).unwrap_or_default();
                let previous = (k > 0).then(|| job.chunks[k - 1].chunk.clone());
                let ctx = ctx.clone();
                match stage {
                    Stage::Clean => {
                        let before = previous.map(|p| tail(&chunk::source_text(blocks, &p, false))).unwrap_or_default();
                        let fixes = cj.fixes;
                        set.spawn(async move { Finished::Chunk(clean_one(ctx, c, k + 1, n, before, fixes).await) });
                    }
                    Stage::CleanReview => {
                        let (source, output, pages) = (chunk::source_text(blocks, &c, false), read("out", c.start), chunk::image_pages(blocks, &c));
                        set.spawn(async move { Finished::Chunk(review_one(ctx, c, Stage::CleanReview, source, output, pages).await) });
                    }
                    Stage::Translate => {
                        let source = read("out", c.start);
                        let before = previous.map(|p| tail(&read("out", p.start))).unwrap_or_default();
                        let terms = terms_lines(job.terms.as_deref().unwrap_or(&[]), Some(&source));
                        let fixes = cj.tr_fixes;
                        set.spawn(async move { Finished::Chunk(translate_one(ctx, c, k + 1, n, source, before, terms, fixes).await) });
                    }
                    Stage::TranslateReview => {
                        let (source, output) = (read("out", c.start), read("tr", c.start));
                        set.spawn(async move { Finished::Chunk(review_one(ctx, c, Stage::TranslateReview, source, output, (0, 0)).await) });
                    }
                }
            }
        }
        let Some(joined) = set.join_next().await else { break };
        match joined {
            Err(e) => {
                failure.get_or_insert(format!("出了意外：{e}"));
            }
            Ok(Finished::Terms(r, backup)) => match r {
                Ok((terms, how)) => {
                    if backup {
                        ctx.count(true);
                    }
                    job.terms_note = format!("{} 条，{how}", terms.len());
                    event(job, format!("术语表定好了：{}", job.terms_note));
                    job.terms = Some(terms);
                }
                Err(e) => {
                    job.terms_note = format!("没定成（{e}），照没有术语表翻");
                    event(job, format!("术语表{}", job.terms_note));
                    job.terms = Some(vec![]);
                }
            },
            Ok(Finished::Chunk(o)) => {
                busy.remove(&o.start);
                handle(folder, job, blocks, o, &mut failure)?;
            }
        }
        job.fixes = ctx.fixes.load(Ordering::SeqCst);
        job.worker_requests = ctx.worker_requests.load(Ordering::SeqCst);
        job.supervisor_requests = ctx.supervisor_requests.load(Ordering::SeqCst);
        save(folder, job)?;
        progress(report, job);
    }
    match failure {
        Some(f) => Err(f),
        None => Ok(()),
    }
}

/// A chunk's step ended: its file, its state, a split, a stop.
fn handle(folder: &Path, job: &mut Job, blocks: &[Block], o: Outcome, failure: &mut Option<String>) -> Result<(), String> {
    let Some(k) = job.chunks.iter().position(|c| c.chunk.start == o.start) else { return Ok(()) };
    {
        let cj = &mut job.chunks[k];
        cj.requests += o.requests;
        if o.stage == Stage::Translate { cj.tr_fixes = o.fixes } else if o.stage == Stage::Clean { cj.fixes = o.fixes }
        cj.seconds += o.seconds;
        cj.notes.extend(o.notes);
        cj.by_supervisor |= o.by_supervisor;
    }
    let write = |text: &str| std::fs::write(chunk_file(folder, o.start, o.stage.file()), text).map_err(|e| format!("没能写中间结果：{e}"));
    let done = |cj: &mut ChunkJob, state: State| match o.stage {
        Stage::Clean => cj.state = state,
        Stage::CleanReview => cj.reviewed = true,
        Stage::Translate => cj.tr = state,
        Stage::TranslateReview => cj.tr_reviewed = true,
    };
    match o.res {
        Res::Done(text) => {
            write(&text)?;
            done(&mut job.chunks[k], State::Done);
        }
        Res::Accepted(text, reason) => {
            write(&text)?;
            done(&mut job.chunks[k], State::Accepted);
            job.chunks[k].notes.push(format!("Supervisor 接受了{}：{reason}", o.stage.name()));
        }
        Res::Split(at) => {
            let old = job.chunks.remove(k);
            for (i, c) in [Chunk { end: at, ..old.chunk.clone() }, Chunk { start: at, ..old.chunk.clone() }].into_iter().enumerate() {
                let mut cj = new_chunk(job, blocks, c);
                if i == 0 {
                    (cj.requests, cj.notes) = (old.requests, old.notes.clone());
                }
                job.chunks.insert(k + i, cj);
            }
            event(job, format!("第 {} 块拆成了两块", k + 1));
        }
        Res::Stop(reason) => {
            let (a, b) = job.chunks[k].pages;
            event(job, format!("第 {} 块{}停下：{reason}", k + 1, o.stage.name()));
            failure.get_or_insert(if a > 0 { format!("第 {} 块（原文第 {a}～{b} 页）{}：{reason}", k + 1, o.stage.name()) } else { format!("第 {} 块{}：{reason}", k + 1, o.stage.name()) });
        }
    }
    Ok(())
}

/// The pages' images for a cloud model (local models don't get them).
fn page_images(ctx: &Ctx, route: &Route, pages: (usize, usize)) -> Vec<Image> {
    if ctx.rendered == 0 || pages.0 == 0 || !matches!(route.target, Target::Cloud { .. }) {
        return vec![];
    }
    (pages.0..=pages.1).take(IMAGES_A_CHUNK).filter_map(|n| std::fs::read(ctx.pages_dir.join(pdf::page_name(n))).ok()).map(|d| Image { media_type: "image/jpeg".into(), data: office::base64(&d) }).collect()
}

/// What the Supervisor chose for a step that failed its check.
enum Decision {
    Retry(Option<String>),
    Split(usize),
    Think,
    Supervisor,
    Accept(String),
    Stop(String),
    /// Why not: the request failed, or no usable answer.
    Unreadable(String),
}

#[allow(clippy::too_many_arguments)]
async fn ask_fix(ctx: &Ctx, k: usize, n: usize, part: Part, cuts: Option<&[usize]>, source: &str, output: &str, said: &str, images: Vec<Image>, notes: &mut Vec<String>) -> Decision {
    let m = &ctx.models;
    ctx.count(true);
    let asked = ask_sup(m, m.sl, prompts::fix_system(cuts.is_none(), ctx.md), prompts::fix_message(k, n, part, cuts, source, output, said), images, 1200).await;
    if let Ok((_, _, true)) = asked {
        ctx.count(true);
        notes.push(by_backup("排查", m));
    }
    let d = match asked {
        Err(e) => return Decision::Unreadable(format!("请求失败：{e}")),
        Ok((t, cut, _)) => match chunk::json_in(&t) {
            Some(d) => d,
            None if cut => return Decision::Unreadable("回答到了长度上限".into()),
            None => return Decision::Unreadable(format!("回答里没有 JSON：{}", t.trim().chars().take(80).collect::<String>())),
        },
    };
    let field = |f: &str| d[f].as_str().unwrap_or("").trim().to_string();
    match d["action"].as_str().unwrap_or("") {
        "retry" => Decision::Retry(Some(field("hint")).filter(|h| !h.is_empty())),
        "split" => field("at").trim_start_matches(['c', 'C']).parse::<usize>().ok().filter(|a| cuts.is_some_and(|c| c.contains(a))).map_or_else(|| Decision::Unreadable(format!("拆分点「{}」不能切", field("at"))), Decision::Split),
        "think" => Decision::Think,
        "supervisor" => Decision::Supervisor,
        "accept" => Decision::Accept(field("reason")),
        "stop" => Decision::Stop(field("reason")),
        a => Decision::Unreadable(format!("没有这种做法：「{a}」")),
    }
}

/// After a failed Worker request: wait and ask again (15 s, then 45 s: the three requests that stopped
/// Pargent broke off within the same three minutes on OpenRouter), then the backup model, then stop.
enum Again {
    Wait(Duration),
    Backup(String),
    Stop,
}

fn again<'a>(e: &str, tries: &mut usize, route: &Route, backup: &mut Option<&'a Route>, m: &'a Models) -> Again {
    // the key, the credit or the request itself: asking again won't help
    let hopeless = [provider::failure::UNAUTHORIZED, provider::failure::CREDITS].contains(&e) || provider::rejected(e);
    if !hopeless && *tries < 2 {
        let secs = if matches!(route.target, Target::Mock { .. }) { 0 } else { [15, 45][*tries] };
        *tries += 1;
        return Again::Wait(Duration::from_secs(secs));
    }
    match m.backup_for(route).filter(|_| backup.is_none()) {
        Some(b) => {
            *backup = Some(b);
            *tries = 2;
            Again::Backup(b.name.clone())
        }
        None => Again::Stop,
    }
}

/// The source just before a chunk and just after it (8 blocks each): what its page images also show.
fn neighbor_text(blocks: &[Block], c: &Chunk) -> (String, String) {
    let around = |start: usize, end: usize| chunk::source_text(blocks, &Chunk { start, end, ..Default::default() }, false);
    (around(c.start.saturating_sub(8), c.start), around(c.end, (c.end + 8).min(blocks.len())))
}

/// 整理 one chunk: the Worker, the checks, the Supervisor when they fail.
async fn clean_one(ctx: Arc<Ctx>, c: Chunk, k: usize, n: usize, before: String, prior_fixes: u32) -> Outcome {
    let started = Instant::now();
    let blocks = ctx.blocks.clone();
    let m = &ctx.models;
    let marked = chunk::source_text(&blocks, &c, true);
    let plain = chunk::source_text(&blocks, &c, false);
    let neighbors = neighbor_text(&blocks, &c);
    let pages = chunk::pages(&blocks, &c);
    let mut o = Outcome::new(c.start, Stage::Clean, prior_fixes);
    let (mut hint, mut think, mut tries, mut backup, mut roomier): (Option<String>, bool, usize, Option<&Route>, bool) = (None, false, 0, None, false);
    o.res = loop {
        let (route, level) = if let Some(b) = backup { (b, m.wl) } else if o.by_supervisor { (&m.sup, m.sl) } else if think { (&m.worker, Level::High) } else { (&m.worker, m.wl) };
        let images = page_images(&ctx, route, chunk::image_pages(&blocks, &c));
        let system = prompts::worker_system(ctx.md, ctx.english, c.part, !images.is_empty());
        let user = prompts::worker_message(&WorkerAsk { note: &c.note, hint: hint.as_deref(), part: c.part, pages, k, n, before: &before, source: &marked });
        let max = reply_room(route, context::estimate(&marked) * 2 + 1024, level) + if roomier { 16_384 } else { 0 };
        o.requests += 1;
        ctx.count(o.by_supervisor);
        let (text, cut) = match ask(route, level, system, user, images, max).await {
            Ok(x) => x,
            // Alibaba's content check: the backup model does this chunk (asking again won't help)
            Err(e) if e == provider::failure::INSPECTED && backup.is_none() && m.backup_for(route).is_some() => {
                backup = m.backup_for(route);
                o.notes.push(format!("整理被阿里云的内容审核拦下，这一块改用备用模型 {}", backup.map_or("", |b| b.name.as_str())));
                continue;
            }
            Err(e) => match again(&e, &mut tries, route, &mut backup, m) {
                Again::Wait(s) => {
                    o.notes.push(format!("整理的请求失败（{e}），{} 秒后再试", s.as_secs()));
                    tokio::time::sleep(s).await;
                    continue;
                }
                Again::Backup(b) => {
                    o.notes.push(format!("整理的请求又失败了（{e}），这一块改用备用模型 {b}"));
                    continue;
                }
                Again::Stop => break Res::Stop(format!("请求失败：{e}")),
            },
        };
        let (output, made_up) = prompts::drop_made_up_images(&plain, prompts::money(&plain, prompts::cleaned(&text)));
        if made_up > 0 {
            o.notes.push(format!("整理结果里有 {made_up} 张原文没有的图（模型编的地址），程序删掉了"));
        }
        let (output, links) = prompts::drop_made_up_links(&plain, output);
        if links > 0 {
            o.notes.push(format!("整理结果里有 {links} 个原文没有的链接，程序只留了文字"));
        }
        // thinking took the whole reply (a model that won't stop thinking): more room, no Supervisor
        if output.trim().is_empty() && cut && !roomier {
            roomier = true;
            o.notes.push("整理的回复长度被思考用完了，没写出结果，多留空间再试一次".into());
            continue;
        }
        // even one block copied off a page image from the chunk just before or after
        let output = match check::trim_neighbors(&plain, &output, &neighbors.0, &neighbors.1) {
            Some((kept, n)) => {
                o.notes.push(format!("结果头尾有照原页图抄来的前后块的内容，程序删掉了 {n} 段"));
                kept
            }
            None => output,
        };
        let mut issues = check::check(&plain, &output, cut);
        if !ctx.md {
            issues.extend(check::heading_style(&plain, &output));
        }
        if issues.is_empty() {
            break Res::Done(output);
        }
        // copied off a page image from the chunk before or after: taken off by the program when the rest passes
        if issues.iter().any(|i| i.code == "added") {
            if let Some((kept, n)) = check::trim_copied(&plain, &output) {
                if check::check(&plain, &kept, cut).is_empty() && (ctx.md || check::heading_style(&plain, &kept).is_none()) {
                    o.notes.push(format!("结果头尾有照原页图抄来的别的块的内容，程序删掉了 {n} 段"));
                    break Res::Done(kept);
                }
            }
        }
        let said = check::said(&issues);
        o.notes.push(format!("整理的检查没过：{said}"));
        if o.fixes >= FIXES_PER_CHUNK || ctx.fixes.load(Ordering::SeqCst) >= ctx.fixes_cap {
            break Res::Stop(format!("整理了几次都没通过检查（{said}）"));
        }
        o.fixes += 1;
        ctx.fixes.fetch_add(1, Ordering::SeqCst);
        o.requests += 1;
        let cuts: Vec<usize> = (c.start + 1..c.end).filter(|i| mmd::cuttable(&blocks, *i)).collect();
        match ask_fix(&ctx, k, n, c.part, Some(&cuts), &plain, &output, &said, page_images(&ctx, &m.sup, chunk::image_pages(&blocks, &c)), &mut o.notes).await {
            Decision::Retry(h) => {
                o.notes.push(format!("Supervisor：带提示重做（{}）", h.as_deref().unwrap_or("没写提示")));
                hint = h;
            }
            Decision::Split(a) => {
                o.notes.push(format!("Supervisor：从 c{a} 拆成两块重做"));
                break Res::Split(a);
            }
            Decision::Think => {
                think = true;
                o.notes.push("Supervisor：打开思考重做".into());
            }
            Decision::Supervisor => {
                (o.by_supervisor, backup) = (true, None);
                o.notes.push("Supervisor：这一块它自己来整理".into());
            }
            Decision::Accept(r) => break Res::Accepted(output, r),
            Decision::Stop(r) => break Res::Stop(format!("Supervisor 让停下：{r}")),
            Decision::Unreadable(why) => o.notes.push(format!("Supervisor 没给出能用的做法（{why}），照原样重做")),
        }
    };
    o.seconds = started.elapsed().as_secs();
    o
}

/// 翻译 one chunk (简繁: OpenCC's draft, looked over by the Worker); the checks, the Supervisor.
#[allow(clippy::too_many_arguments)]
async fn translate_one(ctx: Arc<Ctx>, c: Chunk, k: usize, n: usize, source: String, before: String, terms: String, prior_fixes: u32) -> Outcome {
    let started = Instant::now();
    let m = &ctx.models;
    let mut o = Outcome::new(c.start, Stage::Translate, prior_fixes);
    let (from, to) = ctx.lang.says();
    let draft = match ctx.lang.script().map(|t| script::convert(&source, t)) {
        Some(Ok(d)) => Some(d),
        Some(Err(e)) => {
            o.res = Res::Stop(e);
            return o;
        }
        None => None,
    };
    let title_only = c.part == Part::Front && ctx.title_only;
    let (mut hint, mut think, mut tries, mut backup, mut roomier): (Option<String>, bool, usize, Option<&Route>, bool) = (None, false, 0, None, false);
    o.res = loop {
        let (route, level) = if let Some(b) = backup { (b, m.wl) } else if o.by_supervisor { (&m.sup, m.sl) } else if think { (&m.worker, Level::High) } else { (&m.worker, m.wl) };
        let (system, user) = match &draft {
            Some(d) => {
                let mut user = prompts::script_message(&source, d);
                if let Some(h) = &hint {
                    user = format!("【Supervisor 的提示】上一次没通过检查。{h}\n\n{user}");
                }
                (prompts::script_system(ctx.lang.script() == Some(true), &ctx.wishes), user)
            }
            None => (prompts::translate_system(from, to, ctx.md, &ctx.wishes, title_only), prompts::translate_message(&TranslateAsk { note: &c.note, hint: hint.as_deref(), part: c.part, k, n, terms: &terms, before: &before, source: &source })),
        };
        let max = reply_room(route, context::estimate(&source) * 3 + 1024, level) + if roomier { 16_384 } else { 0 };
        o.requests += 1;
        ctx.count(o.by_supervisor);
        let (text, cut) = match ask(route, level, system, user, vec![], max).await {
            Ok(x) => x,
            // Alibaba's content check: the backup model does this chunk (asking again won't help)
            Err(e) if e == provider::failure::INSPECTED && backup.is_none() && m.backup_for(route).is_some() => {
                backup = m.backup_for(route);
                o.notes.push(format!("翻译被阿里云的内容审核拦下，这一块改用备用模型 {}", backup.map_or("", |b| b.name.as_str())));
                continue;
            }
            Err(e) => match again(&e, &mut tries, route, &mut backup, m) {
                Again::Wait(s) => {
                    o.notes.push(format!("翻译的请求失败（{e}），{} 秒后再试", s.as_secs()));
                    tokio::time::sleep(s).await;
                    continue;
                }
                Again::Backup(b) => {
                    o.notes.push(format!("翻译的请求又失败了（{e}），这一块改用备用模型 {b}"));
                    continue;
                }
                Again::Stop => break Res::Stop(format!("请求失败：{e}")),
            },
        };
        let (output, links) = prompts::drop_made_up_links(&source, prompts::money(&source, prompts::cleaned(&text)));
        if links > 0 {
            o.notes.push(format!("译文里有 {links} 个原文没有的链接，程序只留了文字"));
        }
        let mut output = output;
        if title_only && keep_authors(&source, &mut output) {
            o.notes.push("译文里的作者和单位被译了，程序换回了原文".into());
        }
        // a traditional character now and then in an English → 简体 translation (DeepSeek's 「中東」)
        let output = match script::simplified(&output) {
            Ok(simple) if ctx.lang == Lang::EnZh && draft.is_none() && simple != output => {
                let n = output.chars().zip(simple.chars()).filter(|(a, b)| a != b).count();
                o.notes.push(format!("译文里混进了 {n} 个繁体字，程序换成了简体字"));
                simple
            }
            _ => output,
        };
        // thinking took the whole reply (a model that won't stop thinking): more room, no Supervisor
        if output.trim().is_empty() && cut && !roomier {
            roomier = true;
            o.notes.push("翻译的回复长度被思考用完了，没写出结果，多留空间再试一次".into());
            continue;
        }
        let mut issues = if draft.is_some() { check::check_script(&source, &output, cut) } else { check::check_translation(&source, &output, cut, !title_only) };
        if !ctx.md {
            issues.extend(check::heading_style(&source, &output));
        }
        if issues.is_empty() {
            break Res::Done(output);
        }
        let said = check::said(&issues);
        o.notes.push(format!("翻译的检查没过：{said}"));
        if o.fixes >= FIXES_PER_CHUNK || ctx.fixes.load(Ordering::SeqCst) >= ctx.fixes_cap {
            break Res::Stop(format!("翻译了几次都没通过检查（{said}）"));
        }
        o.fixes += 1;
        ctx.fixes.fetch_add(1, Ordering::SeqCst);
        o.requests += 1;
        match ask_fix(&ctx, k, n, c.part, None, &source, &output, &said, vec![], &mut o.notes).await {
            Decision::Retry(h) => {
                o.notes.push(format!("Supervisor：带提示重译（{}）", h.as_deref().unwrap_or("没写提示")));
                hint = h;
            }
            Decision::Think => {
                think = true;
                o.notes.push("Supervisor：打开思考重译".into());
            }
            Decision::Supervisor => {
                (o.by_supervisor, backup) = (true, None);
                o.notes.push("Supervisor：这一块它自己来翻".into());
            }
            Decision::Accept(r) => break Res::Accepted(output, r),
            Decision::Stop(r) => break Res::Stop(format!("Supervisor 让停下：{r}")),
            Decision::Split(_) => o.notes.push("Supervisor 没给出能用的做法（翻译不能拆块），照原样重译".into()),
            Decision::Unreadable(why) => o.notes.push(format!("Supervisor 没给出能用的做法（{why}），照原样重译")),
        }
    };
    o.seconds = started.elapsed().as_secs();
    o
}

/// 精修: the Supervisor's fixes to a chunk's 整理 or 翻译 (kept as it was when it can't be done).
async fn review_one(ctx: Arc<Ctx>, c: Chunk, stage: Stage, source: String, output: String, pages: (usize, usize)) -> Outcome {
    let started = Instant::now();
    let m = &ctx.models;
    let translating = stage == Stage::TranslateReview;
    let mut o = Outcome::new(c.start, stage, 0);
    let images = if translating { vec![] } else { page_images(&ctx, &m.sup, pages) };
    o.requests += 1;
    ctx.count(true);
    let asked = ask_sup(m, m.sl, prompts::review_system(translating, ctx.md), prompts::review_message(&source, &output), images, context::estimate(&output) / 2 + 2000).await;
    if let Ok((_, _, true)) = asked {
        o.requests += 1;
        ctx.count(true);
        o.notes.push(by_backup(stage.name(), m));
    }
    let text = match asked.map(|(reply, cut, _)| (reply, cut)) {
        Ok((reply, cut)) => match prompts::fixes_in(&reply) {
            Some(fixes) => {
                // MMD's \\section*{} turned into a # heading (Sonnet did): the heading style must not change
                let (fixes, restyled): (Vec<_>, Vec<_>) = fixes.into_iter().partition(|f| ctx.md || !prompts::to_markdown_heading(f));
                if !restyled.is_empty() {
                    o.notes.push(format!("{}：{} 处要把 MMD 的标题改成 # 写法，没改", stage.name(), restyled.len()));
                }
                let (mut t, skipped) = prompts::apply(&output, &fixes);
                if translating && c.part == Part::Front && ctx.title_only && keep_authors(&source, &mut t) {
                    o.notes.push(format!("{}改了不翻的作者和单位，程序换回了原文", stage.name()));
                }
                let not = if skipped.is_empty() { String::new() } else { format!("，{} 处没对上原话", skipped.len()) };
                let what: Vec<String> = fixes.iter().filter(|f| !skipped.contains(f)).map(|f| if f.why.trim().is_empty() { format!("「{}」→「{}」", f.find, f.replace) } else { format!("「{}」→「{}」（{}）", f.find, f.replace, f.why.trim()) }).collect();
                let what = if what.is_empty() { String::new() } else { format!("：{}", what.join("；")) };
                o.notes.push(format!("{}：改了 {} 处{not}{what}", stage.name(), fixes.len() - skipped.len()));
                t
            }
            None => {
                o.notes.push(format!("{}的回答读不懂（{}），没改", stage.name(), if cut { "到了长度上限" } else { "没有 JSON" }));
                output
            }
        },
        Err(e) => {
            o.notes.push(format!("{}没成功（{e}），没改", stage.name()));
            output
        }
    };
    o.res = Res::Done(text);
    o.seconds = started.elapsed().as_secs();
    o
}

/// The parts left untranslated, as the page says them.
fn kept_parts(job: &Job) -> Vec<&'static str> {
    job.skip.iter().map(|p| if *p == Part::Front { "作者和单位" } else { p.name() }).collect()
}

/// `find` is inside the text's \author{…} (left untranslated with 作者和单位).
fn in_author(text: &str, find: &str) -> bool {
    let (Some((start, end)), Some(at)) = (author_span(text), text.find(find)) else { return false };
    at >= start && at < end
}

/// Where the text's \author{…} is (to its closing brace).
fn author_span(text: &str) -> Option<(usize, usize)> {
    let start = text.find("\\author{")?;
    let mut depth = 0;
    let end = text[start..].char_indices().find_map(|(i, c)| {
        depth += match c { '{' => 1, '}' => -1, _ => 0 };
        (c == '}' && depth == 0).then_some(start + i + 1)
    });
    Some((start, end.unwrap_or(text.len())))
}

/// 作者和单位 left untranslated: the translation's \author{…} put back as the source has it (DeepSeek
/// translated the affiliations against the request). Whether it changed anything.
fn keep_authors(source: &str, output: &mut String) -> bool {
    let (Some((a, b)), Some((c, d))) = (author_span(source), author_span(output)) else { return false };
    if source[a..b] == output[c..d] {
        return false;
    }
    output.replace_range(c..d, &source[a..b]);
    true
}

/// 通读: the Supervisor reads the translation (in windows when it doesn't fit) for what spans chunks;
/// a fix is applied where its words are in exactly one chunk, once.
async fn read_through(report: &Report, folder: &Path, job: &mut Job, m: &Models) -> Result<(), String> {
    let sup = &m.sup;
    report.step("通读…");
    let read = |start: usize| std::fs::read_to_string(chunk_file(folder, start, "tr")).unwrap_or_default();
    let ks: Vec<usize> = (0..job.chunks.len()).filter(|k| job.chunks[*k].tr != State::Skipped).collect();
    let room = (sup.window * 4 / 10 - 6000).max(3000);
    let mut windows: Vec<Vec<usize>> = vec![];
    let mut size = 0;
    for k in ks {
        let t = context::estimate(&read(job.chunks[k].chunk.start));
        if windows.is_empty() || size + t > room {
            windows.push(vec![]);
            size = 0;
        }
        size += t;
        windows.last_mut().unwrap().push(k);
    }
    let terms = terms_lines(job.terms.as_deref().unwrap_or(&[]), None);
    let kept = kept_parts(job);
    let (mut applied, mut skipped, mut read_n) = (vec![], vec![], 0);
    for w in &windows {
        let text: String = w.iter().map(|k| format!("⟦第 {} 块⟧\n{}\n\n", k + 1, read(job.chunks[*k].chunk.start))).collect();
        let left = check::leftovers(&text);
        // nothing across chunks to check when no article goes on into the next chunk (one chunk, or an article each)
        if !w.windows(2).any(|p| job.chunks[p[0]].chunk.article == job.chunks[p[1]].chunk.article) && left.is_empty() {
            continue;
        }
        read_n += 1;
        job.supervisor_requests += 1;
        report.counts(job);
        let asked = ask_sup(m, m.sl, prompts::read_system(), prompts::read_message(&terms, &text, &left, &kept), vec![], 4000).await;
        if let Ok((_, _, true)) = asked {
            job.supervisor_requests += 1;
            job.read_notes.push(by_backup("通读", m));
        }
        let fixes = match asked.map(|(reply, cut, _)| (reply, cut)) {
            Ok((reply, cut)) => match prompts::fixes_in(&reply) {
                Some(f) => f,
                None => {
                    job.read_notes.push(format!("有一段通读的回答读不懂（{}），没改", if cut { "到了长度上限" } else { "没有 JSON" }));
                    continue;
                }
            },
            Err(e) => {
                job.read_notes.push(format!("有一段通读没成功（{e}），没改"));
                continue;
            }
        };
        for f in fixes {
            let hits: Vec<usize> = w.iter().copied().filter(|k| !f.find.is_empty() && read(job.chunks[*k].chunk.start).contains(&f.find)).collect();
            let why_not = match hits.as_slice() {
                _ if f.find == f.replace => Some("没有改动"),
                [] => Some("哪块都没有这句原话"),
                [k] if read(job.chunks[*k].chunk.start).matches(&f.find).count() > 1 => Some("这句原话出现了好几次"),
                [k] if job.skip.contains(&Part::Front) && job.chunks[*k].chunk.part == Part::Front && in_author(&read(job.chunks[*k].chunk.start), &f.find) => Some("是不翻的作者和单位"),
                [_] => None,
                _ => Some("这句原话出现了好几次"),
            };
            if let Some(why) = why_not {
                skipped.push(format!("没改：「{}」（{why}）", f.find.chars().take(40).collect::<String>().replace('\n', " ")));
                continue;
            }
            let start = job.chunks[hits[0]].chunk.start;
            let text = read(start).replacen(&f.find, &f.replace, 1);
            std::fs::write(chunk_file(folder, start, "tr"), text).map_err(|e| format!("没能写中间结果：{e}"))?;
            applied.push(if f.why.trim().is_empty() { format!("「{}」→「{}」", f.find, f.replace) } else { format!("「{}」→「{}」（{}）", f.find, f.replace, f.why.trim()) });
        }
    }
    let not = if skipped.is_empty() { String::new() } else { format!("，{} 处没改", skipped.len()) };
    job.read_notes.insert(0, if read_n > 0 { format!("读了 {read_n} 段，改了 {} 处{not}", applied.len()) } else if job.chunks.len() > 1 { "每篇文章只有一块，也没有漏译的英文词，不用通读".into() } else { "只有一块，也没有漏译的英文词，不用通读".into() });
    job.read_notes.extend(applied);
    job.read_notes.extend(skipped);
    job.read_through = true;
    event(job, "通读完了");
    save(folder, job)
}

// ---------- 4. the results, the log ----------

/// One kind of result (`ext`: the chunk files; `stem`: 译文 / 整理稿): one file, or one an article.
fn write_set(folder: &Path, job: &Job, ext: &str, stem: &str) -> Result<Vec<String>, String> {
    let suffix = if job.md { "md" } else { "mmd" };
    let mut by_article: Vec<(usize, Vec<String>)> = vec![];
    let mut chunks: Vec<&ChunkJob> = job.chunks.iter().collect();
    chunks.sort_by_key(|c| c.chunk.start);
    for (k, cj) in chunks.iter().enumerate() {
        // a part not translated: its cleaned text
        let file = if ext == "tr" && cj.tr == State::Skipped { "out" } else { ext };
        let text = std::fs::read_to_string(chunk_file(folder, cj.chunk.start, file)).map_err(|_| format!("第 {} 块的结果不见了", k + 1))?;
        match by_article.last_mut() {
            Some((a, v)) if *a == cj.chunk.article => v.push(text),
            _ => by_article.push((cj.chunk.article, vec![text])),
        }
    }
    let many = by_article.len() > 1;
    let mut out = vec![];
    for (a, texts) in by_article {
        let name = if many {
            let title = job.articles.iter().find(|x| x.article == a).map(|x| safe_name(&x.title)).unwrap_or_else(|| format!("第 {a} 篇"));
            format!("{stem} {a} · {}.{suffix}", title.chars().take(40).collect::<String>())
        } else {
            format!("{stem}.{suffix}")
        };
        std::fs::write(folder.join(&name), texts.join("\n\n") + "\n").map_err(|e| format!("没能写 {name}：{e}"))?;
        out.push(name);
    }
    Ok(out)
}

/// The results in order: 译文, 整理稿 (not for a text file), 术语表. Returns the names written.
pub fn assemble(folder: &Path, job: &Job) -> Result<Vec<String>, String> {
    let mut out = vec![];
    if job.steps == Steps::Translate {
        out.extend(write_set(folder, job, "tr", "译文")?);
    }
    if job.kind != Some(Kind::Text) {
        out.extend(write_set(folder, job, "out", "整理稿")?);
    }
    if let Some(terms) = job.terms.as_ref().filter(|t| !t.is_empty()) {
        let rows: String = terms.iter().map(|t| format!("| {} | {} | {} |\n", t.source.replace('|', "\\|"), t.target.replace('|', "\\|"), t.note.replace('|', "\\|"))).collect();
        std::fs::write(folder.join(TERMS_FILE), format!("# 术语表 · {}\n\n{}\n\n| 原文 | 译文 | 说明 |\n|---|---|---|\n{rows}", job.name, job.terms_note)).map_err(|e| format!("没能写术语表：{e}"))?;
        out.push(TERMS_FILE.into());
    }
    Ok(out)
}

fn duration(s: u64) -> String {
    match (s / 3600, s / 60 % 60, s % 60) {
        (0, 0, x) => format!("{x} 秒"),
        (0, m, x) => format!("{m} 分 {x} 秒"),
        (h, m, _) => format!("{h} 小时 {m} 分"),
    }
}

fn state_name(s: State, by_supervisor: bool) -> &'static str {
    match s {
        State::Done if by_supervisor => "完成（Supervisor 做的）",
        State::Done => "完成",
        State::Accepted => "Supervisor 接受",
        State::Skipped => "不翻",
        State::Pending => "还没做完",
    }
}

/// The work log (工作日志.md).
// ---------- 5. 另存为 ----------

/// Mathpix's image links last this long after 提取.
const LINK_DAYS: f64 = 30.0;

/// 另存为: each result sent to Mathpix's converter (`keys` None: the mock writes stand-ins). Returns the
/// files written; what didn't go is in `convert_notes`.
async fn convert(report: &Report, folder: &Path, job: &mut Job, keys: Option<&mathpix::Keys>, files: &[String]) -> Vec<String> {
    let formats = job.formats.clone();
    let names: Vec<&str> = formats.iter().map(|f| f.name()).collect();
    let fresh = job.extracted_on.is_empty() || days_since(&job.extracted_on) < LINK_DAYS;
    let mut made = vec![];
    for (i, file) in files.iter().enumerate() {
        report.step(&format!("另存为 {}（{}/{}）…", names.join("、"), i + 1, files.len()));
        let Ok(text) = std::fs::read_to_string(folder.join(file)) else { continue };
        let (mmd, left_out) = linked(&text, if fresh { &job.images } else { &[] });
        if left_out > 0 {
            let why = if fresh { "不是 Mathpix 识别出的图，没有它的链接" } else { "提取已超过 30 天，Mathpix 的图片链接失效了" };
            job.convert_notes.push(format!("{file}：{left_out} 张图没放进去（{why}）"));
        }
        let stem = file.rsplit_once('.').map_or(file.as_str(), |(s, _)| s);
        let result = match keys {
            None => mock_convert(&mmd, &formats),
            Some(k) => mathpix_convert(k, &mmd, &formats).await,
        };
        match result {
            Err(e) => job.convert_notes.push(format!("{file} 没能另存：{e}")),
            Ok(outs) => {
                for (f, r) in outs {
                    let name = format!("{stem}.{}", f.key());
                    match r.and_then(|b| std::fs::write(folder.join(&name), b).map_err(|e| format!("没能写：{e}"))) {
                        Ok(()) => made.push(name),
                        Err(e) => job.convert_notes.push(format!("{file} 没能另存为 {}：{e}", f.name())),
                    }
                }
            }
        }
    }
    job.converted = true;
    event(job, format!("另存为 {}：{} 个文件", names.join("、"), made.len()));
    made
}

/// Days since a "YYYY-MM-DD" date (SQLite's calendar; 0 when it can't tell).
fn days_since(date: &str) -> f64 {
    rusqlite::Connection::open_in_memory().and_then(|c| c.query_row("SELECT julianday('now', 'localtime') - julianday(?1)", [date], |r| r.get::<_, Option<f64>>(0))).ok().flatten().unwrap_or(0.0)
}

/// The text for the converter: assets/ images given back their links; images it can't reach (no link, a
/// local path) taken out. Returns it and how many were taken out.
fn linked(text: &str, links: &[(String, String)]) -> (String, usize) {
    let mut out = text.to_string();
    for (name, url) in links {
        out = out.replace(&format!("{ASSETS}/{name}"), url);
    }
    let reachable = |target: &str| target.starts_with("https://") || target.starts_with("http://");
    let mut left_out = 0;
    // ![alt](target) and \includegraphics[…]{target}
    let mut result = String::with_capacity(out.len());
    let mut rest = out.as_str();
    loop {
        let md = rest.find("![");
        let tex = rest.find("\\includegraphics");
        let Some(at) = [md, tex].into_iter().flatten().min() else { break };
        result.push_str(&rest[..at]);
        let tail = &rest[at..];
        let span = if Some(at) == md {
            tail.find("](").and_then(|o| tail[o + 2..].find(')').map(|c| (o + 2, o + 2 + c, o + 3 + c))).filter(|(_, _, end)| !tail[..*end].contains('\n'))
        } else {
            // past the options: \includegraphics[alt={},max width=\textwidth]{target}
            let mut o = "\\includegraphics".len();
            if tail[o..].starts_with('[') {
                let mut depth = 0;
                o = tail[o..].char_indices().find(|(_, c)| {
                    depth += match c { '{' => 1, '}' => -1, _ => 0 };
                    *c == ']' && depth == 0
                }).map_or(tail.len(), |(e, _)| o + e + 1);
            }
            tail[o..].starts_with('{').then_some(o).and_then(|o| tail[o + 1..].find('}').map(|c| (o + 1, o + 1 + c, o + 2 + c))).filter(|(_, _, end)| !tail[..*end].contains('\n'))
        };
        match span {
            Some((a, b, end)) => {
                if reachable(tail[a..b].trim()) {
                    result.push_str(&tail[..end]);
                } else {
                    left_out += 1;
                }
                rest = &tail[end..];
            }
            None => {
                result.push_str(&tail[..2]);
                rest = &tail[2..];
            }
        }
    }
    result.push_str(rest);
    (result, left_out)
}

/// Mathpix's converter: submit, wait for each format (5 minutes at most), fetch each.
async fn mathpix_convert(keys: &mathpix::Keys, mmd: &str, formats: &[Format]) -> Result<Vec<(Format, Result<Vec<u8>, String>)>, String> {
    let wanted: Vec<&str> = formats.iter().map(|f| f.key()).collect();
    let id = mathpix::convert(keys, mmd, &wanted).await?;
    let deadline = Instant::now() + Duration::from_secs(300);
    let states = loop {
        let states = mathpix::conversion(keys, &id).await?;
        let state = |k: &str| states.iter().find(|(f, _)| f == k).map_or("processing", |(_, s)| s.as_str());
        if wanted.iter().all(|k| state(k) != "processing") || Instant::now() > deadline {
            break states;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    let mut out = vec![];
    for f in formats {
        let state = states.iter().find(|(k, _)| k == f.key()).map_or("processing", |(_, s)| s.as_str());
        let r = match state {
            "completed" => {
                let mut got = Err("Mathpix 的文件一直没准备好".to_string());
                for _ in 0..20 {
                    match mathpix::converted(keys, &id, f.key()).await {
                        Ok(Some(b)) => {
                            got = Ok(b);
                            break;
                        }
                        Ok(None) => tokio::time::sleep(Duration::from_secs(2)).await,
                        Err(e) => {
                            got = Err(e);
                            break;
                        }
                    }
                }
                got
            }
            "processing" => Err("Mathpix 5 分钟还没转好".into()),
            _ => Err("Mathpix 没能转出来".into()),
        };
        out.push((*f, r));
    }
    Ok(out)
}

/// The mock's stand-ins: a real Word file (小拾's writer), a small PDF, zips holding the text.
fn mock_convert(mmd: &str, formats: &[Format]) -> Result<Vec<(Format, Result<Vec<u8>, String>)>, String> {
    let zip_one = |name: &str, body: &str| -> Result<Vec<u8>, String> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w.start_file(name, zip::write::SimpleFileOptions::default()).map_err(|e| e.to_string())?;
        w.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
        Ok(w.finish().map_err(|e| e.to_string())?.into_inner())
    };
    Ok(formats
        .iter()
        .map(|f| {
            let r = match f {
                Format::Docx => crate::picky::word::markdown_to_docx(mmd, &[]),
                Format::Pdf => Ok(mock_pdf()),
                Format::Latex => zip_one("document.tex", &format!("% 模拟转出的 LaTeX\n{mmd}")),
                Format::Html => zip_one("index.html", &format!("<!doctype html><meta charset=\"utf-8\"><pre>{}</pre>", mmd.replace('&', "&amp;").replace('<', "&lt;"))),
            };
            (*f, r)
        })
        .collect())
}

/// A one-page PDF saying it's the mock's.
fn mock_pdf() -> Vec<u8> {
    let content = "BT /F1 18 Tf 72 720 Td (Pub2MD mock PDF) Tj ET";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![];
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

/// A result for the page's 预览: its assets/ images inlined as data: links (each up to 8 MB, 40 MB in all).
pub fn inline_images(text: &str, folder: &Path) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut total = 0usize;
    let prefix = format!("{ASSETS}/");
    while let Some(i) = rest.find(&prefix) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.find(|c: char| c.is_whitespace() || ")}\"'<>]".contains(c)).unwrap_or(tail.len());
        let path = &tail[..end];
        let name = &path[prefix.len()..];
        let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
        let mime = match ext.as_str() {
            "jpg" | "jpeg" => Some("image/jpeg"),
            "png" => Some("image/png"),
            "gif" => Some("image/gif"),
            "webp" => Some("image/webp"),
            "svg" => Some("image/svg+xml"),
            _ => None,
        };
        let data = (!name.contains("..") && !name.contains('/')).then(|| std::fs::read(folder.join(ASSETS).join(name)).ok()).flatten().filter(|d| d.len() <= 8_000_000 && total + d.len() <= 40_000_000);
        match (mime, data) {
            (Some(m), Some(d)) => {
                total += d.len();
                out.push_str(&format!("data:{m};base64,{}", crate::picky::office::base64(&d)));
            }
            _ => out.push_str(path),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// What the two models are told with these settings (the page's 提示词): (who · what, the rules), marks
/// left off. `web`: 联网查术语 as it would be.
pub fn prompts_shown(s: &Settings, web: bool) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    if s.steps == Steps::Extract {
        return out;
    }
    let translate = s.steps == Steps::Translate;
    let w = prompts::wishes(&s.style, &s.domains, &s.prompt);
    let (from, to) = s.lang.says();
    let mut add = |who: &str, text: String| out.push((who.to_string(), prompts::unmarked(&text).to_string()));
    add("Worker · 整理", prompts::worker_system(s.md, s.english_only, Part::Body, true));
    if translate {
        add("Worker · 翻译", if s.lang.script().is_some() { prompts::script_system(s.lang.script() == Some(true), &w) } else { prompts::translate_system(from, to, s.md, &w, false) });
    }
    let b = Budget::for_window(131_072, TARGET);
    add("Supervisor · 审定切块", prompts::plan_system(b.target, b.max));
    if translate && s.lang == Lang::EnZh {
        add("Supervisor · 术语表", prompts::terms_system(from, to, &w, web));
    }
    add("Supervisor · 出错排查", prompts::fix_system(false, s.md));
    if s.refine {
        add("Supervisor · 精修", prompts::review_system(translate, s.md));
    }
    if translate {
        add("Supervisor · 通读", prompts::read_system());
    }
    out
}

pub fn log(job: &Job, how: &str) -> String {
    let kind = match job.kind {
        Some(Kind::Pdf) => "PDF",
        Some(Kind::Word) => "Word",
        Some(Kind::Images) => "图片",
        Some(Kind::Text) => "文本",
        None => "文件",
    };
    let translating = job.steps == Steps::Translate;
    let files: Vec<&str> = job.sources.iter().map(|s| s.rsplit('/').next().unwrap_or(s)).collect();
    let mut s = format!("# 工作日志 · {}\n\n", job.name);
    s.push_str(&format!("- 结果：{how}\n"));
    s.push_str(&format!("- 文件：{}（{kind}，{} 页）\n", files.join("、"), job.pages));
    s.push_str(&format!("- 开始：{}；用时 {}（停下的时间不算）\n", job.started, duration(job.elapsed)));
    let steps = match job.steps {
        Steps::Extract => "只提取",
        Steps::Clean => "提取 + 整理",
        Steps::Translate if job.kind == Some(Kind::Text) => "翻译",
        Steps::Translate => "提取 + 整理 + 翻译",
    };
    s.push_str(&format!("- 做了：{steps}；输出 {}；双语内容{}；精修{}\n", if job.md { "Markdown" } else { "MMD" }, if job.english_only { "只留英文" } else { "都留" }, if job.refine { "开" } else { "关" }));
    if translating {
        let (from, to) = job.lang.says();
        let pick = |list: &[(&'static str, &'static str, &'static str)], k: &str| -> &'static str { list.iter().find(|x| x.0 == k).map_or("", |x| x.1) };
        let skip = kept_parts(job);
        s.push_str(&format!("- 翻译：{from} → {to}；文体 {}；领域 {}；不翻：{}\n", pick(super::STYLES, &job.style), super::domain_names(&job.domains), if skip.is_empty() { "无".to_string() } else { skip.join("、") }));
        if !job.prompt.trim().is_empty() {
            s.push_str(&format!("- 自己写的要求：{}\n", job.prompt.trim()));
        }
        if needs_terms(job) {
            s.push_str(&format!("- 术语表：{}\n", if job.terms_note.is_empty() { "还没定" } else { &job.terms_note }));
        }
    }
    if !job.formats.is_empty() {
        s.push_str(&format!("- 另存为：{}\n", job.formats.iter().map(|f| f.name()).collect::<Vec<_>>().join("、")));
    }
    if job.kind != Some(Kind::Text) {
        s.push_str(&format!("- Mathpix：{}\n", if job.reused { "文件和上次的一样，用了上次的结果" } else if job.pdf_id.is_some() { "上传了一次" } else { "没有上传（模拟）" }));
    }
    if job.steps != Steps::Extract {
        if !job.models.is_empty() {
            s.push_str(&format!("- 模型：{}\n", job.models));
        }
        s.push_str(&format!("- 请求：Worker {} 次，Supervisor {} 次，共 {} 次\n", job.worker_requests, job.supervisor_requests, job.worker_requests + job.supervisor_requests));
    }
    if !job.chunks.is_empty() {
        s.push_str(if translating { "\n## 切块\n\n| 块 | 原文页 | 部分 | 约词元 | 请求 | 用时 | 整理 | 翻译 |\n|---|---|---|---|---|---|---|---|\n" } else { "\n## 切块\n\n| 块 | 原文页 | 部分 | 约词元 | 请求 | 用时 | 整理 |\n|---|---|---|---|---|---|---|\n" });
        for (i, c) in job.chunks.iter().enumerate() {
            let pages = if c.pages.0 == 0 { "—".to_string() } else if c.pages.0 == c.pages.1 { c.pages.0.to_string() } else { format!("{}～{}", c.pages.0, c.pages.1) };
            let article = if job.articles.len() > 1 { format!("第 {} 篇 · ", c.chunk.article) } else { String::new() };
            let clean = if job.kind == Some(Kind::Text) { "—" } else { state_name(c.state, c.by_supervisor) };
            s.push_str(&format!("| {} | {pages} | {article}{} | {} | {} | {} | {clean} |", i + 1, c.chunk.part.name(), c.tokens, c.requests, duration(c.seconds)));
            if translating {
                s.push_str(&format!(" {} |", state_name(c.tr, false)));
            }
            s.push('\n');
        }
        let detail: Vec<String> = job
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.chunk.note.is_empty() || !c.notes.is_empty())
            .map(|(i, c)| {
                let mut line = format!("- 第 {} 块", i + 1);
                if !c.chunk.note.is_empty() {
                    line.push_str(&format!("：交代「{}」", c.chunk.note));
                }
                if !c.notes.is_empty() {
                    line.push_str(&format!("；经过：{}", c.notes.join("；")));
                }
                line
            })
            .collect();
        if !detail.is_empty() {
            s.push_str(&format!("\n## 每块的交代和经过\n\n{}\n", detail.join("\n")));
        }
    }
    if job.articles.len() > 1 {
        s.push_str(&format!("\n## 文章\n\n{}\n", job.articles.iter().map(|a| format!("- 第 {} 篇：{}", a.article, a.title)).collect::<Vec<_>>().join("\n")));
    }
    if !job.plan_notes.is_empty() {
        s.push_str(&format!("\n## Supervisor 的切法里没采用的\n\n{}\n", job.plan_notes.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n")));
    }
    if !job.read_notes.is_empty() {
        s.push_str(&format!("\n## 通读\n\n{}\n", job.read_notes.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n")));
    }
    if !job.convert_notes.is_empty() {
        s.push_str(&format!("\n## 另存为\n\n{}\n", job.convert_notes.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n")));
    }
    s.push_str(&format!("\n## 经过\n\n{}\n", job.events.iter().map(|e| format!("- {e}")).collect::<Vec<_>>().join("\n")));
    s
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::mmd::tests::PAPER;
    use super::*;
    use crate::picky::cloud::Think;

    fn mock(window: i64) -> Route {
        Route { target: Target::Mock { thinking: false }, model: "local:mock".into(), name: "Mock".into(), window, think: Think::Switch }
    }

    fn models(parallel: usize) -> Models {
        Models { worker: mock(32_768), sup: mock(32_768), wl: Level::Off, sl: Level::High, parallel, backup: None }
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn unpack_points_images_at_assets() {
        let crop = "https://cdn.mathpix.com/cropped/xyz-1.jpg?height=396&width=423&top_left_y=172&top_left_x=56";
        let text = format!("\\includegraphics[max width=\\textwidth]{{https://cdn.mathpix.com/cropped/abc-03.jpg?height=1127&width=775}}\n\n![](https://cdn.mathpix.com/cropped/missing.jpg?x=1)\n\n![](images/def.png)\n\n![]({crop})");
        // Mathpix's zip names a cropped image by its link's numbers
        let zip = zip_of(&[("doc/doc.mmd", b"zip text"), ("doc/images/abc-03.jpg", b"jpg"), ("doc/images/def.png", b"png"), ("doc/images/xyz-1_396_423_172_56.jpg", b"jpg")]);
        let (t, images, links) = unpack(&zip, Some(text)).unwrap();
        assert!(unpack(&zip_of(&[("doc/doc.mmd", b"![](./images/def.png)"), ("doc/images/def.png", b"png")]), None).unwrap().0 == "![](assets/def.png)");
        assert_eq!(links, [("abc-03.jpg".to_string(), "https://cdn.mathpix.com/cropped/abc-03.jpg?height=1127&width=775".to_string()), ("xyz-1_396_423_172_56.jpg".to_string(), crop.to_string())]);
        assert!(t.contains("{assets/abc-03.jpg}") && t.contains("(https://cdn.mathpix.com/cropped/missing.jpg?x=1)") && t.contains("(assets/def.png)") && t.contains("(assets/xyz-1_396_423_172_56.jpg)"), "{t}");
        assert_eq!(images.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["abc-03.jpg", "def.png", "xyz-1_396_423_172_56.jpg"]);
        // no downloaded text: the zip's
        assert_eq!(unpack(&zip, None).unwrap().0, "zip text");
        assert!(unpack(b"not a zip", None).is_err());
        assert!(unpack(&zip_of(&[("a.jpg", b"x")]), None).unwrap_err().contains("没有文字"));
    }

    #[test]
    fn a_step_blocked_by_the_content_check_goes_to_the_backup() {
        // the mock turns 审核测试 down for a model that thinks
        let thinker = |model: &str| Route { target: Target::Mock { thinking: true }, model: model.into(), ..mock(32_768) };
        let backup = Route { model: "cloud:deepseek-flash".into(), name: "DeepSeek Flash".into(), ..mock(32_768) };
        let dir = crate::db::tests::temp_dir("pub2md-inspected");
        let text = PAPER.replacen("Attention Is All You Need", "Attention Is All You Need 审核测试", 1);
        let (mut job, blocks) = job_for(&dir, &text, Steps::Translate);
        let mut m = Models { worker: mock(32_768), sup: thinker("cloud:qwen"), wl: Level::Off, sl: Level::High, parallel: 4, backup: Some(backup.clone()) };
        tauri::async_runtime::block_on(plan(&Report::none(), &dir, &mut job, &blocks, &m)).unwrap();
        assert!(job.planned && job.events.iter().any(|e| e.ends_with("审定切块被阿里云的内容审核拦下，改用备用模型 DeepSeek Flash")) && (job.worker_requests, job.supervisor_requests) == (0, 2), "{:?}", job.events);
        assert!(matches!(tauri::async_runtime::block_on(terms_task(&job, &text, &m, Lookup::Off)), Finished::Terms(Ok((t, how)), true) if !t.is_empty() && how.starts_with("定术语表被阿里云")));
        // 不用 (or the backup is the model turned down): nothing else to ask
        m.backup = None;
        assert!(matches!(tauri::async_runtime::block_on(terms_task(&job, &text, &m, Lookup::Off)), Finished::Terms(Err(e), false) if e == crate::picky::provider::failure::INSPECTED));
        m.backup = Some(m.sup.clone());
        assert!(m.backup_for(&m.sup).is_none());
        // the Worker turned down: the backup does the chunk
        let dir = crate::db::tests::temp_dir("pub2md-inspected-worker");
        let (mut job, blocks) = job_for(&dir, &text, Steps::Clean);
        let m = Models { worker: thinker("cloud:qwen-flash"), sup: mock(32_768), wl: Level::High, sl: Level::Off, parallel: 4, backup: Some(backup) };
        run_mock(&dir, &mut job, &blocks, m).unwrap();
        let first = &job.chunks[0];
        assert!(job.chunks.iter().all(|c| c.state == State::Done) && !first.by_supervisor && first.notes == ["整理被阿里云的内容审核拦下，这一块改用备用模型 DeepSeek Flash"], "{:?}", first.notes);
    }

    /// Tokens each route used (by its name) in a real run: (requests, prompt, completion).
    static USED: std::sync::Mutex<Vec<(String, i64, i64, i64)>> = std::sync::Mutex::new(Vec::new());

    pub(crate) fn used(route: &str, prompt: i64, completion: i64) {
        // each request as it comes (a watcher can add up the cost while it runs)
        eprintln!("[用量] {route} 输入 {prompt} 输出 {completion}");
        let mut u = USED.lock().unwrap_or_else(|e| e.into_inner());
        match u.iter_mut().find(|x| x.0 == route) {
            Some(x) => (x.1, x.2, x.3) = (x.1 + 1, x.2 + prompt, x.3 + completion),
            None => u.push((route.to_string(), 1, prompt, completion)),
        }
    }

    fn job_for(dir: &Path, text: &str, steps: Steps) -> (Job, Arc<Vec<Block>>) {
        std::fs::write(dir.join(ORIGINAL), text).unwrap();
        std::fs::create_dir_all(work_dir(dir)).unwrap();
        let (blocks, pages) = mmd::blocks(text);
        let s = Settings::default();
        let job = Job { name: "测试".into(), kind: Some(Kind::Pdf), steps, extracted: true, pages, started: "2026-10-21 10:00".into(), style: s.style, domains: s.domains, skip: s.skip, ..Default::default() };
        (job, Arc::new(blocks))
    }

    fn one_chunk(job: &mut Job, blocks: &[Block]) {
        job.chunks = vec![new_chunk(job, blocks, Chunk { start: 0, end: blocks.len(), part: Part::Body, article: 1, note: String::new() })];
        job.planned = true;
    }

    /// The steps after extraction, as steps() runs them, with the mock models.
    fn run_mock(dir: &Path, job: &mut Job, blocks: &Arc<Vec<Block>>, m: Models) -> Result<Vec<String>, String> {
        tauri::async_runtime::block_on(async {
            let text = std::fs::read_to_string(original(dir, job)).unwrap();
            let mut set = JoinSet::new();
            if needs_terms(job) && job.terms.is_none() {
                set.spawn(terms_task(job, &text, &m, Lookup::Off));
                job.supervisor_requests += 1;
            }
            if !job.planned {
                plan(&Report::none(), dir, job, blocks, &m).await?;
            }
            work(&Report::none(), dir, job, blocks, &m, set).await?;
            if job.steps == Steps::Translate && !job.read_through {
                read_through(&Report::none(), dir, job, &m).await?;
            }
            assemble(dir, job)
        })
    }

    #[test]
    fn clean_with_the_mock_models() {
        let dir = crate::db::tests::temp_dir("pub2md-flow");
        let (mut job, blocks) = job_for(&dir, PAPER, Steps::Clean);
        tauri::async_runtime::block_on(plan(&Report::none(), &dir, &mut job, &blocks, &Models { worker: mock(4_000), sup: mock(32_768), wl: Level::Off, sl: Level::High, parallel: 4, backup: None })).unwrap();
        assert!(job.planned && job.chunks.len() >= 2 && job.supervisor_requests == 1, "{:?}", job.chunks.len());
        assert!(job.chunks.iter().all(|c| c.chunk.note.starts_with("模拟交代")));
        let names = run_mock(&dir, &mut job, &blocks, models(4)).unwrap();
        assert!(job.chunks.iter().all(|c| c.state == State::Done && c.tr == State::Pending));
        assert_eq!(job.worker_requests as usize, job.chunks.len());
        assert_eq!(names, ["整理稿.mmd"]);
        let out = std::fs::read_to_string(dir.join("整理稿.mmd")).unwrap();
        // the mock Worker gives each chunk back: everything is there, in order, without position lines
        assert!(out.starts_with("\\title{") && out.contains("[2] Dzmitry Bahdanau") && !out.contains("【原文第"));
        assert!(out.find("1 Introduction").unwrap() < out.find("7 Conclusion").unwrap());
        let l = log(&job, "完成");
        assert!(l.contains("| 1 | 1 |") && l.contains("Worker") && l.contains("切成") && !l.contains("| 翻译 |"), "{l}");
        // saved as it went: a reload has the same chunks
        assert_eq!(load_job(&dir).unwrap().chunks, job.chunks);
    }

    #[test]
    fn replies_cut_off_and_room_for_thinking() {
        assert_eq!((fixes_cap(10), fixes_cap(26), fixes_cap(40)), (6, 8, 13));
        let whole = "好的\n{\"terms\":[\n{\"source\":\"Smithsonian\",\"target\":\"史密森学会\",\"note\":\"\"}\n]}";
        assert_eq!(terms_in(whole), Some((vec![Term { source: "Smithsonian".into(), target: "史密森学会".into(), note: String::new() }], true)));
        // cut off inside the third entry; a brace and a quote inside strings don't confuse it
        let cut = "{\"terms\":[\n{\"source\":\"DJT\",\"target\":\"DJT\",\"note\":\"车牌 {\\\"DJT\\\"}\"},\n{\"source\":\"Times\",\"target\":\"《纽约时报》\"},\n{\"source\":\"divisive\",\"target\":\"分裂";
        let (terms, whole) = terms_in(cut).unwrap();
        assert_eq!((terms.iter().map(|t| t.target.as_str()).collect::<Vec<_>>(), whole), (vec!["DJT", "《纽约时报》"], false));
        assert_eq!((terms[0].note.as_str(), terms_in("读不懂"), terms_level(Level::High), terms_level(Level::Off)), ("车牌 {\"DJT\"}", None, Level::Low, Level::Off));
        // reply room: thinking gets its share
        use crate::picky::cloud::Think;
        let r = |think: Think| Route { think, ..mock(32_768) };
        assert_eq!([reply_room(&r(Think::Switch), 1000, Level::Off), reply_room(&r(Think::Switch), 1000, Level::Medium), reply_room(&r(Think::Never), 1000, Level::High)], [1000, 17_384, 1000]);
        // GLM-5.3 thinks at 关 too
        assert_eq!(reply_room(&r(Think::Always), 1000, Level::Off), 9_192);
    }

    #[test]
    fn translate_with_terms_and_read_through() {
        let dir = crate::db::tests::temp_dir("pub2md-translate");
        let (mut job, blocks) = job_for(&dir, PAPER, Steps::Translate);
        let names = run_mock(&dir, &mut job, &blocks, models(3)).unwrap();
        assert_eq!(names, ["译文.mmd", "整理稿.mmd", TERMS_FILE]);
        let tr = std::fs::read_to_string(dir.join("译文.mmd")).unwrap();
        // the body translated (the mock turns words into 「译」), the references kept as they are
        assert!(tr.contains("译") && tr.contains("[2] Dzmitry Bahdanau. Neural machine translation."), "{tr}");
        assert!(job.chunks.iter().any(|c| c.chunk.part == Part::References && c.tr == State::Skipped));
        assert!(job.chunks.iter().filter(|c| c.chunk.part != Part::References).all(|c| c.tr == State::Done), "{:?}", job.chunks.iter().map(|c| (c.chunk.part, c.tr, c.notes.clone())).collect::<Vec<_>>());
        assert_eq!(job.terms.as_ref().map(|t| t.len()), Some(1));
        assert!(std::fs::read_to_string(dir.join(TERMS_FILE)).unwrap().contains("| Transformer | Transformer |"));
        // plan, term list, read-through: Supervisor; 整理 and 翻译 (not the references): Worker
        let translated = job.chunks.iter().filter(|c| c.tr == State::Done).count();
        assert_eq!((job.supervisor_requests, job.worker_requests as usize), (3, job.chunks.len() + translated));
        assert!(job.read_through && job.read_notes[0].starts_with("读了 1 段，改了 0 处"));
        // the authors and affiliations kept as they are: no fix there (Sol translated one)
        let front = "\\title{\n机器学习教程\n}\n\n\\author{\nF. Pargent \\({ }^{1}\\) \\\\\n\\({ }^{1}\\) Department Psychology, LMU Munich\n}\n\n摘要。";
        assert!(in_author(front, "Department Psychology, LMU Munich") && !in_author(front, "摘要") && !in_author(front, "机器学习教程"));
        let mut tr = front.replace("Department Psychology, LMU Munich", "慕尼黑大学心理学系");
        assert!(keep_authors(front, &mut tr) && tr == front && !keep_authors(front, &mut tr));
        assert!(prompts::read_message("", "译文", &[], &kept_parts(&job)).contains("【照原文不翻的部分】作者和单位、致谢、参考文献、附录"));
        // an article a chunk: nothing across chunks to read
        let asked = job.supervisor_requests;
        let mut each = Job { read_through: false, read_notes: vec![], ..job.clone() };
        for (i, c) in each.chunks.iter_mut().enumerate() {
            c.chunk.article = i + 1;
        }
        tauri::async_runtime::block_on(read_through(&Report::none(), &dir, &mut each, &models(3))).unwrap();
        assert_eq!((each.supervisor_requests, each.read_notes[0].as_str()), (asked, "每篇文章只有一块，也没有漏译的英文词，不用通读"));
        let l = log(&job, "完成");
        assert!(l.contains("| 翻译 |") && l.contains("不翻") && l.contains("## 通读") && l.contains("术语表：1 条"), "{l}");
    }

    #[test]
    fn refine_reviews_every_chunk() {
        let dir = crate::db::tests::temp_dir("pub2md-refine");
        let (mut job, blocks) = job_for(&dir, "First paragraph of the text, long enough to be a chunk.\n\nSecond paragraph.", Steps::Translate);
        job.refine = true;
        one_chunk(&mut job, &blocks);
        run_mock(&dir, &mut job, &blocks, models(2)).unwrap();
        let c = &job.chunks[0];
        assert!(c.reviewed && c.tr_reviewed && c.notes.iter().any(|n| n.starts_with("整理的精修：改了 0 处")) && c.notes.iter().any(|n| n.starts_with("翻译的精修")));
        // term list, two reviews; one chunk with nothing left in English: no read-through
        assert_eq!((job.supervisor_requests, job.worker_requests), (3, 2));
        assert!(job.read_through && job.read_notes[0].starts_with("只有一块"));
    }

    #[test]
    fn only_translated_text_and_script() {
        let dir = crate::db::tests::temp_dir("pub2md-text");
        let md = dir.join("note.md");
        std::fs::write(&md, "# A Note\n\nThis is a short English note about the model.").unwrap();
        let mut job = Job { name: "note".into(), sources: vec![md.display().to_string()], kind: Some(Kind::Text), steps: Steps::Translate, md: true, ..Default::default() };
        std::fs::create_dir_all(work_dir(&dir)).unwrap();
        take_text(&dir, &mut job).unwrap();
        let blocks = Arc::new(mmd::blocks(&std::fs::read_to_string(original(&dir, &job)).unwrap()).0);
        let names = run_mock(&dir, &mut job, &blocks, models(2)).unwrap();
        assert_eq!(names, ["译文.md", TERMS_FILE]);
        assert!(std::fs::read_to_string(dir.join("译文.md")).unwrap().starts_with("# 译 译"));
        // no 整理: only the translations (and the term list, the cut, the read-through)
        assert_eq!(job.worker_requests as usize, job.chunks.len());
        assert!(log(&job, "完成").contains("做了：翻译"));
        // 简繁: OpenCC, then the model; no term list
        let dir = crate::db::tests::temp_dir("pub2md-script");
        let (mut job, blocks) = job_for(&dir, "开放中文转换是完全由 Rust 实现的。这一段写得长一点，好让检查有足够的字数可以比较长短。", Steps::Translate);
        job.lang = Lang::ZhHant;
        one_chunk(&mut job, &blocks);
        run_mock(&dir, &mut job, &blocks, models(1)).unwrap();
        let tr = std::fs::read_to_string(dir.join("译文.mmd")).unwrap();
        assert!(tr.starts_with("開放中文轉換是完全由 Rust 實現的。"), "{tr}");
        // no term list for 简繁
        assert!(job.terms.is_none() && !dir.join(TERMS_FILE).exists());
    }

    #[test]
    fn a_failing_check_goes_to_the_supervisor() {
        let dir = crate::db::tests::temp_dir("pub2md-fix");
        let text = format!("{}\n\n{}", "检查测试：这是一段比较长的文字，用来让模拟的整理结果第一次少掉一大截。".repeat(12), "第二段，也写得长一点，保证有足够的字数。".repeat(10));
        let (mut job, blocks) = job_for(&dir, &text, Steps::Clean);
        one_chunk(&mut job, &blocks);
        run_mock(&dir, &mut job, &blocks, models(2)).unwrap();
        let c = &job.chunks[0];
        assert_eq!((c.state, c.requests, c.fixes, job.supervisor_requests, job.worker_requests), (State::Done, 3, 1, 1, 2));
        assert!(c.notes[0].starts_with("整理的检查没过：比原文短很多") && c.notes[1].contains("带提示重做"), "{:?}", c.notes);
        // the same in 翻译
        let dir = crate::db::tests::temp_dir("pub2md-trfix");
        let text = format!("检查测试 {}", "The model works well on long inputs and short ones. ".repeat(12));
        let (mut job, blocks) = job_for(&dir, &text, Steps::Translate);
        job.terms = Some(vec![]);
        one_chunk(&mut job, &blocks);
        run_mock(&dir, &mut job, &blocks, models(1)).unwrap();
        let c = &job.chunks[0];
        assert_eq!((c.tr, c.tr_fixes), (State::Done, 1));
        assert!(c.notes.iter().any(|n| n.starts_with("翻译的检查没过：译文比原文短很多")) && c.notes.iter().any(|n| n.contains("带提示重译")), "{:?}", c.notes);
    }

    #[test]
    fn a_failing_request_stops_and_keeps_the_rest() {
        let dir = crate::db::tests::temp_dir("pub2md-stop");
        let text = "First paragraph that is fine.\n\n\\section*{2 Next}\n\n失败测试 in this one.";
        let (mut job, blocks) = job_for(&dir, text, Steps::Clean);
        job.chunks = vec![
            new_chunk(&job, &blocks, Chunk { start: 0, end: 1, part: Part::Body, article: 1, note: String::new() }),
            new_chunk(&job, &blocks, Chunk { start: 1, end: 3, part: Part::Body, article: 1, note: String::new() }),
        ];
        job.chunks[1].pages = (1, 1);
        job.planned = true;
        let e = run_mock(&dir, &mut job, &blocks, models(1)).unwrap_err();
        assert!(e.starts_with("第 2 块") && e.contains("请求失败"), "{e}");
        assert_eq!((job.chunks[0].state, job.chunks[1].state, job.chunks[1].requests), (State::Done, State::Pending, 3));
        assert!(job.chunks[1].notes[0].contains("秒后再试") && job.chunks[1].notes[1].contains("秒后再试"));
        // 继续 picks up the saved job: only the second chunk is left
        assert_eq!(load_job(&dir).unwrap().chunks.iter().filter(|c| c.state == State::Pending).count(), 1);
        assert!(log(&job, "出错停下").contains("还没做完"));
        // with a backup model: it gets one go before the stop
        let dir = crate::db::tests::temp_dir("pub2md-stop-backup");
        let (mut job, blocks) = job_for(&dir, text, Steps::Clean);
        job.chunks = vec![new_chunk(&job, &blocks, Chunk { start: 1, end: 3, part: Part::Body, article: 1, note: String::new() })];
        job.planned = true;
        let mut m = models(1);
        m.backup = Some(Route { model: "local:backup".into(), name: "备用".into(), ..mock(32_768) });
        assert!(run_mock(&dir, &mut job, &blocks, m).is_err());
        assert_eq!(job.chunks[0].requests, 4);
        assert!(job.chunks[0].notes[2].contains("改用备用模型 备用"), "{:?}", job.chunks[0].notes);
    }

    /// A PDF and two photos through every step but the routes (mock Mathpix, mock models).
    #[test]
    fn files_through_every_step() {
        let dir = crate::db::tests::temp_dir("pub2md-e2e");
        let paper = dir.join("paper.pdf");
        std::fs::write(&paper, pdf::tests::sample_pdf(&["Introduction to the topic.", "Results are good.", "We conclude here."])).unwrap();
        let png = dir.join("b.png");
        std::fs::write(&png, office::tests_png()).unwrap();
        for (sources, kind) in [(vec![paper.clone()], Kind::Pdf), (vec![png.clone(), png.clone()], Kind::Images)] {
            let folder = dir.join(format!("out-{kind:?}"));
            std::fs::create_dir_all(work_dir(&folder)).unwrap();
            let mut job = Job { name: "e2e".into(), sources: sources.iter().map(|p| p.display().to_string()).collect(), kind: Some(kind), steps: Steps::Translate, skip: Settings::default().skip, ..Default::default() };
            tauri::async_runtime::block_on(async {
                extract(&Report::none(), &folder, &mut job, Extraction { keys: None, earlier: None }).await.unwrap();
                assert!(job.extracted && folder.join(ORIGINAL).is_file() && folder.join(RAW_ZIP).is_file());
                assert_eq!(job.pages, if kind == Kind::Pdf { 3 } else { 2 });
                job.rendered = render(&folder, &job).await.unwrap();
                assert_eq!(job.rendered, job.pages);
            });
            let text = std::fs::read_to_string(folder.join(ORIGINAL)).unwrap();
            let blocks = Arc::new(mmd::blocks(&text).0);
            let names = run_mock(&folder, &mut job, &blocks, models(4)).unwrap();
            let out = std::fs::read_to_string(folder.join(&names[1])).unwrap();
            if kind == Kind::Pdf {
                assert!(out.contains("Introduction to the topic.") && out.contains("We conclude here."), "{out}");
                assert!(std::fs::read_to_string(folder.join(&names[0])).unwrap().contains("译 译 译 译."));
            } else {
                assert!(out.contains("模拟识别的图片：b.png"), "{out}");
            }
            assert!(work_dir(&folder).join("pages").join(pdf::page_name(1)).is_file());
        }
        // the same files again: the earlier Mathpix result is used, nothing is uploaded
        let folder = dir.join("again");
        std::fs::create_dir_all(work_dir(&folder)).unwrap();
        let mut job = Job { name: "again".into(), sources: vec![paper.display().to_string()], kind: Some(Kind::Pdf), ..Default::default() };
        let earlier = dir.join("out-Pdf").join(RAW_ZIP);
        tauri::async_runtime::block_on(extract(&Report::none(), &folder, &mut job, Extraction { keys: None, earlier: Some(Earlier { zip: earlier, extracted: "2026-10-20".into(), images: vec![("a.jpg".into(), "https://cdn.mathpix.com/cropped/a.jpg".into())] }) })).unwrap();
        assert!(job.reused && job.events.iter().any(|e| e.contains("用了上次的 Mathpix 结果")));
        assert_eq!((job.extracted_on.as_str(), job.images.len()), ("2026-10-20", 1));
        assert!(std::fs::read_to_string(folder.join(ORIGINAL)).unwrap().contains("We conclude here."));
    }

    #[test]
    fn another_format_with_the_mock() {
        let dir = crate::db::tests::temp_dir("pub2md-convert");
        let text = "\\section*{标题}\n\n正文 \\(x^2\\)。\n\n![](assets/a.jpg)\n\n\\includegraphics[max width=\\textwidth]{figure.png}\n\n![](https://example.com/b.png)";
        std::fs::write(dir.join("译文.mmd"), text).unwrap();
        let (sent, left) = linked(text, &[("a.jpg".into(), "https://cdn.mathpix.com/cropped/a.jpg?x=1".into())]);
        assert!(sent.contains("![](https://cdn.mathpix.com/cropped/a.jpg?x=1)") && sent.contains("(https://example.com/b.png)") && !sent.contains("figure.png") && left == 1, "{sent}");
        // options with braces in them: the target is after them
        let figure = "\\begin{figure}\n\\includegraphics[alt={},max width=\\textwidth]{assets/c.jpg}\n\\caption{说明}\n\\end{figure}";
        let (sent, left) = linked(figure, &[("c.jpg".into(), "https://cdn.mathpix.com/cropped/c.jpg?x=1".into())]);
        assert_eq!((sent.as_str(), left), ("\\begin{figure}\n\\includegraphics[alt={},max width=\\textwidth]{https://cdn.mathpix.com/cropped/c.jpg?x=1}\n\\caption{说明}\n\\end{figure}", 0));
        let (sent, left) = linked(figure, &[]);
        assert!(!sent.contains("includegraphics") && !sent.contains("max width") && sent.contains("\\caption{说明}") && left == 1, "{sent}");
        let mut job = Job { formats: vec![Format::Docx, Format::Pdf, Format::Latex, Format::Html], images: vec![("a.jpg".into(), "https://cdn.mathpix.com/cropped/a.jpg?x=1".into())], extracted_on: clock("%Y-%m-%d"), ..Default::default() };
        let made = tauri::async_runtime::block_on(convert(&Report::none(), &dir, &mut job, None, &["译文.mmd".to_string()]));
        assert_eq!(made, ["译文.docx", "译文.pdf", "译文.tex.zip", "译文.html.zip"]);
        let head = |f: &str| std::fs::read(dir.join(f)).unwrap()[..4].to_vec();
        assert_eq!((head("译文.docx"), head("译文.pdf"), head("译文.html.zip")), (b"PK\x03\x04".to_vec(), b"%PDF".to_vec(), b"PK\x03\x04".to_vec()));
        assert!(job.converted && job.convert_notes.len() == 1 && job.convert_notes[0].contains("1 张图没放进去（不是 Mathpix 识别出的图"));
        // older than 30 days: Mathpix's links are gone, its images stay out too
        let mut old = Job { formats: vec![Format::Docx], images: job.images.clone(), extracted_on: "2026-01-01".into(), ..Default::default() };
        tauri::async_runtime::block_on(convert(&Report::none(), &dir, &mut old, None, &["译文.mmd".to_string()]));
        assert!(old.convert_notes[0].contains("2 张图没放进去（提取已超过 30 天"), "{:?}", old.convert_notes);
        assert!(log(&job, "完成").contains("- 另存为：Word、PDF、LaTeX、HTML") && log(&job, "完成").contains("## 另存为"));
    }

    #[test]
    fn preview_inlines_the_images() {
        let dir = crate::db::tests::temp_dir("pub2md-preview");
        std::fs::create_dir_all(dir.join(ASSETS)).unwrap();
        std::fs::write(dir.join(ASSETS).join("a.jpg"), b"jpgdata").unwrap();
        let t = inline_images("![](assets/a.jpg) \\includegraphics{assets/a.jpg} ![](assets/gone.png) ![](assets/../x.jpg)", &dir);
        assert_eq!(t.matches("data:image/jpeg;base64,anBnZGF0YQ==").count(), 2, "{t}");
        assert!(t.contains("(assets/gone.png)") && t.contains("(assets/../x.jpg)"));
    }

    #[test]
    fn prompts_shown_follow_the_settings() {
        let who = |s: &Settings| prompts_shown(s, true).into_iter().map(|(w, _)| w).collect::<Vec<_>>();
        let s = Settings::default();
        assert_eq!(who(&s), ["Worker · 整理", "Worker · 翻译", "Supervisor · 审定切块", "Supervisor · 术语表", "Supervisor · 出错排查", "Supervisor · 通读"]);
        assert_eq!(who(&Settings { steps: Steps::Clean, refine: true, ..s.clone() }), ["Worker · 整理", "Supervisor · 审定切块", "Supervisor · 出错排查", "Supervisor · 精修"]);
        assert!(who(&Settings { steps: Steps::Extract, ..s.clone() }).is_empty());
        let shown = prompts_shown(&Settings { prompt: "人名保留英文".into(), ..s }, false);
        assert!(shown.iter().all(|(_, t)| !t.starts_with("【")) && shown[1].1.contains("人名保留英文") && shown[3].1.contains("这次不联网"));
    }

    /// Pub2MD with the real Mathpix and real cloud models (ask first: Mathpix charges by the page):
    ///   POMELO_MATHPIX_KEY_FILE=<App ID on the first line, App Key on the second>
    ///   POMELO_CLOUD_KEY_FILE=… [POMELO_CLOUD_SERVICE=…] [POMELO_CLOUD_MODEL=…] [POMELO_CLOUD_THINK=always|never]  (the Worker;
    ///     always: a model that can't stop thinking, GLM-5.3 on OpenRouter)
    ///   [POMELO_SUP_KEY_FILE=… POMELO_SUP_SERVICE=… POMELO_SUP_MODEL=… POMELO_SUP_THINK=…]  (the Supervisor; else the Worker's)
    ///   [POMELO_BACKUP_KEY_FILE=… POMELO_BACKUP_SERVICE=… POMELO_BACKUP_MODEL=…]  (the backup for a content check; none without)
    ///   POMELO_PUB2MD_FILE=<a PDF / Word file / image / text file; images joined by | make one document> [POMELO_PUB2MD_OUT=<folder>]
    ///   [POMELO_PARALLEL=4] [POMELO_STEPS=extract|clean|translate] [POMELO_REFINE=1] [POMELO_WEB=0|1]
    ///   [POMELO_LANG=enzh|hant|hans (简体 → 繁体 / 繁体 → 简体)] [POMELO_STYLE=news…] [POMELO_DOMAIN=cs…]
    ///   [POMELO_WORKER_LEVEL=…] [POMELO_SUP_LEVEL=off|low|medium|high (else 关 / 高)] [POMELO_FORMATS=docx,pdf,tex,html (另存为, by Mathpix)]
    ///   [POMELO_PUB2MD_EARLIER=<an earlier output folder of the same file: its Mathpix result, nothing uploaded>]
    ///   [POMELO_PUB2MD_RESUME=<a stopped run's document folder: 继续 from where it stopped, with the models given here>]
    ///   cargo test real_pub2md -- --ignored --nocapture
    /// Leaves the results and the work log in the folder (a temp one by default) and prints where, the requests,
    /// the time and each model's tokens (用量).
    #[test]
    #[ignore]
    fn real_pub2md() {
        let (Some((target, window)), Ok(file)) = (crate::picky::chat::tests::cloud_target(), std::env::var("POMELO_PUB2MD_FILE")) else {
            eprintln!("needs POMELO_CLOUD_KEY_FILE and POMELO_PUB2MD_FILE (and POMELO_MATHPIX_KEY_FILE but for a text file)");
            return;
        };
        let keys = std::env::var("POMELO_MATHPIX_KEY_FILE").ok().map(|f| {
            let lines: Vec<String> = std::fs::read_to_string(f).expect("the Mathpix key file").lines().map(|l| l.trim().rsplit('=').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()).collect();
            mathpix::Keys { id: lines[0].clone(), key: lines[1].clone() }
        });
        let api = |t: &Target| match t {
            Target::Cloud { model, .. } => format!("{:?} {}", model.service, model.api_model),
            _ => "mock".into(),
        };
        let (worker, sup, backup) = real_routes(target, window);
        let paths: Vec<PathBuf> = file.split('|').map(PathBuf::from).collect();
        let base = std::env::var("POMELO_PUB2MD_OUT").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("pomelo-pub2md-real"));
        // 继续: a stopped run's folder goes on where it stopped, as the page's button does
        let resume = std::env::var("POMELO_PUB2MD_RESUME").ok().map(PathBuf::from);
        let folder = resume.clone().unwrap_or_else(|| unique_dir(&base, &source::name_of(&paths)));
        std::fs::create_dir_all(work_dir(&folder)).unwrap();
        let kind = source::kind_of(&paths);
        let steps = match std::env::var("POMELO_STEPS").as_deref() {
            Ok("extract") => Steps::Extract,
            Ok("clean") => Steps::Clean,
            _ => Steps::Translate,
        };
        let d = Settings::default();
        let web = std::env::var("POMELO_WEB").ok().map(|v| v == "1");
        let lang = match std::env::var("POMELO_LANG").as_deref() {
            Ok("hant") => Lang::ZhHant,
            Ok("hans") => Lang::HantZh,
            _ => Lang::EnZh,
        };
        let formats: Vec<Format> = std::env::var("POMELO_FORMATS").unwrap_or_default().split(',').map(str::trim).filter(|f| !f.is_empty()).filter_map(|f| [Format::Docx, Format::Pdf, Format::Latex, Format::Html].into_iter().find(|x| x.key().starts_with(f))).collect();
        let var = |v: &str, or: String| std::env::var(v).unwrap_or(or);
        let mut job = Job { name: source::name_of(&paths), sources: paths.iter().map(|p| p.display().to_string()).collect(), kind: Some(kind), steps, lang, formats, started: clock("%Y-%m-%d %H:%M"), style: var("POMELO_STYLE", d.style), domains: std::env::var("POMELO_DOMAIN").map(|v| v.split(',').map(|d| d.trim().to_string()).collect()).unwrap_or(d.domains), skip: d.skip, web, refine: std::env::var("POMELO_REFINE").as_deref() == Ok("1"), ..Default::default() };
        if resume.is_some() {
            job = load_job(&folder).unwrap();
            event(&mut job, "继续");
        }
        let (wl, sl) = (level_var("POMELO_WORKER_LEVEL", Level::Off), level_var("POMELO_SUP_LEVEL", Level::High));
        job.models = format!("Worker：{}（{}）　Supervisor：{}（{}）{}", api(&worker.target), wl.name(), api(&sup.target), sl.name(), backup.as_ref().map_or(String::new(), |b| format!("　备用：{}", api(&b.target))));
        let parallel = std::env::var("POMELO_PARALLEL").ok().and_then(|p| p.parse().ok()).unwrap_or(4);
        let say = Report(Arc::new(|c| {
            if let Change::Step(s) = c {
                eprintln!("[pub2md] {s}");
            }
        }));
        let started = Instant::now();
        let m = Models { worker, sup, wl, sl, parallel, backup };
        let result: Result<Vec<String>, String> = tauri::async_runtime::block_on(async {
            if kind == Kind::Text && !job.extracted {
                take_text(&folder, &mut job)?;
            } else if !job.extracted {
                let earlier = std::env::var("POMELO_PUB2MD_EARLIER").ok().map(|f| {
                    // its downloaded text too (with Mathpix's links), as a first run has it
                    let _ = std::fs::create_dir_all(work_dir(&folder).join("mathpix"));
                    let _ = std::fs::copy(work_dir(Path::new(&f)).join("mathpix/extraction.mmd"), work_dir(&folder).join("mathpix/extraction.mmd"));
                    Earlier { zip: PathBuf::from(f).join(RAW_ZIP), extracted: clock("%Y-%m-%d"), images: vec![] }
                });
                extract(&say, &folder, &mut job, Extraction { keys: keys.clone(), earlier }).await?;
                if steps == Steps::Extract {
                    return Ok(vec![]);
                }
                if kind != Kind::Word {
                    job.rendered = render(&folder, &job).await.unwrap_or(0);
                }
            }
            let text = std::fs::read_to_string(original(&folder, &job)).map_err(|e| e.to_string())?;
            let blocks = Arc::new(mmd::blocks(&text).0);
            let mut set = JoinSet::new();
            if needs_terms(&job) && job.terms.is_none() {
                let lookup = search::lookup(job.web, &m.sup, None);
                eprintln!("[pub2md] 术语表：{lookup:?}");
                set.spawn(terms_task(&job, &text, &m, lookup));
                job.supervisor_requests += 1;
            }
            if !job.planned {
                plan(&say, &folder, &mut job, &blocks, &m).await?;
            }
            work(&say, &folder, &mut job, &blocks, &m, set).await?;
            if job.steps == Steps::Translate && !job.read_through {
                read_through(&say, &folder, &mut job, &m).await?;
            }
            let mut written = assemble(&folder, &job)?;
            if !job.formats.is_empty() {
                let results: Vec<String> = written.iter().filter(|f| *f != TERMS_FILE && (f.ends_with(".mmd") || f.ends_with(".md"))).cloned().collect();
                written.extend(convert(&say, &folder, &mut job, keys.as_ref(), &results).await);
            }
            Ok(written)
        });
        job.elapsed += started.elapsed().as_secs();
        let how = match &result {
            Ok(_) => "完成".to_string(),
            Err(e) => format!("出错停下：{e}"),
        };
        let _ = save(&folder, &job);
        std::fs::write(folder.join(LOG), log(&job, &how)).unwrap();
        eprintln!("[pub2md] {how}；{} 块；请求 Worker {} · Supervisor {}；用时 {}\n[pub2md] {}", job.chunks.len(), job.worker_requests, job.supervisor_requests, duration(job.elapsed), folder.display());
        for (route, n, prompt, completion) in USED.lock().unwrap().iter() {
            eprintln!("[pub2md] 用量 {route}：{n} 次，输入 {prompt}，输出 {completion}");
        }
    }

    fn level_var(var: &str, or: Level) -> Level {
        match std::env::var(var).as_deref() {
            Ok("off") => Level::Off,
            Ok("low") => Level::Low,
            Ok("medium") => Level::Medium,
            Ok("high") => Level::High,
            _ => or,
        }
    }

    /// The Worker's route, the Supervisor's (POMELO_SUP_KEY_FILE / _SERVICE / _MODEL, else the Worker's)
    /// and the backup (POMELO_BACKUP_…, none without its key file).
    fn real_routes(target: Target, window: i64) -> (Route, Route, Option<Route>) {
        let route = |t: Target, window: i64, name: &str| {
            let think = if let Target::Cloud { model, .. } = &t { model.thinking } else { crate::picky::cloud::Think::Switch };
            Route { target: t, model: format!("cloud:{name}"), name: name.into(), window, think }
        };
        // cloud_target with POMELO_<who>_… in place of POMELO_CLOUD_…
        let target_of = |who: &str| {
            let mut vars = vec![];
            for (own, var) in [("KEY_FILE", "POMELO_CLOUD_KEY_FILE"), ("SERVICE", "POMELO_CLOUD_SERVICE"), ("MODEL", "POMELO_CLOUD_MODEL"), ("THINK", "POMELO_CLOUD_THINK")] {
                if let Ok(v) = std::env::var(format!("POMELO_{who}_{own}")) {
                    vars.push((var, std::env::var(var).ok()));
                    std::env::set_var(var, v);
                }
            }
            let t = crate::picky::chat::tests::cloud_target().unwrap();
            for (var, old) in vars {
                match old {
                    Some(v) => std::env::set_var(var, v),
                    None => std::env::remove_var(var),
                }
            }
            t
        };
        let (t, w) = target_of("SUP");
        let backup = std::env::var("POMELO_BACKUP_KEY_FILE").is_ok().then(|| target_of("BACKUP")).map(|(t, w)| route(t, w, "备用"));
        (route(target, window, "Worker"), route(t, w, "Supervisor"), backup)
    }

    /// What the Supervisor really answers to the term list and to a failed check (raw: reasoning size,
    /// the reply, why it ended), on an earlier run's 原文.mmd; no Mathpix.
    ///   POMELO_PUB2MD_MMD=<原文.mmd> POMELO_CLOUD_KEY_FILE=… POMELO_SUP_…=… [POMELO_PROBE=terms|fix|plan|read|search] [POMELO_PROBE_PAGES=<.pub2md/pages>] [POMELO_SUP_LEVEL=off|low|medium|high]
    ///   (search: the term list through search::ask, the Supervisor's service searching)
    #[test]
    #[ignore]
    fn real_pub2md_probe() {
        let (Some((target, window)), Ok(file)) = (crate::picky::chat::tests::cloud_target(), std::env::var("POMELO_PUB2MD_MMD")) else { return };
        let (_, sup, _) = real_routes(target, window);
        let level = level_var("POMELO_SUP_LEVEL", Level::High);
        let text = std::fs::read_to_string(&file).unwrap();
        let blocks = mmd::blocks(&text).0;
        let job = Job { name: "probe".into(), steps: Steps::Translate, ..Default::default() };
        let (from, to) = job.lang.says();
        let mut asks = vec![];
        let probe = std::env::var("POMELO_PROBE").unwrap_or_default();
        if probe == "search" {
            // the term list as the job asks it, looking up with the Supervisor's own search
            let level = terms_level(level);
            let system = prompts::terms_system(from, to, &wishes(&job), true);
            let user = prompts::terms_message(&job.name, &fit(&text, 20_000).0, false);
            let started = Instant::now();
            let r = tauri::async_runtime::block_on(search::ask(&sup, level, &system, &user, reply_room(&sup, 8000, level), &Lookup::Native));
            eprintln!("[probe] 术语表（联网）：用时 {} 秒，{}", started.elapsed().as_secs(), r.as_ref().map_or_else(|e| format!("出错：{e}"), |(t, how)| format!("{how}，回答 {} 字，读出 {} 条", t.chars().count(), terms_in(t).map_or(0, |(t, _)| t.len()))));
            if let Ok((t, _)) = r {
                eprintln!("[probe] 回答：\n{t}\n[probe] ——");
            }
            return;
        }
        if probe == "plan" {
            let parts = mmd::parts(&blocks);
            let budget = Budget::for_window(window, TARGET);
            let draft = chunk::draft(&blocks, &parts, budget);
            let outline = chunk::outline(&blocks, &parts, &draft, true);
            asks.push(("审定切块", prompts::plan_system(budget.target, budget.max), prompts::plan_message("probe", mmd::blocks(&text).1, &outline, true), reply_room(&sup, 1500 + 150 * draft.len() as i64, level), vec![], level));
        }
        if probe == "read" {
            // POMELO_PUB2MD_MMD is the translation here, its chunks marked ⟦第 k 块⟧
            asks.push(("通读", prompts::read_system(), prompts::read_message("", &text, &check::leftovers(&text), &[]), reply_room(&sup, 4000, level), vec![], level));
        }
        if probe.is_empty() || probe == "terms" {
            asks.push(("术语表", prompts::terms_system(from, to, &wishes(&job), false), prompts::terms_message(&job.name, &fit(&text, 20_000).0, false), reply_room(&sup, 8000, terms_level(level)), vec![], terms_level(level)));
        }
        if probe.is_empty() || probe == "fix" {
            let c = Chunk { start: 0, end: blocks.len(), ..Default::default() };
            let plain = chunk::source_text(&blocks, &c, false);
            let output: String = blocks.iter().filter(|b| !b.text.contains("\\begin{tabular}")).map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n");
            let issues = check::said(&check::check(&plain, &output, false));
            let images: Vec<Image> = std::env::var("POMELO_PROBE_PAGES").map(|d| (1..=IMAGES_A_CHUNK).filter_map(|n| std::fs::read(PathBuf::from(&d).join(pdf::page_name(n))).ok()).map(|d| Image { media_type: "image/jpeg".into(), data: office::base64(&d) }).collect()).unwrap_or_default();
            asks.push(("排查", prompts::fix_system(false, false), prompts::fix_message(1, 1, Part::Body, Some(&[]), &plain, &output, &issues), reply_room(&sup, 1200, level), images, level));
        }
        tauri::async_runtime::block_on(async {
            for (what, system, user, max, images, level) in asks {
                let n = images.len();
                let mut msg = Msg::new("user", &user);
                msg.images = images;
                let started = Instant::now();
                let (mut reasoning, mut content, mut end, mut usage) = (0usize, String::new(), String::new(), (0, 0));
                let opts = Options { max_tokens: Some(max), thinking: Some(level.thinking()), effort: level.effort(), ..Default::default() };
                let r = provider::stream(sup.target.clone(), vec![Msg::new("system", &system), msg], opts, |e| match e {
                    Event::Reasoning(t) => reasoning += t.chars().count(),
                    Event::Content(t) => content.push_str(&t),
                    Event::Finished(r) => end = r,
                    Event::Usage { prompt, completion } => usage = (prompt, completion),
                    _ => {}
                })
                .await;
                eprintln!("[probe] {what}（{n} 张图）：max {max}，用时 {} 秒，结果 {r:?}，结束 {end}，用量 {usage:?}，思考 {reasoning} 字，回答 {} 字，能读出 JSON：{}", started.elapsed().as_secs(), content.chars().count(), chunk::json_in(&content).is_some());
                eprintln!("[probe] 回答：\n{content}\n[probe] ——");
            }
        });
    }

    /// What trim_neighbors takes off earlier real runs' 整理 results: POMELO_REPLAY=<folder of runs>
    #[test]
    #[ignore]
    fn replay_neighbor_trim() {
        let Ok(dir) = std::env::var("POMELO_REPLAY") else { return };
        let mut folders = vec![];
        let mut stack = vec![PathBuf::from(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.join(".pub2md/job.json").is_file() {
                    folders.push(p.clone());
                }
                if p.is_dir() && !p.ends_with(".pub2md") {
                    stack.push(p);
                }
            }
        }
        folders.sort();
        let (mut chunks, mut hits) = (0, 0);
        for f in &folders {
            let (Ok(job), Ok(text)) = (load_job(f), std::fs::read_to_string(f.join(ORIGINAL))) else { continue };
            let blocks = mmd::blocks(&text).0;
            for c in &job.chunks {
                let Ok(out) = std::fs::read_to_string(chunk_file(f, c.chunk.start, "out")) else { continue };
                if c.chunk.end > blocks.len() {
                    continue;
                }
                chunks += 1;
                let (before, after) = neighbor_text(&blocks, &c.chunk);
                if let Some((kept, n)) = check::trim_neighbors(&chunk::source_text(&blocks, &c.chunk, false), &out, &before, &after) {
                    hits += 1;
                    let gone: Vec<String> = mmd::blocks(&out).0.into_iter().filter(|b| !kept.contains(&b.text)).map(|b| b.text.chars().take(90).collect::<String>().replace('\n', "⏎")).collect();
                    eprintln!("[replay] {} · 从第 {} 块起：删 {n} 段\n    {}", f.file_name().unwrap().to_string_lossy(), c.chunk.start, gone.join("\n    "));
                }
            }
        }
        eprintln!("[replay] {} 份、{chunks} 块，{hits} 块会删", folders.len());
    }

    #[test]
    fn articles_become_files_and_names_are_safe() {
        let dir = crate::db::tests::temp_dir("pub2md-articles");
        std::fs::create_dir_all(work_dir(&dir).join("chunks")).unwrap();
        let mut job = Job { md: true, steps: Steps::Clean, ..Default::default() };
        for (start, article, text) in [(0, 1, "A1"), (3, 1, "A2"), (5, 2, "B")] {
            std::fs::write(chunk_file(&dir, start, "out"), text).unwrap();
            job.chunks.push(ChunkJob { chunk: Chunk { start, end: start + 1, article, ..Default::default() }, ..Default::default() });
        }
        job.articles = vec![Article { article: 1, title: "On/Off: A Study".into() }];
        let names = assemble(&dir, &job).unwrap();
        assert_eq!(names, ["整理稿 1 · On-Off- A Study.md", "整理稿 2 · 第 2 篇.md"]);
        assert_eq!(std::fs::read_to_string(dir.join(&names[0])).unwrap(), "A1\n\nA2\n");
        assert_eq!(duration(3725), "1 小时 2 分");
        assert_eq!(safe_name("  .hidden "), "hidden");
        assert_eq!(fit("abcdefgh".repeat(100).as_str(), 50).1, true);
        assert_eq!(terms_lines(&[Term { source: "Transformer".into(), target: "Transformer".into(), note: String::new() }, Term { source: "BLEU".into(), target: "BLEU 分数".into(), note: "指标".into() }], Some("the bleu score")), "- BLEU → BLEU 分数（指标）");
    }
}
