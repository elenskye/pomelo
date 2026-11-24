//! The print page (print.html in a hidden window `print`): 见字如面's PDF and Word are drawn there, one article at a
//! time, as A4 pages (1 CSS px = 1 pt), and come back here. Rules:
//! - PDF: the page shows one page at a time at its top-left; WebKit draws it (`createPDF` of 595 × 842), and PDFKit
//!   puts the pages together with the contents' links (to a page and height), web links, an outline (the headings)
//!   and the title. Text stays text.
//! - Word: the page sends the article as blocks (docx.rs writes the file); formulas and diagrams become pictures, drawn
//!   the same way (a PDF of their box, rendered by PDFKit at 3×).
//! - One export at a time. The job (articles, where each goes, options) is Rust's; the page asks for it, reports each
//!   file (written, or why not) and finishes; the window is closed then (or after 30 minutes).

use crate::modules::articles::docx;
use crate::modules::articles::export::{write_new, Options, Planned};
use crate::modules::articles::import::Left;
use crate::modules::attachments;
use crate::youzi::pdf::{bytes_of, ns_string, open, Point, Rect, Size, MEDIA_BOX};
use crate::AppState;
use objc2::rc::{autoreleasepool, Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
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
        let rect = Rect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: w, height: h } };
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

// ---------- PDFKit: pages together, links, outline ----------

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

unsafe fn alloc(class: &std::ffi::CStr) -> Result<Allocated<AnyObject>, String> {
    let c = AnyClass::get(class).ok_or("系统的 PDFKit 不可用")?;
    Ok(msg_send![c, alloc])
}

unsafe fn destination(page: &AnyObject, x: f64, y: f64) -> Result<Retained<AnyObject>, String> {
    let a = alloc(c"PDFDestination")?;
    Ok(msg_send![a, initWithPage: page, atPoint: Point { x, y }])
}

/// The pages (one-page PDFs) as one document with its links, outline and title.
pub fn assemble(pages: &[Vec<u8>], links: &[PdfLink], outline: &[Mark], title: &str) -> Result<Vec<u8>, String> {
    if pages.is_empty() {
        return Err("没有页".into());
    }
    autoreleasepool(|_| unsafe {
        let doc = open(&pages[0])?;
        for (i, bytes) in pages.iter().enumerate().skip(1) {
            let one = open(bytes)?;
            let p: Option<Retained<AnyObject>> = msg_send![&*one, pageAtIndex: 0usize];
            let p = p.ok_or("WebKit 画出的页是空的")?;
            let _: () = msg_send![&*doc, insertPage: &*p, atIndex: i];
        }
        let count: usize = msg_send![&*doc, pageCount];
        let page_at = |i: usize| -> Option<Retained<AnyObject>> {
            if i >= count {
                return None;
            }
            msg_send![&*doc, pageAtIndex: i]
        };
        let height = |p: &AnyObject| -> f64 {
            let r: Rect = msg_send![p, boundsForBox: MEDIA_BOX];
            r.size.height
        };
        let link_type = ns_string("Link");
        for l in links {
            let Some(page) = page_at(l.page) else { continue };
            let h = height(&page);
            let bounds = Rect { origin: Point { x: l.x, y: h - l.y - l.h }, size: Size { width: l.w, height: l.h } };
            let a = alloc(c"PDFAnnotation")?;
            let nil: *mut AnyObject = std::ptr::null_mut();
            let ann: Retained<AnyObject> = msg_send![a, initWithBounds: bounds, forType: &*link_type, withProperties: nil];
            if let (Some(tp), Some(ty)) = (l.to_page, l.to_y) {
                let Some(target) = page_at(tp) else { continue };
                let th = height(&target);
                let d = destination(&target, 0.0, (th - ty).clamp(0.0, th))?;
                let _: () = msg_send![&*ann, setDestination: &*d];
            } else if let Some(u) = &l.url {
                let url_class = AnyClass::get(c"NSURL").ok_or("系统的 Foundation 不可用")?;
                let url: Option<Retained<AnyObject>> = msg_send![url_class, URLWithString: &*ns_string(u)];
                let Some(url) = url else { continue };
                let _: () = msg_send![&*ann, setURL: &*url];
            } else {
                continue;
            }
            let b: Retained<AnyObject> = msg_send![alloc(c"PDFBorder")?, init];
            let _: () = msg_send![&*b, setLineWidth: 0.0f64];
            let _: () = msg_send![&*ann, setBorder: &*b];
            let _: () = msg_send![&*page, addAnnotation: &*ann];
        }
        if !outline.is_empty() {
            let root: Retained<AnyObject> = msg_send![alloc(c"PDFOutline")?, init];
            let mut stack: Vec<(u8, Retained<AnyObject>)> = vec![(0, root.clone())];
            for m in outline {
                let Some(page) = page_at(m.page) else { continue };
                let h = height(&page);
                let item: Retained<AnyObject> = msg_send![alloc(c"PDFOutline")?, init];
                let _: () = msg_send![&*item, setLabel: &*ns_string(&m.title)];
                let d = destination(&page, 0.0, (h - m.y).clamp(0.0, h))?;
                let _: () = msg_send![&*item, setDestination: &*d];
                while stack.len() > 1 && stack.last().is_some_and(|(lv, _)| *lv >= m.level) {
                    stack.pop();
                }
                let parent = &stack.last().ok_or("目录出错了")?.1;
                let n: usize = msg_send![&**parent, numberOfChildren];
                let _: () = msg_send![&**parent, insertChild: &*item, atIndex: n];
                stack.push((m.level, item));
            }
            let _: () = msg_send![&*doc, setOutlineRoot: &*root];
        }
        let dict_class = AnyClass::get(c"NSMutableDictionary").ok_or("系统的 Foundation 不可用")?;
        let attrs: Retained<AnyObject> = msg_send![dict_class, dictionary];
        let _: () = msg_send![&*attrs, setObject: &*ns_string(title), forKey: &*ns_string("Title")];
        let _: () = msg_send![&*attrs, setObject: &*ns_string("Pomelo OS · 见字如面"), forKey: &*ns_string("Creator")];
        let _: () = msg_send![&*doc, setDocumentAttributes: &*attrs];
        let data: Option<Retained<AnyObject>> = msg_send![&*doc, dataRepresentation];
        Ok(bytes_of(&*data.ok_or("没能合成 PDF")?))
    })
}

/// A one-page PDF drawn as a PNG at `scale` (white under it).
pub fn png_of_pdf(pdf: &[u8], scale: f64) -> Result<Vec<u8>, String> {
    autoreleasepool(|_| unsafe {
        let doc = open(pdf)?;
        let page: Option<Retained<AnyObject>> = msg_send![&*doc, pageAtIndex: 0usize];
        let page = page.ok_or("画不出来")?;
        let r: Rect = msg_send![&*page, boundsForBox: MEDIA_BOX];
        let size = Size { width: (r.size.width * scale).round().max(1.0), height: (r.size.height * scale).round().max(1.0) };
        let image: Option<Retained<AnyObject>> = msg_send![&*page, thumbnailOfSize: size, forBox: MEDIA_BOX];
        png_of_image(&*image.ok_or("画不出来")?).ok_or_else(|| "画不出来".to_string())
    })
}

/// NSBitmapImageFileTypePNG / JPEG
const PNG: usize = 4;
const JPEG: usize = 3;

unsafe fn rep_of(image: &AnyObject) -> Option<Retained<AnyObject>> {
    let tiff: Option<Retained<AnyObject>> = msg_send![image, TIFFRepresentation];
    let rep_class = AnyClass::get(c"NSBitmapImageRep")?;
    let alloc: Allocated<AnyObject> = msg_send![rep_class, alloc];
    msg_send![alloc, initWithData: &*tiff?]
}

unsafe fn encode_rep(rep: &AnyObject, kind: usize) -> Option<Vec<u8>> {
    let dict_class = AnyClass::get(c"NSDictionary")?;
    let props: Retained<AnyObject> = msg_send![dict_class, dictionary];
    let out: Option<Retained<AnyObject>> = msg_send![rep, representationUsingType: kind, properties: &*props];
    Some(bytes_of(&*out?))
}

unsafe fn png_of_image(image: &AnyObject) -> Option<Vec<u8>> {
    encode_rep(&*rep_of(image)?, PNG)
}

/// A picture Word can't show (HEIC, WebP, AVIF…) as a JPEG.
pub fn jpeg_of_image(data: &[u8]) -> Option<Vec<u8>> {
    autoreleasepool(|_| unsafe {
        let ns_data_class = AnyClass::get(c"NSData")?;
        let bytes: Retained<AnyObject> = msg_send![ns_data_class, dataWithBytes: data.as_ptr().cast::<c_void>(), length: data.len()];
        let image_class = AnyClass::get(c"NSImage")?;
        let alloc: Allocated<AnyObject> = msg_send![image_class, alloc];
        let image: Option<Retained<AnyObject>> = msg_send![alloc, initWithData: &*bytes];
        encode_rep(&*rep_of(&*image?)?, JPEG)
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
    let pdf = window_pdf(&app, w.clamp(1.0, PAGE.0 + 40.0), h.clamp(1.0, 900.0)).await?;
    let png = tauri::async_runtime::spawn_blocking(move || png_of_pdf(&pdf, 3.0)).await.map_err(|e| e.to_string())??;
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
