//! The site's file library (资源库): every image and document its content uses lives in
//! `public/assets/images/` or `public/assets/documents/` and is registered (registry.rs); content only
//! refers to them. Adding a file anywhere (作品、Now、关于我、职业页、资源库) is `add`: an image is shrunk
//! with macOS `sips` when over the site's 1.5 MB (JPEG, smaller sides step by step; HEIC / TIFF / BMP
//! always become JPEG; a big PNG / WebP is tried as itself, then as JPEG; GIF / SVG / AVIF can't be
//! shrunk and are refused), a document must be ≤ 5 MB. The same bytes already in the library are
//! reused (registered if they weren't); the same bytes elsewhere under public/ (love-shuyu, the site's
//! own icons) are refused, as the site's audit refuses duplicate files. Else stored as `<name>.<ext>`
//! (ASCII lowercase with dashes, else "image" / "file"; `-2`, `-3`… when taken by other bytes) and
//! registered: slug = its name, 用途 = where it was added.
//! The page shows site images through `pomelo-site://localhost/<url>` (lib.rs → `read_public`).

use super::changes::MAX_IMAGE;
use super::registry;
use super::wander::{public_file, url_of};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

const KEEP: [&str; 7] = ["jpg", "jpeg", "png", "webp", "gif", "svg", "avif"];
const TO_JPEG: [&str; 5] = ["heic", "heif", "tif", "tiff", "bmp"];
const SIDES: [u32; 6] = [3200, 2600, 2200, 1800, 1500, 1200];

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// ASCII lowercase, dashes between words; nothing usable → "image".
pub fn clean_stem(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let mut out = String::new();
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c.to_ascii_lowercase());
        } else if (c == '-' || c == ' ' || c == '.') && !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "image".into()
    } else {
        out.chars().take(60).collect()
    }
}

fn sips(args: &[&str], src: &Path, out: &Path) -> bool {
    Command::new("/usr/bin/sips").args(args).arg(src).arg("--out").arg(out).output().map(|o| o.status.success()).unwrap_or(false) && out.is_file()
}

fn size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX)
}

/// The bytes to store and their extension: as they are when small enough, else shrunk (in `tmp`).
fn fitted(src: &Path, ext: &str, tmp: &Path) -> Result<(PathBuf, String), String> {
    let small = size(src) <= MAX_IMAGE;
    if KEEP.contains(&ext) && small {
        return Ok((src.into(), if ext == "jpeg" { "jpg".into() } else { ext.into() }));
    }
    if !KEEP.contains(&ext) && !TO_JPEG.contains(&ext) {
        return Err("只能放图片（JPG、PNG、WebP、GIF、SVG、AVIF、HEIC）".into());
    }
    if matches!(ext, "gif" | "svg" | "avif") {
        return Err(format!("这张 {} 超过了 1.5 MB，Pomelo 压缩不了这种格式，先自己缩小一下", ext.to_uppercase()));
    }
    let long = crate::picky::office::size_of(src).map(|(w, h)| w.max(h)).unwrap_or(0);
    let sides = || std::iter::once(None).chain(SIDES.iter().filter(move |&&s| s < long).map(|&s| Some(s)));
    // PNG / WebP stay themselves if a smaller size is enough
    if matches!(ext, "png" | "webp") {
        let out = tmp.join(format!("fit.{ext}"));
        for side in sides().skip(1) {
            let s = side.unwrap().to_string();
            if sips(&["-Z", &s], src, &out) && size(&out) <= MAX_IMAGE {
                return Ok((out, ext.into()));
            }
        }
    }
    let out = tmp.join("fit.jpg");
    for (i, side) in sides().enumerate() {
        let quality = if i == 0 { "85" } else { "80" };
        let mut args = vec!["-s", "format", "jpeg", "-s", "formatOptions", quality];
        let s = side.map(|s| s.to_string());
        if let Some(s) = &s {
            args.extend(["-Z", s.as_str()]);
        }
        if sips(&args, src, &out) && size(&out) <= MAX_IMAGE {
            return Ok((out, "jpg".into()));
        }
    }
    Err("这张图片缩小不到 1.5 MB 以内".into())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(|e| e.ok()) {
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&e.path(), out),
            Ok(t) if t.is_file() => out.push(e.path()),
            _ => {}
        }
    }
}

/// A file under public/ with exactly these bytes, as its stored path ("public/…").
fn same_file(folder: &Path, bytes: &[u8]) -> Option<String> {
    let mut files = vec![];
    walk(&folder.join("public"), &mut files);
    // one in the library first
    let lib = folder.join("public/assets");
    files.sort_by_key(|p| (!p.starts_with(&lib), p.clone()));
    let hit = files.into_iter().find(|p| std::fs::metadata(p).is_ok_and(|m| m.len() == bytes.len() as u64) && std::fs::read(p).is_ok_and(|b| b == bytes))?;
    Some(format!("public/{}", hit.strip_prefix(folder.join("public")).ok()?.to_string_lossy().replace('\\', "/")))
}

/// Writes bytes as `<stem>.<ext>` in `dir` (or `-2`, `-3`… when the name is taken by other bytes);
/// returns the stored path.
fn store(folder: &Path, dir: &str, stem: &str, ext: &str, bytes: &[u8]) -> Result<String, String> {
    let at = folder.join(dir);
    std::fs::create_dir_all(&at).map_err(|e| format!("建不了文件夹：{e}"))?;
    for n in 1.. {
        let file = if n == 1 { format!("{stem}.{ext}") } else { format!("{stem}-{n}.{ext}") };
        let dest = at.join(&file);
        if dest.exists() {
            continue;
        }
        std::fs::write(&dest, bytes).map_err(|e| format!("存不了文件：{e}"))?;
        return Ok(format!("{dir}/{file}"));
    }
    unreachable!()
}

/// Adds a file to the library (`kind`: image | document) and registers it with `usage` (unless the
/// same bytes were there already); returns its URL on the site ("/assets/images/x.jpg").
pub fn add(folder: &Path, kind: &str, src: &Path, name: &str, usage: &str, tmp: &Path) -> Result<String, String> {
    registry::valid_kind(kind)?;
    let ext = ext_of(name);
    let (bytes, ext) = if kind == "image" {
        std::fs::create_dir_all(tmp).map_err(|e| e.to_string())?;
        let (at, ext) = fitted(src, &ext, tmp)?;
        let bytes = std::fs::read(&at).map_err(|e| format!("读不了图片：{e}"))?;
        if at != src {
            let _ = std::fs::remove_file(&at);
        }
        (bytes, ext)
    } else {
        let bytes = std::fs::read(src).map_err(|e| format!("读不了文件：{e}"))?;
        if bytes.len() as u64 > super::changes::MAX_FILE {
            return Err("文件超过 5 MB 了（网站的规定），先缩小一下".into());
        }
        if ext.is_empty() {
            return Err("文件要有扩展名（比如 .pdf）".into());
        }
        (bytes, ext)
    };
    let dir = registry::upload_dir(kind);
    let stored = match same_file(folder, &bytes) {
        Some(p) if p.starts_with(&format!("{dir}/")) => p,
        Some(p) => return Err(format!("网站里已经有一模一样的文件，但不在资源库里：{p}")),
        None => {
            let stem = clean_stem(name);
            store(folder, dir, if stem == "image" && kind != "image" { "file" } else { &stem }, &ext, &bytes)?
        }
    };
    registry::ensure(folder, kind, &stored, usage)?;
    Ok(url_of(&stored))
}

/// Pasted bytes (a screenshot): written to `tmp` first, then added like a file.
#[cfg(test)]
pub fn import_bytes(folder: &Path, name: &str, bytes: &[u8], tmp: &Path) -> Result<String, String> {
    std::fs::create_dir_all(tmp).map_err(|e| e.to_string())?;
    let ext = ext_of(name);
    let at = tmp.join(format!("pasted.{}", if ext.is_empty() { "png" } else { &ext }));
    std::fs::write(&at, bytes).map_err(|e| e.to_string())?;
    let name = if ext.is_empty() { format!("{name}.png") } else { name.to_string() };
    let r = add(folder, "image", &at, &name, "", tmp);
    let _ = std::fs::remove_file(&at);
    r
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    /// The registration's file name (its slug).
    pub slug: String,
    pub title: String,
    /// As stored ("public/…").
    pub path: String,
    pub url: String,
    pub alt: String,
    pub usage: String,
}

/// 资源库 entries of one kind (image | document), by title.
pub fn assets(folder: &Path, kind: &str) -> Vec<Asset> {
    registry::list(folder).into_iter().filter(|e| e.kind == kind).map(|e| Asset { url: url_of(&e.file), title: e.title, alt: e.note, usage: e.usage, path: e.file, slug: e.slug }).collect()
}

/// A file under public/ for the page (images only).
pub fn read_public(folder: &Path, url: &str) -> Option<(Vec<u8>, &'static str)> {
    let path = public_file(folder, url)?;
    let mime = match ext_of(&path.to_string_lossy()).as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        _ => return None,
    };
    Some((std::fs::read(path).ok()?, mime))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use crate::site::changes::tests::write;

    fn png(dir: &Path, w: u32, h: u32, noisy: bool) -> PathBuf {
        // a BMP drawn by hand, made PNG by sips (noisy = hard to compress)
        let row = (w * 3).div_ceil(4) * 4;
        let mut b = vec![];
        let size = 54 + row * h;
        b.extend(b"BM");
        for v in [size, 0, 54, 40] {
            b.extend(v.to_le_bytes());
        }
        b.extend((w as i32).to_le_bytes());
        b.extend((h as i32).to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(24u16.to_le_bytes());
        for v in [0u32, row * h, 2835, 2835, 0, 0] {
            b.extend(v.to_le_bytes());
        }
        let mut seed = 7u32;
        for _ in 0..h {
            for x in 0..row {
                seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                b.push(if noisy { (seed >> 16) as u8 } else { (x % 251) as u8 });
            }
        }
        let bmp = dir.join(format!("src-{w}-{noisy}.bmp"));
        std::fs::write(&bmp, b).unwrap();
        let out = dir.join(format!("src-{w}-{noisy}.png"));
        assert!(sips(&["-s", "format", "png"], &bmp, &out));
        out
    }

    #[test]
    fn names() {
        assert_eq!(clean_stem("Big Fish.JPG"), "big-fish");
        assert_eq!(clean_stem("截屏2026-10-20 10.11.12.png"), "2026-10-20-10-11-12");
        assert_eq!(clean_stem("粘贴的图片.png"), "image");
        assert_eq!(clean_stem("photo_1--final.webp"), "photo_1-final");
    }

    fn entry(s: &Path, kind: &str, slug: &str) -> serde_json::Value {
        let dir = if kind == "image" { "images" } else { "documents" };
        serde_json::from_str(&std::fs::read_to_string(s.join(format!("src/data/assets/{dir}/{slug}.json"))).unwrap()).unwrap()
    }

    #[test]
    fn adds_reuses_numbers_and_registers() {
        let dir = temp_dir("site-images");
        let site = dir.join("site");
        let tmp = dir.join("tmp");
        let a = dir.join("a.png");
        std::fs::write(&a, [1u8, 2, 3]).unwrap();
        assert_eq!(add(&site, "image", &a, "Cover.PNG", "作品《春》", &tmp).unwrap(), "/assets/images/cover.png");
        let e = entry(&site, "image", "cover");
        assert_eq!((e["asset"]["value"]["file"].as_str(), e["usage"].as_str()), (Some("public/assets/images/cover.png"), Some("作品《春》")));
        // the same bytes again: reused, registered once
        assert_eq!(add(&site, "image", &a, "other.png", "Now", &tmp).unwrap(), "/assets/images/cover.png");
        assert_eq!(registry::list(&site).len(), 1);
        // the name taken by other bytes: numbered, its own slug
        std::fs::write(&a, [9u8]).unwrap();
        assert_eq!(add(&site, "image", &a, "cover.png", "", &tmp).unwrap(), "/assets/images/cover-2.png");
        assert_eq!(entry(&site, "image", "cover-2")["asset"]["value"]["file"], "public/assets/images/cover-2.png");
        assert_eq!(import_bytes(&site, "粘贴的图片", &[5u8, 6], &tmp).unwrap(), "/assets/images/image.png");
        // an unregistered file in the library: reused and registered now
        write(&site, "public/assets/images/loose.png", [4u8, 4]);
        std::fs::write(&a, [4u8, 4]).unwrap();
        assert_eq!(add(&site, "image", &a, "x.png", "关于我 · 肖像", &tmp).unwrap(), "/assets/images/loose.png");
        assert_eq!(entry(&site, "image", "loose")["usage"], "关于我 · 肖像");
        // the same bytes outside the library (love-shuyu, the site's icons): refused
        write(&site, "public/love-shuyu/photos/a.png", [7u8, 7, 7]);
        std::fs::write(&a, [7u8, 7, 7]).unwrap();
        assert!(add(&site, "image", &a, "mine.png", "", &tmp).unwrap_err().contains("public/love-shuyu/photos/a.png"));
        assert!(add(&site, "image", &a, "notes.pdf", "", &tmp).unwrap_err().contains("只能放图片"));
        assert_eq!(read_public(&site, "/assets/images/cover-2.png"), Some((vec![9u8], "image/png")));
        assert_eq!(read_public(&site, "/images/../../a.png"), None);
    }

    #[test]
    fn documents() {
        let dir = temp_dir("site-files");
        let site = dir.join("site");
        let pdf = dir.join("CV 2026.PDF");
        std::fs::write(&pdf, b"%PDF-1.4 x").unwrap();
        assert_eq!(add(&site, "document", &pdf, "CV 2026.PDF", "职业页 · 中文简历", &dir.join("tmp")).unwrap(), "/assets/documents/cv-2026.pdf");
        assert_eq!(entry(&site, "document", "cv-2026")["asset"]["discriminant"], "existingDocument");
        assert_eq!(add(&site, "document", &pdf, "other.pdf", "", &dir.join("tmp")).unwrap(), "/assets/documents/cv-2026.pdf");
        std::fs::write(dir.join("big.pdf"), vec![0u8; MAX_IMAGE as usize * 4]).unwrap();
        assert!(add(&site, "document", &dir.join("big.pdf"), "big.pdf", "", &dir.join("tmp")).unwrap_err().contains("5 MB"));
        std::fs::write(dir.join("noext"), b"x").unwrap();
        assert!(add(&site, "document", &dir.join("noext"), "noext", "", &dir.join("tmp")).unwrap_err().contains("扩展名"));
        assert!(add(&site, "video", &pdf, "a.pdf", "", &dir.join("tmp")).is_err());
    }

    #[test]
    fn shrinks_big_images_under_the_limit() {
        let dir = temp_dir("site-images-big");
        let site = dir.join("site");
        let tmp = dir.join("tmp");
        // noisy: PNG can't get small enough → JPEG
        let big = png(&dir, 1800, 1400, true);
        assert!(size(&big) > MAX_IMAGE);
        let url = add(&site, "image", &big, "photo.png", "", &tmp).unwrap();
        assert_eq!(url, "/assets/images/photo.jpg");
        let stored = site.join("public").join(url.trim_start_matches('/'));
        assert!(size(&stored) <= MAX_IMAGE);
        // a GIF can't be shrunk
        std::fs::write(dir.join("big.gif"), vec![0u8; MAX_IMAGE as usize + 1]).unwrap();
        assert!(add(&site, "image", &dir.join("big.gif"), "big.gif", "", &tmp).unwrap_err().contains("GIF"));
    }

    #[test]
    fn library_by_kind() {
        let dir = temp_dir("site-assets");
        let s = dir.join("site");
        write(&s, "src/data/assets/images/big-fish.json", r#"{"title":"big-fish","asset":{"discriminant":"existingImage","value":{"file":"public/assets/images/big-fish.jpg","alt":"大鱼"}},"usage":"作品《大鱼》"}"#);
        write(&s, "src/data/assets/documents/cv.json", r#"{"title":"cv","asset":{"discriminant":"existingDocument","value":{"file":"public/assets/documents/cv.pdf","description":"简历"}},"usage":""}"#);
        write(&s, "src/data/assets/images/broken.json", "{");
        let a = assets(&s, "image");
        assert_eq!(a.iter().map(|x| (x.slug.as_str(), x.url.as_str(), x.alt.as_str(), x.path.as_str())).collect::<Vec<_>>(), [("big-fish", "/assets/images/big-fish.jpg", "大鱼", "public/assets/images/big-fish.jpg")]);
        assert_eq!(assets(&s, "document")[0].alt, "简历");
    }
}
