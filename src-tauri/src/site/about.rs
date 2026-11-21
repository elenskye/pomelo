//! 关于我 (`src/content/about/<zh|en>.md`: contextIntro 一点背景, timeline 时间轴 [role, org, period,
//! description], body = story 一点故事) and its pictures (`src/data/about-images.json`: portraitRefs /
//! artworkRefs, library images "public/assets/images/…").
//! - Time line rows need 职位、公司、时间段 (the site's schema requires them).
//! - A list is written only when it changed; both always there (the site spreads them).

use super::doc::{self, read_json, read_md, Prints};
use super::wander::check_image;
use super::yaml::Y;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const ORDER: [&str; 3] = ["language", "contextIntro", "timeline"];
pub const IMAGES: &str = "src/data/about-images.json";

fn file(folder: &Path, lang: &str) -> PathBuf {
    folder.join("src/content/about").join(format!("{lang}.md"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Entry {
    pub role: String,
    pub org: String,
    pub period: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Version {
    pub context_intro: String,
    pub timeline: Vec<Entry>,
    pub story: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Images {
    /// Stored paths, in the order the site shows them.
    pub portraits: Vec<String>,
    pub artworks: Vec<String>,
    pub print: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct About {
    pub zh: Option<Version>,
    pub en: Option<Version>,
    pub images: Images,
    pub prints: Prints,
}

fn refs(v: &Y, key: &str) -> Vec<String> {
    doc::strings(v, key).into_iter().filter(|s| !s.is_empty()).collect()
}

pub fn load(folder: &Path) -> Result<About, String> {
    let mut a = About::default();
    for l in ["zh", "en"] {
        let Some(d) = read_md(&file(folder, l))? else { continue };
        let timeline = d.front.get("timeline").map(|t| t.seq().iter().map(|e| Entry { role: doc::text(e, "role"), org: doc::text(e, "org"), period: doc::text(e, "period"), description: doc::text(e, "description") }).collect()).unwrap_or_default();
        let v = Version { context_intro: doc::text(&d.front, "contextIntro"), timeline, story: d.body };
        if l == "zh" {
            (a.zh, a.prints.zh) = (Some(v), Some(d.print));
        } else {
            (a.en, a.prints.en) = (Some(v), Some(d.print));
        }
    }
    if let Some((v, print)) = read_json(&folder.join(IMAGES))? {
        a.images = Images { portraits: refs(&v, "portraitRefs"), artworks: refs(&v, "artworkRefs"), print: Some(print) };
    }
    Ok(a)
}

pub fn save(folder: &Path, a: &About) -> Result<About, String> {
    let mut olds = vec![];
    for l in ["zh", "en"] {
        let p = file(folder, l);
        doc::check_print(&p, a.prints.of(l), if l == "zh" { "关于我（中文）" } else { "关于我（英文）" })?;
        let given = if l == "zh" { &a.zh } else { &a.en };
        let old = read_md(&p)?;
        if old.is_some() != given.is_some() {
            return Err("关于我的版本和文件对不上，先重新读取".into());
        }
        if let Some(v) = given {
            if v.timeline.iter().any(|e| e.role.trim().is_empty() || e.org.trim().is_empty() || e.period.trim().is_empty()) {
                return Err("时间轴的每一行都要填职位 / 身份、公司 / 学校和时间段".into());
            }
        }
        olds.push((l, p, old, given));
    }
    let ip = folder.join(IMAGES);
    doc::check_print(&ip, a.images.print.as_ref(), "关于我的图片")?;
    for path in a.images.portraits.iter().chain(&a.images.artworks) {
        check_image(folder, path, "图片")?;
    }
    for (l, p, old, given) in olds {
        let (Some(old), Some(v)) = (old, given) else { continue };
        let mut f = old.front.clone();
        doc::set(&mut f, &ORDER, "language", Y::Str(l.into()));
        doc::set_text(&mut f, &ORDER, "contextIntro", v.context_intro.trim_end());
        let rows = v.timeline.iter().map(|e| doc::map(&[("role", e.role.trim()), ("org", e.org.trim()), ("period", e.period.trim()), ("description", e.description.trim_end())])).collect();
        doc::set(&mut f, &ORDER, "timeline", Y::Seq(rows));
        doc::write_md(&p, Some(&old), &f, &doc::body(&v.story))?;
    }
    let old = read_json(&ip)?.map(|(v, _)| v);
    let mut v = old.clone().unwrap_or(Y::Map(vec![]));
    for (key, list) in [("portraitRefs", &a.images.portraits), ("artworkRefs", &a.images.artworks)] {
        if old.as_ref().map(|o| refs(o, key)).as_ref() != Some(list) || v.get(key).is_none() {
            v.set(key, doc::strs(list), &[]);
        }
    }
    doc::write_json(&ip, old.as_ref(), &v)?;
    load(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use crate::site::changes::tests::write;

    #[test]
    fn versions_timeline_and_pictures() {
        let dir = temp_dir("site-about");
        let s = dir.join("site");
        write(&s, "src/content/about/zh.md", "---\nlanguage: zh\ncontextIntro: |-\n  第一段\n\n  第二段\ntimeline:\n  - role: 学生\n    org: 大学\n    period: 2022 – 2026\n    description: 一句话\n---\n故事\n");
        write(&s, "src/content/about/en.md", "---\nlanguage: en\ntimeline: []\n---\nStory\n");
        write(&s, IMAGES, "{\n  \"portraitRefs\": [\n    \"public/assets/images/a.png\"\n  ],\n  \"artworkRefs\": [\n    \"public/assets/images/x.jpg\"\n  ]\n}\n");
        for p in ["public/assets/images/a.png", "public/assets/images/b.png", "public/assets/images/x.jpg", "public/images/og/x.png"] {
            write(&s, p, [1u8]);
        }
        let a = load(&s).unwrap();
        assert_eq!(a.zh.as_ref().unwrap().context_intro, "第一段\n\n第二段");
        assert_eq!(a.zh.as_ref().unwrap().timeline[0], Entry { role: "学生".into(), org: "大学".into(), period: "2022 – 2026".into(), description: "一句话".into() });
        assert_eq!((a.images.portraits.clone(), a.images.artworks.clone()), (vec!["public/assets/images/a.png".to_string()], vec!["public/assets/images/x.jpg".to_string()]));
        // saved as is: nothing written
        let files: Vec<String> = ["src/content/about/zh.md", "src/content/about/en.md", IMAGES].iter().map(|p| std::fs::read_to_string(s.join(p)).unwrap()).collect();
        let a = save(&s, &a).unwrap();
        assert_eq!(files, ["src/content/about/zh.md", "src/content/about/en.md", IMAGES].iter().map(|p| std::fs::read_to_string(s.join(p)).unwrap()).collect::<Vec<_>>());
        // a row without a period is refused; a new portrait rewrites only that kind
        let mut b = a.clone();
        b.en.as_mut().unwrap().timeline.push(Entry { role: "Dev".into(), org: "X".into(), ..Default::default() });
        assert!(save(&s, &b).unwrap_err().contains("时间段"));
        let mut b = a.clone();
        b.images.portraits.insert(0, "public/assets/images/b.png".into());
        b.en.as_mut().unwrap().timeline.push(Entry { role: "Dev".into(), org: "X".into(), period: "2026".into(), description: String::new() });
        let b = save(&s, &b).unwrap();
        assert_eq!(std::fs::read_to_string(s.join(IMAGES)).unwrap(), "{\n  \"portraitRefs\": [\n    \"public/assets/images/b.png\",\n    \"public/assets/images/a.png\"\n  ],\n  \"artworkRefs\": [\n    \"public/assets/images/x.jpg\"\n  ]\n}\n");
        assert_eq!(std::fs::read_to_string(s.join("src/content/about/en.md")).unwrap(), "---\nlanguage: en\ntimeline:\n  - role: Dev\n    org: X\n    period: '2026'\n---\nStory\n");
        assert_eq!(b.images.portraits.len(), 2);
        let mut c = b.clone();
        c.images.artworks.push("public/assets/images/missing.jpg".into());
        assert!(save(&s, &c).unwrap_err().contains("missing.jpg"));
        let mut c = b.clone();
        c.images.artworks.push("public/images/og/x.png".into());
        assert!(save(&s, &c).unwrap_err().contains("资源库"));
    }
}
