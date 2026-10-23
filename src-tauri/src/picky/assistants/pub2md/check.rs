//! The program's checks on a chunk's result (no model, no images): it compares text with text.
//! - cut off: the service said the reply hit its length, or an environment / display math isn't closed;
//! - much shorter (under 60%) or longer (over 150%) than the source, for chunks long enough to tell;
//! - fewer display equations, tables, images or footnotes than the source; two headings fewer; far
//!   fewer inline formulas;
//! - another language (the share of CJK characters moved by more than 30 points).
//! Seeing whether the content matches the page is the Worker's and the Supervisor's job.
//! A translation (`check_translation`) keeps the same structure (formulas, tables, images, footnotes,
//! headings, citation numbers), is of a sensible length in tokens, is Chinese, and loses no paragraph
//! (as many paragraphs, none much shorter than its source: a dropped sentence shows); a 简繁 pass
//! (`check_script`) keeps structure and length.

use super::mmd::{self, counts, Kind};

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub code: &'static str,
    pub says: String,
}

fn issue(code: &'static str, says: String) -> Issue {
    Issue { code, says }
}

/// Closed `\begin` / `\end`, `\[` / `\]` and `$$`.
fn unclosed(text: &str) -> bool {
    text.matches("\\begin{").count() != text.matches("\\end{").count() || text.matches("\\[").count() != text.matches("\\]").count() || text.matches("$$").count() % 2 == 1
}

/// What's wrong with `output` as the cleaned `source` (empty = passes). `cut`: the reply hit its length.
pub fn check(source: &str, output: &str, cut: bool) -> Vec<Issue> {
    let mut out = vec![];
    if output.trim().is_empty() {
        out.push(issue("empty", "结果是空的".into()));
        return out;
    }
    if cut {
        out.push(issue("cut", "回复写到了长度上限，被截断了".into()));
    } else if unclosed(output) && !unclosed(source) {
        out.push(issue("unclosed", "有公式或表格没写完（可能被截断了）".into()));
    }
    out.extend(echoed(source, output));
    let (s, o) = (counts(source), counts(output));
    if s.chars >= 300 {
        let ratio = o.chars as f64 / s.chars as f64;
        if ratio < 0.6 {
            out.push(issue("short", format!("比原文短很多（原文 {} 字，结果 {} 字）", s.chars, o.chars)));
        } else if ratio > 1.5 {
            out.push(issue("long", format!("比原文长很多（原文 {} 字，结果 {} 字，可能重复了）", s.chars, o.chars)));
        }
    }
    let fewer = |what: &str, a: usize, b: usize| (b < a).then(|| format!("{what}少了（原文 {a} 个，结果 {b} 个）"));
    for (code, what, a, b) in [("math", "独立公式", s.display_math, o.display_math), ("tables", "表格", s.tables, o.tables), ("images", "图", s.images, o.images), ("footnotes", "脚注", s.footnotes, o.footnotes)] {
        if let Some(says) = fewer(what, a, b) {
            out.push(issue(code, says));
        }
    }
    if o.headings + 2 <= s.headings {
        out.push(issue("headings", format!("标题少了（原文 {} 个，结果 {} 个）", s.headings, o.headings)));
    } else if sections(output) < sections(source) {
        out.push(issue("headings", format!("章节标题少了（原文 {} 个，结果 {} 个）", sections(source), sections(output))));
    }
    if s.inline_math >= 5 && (o.inline_math as f64) < s.inline_math as f64 * 0.8 - 2.0 {
        out.push(issue("inline", format!("行内公式少了很多（原文 {} 个，结果 {} 个）", s.inline_math, o.inline_math)));
    }
    let gone = dropped(source, output);
    if gone.len() >= 2 {
        let opening: Vec<&str> = gone[0].split_whitespace().take(6).collect();
        out.push(issue("dropped", format!("漏了段落（从「{}…」起连着 {} 段）", opening.join(" "), gone.len())));
    }
    if let Some(h) = foreign_headings(source, output).first() {
        out.push(issue("added", format!("多了原文里没有的标题「{h}」（可能照原页图抄了别的块）")));
    }
    let extra = dropped(output, source);
    if extra.len() >= 2 {
        let opening: Vec<&str> = extra[0].split_whitespace().take(6).collect();
        out.push(issue("added", format!("多了原文里没有的段落（从「{}…」起连着 {} 段，可能照原页图抄了别的块）", opening.join(" "), extra.len())));
    }
    if s.chars >= 100 && o.chars > 0 {
        let share = |c: super::mmd::Counts| c.wide as f64 / c.chars as f64;
        if (share(s) - share(o)).abs() > 0.3 {
            out.push(issue("language", "语言和原文不一样了".into()));
        }
    }
    out
}

/// Top-level headings (\section, #): one lost is a lost part of the document.
fn sections(text: &str) -> usize {
    text.lines().filter(|l| l.trim_start().starts_with("\\section") || l.starts_with("# ")).count()
}

/// MMD's headings written as Markdown ones (Haiku: \section → #, \subsection → ##): the document mixes the
/// two. Only for MMD out (Markdown out turns them into # on purpose).
pub fn heading_style(source: &str, output: &str) -> Option<Issue> {
    let latex = |t: &str| t.lines().filter(|l| ["\\section", "\\subsection", "\\subsubsection"].iter().any(|h| l.trim_start().starts_with(h))).count();
    let md = |t: &str| t.lines().filter(|l| l.starts_with('#') && l.trim_start_matches('#').starts_with(' ')).count();
    (latex(output) < latex(source) && md(output) > md(source)).then(|| issue("heading_style", format!("标题的写法改了（原文 {} 个 \\section 这类标题，结果只有 {} 个，其余写成了 #）", latex(source), latex(output))))
}

/// The longest run of `a`'s paragraphs whose openings aren't in `b`: the cleaned text lost them (Haiku
/// dropped a heading and four paragraphs, under every other check), or, the other way, has passages the
/// source hasn't (copied off a page image from the next chunk). The opening: the first 24 letters, digits or CJK
/// characters, lowercased (fixed OCR rarely sits there); short ones (headers, captions) don't count.
fn dropped(a: &str, b: &str) -> Vec<String> {
    let (source, output) = (a, b);
    // \begin{itemize} lines left out: where a list starts is the cleaning's to tidy
    let key = |t: &str| {
        let t: Vec<&str> = t.lines().filter(|l| !["\\begin{", "\\end{"].iter().any(|e| l.trim_start().starts_with(e))).collect();
        prose(&t.join("\n")).chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>()
    };
    let all = key(output);
    let (mut run, mut longest): (Vec<String>, Vec<String>) = (vec![], vec![]);
    for p in paragraphs(source) {
        let k = key(&p);
        if k.chars().count() < 60 {
            continue;
        }
        // its opening, or the opening after a line or two (a page number and running head Mathpix put on top)
        let lines: Vec<&str> = p.lines().collect();
        let found = (0..lines.len().min(3)).any(|skip| {
            let k = key(&lines[skip..].join("\n"));
            k.chars().count() >= 24 && all.contains(&k.chars().take(24).collect::<String>())
        });
        if found {
            if run.len() > longest.len() {
                longest = std::mem::take(&mut run);
            }
            run.clear();
        } else {
            run.push(p);
        }
    }
    if run.len() > longest.len() { run } else { longest }
}

/// A heading's words, lowercased (letters, digits, CJK only).
fn heading_key(line: &str) -> Option<String> {
    let t = line.trim_start();
    let inner = if let Some(r) = ["\\section", "\\subsection", "\\subsubsection"].iter().find_map(|h| t.strip_prefix(h)) {
        r.split_once('{')?.1.split('}').next()?
    } else if t.starts_with('#') && t.trim_start_matches('#').starts_with(' ') {
        t.trim_start_matches('#')
    } else {
        return None;
    };
    Some(prose(inner).chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect())
}

/// Headings in the cleaned text whose words the source hasn't anywhere (a heading made from its own bold
/// lead-in is fine): copied off a page image from the next chunk.
fn foreign_headings(source: &str, output: &str) -> Vec<String> {
    let all: String = prose(source).chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    output.lines().filter(|l| heading_key(l).is_some_and(|k| !k.is_empty() && !all.contains(&k))).map(|l| l.trim().chars().take(60).collect()).collect()
}

/// The cleaned text without what it copied off a page image from the chunk before or after (DeepSeek did in
/// 5 of Pargent's 27 chunks): the blocks at its start or end the source hasn't (a paragraph's opening, a
/// short line's words, an image's link), when they hold at least two long paragraphs. The text kept and how
/// many blocks went; None when there's nothing to take off. A heading the source hasn't (Economic Roots'
/// next 「Supplementary materials」 and 「Notes」, short ones) counts like the two paragraphs.
pub fn trim_copied(source: &str, output: &str) -> Option<(String, usize)> {
    let key = |t: &str| prose(t).chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let all = key(source);
    // Some(long paragraph?) when the block isn't the source's; None when it is (or can't tell: math, tables)
    let foreign = |b: &mmd::Block| -> Option<bool> {
        match b.kind {
            Kind::Heading if b.text.lines().next().and_then(heading_key).is_some_and(|k| !k.is_empty() && !all.contains(&k)) => Some(true),
            Kind::Paragraph | Kind::Heading | Kind::Title | Kind::Caption | Kind::Footnote => {
                let k = key(&b.text);
                if k.chars().count() >= 60 {
                    (!all.contains(&k.chars().take(24).collect::<String>())).then_some(true)
                } else {
                    (!k.is_empty() && !all.contains(&k)).then_some(false)
                }
            }
            Kind::Figure => {
                let links: Vec<&str> = b.text.split(['(', ')', '{', '}']).filter(|t| t.starts_with("assets/") || t.starts_with("http")).collect();
                (!links.is_empty() && links.iter().all(|l| !source.contains(l))).then_some(false)
            }
            _ => None,
        }
    };
    let blocks = mmd::blocks(output).0;
    let (mut a, mut z, mut long) = (0, blocks.len(), (0, 0));
    while z > a {
        match foreign(&blocks[z - 1]) {
            Some(l) => (z, long.1) = (z - 1, long.1 + l as usize),
            None => break,
        }
    }
    while a < z {
        match foreign(&blocks[a]) {
            Some(l) => (a, long.0) = (a + 1, long.0 + l as usize),
            None => break,
        }
    }
    let (a, z) = (if long.0 >= 2 { a } else { 0 }, if long.1 >= 2 { z } else { blocks.len() });
    if (a, z) == (0, blocks.len()) || a >= z {
        return None;
    }
    // cut the reply itself (its own line breaks kept) from the first block kept to the end of the last
    let from = output.find(blocks[a].text.as_str())?;
    let last = blocks[z - 1].text.as_str();
    let to = output.rfind(last)? + last.len();
    (from < to).then(|| (output[from..to].trim().to_string(), a + blocks.len() - z))
}

/// The cleaned text without what it copied off a page image from the chunk just before or after, even one
/// block (DeepSeek in Pargent: the next chunk's first paragraph cut where its page ends, the abstract's
/// keywords, the chunk before's last lines): blocks before its first own text that the source hasn't but
/// `before` (the source just before the chunk) has, and after its last own text that `after` has; its own
/// figures there stay (DeepSeek put the next chunk's heading and paragraph above the chunk's last figure).
/// The text kept and how many blocks went.
pub fn trim_neighbors(source: &str, output: &str, before: &str, after: &str) -> Option<(String, usize)> {
    let key = |t: &str| prose(t).chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let (own, prev, next) = (key(source), key(before), key(after));
    let text = |b: &mmd::Block| matches!(b.kind, Kind::Paragraph | Kind::Heading | Kind::Caption | Kind::Footnote);
    let probe = |b: &mmd::Block| key(&b.text).chars().take(24).collect::<String>();
    let theirs = |b: &mmd::Block, other: &str| {
        if text(b) {
            let p = probe(b);
            p.chars().count() >= 8 && !own.contains(&p) && other.contains(&p)
        } else if b.kind == Kind::Figure {
            let links: Vec<&str> = b.text.split(['(', ')', '{', '}']).filter(|t| t.starts_with("assets/") || t.starts_with("http")).collect();
            !links.is_empty() && links.iter().all(|l| !source.contains(l) && (before.contains(l) || after.contains(l)))
        } else {
            false
        }
    };
    let blocks = mmd::blocks(output).0;
    // the chunk's own text: where its first and last are
    let mine: Vec<usize> = (0..blocks.len()).filter(|&i| text(&blocks[i]) && own.contains(&probe(&blocks[i]))).collect();
    let (first, last) = (mine.first().copied().unwrap_or(blocks.len()), mine.last().copied().unwrap_or(0));
    let gone: Vec<usize> = (0..blocks.len()).filter(|&i| (i < first && theirs(&blocks[i], &prev)) || (i > last && theirs(&blocks[i], &next))).collect();
    if gone.is_empty() || gone.len() == blocks.len() {
        return None;
    }
    // cut the reply itself: the blocks kept, each from where it is in the reply, joined as they were
    let mut kept = vec![];
    let mut at = 0;
    for (i, b) in blocks.iter().enumerate() {
        let start = at + output[at..].find(b.text.as_str())?;
        at = start + b.text.len();
        if !gone.contains(&i) {
            kept.push(&output[start..at]);
        }
    }
    Some((kept.join("\n\n"), gone.len()))
}

/// The request's headings left in a result (ones the source hasn't).
fn echoed(source: &str, output: &str) -> Option<Issue> {
    super::prompts::ECHOED.iter().find(|m| output.contains(*m) && !source.contains(*m)).map(|m| issue("echoed", format!("结果里混进了给模型的说明（{m}）")))
}

/// What's the same in any rewrite of a chunk: not empty, not cut, the structure kept.
fn structure(source: &str, output: &str, cut: bool, out: &mut Vec<Issue>) -> bool {
    if output.trim().is_empty() {
        out.push(issue("empty", "结果是空的".into()));
        return false;
    }
    out.extend(echoed(source, output));
    if cut {
        out.push(issue("cut", "回复写到了长度上限，被截断了".into()));
    } else if unclosed(output) && !unclosed(source) {
        out.push(issue("unclosed", "有公式或表格没写完（可能被截断了）".into()));
    }
    let (s, o) = (counts(source), counts(output));
    for (code, what, a, b) in [("math", "独立公式", s.display_math, o.display_math), ("tables", "表格", s.tables, o.tables), ("images", "图", s.images, o.images), ("footnotes", "脚注", s.footnotes, o.footnotes)] {
        if b < a {
            out.push(issue(code, format!("{what}少了（原文 {a} 个，结果 {b} 个）")));
        }
    }
    if o.headings + 2 <= s.headings {
        out.push(issue("headings", format!("标题少了（原文 {} 个，结果 {} 个）", s.headings, o.headings)));
    } else if sections(output) < sections(source) {
        out.push(issue("headings", format!("章节标题少了（原文 {} 个，结果 {} 个）", sections(source), sections(output))));
    }
    if s.inline_math >= 5 && (o.inline_math as f64) < s.inline_math as f64 * 0.8 - 2.0 {
        out.push(issue("inline", format!("行内公式少了很多（原文 {} 个，结果 {} 个）", s.inline_math, o.inline_math)));
    }
    true
}

/// 「[12]」「[3, 5]」: citation numbers.
fn citations(text: &str) -> usize {
    text.match_indices('[').filter(|(i, _)| text[i + 1..].chars().next().is_some_and(|c| c.is_ascii_digit())).count()
}

/// The prose of a text: formula, table and image lines, LaTeX command names, links and inline formulas
/// left out (each leaves a space).
fn prose(text: &str) -> String {
    let mut lines = String::with_capacity(text.len());
    let (mut math, mut env) = (false, 0i64);
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("\\[") {
            math = !t.contains("\\]");
            continue;
        }
        if t.starts_with("\\]") {
            math = false;
            continue;
        }
        if t.starts_with('$') {
            // a whole formula on the line, else its start or end
            if !t.get(2..).unwrap_or("").contains('$') {
                math = !math;
            }
            continue;
        }
        if t.starts_with("\\includegraphics") || t.starts_with("![") {
            continue;
        }
        env += ["equation", "align", "gather", "tabular"].iter().map(|e| t.matches(&format!("\\begin{{{e}")).count() as i64 - t.matches(&format!("\\end{{{e}")).count() as i64).sum::<i64>();
        if math || env > 0 {
            continue;
        }
        lines.push_str(line);
        lines.push('\n');
    }
    let chars: Vec<char> = lines.chars().collect();
    let (mut out, mut i, mut inline) = (String::with_capacity(lines.len()), 0, false);
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && matches!(chars.get(i + 1), Some('(') | Some(')')) {
            inline = chars[i + 1] == '(';
            i += 2;
            out.push(' ');
            continue;
        }
        if c == '\\' {
            // a LaTeX command's name
            i += 1;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            out.push(' ');
            continue;
        }
        if !inline && c == 'h' && chars[i..].iter().take(4).collect::<String>() == "http" {
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != ')' {
                i += 1;
            }
            out.push(' ');
            continue;
        }
        if !inline {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// How Chinese the prose is: CJK characters against English words (about five letters each).
fn chinese_share(text: &str) -> Option<f64> {
    let p = prose(text);
    let wide = p.chars().filter(|c| crate::picky::context::is_wide(*c)).count() as f64;
    let words = p.chars().filter(char::is_ascii_alphabetic).count() as f64 / 5.0;
    (wide + words >= 20.0).then(|| wide / (wide + words))
}

/// Lowercase English words standing alone in Chinese prose (most likely left untranslated): each once,
/// at most 20. Names, acronyms and English phrases (words next to other words) aren't.
pub fn leftovers(text: &str) -> Vec<String> {
    let p: Vec<char> = prose(text).chars().collect();
    let quiet = |c: char| c == ' ' || "\"'“”‘’「」".contains(c);
    let neighbor = |mut j: isize, step: isize| -> Option<char> {
        while j >= 0 && (j as usize) < p.len() && quiet(p[j as usize]) {
            j += step;
        }
        (j >= 0 && (j as usize) < p.len()).then(|| p[j as usize])
    };
    let (mut out, mut i): (Vec<String>, usize) = (vec![], 0);
    while i < p.len() && out.len() < 20 {
        if !p[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let a = i;
        while i < p.len() && (p[i].is_ascii_alphanumeric() || p[i] == '-') {
            i += 1;
        }
        let word: String = p[a..i].iter().collect();
        let (before, after) = (neighbor(a as isize - 1, -1), neighbor(i as isize, 1));
        let english = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
        let wide = |c: Option<char>| c.is_some_and(crate::picky::context::is_wide);
        if word.len() >= 3 && word.chars().all(|c| c.is_ascii_lowercase() || c == '-') && !english(before) && !english(after) && (wide(before) || wide(after)) && !out.contains(&word) {
            out.push(word);
        }
    }
    out
}

/// What's wrong with `output` as the translation of `source` into Chinese (`language`: check it's Chinese).
pub fn check_translation(source: &str, output: &str, cut: bool, language: bool) -> Vec<Issue> {
    let mut out = vec![];
    if !structure(source, output, cut, &mut out) {
        return out;
    }
    let (a, b) = (citations(source), citations(output));
    if a >= 3 && (b as f64) < a as f64 * 0.8 - 1.0 {
        out.push(issue("citations", format!("引用编号少了（原文 {a} 个，结果 {b} 个）")));
    }
    let (s, o) = (crate::picky::context::estimate(source), crate::picky::context::estimate(output));
    if s >= 80 {
        let ratio = o as f64 / s as f64;
        if ratio < 0.35 {
            out.push(issue("short", "译文比原文短很多（可能漏译了）".into()));
        } else if ratio > 2.5 {
            out.push(issue("long", "译文比原文长很多（可能重复了）".into()));
        }
    }
    if language && chinese_share(output).is_some_and(|share| share < 0.4) {
        out.push(issue("language", "没有译成中文".into()));
    }
    if language {
        if let Some(i) = lost_paragraph(source, output) {
            out.push(i);
        }
    }
    out
}

/// The paragraphs of a text (mmd's blocks: headings, formulas, tables, figures left out).
fn paragraphs(text: &str) -> Vec<String> {
    mmd::blocks(text).0.into_iter().filter(|b| b.kind == Kind::Paragraph && !prose(&b.text).trim().is_empty()).map(|b| b.text).collect()
}

/// A paragraph missing from a translation into Chinese, or one much shorter than its source. Chinese
/// runs about 1.4–2 characters to an English word (an English word left in counts 1.6, a digit 0.5: 「9月」
/// says September): a paragraph under 0.75 of the chunk's usual rate (and always under 0.9) has lost a
/// good part. A short one (under 60 words) varies more — news with numbers is terse (「compared with the
/// previous three months」: 环比) — and has to fall under half of it, or under 0.6 with fewer sentences
/// (。！？) than the English. The issue names the paragraph by its first words: the Supervisor counts paragraphs its own way.
fn lost_paragraph(source: &str, output: &str) -> Option<Issue> {
    let (a, b) = (paragraphs(source), paragraphs(output));
    if a.len() >= 3 && b.len() + (a.len() / 10).max(1) <= a.len() {
        return Some(issue("paragraphs", format!("段落少了（原文 {} 段，译文 {} 段）", a.len(), b.len())));
    }
    if a.len() != b.len() {
        return None;
    }
    let words = |t: &str| prose(t).split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()).count() as f64;
    let wide = |t: &str| prose(t).chars().filter(|c| crate::picky::context::is_wide(*c)).count() as f64;
    let digits = |t: &str| prose(t).chars().filter(char::is_ascii_digit).count() as f64;
    let rates: Vec<(usize, f64, f64)> = a.iter().zip(&b).enumerate().filter(|(_, (s, _))| words(s) >= 30.0).map(|(k, (s, o))| (k, (wide(o) + 1.6 * words(o) + 0.5 * digits(o)) / words(s), words(s))).collect();
    let mut sorted: Vec<f64> = rates.iter().map(|r| r.1).collect();
    sorted.sort_by(f64::total_cmp);
    let usual = if sorted.len() >= 3 { sorted[sorted.len() / 2] } else { 0.0 };
    let k = rates.iter().find(|(k, rate, words)| if *words >= 60.0 { *rate < (usual * 0.75).max(0.9) } else { *rate < usual * 0.5 || (*rate < (usual * 0.6).max(0.9) && sentences_zh(&b[*k]) < sentences_en(&a[*k])) })?.0;
    let opening: Vec<&str> = a[k].split_whitespace().take(6).collect();
    Some(issue("paragraph_short", format!("第 {} 段（「{}…」）译文比原文短很多（可能漏了句子）", k + 1, opening.join(" "))))
}

/// Sentences of English prose: an end (. ! ?) before a space and a capital or a quote, and the last one.
fn sentences_en(text: &str) -> usize {
    let c: Vec<char> = prose(text).trim().chars().collect();
    1 + c.windows(3).filter(|w| ".!?".contains(w[0]) && w[1] == ' ' && (w[2].is_ascii_uppercase() || "\"“".contains(w[2]))).count()
}

/// Sentences of Chinese prose: 。！？ (the last one may have none).
fn sentences_zh(text: &str) -> usize {
    prose(text).chars().filter(|c| "。！？".contains(*c)).count().max(1)
}

/// What's wrong with `output` as `source` in the other script.
pub fn check_script(source: &str, output: &str, cut: bool) -> Vec<Issue> {
    let mut out = vec![];
    if !structure(source, output, cut, &mut out) {
        return out;
    }
    let (s, o) = (counts(source).chars, counts(output).chars);
    if s >= 100 {
        let ratio = o as f64 / s as f64;
        if !(0.8..=1.25).contains(&ratio) {
            out.push(issue(if ratio < 1.0 { "short" } else { "long" }, format!("和原文长短差得多（原文 {s} 字，结果 {o} 字）")));
        }
    }
    out
}

/// The issues in one line (for the log and the Supervisor).
pub fn said(issues: &[Issue]) -> String {
    issues.iter().map(|i| i.says.as_str()).collect::<Vec<_>>().join("；")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_checks() {
        let src = format!("\\section*{{1 Intro}}\n\n{} [1] [2] [3, 4].\n\n\\[\nx=1\n\\]", "The model works well on long inputs. ".repeat(10));
        let good = format!("\\section*{{1 引言}}\n\n{} [1] [2] [3, 4]。\n\n\\[\nx=1\n\\]", "这个模型在长输入上效果很好。".repeat(10));
        assert!(check_translation(&src, &good, false, true).is_empty(), "{:?}", check_translation(&src, &good, false, true));
        let codes = |o: &str| check_translation(&src, o, false, true).iter().map(|i| i.code).collect::<Vec<_>>();
        assert!(codes(&good.replace("[1] [2] [3, 4]", "")).contains(&"citations"));
        assert!(codes(&good.replace("\\[\nx=1\n\\]", "")).contains(&"math"));
        assert!(codes("\\section*{1 引言}\n\n很好。\n\n\\[\nx=1\n\\] [1] [2] [3, 4]").contains(&"short"));
        assert!(codes(&src).contains(&"language"));
        assert!(!check_translation(&src, &src, false, false).iter().any(|i| i.code == "language"));
        // formulas, tables, links and images don't count: a translated chunk full of them is still Chinese
        let heavy = format!("{good}\n\n\\begin{{tabular}}{{l}}\nSelf-Attention & O(n^2 d) & long English cell text here \\\\\n\\end{{tabular}}\n\n\\includegraphics{{https://cdn.mathpix.com/cropped/abc-03.jpg?height=1127&width=775&top_left_y=247}}\n\n公式 \\(\\operatorname{{softmax}}\\left(\\frac{{Q K^{{T}}}}{{\\sqrt{{d_{{k}}}}}}\\right) V\\) 很长。");
        assert_eq!(chinese_share(&heavy).map(|s| s > 0.9), Some(true));
        assert!(chinese_share("x").is_none());
        assert!(chinese_share("$\n这一行只有一个美元符号，不该出错。").is_none());
        let tr = "据《晚星报》报道，一个 visibly“恼火”的人；世界上 literally 每个部分；史密森学会（the Smithsonian）；DJT 车牌；roadeo 是混成词；\\(x\\) 公式里的 \\textbf 不算；https://example.com/path 也不算。";
        assert_eq!(leftovers(tr), ["visibly", "literally", "roadeo"]);
        // a paragraph dropped, or half of one
        let en = |n: usize| (1..=n).map(|i| format!("Paragraph {i} tells the story of the convention center plans that never came to pass in New York or Washington, and what the festival and the exhibition left behind for the people who saw them.")).collect::<Vec<_>>().join("\n\n");
        let zh = |n: usize, cut: usize| (1..=n).map(|i| if i == cut { "第二段只剩一句。".to_string() } else { format!("第 {i} 段讲的是那些在纽约和华盛顿都没能建成的会展中心计划，以及那场民俗节和那次展览给看过它们的人留下了什么，这些事后来很少有人再提起，但当年的报纸都写过。") }).collect::<Vec<_>>().join("\n\n");
        assert!(lost_paragraph(&en(4), &zh(4, 0)).is_none());
        assert_eq!(lost_paragraph(&en(4), &zh(3, 0)).map(|i| i.code), Some("paragraphs"));
        assert_eq!(lost_paragraph(&en(4), &zh(4, 2)).map(|i| i.says), Some("第 2 段（「Paragraph 2 tells the story of…」）译文比原文短很多（可能漏了句子）".into()));
        // The Economist's terse business news, all there (the 3.3% and 0.5% paragraphs read short); a sentence dropped is still seen
        let news = ["Kevin Warsh struck a hawkish tone in a keynote speech to central bankers gathered at Jackson Hole. The Federal Reserve's chairman said it still had \"work to do\" to curtail inflation, which markets took to mean an interest-rate rise is on the cards, possibly at the Fed's next meeting.", "The euro area recorded a 3.3\\% year-on-year rise in the consumer-price index for August. In July the CPI rose by 2.9\\%. Higher inflation makes it all the more certain that the European Central Bank will lift interest rates again when it meets on September 10th. In June it increased rates by a quarter of a percentage point.", "Brazil's economy grew by 0.5\\% in the second quarter compared with the previous three months, or by 2\\% on a year-on-year basis. The GDP data were the last to be released before the presidential election on October 4th.", "Shein's share price closed down slightly on its first day of trading on the Hong Kong exchange, then tumbled the following day. The ultra-cheap clothing retailer was valued at \\$26bn at its IPO, a fraction of its worth four years ago, when it started its quest to become a public company. Meanwhile, France began imposing fees on cheap fast-fashion firms such as Shein and Temu. This is part of a new law that its supporters say will curb sales in a business that floods markets with environmentally unfriendly disposable clothing."].join("\n\n");
        let brazil = "巴西经济第二季度环比增长0.5\\%，同比增长2\\%。这是10月4日总统选举前发布的最后一批GDP数据。";
        let news_zh = |brazil: &str| ["凯文·沃什在杰克逊霍尔向与会的央行官员发表主旨演讲时语调偏鹰。这位美联储主席表示，在遏制通胀方面仍有“工作要做”，市场将此解读为加息的可能性存在，或许就在美联储下次会议上。", "欧元区8月消费者价格指数同比上涨3.3\\%。7月CPI上涨2.9\\%。通胀走高使欧洲央行在9月10日会议上再次加息更加确定无疑。6月该行已将利率上调25个基点。", brazil, "Shein在香港交易所上市首日股价收盘小幅下跌，次日大幅下挫。这家超低价服装零售商IPO估值260亿美元，仅为四年前其启动上市进程时市值的一小部分。与此同时，法国开始对Shein和Temu等廉价快时尚公司收费。这是一项新法律的一部分，其支持者称该法将遏制一个向市场大量投放不环保一次性服装的行业的销售。"].join("\n\n");
        assert!(lost_paragraph(&news, &news_zh(brazil)).is_none());
        assert_eq!((sentences_en(news.split("\n\n").nth(1).unwrap()), sentences_zh(brazil), sentences_zh("没有句号")), (4, 2, 1));
        assert_eq!(lost_paragraph(&news, &news_zh(brazil.split_once('。').unwrap().0)).map(|i| i.says), Some("第 3 段（「Brazil's economy grew by 0.5\\% in…」）译文比原文短很多（可能漏了句子）".into()));
        // after a display formula the prose counts again
        assert!(chinese_share(&format!("\\[\nx = y\n\\]\n{}", "This English sentence comes after the formula. ".repeat(4))).is_some_and(|s| s < 0.1));
        let hans = "这是一段简体中文的文字，用来检查繁体转换的长短。".repeat(6);
        let hant = crate::picky::assistants::pub2md::script::convert(&hans, true).unwrap();
        assert!(check_script(&hans, &hant, false).is_empty());
        assert!(check_script(&hans, &hant.chars().take(hant.chars().count() / 2).collect::<String>(), false).iter().any(|i| i.code == "short"));
    }

    #[test]
    fn a_dropped_passage_or_section_is_seen() {
        let para = |i: usize| format!("Paragraph {i} opens with its own words about the model and the data, and it goes on for a while so that it counts.");
        let source = format!("\\section*{{3 Model}}\n\n{}\n\n\\section*{{4 Why Self-Attention}}\n\n{}\n\n{}\n\n{}", para(1), para(2), para(3), para(4));
        assert!(check(&source, &source, false).is_empty());
        // the heading and two paragraphs gone (the rest long enough to pass the length check)
        let cut = format!("\\section*{{3 Model}}\n\n{}\n\n{}\n\n{}", para(1), para(4), para(4));
        let said: Vec<String> = check(&source, &cut, false).into_iter().map(|i| i.says).collect();
        assert_eq!(said, ["章节标题少了（原文 2 个，结果 1 个）", "漏了段落（从「Paragraph 2 opens with its own…」起连着 2 段）"]);
        // a page number and running head on top of a paragraph, taken off by the cleaning: fine
        let headed = format!("{}\n\n810\nMikel Norris\n\\begin{{itemize}}\n\\item[2] {}\n\n812\nMikel Norris\n{}", para(1), para(2), para(3));
        assert!(dropped(&headed, &format!("\\begin{{itemize}}\n\\item[1] {}\n\\item[2] {}\n\\end{{itemize}}\n\n{}", para(1), para(2), para(3))).is_empty());
        // one paragraph whose opening was fixed: fine
        assert!(check(&source, &source.replace("Paragraph 3 opens", "Paragraph three opens"), false).is_empty());
        // the request's headings in the result
        assert!(check_translation(&source, &format!("{source}\n【术语表】\n- a → 甲"), false, false).iter().any(|i| i.code == "echoed"));
        // the next chunk's passage copied off the page image
        let copied = format!("{source}\n\n{}\n\n{}", para(5), para(6));
        assert_eq!(check(&source, &copied, false).iter().map(|i| i.code).collect::<Vec<_>>(), ["added"]);
        // a translation that turned \section into ## is told
        assert!(check_translation(&source, &source.replace("\\section*{4 Why Self-Attention}", "## 4 Why"), false, false).iter().any(|i| i.code == "headings"));
        assert!(heading_style(&source, &source.replace("\\section*{4 Why Self-Attention}", "# 4 Why")).is_some() && heading_style(&source, &source).is_none());
    }

    #[test]
    fn copied_passages_are_trimmed() {
        let para = |i: usize| format!("Paragraph {i} opens with its own words about the model and the data, and it goes on for a while so that it counts.");
        let source = format!("\\section*{{3 Model}}\n\n{}\n\n{}", para(1), para(2));
        // the next chunk's heading, two paragraphs and figure copied at the end
        let out = format!("{source}\n\n\\section*{{4 Why}}\n\n{}\n\n![](assets/next.jpg)\n\n{}", para(3), para(4));
        assert_eq!(trim_copied(&source, &out), Some((source.clone(), 4)));
        // the chunk before's two paragraphs at the start
        assert_eq!(trim_copied(&source, &format!("{}\n\n{}\n\n{source}", para(8), para(9))).map(|t| t.1), Some(2));
        // the next chunk's short sections: their headings are enough
        let short = format!("{source}\n\n\\section*{{Supplementary materials}}\n\nTo view it, please visit the site.\n\n\\section*{{Notes}}\n\n\\footnotetext{{1 The note of the next chunk.}}");
        assert_eq!(trim_copied(&source, &short), Some((source.clone(), 4)));
        assert!(check(&source, &short, false).iter().any(|i| i.code == "added"));
        // a heading made from the source's own bold lead-in: fine
        assert!(foreign_headings("Acknowledgements We thank them.", "\\section*{Acknowledgements}\n\nWe thank them.").is_empty());
        // one foreign paragraph, or a fixed heading: left for the checks
        assert_eq!(trim_copied(&source, &format!("{source}\n\n{}", para(3))), None);
        // … unless it is the next chunk's (its start, cut where the page ends) or the chunk before's
        let next = format!("{}\n\n{}", para(3), para(4));
        let cut_off: String = para(3).chars().take(50).collect();
        assert_eq!(trim_neighbors(&source, &format!("{source}\n\n{cut_off}"), "", &next), Some((source.clone(), 1)));
        assert_eq!(trim_neighbors(&source, &format!("Received 7/13/22; Revision accepted\n\n{source}\n\n{cut_off}"), "Keywords: ML\n\nReceived 7/13/22; Revision accepted", &next), Some((source.clone(), 2)));
        // the next chunk's heading and paragraph put above the chunk's own last figure: the figure stays
        let fig = "\\begin{figure}\n\\includegraphics{assets/own.jpg}\n\\caption{Fig. 7.}\n\\end{figure}";
        let src_fig = format!("{source}\n\n{fig}");
        let out = format!("{source}\n\n\\section*{{Practical Exercise 2}}\n\n{}\n\n{fig}", para(3));
        assert_eq!(trim_neighbors(&src_fig, &out, "", &format!("\\section*{{Practical Exercise 2}}\n\n{next}")), Some((src_fig.clone(), 2)));
        // a paragraph the source has too, or one nobody has (a rewording): kept
        assert_eq!(trim_neighbors(&source, &source, &para(2), &para(1)), None);
        assert_eq!(trim_neighbors(&source, &format!("{source}\n\n{}", para(7)), "", &next), None);
        assert_eq!(trim_copied(&source, &source.replace("3 Model", "3 Models")), None);
    }

    #[test]
    fn checks() {
        let source = format!("\\section*{{1 Intro}}\n\n{}\n\n\\[\nx=1\n\\]\n\n\\begin{{tabular}}{{l}}\na\n\\end{{tabular}}\n\n\\includegraphics{{a.jpg}}\n\n\\footnotetext{{note}}", "Some text here. ".repeat(40));
        assert!(check(&source, &source, false).is_empty());
        let codes = |o: &str, cut| check(&source, o, cut).iter().map(|i| i.code).collect::<Vec<_>>();
        assert_eq!(codes("", false), ["empty"]);
        assert!(codes(&source, true).contains(&"cut"));
        assert!(codes(&source.replace("\\end{tabular}", ""), false).contains(&"unclosed"));
        let short: String = source.chars().take(200).collect();
        let c = codes(&short, false);
        assert!(c.contains(&"short") && c.contains(&"math") && c.contains(&"tables") && c.contains(&"images") && c.contains(&"footnotes"), "{c:?}");
        assert!(codes(&format!("{source}\n\n{source}"), false).contains(&"long"));
        assert!(codes(&source.replace("Some text here. ", "这里有一些文字。"), false).contains(&"language"));
        assert!(said(&check(&source, "", false)).contains("空"));
        // MD output: tables as pipes, formulas with dollars
        let md = format!("# 1 Intro\n\n{}\n\n$$\nx=1\n$$\n\n| a |\n|---|\n\n![](a.jpg)\n\n[^1]\n\n[^1]: note", "Some text here. ".repeat(40));
        assert!(check(&source, &md, false).is_empty(), "{:?}", check(&source, &md, false));
    }

    /// The translation checks on real files (no model): a 整理稿 and its 译文.
    ///   POMELO_CHECK_PAIRS="<整理稿>|<译文>;…" cargo test real_check_files -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_check_files() {
        for pair in std::env::var("POMELO_CHECK_PAIRS").unwrap_or_default().split(';').filter(|p| !p.is_empty()) {
            let (a, b) = pair.split_once('|').unwrap();
            let (s, o) = (std::fs::read_to_string(a).unwrap(), std::fs::read_to_string(b).unwrap());
            eprintln!("[check] {b}：{}（{} 段 / {} 段）", said(&check_translation(&s, &o, false, true)), paragraphs(&s).len(), paragraphs(&o).len());
        }
    }
}
