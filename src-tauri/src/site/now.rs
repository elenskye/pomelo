//! Now 月度记录 (`src/content/now/<YYYY-MM>.<zh|en>.md`: title (kept as it is), language,
//! year, month, body) and 当下状态 (`src/data/now-status.json`: one line each in zh and en). One month in
//! two languages; its year and month are fixed when it is made (the file name is the month).

use super::doc::{self, read_json, read_md, Prints};
use super::yaml::Y;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DIR: &str = "src/content/now";
const ORDER: [&str; 4] = ["title", "language", "year", "month"];
const STATUS: &str = "src/data/now-status.json";

fn file(folder: &Path, month: &str, lang: &str) -> PathBuf {
    folder.join(DIR).join(format!("{month}.{lang}.md"))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// YYYY-MM (the file name).
    pub month: String,
    pub langs: Vec<&'static str>,
}

/// Every month, newest first.
pub fn list(folder: &Path) -> Result<Vec<Row>, String> {
    let mut months: Vec<String> = std::fs::read_dir(folder.join(DIR))
        .map_err(|e| format!("读不了 Now 文件夹：{e}"))?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            ["zh", "en"].iter().find_map(|l| n.strip_suffix(&format!(".{l}.md")).map(str::to_string))
        })
        .collect();
    months.sort();
    months.dedup();
    months.reverse();
    Ok(months.into_iter().map(|m| Row { langs: ["zh", "en"].into_iter().filter(|l| file(folder, &m, l).exists()).collect(), month: m }).collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Month {
    pub month: String,
    /// The bodies (None: no file in that language).
    pub zh: Option<String>,
    pub en: Option<String>,
    pub prints: Prints,
}

fn valid(month: &str) -> Result<(), String> {
    if super::wander::valid_month(month) {
        Ok(())
    } else {
        Err("年月要写成 2026-07 这样".into())
    }
}

pub fn load(folder: &Path, month: &str) -> Result<Month, String> {
    valid(month)?;
    let zh = read_md(&file(folder, month, "zh"))?;
    let en = read_md(&file(folder, month, "en"))?;
    if zh.is_none() && en.is_none() {
        return Err("找不到这个月的记录，可能已经删掉了".into());
    }
    Ok(Month { month: month.into(), prints: Prints { zh: zh.as_ref().map(|d| d.print.clone()), en: en.as_ref().map(|d| d.print.clone()) }, zh: zh.map(|d| d.body), en: en.map(|d| d.body) })
}

fn front(old: Option<&Y>, lang: &str, month: &str) -> Y {
    let mut f = old.cloned().unwrap_or(Y::Map(vec![("title".into(), Y::Str(String::new()))]));
    let (y, m) = month.split_once('-').unwrap_or_default();
    doc::set(&mut f, &ORDER, "language", Y::Str(lang.into()));
    doc::set(&mut f, &ORDER, "year", Y::Int(y.parse().unwrap_or(0)));
    doc::set(&mut f, &ORDER, "month", Y::Int(m.parse().unwrap_or(0)));
    f
}

pub fn save(folder: &Path, m: &Month) -> Result<Month, String> {
    valid(&m.month)?;
    let mut olds = vec![];
    for l in ["zh", "en"] {
        let p = file(folder, &m.month, l);
        doc::check_print(&p, m.prints.of(l), if l == "zh" { "中文版" } else { "英文版" })?;
        let given = if l == "zh" { &m.zh } else { &m.en };
        let old = read_md(&p)?;
        if old.is_some() != given.is_some() {
            return Err("这个月的版本和文件对不上，先重新读取".into());
        }
        olds.push((l, p, old, given));
    }
    for (l, p, old, given) in olds {
        let (Some(old), Some(body)) = (old, given) else { continue };
        doc::write_md(&p, Some(&old), &front(Some(&old.front), l, &m.month), &doc::body(body))?;
    }
    load(folder, &m.month)
}

/// A new month in one language (an empty body).
pub fn create(folder: &Path, month: &str, lang: &str) -> Result<Month, String> {
    valid(month)?;
    if !["zh", "en"].contains(&lang) {
        return Err("语言不对".into());
    }
    let p = file(folder, month, lang);
    if p.exists() {
        return Err("这个月已经有这个语言的记录了".into());
    }
    doc::write_md(&p, None, &front(None, lang, month), "")?;
    load(folder, month)
}

pub fn delete(folder: &Path, month: &str, prints: &Prints) -> Result<(), String> {
    valid(month)?;
    for l in ["zh", "en"] {
        let p = file(folder, month, l);
        doc::check_print(&p, prints.of(l), if l == "zh" { "中文版" } else { "英文版" })?;
    }
    for l in ["zh", "en"] {
        let p = file(folder, month, l);
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| format!("删不了：{e}"))?;
        }
    }
    Ok(())
}

// ---------- 当下状态 ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Status {
    pub zh: String,
    pub en: String,
    pub print: Option<String>,
}

pub fn status(folder: &Path) -> Result<Status, String> {
    Ok(match read_json(&folder.join(STATUS))? {
        Some((v, print)) => Status { zh: doc::text(&v, "zh"), en: doc::text(&v, "en"), print: Some(print) },
        None => Status::default(),
    })
}

pub fn save_status(folder: &Path, s: &Status) -> Result<Status, String> {
    let p = folder.join(STATUS);
    doc::check_print(&p, s.print.as_ref(), "当下状态")?;
    let old = read_json(&p)?.map(|(v, _)| v);
    let mut v = old.clone().unwrap_or(Y::Map(vec![]));
    for (k, val) in [("zh", &s.zh), ("en", &s.en)] {
        // the site shows both: kept even when empty
        v.set(k, Y::Str(val.trim().to_string()), &[]);
    }
    doc::write_json(&p, old.as_ref(), &v)?;
    status(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use crate::site::changes::tests::write;

    #[test]
    fn months_and_status() {
        let dir = temp_dir("site-now");
        let s = dir.join("site");
        write(&s, "src/content/now/2026-07.zh.md", "---\ntitle: ''\nlanguage: zh\nyear: 2026\nmonth: 7\n---\n七月\n");
        write(&s, "src/data/now-status.json", "{\n  \"zh\": \"研究生在读\",\n  \"en\": \"Open to Work\"\n}\n");
        let m = create(&s, "2026-10", "zh").unwrap();
        assert_eq!(std::fs::read_to_string(file(&s, "2026-10", "zh")).unwrap(), "---\ntitle: ''\nlanguage: zh\nyear: 2026\nmonth: 10\n---\n");
        assert!(create(&s, "2026-10", "zh").unwrap_err().contains("已经有"));
        assert_eq!(list(&s).unwrap(), vec![Row { month: "2026-10".into(), langs: vec!["zh"] }, Row { month: "2026-07".into(), langs: vec!["zh"] }]);
        let mut m = Month { zh: Some("十月".into()), ..m };
        m = save(&s, &m).unwrap();
        assert_eq!(m.zh.as_deref(), Some("十月\n"));
        // an English version, then both saved; unchanged files stay as they are
        let m = create(&s, "2026-10", "en").unwrap();
        let before = std::fs::read_to_string(file(&s, "2026-07", "zh")).unwrap();
        let m = save(&s, &Month { en: Some("October".into()), ..m }).unwrap();
        assert_eq!(std::fs::read_to_string(file(&s, "2026-10", "en")).unwrap(), "---\ntitle: ''\nlanguage: en\nyear: 2026\nmonth: 10\n---\nOctober\n");
        assert_eq!(std::fs::read_to_string(file(&s, "2026-07", "zh")).unwrap(), before);
        // stale prints refused; delete removes both
        assert!(save(&s, &Month { prints: Prints::default(), ..m.clone() }).is_err());
        delete(&s, "2026-10", &m.prints).unwrap();
        assert!(load(&s, "2026-10").is_err() && create(&s, "2026-13", "zh").is_err());
        // 当下状态
        let st = status(&s).unwrap();
        assert_eq!((st.zh.as_str(), st.en.as_str()), ("研究生在读", "Open to Work"));
        let st = save_status(&s, &Status { zh: "在职".into(), ..st }).unwrap();
        assert_eq!(std::fs::read_to_string(s.join(STATUS)).unwrap(), "{\n  \"zh\": \"在职\",\n  \"en\": \"Open to Work\"\n}\n");
        assert!(save_status(&s, &Status { print: None, ..st }).is_err());
    }
}
