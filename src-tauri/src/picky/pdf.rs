//! Text of a PDF, page by page, with macOS's PDFKit (no extra crate: the framework is linked and
//! called through objc2). Works on the bytes given; never follows links or companion files.
//! Also SHA-256 through CommonCrypto (libSystem), for storing each file's bytes once.

use objc2::rc::{autoreleasepool, Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::msg_send;
use objc2_foundation::NSString;
use std::ffi::c_void;

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

/// Text of every page (empty for pages without text).
pub fn pages(data: &[u8]) -> Result<Vec<String>, String> {
    autoreleasepool(|_| unsafe {
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
    fn sha256() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
