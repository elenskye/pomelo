//! PDFs through engine-pdf (the shared engine, v1.2: PDFKit, PDFium putting back lost ligatures, Vision). Pomelo
//! keeps what it does with them; this only adapts: page texts as attachments and the knowledge base take them,
//! Pomelo's own wording for files that can't be opened, the page images' file names. Works on the bytes given;
//! never follows links or companion files. Also SHA-256 through CommonCrypto (libSystem), for storing each
//! file's bytes once.

use engine_pdf::compose::{self, ImageSource};
use engine_pdf::{Cancel, Content, Document, Error, ImageFormat, Ocr, ReadOptions};
use std::ffi::c_void;
use std::path::{Path, PathBuf};

extern "C" {
    fn CC_SHA256(data: *const c_void, len: u32, md: *mut u8) -> *mut u8;
}

pub fn sha256_hex(data: &[u8]) -> String {
    assert!(data.len() <= u32::MAX as usize);
    let mut out = [0u8; 32];
    unsafe { CC_SHA256(data.as_ptr().cast(), data.len() as u32, out.as_mut_ptr()) };
    out.iter().map(|b| format!("{b:02x}")).collect()
}

/// Pomelo's words for a PDF that can't be opened (知识库 keeps them as the file's error).
pub fn error_text(e: Error) -> String {
    match e {
        Error::Locked => "PDF 已加密，请先解除密码后再添加".into(),
        Error::NotPdf | Error::Damaged => "PDF 文件已损坏，无法读取".into(),
        e => e.to_string(),
    }
}

pub fn open(data: &[u8]) -> Result<Document, String> {
    Document::from_bytes(data).map_err(error_text)
}

pub fn page_count(data: &[u8]) -> Result<usize, String> {
    Ok(open(data)?.page_count() as usize)
}

/// A page's own text ("" for a scan, a blank page, a garbled layer or a page that can't be read).
pub fn text_of(content: Content) -> String {
    match content {
        Content::Text(t) | Content::SearchableText(t) => t,
        Content::Ocr(o) => o.text,
        Content::NoText { partial, .. } => partial,
        Content::Blank | Content::Failed(_) => String::new(),
    }
}

/// Text of every page, from its text layer only.
pub fn pages(data: &[u8]) -> Result<Vec<String>, String> {
    let doc = open(data)?;
    let all: Vec<u32> = (1..=doc.page_count()).collect();
    let opts = ReadOptions { ocr: Ocr::Off, render: None, ..Default::default() };
    let read = engine_pdf::read(&doc, &all, &opts, &Cancel::new()).map_err(error_text)?;
    Ok(read.into_iter().map(|p| text_of(p.content)).collect())
}

/// Page `page` (1-based) as a JPEG, the longer side `max_side` px (what a reader shows: the CropBox).
pub fn page_jpeg(data: &[u8], page: usize, max_side: u32) -> Result<Vec<u8>, String> {
    Ok(open(data)?.render(page as u32, ImageFormat::Jpeg, max_side).map_err(error_text)?.bytes)
}

/// Draws every page as `page-0001.jpg`… in `dir` (the longer side `max_side` px). Returns the page count.
pub fn render_pages(data: &[u8], dir: &Path, max_side: u32) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("没能准备原页图的文件夹：{e}"))?;
    let doc = open(data)?;
    for n in 1..=doc.page_count() {
        let jpeg = doc.render(n, ImageFormat::Jpeg, max_side).map_err(error_text)?;
        std::fs::write(dir.join(page_name(n as usize)), jpeg.bytes).map_err(|e| format!("没能存下原页图：{e}"))?;
    }
    Ok(doc.page_count() as usize)
}

/// The pages (1-based) holding images, with how many.
pub fn image_pages(data: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    let doc = open(data)?;
    Ok(doc.image_counts(&Cancel::new()).map_err(error_text)?.into_iter().map(|(p, n)| (p as usize, n)).collect())
}

/// The file name of page `n`'s image (1-based).
pub fn page_name(n: usize) -> String {
    format!("page-{n:04}.jpg")
}

/// Images (in this order, one page each; a TIFF of several pages a page each) packed into a PDF.
pub fn images_to_pdf(paths: &[PathBuf]) -> Result<Vec<u8>, String> {
    let sources: Vec<ImageSource> = paths.iter().map(|p| ImageSource::Path(p)).collect();
    compose::images(&sources, &Cancel::new()).map_err(|e| match e {
        Error::ImageInput { index } => {
            let name = paths.get(index.saturating_sub(1)).and_then(|p| p.file_name()).map_or_else(|| "图片".into(), |n| n.to_string_lossy().to_string());
            format!("读不了图片「{name}」")
        }
        e => e.to_string(),
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
        assert_eq!(render_pages(&pdf, &dir.join("pages"), 400).unwrap(), 2);
        for n in [1, 2] {
            let jpg = std::fs::read(dir.join("pages").join(page_name(n))).unwrap();
            assert_eq!(&jpg[..3], &[0xFF, 0xD8, 0xFF]);
        }
        // 300 × 200 points → 400 px wide
        let (w, h) = crate::youzi::office::size_of(&dir.join("pages/page-0001.jpg")).unwrap();
        assert!(w == 400 && (266..=267).contains(&h), "{w} × {h}");
        let png = dir.join("a.png");
        std::fs::write(&png, crate::youzi::office::tests_png()).unwrap();
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
        std::fs::write(&small, crate::youzi::office::tests_png()).unwrap();
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


/// Phase 1 of 1.2.0 (engine-pdf): what the PDF code makes of real files, to compare before and after.
/// `POMELO_PDF_CORPUS` (folders, `:`-separated) → `POMELO_PDF_DUMP` (JSON).
#[cfg(test)]
mod compare {
    use super::*;

    fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                return None;
            }
            let m = b[i + 1];
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if matches!(m, 0xC0..=0xC3) {
                return Some((u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32, u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32));
            }
            i += 2 + len;
        }
        None
    }

    #[test]
    #[ignore]
    fn dump_corpus() {
        let (Ok(corpus), Ok(out)) = (std::env::var("POMELO_PDF_CORPUS"), std::env::var("POMELO_PDF_DUMP")) else { return };
        let mut files: Vec<PathBuf> = corpus.split(':').flat_map(|d| walk(Path::new(d))).collect();
        files.sort();
        let tmp = crate::db::tests::temp_dir("pdf-dump");
        let mut all = vec![];
        for f in files {
            let data = std::fs::read(&f).unwrap();
            let t = std::time::Instant::now();
            let read = crate::youzi::files::read(&f, true);
            let ms = t.elapsed().as_millis();
            let read = match read {
                Ok(p) => serde_json::json!({ "pages": p.page_count, "segments": p.segments.iter().map(|s| [s.label(), s.content.clone()]).collect::<Vec<_>>() }),
                Err(e) => serde_json::json!({ "error": e }),
            };
            let dir = tmp.join("pages");
            let _ = std::fs::remove_dir_all(&dir);
            let rendered = render_pages(&data, &dir, 1568);
            let dims: Vec<Option<(u32, u32)>> = match &rendered {
                Ok(n) => (1..=*n).map(|i| std::fs::read(dir.join(page_name(i))).ok().and_then(|b| jpeg_size(&b))).collect(),
                Err(_) => vec![],
            };
            all.push(serde_json::json!({
                "file": f.to_string_lossy(),
                "read": read,
                "read_ms": ms,
                "images": image_pages(&data).map_err(|e| e.to_string()),
                "images_text": crate::youzi::office::images_text("x", &data, "pdf"),
                "dims": dims,
                "render_error": rendered.err(),
            }));
            eprintln!("{} {ms} ms", f.display());
        }
        std::fs::write(out, serde_json::to_string_pretty(&all).unwrap()).unwrap();
    }

    fn walk(d: &Path) -> Vec<PathBuf> {
        let mut out = vec![];
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
                out.push(p);
            }
        }
        out
    }

    /// One A4 page (595 × 842, like WebKit's) with a line of text.
    fn a4(text: &str) -> Vec<u8> {
        let stream = format!("BT /F1 24 Tf 72 760 Td ({text}) Tj ET");
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".into(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 5 0 R /Resources << /Font << /F1 3 0 R >> >> >>".into(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        ];
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

    /// 见字如面's PDF: pages, links, outline and title, read back by engine-pdf.
    #[test]
    #[ignore]
    fn dump_assemble() {
        let Ok(out) = std::env::var("POMELO_PDF_DUMP") else { return };
        use crate::print::{assemble, Mark, PdfLink};
        let pages: Vec<Vec<u8>> = ["First page", "Second page", "Third page"].iter().map(|t| a4(t)).collect();
        let link = |page, x, y, to_page: Option<usize>, to_y: Option<f64>, url: Option<&str>| PdfLink { page, x, y, w: 120.0, h: 18.0, to_page, to_y, url: url.map(String::from) };
        let links = vec![
            link(0, 72.0, 100.0, Some(2), Some(300.0), None),
            link(1, 80.5, 400.25, None, None, Some("https://example.com/a?b=1&c=中文")),
            link(2, 72.0, 700.0, Some(0), Some(0.0), None),
            link(9, 72.0, 700.0, Some(0), Some(0.0), None),
            link(0, 72.0, 200.0, Some(9), Some(0.0), None),
        ];
        let mark = |level, title: &str, page, y| Mark { level, title: title.into(), page, y };
        let outline = vec![mark(1, "一、开头", 0, 72.0), mark(2, "1.1 小节", 0, 400.0), mark(3, "1.1.1 更小", 1, 50.0), mark(2, "1.2 另一节", 1, 600.0), mark(1, "二、结尾", 2, 100.0), mark(1, "越界", 7, 0.0)];
        let pdf = assemble(&pages, &links, &outline, "见字如面 · 测试").unwrap();
        let doc = engine_pdf::Document::from_bytes(&pdf).unwrap();
        let annotations: Vec<_> = (1..=doc.page_count()).map(|p| doc.annotations(p).unwrap()).collect();
        let info = doc.info();
        let dump = serde_json::json!({
            "pages": doc.page_count(),
            "texts": (1..=doc.page_count()).map(|p| doc.text(p).unwrap()).collect::<Vec<_>>(),
            "title": info.title, "creator": info.creator,
            "outline": doc.outline(),
            "annotations": annotations,
        });
        std::fs::write(out, serde_json::to_string_pretty(&dump).unwrap()).unwrap();
    }
}
