//! 读文件 (v0.4.5 阶段 9, `pomelo_file`): the question's own files — attachments (this and earlier
//! questions on the path) and the project files it uses — read from their stored text, never any other
//! file. Three views: `outline` (each file's segments: 第 3 页 / a heading, how it starts), `read`
//! (segments by number, 1-based as the outline shows them) and `find` (segments containing every word).
//! What it reads is numbered as a source (cite.rs) for the reply to cite. `read_view` / `find_view` /
//! `outline` are shared with 查知识库's whole-file reading (knowledge.rs `browse`).
//! - Long material (`excerpts`): when the question's files don't fit and this skill is offered, the
//!   question goes with their outline and the segments that share the most words with it (rarer words
//!   count more; at most 8, each at least half the best score, cut to about 1500 characters; a file
//!   none of whose segments was picked shows its first one), and 小拾 reads the rest with the tool —
//!   instead of reading everything in segments first. The outline numbers segments 「第 3 段」, never
//!   in brackets, so they aren't taken for citations.

use crate::picky::cite::Sources;
use crate::picky::context;
use crate::picky::data::Answer;
use crate::picky::files::{self, Segment};
use crate::picky::reading::{Kind, Source};
use rusqlite::Connection;
use serde_json::Value;

pub const TOOL: &str = "pomelo_file";
const EXCERPT_CHARS: usize = 1500;
const OUTLINE_START: usize = 24;
/// Long material: at most this many segments go with the question, each scoring at least this share of the best.
const PICK_MAX: usize = 8;
const PICK_SHARE: f64 = 0.5;

/// A file the question carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub artifact: i64,
    pub name: String,
}

/// The question's text files, in order (images have nothing to read).
pub fn materials(sources: &[Source]) -> Vec<Material> {
    let mut out: Vec<Material> = vec![];
    for s in sources.iter().filter(|s| matches!(s.kind, Kind::Attachment | Kind::ProjectFile | Kind::Excerpt) && s.artifact > 0) {
        if !out.iter().any(|m| m.artifact == s.artifact) {
            let name = s.label.split(" · ").next().unwrap_or(&s.label).to_string();
            out.push(Material { artifact: s.artifact, name });
        }
    }
    out
}

fn flat(s: &str, n: usize) -> String {
    let f: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if f.chars().count() > n { format!("{}…", f.chars().take(n).collect::<String>()) } else { f }
}

/// One file's outline: 「第 3 段 · 第 2 页：开头的字…」 per segment (a long one shows its start and end);
/// no brackets, which are for citing.
pub(crate) fn outline(name: &str, segs: &[Segment], lines_max: usize) -> String {
    // how it starts, after its own heading line
    let start = |s: &Segment| {
        let c = s.content.trim_start();
        let body = if c.starts_with('#') { c.split_once('\n').map_or("", |x| x.1) } else { c };
        flat(body, OUTLINE_START)
    };
    let line = |s: &Segment| {
        let (n, label) = (format!("第 {} 段", s.ordinal + 1), s.label());
        if label == n { format!("{n}：{}", start(s)) } else { format!("{n} · {label}：{}", start(s)) }
    };
    let mut out = vec![format!("《{name}》共 {} 段：", segs.len())];
    if segs.len() <= lines_max {
        out.extend(segs.iter().map(line));
    } else {
        let head = lines_max * 2 / 3;
        out.extend(segs[..head].iter().map(line));
        out.push(format!("……（中间还有 {} 段）", segs.len() - head - (lines_max - head)));
        out.extend(segs[segs.len() - (lines_max - head)..].iter().map(line));
    }
    out.join("\n")
}

/// Words of a question to match: pairs of Chinese characters, Latin words of 2+ letters.
fn terms(q: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let chars: Vec<char> = q.to_lowercase().chars().collect();
    let han = |c: char| ('\u{4e00}'..='\u{9fff}').contains(&c);
    for w in chars.windows(2) {
        if han(w[0]) && han(w[1]) {
            out.push(w.iter().collect());
        }
    }
    let mut word = String::new();
    for c in chars.iter().chain(std::iter::once(&' ')) {
        if c.is_ascii_alphanumeric() {
            word.push(*c);
        } else {
            if word.len() >= 2 {
                out.push(std::mem::take(&mut word));
            }
            word.clear();
        }
    }
    out.sort();
    out.dedup();
    out
}

/// How much each word says: rarer across the segments counts more, one in most of them almost nothing.
fn weights(contents: &[String], terms: &[String]) -> Vec<f64> {
    let n = contents.len() as f64;
    terms.iter().map(|t| ((n + 1.0) / (contents.iter().filter(|c| c.contains(t.as_str())).count() as f64 + 0.5)).ln().max(0.0)).collect()
}

fn score(lower: &str, terms: &[String], weights: &[f64]) -> f64 {
    terms.iter().zip(weights).filter(|(t, _)| lower.contains(t.as_str())).map(|(_, w)| w).sum()
}

/// The question with the outline of material too long to send whole.
pub fn long_prompt(question: &str, directory: &str) -> String {
    format!("{question}\n\n以下附件和资料太长，没有全部给你。这是它们的目录（「第几段」是 pomelo_file 读的时候用的段号，不是出处编号）：\n\n{directory}")
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() > n { format!("{}\n……（这一段后面还有，没给）", s.chars().take(n).collect::<String>()) } else { s.to_string() }
}

/// Long material: the outline of each file (text) and the segments picked for the question (as
/// excerpt sources), within about `budget` tokens.
pub fn excerpts(conn: &Connection, question: &str, sources: &[Source], budget: i64) -> Result<(String, Vec<Source>), String> {
    let mats = materials(sources);
    let mut files_segs: Vec<(Material, Vec<Segment>)> = vec![];
    for m in mats {
        let segs = files::segments(conn, m.artifact)?;
        if !segs.is_empty() {
            files_segs.push((m, segs));
        }
    }
    let est = context::estimate;
    let lines_max = ((budget / 4) / (files_segs.len().max(1) as i64 * 20)).clamp(6, 60) as usize;
    let directory = files_segs.iter().map(|(m, segs)| outline(&m.name, segs, lines_max)).collect::<Vec<_>>().join("\n\n");
    let mut left = budget - est(&directory);
    let t = terms(question);
    let lowers: Vec<String> = files_segs.iter().flat_map(|(_, segs)| segs.iter().map(|s| s.content.to_lowercase())).collect();
    let w = weights(&lowers, &t);
    let mut ranked: Vec<(f64, usize, usize)> = vec![]; // (score, file, segment)
    let mut k = 0;
    for (fi, (_, segs)) in files_segs.iter().enumerate() {
        for si in 0..segs.len() {
            ranked.push((score(&lowers[k], &t, &w), fi, si));
            k += 1;
        }
    }
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    // the best few: the rest are in the outline for the tool
    let best = ranked.first().map_or(0.0, |r| r.0);
    let mut picked: Vec<(usize, usize)> = vec![];
    let take = |fi: usize, si: usize, picked: &mut Vec<(usize, usize)>, left: &mut i64| {
        let cost = est(&clip(&files_segs[fi].1[si].content, EXCERPT_CHARS)) + 16;
        if !picked.contains(&(fi, si)) && cost <= *left {
            *left -= cost;
            picked.push((fi, si));
        }
    };
    for (sc, fi, si) in ranked.iter().copied() {
        if sc <= 0.0 || sc < best * PICK_SHARE || picked.len() >= PICK_MAX {
            break;
        }
        take(fi, si, &mut picked, &mut left);
    }
    for fi in 0..files_segs.len() {
        if !picked.iter().any(|p| p.0 == fi) {
            take(fi, 0, &mut picked, &mut left);
        }
    }
    picked.sort();
    let out = picked
        .into_iter()
        .map(|(fi, si)| {
            let (m, segs) = &files_segs[fi];
            let s = &segs[si];
            Source { kind: Kind::Excerpt, artifact: m.artifact, ordinal: s.ordinal, label: format!("{} · {}", m.name, s.label()), content: clip(&s.content, EXCERPT_CHARS), kb: None, earlier: false }
        })
        .collect();
    Ok((directory, out))
}

fn failed(label: &str, why: &str) -> Answer {
    Answer { module: None, label: format!("读文件 · {label}"), state: "failed", text: format!("没读成：{why}"), records: vec![] }
}

/// Runs `pomelo_file`.
pub fn run(conn: &Connection, mats: &[Material], sources: &mut Sources, arguments: &str, budget: i64) -> Answer {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    if mats.is_empty() {
        return failed("没有文件", "这个问题没有带文件");
    }
    let want = args["file"].as_str().unwrap_or("").trim().to_string();
    let chosen: Vec<&Material> = if want.is_empty() {
        mats.iter().collect()
    } else {
        mats.iter().filter(|m| m.name == want || m.name.contains(&want) || want.contains(m.name.as_str())).collect()
    };
    if chosen.is_empty() {
        return failed("没有这个文件", &format!("file 写文件名（有：{}）", mats.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join("、")));
    }
    let budget = budget.min(crate::picky::data::MAX_RESULT * 2);
    let est = context::estimate;
    let segs_of = |m: &Material| files::segments(conn, m.artifact);
    match args["view"].as_str().unwrap_or("outline") {
        "read" => {
            if chosen.len() > 1 {
                return failed("要说是哪个文件", &format!("file 写文件名（有：{}）", chosen.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join("、")));
            }
            let m = chosen[0];
            match segs_of(m) {
                Ok(segs) => read_view("读文件", &m.name, &segs, &args, budget, &mut |seg, label, content| sources.passage(m.artifact, seg.ordinal, label, "附件和资料", content)),
                Err(e) => failed(&m.name, &e),
            }
        }
        "find" => {
            let loaded: Vec<(&Material, Vec<Segment>)> = chosen.iter().filter_map(|m| segs_of(m).ok().map(|s| (*m, s))).collect();
            let files: Vec<(&str, &[Segment])> = loaded.iter().map(|(m, s)| (m.name.as_str(), s.as_slice())).collect();
            find_view("读文件", &files, &args, budget, &mut |i, seg, label, snippet| sources.passage(loaded[i].0.artifact, seg.ordinal, label, "附件和资料", snippet))
        }
        _ => {
            let mut parts = vec![];
            for m in &chosen {
                match segs_of(m) {
                    Ok(segs) => parts.push(outline(&m.name, &segs, 40)),
                    Err(e) => parts.push(format!("《{}》读不了：{e}", m.name)),
                }
            }
            let mut text = parts.join("\n\n");
            if est(&text) > budget {
                text = format!("{}\n……（目录太长，后面没给；指定 file 再看）", text.chars().take((budget * 2) as usize).collect::<String>());
            }
            Answer { module: None, label: format!("读文件 · 目录（{} 个文件）", chosen.len()), state: "ok", text, records: vec![] }
        }
    }
}

/// Gives a segment its source number (its label and the text given), for the reply to cite.
pub(crate) type Cite<'a> = &'a mut dyn FnMut(&Segment, &str, &str) -> i64;

/// `view=read`: segments `from`–`to` (1-based, as the outline shows) of one file, numbered as sources.
pub(crate) fn read_view(what: &str, name: &str, segs: &[Segment], args: &Value, budget: i64, cite: Cite) -> Answer {
    let est = context::estimate;
    let num = |k: &str| args[k].as_u64().or_else(|| args[k].as_str().and_then(|s| s.trim().parse().ok())).map(|n| n as usize);
    let from = num("from").unwrap_or(1).max(1);
    let to = num("to").unwrap_or(from).max(from).min(segs.len());
    if from > segs.len() {
        return Answer { module: None, label: format!("{what} · {name}"), state: "failed", text: format!("没读成：只有 {} 段", segs.len()), records: vec![] };
    }
    let mut lines = vec![];
    let mut used = 0;
    for s in &segs[from - 1..to] {
        let room = (budget - used - 32).max(0) as usize;
        if room < 50 {
            lines.push(format!("……第 {}–{to} 段放不下了，下次少读几段", s.ordinal + 1));
            break;
        }
        let content = clip(&s.content, room.min(6000));
        let label = format!("{name} · {}", s.label());
        let n = cite(s, &label, &content);
        let line = format!("【[{n}] {label}】\n{content}");
        used += est(&line);
        lines.push(line);
    }
    Answer { module: None, label: format!("{what} · 读《{name}》第 {from}{} 段", if to > from { format!("–{to}") } else { String::new() }), state: "ok", text: lines.join("\n\n"), records: vec![] }
}

/// `view=find`: the segments of these files with every word of `keyword`, most hits first, as numbered
/// snippets (`cite` also gets the file's index).
pub(crate) fn find_view(what: &str, files: &[(&str, &[Segment])], args: &Value, budget: i64, cite: &mut dyn FnMut(usize, &Segment, &str, &str) -> i64) -> Answer {
    let est = context::estimate;
    let words: Vec<String> = args["keyword"].as_str().unwrap_or("").split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return failed("没有要找的字", "keyword 写要找的字");
    }
    let mut hits: Vec<(usize, usize, &Segment)> = vec![];
    for (i, (_, segs)) in files.iter().enumerate() {
        for s in segs.iter() {
            let lower = s.content.to_lowercase();
            if words.iter().all(|w| lower.contains(w.as_str())) {
                hits.push((words.iter().map(|w| lower.matches(w.as_str()).count()).sum(), i, s));
            }
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    let total = hits.len();
    let mut lines = vec![if total == 0 { format!("没找到同时有「{}」的段落。", words.join(" ")) } else { format!("找到 {total} 段（按出现次数排）：") }];
    let mut used = est(&lines[0]);
    for (_, i, s) in hits {
        let lower = s.content.to_lowercase();
        let at = lower.find(words[0].as_str()).map_or(0, |b| lower[..b].chars().count());
        let snippet: String = s.content.chars().skip(at.saturating_sub(150)).take(400).collect();
        let label = format!("{} · {}", files[i].0, s.label());
        let body = format!("{label}（第 {} 段）：{}", s.ordinal + 1, flat(&snippet, 400));
        if used + est(&body) + 4 > budget {
            lines.push("……后面的放不下了，可以换更具体的字".into());
            break;
        }
        let line = format!("[{}] {body}", cite(i, s, &label, &snippet));
        used += est(&line);
        lines.push(line);
    }
    let within = if files.len() == 1 { format!("在《{}》里", files[0].0) } else { String::new() };
    Answer { module: None, label: format!("{what} · {within}找「{}」· {total} 段", words.join(" ")), state: "ok", text: lines.join("\n"), records: vec![] }
}

/// The tool definition.
pub fn tool() -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": TOOL,
            "description": "读这个问题带的文件（附件和项目资料）：view=outline 看目录（每段的编号和开头），view=read 按段号读全文（from、to），view=find 按字找段落（keyword，空格隔开的几个字都要有）。资料太长没有全部给你时用它补读。",
            "parameters": { "type": "object", "properties": {
                "view": { "type": "string", "enum": ["outline", "read", "find"] },
                "file": { "type": "string", "description": "文件名（只有一个文件时可以不写）" },
                "from": { "type": "integer", "description": "read：从第几段（1 开始）" },
                "to": { "type": "integer", "description": "read：到第几段（默认同 from）" },
                "keyword": { "type": "string", "description": "find：要找的字" }
            }, "required": ["view"] }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::files::tests::add;
    use crate::picky::store::tests::memory_db;

    #[test]
    fn reading_finding_and_excerpts() {
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("skill-files");
        let md: String = (1..=12).map(|i| format!("# 第{i}节\n{}\n\n", if i == 9 { "押金在退房验收后七天内退还。".to_string() } else { "这一节讲别的事情。".repeat(30) })).collect();
        let a = add(&c, &dir, "租房手册.md", md.as_bytes());
        let label = |s: &Segment| format!("租房手册.md · {}", s.label());
        let segs = files::segments(&c, a).unwrap();
        let src: Vec<Source> = segs.iter().map(|s| Source { kind: Kind::Attachment, artifact: a, ordinal: s.ordinal, label: label(s), content: s.content.clone(), kb: None, earlier: false }).collect();
        let mats = materials(&src);
        assert_eq!(mats, [Material { artifact: a, name: "租房手册.md".into() }]);
        let mut sources = Sources::default();
        let o = run(&c, &mats, &mut sources, r#"{"view":"outline"}"#, 4000);
        assert!(o.text.contains("《租房手册.md》共 12 段") && o.text.contains("第 9 段 · 第9节：押金在退房验收后七天内退还") && !o.text.contains("[9]"), "{}", o.text);
        let f = run(&c, &mats, &mut sources, r#"{"view":"find","keyword":"押金 退还"}"#, 4000);
        assert!(f.text.contains("找到 1 段") && f.text.contains("（第 9 段）"), "{}", f.text);
        let r = run(&c, &mats, &mut sources, r#"{"view":"read","from":9}"#, 4000);
        assert!(r.text.starts_with("【[1] 租房手册.md · 第9节】") && r.text.contains("七天内退还"), "{}", r.text); // found before: same number
        assert_eq!(sources.0.len(), 1);
        assert!(run(&c, &mats, &mut sources, r#"{"view":"read","file":"别的.md","from":1}"#, 4000).state == "failed");
        // long material: the outline and the segment that matches the question first
        let (directory, picked) = excerpts(&c, "押金什么时候退还", &src, 600).unwrap();
        assert!(directory.contains("共 12 段"));
        assert_eq!(picked[0].ordinal, 8);
        assert!(picked.iter().all(|p| p.kind == Kind::Excerpt));
        // words in every segment don't pull them all in
        let (_, picked) = excerpts(&c, "这一节讲的押金什么时候退还", &src, 4000).unwrap();
        assert_eq!(picked.iter().map(|p| p.ordinal).collect::<Vec<_>>(), [8]);
    }
}
