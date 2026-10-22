//! What goes into Pub2MD: a PDF, a Word file, or images (one or several, packed into one PDF in name
//! order: one page each); MD / MMD / TXT files (each its own document) are only translated.

use crate::picky::pdf;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Pdf,
    Word,
    Images,
    /// MD / MMD / TXT: 翻译 only.
    Text,
}

const IMAGES: &[&str] = &["png", "jpg", "jpeg", "heic", "heif", "webp", "tif", "tiff", "bmp", "gif"];
const WORD: &[&str] = &["docx", "doc"];
const TEXT: &[&str] = &["md", "mmd", "txt", "markdown"];

fn ext(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

fn file_name(p: &Path) -> String {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()
}

/// Dropped or picked files as documents: the images together, every other file on its own.
pub fn group(paths: &[PathBuf]) -> Result<Vec<Vec<PathBuf>>, String> {
    let (mut docs, mut images, mut other) = (vec![], vec![], vec![]);
    for p in paths {
        if !p.is_file() {
            return Err(format!("找不到文件「{}」", file_name(p)));
        }
        let e = ext(p);
        if e == "pdf" || WORD.contains(&e.as_str()) || TEXT.contains(&e.as_str()) {
            docs.push(vec![p.clone()]);
        } else if IMAGES.contains(&e.as_str()) {
            images.push(p.clone());
        } else {
            other.push(file_name(p));
        }
    }
    if !other.is_empty() {
        return Err(format!("放不进来：{}（只收 PDF、Word、图片和 MD、MMD、TXT）", other.join("、")));
    }
    if !images.is_empty() {
        images.sort_by_key(|p| file_name(p).to_lowercase());
        docs.push(images);
    }
    Ok(docs)
}

pub fn kind_of(paths: &[PathBuf]) -> Kind {
    match paths.first().map(|p| ext(p)).as_deref() {
        Some("pdf") => Kind::Pdf,
        Some(e) if WORD.contains(&e) => Kind::Word,
        Some(e) if TEXT.contains(&e) => Kind::Text,
        _ => Kind::Images,
    }
}

/// The document's name: the file's, or 「第一张 等 3 张」.
pub fn name_of(paths: &[PathBuf]) -> String {
    let stem = |p: &PathBuf| p.file_stem().and_then(|s| s.to_str()).unwrap_or("文档").to_string();
    match paths {
        [] => "文档".into(),
        [one] => stem(one),
        [first, ..] => format!("{} 等 {} 张", stem(first), paths.len()),
    }
}

/// What Mathpix gets: (a fixed file name, its type, the bytes). Images go as one PDF.
pub fn upload(kind: Kind, paths: &[PathBuf]) -> Result<(String, &'static str, Vec<u8>), String> {
    let read = |p: &PathBuf| std::fs::read(p).map_err(|e| format!("读不了「{}」：{e}", file_name(p)));
    Ok(match kind {
        Kind::Pdf => {
            let data = read(&paths[0])?;
            if !data.starts_with(b"%PDF-") {
                return Err(format!("「{}」不是 PDF", file_name(&paths[0])));
            }
            ("input.pdf".into(), "application/pdf", data)
        }
        Kind::Word => {
            let e = ext(&paths[0]);
            let mime = if e == "doc" { "application/msword" } else { "application/vnd.openxmlformats-officedocument.wordprocessingml.document" };
            (format!("input.{e}"), mime, read(&paths[0])?)
        }
        Kind::Images => ("input.pdf".into(), "application/pdf", pdf::images_to_pdf(paths)?),
        Kind::Text => return Err("文本文件不用上传".into()),
    })
}

/// Same files, same fingerprint (for reusing an earlier Mathpix result).
pub fn fingerprint(paths: &[PathBuf]) -> Result<String, String> {
    let mut all = String::new();
    for p in paths {
        let data = std::fs::read(p).map_err(|e| format!("读不了「{}」：{e}", file_name(p)))?;
        all.push_str(&pdf::sha256_hex(&data));
        all.push('\n');
    }
    Ok(pdf::sha256_hex(all.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_become_documents() {
        let dir = crate::db::tests::temp_dir("pub2md-source");
        let f = |n: &str| {
            let p = dir.join(n);
            std::fs::write(&p, b"x").unwrap();
            p
        };
        let (a, b, img2, img1) = (f("a.pdf"), f("b.docx"), f("IMG_2.jpg"), f("img_1.HEIC"));
        let docs = group(&[img2.clone(), a.clone(), img1.clone(), b.clone()]).unwrap();
        assert_eq!(docs, vec![vec![a.clone()], vec![b.clone()], vec![img1.clone(), img2.clone()]]);
        assert_eq!((kind_of(&docs[0]), kind_of(&docs[1]), kind_of(&docs[2])), (Kind::Pdf, Kind::Word, Kind::Images));
        assert_eq!((name_of(&docs[0]), name_of(&docs[2])), ("a".to_string(), "img_1 等 2 张".to_string()));
        assert!(group(&[f("x.zip")]).unwrap_err().contains("x.zip"));
        let md = f("y.md");
        assert_eq!((group(&[md.clone()]).unwrap(), kind_of(&[md])), (vec![vec![dir.join("y.md")]], Kind::Text));
        assert!(group(&[dir.join("gone.pdf")]).unwrap_err().contains("找不到"));
        assert!(upload(Kind::Pdf, &[a.clone()]).unwrap_err().contains("不是 PDF"));
        assert_eq!(fingerprint(&[a.clone()]).unwrap(), fingerprint(&[a]).unwrap());
    }
}
