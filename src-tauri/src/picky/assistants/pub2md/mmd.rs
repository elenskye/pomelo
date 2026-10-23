//! Reading Mathpix Markdown for Pub2MD (no model): blocks, the pages they are on, where a chunk may be
//! cut, which part of a paper each block is in, and the counts the checks compare (check.rs).
//! - A block is a run of lines up to a blank line or `\pagebreak` (Mathpix's page marker), except that
//!   an environment (`\begin{…}` … `\end{…}`), display math (`\[ … \]`, `$$ … $$`) or a command whose
//!   braces span lines (`\title{`, `\author{`, `\footnotetext{`…) stays whole, page breaks inside it
//!   included. Display math written right under its paragraph stays in that paragraph's block.
//! - Cut points (before block i): never right after a heading or a title, never between a figure /
//!   table and its caption, never inside a sentence going on over a page break (the paragraph doesn't end
//!   with closing punctuation, or the next starts lower-case), never between an equation and its lead-in.
//! - Parts (前置信息、摘要和关键词、正文、结论、致谢、参考文献、附录) come from the headings; a document
//!   without a title, authors or an abstract near its start is all 正文.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Title,
    Author,
    Abstract,
    Heading,
    Paragraph,
    Math,
    Table,
    Figure,
    Caption,
    Footnote,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub kind: Kind,
    pub text: String,
    /// The page it starts on (1-based) and ends on.
    pub page: usize,
    pub end_page: usize,
    /// Headings: 1 = section, 2 = subsection…
    pub level: u8,
}

/// Unescaped `{` minus `}`.
fn brace_balance(line: &str) -> i64 {
    let mut n = 0i64;
    let mut escaped = false;
    for c in line.chars() {
        match c {
            '\\' if !escaped => {
                escaped = true;
                continue;
            }
            '{' if !escaped => n += 1,
            '}' if !escaped => n -= 1,
            _ => {}
        }
        escaped = false;
    }
    n
}

const SPANNING: &[&str] = &["\\title{", "\\author{", "\\footnotetext{", "\\caption{", "\\section", "\\subsection", "\\subsubsection", "\\footnote{"];

/// What keeps a block open after its lines so far.
#[derive(Default)]
struct Open {
    envs: i64,
    braces: i64,
    bracket_math: i64,
    dollars: bool,
    /// Lines taken while open (a runaway guard).
    lines: usize,
}

impl Open {
    fn any(&self) -> bool {
        self.envs > 0 || self.braces > 0 || self.bracket_math > 0 || self.dollars
    }
    fn take(&mut self, line: &str, first: bool) {
        let t = line.trim();
        self.envs += t.matches("\\begin{").count() as i64 - t.matches("\\end{").count() as i64;
        self.bracket_math += t.matches("\\[").count() as i64 - t.matches("\\]").count() as i64;
        if t.matches("$$").count() % 2 == 1 {
            self.dollars = !self.dollars;
        }
        if first && SPANNING.iter().any(|s| t.starts_with(s)) || self.braces > 0 {
            self.braces += brace_balance(t);
        }
        if self.any() {
            self.lines += 1;
        }
        // an unclosed brace / environment from bad OCR must not swallow the document
        if self.lines > 400 {
            *self = Open::default();
        }
        self.envs = self.envs.max(0);
        self.bracket_math = self.bracket_math.max(0);
        self.braces = self.braces.max(0);
    }
}

fn kind_of(first: &str) -> (Kind, u8) {
    let t = first.trim_start();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if hashes > 0 && hashes <= 6 && t[hashes..].starts_with(' ') {
        return (Kind::Heading, hashes as u8);
    }
    for (p, k, l) in [
        ("\\title{", Kind::Title, 0),
        ("\\author{", Kind::Author, 0),
        ("\\section", Kind::Heading, 1),
        ("\\subsection", Kind::Heading, 2),
        ("\\subsubsection", Kind::Heading, 3),
        ("\\begin{abstract}", Kind::Abstract, 0),
        ("\\begin{table", Kind::Table, 0),
        ("\\begin{tabular", Kind::Table, 0),
        ("\\begin{longtable", Kind::Table, 0),
        ("|", Kind::Table, 0),
        ("\\begin{figure", Kind::Figure, 0),
        ("\\includegraphics", Kind::Figure, 0),
        ("![", Kind::Figure, 0),
        ("\\footnotetext{", Kind::Footnote, 0),
        ("\\[", Kind::Math, 0),
        ("$$", Kind::Math, 0),
        ("\\begin{equation", Kind::Math, 0),
        ("\\begin{align", Kind::Math, 0),
        ("\\begin{gather", Kind::Math, 0),
    ] {
        if t.starts_with(p) {
            return (k, l);
        }
    }
    if is_caption(t) {
        return (Kind::Caption, 0);
    }
    (Kind::Paragraph, 0)
}

/// 「Figure 1: …」「Table 2.」「Fig. 3」「图 1」「表 2」
fn is_caption(t: &str) -> bool {
    let t = t.trim_start_matches(['*', '_']);
    for p in ["Figure", "Fig.", "Fig", "Table", "TABLE", "FIGURE", "图", "表"] {
        if let Some(rest) = t.strip_prefix(p) {
            return rest.trim_start().chars().next().is_some_and(|c| c.is_ascii_digit() || "IVX".contains(c));
        }
    }
    false
}

/// The blocks and the number of pages (Mathpix ends the last page with `\pagebreak` too: an empty last page isn't one).
pub fn blocks(mmd: &str) -> (Vec<Block>, usize) {
    let mut out: Vec<Block> = vec![];
    let mut page = 1usize;
    let mut cur: Vec<&str> = vec![];
    let mut start = 1usize;
    let mut open = Open::default();
    let flush = |out: &mut Vec<Block>, cur: &mut Vec<&str>, start: usize, end: usize| {
        while cur.last().is_some_and(|l| l.trim().is_empty()) {
            cur.pop();
        }
        if cur.is_empty() {
            return;
        }
        let (kind, level) = kind_of(cur[0]);
        out.push(Block { kind, text: cur.join("\n"), page: start, end_page: end, level });
        cur.clear();
    };
    for line in mmd.lines() {
        let t = line.trim();
        if t == "\\pagebreak" {
            if open.any() {
                page += 1;
                continue;
            }
            flush(&mut out, &mut cur, start, page);
            page += 1;
            continue;
        }
        if !open.any() && t.is_empty() {
            flush(&mut out, &mut cur, start, page);
            continue;
        }
        if cur.is_empty() {
            start = page;
        }
        let first = cur.is_empty();
        cur.push(line);
        open.take(line, first);
    }
    flush(&mut out, &mut cur, start, page);
    let pages = out.last().map_or(1, |b| b.end_page);
    (out, pages)
}

/// Ends a sentence (closing quotes / brackets / emphasis, and footnote marks like `\({ }^{2}\)`, after it allowed).
fn ends_sentence(text: &str) -> bool {
    let mut t = text.trim_end();
    // a superscript footnote mark Mathpix writes after the full stop
    while let Some(open) = t.ends_with("\\)").then(|| t.rfind("\\(")).flatten() {
        let mark = &t[open..];
        if mark.contains('^') && mark.chars().count() <= 24 {
            t = t[..open].trim_end();
        } else {
            break;
        }
    }
    let t = t.trim_end_matches(['"', '”', '’', '\'', ')', '）', ']', '】', '*', '_', '}']);
    t.chars().last().is_some_and(|c| ".!?。！？…;；".contains(c))
}

fn starts_lower(text: &str) -> bool {
    text.trim_start().chars().next().is_some_and(|c| c.is_lowercase())
}

/// Whether a chunk may start at block `i` (0 always; never past the end).
pub fn cuttable(blocks: &[Block], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    if i >= blocks.len() {
        return false;
    }
    let (prev, next) = (&blocks[i - 1], &blocks[i]);
    if matches!(prev.kind, Kind::Heading | Kind::Title) {
        return false;
    }
    let figure = |k: Kind| matches!(k, Kind::Figure | Kind::Table);
    if (figure(prev.kind) && next.kind == Kind::Caption) || (prev.kind == Kind::Caption && figure(next.kind)) {
        return false;
    }
    if next.kind == Kind::Math && prev.kind == Kind::Paragraph {
        return false;
    }
    if next.kind == Kind::Paragraph && starts_lower(&next.text) {
        return false;
    }
    if prev.kind == Kind::Paragraph && next.kind == Kind::Paragraph && !ends_sentence(&prev.text) {
        return false;
    }
    !across_asides(blocks, i)
}

/// A page number, running head or footer Mathpix left as its own block: a short line first (or second)
/// on its page, or last on it. (A running head like「Pargent et al.」ends like a sentence.)
fn furniture(blocks: &[Block], j: usize) -> bool {
    let b = &blocks[j];
    let t = b.text.trim();
    let short = b.kind == Kind::Paragraph && !t.contains('\n') && t.chars().count() <= 60;
    let first = j == 0 || blocks[j - 1].end_page < b.page || (j >= 2 && blocks[j - 2].end_page < b.page);
    let last = blocks.get(j + 1).is_some_and(|n| n.page > b.end_page);
    short && (first || last)
}

/// Block `i` is inside a sentence that goes on over a page with page furniture or a figure, table,
/// caption or footnote in between: both Workers would make the paragraph whole from the page images.
fn across_asides(blocks: &[Block], i: usize) -> bool {
    // the text at block j: a paragraph, or what Mathpix wrote right under a figure's \end{…}; None to step over
    let text = |j: usize| -> Result<Option<&str>, ()> {
        let b = &blocks[j];
        if b.kind == Kind::Paragraph && !furniture(blocks, j) {
            return Ok(Some(b.text.as_str()));
        }
        if !(furniture(blocks, j) || matches!(b.kind, Kind::Figure | Kind::Table | Kind::Caption | Kind::Footnote)) {
            return Err(());
        }
        Ok(text_after_env(&b.text))
    };
    let prev = (i.saturating_sub(6)..i).rev().map(text).find(|t| !matches!(t, Ok(None)));
    let next = (i..blocks.len().min(i + 6)).map(text).find(|t| !matches!(t, Ok(None)));
    match (prev, next) {
        (Some(Ok(Some(a))), Some(Ok(Some(b)))) => !ends_sentence(a) || starts_lower(b),
        _ => false,
    }
}

/// Text in a figure / table block after its environment ends (no blank line between them).
fn text_after_env(text: &str) -> Option<&str> {
    let lines: Vec<&str> = text.lines().collect();
    let end = lines.iter().rposition(|l| l.trim_start().starts_with("\\end{"))?;
    let start = text.lines().take(end + 1).map(|l| l.len() + 1).sum::<usize>().min(text.len());
    Some(text[start..].trim()).filter(|t| !t.is_empty())
}

// ---------- parts of a paper ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Part {
    Front,
    Abstract,
    #[default]
    Body,
    Conclusion,
    Acknowledgments,
    References,
    Appendix,
}

impl Part {
    pub const ALL: [Part; 7] = [Part::Front, Part::Abstract, Part::Body, Part::Conclusion, Part::Acknowledgments, Part::References, Part::Appendix];
    pub fn name(self) -> &'static str {
        match self {
            Part::Front => "前置信息",
            Part::Abstract => "摘要和关键词",
            Part::Body => "正文",
            Part::Conclusion => "结论",
            Part::Acknowledgments => "致谢",
            Part::References => "参考文献",
            Part::Appendix => "附录",
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Part::Front => "front",
            Part::Abstract => "abstract",
            Part::Body => "body",
            Part::Conclusion => "conclusion",
            Part::Acknowledgments => "acknowledgments",
            Part::References => "references",
            Part::Appendix => "appendix",
        }
    }
    pub fn from_code(code: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|p| p.code() == code)
    }
}

/// A heading's words: no command, numbering or punctuation, lower case.
pub fn heading_words(text: &str) -> String {
    let first = text.lines().next().unwrap_or("");
    let mut t = first.trim().trim_start_matches('#').trim().to_string();
    for cmd in ["\\section*{", "\\section{", "\\subsection*{", "\\subsection{", "\\subsubsection*{", "\\subsubsection{"] {
        if let Some(rest) = t.strip_prefix(cmd) {
            t = rest.trim_end().trim_end_matches('}').to_string();
        }
    }
    let t = t.replace(['*', '_'], "");
    // Chinese numbering: 六、 / 第一章 / 一．
    let t = {
        let digits = |c: char| "一二三四五六七八九十百零".contains(c);
        let rest = t.trim_start_matches('第').trim_start_matches(digits);
        match rest.chars().next() {
            Some(c) if rest.len() < t.len() && "、.．章节 ".contains(c) => rest[c.len_utf8()..].trim().to_string(),
            _ => t,
        }
    };
    // numbering: 1 / 1. / 3.2 / IV. / A.
    let words: Vec<&str> = t.split_whitespace().collect();
    let numbering = |w: &str| {
        let w = w.trim_end_matches(['.', ')', '、', ':']);
        !w.is_empty() && (w.chars().all(|c| c.is_ascii_digit() || c == '.') || (w.len() <= 4 && w.chars().all(|c| "IVXL".contains(c))) || (w.len() == 1 && w.chars().all(|c| c.is_ascii_uppercase())))
    };
    let rest: Vec<&str> = if words.len() > 1 && numbering(words[0]) { words[1..].to_vec() } else { words };
    rest.join(" ").trim_matches(|c: char| c.is_ascii_punctuation() || c == '：').to_lowercase()
}

/// What a heading says it is (None: an ordinary section).
pub fn heading_part(text: &str) -> Option<Part> {
    let w = heading_words(text);
    let has = |xs: &[&str]| xs.iter().any(|x| w == *x || w.starts_with(&format!("{x} ")));
    if has(&["abstract", "摘要", "summary"]) {
        return Some(Part::Abstract);
    }
    if has(&["references", "reference", "bibliography", "works cited", "literature cited", "literature", "参考文献"]) {
        return Some(Part::References);
    }
    if has(&["acknowledgments", "acknowledgements", "acknowledgment", "acknowledgement", "funding", "致谢", "data availability", "author contributions", "conflict of interest", "conflicts of interest", "competing interests", "declarations", "declaration of competing interest", "ethics statement"]) {
        return Some(Part::Acknowledgments);
    }
    if has(&["appendix", "appendices", "附录", "supplementary", "supplementary material", "supplementary materials", "online appendix"]) || w.starts_with("appendix") {
        return Some(Part::Appendix);
    }
    if w.contains("conclusion") || has(&["concluding remarks", "结论", "结语", "结论与讨论"]) {
        return Some(Part::Conclusion);
    }
    None
}

fn starts_with_any(text: &str, words: &[&str]) -> bool {
    let t = text.trim_start().trim_start_matches(['*', '_', '\\', '{']).trim_start_matches("textbf{").to_lowercase();
    words.iter().any(|w| t.starts_with(w))
}

/// The part of each block.
pub fn parts(blocks: &[Block]) -> Vec<Part> {
    let head = &blocks[..blocks.len().min(15)];
    let paper = head.iter().take(6).any(|b| matches!(b.kind, Kind::Title | Kind::Author))
        || head.iter().any(|b| b.kind == Kind::Abstract || (b.kind == Kind::Heading && heading_part(&b.text) == Some(Part::Abstract)) || starts_with_any(&b.text, &["abstract", "摘要"]));
    let mut part = if paper { Part::Front } else { Part::Body };
    let mut out = Vec::with_capacity(blocks.len());
    for b in blocks {
        match b.kind {
            Kind::Abstract => part = Part::Abstract,
            Kind::Heading => match heading_part(&b.text) {
                Some(p) => part = p,
                None if b.level <= 1 => {
                    part = match part {
                        Part::Front | Part::Abstract | Part::Conclusion => Part::Body,
                        Part::References | Part::Appendix => Part::Appendix,
                        p => if p == Part::Acknowledgments { Part::Body } else { p },
                    }
                }
                None => {
                    if matches!(part, Part::Front | Part::Abstract) {
                        part = Part::Body;
                    }
                }
            },
            Kind::Paragraph if matches!(part, Part::Front | Part::Abstract) && starts_with_any(&b.text, &["abstract", "摘要"]) => part = Part::Abstract,
            Kind::Paragraph if matches!(part, Part::Front | Part::Abstract) && starts_with_any(&b.text, &["keywords", "key words", "index terms", "关键词", "关键字"]) => part = Part::Abstract,
            _ => {}
        }
        out.push(part);
    }
    out
}

// ---------- counts the checks compare ----------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub inline_math: usize,
    pub display_math: usize,
    pub tables: usize,
    pub images: usize,
    pub footnotes: usize,
    pub headings: usize,
    /// Non-blank characters.
    pub chars: usize,
    /// Of them CJK (one-token) characters.
    pub wide: usize,
}

pub fn counts(text: &str) -> Counts {
    let lines: Vec<&str> = text.lines().collect();
    let md_tables = lines.windows(2).filter(|w| w[0].trim_start().starts_with('|') && w[1].trim_start().starts_with('|') && w[1].contains("---")).count();
    // \$ is a dollar sign (\$370bn), not math
    let dollars = text.replace("\\$", "");
    let singles = dollars.replace("$$", "").matches('$').count() / 2;
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    Counts {
        inline_math: text.matches("\\(").count() + singles,
        display_math: text.matches("\\[").count() + dollars.matches("$$").count() / 2 + ["equation", "align", "gather", "multline"].iter().map(|e| text.matches(&format!("\\begin{{{e}")).count()).sum::<usize>(),
        tables: text.matches("\\begin{tabular").count() + text.matches("\\begin{longtable").count() + md_tables,
        images: text.matches("\\includegraphics").count() + text.matches("![").count(),
        footnotes: text.matches("\\footnotetext{").count() + text.matches("\\footnote{").count() + text.matches("[^").count() / 2,
        headings: lines.iter().filter(|l| {
            let t = l.trim_start();
            t.starts_with("\\section") || t.starts_with("\\subsection") || t.starts_with("\\subsubsection") || (t.starts_with('#') && t.trim_start_matches('#').starts_with(' '))
        })
        .count(),
        chars: chars.len(),
        wide: chars.iter().filter(|c| crate::picky::context::is_wide(**c)).count(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Shaped like Mathpix's output for a paper (Attention Is All You Need, shortened).
    pub const PAPER: &str = r"\title{
Attention Is All You Need
}

\author{
Ashish Vaswani* \\ Google Brain
}

\begin{abstract}
The dominant sequence transduction models are based on complex recurrent networks.
\end{abstract}

\section*{1 Introduction}

Recurrent neural networks have been firmly established [12].

\footnotetext{
*Equal contribution.
}

31st Conference on Neural Information Processing Systems (NIPS 2017), Long Beach, CA, USA.
\pagebreak


Recurrent models typically factor computation along the symbol positions of the input

and output sequences, which precludes parallelization.

\section*{3 Model Architecture}

\begin{figure}
\includegraphics[alt={},max width=\textwidth]{https://cdn.mathpix.com/cropped/abc-03.jpg?height=1127}
\captionsetup{labelformat=empty}
\caption{Figure 1: The Transformer - model architecture.}
\end{figure}

We compute the matrix of outputs as:
\[
\operatorname{Attention}(Q, K, V)=\operatorname{softmax}\left(\frac{Q K^{T}}{\sqrt{d_{k}}}\right) V
\]
The two most commonly used attention functions are additive attention [2].
\pagebreak

\begin{table}
\caption{Table 1: Maximum path lengths.}
\begin{tabular}[t]{|l|l|}
\hline Layer & Complexity \\

\hline Self-Attention & \(O(n^{2} \cdot d)\) \\
\hline
\end{tabular}
\end{table}

\section*{7 Conclusion}

In this work, we presented the Transformer.

\section*{References}

[1] Jimmy Lei Ba, Jamie Ryan Kiros, and Geoffrey E Hinton. Layer normalization. 2016.

[2] Dzmitry Bahdanau. Neural machine translation. 2014.
";

    #[test]
    fn blocks_pages_and_kinds() {
        let (b, pages) = blocks(PAPER);
        assert_eq!((pages, blocks(&format!("{PAPER}\n\\pagebreak\n")).1), (3, 3));
        let kinds: Vec<Kind> = b.iter().map(|x| x.kind).collect();
        use Kind::*;
        assert_eq!(kinds, [Title, Author, Abstract, Heading, Paragraph, Footnote, Paragraph, Paragraph, Paragraph, Heading, Figure, Paragraph, Table, Heading, Paragraph, Heading, Paragraph, Paragraph]);
        // the equation stays with its lead-in and the sentence after it
        assert!(b[11].text.contains("\\operatorname{Attention}") && b[11].text.ends_with("attention [2]."));
        // the table keeps its blank line; pages
        assert!(b[12].text.contains("Layer & Complexity \\\\\n\n\\hline"));
        assert_eq!((b[6].page, b[7].page, b[12].page, b[16].page), (1, 2, 3, 3));
        assert_eq!(b[3].level, 1);
        // an unclosed brace doesn't swallow the rest
        let mut text = String::from("\\footnotetext{ never closed\n\n");
        for i in 0..500 {
            text.push_str(&format!("Line {i}.\n\n"));
        }
        let (b, _) = blocks(&text);
        assert!(b.len() > 50);
    }

    #[test]
    fn cut_points() {
        let (b, _) = blocks(PAPER);
        assert!(cuttable(&b, 0));
        assert!(!cuttable(&b, 4)); // right after a heading
        assert!(!cuttable(&b, 8)); // 「…of the input」 / 「and output…」: one sentence
        assert!(cuttable(&b, 9)); // before 3 Model Architecture
        assert!(cuttable(&b, 13) && !cuttable(&b, 99));
        let caption = vec![
            Block { kind: Kind::Table, text: "\\begin{tabular}…".into(), page: 1, end_page: 1, level: 0 },
            Block { kind: Kind::Caption, text: "Table 2: BLEU.".into(), page: 1, end_page: 1, level: 0 },
            Block { kind: Kind::Paragraph, text: "As shown above:".into(), page: 1, end_page: 1, level: 0 },
            Block { kind: Kind::Math, text: "$$x$$".into(), page: 1, end_page: 1, level: 0 },
        ];
        assert!(!cuttable(&caption, 1) && cuttable(&caption, 2) && !cuttable(&caption, 3));
        // a paragraph going on over a page, a page number, a running head and a figure in between (Pargent)
        let over = "Body that ends here. \n\nDifferent types of resampling strategies. An important practical issue when using the holdout estimator\n\\pagebreak\n\n10\n\nPargent et al.\n\n\\begin{figure}\n\\includegraphics{a.jpg}\n\\caption{Fig. 3.}\n\\end{figure}\net al., 2021) typically suggest two thirds.\n\nNext paragraph.";
        let (b, _) = blocks(over);
        assert_eq!(b.iter().map(|b| b.kind).collect::<Vec<_>>(), [Kind::Paragraph, Kind::Paragraph, Kind::Paragraph, Kind::Paragraph, Kind::Figure, Kind::Paragraph]);
        assert!(cuttable(&b, 1) && (2..=4).all(|i| !cuttable(&b, i)) && cuttable(&b, 5), "{b:?}");
        assert_eq!(text_after_env(&b[4].text), Some("et al., 2021) typically suggest two thirds."));
        // a figure whose own text (under \end{figure}) ends the sentence: a cut point after it
        assert!(!across_asides(&b, 5) && cuttable(&b, 5));
        assert!(ends_sentence("not the default in practice. \\({ }^{2}\\)") && ends_sentence("done.\\({ }^{1,2}\\)") && !ends_sentence("the value \\(x^{2}\\)"));
        // the going-on as its own paragraph after the figure
        let (b, _) = blocks(&over.replace("\\end{figure}\net al.", "\\end{figure}\n\net al."));
        assert!((2..=5).all(|i| !cuttable(&b, i)) && cuttable(&b, 6), "{b:?}");
        // the same page's text, a figure between whole sentences: still a cut point
        let whole = "One sentence.\n\n\\begin{figure}\n\\includegraphics{a.jpg}\n\\end{figure}\n\nAnother one.";
        let (b, _) = blocks(whole);
        assert!(cuttable(&b, 1) && cuttable(&b, 2));
    }

    #[test]
    fn parts_of_a_paper() {
        let (b, _) = blocks(PAPER);
        let p = parts(&b);
        assert_eq!(&p[..3], [Part::Front, Part::Front, Part::Abstract]);
        assert_eq!((p[3], p[10], p[13], p[14], p[15], p[17]), (Part::Body, Part::Body, Part::Conclusion, Part::Conclusion, Part::References, Part::References));
        assert_eq!(heading_part("\\section*{Appendix A: Proofs}"), Some(Part::Appendix));
        assert_eq!(heading_part("## 5 Concluding Remarks"), Some(Part::Conclusion));
        assert_eq!(heading_part("\\section*{Acknowledgements}"), Some(Part::Acknowledgments));
        assert_eq!(heading_part("\\section*{六、参考文献}"), Some(Part::References));
        assert_eq!(heading_part("## 第五章 结论"), Some(Part::Conclusion));
        assert_eq!(heading_part("## 参考文献"), Some(Part::References));
        assert_eq!(heading_part("\\section*{3.2 Attention}"), None);
        assert_eq!(heading_words("\\section*{IV. Results}"), "results");
        // not a paper: everything is 正文
        let (plain, _) = blocks("# 周报\n\n这周做了三件事。\n\n## 下周\n\n继续。");
        assert!(parts(&plain).iter().all(|x| *x == Part::Body));
        // a heading after the references starts the appendix
        let (tail, _) = blocks("\\title{X}\n\n\\section*{References}\n\n[1] A.\n\n\\section*{A Proof of Theorem 1}\n\nWe show.");
        assert_eq!(parts(&tail)[3..], [Part::Appendix, Part::Appendix]);
    }

    #[test]
    fn counts_for_checks() {
        let c = counts(PAPER);
        assert_eq!((c.display_math, c.tables, c.images, c.footnotes, c.headings), (1, 1, 1, 1, 4));
        assert_eq!(c.inline_math, 1);
        let md = counts("# T\n\n$x$ and $$y$$ and ![](a.png)\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n脚注[^1]\n\n[^1]: 注");
        assert_eq!((md.headings, md.inline_math, md.display_math, md.images, md.tables, md.footnotes), (1, 1, 1, 1, 1, 1));
        assert_eq!(counts("中文 ab").wide, 2);
        // money isn't math: 「\$370bn」 translated to 「3700 亿美元」 loses no formula
        assert_eq!(counts("worth \\(\\$ 5.4 \\mathrm{trn}\\), then \\$370bn and \\$1trn, \\$5bn").inline_math, 1);
        assert_eq!(counts("\\$5 and $x$").inline_math, 1);
    }

    /// The cut points earlier real runs' 原文.mmd lose to `across_asides`, and the chunk starts of a run
    /// that aren't cut points any more: POMELO_REPLAY=<folder of runs> [POMELO_REPLAY_JOB=<a document folder>]
    #[test]
    #[ignore]
    fn replay_cut_points() {
        let Ok(dir) = std::env::var("POMELO_REPLAY") else { return };
        let mut files = vec![];
        let mut stack = vec![std::path::PathBuf::from(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.file_name().is_some_and(|n| n == "原文.mmd") {
                    files.push(p);
                }
            }
        }
        files.sort();
        for f in &files {
            let (b, _) = blocks(&std::fs::read_to_string(f).unwrap());
            let lost: Vec<usize> = (1..b.len()).filter(|&i| across_asides(&b, i) && cuttable_before(&b, i)).collect();
            eprintln!("[replay] {}：{} 块，{} 个切点，少了 {} 个", f.parent().unwrap().display(), b.len(), (0..b.len()).filter(|&i| cuttable(&b, i)).count(), lost.len());
            for i in lost {
                eprintln!("    第 {i} 块前：「{}」|「{}」", b[i - 1].text.chars().take(60).collect::<String>().replace('\n', " "), b[i].text.chars().take(60).collect::<String>().replace('\n', " "));
            }
        }
        if let Ok(job) = std::env::var("POMELO_REPLAY_JOB") {
            let folder = std::path::Path::new(&job);
            let (b, _) = blocks(&std::fs::read_to_string(folder.join("原文.mmd")).unwrap());
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(folder.join(".pub2md/job.json")).unwrap()).unwrap();
            for c in v["chunks"].as_array().unwrap() {
                let s = c["chunk"]["start"].as_u64().unwrap() as usize;
                if !cuttable(&b, s) {
                    eprintln!("[replay] 那次的块从第 {s} 块开始，现在不能在这儿切：「{}」", b[s].text.chars().take(80).collect::<String>().replace('\n', " "));
                } else if s > 0 && std::env::var("POMELO_REPLAY_SHOW").is_ok() {
                    let tail: String = b[s - 1].text.chars().rev().take(70).collect::<Vec<_>>().into_iter().rev().collect();
                    eprintln!("[replay] 第 {s} 块起：「…{}」|「{}」", tail.replace('\n', " "), b[s].text.chars().take(60).collect::<String>().replace('\n', " "));
                }
            }
        }
    }

    /// The rules before `across_asides`.
    fn cuttable_before(blocks: &[Block], i: usize) -> bool {
        let (prev, next) = (&blocks[i - 1], &blocks[i]);
        let figure = |k: Kind| matches!(k, Kind::Figure | Kind::Table);
        !(matches!(prev.kind, Kind::Heading | Kind::Title)
            || (figure(prev.kind) && next.kind == Kind::Caption)
            || (prev.kind == Kind::Caption && figure(next.kind))
            || (next.kind == Kind::Math && prev.kind == Kind::Paragraph)
            || (next.kind == Kind::Paragraph && starts_lower(&next.text))
            || (prev.kind == Kind::Paragraph && next.kind == Kind::Paragraph && !ends_sentence(&prev.text)))
    }
}
