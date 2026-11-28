//! The print page (print.html in a hidden window `print`): 见字如面's PDF and Word are drawn there, one article at a
//! time, as A4 pages (1 CSS px = 1 pt), and come back here. Rules:
//! - PDF: the page shows one page at a time at its top-left; WebKit draws it (`createPDF` of 595 × 842), and engine-pdf
//!   puts the pages together with the contents' links (to a page and height), web links, an outline (the headings)
//!   and the title. Text stays text.
//! - Word: the page sends the article as blocks (docx.rs writes the file); formulas and diagrams become pictures, drawn
//!   the same way (a PDF of their box, rendered by engine-pdf at 3×).
//! - One export at a time. The job (articles, where each goes, options) is Rust's; the page asks for it, reports each
//!   file (written, or why not) and finishes; the window is closed then (or after 30 minutes).

use crate::modules::articles::docx;
use crate::modules::articles::export::{write_new, Options, Planned};
use crate::modules::articles::import::Left;
use crate::modules::attachments;
use crate::AppState;
use objc2::rc::{autoreleasepool, Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::encode::{Encode, Encoding};
use objc2::msg_send;
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

pub const LABEL: &str = "print";
/// A4 in points.
pub const PAGE: (f64, f64) = (595.0, 842.0);

struct Job {
    id: u64,
    items: Vec<Planned>,
    options: Options,
    pages: Vec<Vec<u8>>,
    pictures: Vec<docx::Picture>,
    written: usize,
    failed: usize,
    left: Vec<Left>,
    done: Option<oneshot::Sender<()>>,
}

#[derive(Default)]
pub struct Print {
    job: Mutex<Option<Job>>,
    seq: Mutex<u64>,
}

impl Print {
    fn with<T>(&self, id: u64, f: impl FnOnce(&mut Job) -> Result<T, String>) -> Result<T, String> {
        let mut g = self.job.lock().map_err(|_| "导出出错了".to_string())?;
        match g.as_mut() {
            Some(j) if j.id == id => f(j),
            _ => Err("这次导出已经结束了".into()),
        }
    }
}

/// Draws the planned articles as PDF / Word in the print window; returns (files written, what was left out).
pub async fn run(app: &AppHandle, items: Vec<Planned>, options: Options) -> Result<(usize, Vec<Left>), String> {
    let print = app.state::<Print>();
    let (tx, rx) = oneshot::channel();
    let id = {
        let mut seq = print.seq.lock().map_err(|_| "导出出错了".to_string())?;
        *seq += 1;
        *seq
    };
    {
        let mut g = print.job.lock().map_err(|_| "导出出错了".to_string())?;
        if g.is_some() {
            return Err("上一次导出还没做完".into());
        }
        *g = Some(Job { id, items, options, pages: vec![], pictures: vec![], written: 0, failed: 0, left: vec![], done: Some(tx) });
    }
    if let Some(old) = app.get_webview_window(LABEL) {
        let _ = old.destroy();
    }
    let made = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(format!("print.html?job={id}").into()))
        .title("导出")
        .visible(false)
        .focused(false)
        .inner_size(PAGE.0 + 45.0, 900.0)
        .resizable(false)
        .build();
    if let Err(e) = made {
        print.job.lock().map_err(|_| "导出出错了".to_string())?.take();
        return Err(format!("没能打开排版的窗口：{e}"));
    }
    let finished = tokio::time::timeout(Duration::from_secs(30 * 60), rx).await;
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
    let job = print.job.lock().map_err(|_| "导出出错了".to_string())?.take();
    let job = job.ok_or("导出出错了")?;
    if finished.is_err() {
        let mut left = job.left;
        left.push(Left { title: "导出".into(), what: "排版太久没有做完，停下了".into() });
        return Ok((job.written, left));
    }
    Ok((job.written, job.left))
}

// ---------- WebKit: a PDF of the window's top-left w × h ----------

async fn window_pdf(app: &AppHandle, w: f64, h: f64) -> Result<Vec<u8>, String> {
    let win = app.get_webview_window(LABEL).ok_or("排版的窗口不见了")?;
    let (tx, rx) = oneshot::channel::<Result<Vec<u8>, String>>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    win.with_webview(move |wv| unsafe {
        let wk = wv.inner() as *mut AnyObject;
        let send = move |r: Result<Vec<u8>, String>| {
            if let Some(t) = tx.lock().ok().and_then(|mut g| g.take()) {
                let _ = t.send(r);
            }
        };
        let Some(cfg_class) = AnyClass::get(c"WKPDFConfiguration") else {
            return send(Err("这台 Mac 的 WebKit 不能导出 PDF".into()));
        };
        let cfg: Retained<AnyObject> = msg_send![cfg_class, new];
        let rect = CgRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: w, height: h } };
        let _: () = msg_send![&*cfg, setRect: rect];
        let send = Arc::new(send);
        let block = block2::RcBlock::new(move |data: *mut AnyObject, error: *mut AnyObject| {
            let r = if !data.is_null() {
                Ok(bytes_of(&*data))
            } else if !error.is_null() {
                let d: Retained<objc2_foundation::NSString> = msg_send![error, localizedDescription];
                Err(format!("WebKit 没画出来：{d}"))
            } else {
                Err("WebKit 没画出来".into())
            };
            send(r);
        });
        let _: () = msg_send![wk, createPDFWithConfiguration: &*cfg, completionHandler: &*block];
    })
    .map_err(|e| format!("排版的窗口出错了：{e}"))?;
    match tokio::time::timeout(Duration::from_secs(60), rx).await {
        Ok(Ok(r)) => r,
        _ => Err("WebKit 没画出来（超时）".into()),
    }
}

// ---------- engine-pdf: pages together, links, outline ----------

/// A link on a page (CSS px from the page's top-left): to a page and height, or a web address.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfLink {
    pub page: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub to_page: Option<usize>,
    #[serde(default)]
    pub to_y: Option<f64>,
    #[serde(default)]
    pub url: Option<String>,
}

/// A heading for the outline (level 1 = the top).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mark {
    pub level: u8,
    pub title: String,
    pub page: usize,
    pub y: f64,
}

/// A web address as a PDF link takes it: what isn't plain ASCII percent-encoded (as NSURL did); None for one a
/// PDF link can't hold (no scheme, file:, javascript:).
fn link_url(u: &str) -> Option<String> {
    let (scheme, rest) = u.trim().split_once(':')?;
    let ok = !rest.is_empty() && scheme.starts_with(|c: char| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !ok || ["file", "javascript"].contains(&scheme.to_ascii_lowercase().as_str()) || u.chars().any(char::is_control) {
        return None;
    }
    let mut out = String::new();
    for c in u.trim().chars() {
        if c.is_ascii_graphic() && !"<>\"\\^`{|}".contains(c) {
            out.push(c);
        } else {
            let mut b = [0; 4];
            for byte in c.encode_utf8(&mut b).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    Some(out)
}

/// The pages (one-page PDFs of A4, from WebKit) as one document with its links, outline and title.
pub fn assemble(pages: &[Vec<u8>], links: &[PdfLink], outline: &[Mark], title: &str) -> Result<Vec<u8>, String> {
    use engine_pdf::compose::{Assembly, AssemblyLink};
    use engine_pdf::edit::{InfoPatch, OutlineItem, Rect, Target};
    if pages.is_empty() {
        return Err("没有页".into());
    }
    let count = pages.len();
    let h = PAGE.1;
    let to = |page: usize, y: f64| Target::Page { page: page as u32 + 1, y: (h - y).clamp(0.0, h) };
    let links = links
        .iter()
        .filter(|l| l.page < count)
        .filter_map(|l| {
            let target = match (l.to_page, l.to_y, &l.url) {
                (Some(p), Some(y), _) if p < count => to(p, y),
                (Some(_), Some(_), _) => return None,
                (_, _, Some(u)) => Target::Url { url: link_url(u)? },
                _ => return None,
            };
            Some(AssemblyLink { page: l.page as u32 + 1, rect: Rect::from_top(h, l.x, l.y, l.w, l.h), target })
        })
        .collect();
    // the headings, nested by level
    let mut roots: Vec<OutlineItem> = vec![];
    let mut stack: Vec<(u8, OutlineItem)> = vec![];
    let close = |stack: &mut Vec<(u8, OutlineItem)>, roots: &mut Vec<OutlineItem>| {
        let (_, item) = stack.pop().expect("an open heading");
        match stack.last_mut() {
            Some((_, parent)) => parent.children.push(item),
            None => roots.push(item),
        }
    };
    for m in outline.iter().filter(|m| m.page < count) {
        while stack.last().is_some_and(|(lv, _)| *lv >= m.level) {
            close(&mut stack, &mut roots);
        }
        stack.push((m.level, OutlineItem { title: m.title.clone(), target: to(m.page, m.y), children: vec![] }));
    }
    while !stack.is_empty() {
        close(&mut stack, &mut roots);
    }
    let how = Assembly {
        links,
        outline: (!roots.is_empty()).then_some(roots),
        info: Some(InfoPatch { title: Some(title.to_string()), creator: Some("Pomelo OS · 见字如面".into()), ..Default::default() }),
    };
    let parts: Vec<&[u8]> = pages.iter().map(Vec::as_slice).collect();
    engine_pdf::compose::assemble(&parts, &how, &engine_pdf::Cancel::new()).map_err(|e| match e {
        engine_pdf::Error::NotPdf | engine_pdf::Error::Damaged => "WebKit 画出的页是空的".to_string(),
        e => e.to_string(),
    })
}

/// A one-page PDF (a box of `w` × `h` points) drawn as a PNG at `scale` (white under it).
pub fn png_of_pdf(pdf: &[u8], w: f64, h: f64, scale: f64) -> Result<Vec<u8>, String> {
    let side = (w.max(h) * scale).round().max(1.0) as u32;
    let doc = crate::youzi::pdf::open(pdf)?;
    Ok(doc.render(1, engine_pdf::ImageFormat::Png, side).map_err(|_| "画不出来".to_string())?.bytes)
}

// ---------- AppKit: an image as JPEG ----------

#[repr(C)]
#[derive(Clone, Copy)]
struct Size {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CgRect {
    origin: Point,
    size: Size,
}

unsafe impl Encode for Size {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl Encode for Point {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl Encode for CgRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
}

/// An NSData's bytes.
unsafe fn bytes_of(data: &AnyObject) -> Vec<u8> {
    let len: usize = msg_send![data, length];
    let ptr: *const u8 = msg_send![data, bytes];
    if ptr.is_null() || len == 0 {
        return vec![];
    }
    std::slice::from_raw_parts(ptr, len).to_vec()
}

/// NSBitmapImageFileTypeJPEG
const JPEG: usize = 3;

/// A picture Word can't show (HEIC, WebP, AVIF…) as a JPEG.
pub fn jpeg_of_image(data: &[u8]) -> Option<Vec<u8>> {
    autoreleasepool(|_| unsafe {
        let ns_data_class = AnyClass::get(c"NSData")?;
        let bytes: Retained<AnyObject> = msg_send![ns_data_class, dataWithBytes: data.as_ptr().cast::<c_void>(), length: data.len()];
        let image_class = AnyClass::get(c"NSImage")?;
        let alloc: Allocated<AnyObject> = msg_send![image_class, alloc];
        let image: Option<Retained<AnyObject>> = msg_send![alloc, initWithData: &*bytes];
        let tiff: Option<Retained<AnyObject>> = msg_send![&*image?, TIFFRepresentation];
        let rep_class = AnyClass::get(c"NSBitmapImageRep")?;
        let alloc: Allocated<AnyObject> = msg_send![rep_class, alloc];
        let rep: Option<Retained<AnyObject>> = msg_send![alloc, initWithData: &*tiff?];
        let dict_class = AnyClass::get(c"NSDictionary")?;
        let props: Retained<AnyObject> = msg_send![dict_class, dictionary];
        let out: Option<Retained<AnyObject>> = msg_send![&*rep?, representationUsingType: JPEG, properties: &*props];
        Some(bytes_of(&*out?))
    })
}

// ---------- commands (the print page) ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: u64,
    pub items: Vec<ItemView>,
    pub options: Options,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub files: Vec<attachments::FileInfo>,
}

/// The job now (the page's address carries its number too).
#[tauri::command]
pub fn print_job(print: State<Print>) -> Result<JobView, String> {
    let id = print.job.lock().map_err(|_| "导出出错了".to_string())?.as_ref().map(|j| j.id).ok_or("没有在导出")?;
    print.with(id, |j| {
        Ok(JobView {
            id,
            options: j.options.clone(),
            items: j.items.iter().map(|p| ItemView { id: p.id, title: p.title.clone(), content: p.content.clone(), tags: p.tags.clone(), created_at: p.created_at.clone(), updated_at: p.updated_at.clone(), files: p.files.clone() }).collect(),
        })
    })
}

/// The page shown now becomes the next page of the PDF being made.
#[tauri::command]
pub async fn print_page(app: AppHandle, job: u64) -> Result<usize, String> {
    let pdf = window_pdf(&app, PAGE.0, PAGE.1).await?;
    app.state::<Print>().with(job, |j| {
        j.pages.push(pdf);
        Ok(j.pages.len())
    })
}

/// The box at the window's top-left (w × h) as a picture for Word; returns its number.
#[tauri::command]
pub async fn print_picture(app: AppHandle, job: u64, w: f64, h: f64) -> Result<usize, String> {
    let (w, h) = (w.clamp(1.0, PAGE.0 + 40.0), h.clamp(1.0, 900.0));
    let pdf = window_pdf(&app, w, h).await?;
    let png = tauri::async_runtime::spawn_blocking(move || png_of_pdf(&pdf, w, h, 3.0)).await.map_err(|e| e.to_string())??;
    app.state::<Print>().with(job, |j| {
        j.pictures.push(docx::Picture { bytes: png, ext: "png".into() });
        Ok(j.pictures.len() - 1)
    })
}

/// Item `index` written or not: counted (a failure noted under its title), the page told.
fn settle(app: &AppHandle, j: &mut Job, index: usize, made: Result<(), String>) {
    match made {
        Ok(()) => j.written += 1,
        Err(what) => {
            j.failed += 1;
            j.left.push(Left { title: j.items.get(index).map(|i| i.title.clone()).unwrap_or_default(), what });
        }
    }
    let _ = app.emit_to("main", "articles-export", serde_json::json!({ "done": j.written + j.failed, "total": j.items.len() }));
}

/// Item `index`'s pages together, written to its place.
#[tauri::command]
pub async fn print_pdf_save(app: AppHandle, job: u64, index: usize, links: Vec<PdfLink>, outline: Vec<Mark>) -> Result<(), String> {
    let print = app.state::<Print>();
    let (pages, path, title) = print.with(job, |j| {
        let it = j.items.get(index).ok_or("没有这一篇")?;
        Ok((std::mem::take(&mut j.pages), it.path.clone(), it.title.clone()))
    })?;
    let made = tauri::async_runtime::spawn_blocking(move || assemble(&pages, &links, &outline, &title).and_then(|pdf| write_new(&path, &pdf))).await.map_err(|e| e.to_string())?;
    print.with(job, |j| {
        settle(&app, j, index, made);
        Ok(())
    })
}

/// Item `index` as Word, written to its place.
#[tauri::command]
pub async fn print_docx_save(app: AppHandle, state: State<'_, AppState>, job: u64, index: usize, doc: docx::Doc) -> Result<(), String> {
    let print = app.state::<Print>();
    let (pictures, item, options) = print.with(job, |j| Ok((std::mem::take(&mut j.pictures), j.items.get(index).cloned().ok_or("没有这一篇")?, j.options.clone())))?;
    let dir = attachments::folder_of(&state.paths.root);
    // the article's own pictures, read here
    let mut files = std::collections::HashMap::new();
    state.with_db(|c| {
        for f in item.files.iter().filter(|f| f.kind == "image" && !f.missing) {
            if let Ok(p) = attachments::path_of(c, &dir, f.id) {
                if let Ok(b) = std::fs::read(p) {
                    files.insert(f.id, (b, f.name.clone()));
                }
            }
        }
        Ok(())
    })?;
    let path = item.path.clone();
    let made = tauri::async_runtime::spawn_blocking(move || docx::write(&doc, &options, &pictures, &files).and_then(|b| write_new(&path, &b))).await.map_err(|e| e.to_string())?;
    print.with(job, |j| {
        settle(&app, j, index, made);
        Ok(())
    })
}

/// An article the page couldn't draw.
#[tauri::command]
pub fn print_fail(app: AppHandle, print: State<Print>, job: u64, index: usize, error: String) -> Result<(), String> {
    print.with(job, |j| {
        j.pages.clear();
        j.pictures.clear();
        settle(&app, j, index, Err(error));
        Ok(())
    })
}

#[tauri::command]
pub fn print_done(print: State<Print>, job: u64) -> Result<(), String> {
    print.with(job, |j| {
        if let Some(tx) = j.done.take() {
            let _ = tx.send(());
        }
        Ok(())
    })
}
