//! 站点: 话题与类别 (`src/data/wander-taxonomy.json`: categories / topics, each {slug, zh, en}) and 外部链接
//! (`src/data/social-links.json`: links [{key, label, href}]).
//! - A term's slug is its stable ID: works store it. Kept as it is once made (renaming is the names);
//!   new ones lowercase letters, digits and dashes, unique in their list. Removing one is the site's
//!   to absorb (works lose it at render); Pomelo says how many works use it first.

use super::doc::{self, read_json};
use super::wander::{self, valid_slug, Term};
use super::yaml::Y;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const TAXONOMY: &str = "src/data/wander-taxonomy.json";
pub const LINKS: &str = "src/data/social-links.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Link {
    /// The icon: github / linkedin / email, others a plain one.
    pub key: String,
    pub label: String,
    pub href: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TermsIn {
    pub categories: Vec<TermIn>,
    pub topics: Vec<TermIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TermIn {
    pub slug: String,
    pub zh: String,
    pub en: String,
    /// Works using it (filled when read).
    pub used: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SiteData {
    pub terms: TermsIn,
    pub links: Vec<Link>,
    pub terms_print: Option<String>,
    pub links_print: Option<String>,
}

pub fn load(folder: &Path) -> Result<SiteData, String> {
    let tax = wander::taxonomy(folder);
    let works = wander::list(folder).map(|w| w.works).unwrap_or_default();
    let with_use = |list: Vec<Term>, topics: bool| {
        list.into_iter()
            .map(|t| {
                let used = works.iter().filter(|w| if topics { w.topics.contains(&t.slug) } else { w.category == t.slug }).count();
                TermIn { slug: t.slug, zh: t.zh, en: t.en, used }
            })
            .collect()
    };
    let links_file = read_json(&folder.join(LINKS))?;
    let links = links_file.as_ref().map(|(v, _)| v.get("links").map(|l| l.seq().iter().map(|x| Link { key: doc::text(x, "key"), label: doc::text(x, "label"), href: doc::text(x, "href") }).collect()).unwrap_or_default()).unwrap_or_default();
    Ok(SiteData {
        terms: TermsIn { categories: with_use(tax.categories, false), topics: with_use(tax.topics, true) },
        links,
        terms_print: doc::print_of(&folder.join(TAXONOMY)),
        links_print: links_file.map(|(_, p)| p),
    })
}

fn check_terms(list: &[TermIn], which: &str) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for t in list {
        if !valid_slug(&t.slug) {
            return Err(format!("{which}的 ID「{}」只能用小写英文字母、数字和短横线", t.slug));
        }
        if !seen.insert(&t.slug) {
            return Err(format!("{which}的 ID「{}」重复了", t.slug));
        }
        if t.zh.trim().is_empty() || t.en.trim().is_empty() {
            return Err(format!("{which}「{}」要有中文名和英文名", t.slug));
        }
    }
    Ok(())
}

/// Saves the taxonomy; true when it changed (and was written).
pub fn save_terms(folder: &Path, terms: &TermsIn, print: Option<&String>) -> Result<bool, String> {
    check_terms(&terms.categories, "类别")?;
    check_terms(&terms.topics, "话题")?;
    let p = folder.join(TAXONOMY);
    doc::check_print(&p, print, "话题与类别")?;
    let old = read_json(&p)?.map(|(v, _)| v);
    let mut v = old.clone().unwrap_or(Y::Map(vec![]));
    let list = |l: &[TermIn]| Y::Seq(l.iter().map(|t| doc::map(&[("slug", t.slug.as_str()), ("zh", t.zh.trim()), ("en", t.en.trim())])).collect());
    v.set("categories", list(&terms.categories), &["topics"]);
    v.set("topics", list(&terms.topics), &[]);
    doc::write_json(&p, old.as_ref(), &v)?;
    Ok(old.as_ref() != Some(&v))
}

pub fn save_links(folder: &Path, links: &[Link], print: Option<&String>) -> Result<(), String> {
    for l in links {
        if l.key.trim().is_empty() || l.label.trim().is_empty() || l.href.trim().is_empty() {
            return Err("每个外部链接都要有图标、名称和链接".into());
        }
    }
    let p = folder.join(LINKS);
    doc::check_print(&p, print, "外部链接")?;
    let old = read_json(&p)?.map(|(v, _)| v);
    let mut v = old.clone().unwrap_or(Y::Map(vec![]));
    v.set("links", Y::Seq(links.iter().map(|l| doc::map(&[("key", l.key.trim()), ("label", l.label.trim()), ("href", l.href.trim())])).collect()), &[]);
    doc::write_json(&p, old.as_ref(), &v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use crate::site::changes::tests::write;

    #[test]
    fn terms_and_links() {
        let dir = temp_dir("site-terms");
        let s = dir.join("site");
        write(&s, TAXONOMY, "{\n  \"categories\": [\n    {\n      \"slug\": \"notes\",\n      \"zh\": \"写作\",\n      \"en\": \"Notes\"\n    }\n  ],\n  \"topics\": [\n    {\n      \"slug\": \"essays\",\n      \"zh\": \"随笔\",\n      \"en\": \"Essays\"\n    }\n  ]\n}\n");
        write(&s, LINKS, "{\n  \"links\": [\n    { \"key\": \"github\", \"label\": \"GitHub\", \"href\": \"https://github.com/x\" }\n  ]\n}\n");
        write(&s, "src/content/wander/a.zh.md", "---\ntitle: A\ncategory: notes\ntopics:\n  - essays\n---\n");
        let d = load(&s).unwrap();
        assert_eq!((d.terms.categories[0].used, d.terms.topics[0].used), (1, 1));
        assert_eq!(d.links, vec![Link { key: "github".into(), label: "GitHub".into(), href: "https://github.com/x".into() }]);
        // unchanged: nothing written
        assert!(!save_terms(&s, &d.terms, d.terms_print.as_ref()).unwrap());
        let mut t = d.terms.clone();
        t.topics.push(TermIn { slug: "travelogue".into(), zh: "游记".into(), en: "Travelogue".into(), used: 0 });
        t.categories[0].zh = "随写".into();
        assert!(save_terms(&s, &t, d.terms_print.as_ref()).unwrap());
        assert!(std::fs::read_to_string(s.join(TAXONOMY)).unwrap().contains("\"zh\": \"随写\"") && wander::taxonomy(&s).topics.len() == 2);
        let mut bad = t.clone();
        bad.topics[1].slug = "essays".into();
        assert!(save_terms(&s, &bad, doc::print_of(&s.join(TAXONOMY)).as_ref()).unwrap_err().contains("重复"));
        bad.topics[1].slug = "Travel Notes".into();
        assert!(save_terms(&s, &bad, doc::print_of(&s.join(TAXONOMY)).as_ref()).unwrap_err().contains("小写"));
        assert!(save_terms(&s, &t, d.terms_print.as_ref()).unwrap_err().contains("别处被改过了"));
        // links: an older JSON layout
        let mut links = d.links.clone();
        links.push(Link { key: "email".into(), label: "Email".into(), href: "mailto:a@b.c".into() });
        save_links(&s, &links, d.links_print.as_ref()).unwrap();
        assert_eq!(std::fs::read_to_string(s.join(LINKS)).unwrap(), "{\n  \"links\": [\n    {\n      \"key\": \"github\",\n      \"label\": \"GitHub\",\n      \"href\": \"https://github.com/x\"\n    },\n    {\n      \"key\": \"email\",\n      \"label\": \"Email\",\n      \"href\": \"mailto:a@b.c\"\n    }\n  ]\n}\n");
        assert!(save_links(&s, &[Link::default()], doc::print_of(&s.join(LINKS)).as_ref()).unwrap_err().contains("都要有"));
    }
}
