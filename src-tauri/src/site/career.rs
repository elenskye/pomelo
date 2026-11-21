//! 职业页 (`src/content/career/<zh|en>.md`, each language its own): email (required by the site),
//! resumeDownloads [label, fileRef], summary, sections
//! [title, content: {discriminant: simple | detailed, value: [...]}], body = extra 附加内容.
//! - simple rows: heading, items (text); detailed rows: period, role, org, bullets (one line each).
//! - A résumé needs its label and a document of the library (`public/assets/documents/`).
//! - Rows are rebuilt in the site's format (empty text left out); top-level keys Pomelo doesn't
//!   know stay.

use super::doc::{self, read_md, Prints};
use super::wander::public_file;
use super::yaml::Y;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const ORDER: [&str; 5] = ["language", "email", "resumeDownloads", "summary", "sections"];

fn file(folder: &Path, lang: &str) -> PathBuf {
    folder.join("src/content/career").join(format!("{lang}.md"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Resume {
    pub label: String,
    /// The stored path ("public/assets/documents/…").
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Simple {
    pub heading: String,
    pub items: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Detailed {
    pub period: String,
    pub role: String,
    pub org: String,
    pub bullets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Section {
    pub title: String,
    /// simple | detailed
    pub mode: String,
    pub simple: Vec<Simple>,
    pub detailed: Vec<Detailed>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Version {
    pub email: String,
    pub summary: String,
    pub resumes: Vec<Resume>,
    pub sections: Vec<Section>,
    pub extra: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Career {
    pub zh: Option<Version>,
    pub en: Option<Version>,
    pub prints: Prints,
}

fn read_version(front: &Y, body: String) -> Version {
    let resumes = front
        .get("resumeDownloads")
        .map(|r| {
            r.seq()
                .iter()
                .map(|it| Resume { label: doc::text(it, "label"), file: doc::text(it, "fileRef") })
                .collect()
        })
        .unwrap_or_default();
    let sections = front
        .get("sections")
        .map(|s| {
            s.seq()
                .iter()
                .map(|sec| {
                    let content = sec.get("content");
                    let mode = content.map(|c| doc::text(c, "discriminant")).filter(|m| m == "simple").unwrap_or_else(|| "detailed".into());
                    let rows = content.and_then(|c| c.get("value")).map(Y::seq).unwrap_or(&[]);
                    let (mut simple, mut detailed) = (vec![], vec![]);
                    for r in rows {
                        if mode == "simple" {
                            simple.push(Simple { heading: doc::text(r, "heading"), items: doc::text(r, "items") });
                        } else {
                            detailed.push(Detailed { period: doc::text(r, "period"), role: doc::text(r, "role"), org: doc::text(r, "org"), bullets: doc::strings(r, "bullets") });
                        }
                    }
                    Section { title: doc::text(sec, "title"), mode, simple, detailed }
                })
                .collect()
        })
        .unwrap_or_default();
    Version { email: doc::text(front, "email"), summary: doc::text(front, "summary"), resumes, sections, extra: body }
}

pub fn load(folder: &Path) -> Result<Career, String> {
    let mut c = Career::default();
    for l in ["zh", "en"] {
        let Some(d) = read_md(&file(folder, l))? else { continue };
        let v = read_version(&d.front, d.body);
        if l == "zh" {
            (c.zh, c.prints.zh) = (Some(v), Some(d.print));
        } else {
            (c.en, c.prints.en) = (Some(v), Some(d.print));
        }
    }
    Ok(c)
}

fn check(folder: &Path, lang: &str, v: &Version) -> Result<(), String> {
    let which = if lang == "zh" { "中文" } else { "英文" };
    if v.email.trim().is_empty() {
        return Err(format!("{which}职业页的联系邮箱不能空着"));
    }
    for r in &v.resumes {
        if r.label.trim().is_empty() || r.file.trim().is_empty() {
            return Err(format!("{which}职业页的每个简历都要有按钮文字和文件"));
        }
        if !r.file.starts_with("public/assets/documents/") {
            return Err(format!("{which}简历要用资源库里的文档：{}", r.file));
        }
        if !public_file(folder, &r.file).is_some_and(|p| p.is_file()) {
            return Err(format!("简历文件不在网站文件夹里：{}", r.file));
        }
    }
    if v.sections.iter().any(|s| s.mode != "simple" && s.mode != "detailed") {
        return Err("模块的内容模式不对".into());
    }
    Ok(())
}

fn write_front(old: &Y, lang: &str, v: &Version) -> Y {
    let mut f = old.clone();
    doc::set(&mut f, &ORDER, "language", Y::Str(lang.into()));
    doc::set(&mut f, &ORDER, "email", Y::Str(v.email.trim().into()));
    let resumes = v.resumes.iter().map(|r| doc::map(&[("label", r.label.trim()), ("fileRef", r.file.trim())])).collect();
    doc::set(&mut f, &ORDER, "resumeDownloads", Y::Seq(resumes));
    doc::set_text(&mut f, &ORDER, "summary", v.summary.trim_end());
    let sections = v
        .sections
        .iter()
        .map(|s| {
            let rows: Vec<Y> = if s.mode == "simple" {
                s.simple.iter().map(|r| doc::map(&[("heading", r.heading.trim()), ("items", r.items.trim_end())])).collect()
            } else {
                s.detailed
                    .iter()
                    .map(|r| {
                        let Y::Map(mut m) = doc::map(&[("period", r.period.trim()), ("role", r.role.trim()), ("org", r.org.trim())]) else { unreachable!() };
                        m.push(("bullets".into(), doc::strs(&r.bullets.iter().map(|b| b.trim().to_string()).filter(|b| !b.is_empty()).collect::<Vec<_>>())));
                        Y::Map(m)
                    })
                    .collect()
            };
            let Y::Map(mut m) = doc::map(&[("title", s.title.trim())]) else { unreachable!() };
            m.push(("content".into(), Y::Map(vec![("discriminant".into(), Y::Str(s.mode.clone())), ("value".into(), Y::Seq(rows))])));
            Y::Map(m)
        })
        .collect();
    doc::set(&mut f, &ORDER, "sections", Y::Seq(sections));
    f
}

pub fn save(folder: &Path, c: &Career) -> Result<Career, String> {
    let mut olds = vec![];
    for l in ["zh", "en"] {
        let p = file(folder, l);
        doc::check_print(&p, c.prints.of(l), if l == "zh" { "职业页（中文）" } else { "职业页（英文）" })?;
        let given = if l == "zh" { &c.zh } else { &c.en };
        let old = read_md(&p)?;
        if old.is_some() != given.is_some() {
            return Err("职业页的版本和文件对不上，先重新读取".into());
        }
        if let Some(v) = given {
            check(folder, l, v)?;
        }
        olds.push((l, p, old, given));
    }
    for (l, p, old, given) in olds {
        let (Some(old), Some(v)) = (old, given) else { continue };
        doc::write_md(&p, Some(&old), &write_front(&old.front, l, v), &doc::body(&v.extra))?;
    }
    load(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use crate::site::changes::tests::write;

    const ZH: &str = "---\nlanguage: zh\nemail: me@example.com\nresumeDownloads:\n  - label: 下载简历\n    fileRef: public/assets/documents/cv.pdf\nsummary: 一段摘要\nsections:\n  - title: 技能\n    content:\n      discriminant: simple\n      value:\n        - heading: 编程\n          items: Rust\n  - title: 项目\n    content:\n      discriminant: detailed\n      value:\n        - period: '2025'\n          role: 作者\n          org: Pomelo\n          bullets:\n            - 写了它\n---\n";

    #[test]
    fn reads_and_writes_in_the_site_format() {
        let dir = temp_dir("site-career");
        let s = dir.join("site");
        write(&s, "src/content/career/zh.md", ZH);
        write(&s, "public/assets/documents/cv.pdf", b"%PDF");
        write(&s, "public/cv/en/cv.pdf", b"%PDF-en");
        let c = load(&s).unwrap();
        let v = c.zh.clone().unwrap();
        assert_eq!(v.resumes, vec![Resume { label: "下载简历".into(), file: "public/assets/documents/cv.pdf".into() }]);
        assert_eq!((v.sections[0].mode.as_str(), v.sections[0].simple[0].items.as_str()), ("simple", "Rust"));
        assert_eq!(v.sections[1].detailed[0], Detailed { period: "2025".into(), role: "作者".into(), org: "Pomelo".into(), bullets: vec!["写了它".into()] });
        // written by Pomelo: the same text as before; saved as is: untouched
        let old = read_md(&s.join("src/content/career/zh.md")).unwrap().unwrap();
        assert_eq!(crate::site::yaml::join_front(&write_front(&old.front, "zh", &v), ""), ZH);
        let c = save(&s, &c).unwrap();
        assert_eq!(std::fs::read_to_string(s.join("src/content/career/zh.md")).unwrap(), ZH);
        // a new section, a removed row, an empty bullet dropped
        let mut d = c.clone();
        let v = d.zh.as_mut().unwrap();
        v.sections.remove(0);
        v.sections.push(Section { title: String::new(), mode: "detailed".into(), detailed: vec![Detailed { role: "实习".into(), bullets: vec!["".into(), "做事".into()], ..Default::default() }], ..Default::default() });
        v.extra = "附加".into();
        save(&s, &d).unwrap();
        assert_eq!(
            std::fs::read_to_string(s.join("src/content/career/zh.md")).unwrap(),
            "---\nlanguage: zh\nemail: me@example.com\nresumeDownloads:\n  - label: 下载简历\n    fileRef: public/assets/documents/cv.pdf\nsummary: 一段摘要\nsections:\n  - title: 项目\n    content:\n      discriminant: detailed\n      value:\n        - period: '2025'\n          role: 作者\n          org: Pomelo\n          bullets:\n            - 写了它\n  - content:\n      discriminant: detailed\n      value:\n        - role: 实习\n          bullets:\n            - 做事\n---\n附加\n"
        );
    }

    #[test]
    fn refuses_what_the_site_would_break_on() {
        let dir = temp_dir("site-career-bad");
        let s = dir.join("site");
        write(&s, "src/content/career/zh.md", ZH);
        write(&s, "public/assets/documents/cv.pdf", b"%PDF");
        write(&s, "public/cv/en/cv.pdf", b"%PDF-en");
        let c = load(&s).unwrap();
        let bad = |f: &dyn Fn(&mut Version)| {
            let mut d = c.clone();
            f(d.zh.as_mut().unwrap());
            save(&s, &d).unwrap_err()
        };
        assert!(bad(&|v| v.email = " ".into()).contains("联系邮箱"));
        assert!(bad(&|v| v.resumes[0].label.clear()).contains("按钮文字"));
        assert!(bad(&|v| v.resumes[0].file = "public/cv/en/cv.pdf".into()).contains("资源库"));
        assert!(bad(&|v| v.resumes[0].file = "public/assets/documents/none.pdf".into()).contains("不在网站文件夹里"));
        assert!(bad(&|v| v.sections[0].mode = "x".into()).contains("模式"));
        write(&s, "src/content/career/zh.md", "---\nlanguage: zh\nemail: x@y.z\n---\n");
        assert!(save(&s, &c).unwrap_err().contains("别处被改过了"));
    }
}
