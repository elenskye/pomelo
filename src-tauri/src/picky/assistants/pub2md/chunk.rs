//! Cutting a document into chunks (Pub2MD 切块): the program's draft, what the Supervisor is shown and
//! how its answer is taken.
//! - Draft: blocks in order, a chunk about `target` estimated tokens (never over `max` unless one block
//!   is bigger), cut only where mmd.rs allows, always at a change of part, preferably before a section
//!   heading once the chunk is big enough; a small tail joins the chunk before it (same part).
//! - The Supervisor sees the text (or, when it doesn't fit, the start of each block) with every cut point
//!   marked ⟦c12⟧, where the draft starts each chunk and the parts the program found, and answers with
//!   the chunks it wants (start cut point, part, article, a note for the Worker) and the articles' titles.
//! - Its answer is taken where it can be: starts that aren't cut points are dropped, a chunk over `max`
//!   is cut again by the draft rules, a tiny one joins the one before it; each change is said for the log.

use super::mmd::{cuttable, Block, Kind, Part};
use crate::picky::context::estimate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chunk {
    /// Blocks [start, end).
    pub start: usize,
    pub end: usize,
    pub part: Part,
    /// 1-based: which article of the file.
    pub article: usize,
    /// The Supervisor's words for the Worker.
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    pub target: i64,
    pub max: i64,
}

impl Budget {
    /// About `target` tokens a chunk (a quarter of what the Worker's window holds at most).
    pub fn for_window(window: i64, target: i64) -> Budget {
        let target = target.min(window / 4).max(300);
        Budget { target, max: target * 3 / 2 }
    }
}

fn size(b: &Block) -> i64 {
    estimate(&b.text) + 2
}

pub fn tokens(blocks: &[Block], start: usize, end: usize) -> i64 {
    blocks[start..end].iter().map(size).sum()
}

/// The draft for blocks [from, to).
fn draft_range(blocks: &[Block], parts: &[Part], b: Budget, from: usize, to: usize) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut start = from;
    while start < to {
        let mut size_so_far = 0i64;
        let mut best: Option<(usize, i64)> = None;
        let mut end = to;
        let mut j = start;
        while j < to {
            if j > start && cuttable(blocks, j) {
                if parts[j] != parts[j - 1] {
                    end = j;
                    break;
                }
                let heading = blocks[j].kind == Kind::Heading && blocks[j].level <= 2;
                if heading && size_so_far * 10 >= b.target * 6 {
                    end = j;
                    break;
                }
            }
            let s = size(&blocks[j]);
            if j > start && size_so_far + s > b.target {
                if cuttable(blocks, j) && size_so_far * 3 >= b.target {
                    end = j;
                    break;
                }
                if let Some((at, _)) = best.filter(|(_, sz)| sz * 3 >= b.target) {
                    end = at;
                    break;
                }
                if size_so_far + s > b.max {
                    if cuttable(blocks, j) {
                        end = j;
                        break;
                    }
                    if let Some((at, _)) = best {
                        end = at;
                        break;
                    }
                }
            }
            if j > start && cuttable(blocks, j) {
                best = Some((j, size_so_far));
            }
            size_so_far += s;
            j += 1;
        }
        out.push((start, end));
        start = end;
    }
    // a small tail joins the chunk before it (same part, still within max)
    let mut merged: Vec<(usize, usize)> = vec![];
    for (s, e) in out {
        if let Some(last) = merged.last_mut() {
            let small = tokens(blocks, s, e) * 4 < b.target;
            if small && parts[s] == parts[last.0] && tokens(blocks, last.0, e) <= b.max {
                last.1 = e;
                continue;
            }
        }
        merged.push((s, e));
    }
    merged
}

pub fn draft(blocks: &[Block], parts: &[Part], b: Budget) -> Vec<Chunk> {
    draft_range(blocks, parts, b, 0, blocks.len()).into_iter().map(|(start, end)| Chunk { start, end, part: parts[start], article: 1, note: String::new() }).collect()
}

/// The pages a chunk covers.
pub fn pages(blocks: &[Block], c: &Chunk) -> (usize, usize) {
    (blocks[c.start].page, blocks[c.end - 1].end_page)
}

/// The pages whose images go with a chunk: a first or last page it has under 300 characters on (a
/// running header, a heading's line) is left out, its image showing another chunk's text (The Economist:
/// the Worker copied the next article from it).
pub fn image_pages(blocks: &[Block], c: &Chunk) -> (usize, usize) {
    let (mut a, mut z) = pages(blocks, c);
    let on = |p: usize| blocks[c.start..c.end].iter().filter(|b| b.page <= p && p <= b.end_page).map(|b| b.text.chars().count()).sum::<usize>();
    while a < z && on(a) < 300 {
        a += 1;
    }
    while z > a && on(z) < 300 {
        z -= 1;
    }
    (a, z)
}

/// A page's position line in the Worker's text (removed from its answer).
pub fn page_line(n: usize) -> String {
    format!("【原文第 {n} 页】")
}

pub fn is_page_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("【原文第 ") && t.ends_with(" 页】")
}

/// The chunk's source, with a position line where each page starts (`markers`).
pub fn source_text(blocks: &[Block], c: &Chunk, markers: bool) -> String {
    let mut out = String::new();
    let mut page = 0;
    for b in &blocks[c.start..c.end] {
        if markers && b.page != page {
            out.push_str(&page_line(b.page));
            out.push_str("\n\n");
        }
        page = b.end_page;
        out.push_str(&b.text);
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

// ---------- the Supervisor ----------

/// What the Supervisor is shown: every cut point ⟦cN⟧, the draft's chunk starts, the parts found, pages;
/// whole blocks when `full`, else each block's start.
pub fn outline(blocks: &[Block], parts: &[Part], draft: &[Chunk], full: bool) -> String {
    let mut out = String::new();
    let mut page = 0;
    let mut part: Option<Part> = None;
    for (i, b) in blocks.iter().enumerate() {
        if b.page != page {
            out.push_str(&format!("{}\n", page_line(b.page)));
            page = b.end_page;
        }
        if let Some(k) = draft.iter().position(|c| c.start == i) {
            out.push_str(&format!("⟦草案第 {} 块从这里开始⟧\n", k + 1));
        }
        if part != Some(parts[i]) {
            out.push_str(&format!("【程序认出：{}（{}）】\n", parts[i].name(), parts[i].code()));
            part = Some(parts[i]);
        }
        if cuttable(blocks, i) {
            out.push_str(&format!("⟦c{i}⟧\n"));
        }
        if full || b.kind == Kind::Heading || b.kind == Kind::Title {
            out.push_str(&b.text);
        } else {
            let head: String = b.text.chars().take(120).collect();
            let more = b.text.chars().count() > 120;
            out.push_str(&format!("{head}{}（约 {} 词元）", if more { "……" } else { "" }, size(b)));
        }
        out.push_str("\n\n");
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanChunk {
    pub start: String,
    pub part: Option<String>,
    pub article: Option<usize>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Article {
    pub article: usize,
    pub title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Plan {
    pub chunks: Vec<PlanChunk>,
    pub articles: Vec<Article>,
}

/// The JSON object in a reply (fences and words around it allowed).
pub fn json_in(reply: &str) -> Option<serde_json::Value> {
    let (a, b) = (reply.find('{')?, reply.rfind('}')?);
    let body = reply.get(a..=b)?;
    serde_json::from_str(body).ok().or_else(|| serde_json::from_str(&repair_quotes(body)).ok())
}

/// Quotes a model forgot to escape inside a string (`"target":""王者之剑"行动"`): a quote with no
/// , : } ] after it doesn't end the string, it's part of it.
fn repair_quotes(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 16);
    let (mut in_str, mut escaped) = (false, false);
    for (i, &c) in chars.iter().enumerate() {
        if in_str && escaped {
            escaped = false;
        } else if in_str && c == '\\' {
            escaped = true;
        } else if c == '"' && !in_str {
            in_str = true;
        } else if c == '"' {
            if chars[i + 1..].iter().find(|c| !c.is_whitespace()).is_none_or(|n| ",:}]".contains(*n)) {
                in_str = false;
            } else {
                out.push('\\');
            }
        }
        out.push(c);
    }
    out
}

/// Every complete JSON object in a reply, inner ones too (what a cut-off reply still has).
pub fn json_objects(reply: &str) -> Vec<serde_json::Value> {
    let reply = &repair_quotes(reply);
    let (mut opens, mut out, mut in_str, mut escaped) = (vec![], vec![], false, false);
    for (i, ch) in reply.char_indices() {
        match ch {
            _ if escaped => escaped = false,
            '\\' if in_str => escaped = true,
            '"' => in_str = !in_str,
            '{' if !in_str => opens.push(i),
            '}' if !in_str => {
                if let Some(a) = opens.pop() {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&reply[a..=i]) {
                        out.push(v);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The Supervisor's chunks, taken where they can be. Returns them and what wasn't taken (for the log).
pub fn apply(blocks: &[Block], parts: &[Part], plan: &Plan, b: Budget) -> (Vec<Chunk>, Vec<String>) {
    let mut said = vec![];
    let n = blocks.len();
    let mut starts: Vec<(usize, PlanChunk)> = vec![];
    for pc in &plan.chunks {
        let at = pc.start.trim().trim_start_matches(['c', 'C']).parse::<usize>().ok();
        match at {
            Some(i) if i < n && cuttable(blocks, i) && starts.last().is_none_or(|(l, _)| *l < i) => starts.push((i, pc.clone())),
            _ => said.push(format!("没用 Supervisor 的切点「{}」（不是能切的地方或顺序不对）", pc.start)),
        }
    }
    if starts.first().is_none_or(|(i, _)| *i != 0) {
        starts.insert(0, (0, PlanChunk { start: "c0".into(), ..Default::default() }));
    }
    // an article listed with no chunk of its own (Qwen put two articles in one chunk): it starts at its title
    let flat = |t: &str| t.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    for a in plan.articles.iter().filter(|a| a.article > 1 && !plan.chunks.iter().any(|c| c.article == Some(a.article))) {
        let title = flat(&a.title);
        let after = starts.iter().filter(|(_, c)| c.article.is_some_and(|x| x < a.article)).map(|(i, _)| *i).max().unwrap_or(0);
        let at = (after + 1..n).find(|&i| matches!(blocks[i].kind, Kind::Title | Kind::Heading) && title.len() >= 4 && flat(&blocks[i].text).contains(&title) && cuttable(blocks, i));
        if let Some(i) = at.filter(|i| !starts.iter().any(|(s, _)| s == i)) {
            let k = starts.iter().position(|(s, _)| *s > i).unwrap_or(starts.len());
            let note = starts[k - 1].1.note.clone();
            starts.insert(k, (i, PlanChunk { start: format!("c{i}"), article: Some(a.article), note, ..Default::default() }));
            said.push(format!("第 {} 篇「{}」没有自己的一块，从它的标题（c{i}）切开", a.article, a.title));
        }
    }
    let mut chunks: Vec<Chunk> = vec![];
    let mut article = 1usize;
    for (k, (start, pc)) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(n, |(s, _)| *s);
        let part = pc.part.as_deref().and_then(Part::from_code).unwrap_or(parts[*start]);
        if let Some(a) = pc.article.filter(|a| *a >= article && *a <= article + 1) {
            article = a;
        }
        chunks.push(Chunk { start: *start, end, part, article, note: pc.note.clone().unwrap_or_default().trim().chars().take(500).collect() });
    }
    // sizes: an oversized chunk is cut again, a tiny one joins the one before it
    let mut sized: Vec<Chunk> = vec![];
    for c in chunks {
        let t = tokens(blocks, c.start, c.end);
        if t > b.max && c.end - c.start > 1 {
            let pieces = draft_range(blocks, &vec![c.part; n], b, c.start, c.end);
            if pieces.len() > 1 {
                said.push(format!("Supervisor 的一块（c{}）太大（约 {t} 词元），程序又切成了 {} 块", c.start, pieces.len()));
            }
            for (s, e) in pieces {
                sized.push(Chunk { start: s, end: e, ..c.clone() });
            }
            continue;
        }
        if let Some(last) = sized.last_mut() {
            if t * 5 < b.target && last.article == c.article && last.part == c.part && tokens(blocks, last.start, c.end) <= b.max {
                said.push(format!("Supervisor 的一块（c{}）太小，并进了前一块", c.start));
                last.end = c.end;
                if !c.note.is_empty() {
                    last.note = format!("{} {}", last.note, c.note).trim().to_string();
                }
                continue;
            }
        }
        sized.push(c);
    }
    (sized, said)
}

#[cfg(test)]
mod tests {
    use super::super::mmd::{blocks, parts, tests::PAPER};
    use super::*;

    fn long_doc() -> String {
        let mut s = String::from("\\title{T}\n\n\\begin{abstract}\nShort abstract.\n\\end{abstract}\n\n");
        for sec in 1..=4 {
            s.push_str(&format!("\\section*{{{sec} Section}}\n\n"));
            for p in 0..6 {
                s.push_str(&format!("Paragraph {sec}.{p} {}.\n\n", "word ".repeat(80).trim()));
                if p == 2 {
                    s.push_str("\\pagebreak\n\n");
                }
            }
        }
        s.push_str("\\section*{References}\n\n[1] A.\n\n[2] B.\n");
        s
    }

    #[test]
    fn draft_respects_budget_parts_and_cut_points() {
        let text = long_doc();
        let (b, _) = blocks(&text);
        let p = parts(&b);
        let budget = Budget { target: 600, max: 900 };
        let d = draft(&b, &p, budget);
        // chunks cover everything in order, start at cut points, keep one part each
        assert_eq!(d.first().unwrap().start, 0);
        assert_eq!(d.last().unwrap().end, b.len());
        for w in d.windows(2) {
            assert_eq!(w[0].end, w[1].start);
            assert!(cuttable(&b, w[1].start));
        }
        // the title can't be cut from the abstract under it; every other chunk is in one part
        for c in &d[1..] {
            assert!(p[c.start..c.end].iter().all(|x| *x == c.part), "{c:?}");
        }
        for c in &d {
            assert!(tokens(&b, c.start, c.end) <= budget.max || c.end - c.start == 1);
        }
        assert_eq!(d.last().unwrap().part, Part::References);
        // the front (title + abstract) is its own small piece? it joins nothing of another part
        assert!(d.iter().any(|c| c.part == Part::Front));
        assert!(d.len() >= 4, "{}", d.len());
        let src = source_text(&b, &d[1], true);
        assert!(src.starts_with("【原文第 1 页】") && is_page_line("【原文第 12 页】"));
    }

    #[test]
    fn a_page_the_chunk_only_touches_is_not_shown() {
        let body = "Body text of the article that fills the page. ".repeat(10);
        let (b, _) = blocks(&format!("{body}\n\n{body}\n\n\\pagebreak\n\nThe Economist September 5th 2026 Leaders 7\n\n\\pagebreak\n\nNext."));
        let c = Chunk { start: 0, end: 3, ..Default::default() };
        assert_eq!((pages(&b, &c), image_pages(&b, &c)), ((1, 2), (1, 1)));
        // a few words going on to the next page don't bring it; a page of its text does
        let all = |b: &[Block]| Chunk { start: 0, end: b.len(), ..Default::default() };
        let (b, _) = blocks(&format!("{body}\n\n{body}\n\n\\pagebreak\n\nand it goes on."));
        assert_eq!((pages(&b, &all(&b)), image_pages(&b, &all(&b))), ((1, 2), (1, 1)));
        let (b, _) = blocks(&format!("{body}\n\n\\pagebreak\n\n{body}"));
        assert_eq!(image_pages(&b, &all(&b)), (1, 2));
    }

    #[test]
    fn quotes_left_unescaped_are_repaired() {
        let reply = "{\"terms\":[\n{\"source\":\"Operation Excalibur\",\"target\":\"\"王者之剑\"行动\",\"note\":\"\"},\n{\"source\":\"a \\\"b\\\"\",\"target\":\"乙\"}\n]}";
        let v = json_in(reply).unwrap();
        assert_eq!((v["terms"][0]["target"].as_str(), v["terms"][1]["source"].as_str()), (Some("\"王者之剑\"行动"), Some("a \"b\"")));
        assert_eq!(json_objects(&reply[..reply.len() - 30]).len(), 1);
    }

    #[test]
    fn supervisor_plan_is_taken_where_it_can_be() {
        let (b, _) = blocks(PAPER);
        let p = parts(&b);
        let budget = Budget { target: 5000, max: 7500 };
        let d = draft(&b, &p, budget);
        let o = outline(&b, &p, &d, true);
        assert!(o.contains("⟦c9⟧") && !o.contains("⟦c4⟧") && o.contains("⟦草案第 1 块从这里开始⟧") && o.contains("【程序认出：参考文献（references）】"));
        let short = outline(&b, &p, &d, false);
        assert!(short.contains("词元") && short.len() < o.len() + 400);
        let plan: Plan = serde_json::from_value(json_in("好的：\n```json\n{\"chunks\":[{\"start\":\"c0\",\"part\":\"front\",\"note\":\"标题和作者\"},{\"start\":\"c4\"},{\"start\":\"c9\",\"note\":\"模型结构\"},{\"start\":\"c3\"},{\"start\":\"c15\",\"part\":\"references\"}],\"articles\":[{\"article\":1,\"title\":\"Attention\"}]}\n```").unwrap()).unwrap();
        let (chunks, said) = apply(&b, &p, &plan, budget);
        assert_eq!(chunks.iter().map(|c| c.start).collect::<Vec<_>>(), [0, 9, 15]);
        assert_eq!((chunks[0].note.as_str(), chunks[0].part, chunks[2].part), ("标题和作者", Part::Front, Part::References));
        assert_eq!(said.len(), 2, "{said:?}"); // c4 (after a heading) and c3 (out of order)
        assert_eq!(plan.articles[0].title, "Attention");
        // no plan at all: one chunk from the start
        let (all, _) = apply(&b, &p, &Plan::default(), budget);
        assert_eq!((all.len(), all[0].start, all[0].end), (1, 0, b.len()));
        // an oversized chunk is cut again
        let (small, said) = apply(&b, &p, &Plan::default(), Budget { target: 60, max: 90 });
        assert!(small.len() > 3 && said.iter().any(|s| s.contains("太大")));
        // two articles in one chunk: the second starts at its title
        let mag = "\\section*{The world this week Business}\n\nBonds fell.\n\nShein listed.\n\n\\title{\nThe sorcerer of silicon\n}\n\nNVIDIA WAS named in 1993.";
        let (b2, _) = blocks(mag);
        let two: Plan = serde_json::from_value(serde_json::json!({ "chunks": [{ "start": "c0", "article": 1, "note": "两篇" }], "articles": [{ "article": 1, "title": "The world this week Business" }, { "article": 2, "title": "The sorcerer of silicon" }] })).unwrap();
        let (chunks, said) = apply(&b2, &parts(&b2), &two, budget);
        assert_eq!(chunks.iter().map(|c| (c.start, c.article)).collect::<Vec<_>>(), [(0, 1), (3, 2)], "{said:?}");
        assert!(said[0].contains("第 2 篇「The sorcerer of silicon」没有自己的一块"));
    }
}
