//! 作品 (Wander): `src/content/wander/<slug>.zh.md` / `<slug>.en.md`, the same slug = one work in two
//! languages. Fields as the site's content.config.ts has them:
//! - Shared (both files written alike): category (a taxonomy slug or ''), topics, month (YYYY-MM, may be
//!   empty), cover (`thumbnailRef`), gallery images (`imageRef`). Each language its own: title
//!   (required), summary, body, gallery alt / caption; the zh file also englishTitle / englishSummary
//!   (the English stub while there's no English version).
//! - Written in the site's format (yaml.rs): keys keep their place, a new one goes where the schema has
//!   it, empty text is left out, `topics` / `category` always there, `gallery: []` only if it was.
//!   Keys Pomelo doesn't know stay as they are. A file whose fields and body wouldn't change is not
//!   rewritten (so it keeps an older formatting until it really changes).
//! - Every write checks the files are still what was read (fingerprints) — an editor or git may have
//!   changed them meanwhile.
//! - A gallery has none or at least two images (the site's qa-wander checks every gallery page for its
//!   previous / next previews; one image has neither).
//! - Cover and gallery images are files of the library (`public/assets/images/`, images.rs); renaming or
//!   deleting a work leaves them there.

pub use super::doc::Prints;
use super::doc::{fingerprint, read_md as read_doc, Doc};
use super::yaml::{self, Y};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DIR: &str = "src/content/wander";
const ORDER: [&str; 10] = ["title", "language", "category", "topics", "month", "summary", "englishTitle", "englishSummary", "thumbnailRef", "gallery"];
pub const LANGS: [&str; 2] = ["zh", "en"];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Term {
    pub slug: String,
    pub zh: String,
    pub en: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Taxonomy {
    pub categories: Vec<Term>,
    pub topics: Vec<Term>,
}

/// src/data/wander-taxonomy.json (missing / unreadable = empty).
pub fn taxonomy(folder: &Path) -> Taxonomy {
    let v: serde_json::Value = std::fs::read_to_string(folder.join("src/data/wander-taxonomy.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let terms = |k: &str| {
        v[k].as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| Some(Term { slug: t["slug"].as_str()?.to_string(), zh: t["zh"].as_str().unwrap_or_default().to_string(), en: t["en"].as_str().unwrap_or_default().to_string() }))
            .filter(|t| !t.slug.is_empty())
            .collect()
    };
    Taxonomy { categories: terms("categories"), topics: terms("topics") }
}

pub fn valid_slug(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && s.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

pub fn valid_month(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7 && b[..4].iter().all(u8::is_ascii_digit) && b[4] == b'-' && matches!(&s[5..], "01" | "02" | "03" | "04" | "05" | "06" | "07" | "08" | "09" | "10" | "11" | "12")
}

fn file(folder: &Path, slug: &str, lang: &str) -> PathBuf {
    folder.join(DIR).join(format!("{slug}.{lang}.md"))
}

/// A stored path ("public/…") → the site's URL for it ("/…").
pub fn url_of(stored: &str) -> String {
    match stored.strip_prefix("public/") {
        Some(rest) => format!("/{rest}"),
        None => stored.to_string(),
    }
}

/// The file on disk behind a stored path or URL (None outside public/).
pub fn public_file(folder: &Path, stored: &str) -> Option<PathBuf> {
    let rel = stored.strip_prefix("public/").or_else(|| stored.strip_prefix('/'))?;
    let rel = rel.split(['?', '#']).next().unwrap_or(rel);
    if rel.is_empty() || rel.split('/').any(|p| p.is_empty() || p == "." || p == "..") {
        return None;
    }
    Some(folder.join("public").join(rel))
}

fn text(front: &Y, key: &str) -> String {
    front.get(key).and_then(Y::text).unwrap_or_default()
}

fn strings(front: &Y, key: &str) -> Vec<String> {
    front.get(key).map(|v| v.seq().iter().filter_map(Y::text).collect()).unwrap_or_default()
}

// ---------- the list ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub slug: String,
    pub title: String,
    pub month: String,
    pub category: String,
    pub topics: Vec<String>,
    /// zh / en files present.
    pub langs: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Works {
    pub works: Vec<Row>,
    pub taxonomy: Taxonomy,
    /// Files that couldn't be read (name: why).
    pub broken: Vec<String>,
}

/// Every work, newest month first (then by slug).
pub fn list(folder: &Path) -> Result<Works, String> {
    let dir = folder.join(DIR);
    let mut slugs: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| format!("读不了作品文件夹：{e}"))?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            LANGS.iter().find_map(|l| n.strip_suffix(&format!(".{l}.md")).map(str::to_string))
        })
        .collect();
    slugs.sort();
    slugs.dedup();
    let mut works = vec![];
    let mut broken = vec![];
    for slug in slugs {
        let mut docs = vec![];
        for l in LANGS {
            match read_doc(&file(folder, &slug, l)) {
                Ok(Some(d)) => docs.push((l, d)),
                Ok(None) => {}
                Err(e) => broken.push(e),
            }
        }
        let Some((_, main)) = docs.first() else { continue };
        works.push(Row {
            title: text(&main.front, "title"),
            month: text(&main.front, "month"),
            category: text(&main.front, "category"),
            topics: strings(&main.front, "topics"),
            langs: docs.iter().map(|(l, _)| *l).collect(),
            slug,
        });
    }
    works.sort_by(|a, b| b.month.cmp(&a.month).then(a.slug.cmp(&b.slug)));
    Ok(Works { works, taxonomy: taxonomy(folder), broken })
}

// ---------- one work ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub title: String,
    pub summary: String,
    pub body: String,
    /// zh only: the English stub's title / summary.
    #[serde(default)]
    pub english_title: String,
    #[serde(default)]
    pub english_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct GalleryItem {
    /// The stored path ("public/assets/images/…").
    pub image: String,
    #[serde(default)]
    pub alt_zh: String,
    #[serde(default)]
    pub caption_zh: String,
    #[serde(default)]
    pub alt_en: String,
    #[serde(default)]
    pub caption_en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    pub slug: String,
    pub category: String,
    pub topics: Vec<String>,
    pub month: String,
    /// The cover's stored path.
    pub cover: Option<String>,
    pub gallery: Vec<GalleryItem>,
    pub zh: Option<Version>,
    pub en: Option<Version>,
    pub prints: Prints,
}

fn gallery_of(front: &Y) -> Vec<(String, String, String)> {
    front
        .get("gallery")
        .map(|g| {
            g.seq()
                .iter()
                .filter_map(|it| {
                    let image = it.get("imageRef").and_then(Y::text).filter(|s| !s.is_empty())?;
                    Some((image, it.get("alt").and_then(Y::text).unwrap_or_default(), it.get("caption").and_then(Y::text).unwrap_or_default()))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn load(folder: &Path, slug: &str) -> Result<Work, String> {
    if !valid_slug(slug) {
        return Err("作品的 slug 不对".into());
    }
    let zh = read_doc(&file(folder, slug, "zh"))?;
    let en = read_doc(&file(folder, slug, "en"))?;
    let main = zh.as_ref().or(en.as_ref()).ok_or("找不到这个作品，可能已经删掉或改名了")?;
    let cover = main.front.get("thumbnailRef").and_then(Y::text).filter(|s| !s.is_empty());
    let g_main = gallery_of(&main.front);
    let g_en = en.as_ref().map(|d| gallery_of(&d.front)).unwrap_or_default();
    let gallery = g_main
        .iter()
        .enumerate()
        .map(|(i, (image, alt, caption))| {
            let mut it = GalleryItem { image: image.clone(), ..Default::default() };
            if zh.is_some() {
                (it.alt_zh, it.caption_zh) = (alt.clone(), caption.clone());
                // the English alt of the same image (same place first)
                if let Some((_, a, c)) = g_en.get(i).filter(|e| e.0 == *image).or_else(|| g_en.iter().find(|e| e.0 == *image)) {
                    (it.alt_en, it.caption_en) = (a.clone(), c.clone());
                }
            } else {
                (it.alt_en, it.caption_en) = (alt.clone(), caption.clone());
            }
            it
        })
        .collect();
    let version = |d: &Doc| Version {
        title: text(&d.front, "title"),
        summary: text(&d.front, "summary"),
        body: d.body.clone(),
        english_title: text(&d.front, "englishTitle"),
        english_summary: text(&d.front, "englishSummary"),
    };
    Ok(Work {
        slug: slug.into(),
        category: text(&main.front, "category"),
        topics: strings(&main.front, "topics"),
        month: text(&main.front, "month"),
        cover,
        gallery,
        zh: zh.as_ref().map(version),
        en: en.as_ref().map(version),
        prints: Prints { zh: zh.map(|d| d.print), en: en.map(|d| d.print) },
    })
}

/// The files are still what was read.
fn check_prints(folder: &Path, slug: &str, prints: &Prints) -> Result<(), String> {
    for l in LANGS {
        let now = std::fs::read(file(folder, slug, l)).ok().map(|b| fingerprint(&b));
        if now.as_ref() != prints.of(l) {
            let which = if l == "zh" { "中文" } else { "英文" };
            return Err(format!("{which}版在别处被改过了（编辑器或 git），先重新读取再改"));
        }
    }
    Ok(())
}

fn set_text(front: &mut Y, key: &str, v: &str) {
    if v.is_empty() {
        front.remove(key);
    } else {
        front.set(key, Y::Str(v.to_string()), after(key));
    }
}

/// The schema keys after `key` (a new key goes before the first of them present).
fn after(key: &str) -> &'static [&'static str] {
    let i = ORDER.iter().position(|k| *k == key).map_or(ORDER.len(), |i| i + 1);
    &ORDER[i..]
}

/// A library image that exists.
pub fn check_image(folder: &Path, stored: &str, what: &str) -> Result<(), String> {
    if !stored.starts_with("public/assets/images/") {
        return Err(format!("{what}要用资源库里的图片：{stored}"));
    }
    match public_file(folder, stored) {
        Some(p) if p.is_file() => Ok(()),
        _ => Err(format!("{what}不在网站文件夹里：{stored}")),
    }
}

/// A work's title (中文 first) for 用途 in the library.
pub fn title_of(folder: &Path, slug: &str) -> Option<String> {
    LANGS.iter().find_map(|l| read_doc(&file(folder, slug, l)).ok().flatten().map(|d| text(&d.front, "title")).filter(|t| !t.is_empty()))
}

/// The fields and body a version's file would have.
fn render(old: Option<&Doc>, lang: &str, w: &Work, v: &Version) -> (Y, String) {
    let mut front = old.map(|d| d.front.clone()).unwrap_or(Y::Map(vec![]));
    front.set("title", Y::Str(v.title.trim().to_string()), after("title"));
    front.set("language", Y::Str(lang.into()), after("language"));
    front.set("category", Y::Str(w.category.clone()), after("category"));
    front.set("topics", Y::Seq(w.topics.iter().map(|t| Y::Str(t.clone())).collect()), after("topics"));
    set_text(&mut front, "month", &w.month);
    set_text(&mut front, "summary", v.summary.trim_end());
    if lang == "zh" {
        set_text(&mut front, "englishTitle", v.english_title.trim());
        set_text(&mut front, "englishSummary", v.english_summary.trim_end());
    }
    match &w.cover {
        Some(c) => front.set("thumbnailRef", Y::Str(c.clone()), after("thumbnailRef")),
        None => front.remove("thumbnailRef"),
    }
    let items: Vec<Y> = w
        .gallery
        .iter()
        .map(|g| {
            let (alt, caption) = if lang == "zh" { (&g.alt_zh, &g.caption_zh) } else { (&g.alt_en, &g.caption_en) };
            let mut m = vec![("imageRef".to_string(), Y::Str(g.image.clone()))];
            for (k, val) in [("alt", alt), ("caption", caption)] {
                if !val.trim().is_empty() {
                    m.push((k.to_string(), Y::Str(val.trim_end().to_string())));
                }
            }
            Y::Map(m)
        })
        .collect();
    if !items.is_empty() || front.get("gallery").is_some() {
        front.set("gallery", Y::Seq(items), after("gallery"));
    }
    let mut body = v.body.clone();
    if !body.trim().is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    if body.trim().is_empty() {
        body.clear();
    }
    (front, body)
}

fn render_text(old: Option<&Doc>, lang: &str, w: &Work, v: &Version) -> String {
    let (front, body) = render(old, lang, w, v);
    yaml::join_front(&front, &body)
}

/// Saves both versions (each that exists); returns the work as now on disk.
pub fn save(folder: &Path, w: &Work, taxonomy: &Taxonomy) -> Result<Work, String> {
    if !valid_slug(&w.slug) {
        return Err("作品的 slug 不对".into());
    }
    check_prints(folder, &w.slug, &w.prints)?;
    let mut old = vec![];
    for l in LANGS {
        let doc = read_doc(&file(folder, &w.slug, l))?;
        let given = if l == "zh" { w.zh.as_ref() } else { w.en.as_ref() };
        if doc.is_some() != given.is_some() {
            return Err("作品的版本和文件对不上，先重新读取".into());
        }
        if let Some(v) = given {
            if v.title.trim().is_empty() {
                return Err(if l == "zh" { "中文标题不能空着" } else { "英文标题不能空着" }.into());
            }
        }
        old.push((l, doc, given));
    }
    if !w.month.is_empty() && !valid_month(&w.month) {
        return Err("年月要写成 2025-05 这样".into());
    }
    let had = old.iter().find_map(|(_, d, _)| d.as_ref()).map(|d| text(&d.front, "category")).unwrap_or_default();
    if !w.category.is_empty() && w.category != had && !taxonomy.categories.iter().any(|c| c.slug == w.category) {
        return Err("没有这个类别".into());
    }
    if let Some(c) = &w.cover {
        check_image(folder, c, "封面图片")?;
    }
    if w.gallery.len() == 1 {
        return Err("画廊至少要两张图（网站的检查要求）；只有一张就当封面或放进正文".into());
    }
    for g in &w.gallery {
        check_image(folder, &g.image, "画廊里的图片")?;
    }
    for (l, doc, given) in &old {
        let (Some(doc), Some(v)) = (doc, given) else { continue };
        let (front, body) = render(Some(doc), l, w, v);
        if front == doc.front && body.trim_end() == doc.body.trim_end() {
            continue;
        }
        let path = file(folder, &w.slug, l);
        std::fs::write(&path, yaml::join_front(&front, &body)).map_err(|e| format!("写不了 {}：{e}", path.display()))?;
    }
    load(folder, &w.slug)
}

/// 新作品: one version (zh or en) with a title; this month by default.
pub fn create(folder: &Path, slug: &str, lang: &str, title: &str, month: &str) -> Result<Work, String> {
    if !valid_slug(slug) {
        return Err("slug 只能用小写英文字母、数字和短横线，比如 big-fish".into());
    }
    if !LANGS.contains(&lang) {
        return Err("语言不对".into());
    }
    if title.trim().is_empty() {
        return Err("标题不能空着".into());
    }
    if LANGS.iter().any(|l| file(folder, slug, l).exists()) {
        return Err(format!("已经有叫 {slug} 的作品了"));
    }
    let w = Work { slug: slug.into(), month: month.into(), ..Default::default() };
    let v = Version { title: title.trim().into(), ..Default::default() };
    std::fs::create_dir_all(folder.join(DIR)).map_err(|e| e.to_string())?;
    std::fs::write(file(folder, slug, lang), render_text(None, lang, &w, &v)).map_err(|e| format!("写不了新作品：{e}"))?;
    load(folder, slug)
}

/// 新建英文版 / 中文版: the other version's shared fields; an English one starts from the stub's title
/// and summary.
pub fn add_version(folder: &Path, slug: &str, lang: &str, prints: &Prints) -> Result<Work, String> {
    check_prints(folder, slug, prints)?;
    let mut w = load(folder, slug)?;
    let (have, other) = if lang == "en" { (&w.en, &w.zh) } else { (&w.zh, &w.en) };
    if have.is_some() {
        return Err("这个版本已经有了".into());
    }
    let other = other.clone().ok_or("找不到这个作品")?;
    let v = if lang == "en" {
        Version { title: if other.english_title.trim().is_empty() { other.title.clone() } else { other.english_title.clone() }, summary: other.english_summary.clone(), ..Default::default() }
    } else {
        Version { title: other.title.clone(), ..Default::default() }
    };
    for g in &mut w.gallery {
        if lang == "en" {
            (g.alt_en, g.caption_en) = (String::new(), String::new());
        } else {
            (g.alt_zh, g.caption_zh) = (String::new(), String::new());
        }
    }
    std::fs::write(file(folder, slug, lang), render_text(None, lang, &w, &v)).map_err(|e| format!("写不了新版本：{e}"))?;
    load(folder, slug)
}

/// Renames both files (the images stay where they are).
pub fn rename(folder: &Path, slug: &str, to: &str, prints: &Prints) -> Result<Work, String> {
    if !valid_slug(to) {
        return Err("slug 只能用小写英文字母、数字和短横线，比如 big-fish".into());
    }
    if to == slug {
        return load(folder, slug);
    }
    check_prints(folder, slug, prints)?;
    if LANGS.iter().any(|l| file(folder, to, l).exists()) {
        return Err(format!("已经有叫 {to} 的作品了"));
    }
    for l in LANGS {
        let from = file(folder, slug, l);
        if from.exists() {
            std::fs::rename(&from, file(folder, to, l)).map_err(|e| format!("改名失败：{e}"))?;
        }
    }
    load(folder, to)
}

/// Deletes both files (not the images: others may use them).
pub fn delete(folder: &Path, slug: &str, prints: &Prints) -> Result<(), String> {
    if !valid_slug(slug) {
        return Err("作品的 slug 不对".into());
    }
    check_prints(folder, slug, prints)?;
    for l in LANGS {
        let p = file(folder, slug, l);
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| format!("删不了 {}：{e}", p.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use crate::site::changes::tests::write;

    const ZH: &str = "---\ntitle: 百言集 | 大鱼\nlanguage: zh\ncategory: literature\ntopics:\n  - essays\nmonth: '2025-10'\nthumbnailRef: public/assets/images/card.webp\nenglishTitle: 'Hundred Tales: Big Fish'\nenglishSummary: An essay.\ncustom: kept\n---\n![](/assets/images/big-fish.jpg)\n\n正文\n";

    fn site(dir: &Path) -> PathBuf {
        let s = dir.join("site");
        write(&s, "src/content/wander/big-fish.zh.md", ZH);
        write(&s, "src/content/wander/photos.zh.md", "---\ntitle: 摄影集\nlanguage: zh\ncategory: gallery\ntopics: []\nmonth: 2025-09\ngallery:\n  - imageRef: public/assets/images/a.jpg\n    alt: 一\n  - imageRef: public/assets/images/b.jpg\n---\n");
        write(&s, "src/content/wander/photos.en.md", "---\ntitle: Photos\nlanguage: en\ncategory: gallery\ntopics: []\nmonth: 2025-09\nsummary: Ten photos.\ngallery:\n  - imageRef: public/assets/images/a.jpg\n    alt: One\n    caption: First\n  - imageRef: public/assets/images/b.jpg\n---\n");
        write(&s, "src/content/wander/notes.txt", "x");
        write(&s, "src/data/wander-taxonomy.json", r#"{"categories":[{"slug":"literature","zh":"创作","en":"Literature"},{"slug":"gallery","zh":"画廊","en":"Gallery"}],"topics":[{"slug":"essays","zh":"随笔","en":"Essays"}]}"#);
        for p in ["public/assets/images/card.webp", "public/assets/images/a.jpg", "public/assets/images/b.jpg", "public/assets/images/new.jpg", "public/images/og/x.png"] {
            write(&s, p, [1u8]);
        }
        s
    }

    #[test]
    fn lists_works_and_taxonomy() {
        let dir = temp_dir("site-wander-list");
        let s = site(&dir);
        write(&s, "src/content/wander/broken.zh.md", "no front matter");
        let w = list(&s).unwrap();
        assert_eq!(w.works.iter().map(|r| (r.slug.as_str(), r.title.as_str(), r.langs.clone())).collect::<Vec<_>>(), [("big-fish", "百言集 | 大鱼", vec!["zh"]), ("photos", "摄影集", vec!["zh", "en"])]);
        assert_eq!(w.works[0].month, "2025-10");
        assert_eq!(w.taxonomy.categories[0], Term { slug: "literature".into(), zh: "创作".into(), en: "Literature".into() });
        assert_eq!(w.broken.len(), 1);
        assert!(valid_month("2025-12") && !valid_month("2025-13") && !valid_month("2025-1"));
        assert!(valid_slug("big-fish-2") && !valid_slug("Big") && !valid_slug("a--b") && !valid_slug("-a") && !valid_slug("中文"));
    }

    #[test]
    fn loads_both_versions_and_saves_unchanged_as_is() {
        let dir = temp_dir("site-wander-load");
        let s = site(&dir);
        let w = load(&s, "photos").unwrap();
        assert_eq!(w.gallery, vec![
            GalleryItem { image: "public/assets/images/a.jpg".into(), alt_zh: "一".into(), caption_zh: "".into(), alt_en: "One".into(), caption_en: "First".into() },
            GalleryItem { image: "public/assets/images/b.jpg".into(), ..Default::default() },
        ]);
        assert_eq!((w.zh.as_ref().unwrap().title.as_str(), w.en.as_ref().unwrap().summary.as_str()), ("摄影集", "Ten photos."));
        let before: Vec<_> = LANGS.iter().map(|l| std::fs::read_to_string(file(&s, "photos", l)).unwrap()).collect();
        let tax = taxonomy(&s);
        let again = save(&s, &w, &tax).unwrap();
        assert_eq!(again, w);
        let after: Vec<_> = LANGS.iter().map(|l| std::fs::read_to_string(file(&s, "photos", l)).unwrap()).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn saves_shared_fields_to_both_and_keeps_unknown_keys() {
        let dir = temp_dir("site-wander-save");
        let s = site(&dir);
        let tax = taxonomy(&s);
        let mut w = load(&s, "big-fish").unwrap();
        w.month = "2025-11".into();
        w.topics = vec![];
        w.cover = Some("public/assets/images/new.jpg".into());
        w.gallery = vec![GalleryItem { image: "public/assets/images/a.jpg".into(), alt_zh: "第一张".into(), ..Default::default() }, GalleryItem { image: "public/assets/images/b.jpg".into(), ..Default::default() }];
        w.zh.as_mut().unwrap().summary = "一句话".into();
        w.zh.as_mut().unwrap().english_summary = "".into();
        w.zh.as_mut().unwrap().body = "新的正文".into();
        let w = save(&s, &w, &tax).unwrap();
        assert_eq!(
            std::fs::read_to_string(file(&s, "big-fish", "zh")).unwrap(),
            "---\ntitle: 百言集 | 大鱼\nlanguage: zh\ncategory: literature\ntopics: []\nmonth: 2025-11\nsummary: 一句话\nthumbnailRef: public/assets/images/new.jpg\nenglishTitle: 'Hundred Tales: Big Fish'\ncustom: kept\ngallery:\n  - imageRef: public/assets/images/a.jpg\n    alt: 第一张\n  - imageRef: public/assets/images/b.jpg\n---\n新的正文\n"
        );
        // a new English version: the stub's title, shared fields, its own (empty) alts
        let w = add_version(&s, "big-fish", "en", &w.prints).unwrap();
        assert_eq!(
            std::fs::read_to_string(file(&s, "big-fish", "en")).unwrap(),
            "---\ntitle: 'Hundred Tales: Big Fish'\nlanguage: en\ncategory: literature\ntopics: []\nmonth: 2025-11\nthumbnailRef: public/assets/images/new.jpg\ngallery:\n  - imageRef: public/assets/images/a.jpg\n  - imageRef: public/assets/images/b.jpg\n---\n"
        );
        assert!(add_version(&s, "big-fish", "en", &w.prints).unwrap_err().contains("已经有了"));
        // both written together
        let mut w2 = w.clone();
        w2.category = "gallery".into();
        w2.en.as_mut().unwrap().body = "Body".into();
        w2.gallery[0].alt_en = "First".into();
        let w2 = save(&s, &w2, &tax).unwrap();
        assert_eq!(w2.category, "gallery");
        assert!(std::fs::read_to_string(file(&s, "big-fish", "en")).unwrap().contains("category: gallery\n") && std::fs::read_to_string(file(&s, "big-fish", "zh")).unwrap().contains("category: gallery\n"));
        assert_eq!(w2.gallery[0].alt_en, "First");
        // removing the cover removes its field
        let mut w3 = w2.clone();
        w3.cover = None;
        let w3 = save(&s, &w3, &tax).unwrap();
        assert!(w3.cover.is_none() && !std::fs::read_to_string(file(&s, "big-fish", "zh")).unwrap().contains("thumbnail"));
    }

    #[test]
    fn untouched_versions_keep_their_old_formatting() {
        let dir = temp_dir("site-wander-old");
        let s = site(&dir);
        write(&s, "src/content/wander/big-fish.en.md", "---\ntitle: 'Big Fish'\nlanguage: en\ncategory: literature\ntopics: [essays]\nmonth: '2025-10'\nthumbnailRef: public/assets/images/card.webp\n---\nBody");
        let tax = taxonomy(&s);
        let mut w = load(&s, "big-fish").unwrap();
        let en_before = std::fs::read_to_string(file(&s, "big-fish", "en")).unwrap();
        w.zh.as_mut().unwrap().body = "改了中文".into();
        save(&s, &w, &tax).unwrap();
        assert_eq!(std::fs::read_to_string(file(&s, "big-fish", "en")).unwrap(), en_before);
        assert!(std::fs::read_to_string(file(&s, "big-fish", "zh")).unwrap().contains("month: 2025-10\n"));
    }

    #[test]
    fn refuses_stale_or_bad_saves() {
        let dir = temp_dir("site-wander-refuse");
        let s = site(&dir);
        let tax = taxonomy(&s);
        let w = load(&s, "photos").unwrap();
        let mut bad = w.clone();
        bad.month = "2025-5".into();
        assert!(save(&s, &bad, &tax).unwrap_err().contains("年月"));
        let mut bad = w.clone();
        bad.category = "nope".into();
        assert!(save(&s, &bad, &tax).unwrap_err().contains("类别"));
        let mut bad = w.clone();
        bad.en.as_mut().unwrap().title = " ".into();
        assert!(save(&s, &bad, &tax).unwrap_err().contains("英文标题"));
        let mut bad = w.clone();
        bad.cover = Some("public/assets/images/missing.jpg".into());
        assert!(save(&s, &bad, &tax).unwrap_err().contains("封面"));
        bad.cover = Some("public/images/og/x.png".into());
        assert!(save(&s, &bad, &tax).unwrap_err().contains("资源库"));
        let mut bad = w.clone();
        bad.gallery.truncate(1);
        assert!(save(&s, &bad, &tax).unwrap_err().contains("至少要两张"));
        let mut bad = w.clone();
        bad.cover = Some("public/../secret".into());
        assert!(save(&s, &bad, &tax).is_err());
        // changed elsewhere since it was read
        write(&s, "src/content/wander/photos.en.md", "---\ntitle: Photos!\nlanguage: en\n---\n");
        assert!(save(&s, &w, &tax).unwrap_err().contains("英文版在别处被改过了"));
        assert!(delete(&s, "photos", &w.prints).is_err());
        assert!(rename(&s, "photos", "pics", &w.prints).is_err());
    }

    #[test]
    fn create_rename_delete() {
        let dir = temp_dir("site-wander-crud");
        let s = site(&dir);
        assert!(create(&s, "Big Fish", "zh", "x", "").unwrap_err().contains("slug"));
        assert!(create(&s, "big-fish", "zh", "x", "").unwrap_err().contains("已经有"));
        assert!(create(&s, "spring", "zh", " ", "").unwrap_err().contains("标题"));
        let w = create(&s, "spring", "en", "Spring Walk", "2026-10").unwrap();
        assert_eq!(std::fs::read_to_string(file(&s, "spring", "en")).unwrap(), "---\ntitle: Spring Walk\nlanguage: en\ncategory: ''\ntopics: []\nmonth: 2026-10\n---\n");
        assert!(w.zh.is_none() && w.en.is_some());
        let w = rename(&s, "spring", "spring-walk", &w.prints).unwrap();
        assert!(w.slug == "spring-walk" && !file(&s, "spring", "en").exists() && file(&s, "spring-walk", "en").exists());
        assert!(rename(&s, "spring-walk", "photos", &w.prints).unwrap_err().contains("已经有"));
        delete(&s, "spring-walk", &w.prints).unwrap();
        assert!(!file(&s, "spring-walk", "en").exists() && load(&s, "spring-walk").is_err());
    }

    #[test]
    fn image_paths() {
        assert_eq!(url_of("public/images/a b.jpg"), "/images/a b.jpg");
        assert_eq!(url_of("/images/x.jpg"), "/images/x.jpg");
        let f = Path::new("/site");
        assert_eq!(public_file(f, "public/images/a.jpg"), Some(PathBuf::from("/site/public/images/a.jpg")));
        assert_eq!(public_file(f, "/images/a.jpg?v=2"), Some(PathBuf::from("/site/public/images/a.jpg")));
        assert_eq!(public_file(f, "/images/../../etc/passwd"), None);
        assert_eq!(public_file(f, "images/a.jpg"), None);
    }
}
