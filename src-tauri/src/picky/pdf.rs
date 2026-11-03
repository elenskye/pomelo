//! Text of a PDF, page by page, with macOS's PDFKit (no extra crate: the framework is linked and
//! called through objc2). Works on the bytes given; never follows links or companion files.
//! Pub2MD (v0.6) also draws each page as a JPEG (`render_pages`, the 原页图) and packs images into one PDF
//! (`images_to_pdf`: AppKit's NSImage reads every format macOS can, HEIC included, upright by its EXIF).
//! Also SHA-256 through CommonCrypto (libSystem), for storing each file's bytes once.

use objc2::encode::{Encode, Encoding};
use objc2::rc::{autoreleasepool, Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::msg_send;
use objc2_foundation::NSString;
use std::ffi::{c_char, c_void, CStr};
use std::path::{Path, PathBuf};

#[link(name = "PDFKit", kind = "framework")]
extern "C" {}

extern "C" {
    fn CC_SHA256(data: *const c_void, len: u32, md: *mut u8) -> *mut u8;
}

pub fn sha256_hex(data: &[u8]) -> String {
    assert!(data.len() <= u32::MAX as usize);
    let mut out = [0u8; 32];
    unsafe { CC_SHA256(data.as_ptr().cast(), data.len() as u32, out.as_mut_ptr()) };
    out.iter().map(|b| format!("{b:02x}")).collect()
}

/// The document (inside an autorelease pool).
unsafe fn open(data: &[u8]) -> Result<Retained<AnyObject>, String> {
    let ns_data_class = AnyClass::get(c"NSData").ok_or("读不了 PDF")?;
    let bytes: Retained<AnyObject> = msg_send![ns_data_class, dataWithBytes: data.as_ptr().cast::<c_void>(), length: data.len()];
    let doc_class = AnyClass::get(c"PDFDocument").ok_or("读不了 PDF：系统的 PDFKit 不可用")?;
    let alloc: Allocated<AnyObject> = msg_send![doc_class, alloc];
    let doc: Option<Retained<AnyObject>> = msg_send![alloc, initWithData: &*bytes];
    let doc = doc.ok_or("PDF 文件已损坏，无法读取")?;
    let locked: bool = msg_send![&*doc, isLocked];
    if locked {
        return Err("PDF 已加密，请先解除密码后再添加".to_string());
    }
    Ok(doc)
}

/// Text of every page (empty for pages without text).
pub fn pages(data: &[u8]) -> Result<Vec<String>, String> {
    autoreleasepool(|_| unsafe {
        let doc = open(data)?;
        let count: usize = msg_send![&*doc, pageCount];
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            autoreleasepool(|_| {
                let page: Option<Retained<AnyObject>> = msg_send![&*doc, pageAtIndex: i];
                let text: Option<Retained<NSString>> = match &page {
                    Some(p) => msg_send![&**p, string],
                    None => None,
                };
                out.push(text.map(|s| s.to_string()).unwrap_or_default());
            });
        }
        Ok(out)
    })
}

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
struct Rect {
    origin: Point,
    size: Size,
}

unsafe impl Encode for Size {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl Encode for Point {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
}

/// kPDFDisplayBoxMediaBox
const MEDIA_BOX: isize = 0;
/// NSBitmapImageFileTypeJPEG
const JPEG: usize = 3;

unsafe fn ns_string(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
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

/// An NSImage as JPEG bytes (quality 0.85).
unsafe fn jpeg_of(image: &AnyObject) -> Option<Vec<u8>> {
    let tiff: Option<Retained<AnyObject>> = msg_send![image, TIFFRepresentation];
    let rep_class = AnyClass::get(c"NSBitmapImageRep")?;
    let alloc: Allocated<AnyObject> = msg_send![rep_class, alloc];
    let rep: Option<Retained<AnyObject>> = msg_send![alloc, initWithData: &*tiff?];
    let number_class = AnyClass::get(c"NSNumber")?;
    let quality: Retained<AnyObject> = msg_send![number_class, numberWithDouble: 0.85f64];
    let dict_class = AnyClass::get(c"NSDictionary")?;
    let key = ns_string("NSImageCompressionFactor");
    let props: Retained<AnyObject> = msg_send![dict_class, dictionaryWithObject: &*quality, forKey: &*key];
    let jpeg: Option<Retained<AnyObject>> = msg_send![&*rep?, representationUsingType: JPEG, properties: &*props];
    Some(bytes_of(&*jpeg?))
}

/// Draws every page as `page-0001.jpg`… in `dir` (the longer side `max_side` px). Returns the page count.
pub fn render_pages(data: &[u8], dir: &Path, max_side: f64) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("没能准备原页图的文件夹：{e}"))?;
    autoreleasepool(|_| unsafe {
        let doc = open(data)?;
        let count: usize = msg_send![&*doc, pageCount];
        for i in 0..count {
            autoreleasepool(|_| -> Result<(), String> {
                let jpeg = jpeg_page(&doc, i, max_side)?;
                std::fs::write(dir.join(page_name(i + 1)), jpeg).map_err(|e| format!("没能存下原页图：{e}"))
            })?;
        }
        Ok(count)
    })
}


unsafe fn jpeg_page(doc: &AnyObject, i: usize, max_side: f64) -> Result<Vec<u8>, String> {
    let page: Option<Retained<AnyObject>> = msg_send![doc, pageAtIndex: i];
    let page = page.ok_or_else(|| format!("PDF 的第 {} 页读不出来", i + 1))?;
    let rect: Rect = msg_send![&*page, boundsForBox: MEDIA_BOX];
    let rotation: isize = msg_send![&*page, rotation];
    let (mut w, mut h) = (rect.size.width.max(1.0), rect.size.height.max(1.0));
    if rotation.rem_euclid(180) == 90 {
        std::mem::swap(&mut w, &mut h);
    }
    let scale = max_side / w.max(h);
    let size = Size { width: (w * scale).round(), height: (h * scale).round() };
    let image: Option<Retained<AnyObject>> = msg_send![&*page, thumbnailOfSize: size, forBox: MEDIA_BOX];
    let jpeg = image.and_then(|im| jpeg_of(&im)).ok_or_else(|| format!("没能画出 PDF 的第 {} 页", i + 1))?;
    Ok(jpeg)
}

// ---------- which pages hold images (Quartz's PDF objects: PDFKit doesn't say) ----------

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGDataProviderCreateWithData(info: *mut c_void, data: *const c_void, size: usize, release: *const c_void) -> *mut c_void;
    fn CGDataProviderRelease(provider: *mut c_void);
    fn CGPDFDocumentCreateWithProvider(provider: *mut c_void) -> *mut c_void;
    fn CGPDFDocumentRelease(doc: *mut c_void);
    fn CGPDFDocumentGetNumberOfPages(doc: *mut c_void) -> usize;
    fn CGPDFDocumentGetPage(doc: *mut c_void, number: usize) -> *mut c_void;
    fn CGPDFPageGetDictionary(page: *mut c_void) -> *mut c_void;
    fn CGPDFDictionaryGetDictionary(dict: *mut c_void, key: *const c_char, out: *mut *mut c_void) -> bool;
    fn CGPDFDictionaryGetName(dict: *mut c_void, key: *const c_char, out: *mut *const c_char) -> bool;
    fn CGPDFDictionaryGetInteger(dict: *mut c_void, key: *const c_char, out: *mut std::ffi::c_long) -> bool;
    fn CGPDFDictionaryApplyFunction(dict: *mut c_void, f: extern "C" fn(*const c_char, *mut c_void, *mut c_void), info: *mut c_void);
    fn CGPDFObjectGetValue(object: *mut c_void, kind: i32, out: *mut c_void) -> bool;
    fn CGPDFStreamGetDictionary(stream: *mut c_void) -> *mut c_void;
}

/// kCGPDFObjectTypeStream
const PDF_STREAM: i32 = 9;
/// Smaller ones are rules, bullets, masks.
const MIN_IMAGE_SIDE: i64 = 32;
/// Forms inside forms, this deep.
const MAX_FORMS: u32 = 3;

struct Counting {
    images: usize,
    depth: u32,
}

unsafe fn xobjects_of(resources: *mut c_void, count: &mut Counting) {
    let mut xobjects = std::ptr::null_mut();
    if CGPDFDictionaryGetDictionary(resources, c"XObject".as_ptr(), &mut xobjects) {
        CGPDFDictionaryApplyFunction(xobjects, count_xobject, (count as *mut Counting).cast());
    }
}

extern "C" fn count_xobject(_: *const c_char, object: *mut c_void, info: *mut c_void) {
    unsafe {
        let count = &mut *info.cast::<Counting>();
        let mut stream = std::ptr::null_mut::<c_void>();
        if !CGPDFObjectGetValue(object, PDF_STREAM, (&mut stream as *mut *mut c_void).cast()) {
            return;
        }
        let dict = CGPDFStreamGetDictionary(stream);
        let mut subtype = std::ptr::null();
        if !CGPDFDictionaryGetName(dict, c"Subtype".as_ptr(), &mut subtype) {
            return;
        }
        match CStr::from_ptr(subtype).to_bytes() {
            b"Image" => {
                let (mut w, mut h): (std::ffi::c_long, std::ffi::c_long) = (0, 0);
                CGPDFDictionaryGetInteger(dict, c"Width".as_ptr(), &mut w);
                CGPDFDictionaryGetInteger(dict, c"Height".as_ptr(), &mut h);
                if w as i64 >= MIN_IMAGE_SIDE && h as i64 >= MIN_IMAGE_SIDE {
                    count.images += 1;
                }
            }
            b"Form" if count.depth < MAX_FORMS => {
                let mut resources = std::ptr::null_mut();
                if CGPDFDictionaryGetDictionary(dict, c"Resources".as_ptr(), &mut resources) {
                    count.depth += 1;
                    xobjects_of(resources, count);
                    count.depth -= 1;
                }
            }
            _ => {}
        }
    }
}

/// The pages (1-based) holding images, with how many (their own or their parent's resources).
pub fn image_pages(data: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    // PDFKit says when it's broken or locked
    autoreleasepool(|_| unsafe { open(data).map(|_| ()) })?;
    unsafe {
        let provider = CGDataProviderCreateWithData(std::ptr::null_mut(), data.as_ptr().cast(), data.len(), std::ptr::null());
        if provider.is_null() {
            return Err("PDF 文件已损坏，无法读取".into());
        }
        let doc = CGPDFDocumentCreateWithProvider(provider);
        if doc.is_null() {
            CGDataProviderRelease(provider);
            return Err("PDF 文件已损坏，无法读取".into());
        }
        let mut out = vec![];
        for n in 1..=CGPDFDocumentGetNumberOfPages(doc) {
            let page = CGPDFDocumentGetPage(doc, n);
            if page.is_null() {
                continue;
            }
            let mut dict = CGPDFPageGetDictionary(page);
            let mut resources = std::ptr::null_mut();
            // resources may be inherited from the page tree
            for _ in 0..16 {
                if CGPDFDictionaryGetDictionary(dict, c"Resources".as_ptr(), &mut resources) {
                    break;
                }
                let mut parent = std::ptr::null_mut();
                if !CGPDFDictionaryGetDictionary(dict, c"Parent".as_ptr(), &mut parent) {
                    break;
                }
                dict = parent;
            }
            let mut count = Counting { images: 0, depth: 0 };
            if !resources.is_null() {
                xobjects_of(resources, &mut count);
            }
            if count.images > 0 {
                out.push((n, count.images));
            }
        }
        CGPDFDocumentRelease(doc);
        CGDataProviderRelease(provider);
        Ok(out)
    }
}

/// Render one page in memory for local OCR; no original or page image is retained.
pub fn page_jpeg(data: &[u8], index: usize, max_side: f64) -> Result<Vec<u8>, String> {
    autoreleasepool(|_| unsafe { jpeg_page(&*open(data)?, index, max_side) })
}

/// The file name of page `n`'s image (1-based).
pub fn page_name(n: usize) -> String {
    format!("page-{n:04}.jpg")
}

/// Images (in this order, one page each) packed into a PDF.
pub fn images_to_pdf(paths: &[PathBuf]) -> Result<Vec<u8>, String> {
    autoreleasepool(|_| unsafe {
        let doc_class = AnyClass::get(c"PDFDocument").ok_or("系统的 PDFKit 不可用")?;
        let image_class = AnyClass::get(c"NSImage").ok_or("系统的 AppKit 不可用")?;
        let page_class = AnyClass::get(c"PDFPage").ok_or("系统的 PDFKit 不可用")?;
        let alloc: Allocated<AnyObject> = msg_send![doc_class, alloc];
        let doc: Retained<AnyObject> = msg_send![alloc, init];
        let mut at = 0usize;
        for p in paths {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("图片");
            let alloc: Allocated<AnyObject> = msg_send![image_class, alloc];
            let path = ns_string(&p.to_string_lossy());
            let image: Option<Retained<AnyObject>> = msg_send![alloc, initWithContentsOfFile: &*path];
            let image = image.ok_or_else(|| format!("读不了图片「{name}」"))?;
            // a TIFF of several pages: a PDF page each
            let mut images: Vec<Retained<AnyObject>> = vec![];
            if matches!(p.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(), Some("tif" | "tiff")) {
                let reps: Retained<AnyObject> = msg_send![&*image, representations];
                let n: usize = msg_send![&*reps, count];
                for k in 0..if n > 1 { n } else { 0 } {
                    let rep: *mut AnyObject = msg_send![&*reps, objectAtIndex: k];
                    let size: Size = msg_send![rep, size];
                    let alloc: Allocated<AnyObject> = msg_send![image_class, alloc];
                    let one: Retained<AnyObject> = msg_send![alloc, initWithSize: size];
                    let _: () = msg_send![&*one, addRepresentation: rep];
                    images.push(one);
                }
            }
            if images.is_empty() {
                images.push(image);
            }
            for image in images {
                let alloc: Allocated<AnyObject> = msg_send![page_class, alloc];
                let page: Option<Retained<AnyObject>> = msg_send![alloc, initWithImage: &*image];
                let page = page.ok_or_else(|| format!("图片「{name}」放不进 PDF"))?;
                let _: () = msg_send![&*doc, insertPage: &*page, atIndex: at];
                at += 1;
            }
        }
        let data: Option<Retained<AnyObject>> = msg_send![&*doc, dataRepresentation];
        let bytes = data.map(|d| bytes_of(&d)).unwrap_or_default();
        if bytes.is_empty() {
            return Err("没能把图片合成 PDF".into());
        }
        Ok(bytes)
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A small valid PDF with one line of text per page.
    pub fn sample_pdf(pages: &[&str]) -> Vec<u8> {
        let n = pages.len();
        let mut objs: Vec<String> = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            format!("<< /Type /Pages /Kids [{}] /Count {n} >>", (0..n).map(|i| format!("{} 0 R", 4 + i * 2)).collect::<Vec<_>>().join(" ")),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        ];
        for (i, text) in pages.iter().enumerate() {
            let stream = format!("BT /F1 18 Tf 20 100 Td ({text}) Tj ET");
            objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>", 5 + i * 2));
            objs.push(format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()));
        }
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![];
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    #[test]
    fn reads_pages_and_refuses_broken_files() {
        let pdf = sample_pdf(&["Hello page one", "Second page"]);
        let p = pages(&pdf).unwrap();
        assert_eq!(p.len(), 2);
        assert!(p[0].contains("Hello page one") && p[1].contains("Second page"), "{p:?}");
        assert!(pages(b"not a pdf").is_err());
    }

    #[test]
    fn pages_become_jpegs_and_images_a_pdf() {
        let dir = crate::db::tests::temp_dir("pdf-render");
        let pdf = sample_pdf(&["Page one", "Page two"]);
        assert_eq!(render_pages(&pdf, &dir.join("pages"), 400.0).unwrap(), 2);
        for n in [1, 2] {
            let jpg = std::fs::read(dir.join("pages").join(page_name(n))).unwrap();
            assert_eq!(&jpg[..3], &[0xFF, 0xD8, 0xFF]);
        }
        // 300 × 200 points → 400 px wide
        let (w, h) = crate::picky::office::size_of(&dir.join("pages/page-0001.jpg")).unwrap();
        assert!(w == 400 && (266..=267).contains(&h), "{w} × {h}");
        let png = dir.join("a.png");
        std::fs::write(&png, crate::picky::office::tests_png()).unwrap();
        let out = images_to_pdf(&[png.clone(), png.clone()]).unwrap();
        assert!(out.starts_with(b"%PDF"));
        assert_eq!(pages(&out).unwrap().len(), 2);
        assert!(images_to_pdf(&[dir.join("missing.png")]).unwrap_err().contains("missing.png"));
        // a TIFF of two pages: two PDF pages (plus the PNG)
        let tif = |name: &str| {
            let p = dir.join(name);
            assert!(std::process::Command::new("/usr/bin/sips").args(["-s", "format", "tiff"]).arg(&png).arg("--out").arg(&p).output().unwrap().status.success());
            p
        };
        let (a, b, two) = (tif("a.tif"), tif("b.tif"), dir.join("two.tiff"));
        assert!(std::process::Command::new("/usr/bin/tiffutil").arg("-cat").arg(&a).arg(&b).arg("-out").arg(&two).output().unwrap().status.success());
        assert_eq!(pages(&images_to_pdf(&[two, png.clone()]).unwrap()).unwrap().len(), 3);
        assert_eq!(pages(&images_to_pdf(&[a]).unwrap()).unwrap().len(), 1);
    }

    #[test]
    fn says_which_pages_hold_images() {
        let dir = crate::db::tests::temp_dir("pdf-images");
        let small = dir.join("small.png");
        std::fs::write(&small, crate::picky::office::tests_png()).unwrap();
        let big = dir.join("big.png");
        assert!(std::process::Command::new("/usr/bin/sips").args(["-z", "64", "64"]).arg(&small).arg("--out").arg(&big).output().unwrap().status.success());
        // a page with no image; pages that are one big image, one too small to count
        assert_eq!(image_pages(&sample_pdf(&["only text", "more text"])).unwrap(), []);
        let pdf = images_to_pdf(&[big.clone(), small, big]).unwrap();
        assert_eq!(image_pages(&pdf).unwrap(), [(1, 1), (3, 1)]);
        assert!(image_pages(b"not a pdf").is_err());
    }

    #[test]
    fn sha256() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}

