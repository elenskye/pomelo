//! What every content editor shares: a Markdown file (front matter + body) or a JSON data file, read
//! with its fingerprint (written back only if still the same), keys kept in place and new ones put
//! where the schema has them. JSON is written as the site has it: `JSON.stringify(data, null, 2)`
//! plus a newline (keys in their order; read with yaml-rust2, JSON being YAML).

use super::yaml::{self, Y};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::path::Path;

pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The fingerprint of a file now (None: not there).
pub fn print_of(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|b| fingerprint(&b))
}

/// The file is still what was read (`what` names it in the refusal).
pub fn check_print(path: &Path, read: Option<&String>, what: &str) -> Result<(), String> {
    if print_of(path).as_ref() != read {
        return Err(format!("{what}在别处被改过了（编辑器或 git），先重新读取再改"));
    }
    Ok(())
}

/// The zh / en files of something in two languages, as read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Prints {
    pub zh: Option<String>,
    pub en: Option<String>,
}

impl Prints {
    pub fn of(&self, lang: &str) -> Option<&String> {
        if lang == "zh" {
            self.zh.as_ref()
        } else {
            self.en.as_ref()
        }
    }
}

pub struct Doc {
    pub front: Y,
    pub body: String,
    pub print: String,
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn read_md(path: &Path) -> Result<Option<Doc>, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("读不了 {}：{e}", path.display())),
    };
    let name = name_of(path);
    let text = String::from_utf8(bytes.clone()).map_err(|_| format!("{name} 不是 UTF-8 文字"))?;
    let (front, body) = yaml::split_front(&text).ok_or_else(|| format!("{name} 开头没有 --- 字段"))?;
    let front = yaml::parse(front).map_err(|e| format!("{name}：{e}"))?;
    if !matches!(front, Y::Map(_)) {
        return Err(format!("{name} 开头的字段不是一组键值"));
    }
    Ok(Some(Doc { front, body: body.to_string(), print: fingerprint(&bytes) }))
}

/// A JSON object file and its fingerprint (None: not there).
pub fn read_json(path: &Path) -> Result<Option<(Y, String)>, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("读不了 {}：{e}", path.display())),
    };
    let name = name_of(path);
    let text = String::from_utf8(bytes.clone()).map_err(|_| format!("{name} 不是 UTF-8 文字"))?;
    let v = yaml::parse(&text).map_err(|_| format!("{name} 不是正确的 JSON"))?;
    if !matches!(v, Y::Map(_)) {
        return Err(format!("{name} 不是一组键值"));
    }
    Ok(Some((v, fingerprint(&bytes))))
}

/// Writes a Markdown file unless its fields and body are what's there.
pub fn write_md(path: &Path, old: Option<&Doc>, front: &Y, body: &str) -> Result<(), String> {
    if old.is_some_and(|d| &d.front == front && d.body.trim_end() == body.trim_end()) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, yaml::join_front(front, body)).map_err(|e| format!("写不了 {}：{e}", name_of(path)))
}

/// Writes a JSON file unless the data is what's there.
pub fn write_json(path: &Path, old: Option<&Y>, v: &Y) -> Result<(), String> {
    if old == Some(v) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, json(v) + "\n").map_err(|e| format!("写不了 {}：{e}", name_of(path)))
}

/// A body as stored: a newline at the end, nothing when empty.
pub fn body(text: &str) -> String {
    if text.trim().is_empty() {
        String::new()
    } else if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    }
}

/// The schema keys after `key` (a new key goes before the first of them present).
pub fn after<'a>(order: &'a [&'a str], key: &str) -> &'a [&'a str] {
    let i = order.iter().position(|k| *k == key).map_or(order.len(), |i| i + 1);
    &order[i..]
}

/// Text in the site's format: left out when empty.
pub fn set_text(front: &mut Y, order: &[&str], key: &str, v: &str) {
    if v.is_empty() {
        front.remove(key);
    } else {
        front.set(key, Y::Str(v.to_string()), after(order, key));
    }
}

pub fn set(front: &mut Y, order: &[&str], key: &str, v: Y) {
    front.set(key, v, after(order, key));
}

pub fn text(v: &Y, key: &str) -> String {
    v.get(key).and_then(Y::text).unwrap_or_default()
}

pub fn strings(v: &Y, key: &str) -> Vec<String> {
    v.get(key).map(|s| s.seq().iter().filter_map(Y::text).collect()).unwrap_or_default()
}

/// A map from (key, value) pairs, empty text left out (the site's text fields).
pub fn map(pairs: &[(&str, &str)]) -> Y {
    Y::Map(pairs.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.to_string(), Y::Str(v.to_string()))).collect())
}

pub fn strs(list: &[String]) -> Y {
    Y::Seq(list.iter().map(|s| Y::Str(s.clone())).collect())
}

// ---------- JSON.stringify(v, null, 2) ----------

pub fn json(v: &Y) -> String {
    let mut out = String::new();
    write_json_value(&mut out, v, 0);
    out
}

fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_json_value(out: &mut String, v: &Y, level: usize) {
    let pad = |n: usize| "  ".repeat(n);
    match v {
        Y::Null => out.push_str("null"),
        Y::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Y::Int(i) => out.push_str(&i.to_string()),
        Y::Float(f) if f.is_finite() => out.push_str(&if f.fract() == 0.0 && f.abs() < 1e21 { format!("{f:.0}") } else { format!("{f}") }),
        Y::Float(_) => out.push_str("null"),
        Y::Str(s) => json_str(out, s),
        Y::Seq(items) if items.is_empty() => out.push_str("[]"),
        Y::Seq(items) => {
            out.push_str("[\n");
            for (i, it) in items.iter().enumerate() {
                out.push_str(&pad(level + 1));
                write_json_value(out, it, level + 1);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(level));
            out.push(']');
        }
        Y::Map(m) if m.is_empty() => out.push_str("{}"),
        Y::Map(m) => {
            out.push_str("{\n");
            for (i, (k, it)) in m.iter().enumerate() {
                out.push_str(&pad(level + 1));
                json_str(out, k);
                out.push_str(": ");
                write_json_value(out, it, level + 1);
                out.push_str(if i + 1 < m.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(level));
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;

    #[test]
    fn json_like_javascript() {
        let v = yaml::parse(r#"{"title": "wander-big-fish", "asset": {"discriminant": "existingImage", "value": {"file": "public/a \"b\".jpg", "alt": ""}}, "tags": [], "obj": {}, "n": 2, "ok": false, "list": [1, "x\ny"]}"#).unwrap();
        assert_eq!(
            json(&v),
            "{\n  \"title\": \"wander-big-fish\",\n  \"asset\": {\n    \"discriminant\": \"existingImage\",\n    \"value\": {\n      \"file\": \"public/a \\\"b\\\".jpg\",\n      \"alt\": \"\"\n    }\n  },\n  \"tags\": [],\n  \"obj\": {},\n  \"n\": 2,\n  \"ok\": false,\n  \"list\": [\n    1,\n    \"x\\ny\"\n  ]\n}"
        );
        // CJK as is, control characters escaped
        assert_eq!(json(&Y::Str("研究生\u{1}".into())), "\"研究生\\u0001\"");
    }

    #[test]
    fn writes_only_changes_and_checks_prints() {
        let dir = temp_dir("site-doc");
        let p = dir.join("a.json");
        std::fs::write(&p, "{ \"zh\": \"一\",  \"en\": \"one\" }").unwrap();
        let (v, print) = read_json(&p).unwrap().unwrap();
        write_json(&p, Some(&v), &v).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ \"zh\": \"一\",  \"en\": \"one\" }");
        check_print(&p, Some(&print), "状态").unwrap();
        let mut w = v.clone();
        w.set("zh", Y::Str("二".into()), &[]);
        write_json(&p, Some(&v), &w).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\n  \"zh\": \"二\",\n  \"en\": \"one\"\n}\n");
        assert!(check_print(&p, Some(&print), "状态").unwrap_err().contains("状态在别处被改过了"));
        assert!(check_print(&dir.join("none.json"), None, "x").is_ok());
        assert_eq!((body("x"), body("x\n"), body(" \n")), ("x\n".into(), "x\n".into(), String::new()));
    }
}
