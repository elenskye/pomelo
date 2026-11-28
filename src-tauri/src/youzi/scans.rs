//! 知识库's scans (1.2.0): the pages of a library's PDFs without text of their own are recognized on this Mac
//! (engine-pdf: Vision, offline), in the background, one file at a time and a page at a time.
//! - ocr.db (derived like search.db: never backed up, exported or undone; a missing, broken or older one is
//!   made again and the scans are recognized again): `pages` what each page of a file's bytes (sha256) reads
//!   as ("" = nothing recognized); `queue` the library files still to recognize (source, path relative to the
//!   folder, the bytes queued, in order).
//! - Which pages (`plan`): those engine-pdf finds without their own text (a scan, a garbled layer) and not
//!   blank. In a PDF of mostly text pages only those with no text at all (a page with a few words is a figure
//!   or a cover), as engine-pdf's find does.
//! - Building (knowledge.rs) reads a library PDF with what is recognized so far (`read`) and queues the rest:
//!   a file whose pages all wait has no passages yet (`WAITING`), one with nothing recognized is a scan listed
//!   by name (`NOTHING`). A recognized page that reads more than the page's own few words is a passage labelled
//!   「第 3 页 · 本机识别」 (the segment's heading).
//!   When a file is done its doc is built again (passages already embedded keep their vectors). A build that
//!   read a file while it waited checks the queue after saving, so a file finished meanwhile is built again.
//! - The worker (`wake`): one at a time; at launch (taking the scans listed by name before 1.2, `files::
//!   SCANNED_PDF`, into the queue), after a build queues a file and after 接着识别. Vision's first load in the
//!   process is 「准备识别」. 停下 (`pause`) stops a source's files until 接着识别 or the next launch.

use super::files::{self, Segment};
use super::knowledge::{self, Source};
use super::pdf;
use crate::db;
use crate::modules::common::err;
use crate::AppState;
use engine_pdf::{Cancel, Content, Document, Ocr, ReadOptions};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Manager};

pub const VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE pages (
  sha256 TEXT NOT NULL,
  page   INTEGER NOT NULL,
  text   TEXT NOT NULL,
  PRIMARY KEY (sha256, page)
) STRICT, WITHOUT ROWID;
CREATE TABLE queue (
  source_id INTEGER NOT NULL,
  path      TEXT NOT NULL,
  sha256    TEXT,
  seq       INTEGER NOT NULL,
  PRIMARY KEY (source_id, path)
) STRICT;
";

const DB: db::Derived = db::Derived { what: "扫描件的识别结果", not_it: "不是扫描件的识别结果", version: VERSION, schema: SCHEMA, probe: "pages" };

/// A library PDF whose pages all wait to be recognized (kept as the doc's error: no passages yet).
pub const WAITING: &str = "这份扫描件还在本机识别，识别完才能读";
/// A library PDF where nothing was recognized (listed by name, like an image).
pub const NOTHING: &str = "这份 PDF 里没有认得出的字";
/// A recognized page's note (its segment's heading).
pub const LOCAL_OCR: &str = "本机识别";

/// A scan: no passages of its own (waiting, nothing recognized, or seen before 1.2).
pub fn is_scan(error: Option<&str>) -> bool {
    matches!(error, Some(e) if e == WAITING || e == NOTHING || e == files::SCANNED_PDF)
}

pub fn is_pdf(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, x)| x.eq_ignore_ascii_case("pdf"))
}

// ---------- which pages, and the text ----------

#[derive(Debug, Clone, PartialEq)]
enum Plan {
    /// Its own text.
    Text(String),
    /// To recognize (the few words its text layer has).
    Wanted(String),
    /// Not recognized: blank, unreadable, or a figure of a text PDF (its few words).
    Skip(String),
}

fn plan(doc: &Document) -> Result<Vec<Plan>, String> {
    let all: Vec<u32> = (1..=doc.page_count()).collect();
    let opts = ReadOptions { ocr: Ocr::Off, render: None, ..Default::default() };
    let pages = engine_pdf::read(doc, &all, &opts, &Cancel::new()).map_err(pdf::error_text)?;
    let texts = pages.iter().filter(|p| matches!(p.content, Content::Text(_) | Content::SearchableText(_))).count();
    let without = pages.iter().filter(|p| matches!(p.content, Content::NoText { .. })).count();
    let mostly_text = texts * 2 > texts + without;
    Ok(pages
        .into_iter()
        .map(|p| match p.content {
            Content::NoText { partial, .. } if !mostly_text || partial.trim().is_empty() => Plan::Wanted(partial),
            Content::NoText { partial, .. } => Plan::Skip(partial),
            Content::Blank | Content::Failed(_) => Plan::Skip(String::new()),
            c => Plan::Text(pdf::text_of(c)),
        })
        .collect())
}

/// What a library PDF reads as now.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfText {
    /// What its passages are made from: the bytes' sha256, with how many recognized pages (`…+ocr3`).
    pub made_from: String,
    pub segments: Vec<Segment>,
    /// Pages still to recognize (the file is queued).
    pub waiting: usize,
}

fn chars(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count()
}

fn text_from(plan: &[Plan], recognized: &HashMap<u32, String>, sha: &str) -> PdfText {
    let mut segments = vec![];
    let (mut used, mut waiting) = (0, 0);
    for (i, p) in plan.iter().enumerate() {
        let page = i as u32 + 1;
        let (text, note) = match p {
            Plan::Text(t) | Plan::Skip(t) => (t.trim(), None),
            Plan::Wanted(partial) => match recognized.get(&page).map(|t| t.trim()) {
                // only when it reads more than the few words the page has
                Some(t) if chars(t) > chars(partial) => {
                    used += 1;
                    (t, Some(LOCAL_OCR.to_string()))
                }
                Some(_) => (partial.trim(), None),
                None => {
                    waiting += 1;
                    (partial.trim(), None)
                }
            },
        };
        if !text.is_empty() {
            segments.push(Segment { ordinal: segments.len() as i64, page_start: Some(page as i64), page_end: Some(page as i64), heading: note, content: text.to_string() });
        }
    }
    let made_from = if used > 0 { format!("{sha}+ocr{used}") } else { sha.to_string() };
    PdfText { made_from, segments, waiting }
}

// ---------- ocr.db and the worker's state ----------

/// The file being recognized: `page` of `total` pages to recognize done.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Now {
    pub source: i64,
    pub path: String,
    pub page: usize,
    pub total: usize,
}

#[derive(Default)]
struct State {
    now: Option<Now>,
    /// Vision loading for the first page in this process.
    preparing: bool,
    ready: bool,
    paused: HashSet<i64>,
    cancel: Option<(i64, Cancel)>,
}

pub struct Inner {
    db_path: PathBuf,
    db: Mutex<Option<Connection>>,
    state: Mutex<State>,
    run: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub struct Scans(Arc<Inner>);

/// What the manager shows of a source.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceScans {
    pub waiting: i64,
    pub now: Option<Now>,
    pub preparing: bool,
    pub paused: bool,
}

impl Scans {
    pub fn new(data: &Path) -> Self {
        Self(Arc::new(Inner { db_path: data.join("ocr.db"), db: Mutex::new(None), state: Mutex::default(), run: tokio::sync::Mutex::new(()) }))
    }

    pub fn with_db<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T, String>) -> Result<T, String> {
        let mut g = self.0.db.lock().map_err(|_| "扫描件的识别结果打不开，请重启 App".to_string())?;
        if g.is_none() {
            *g = Some(DB.open(&self.0.db_path)?);
        }
        f(g.as_mut().expect("opened"))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A library PDF's text: its own, and what is recognized of the other pages so far.
    pub fn read(&self, data: &[u8]) -> Result<PdfText, String> {
        let sha = pdf::sha256_hex(data);
        let plan = plan(&pdf::open(data)?)?;
        let recognized = self.with_db(|c| recognized(c, &sha))?;
        Ok(text_from(&plan, &recognized, &sha))
    }

    pub fn queue(&self, source: i64, path: &str, sha: Option<&str>) -> Result<(), String> {
        self.with_db(|c| {
            c.execute(
                "INSERT INTO queue (source_id, path, sha256, seq) VALUES (?1, ?2, ?3, (SELECT coalesce(max(seq), 0) + 1 FROM queue))
                 ON CONFLICT (source_id, path) DO UPDATE SET sha256 = excluded.sha256",
                params![source, path, sha],
            )
            .map(|_| ())
            .map_err(err)
        })
    }

    pub fn unqueue(&self, source: i64, path: &str) -> Result<(), String> {
        self.with_db(|c| c.execute("DELETE FROM queue WHERE source_id = ?1 AND path = ?2", params![source, path]).map(|_| ()).map_err(err))
    }

    pub fn is_queued(&self, source: i64, path: &str) -> Result<bool, String> {
        self.with_db(|c| c.query_row("SELECT 1 FROM queue WHERE source_id = ?1 AND path = ?2", params![source, path], |_| Ok(())).optional().map(|r| r.is_some()).map_err(err))
    }

    /// Forgets the queued files of sources that are gone.
    pub fn keep_sources(&self, keep: &HashSet<i64>) -> Result<(), String> {
        let ids: Vec<i64> = self.with_db(|c| {
            let mut st = c.prepare("SELECT DISTINCT source_id FROM queue").map_err(err)?;
            let rows = st.query_map([], |r| r.get(0)).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)
        })?;
        for id in ids.into_iter().filter(|id| !keep.contains(id)) {
            self.with_db(|c| c.execute("DELETE FROM queue WHERE source_id = ?1", [id]).map(|_| ()).map_err(err))?;
        }
        Ok(())
    }

    /// Per source: files queued, the one being recognized, whether 停下.
    pub fn view(&self) -> Result<HashMap<i64, SourceScans>, String> {
        let counts: Vec<(i64, i64)> = self.with_db(|c| {
            let mut st = c.prepare("SELECT source_id, count(*) FROM queue GROUP BY source_id").map_err(err)?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)
        })?;
        let s = self.state();
        let mut out: HashMap<i64, SourceScans> = counts.into_iter().map(|(id, n)| (id, SourceScans { waiting: n, paused: s.paused.contains(&id), ..Default::default() })).collect();
        if let Some(now) = &s.now {
            let v = out.entry(now.source).or_default();
            v.now = Some(now.clone());
            v.preparing = s.preparing;
        }
        Ok(out)
    }

    /// 停下: the source's files wait (until 接着识别 or the next launch); the one being recognized stops.
    pub fn pause(&self, source: i64) {
        let mut s = self.state();
        s.paused.insert(source);
        if let Some((id, c)) = &s.cancel {
            if *id == source {
                c.cancel();
            }
        }
    }

    pub fn resume(&self, source: i64) {
        self.state().paused.remove(&source);
    }
}

fn recognized(c: &Connection, sha: &str) -> Result<HashMap<u32, String>, String> {
    let mut st = c.prepare("SELECT page, text FROM pages WHERE sha256 = ?1").map_err(err)?;
    let rows = st.query_map([sha], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?))).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

#[derive(Debug, Clone)]
struct Queued {
    source: i64,
    path: String,
    sha256: Option<String>,
}

fn next(c: &Connection, live: &HashSet<i64>, paused: &HashSet<i64>) -> Result<Option<Queued>, String> {
    let mut st = c.prepare("SELECT source_id, path, sha256 FROM queue ORDER BY seq").map_err(err)?;
    let rows = st.query_map([], |r| Ok(Queued { source: r.get(0)?, path: r.get(1)?, sha256: r.get(2)? })).map_err(err)?;
    for q in rows {
        let q = q.map_err(err)?;
        if live.contains(&q.source) && !paused.contains(&q.source) {
            return Ok(Some(q));
        }
    }
    Ok(None)
}

// ---------- the worker ----------

/// A page recognized: its text ("" = nothing).
pub type Recognize = dyn Fn(&Document, u32, &Cancel) -> Result<String, String> + Send + Sync;

/// engine-pdf's: the page read with local OCR.
fn vision(doc: &Document, page: u32, cancel: &Cancel) -> Result<String, String> {
    let opts = ReadOptions { ocr: Ocr::auto(), render: None, ..Default::default() };
    let mut text = String::new();
    engine_pdf::read_each(doc, &[page], &opts, cancel, |p| {
        text = match p.content {
            Content::Ocr(o) => o.text,
            Content::Text(t) | Content::SearchableText(t) => t,
            _ => String::new(),
        };
        engine_pdf::Flow::Continue
    })
    .map_err(pdf::error_text)?;
    Ok(text)
}

#[derive(Debug, PartialEq)]
enum Ended {
    Done,
    /// The file changed, went or can't be read: the scan and the next build see to it.
    Gone,
    Stopped,
}

/// Recognizes a queued file's waiting pages, saving each as it is done.
fn recognize(scans: &Scans, file: &Path, q: &Queued, cancel: &Cancel, ocr: &dyn Fn(&Document, u32, &Cancel) -> Result<String, String>) -> Result<Ended, String> {
    let Ok(data) = std::fs::read(file) else { return Ok(Ended::Gone) };
    let sha = pdf::sha256_hex(&data);
    if q.sha256.as_deref().is_some_and(|s| s != sha) {
        return Ok(Ended::Gone);
    }
    let Ok(doc) = pdf::open(&data) else { return Ok(Ended::Gone) };
    let Ok(plan) = plan(&doc) else { return Ok(Ended::Gone) };
    let wanted: Vec<u32> = plan.iter().enumerate().filter(|(_, p)| matches!(p, Plan::Wanted(_))).map(|(i, _)| i as u32 + 1).collect();
    let done = scans.with_db(|c| recognized(c, &sha))?;
    let todo: Vec<u32> = wanted.iter().copied().filter(|p| !done.contains_key(p)).collect();
    let total = wanted.len();
    let mut finished = total - todo.len();
    let show = |page: usize| scans.state().now = Some(Now { source: q.source, path: q.path.clone(), page, total });
    show(finished);
    for page in todo {
        if cancel.is_cancelled() {
            return Ok(Ended::Stopped);
        }
        let first = {
            let mut s = scans.state();
            let first = !s.ready;
            s.preparing = first;
            first
        };
        let text = ocr(&doc, page, cancel);
        if first {
            let mut s = scans.state();
            (s.preparing, s.ready) = (false, true);
        }
        let text = match text {
            Ok(t) => t,
            Err(_) if cancel.is_cancelled() => return Ok(Ended::Stopped),
            Err(_) => String::new(),
        };
        scans.with_db(|c| c.execute("INSERT OR REPLACE INTO pages (sha256, page, text) VALUES (?1, ?2, ?3)", params![sha, page, text]).map(|_| ()).map_err(err))?;
        finished += 1;
        show(finished);
    }
    Ok(Ended::Done)
}

/// Starts the worker (when it isn't running).
pub fn wake(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run(&app, Arc::new(vision)).await });
}

/// The scans listed by name before 1.2 (search.db `kb_docs`) go into the queue and wait; how many.
pub fn adopt(search: &Connection, scans: &Scans) -> Result<usize, String> {
    let old: Vec<(i64, String)> = {
        let mut q = search.prepare("SELECT source_id, path FROM kb_docs WHERE error = ?1").map_err(err)?;
        let rows = q.query_map([files::SCANNED_PDF], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)?
    };
    for (source, path) in &old {
        scans.queue(*source, path, None)?;
        search.execute("UPDATE kb_docs SET error = ?1 WHERE source_id = ?2 AND path = ?3", params![WAITING, source, path]).map_err(err)?;
    }
    Ok(old.len())
}

/// A file recognized (already unqueued: a build that read it meanwhile checks the queue after saving): built again.
pub fn build_again(search: &Connection, source: i64, path: &str) -> Result<(), String> {
    search.execute("UPDATE kb_docs SET fresh = 0 WHERE source_id = ?1 AND path = ?2", params![source, path]).map(|_| ()).map_err(err)
}

/// The next file to recognize: of a folder source still there, not 停下.
fn next_live(app: &AppHandle) -> Result<Option<(Queued, Source)>, String> {
    let st = app.state::<AppState>();
    let live: HashMap<i64, Source> = st.youzi.with_db(|c| knowledge::sources(c, None))?.into_iter().filter(|s| s.kind == "folder" && !s.deleted).map(|s| (s.id, s)).collect();
    let ids: HashSet<i64> = live.keys().copied().collect();
    let paused = st.youzi.scans.state().paused.clone();
    Ok(st.youzi.scans.with_db(|c| next(c, &ids, &paused))?.map(|q| {
        let src = live[&q.source].clone();
        (q, src)
    }))
}

pub(crate) async fn run(app: &AppHandle, ocr: Arc<Recognize>) {
    let st = app.state::<AppState>();
    let scans = st.youzi.scans.clone();
    loop {
        {
            let Ok(_running) = scans.0.run.try_lock() else { return };
            if let Err(e) = st.search.with_db(|c| adopt(c, &scans)) {
                eprintln!("scans: {e}");
            }
            loop {
                let (q, src) = match next_live(app) {
                    Ok(Some(n)) => n,
                    Ok(None) => break,
                    Err(e) => return eprintln!("scans: {e}"),
                };
                let Ok(file) = knowledge::file_path(&src, &q.path) else {
                    let _ = scans.unqueue(q.source, &q.path);
                    continue;
                };
                let cancel = Cancel::new();
                {
                    let mut s = scans.state();
                    if s.paused.contains(&q.source) {
                        continue; // 停下 just now
                    }
                    s.cancel = Some((q.source, cancel.clone()));
                }
                let (s2, q2, ocr2) = (scans.clone(), q.clone(), ocr.clone());
                let ended = tauri::async_runtime::spawn_blocking(move || recognize(&s2, &file, &q2, &cancel, &*ocr2)).await;
                {
                    let mut s = scans.state();
                    (s.now, s.cancel) = (None, None);
                }
                match ended {
                    Ok(Ok(Ended::Done)) => {
                        let _ = scans.unqueue(q.source, &q.path);
                        let _ = st.search.with_db(|c| build_again(c, q.source, &q.path));
                        knowledge::request(app, &[q.source], false);
                    }
                    Ok(Ok(Ended::Gone)) => {
                        let _ = scans.unqueue(q.source, &q.path);
                    }
                    Ok(Ok(Ended::Stopped)) => {}
                    Ok(Err(e)) => return eprintln!("scans: {e}"),
                    Err(e) => return eprintln!("scans: {e}"),
                }
            }
        }
        // a file queued while this one finished (its wake found the worker still running)
        if !matches!(next_live(app), Ok(Some(_))) {
            return;
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

/// The worker's next file recognized with `ocr` (files under `folder`): its source and path.
    pub fn run_one(scans: &Scans, folder: &Path, ocr: &dyn Fn(&Document, u32, &Cancel) -> Result<String, String>) -> Option<(i64, String)> {
        let q = scans.with_db(|c| c.query_row("SELECT source_id, path, sha256 FROM queue ORDER BY seq LIMIT 1", [], |r| Ok(Queued { source: r.get(0)?, path: r.get(1)?, sha256: r.get(2)? })).optional().map_err(err)).unwrap()?;
        assert_eq!(recognize(scans, &folder.join(&q.path), &q, &Cancel::new(), ocr).unwrap(), Ended::Done);
        scans.unqueue(q.source, &q.path).unwrap();
        Some((q.source, q.path))
    }

    pub fn memory() -> Scans {
        let s = Scans::new(Path::new("/nonexistent"));
        let c = Connection::open_in_memory().unwrap();
        db::configure(&c).unwrap();
        DB.prepare(&c).unwrap();
        *s.0.db.lock().unwrap() = Some(c);
        s
    }

    /// A PDF of `texts` ("" = a page drawn as a picture of `scan_text`, no text layer).
    pub fn scanned_pdf(pages: &[&str]) -> Vec<u8> {
        let text = engine_pdf::samples::Sample::TextEn.generate().unwrap();
        let doc = Document::from_bytes(&text).unwrap();
        let picture = engine_pdf::samples::scan(&doc, 1).unwrap();
        let mut parts: Vec<Vec<u8>> = vec![];
        for p in pages {
            parts.push(if p.is_empty() { picture.clone() } else { crate::youzi::pdf::tests::sample_pdf(&[p]) });
        }
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        engine_pdf::compose::assemble(&refs, &Default::default(), &Cancel::new()).unwrap()
    }

    #[test]
    fn plans_which_pages_to_recognize() {
        let words = "A page of text long enough to stand for the whole page.";
        // mostly scans: every page without text
        let pdf = scanned_pdf(&["", "", words]);
        let p = plan(&Document::from_bytes(&pdf).unwrap()).unwrap();
        assert!(matches!(p[0], Plan::Wanted(_)) && matches!(p[1], Plan::Wanted(_)) && matches!(p[2], Plan::Text(_)), "{p:?}");
        // mostly text: a page with a few words (a figure) isn't recognized, one with none is
        let pdf = scanned_pdf(&[words, words, words, "Figure 3", ""]);
        let p = plan(&Document::from_bytes(&pdf).unwrap()).unwrap();
        assert_eq!(p[3], Plan::Skip("Figure 3".into()), "{p:?}");
        assert!(matches!(p[4], Plan::Wanted(_)), "{p:?}");
    }

    #[test]
    fn text_comes_from_the_layer_and_what_is_recognized() {
        let plan = vec![Plan::Text("Own text".into()), Plan::Wanted("".into()), Plan::Wanted("p. 3".into()), Plan::Skip("Figure".into()), Plan::Skip(String::new()), Plan::Wanted("Preface: first page".into())];
        let none = text_from(&plan, &HashMap::new(), "abc");
        assert_eq!((none.made_from.as_str(), none.waiting), ("abc", 3));
        assert_eq!(none.segments.iter().map(|s| (s.label(), s.content.as_str())).collect::<Vec<_>>(), [("第 1 页".into(), "Own text"), ("第 3 页".into(), "p. 3"), ("第 4 页".into(), "Figure"), ("第 6 页".into(), "Preface: first page")]);
        // what reads no more than the page's own few words keeps those
        let some = text_from(&plan, &HashMap::from([(2, "Recognized two".to_string()), (3, String::new()), (6, "Preface: first page".to_string())]), "abc");
        assert_eq!((some.made_from.as_str(), some.waiting), ("abc+ocr1", 0));
        let labels: Vec<_> = some.segments.iter().map(|s| (s.label(), s.content.as_str())).collect();
        assert_eq!(labels, [("第 1 页".into(), "Own text"), ("第 2 页 · 本机识别".into(), "Recognized two"), ("第 3 页".into(), "p. 3"), ("第 4 页".into(), "Figure"), ("第 6 页".into(), "Preface: first page")]);
    }

    #[test]
    fn the_queue_keeps_order_and_skips_paused_or_gone_sources() {
        let s = memory();
        s.queue(1, "a.pdf", Some("x")).unwrap();
        s.queue(2, "b.pdf", None).unwrap();
        s.queue(1, "c.pdf", None).unwrap();
        s.queue(1, "a.pdf", Some("y")).unwrap(); // queued again: same place, new bytes
        let first = s.with_db(|c| next(c, &HashSet::from([1, 2]), &HashSet::new())).unwrap().unwrap();
        assert_eq!((first.source, first.path.as_str(), first.sha256.as_deref()), (1, "a.pdf", Some("y")));
        let next2 = s.with_db(|c| next(c, &HashSet::from([1, 2]), &HashSet::from([1]))).unwrap().unwrap();
        assert_eq!(next2.path, "b.pdf");
        assert!(s.with_db(|c| next(c, &HashSet::from([3]), &HashSet::new())).unwrap().is_none());
        s.pause(1);
        let v = s.view().unwrap();
        assert_eq!((v[&1].waiting, v[&1].paused, v[&2].waiting, v[&2].paused), (2, true, 1, false));
        s.keep_sources(&HashSet::from([2])).unwrap();
        assert!(!s.is_queued(1, "c.pdf").unwrap() && s.is_queued(2, "b.pdf").unwrap());
        s.unqueue(2, "b.pdf").unwrap();
        assert!(s.view().unwrap().is_empty());
    }

    #[test]
    fn recognizing_saves_each_page_and_picks_up_where_it_stopped() {
        let dir = crate::db::tests::temp_dir("scans-recognize");
        let s = memory();
        let file = dir.join("scan.pdf");
        std::fs::write(&file, scanned_pdf(&["", "", ""])).unwrap();
        let sha = pdf::sha256_hex(&std::fs::read(&file).unwrap());
        let q = Queued { source: 1, path: "scan.pdf".into(), sha256: Some(sha.clone()) };
        let seen = Mutex::new(vec![]);
        let cancel = Cancel::new();
        // stopped after the second page
        let fake = |_: &Document, page: u32, c: &Cancel| -> Result<String, String> {
            seen.lock().unwrap().push(page);
            if page == 2 {
                c.cancel();
            }
            Ok(format!("Recognized page {page}"))
        };
        assert_eq!(recognize(&s, &file, &q, &cancel, &fake).unwrap(), Ended::Stopped);
        assert_eq!(s.with_db(|c| recognized(c, &sha)).unwrap().len(), 2);
        assert_eq!(s.state().now, Some(Now { source: 1, path: "scan.pdf".into(), page: 2, total: 3 }));
        assert!(s.state().ready);
        // goes on with the third only
        assert_eq!(recognize(&s, &file, &q, &Cancel::new(), &fake).unwrap(), Ended::Done);
        assert_eq!(*seen.lock().unwrap(), [1, 2, 3]);
        let text = s.read(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!((text.waiting, text.segments.len(), text.made_from), (0, 3, format!("{sha}+ocr3")));
        assert_eq!(text.segments[2].label(), "第 3 页 · 本机识别");
        // changed bytes or a missing file: dropped
        let other = Queued { sha256: Some("other".into()), ..q.clone() };
        assert_eq!(recognize(&s, &file, &other, &Cancel::new(), &fake).unwrap(), Ended::Gone);
        assert_eq!(recognize(&s, &dir.join("missing.pdf"), &q, &Cancel::new(), &fake).unwrap(), Ended::Gone);
    }

    /// Vision for real, offline (its first load in a process takes a while).
    #[test]
    fn vision_reads_a_scanned_page() {
        let pdf = scanned_pdf(&[""]);
        let doc = Document::from_bytes(&pdf).unwrap();
        let text = vision(&doc, 1, &Cancel::new()).unwrap();
        let want = engine_pdf::samples::Sample::TextEn.source_text(1).unwrap();
        let first = want.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
        assert!(text.contains(&first), "{text:?} / {first:?}");
    }
}

/// Real files (1.2.0 Phase 2): every PDF of `POMELO_PDF_CORPUS` (folders, `:`-separated) read as a library file,
/// its waiting pages recognized with Vision, timed → `POMELO_PDF_DUMP` (JSON).
#[cfg(test)]
mod real {
    use super::*;

    #[test]
    #[ignore]
    fn dump_recognized() {
        let (Ok(corpus), Ok(out)) = (std::env::var("POMELO_PDF_CORPUS"), std::env::var("POMELO_PDF_DUMP")) else { return };
        let mut files: Vec<PathBuf> = corpus.split(':').flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "pdf")).collect();
        files.sort();
        let mut all = vec![];
        for f in files {
            let data = std::fs::read(&f).unwrap();
            let t = std::time::Instant::now();
            let layer_only = pdf::pages(&data).map(|p| p.len());
            let layer_ms = t.elapsed().as_millis();
            let t = std::time::Instant::now();
            let Ok(doc) = pdf::open(&data) else {
                all.push(serde_json::json!({ "file": f.to_string_lossy(), "error": layer_only.err() }));
                continue;
            };
            let plan = plan(&doc).unwrap();
            let plan_ms = t.elapsed().as_millis();
            let mut recognized = HashMap::new();
            let t = std::time::Instant::now();
            for (i, p) in plan.iter().enumerate() {
                if matches!(p, Plan::Wanted(_)) {
                    recognized.insert(i as u32 + 1, vision(&doc, i as u32 + 1, &Cancel::new()).unwrap());
                }
            }
            let ocr_ms = t.elapsed().as_millis();
            let text = text_from(&plan, &recognized, "sha");
            eprintln!("{} pages {} wanted {} · layer {layer_ms} ms, plan {plan_ms} ms, ocr {ocr_ms} ms", f.display(), plan.len(), recognized.len());
            all.push(serde_json::json!({
                "file": f.to_string_lossy(), "pages": plan.len(), "wanted": recognized.len(), "layer_ms": layer_ms, "plan_ms": plan_ms, "ocr_ms": ocr_ms,
                "segments": text.segments.iter().map(|s| [s.label(), s.content.clone()]).collect::<Vec<_>>(),
            }));
        }
        std::fs::write(out, serde_json::to_string_pretty(&all).unwrap()).unwrap();
    }
}
