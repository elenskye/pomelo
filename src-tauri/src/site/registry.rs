//! 资源库 (`src/data/assets/<images|documents>/<slug>.json`): {title, asset: {discriminant, value: {file,
//! alt | description}}, usage}. Every file in the library (`public/assets/<images|documents>/`) is
//! registered once, as `existingImage` / `existingDocument` with its "public/assets/…" path; content
//! only refers to library files (the site's audit checks both). A file added through Pomelo is
//! registered by images.rs (`ensure`); its slug is its file name (`-2`… when taken), its title the slug
//! (the site's scripts take one for the other). Keys Pomelo doesn't know stay.
//! 资源库 page: each entry with its size and whether anything uses it (`library`).
//! 删除: first the places using it — the site's own scan (scripts/lib/asset-reference-report.mjs): lines
//! in src/, public/ and a few root files naming its path ("public/…" or "/…"), or a `{% sharedImage
//! image="<slug>" %}`; then only the registration goes, unless asked to delete an unused file too.

use super::doc::{self, read_json};
use super::wander::{public_file, url_of};
use super::yaml::Y;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const TEXT_EXT: [&str; 13] = ["astro", "css", "html", "js", "json", "md", "mdx", "mjs", "svg", "ts", "tsx", "txt", "xml"];
const ROOT_FILES: [&str; 2] = ["astro.config.mjs", "vercel.json"];

fn dir(kind: &str) -> &'static str {
    if kind == "image" {
        "src/data/assets/images"
    } else {
        "src/data/assets/documents"
    }
}

pub fn upload_dir(kind: &str) -> &'static str {
    if kind == "image" {
        "public/assets/images"
    } else {
        "public/assets/documents"
    }
}

fn file(folder: &Path, kind: &str, slug: &str) -> PathBuf {
    folder.join(dir(kind)).join(format!("{slug}.json"))
}

pub fn valid_kind(kind: &str) -> Result<(), String> {
    if kind == "image" || kind == "document" {
        Ok(())
    } else {
        Err("资源的类型不对".into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Entry {
    /// image | document
    pub kind: String,
    /// The file name (stable).
    pub slug: String,
    pub title: String,
    /// The stored path.
    pub file: String,
    /// alt (images) or description (documents).
    pub note: String,
    pub usage: String,
    pub print: Option<String>,
    /// `library` only: the file's size in bytes and whether anything uses it.
    pub size: u64,
    pub used: bool,
}

fn note_key(kind: &str) -> &'static str {
    if kind == "image" {
        "alt"
    } else {
        "description"
    }
}

fn entry(kind: &str, slug: &str, v: &Y, print: String) -> Entry {
    let asset = v.get("asset");
    let value = asset.and_then(|a| a.get("value"));
    Entry {
        kind: kind.into(),
        slug: slug.into(),
        title: doc::text(v, "title"),
        file: value.map(|x| doc::text(x, "file")).unwrap_or_default(),
        note: value.map(|x| doc::text(x, note_key(kind))).unwrap_or_default(),
        usage: doc::text(v, "usage"),
        print: Some(print),
        ..Default::default()
    }
}

/// Both kinds, by title.
pub fn list(folder: &Path) -> Vec<Entry> {
    let mut out = vec![];
    for kind in ["image", "document"] {
        let Ok(rd) = std::fs::read_dir(folder.join(dir(kind))) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(slug) = name.strip_suffix(".json") else { continue };
            if let Ok(Some((v, print))) = read_json(&e.path()) {
                out.push(entry(kind, slug, &v, print));
            }
        }
    }
    out.sort_by(|a, b| (&a.kind, &a.title).cmp(&(&b.kind, &b.title)));
    out
}

fn same_file(folder: &Path, a: &str, b: &str) -> bool {
    match (public_file(folder, a), public_file(folder, b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// The registration of a library file, made now (with `usage`) if it has none.
pub fn ensure(folder: &Path, kind: &str, stored: &str, usage: &str) -> Result<Entry, String> {
    match list(folder).into_iter().find(|e| e.kind == kind && same_file(folder, &e.file, stored)) {
        Some(e) => Ok(e),
        None => register(folder, kind, stored, usage),
    }
}

/// Registers a library file ("public/assets/<images|documents>/…"); its slug comes from its name.
fn register(folder: &Path, kind: &str, stored: &str, usage: &str) -> Result<Entry, String> {
    valid_kind(kind)?;
    let stored = format!("public{}", url_of(stored));
    if !stored.starts_with(&format!("{}/", upload_dir(kind))) || !public_file(folder, &stored).is_some_and(|f| f.is_file()) {
        return Err(format!("文件不在资源库的文件夹里：{stored}"));
    }
    let name = stored.rsplit('/').next().unwrap_or_default();
    let base = super::images::clean_stem(name).replace('_', "-");
    let slug = (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") }).find(|s| !file(folder, kind, s).exists()).unwrap_or(base);
    let disc = if kind == "image" { "existingImage" } else { "existingDocument" };
    let v = Y::Map(vec![
        ("title".into(), Y::Str(slug.clone())),
        ("asset".into(), Y::Map(vec![("discriminant".into(), Y::Str(disc.into())), ("value".into(), Y::Map(vec![("file".into(), Y::Str(stored.clone())), (note_key(kind).into(), Y::Str(String::new()))]))])),
        ("usage".into(), Y::Str(usage.trim().into())),
    ]);
    let p = file(folder, kind, &slug);
    doc::write_json(&p, None, &v)?;
    let (v, print) = read_json(&p)?.ok_or("登记没写成")?;
    Ok(entry(kind, &slug, &v, print))
}

/// Registers every library file of `kind` that isn't yet; returns how many.
pub fn adopt(folder: &Path, kind: &str) -> Result<usize, String> {
    let files = unregistered(folder, kind)?;
    for f in &files {
        register(folder, kind, f, "")?;
    }
    Ok(files.len())
}

/// Changes the alt / description and 用途 (the file and the name stay).
pub fn save(folder: &Path, e: &Entry) -> Result<Entry, String> {
    valid_kind(&e.kind)?;
    let p = file(folder, &e.kind, &e.slug);
    doc::check_print(&p, e.print.as_ref(), "这条登记")?;
    let (old, _) = read_json(&p)?.ok_or("找不到这条登记，可能已经删掉了")?;
    let mut v = old.clone();
    if let Some(Y::Map(asset)) = v.get("asset").cloned() {
        let mut asset = Y::Map(asset);
        if let Some(Y::Map(value)) = asset.get("value").cloned() {
            let mut value = Y::Map(value);
            value.set(note_key(&e.kind), Y::Str(e.note.trim().into()), &[]);
            asset.set("value", value, &[]);
        }
        v.set("asset", asset, &[]);
    }
    v.set("usage", Y::Str(e.usage.trim().into()), &[]);
    doc::write_json(&p, Some(&old), &v)?;
    let (v, print) = read_json(&p)?.ok_or("登记没写成")?;
    Ok(entry(&e.kind, &e.slug, &v, print))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    pub path: String,
    /// Cover | Body (images), as the site's report says.
    pub role: Option<String>,
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out),
            Ok(t) if t.is_file() => out.push(p),
            _ => {}
        }
    }
}

fn text_files(folder: &Path) -> Vec<(PathBuf, String)> {
    let mut files = vec![];
    walk(&folder.join("src"), &mut files);
    walk(&folder.join("public"), &mut files);
    files.extend(ROOT_FILES.iter().map(|f| folder.join(f)).filter(|p| p.is_file()));
    let registry = folder.join("src/data/assets");
    files
        .into_iter()
        .filter(|f| !f.starts_with(&registry) && f.extension().is_some_and(|x| TEXT_EXT.contains(&x.to_string_lossy().to_ascii_lowercase().as_str())))
        .filter_map(|f| std::fs::read_to_string(&f).ok().map(|t| (f, t)))
        .collect()
}

/// What a line naming an entry contains: its path ("public/…" and "/…").
fn direct(folder: &Path, e: &Entry) -> Vec<String> {
    match public_file(folder, &e.file).and_then(|p| p.strip_prefix(folder.join("public")).ok().map(|r| r.to_string_lossy().replace('\\', "/"))) {
        Some(rel) => vec![format!("public/{rel}"), format!("/{rel}")],
        None => vec![],
    }
}

fn shared(e: &Entry, line: &str) -> bool {
    e.kind == "image" && line.contains("sharedImage") && [&e.slug, &e.title].iter().filter(|n| !n.is_empty()).any(|n| line.contains(&format!("image=\"{n}\"")) || line.contains(&format!("image='{n}'")))
}

/// Both kinds, by title, each with its file's size and whether anything uses it.
pub fn library(folder: &Path) -> Vec<Entry> {
    let texts = text_files(folder);
    list(folder)
        .into_iter()
        .map(|mut e| {
            e.size = public_file(folder, &e.file).and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len());
            let d = direct(folder, &e);
            e.used = texts.iter().any(|(_, t)| d.iter().any(|x| t.contains(x.as_str())) || t.lines().any(|l| shared(&e, l)));
            e
        })
        .collect()
}

/// Where a registration is used.
pub fn citations(folder: &Path, kind: &str, slug: &str) -> Result<Vec<Citation>, String> {
    valid_kind(kind)?;
    let (v, _) = read_json(&file(folder, kind, slug))?.ok_or("找不到这条登记")?;
    let e = entry(kind, slug, &v, String::new());
    let d = direct(folder, &e);
    let mut out: Vec<Citation> = vec![];
    for (f, text) in text_files(folder) {
        let rel = f.strip_prefix(folder).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        for line in text.lines() {
            if !(shared(&e, line) || d.iter().any(|x| line.contains(x.as_str()))) {
                continue;
            }
            let role = (kind == "image").then(|| if line.trim_start().starts_with("thumbnail") { "Cover" } else { "Body" }.to_string());
            let c = Citation { path: rel.clone(), role };
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out.sort_by(|a, b| (&a.path, &a.role).cmp(&(&b.path, &b.role)));
    Ok(out)
}

/// Deletes the registration (and its file when asked and nothing uses it).
pub fn delete(folder: &Path, kind: &str, slug: &str, print: Option<&String>, with_file: bool) -> Result<(), String> {
    valid_kind(kind)?;
    let p = file(folder, kind, slug);
    doc::check_print(&p, print, "这条登记")?;
    let (v, _) = read_json(&p)?.ok_or("找不到这条登记")?;
    let e = entry(kind, slug, &v, String::new());
    let used = !citations(folder, kind, slug)?.is_empty();
    std::fs::remove_file(&p).map_err(|e| format!("删不了：{e}"))?;
    if with_file && !used {
        if let Some(f) = public_file(folder, &e.file).filter(|f| f.is_file()) {
            std::fs::remove_file(f).map_err(|e| format!("登记删了，文件没删掉：{e}"))?;
        }
    }
    Ok(())
}

/// Library files of `kind` not registered yet.
pub fn unregistered(folder: &Path, kind: &str) -> Result<Vec<String>, String> {
    valid_kind(kind)?;
    let roots = [upload_dir(kind)];
    let taken: Vec<PathBuf> = list(folder).iter().filter_map(|e| public_file(folder, &e.file)).collect();
    let mut files = vec![];
    for r in roots {
        walk(&folder.join(r), &mut files);
    }
    let mut out: Vec<String> = files
        .into_iter()
        .filter(|p| !taken.contains(p) && p.file_name().is_some_and(|n| !n.to_string_lossy().starts_with('.')))
        .filter_map(|p| p.strip_prefix(folder).ok().map(|r| r.to_string_lossy().replace('\\', "/")))
        .collect();
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use crate::site::changes::tests::write;

    #[test]
    fn register_edit_cite_delete() {
        let dir = temp_dir("site-registry");
        let s = dir.join("site");
        write(&s, "src/data/assets/images/big-fish.json", "{\n  \"title\": \"big-fish\",\n  \"asset\": {\n    \"discriminant\": \"existingImage\",\n    \"value\": {\n      \"file\": \"public/assets/images/big-fish.jpg\",\n      \"alt\": \"\"\n    }\n  },\n  \"usage\": \"作品《大鱼》\"\n}\n");
        write(&s, "public/assets/images/big-fish.jpg", [1u8]);
        write(&s, "public/assets/images/spring_cover.jpg", [2u8, 2]);
        write(&s, "public/assets/documents/cv.pdf", b"%PDF");
        write(&s, "public/love-shuyu/photos/a.jpg", [3u8]);
        write(&s, "src/content/wander/big-fish.zh.md", "---\nthumbnailRef: public/assets/images/big-fish.jpg\n---\n![](/assets/images/big-fish.jpg)\n");
        write(&s, "src/content/wander/x.zh.md", "---\ntitle: x\n---\n{% sharedImage image=\"spring-cover\" /%}\n");
        let l = list(&s);
        assert_eq!((l[0].slug.as_str(), l[0].file.as_str(), l[0].usage.as_str()), ("big-fish", "public/assets/images/big-fish.jpg", "作品《大鱼》"));
        assert_eq!(unregistered(&s, "image").unwrap(), ["public/assets/images/spring_cover.jpg"]);
        assert_eq!(unregistered(&s, "document").unwrap(), ["public/assets/documents/cv.pdf"]);
        // registered from its name; a file outside the library refused; one already registered kept
        let e = ensure(&s, "image", "/assets/images/spring_cover.jpg", "作品《春》").unwrap();
        assert_eq!((e.slug.as_str(), e.usage.as_str()), ("spring-cover", "作品《春》"));
        assert_eq!(std::fs::read_to_string(s.join("src/data/assets/images/spring-cover.json")).unwrap(), "{\n  \"title\": \"spring-cover\",\n  \"asset\": {\n    \"discriminant\": \"existingImage\",\n    \"value\": {\n      \"file\": \"public/assets/images/spring_cover.jpg\",\n      \"alt\": \"\"\n    }\n  },\n  \"usage\": \"作品《春》\"\n}\n");
        assert_eq!(ensure(&s, "image", "public/assets/images/big-fish.jpg", "别的").unwrap().slug, "big-fish");
        assert!(ensure(&s, "image", "public/love-shuyu/photos/a.jpg", "").unwrap_err().contains("不在资源库"));
        let e = save(&s, &Entry { note: "河边的樱花".into(), usage: "".into(), ..e }).unwrap();
        assert!(std::fs::read_to_string(s.join("src/data/assets/images/spring-cover.json")).unwrap().contains("\"alt\": \"河边的樱花\"") && e.usage.is_empty());
        // where they are used; the library says so
        assert_eq!(citations(&s, "image", "big-fish").unwrap(), vec![Citation { path: "src/content/wander/big-fish.zh.md".into(), role: Some("Body".into()) }, Citation { path: "src/content/wander/big-fish.zh.md".into(), role: Some("Cover".into()) }]);
        assert_eq!(citations(&s, "image", "spring-cover").unwrap(), vec![Citation { path: "src/content/wander/x.zh.md".into(), role: Some("Body".into()) }]);
        assert_eq!(adopt(&s, "document").unwrap(), 1);
        let lib = library(&s);
        assert_eq!(lib.iter().map(|e| (e.slug.as_str(), e.size, e.used)).collect::<Vec<_>>(), [("cv", 4, false), ("big-fish", 1, true), ("spring-cover", 2, true)]);
        // a used file stays even when asked; an unused one goes with its registration
        delete(&s, "image", "big-fish", l[0].print.as_ref(), true).unwrap();
        assert!(s.join("public/assets/images/big-fish.jpg").exists() && !s.join("src/data/assets/images/big-fish.json").exists());
        let d = lib.iter().find(|e| e.slug == "cv").unwrap();
        assert!(delete(&s, "document", "cv", None, true).unwrap_err().contains("别处被改过了"));
        delete(&s, "document", "cv", d.print.as_ref(), true).unwrap();
        assert!(!s.join("public/assets/documents/cv.pdf").exists());
    }
}
