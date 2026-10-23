//! Word (.docx) for Work (v0.4.5 阶段 9), no library beyond zip / quick-xml:
//! - `markdown_to_docx`: a reply as a document — headings (# → 标题 1…), paragraphs (a line break kept),
//!   bullet and numbered lists (two levels by indentation), tables, quotes, code blocks, bold / italic /
//!   code inside lines, links as 「文字（网址）」; the cited sources under 「出处」 at the end. Real Word
//!   styles (标题 1–3, 引用, 代码, 网格表), 苹方 for Chinese.
//! - `read_docx`: the body as numbered items for the model — each top-level paragraph (its text from
//!   the `w:t` runs, a tab / line break shown as \t / \n) with its formatting marked: # heading (its
//!   level), - / 1. list item, **bold**, *italic*, <u>underline</u> (direct formatting only; a literal
//!   * is \*); a table as one item, a line per row (「| 格 | 格 |」, marks as above). Fields (页码、目录…),
//!   content controls, text boxes and empty paragraphs can't be changed; nor can a table with a table
//!   inside, a | / tab / line break in a cell, nor a cell with several paragraphs or merged into
//!   the one above.
//! - `edit_docx`: new text for some paragraphs, written by changing only what differs (a character
//!   diff mapped onto the runs): unchanged words keep their run's formatting, inserted text takes the
//!   formatting of the text next to it; a run is split only where bold / italic / underline change
//!   (its other properties kept). A heading level or list comes from the document's own styles
//!   (标题 N by name, the numbering of a list paragraph nearby). A blank line in the new text starts
//!   new paragraphs after it (its properties, no section break); empty text deletes the paragraph
//!   (not one with a bookmark, comment, picture or section break). Everything else in the file —
//!   tables, images, comments, styles — stays byte for byte. A change to a tab or line break inside a
//!   paragraph is refused (it isn't text in a run). A table keeps its rows and cells: only their text
//!   changes (the same way; an empty cell gets a run with its paragraph mark's look).

use std::io::{Read, Write};

// ---------- writing a report ----------

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Span {
    text: String,
    bold: bool,
    italic: bool,
    code: bool,
}

/// **bold**, *italic* / _italic_, `code`, [text](url) → spans.
fn spans(line: &str) -> Vec<Span> {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<Span> = vec![];
    let (mut bold, mut italic) = (false, false);
    let mut cur = String::new();
    let flush = |out: &mut Vec<Span>, cur: &mut String, bold: bool, italic: bool, code: bool| {
        if !cur.is_empty() {
            out.push(Span { text: std::mem::take(cur), bold, italic, code });
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            if let Some(end) = chars[i + 1..].iter().position(|x| *x == '`') {
                flush(&mut out, &mut cur, bold, italic, false);
                out.push(Span { text: chars[i + 1..i + 1 + end].iter().collect(), bold, italic, code: true });
                i += end + 2;
                continue;
            }
        }
        if c == '*' && chars.get(i + 1) == Some(&'*') {
            flush(&mut out, &mut cur, bold, italic, false);
            bold = !bold;
            i += 2;
            continue;
        }
        if (c == '*' || c == '_') && {
            let prev = if i == 0 { ' ' } else { chars[i - 1] };
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            // a single * or _ opens before a word and closes after one (not inside snake_case or 2*3)
            if italic { !prev.is_whitespace() } else { !next.is_whitespace() && !prev.is_alphanumeric() }
        } && (italic || chars[i + 1..].contains(&c))
        {
            flush(&mut out, &mut cur, bold, italic, false);
            italic = !italic;
            i += 1;
            continue;
        }
        if c == '[' {
            let rest: String = chars[i..].iter().collect();
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close..].find(')') {
                    let text = &rest[1..close];
                    let url = &rest[close + 2..close + end];
                    if !text.is_empty() && !text.contains('[') && !url.contains(' ') {
                        cur.push_str(text);
                        if url != text {
                            cur.push_str(&format!("（{url}）"));
                        }
                        i += rest[..close + end + 1].chars().count();
                        continue;
                    }
                }
            }
        }
        cur.push(c);
        i += 1;
    }
    flush(&mut out, &mut cur, bold, italic, false);
    out
}

fn runs(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| {
            let mut pr = String::new();
            if s.bold {
                pr.push_str("<w:b/>");
            }
            if s.italic {
                pr.push_str("<w:i/>");
            }
            if s.code {
                pr.push_str(r#"<w:rFonts w:ascii="Menlo" w:hAnsi="Menlo"/><w:shd w:val="clear" w:color="auto" w:fill="F2F0EC"/>"#);
            }
            let pr = if pr.is_empty() { String::new() } else { format!("<w:rPr>{pr}</w:rPr>") };
            format!(r#"<w:r>{pr}<w:t xml:space="preserve">{}</w:t></w:r>"#, esc(&s.text))
        })
        .collect()
}

/// A paragraph: its style, list (num id, level), and lines (kept apart by line breaks).
fn para(style: Option<&str>, list: Option<(u32, u32)>, lines: &[&str]) -> String {
    let mut ppr = String::new();
    if let Some(s) = style {
        ppr.push_str(&format!(r#"<w:pStyle w:val="{s}"/>"#));
    }
    if let Some((num, lvl)) = list {
        ppr.push_str(&format!(r#"<w:numPr><w:ilvl w:val="{lvl}"/><w:numId w:val="{num}"/></w:numPr>"#));
    }
    let body: Vec<String> = lines.iter().map(|l| runs(&spans(l))).collect();
    let ppr = if ppr.is_empty() { String::new() } else { format!("<w:pPr>{ppr}</w:pPr>") };
    format!("<w:p>{ppr}{}</w:p>", body.join("<w:r><w:br/></w:r>"))
}

fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_rule_row(line: &str) -> bool {
    let c = cells(line);
    !c.is_empty() && c.iter().all(|x| !x.is_empty() && x.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

fn table(rows: &[Vec<String>]) -> String {
    let n = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let grid: String = (0..n).map(|_| r#"<w:gridCol w:w="2000"/>"#).collect();
    let mut out = format!(r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="5000" w:type="pct"/></w:tblPr><w:tblGrid>{grid}</w:tblGrid>"#);
    for (i, row) in rows.iter().enumerate() {
        out.push_str("<w:tr>");
        for j in 0..n {
            let text = row.get(j).map_or("", String::as_str);
            let mut sp = spans(text);
            if i == 0 {
                sp.iter_mut().for_each(|s| s.bold = true);
            }
            out.push_str(&format!(r#"<w:tc><w:tcPr><w:tcW w:w="0" w:type="auto"/></w:tcPr><w:p>{}</w:p></w:tc>"#, runs(&sp)));
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl><w:p/>");
    out
}

/// A list item: (indent level, ordered, text).
fn list_item(line: &str) -> Option<(u32, bool, &str)> {
    let indent = line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>();
    let t = line.trim_start();
    let lvl = ((indent / 2) as u32).min(2);
    for b in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(b) {
            return Some((lvl, false, rest));
        }
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits < 4 {
        let rest = &t[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((lvl, true, r));
        }
    }
    None
}

/// The body XML and how many numbered lists it started (each restarts at 1).
fn body(md: &str) -> (String, u32) {
    let lines: Vec<&str> = md.lines().collect();
    let mut out = String::new();
    let mut ordered_lists = 0u32;
    let mut in_ordered: Option<u32> = None;
    let mut i = 0;
    let mut text: Vec<&str> = vec![];
    let flush = |out: &mut String, text: &mut Vec<&str>| {
        if !text.is_empty() {
            out.push_str(&para(None, None, text));
            text.clear();
        }
    };
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            flush(&mut out, &mut text);
            let fence = &t[..3];
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with(fence) {
                out.push_str(&para(Some("Code"), None, &[lines[i]]));
                i += 1;
            }
            i += 1;
            in_ordered = None;
            continue;
        }
        if t.is_empty() {
            flush(&mut out, &mut text);
            i += 1;
            continue;
        }
        let hashes = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
            flush(&mut out, &mut text);
            out.push_str(&para(Some(&format!("Heading{}", hashes.min(3))), None, &[t[hashes..].trim()]));
            in_ordered = None;
            i += 1;
            continue;
        }
        if t.starts_with('|') && lines.get(i + 1).is_some_and(|n| is_rule_row(n)) {
            flush(&mut out, &mut text);
            let mut rows = vec![cells(t)];
            i += 2;
            while i < lines.len() && lines[i].trim().starts_with('|') {
                rows.push(cells(lines[i]));
                i += 1;
            }
            out.push_str(&table(&rows));
            in_ordered = None;
            continue;
        }
        if t.chars().all(|c| c == '-' || c == '*' || c == '_') && t.len() >= 3 {
            flush(&mut out, &mut text);
            i += 1;
            continue;
        }
        if let Some(q) = t.strip_prefix('>') {
            flush(&mut out, &mut text);
            out.push_str(&para(Some("Quote"), None, &[q.trim_start()]));
            i += 1;
            continue;
        }
        if let Some((lvl, ordered, item)) = list_item(line) {
            flush(&mut out, &mut text);
            let num = if ordered {
                if in_ordered.is_none() || lvl == 0 && i > 0 && list_item(lines[i - 1]).is_none() {
                    ordered_lists += 1;
                    in_ordered = Some(ordered_lists + 1);
                }
                in_ordered.unwrap_or(2)
            } else {
                1
            };
            out.push_str(&para(Some("ListParagraph"), Some((num, lvl)), &[item]));
            i += 1;
            continue;
        }
        in_ordered = None;
        text.push(t);
        i += 1;
    }
    flush(&mut out, &mut text);
    (out, ordered_lists)
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#;
const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
const DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/></Relationships>"#;
const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

fn styles() -> String {
    let heading = |n: u32, size: u32| {
        format!(
            r#"<w:style w:type="paragraph" w:styleId="Heading{n}"><w:name w:val="heading {n}"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="{}" w:after="120"/><w:outlineLvl w:val="{}"/></w:pPr><w:rPr><w:b/><w:sz w:val="{size}"/></w:rPr></w:style>"#,
            if n == 1 { 360 } else { 240 },
            n - 1
        )
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles {W}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Helvetica Neue" w:hAnsi="Helvetica Neue" w:eastAsia="PingFang SC" w:cs="Helvetica Neue"/><w:sz w:val="22"/><w:lang w:val="en-US" w:eastAsia="zh-CN"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="312" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>{}{}{}<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:ind w:left="567"/></w:pPr><w:rPr><w:i/><w:color w:val="666666"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0" w:line="264" w:lineRule="auto"/><w:shd w:val="clear" w:color="auto" w:fill="F2F0EC"/></w:pPr><w:rPr><w:rFonts w:ascii="Menlo" w:hAnsi="Menlo"/><w:sz w:val="18"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="60"/></w:pPr></w:style><w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="BFBFBF"/><w:left w:val="single" w:sz="4" w:space="0" w:color="BFBFBF"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="BFBFBF"/><w:right w:val="single" w:sz="4" w:space="0" w:color="BFBFBF"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="BFBFBF"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="BFBFBF"/></w:tblBorders><w:tblCellMar><w:left w:w="100" w:type="dxa"/><w:right w:w="100" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style></w:styles>"#,
        heading(1, 32),
        heading(2, 28),
        heading(3, 24)
    )
}

fn numbering(ordered_lists: u32) -> String {
    let levels = |ordered: bool| -> String {
        (0..3)
            .map(|l| {
                let (fmt, text) = if ordered { (["decimal", "lowerLetter", "lowerRoman"][l], format!("%{}.", l + 1)) } else { ("bullet", ["•", "◦", "▪"][l].to_string()) };
                format!(r#"<w:lvl w:ilvl="{l}"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/><w:lvlText w:val="{text}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{}" w:hanging="360"/></w:pPr></w:lvl>"#, 720 * (l + 1))
            })
            .collect()
    };
    let mut out = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering {W}><w:abstractNum w:abstractNumId="0">{}</w:abstractNum><w:abstractNum w:abstractNumId="1">{}</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#, levels(false), levels(true));
    for k in 0..ordered_lists.max(1) {
        out.push_str(&format!(r#"<w:num w:numId="{}"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="1"/></w:lvlOverride></w:num>"#, k + 2));
    }
    out.push_str("</w:numbering>");
    out
}

/// A reply (Markdown) as a .docx; `sources`: lines listed under 「出处」 at the end (none: no section).
pub fn markdown_to_docx(md: &str, sources: &[String]) -> Result<Vec<u8>, String> {
    let (mut body_xml, lists) = body(md);
    if !sources.is_empty() {
        body_xml.push_str(&para(Some("Heading1"), None, &["出处"]));
        for s in sources {
            body_xml.push_str(&para(None, None, &[&s.replace('\n', " ")]));
        }
    }
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document {W}><w:body>{body_xml}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let numbering_xml = numbering(lists);
    let styles_xml = styles();
    for (name, text) in [("[Content_Types].xml", CONTENT_TYPES), ("_rels/.rels", RELS), ("word/_rels/document.xml.rels", DOC_RELS), ("word/document.xml", document.as_str()), ("word/styles.xml", styles_xml.as_str()), ("word/numbering.xml", numbering_xml.as_str())] {
        w.start_file(name, opts).map_err(|e| e.to_string())?;
        w.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(w.finish().map_err(|e| e.to_string())?.into_inner())
}

// ---------- reading and changing a document ----------

/// Direct formatting of a run the model can see and change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Flags {
    b: bool,
    i: bool,
    u: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Number,
}

/// A run (`w:r`) of a paragraph: where it is, its properties and its formatting.
#[derive(Debug, Clone)]
struct Run {
    range: (usize, usize),
    rpr: Option<(usize, usize)>,
    flags: Flags,
    /// Only its properties and one text: it can be split to change formatting.
    simple: bool,
}

/// A `w:t` element's text: where its content is in document.xml, where its start tag is, its run.
#[derive(Debug, Clone)]
struct TextNode {
    tag: (usize, usize),
    content: (usize, usize),
    text: String,
    run: usize,
}

/// One character of a paragraph's text: in a text node (index, char offset), or a tab / break.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Slot {
    Text(usize, usize),
    Fixed,
}

/// A table cell: its paragraphs, and why its text can't change.
#[derive(Debug, Clone, Default)]
struct Cell {
    paras: Vec<Item>,
    locked: Option<&'static str>,
}

/// A numbered item of the body: a paragraph, or a whole table (only its cells' text changes).
#[derive(Debug, Clone)]
pub struct Item {
    /// As the model sees it: the text with its formatting marked (headings #, lists - / 1., **bold**,
    /// *italic*, <u>underline</u>).
    pub text: String,
    /// Why it can't be changed (表格、域、空段…), None = it can.
    pub fixed: Option<&'static str>,
    /// Why it can't be deleted whole (书签、批注、图片、分节), None = it can.
    keep: Option<&'static str>,
    plain: String,
    nodes: Vec<TextNode>,
    slots: Vec<Slot>,
    runs: Vec<Run>,
    /// The `w:p` element, its start tag and its `w:pPr`.
    range: (usize, usize),
    open: (usize, usize),
    ppr: Option<(usize, usize)>,
    pstyle: Option<String>,
    heading: u8,
    list: Option<ListKind>,
    /// Its `w:numPr` (another paragraph made a list item copies it).
    numpr: Option<String>,
    /// A table: its rows of cells.
    table: Option<Vec<Vec<Cell>>>,
}

impl Item {
    pub fn is_table(&self) -> bool {
        self.table.is_some()
    }
}

/// A table being read: its text (every cell, for one that can't change), where it started (depth,
/// offset), its rows and the row / cell being read, a table inside it.
#[derive(Default)]
struct TableRead {
    text: String,
    depth: usize,
    start: usize,
    rows: Vec<Vec<Cell>>,
    row: Vec<Cell>,
    cell: Option<Cell>,
    nested: bool,
}

/// A table's rows as written: 「| a | b |」 per line (outer bars optional; a ---- line skipped).
fn table_rows(s: &str) -> Vec<Vec<String>> {
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !(l.contains("---") && l.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))))
        .map(|l| {
            let l = l.strip_prefix('|').unwrap_or(l);
            let l = l.strip_suffix('|').unwrap_or(l);
            l.split('|').map(|c| c.trim().to_string()).collect()
        })
        .collect()
}

/// The table item: editable cells shown with their marks, a line per row.
fn table_item(t: TableRead, range: (usize, usize)) -> Item {
    let blank = Item { text: String::new(), fixed: None, keep: Some("表格"), plain: String::new(), nodes: vec![], slots: vec![], runs: vec![], range, open: range, ppr: None, pstyle: None, heading: 0, list: None, numpr: None, table: None };
    let odd = t.rows.iter().flatten().flat_map(|c| &c.paras).any(|p| p.plain.contains(['|', '\n', '\t']));
    if t.nested || odd || t.rows.is_empty() {
        return Item { text: t.text.trim_end().to_string(), fixed: Some("表格"), ..blank };
    }
    let mut rows = t.rows;
    for c in rows.iter_mut().flatten() {
        if c.locked.is_none() {
            c.locked = match c.paras.as_slice() {
                [] => Some("不是普通文字"),
                [p] => p.fixed.filter(|f| *f != "空段"),
                _ => Some("里有好几段"),
            };
        }
    }
    let cell_text = |c: &Cell| match c.paras.as_slice() {
        [p] if p.fixed.is_none() => p.text.clone(),
        ps => ps.iter().map(|p| p.plain.trim()).filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" "),
    };
    let text = rows.iter().map(|r| format!("| {} |", r.iter().map(cell_text).collect::<Vec<_>>().join(" | "))).collect::<Vec<_>>().join("\n");
    Item { text, table: Some(rows), ..blank }
}

/// What the document's own styles allow: heading styles by level, the list paragraph style, which
/// numberings are bullets.
#[derive(Debug, Clone, Default)]
pub struct Styles {
    headings: Vec<(u8, String)>,
    list_style: Option<String>,
    lists: std::collections::HashMap<String, ListKind>,
}

impl Styles {
    fn heading_of(&self, style: Option<&str>) -> u8 {
        let Some(s) = style else { return 0 };
        if let Some((n, _)) = self.headings.iter().find(|(_, id)| id == s) {
            return *n;
        }
        // no styles part: Word's own ids
        let lower = s.to_lowercase();
        lower.strip_prefix("heading").and_then(|n| n.trim().parse::<u8>().ok()).filter(|n| (1..=9).contains(n)).unwrap_or(0)
    }
    fn heading_id(&self, level: u8) -> Option<String> {
        self.headings.iter().find(|(n, _)| *n == level).map(|(_, id)| id.clone())
    }
}

/// The parsed document: its items and styles.
#[derive(Debug, Clone)]
pub struct Docx {
    pub items: Vec<Item>,
    styles: Styles,
    xml: String,
}

const DOCUMENT: &str = "word/document.xml";

fn bad() -> String {
    "读不了这个 Word 文件，可能已损坏或加了密码".into()
}

fn part(data: &[u8], name: &str) -> Result<Option<String>, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|_| bad())?;
    let Ok(f) = zip.by_name(name) else { return Ok(None) };
    let mut xml = String::new();
    f.take(100 * 1024 * 1024).read_to_string(&mut xml).map_err(|_| bad())?;
    Ok(Some(xml))
}

fn document_xml(data: &[u8]) -> Result<String, String> {
    part(data, DOCUMENT)?.ok_or_else(bad)
}

fn attr(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    e.attributes().flatten().find(|a| AsRef::<str>::as_ref(&a.key.local_name()) == key).and_then(|a| quick_xml::escape::unescape(&a.value).ok().map(|v| v.to_string()))
}

fn on(e: &quick_xml::events::BytesStart) -> bool {
    !matches!(attr(e, "val").as_deref(), Some("0" | "false" | "off" | "none"))
}

/// Heading styles (by their name, 「heading 1」…), the list paragraph style, bullet numberings.
fn styles_of(styles: Option<&str>, numbering: Option<&str>) -> Styles {
    use quick_xml::events::Event;
    let mut out = Styles::default();
    if let Some(xml) = styles {
        let mut r = quick_xml::Reader::from_str(xml);
        let mut id: Option<String> = None;
        while let Ok(ev) = r.read_event() {
            match ev {
                Event::Start(e) if AsRef::<str>::as_ref(&e.local_name()) == "style" => id = attr(&e, "styleId"),
                Event::Empty(e) if AsRef::<str>::as_ref(&e.local_name()) == "name" => {
                    let (Some(i), Some(name)) = (id.clone(), attr(&e, "val")) else { continue };
                    let name = name.to_lowercase();
                    if let Some(n) = name.strip_prefix("heading ").and_then(|n| n.parse::<u8>().ok()) {
                        out.headings.push((n, i));
                    } else if name == "list paragraph" {
                        out.list_style = Some(i);
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    if let Some(xml) = numbering {
        let mut r = quick_xml::Reader::from_str(xml);
        let (mut abs, mut num): (Option<String>, Option<String>) = (None, None);
        let mut level0 = false;
        let mut bullets: Vec<String> = vec![];
        let mut nums: Vec<(String, String)> = vec![];
        while let Ok(ev) = r.read_event() {
            match ev {
                Event::Start(e) => match AsRef::<str>::as_ref(&e.local_name()) {
                    "abstractNum" => abs = attr(&e, "abstractNumId"),
                    "num" => num = attr(&e, "numId"),
                    "lvl" => level0 = attr(&e, "ilvl").as_deref() == Some("0"),
                    _ => {}
                },
                Event::Empty(e) => match AsRef::<str>::as_ref(&e.local_name()) {
                    "numFmt" if level0 && attr(&e, "val").as_deref() == Some("bullet") => bullets.extend(abs.clone()),
                    "abstractNumId" => nums.extend(num.clone().zip(attr(&e, "val"))),
                    _ => {}
                },
                Event::Eof => break,
                _ => {}
            }
        }
        for (n, a) in nums {
            out.lists.insert(n, if bullets.contains(&a) { ListKind::Bullet } else { ListKind::Number });
        }
    }
    out
}

/// The body's items, from document.xml.
fn items(xml: &str, styles: &Styles) -> Result<Vec<Item>, String> {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<String> = vec![];
    let mut out: Vec<Item> = vec![];
    // the paragraph being read (top level) or the table (its text only)
    let mut cur: Option<Item> = None;
    let mut table: Option<TableRead> = None;
    // how deep the paragraph being read is (2: the body's; deeper: a table cell's)
    let mut p_depth = 2;
    let mut open_t: Option<(usize, usize)> = None; // tag range of the w:t being read
    let empty = |range: (usize, usize), open: (usize, usize)| Item { text: String::new(), fixed: None, keep: None, plain: String::new(), nodes: vec![], slots: vec![], runs: vec![], range, open, ppr: None, pstyle: None, heading: 0, list: None, numpr: None, table: None };
    // the run being read: start, rPr range, flags, texts, other content
    let mut run: Option<(usize, Option<(usize, usize)>, Flags, usize, usize)> = None;
    let (mut ppr_at, mut rpr_at, mut numpr_at) = (0, 0, 0);
    let mut num_id: Option<String> = None;
    loop {
        let before = r.buffer_position() as usize;
        let ev = r.read_event().map_err(|_| bad())?;
        let after = r.buffer_position() as usize;
        let in_p = cur.is_some();
        let parent = stack.last().cloned().unwrap_or_default();
        match &ev {
            Event::Start(e) | Event::Empty(e) => {
                let name: String = AsRef::<str>::as_ref(&e.local_name()).to_string();
                let top = stack.len() == 2 && stack[0] == "document" && stack[1] == "body";
                let start = matches!(ev, Event::Start(_));
                let in_cell = table.as_ref().is_some_and(|t| t.cell.is_some() && stack.len() == t.depth + 3);
                if (top || in_cell) && name == "p" {
                    let mut it = empty((before, after), (before, after));
                    if !start {
                        it.fixed = Some("空段");
                        match table.as_mut().and_then(|t| t.cell.as_mut()) {
                            Some(c) if in_cell => c.paras.push(it),
                            _ => out.push(it),
                        }
                    } else {
                        cur = Some(it);
                        num_id = None;
                        p_depth = stack.len();
                    }
                } else if name == "tbl" && start {
                    match table.as_mut() {
                        Some(t) => t.nested = true,
                        None if top => table = Some(TableRead { depth: stack.len(), start: before, ..Default::default() }),
                        None => {}
                    }
                } else if name == "tc" && start && table.as_ref().is_some_and(|t| stack.len() == t.depth + 2) {
                    if let Some(t) = table.as_mut() {
                        t.cell = Some(Cell::default());
                    }
                } else if name == "vMerge" && parent == "tcPr" && attr(e, "val").as_deref() != Some("restart") {
                    if let Some(c) = table.as_mut().and_then(|t| t.cell.as_mut()) {
                        c.locked = Some("和上面的格子合并了");
                    }
                } else if top && name == "sdt" && start {
                    out.push(Item { text: "（内容控件）".into(), fixed: Some("内容控件"), ..empty((before, after), (before, after)) });
                }
                // a text, in a paragraph or a table
                if name == "t" && start && stack.iter().any(|s| s == "r") {
                    open_t = Some((before, after));
                }
                if let Some(c) = cur.as_mut() {
                    let in_run = stack.iter().any(|s| s == "r");
                    match name.as_str() {
                        "pPr" if parent == "p" && stack.len() == p_depth + 1 => {
                            ppr_at = before;
                            if !start {
                                c.ppr = Some((before, after));
                            }
                        }
                        "pStyle" if parent == "pPr" => c.pstyle = attr(e, "val"),
                        "numPr" if parent == "pPr" => {
                            numpr_at = before;
                            if !start {
                                c.numpr = Some(xml[before..after].to_string());
                            }
                        }
                        "numId" if parent == "numPr" => num_id = attr(e, "val"),
                        "sectPr" if parent == "pPr" => c.keep = Some("分节符"),
                        "r" if start && !in_run => run = Some((before, None, Flags::default(), 0, 0)),
                        "rPr" if parent == "r" => {
                            rpr_at = before;
                            if !start {
                                if let Some(x) = run.as_mut() {
                                    x.1 = Some((before, after));
                                }
                            }
                        }
                        "b" | "i" | "u" if parent == "rPr" && in_run => {
                            if let Some(x) = run.as_mut() {
                                let v = on(e);
                                match name.as_str() {
                                    "b" => x.2.b = v,
                                    "i" => x.2.i = v,
                                    _ => x.2.u = v,
                                }
                            }
                        }
                        "t" if in_run && !start => {
                            // an empty w:t
                            if let Some(x) = run.as_mut() {
                                x.3 += 1;
                            }
                        }
                        "tab" if in_run && parent == "r" => {
                            c.plain.push('\t');
                            c.slots.push(Slot::Fixed);
                            if let Some(x) = run.as_mut() {
                                x.4 += 1;
                            }
                        }
                        "br" | "cr" if in_run && parent == "r" => {
                            c.plain.push('\n');
                            c.slots.push(Slot::Fixed);
                            if let Some(x) = run.as_mut() {
                                x.4 += 1;
                            }
                        }
                        "fldChar" | "fldSimple" | "instrText" => c.fixed = Some("域（页码、目录这类）"),
                        "txbxContent" => c.fixed = Some("文本框"),
                        "drawing" | "pict" | "object" => {
                            c.keep = Some("图片");
                            if let Some(x) = run.as_mut() {
                                x.4 += 1;
                            }
                        }
                        "bookmarkStart" | "bookmarkEnd" | "commentRangeStart" | "commentRangeEnd" | "commentReference" => c.keep = Some("书签或批注"),
                        n if in_run && parent == "r" && !matches!(n, "rPr" | "t" | "lastRenderedPageBreak") => {
                            if let Some(x) = run.as_mut() {
                                x.4 += 1;
                            }
                        }
                        _ => {}
                    }
                }
                if start {
                    stack.push(name);
                }
            }
            Event::End(e) => {
                let name: String = AsRef::<str>::as_ref(&e.local_name()).to_string();
                stack.pop();
                match name.as_str() {
                    "t" => {
                        if let Some((tag, start)) = open_t.take().map(|t| (t, t.1)) {
                            let raw = &xml[start..before];
                            let text = quick_xml::escape::unescape(raw).map_err(|_| bad())?.to_string();
                            if let Some(t) = table.as_mut() {
                                t.text.push_str(&text);
                            }
                            if let Some(c) = cur.as_mut() {
                                let idx = c.nodes.len();
                                for (k, ch) in text.chars().enumerate() {
                                    c.plain.push(ch);
                                    c.slots.push(Slot::Text(idx, k));
                                }
                                if let Some(x) = run.as_mut() {
                                    x.3 += 1;
                                }
                                c.nodes.push(TextNode { tag, content: (start, before), text, run: c.runs.len() });
                            }
                        }
                    }
                    "pPr" if in_p && stack.len() == p_depth + 1 => {
                        if let Some(c) = cur.as_mut() {
                            c.ppr = Some((ppr_at, after));
                        }
                    }
                    "numPr" if in_p => {
                        if let Some(c) = cur.as_mut() {
                            c.numpr = Some(xml[numpr_at..after].to_string());
                        }
                    }
                    "rPr" if run.is_some() && stack.last().is_some_and(|s| s == "r") => {
                        if let Some(x) = run.as_mut() {
                            x.1 = Some((rpr_at, after));
                        }
                    }
                    "r" if !stack.iter().any(|s| s == "r") => {
                        if let (Some(c), Some((s, rpr, flags, texts, others))) = (cur.as_mut(), run.take()) {
                            c.runs.push(Run { range: (s, after), rpr, flags, simple: texts == 1 && others == 0 });
                        }
                    }
                    "p" if in_p && stack.len() == p_depth => {
                        if let Some(mut c) = cur.take() {
                            c.range.1 = after;
                            if c.fixed.is_none() && c.nodes.iter().all(|n| n.text.is_empty()) {
                                c.fixed = Some("空段");
                            }
                            c.heading = styles.heading_of(c.pstyle.as_deref());
                            c.list = num_id.take().filter(|n| n != "0").map(|n| styles.lists.get(&n).copied().unwrap_or(ListKind::Number));
                            c.text = if c.fixed.is_some() { c.plain.clone() } else { render(&c) };
                            match table.as_mut().and_then(|t| t.cell.as_mut()) {
                                Some(cell) if p_depth > 2 => cell.paras.push(c),
                                _ => out.push(c),
                            }
                        }
                    }
                    "tc" => {
                        if let Some(t) = table.as_mut() {
                            t.text.push_str(" | ");
                            if stack.len() == t.depth + 2 {
                                if let Some(c) = t.cell.take() {
                                    t.row.push(c);
                                }
                            }
                        }
                    }
                    "tr" => {
                        if let Some(t) = table.as_mut() {
                            let row = t.text.trim_end().trim_end_matches('|').trim_end().to_string();
                            t.text = format!("{row}\n");
                            if stack.len() == t.depth + 1 {
                                let row = std::mem::take(&mut t.row);
                                t.rows.push(row);
                            }
                        }
                    }
                    "tbl" if table.as_ref().is_some_and(|t| t.depth == stack.len()) => {
                        if let Some(t) = table.take() {
                            let start = t.start;
                            out.push(table_item(t, (start, after)));
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// The formatting of each character of a paragraph (a tab / break: none).
fn flags_of(item: &Item) -> Vec<Flags> {
    item.slots.iter().map(|s| if let Slot::Text(n, _) = s { item.runs.get(item.nodes[*n].run).map_or(Flags::default(), |r| r.flags) } else { Flags::default() }).collect()
}

fn is_numbered(s: &str) -> bool {
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && (s[digits..].starts_with(". ") || s[digits..].starts_with(") "))
}

/// Marks: ** bold, * italic, <u> underline toggled where the formatting changes; a literal * is \*.
fn marked(chars: &[(char, Flags)]) -> String {
    let mut out = String::new();
    let mut now = Flags::default();
    let switch = |out: &mut String, now: &mut Flags, to: Flags| {
        if now.u && !to.u {
            out.push_str("</u>");
        }
        if now.i && !to.i {
            out.push('*');
        }
        if now.b && !to.b {
            out.push_str("**");
        }
        if !now.b && to.b {
            out.push_str("**");
        }
        if !now.i && to.i {
            out.push('*');
        }
        if !now.u && to.u {
            out.push_str("<u>");
        }
        *now = to;
    };
    for (c, f) in chars {
        // a tab / break keeps what is around it
        let f = if *c == '\t' || *c == '\n' { now } else { *f };
        if f != now {
            switch(&mut out, &mut now, f);
        }
        if *c == '*' {
            out.push('\\');
        }
        out.push(*c);
    }
    switch(&mut out, &mut now, Flags::default());
    out
}

/// A paragraph as the model sees it: # heading / - or 1. list, then the marked text.
fn render(item: &Item) -> String {
    let chars: Vec<(char, Flags)> = item.plain.chars().zip(flags_of(item)).collect();
    let body = marked(&chars);
    let prefix = if item.heading > 0 {
        format!("{} ", "#".repeat(item.heading as usize))
    } else {
        match item.list {
            Some(ListKind::Bullet) => "- ".into(),
            Some(ListKind::Number) => "1. ".into(),
            // plain text that would read as a mark
            None if body.starts_with('#') || body.starts_with("- ") || is_numbered(&body) => "\\".into(),
            None => String::new(),
        }
    };
    format!("{prefix}{body}")
}

/// One paragraph of new text: its heading level / list, and its characters with their formatting.
#[derive(Debug, Clone, PartialEq)]
struct NewPara {
    heading: u8,
    list: Option<ListKind>,
    chars: Vec<(char, Flags)>,
}

fn parse_para(s: &str) -> NewPara {
    let (mut heading, mut list) = (0u8, None);
    let mut rest = s;
    if let Some(r) = s.strip_prefix('\\').filter(|r| r.starts_with('#') || r.starts_with("- ") || is_numbered(r)) {
        rest = r;
    } else if s.starts_with('#') {
        let n = s.chars().take_while(|c| *c == '#').count();
        if (1..=9).contains(&n) && s[n..].starts_with(' ') {
            heading = n as u8;
            rest = s[n..].trim_start();
        }
    } else if let Some(r) = s.strip_prefix("- ") {
        list = Some(ListKind::Bullet);
        rest = r;
    } else if is_numbered(s) {
        list = Some(ListKind::Number);
        rest = s.split_once(' ').map_or("", |x| x.1);
    }
    let mut chars = vec![];
    let mut f = Flags::default();
    let cs: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let at = |t: &str| cs[i..].iter().take(t.chars().count()).collect::<String>() == t;
        if cs[i] == '\\' && cs.get(i + 1) == Some(&'*') {
            chars.push(('*', f));
            i += 2;
        } else if at("**") {
            f.b = !f.b;
            i += 2;
        } else if cs[i] == '*' {
            f.i = !f.i;
            i += 1;
        } else if at("<u>") {
            f.u = true;
            i += 3;
        } else if at("</u>") {
            f.u = false;
            i += 4;
        } else {
            chars.push((cs[i], f));
            i += 1;
        }
    }
    NewPara { heading, list, chars }
}

/// New text for an item: its paragraphs (a blank line starts another; not when the old text has one).
fn parse_new(item: &Item, new: &str) -> Vec<NewPara> {
    let pieces: Vec<&str> = if item.plain.contains("\n\n") { vec![new] } else { new.split("\n\n").map(|p| p.trim_matches('\n')).filter(|p| !p.trim().is_empty()).collect() };
    pieces.into_iter().map(parse_para).collect()
}

/// Edit operations from `old` to `new` over chars: (kind, old index or insert position, char).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Keep(usize),
    Del(usize),
    /// Insert before old index (old.len() = at the end).
    Ins(usize, char),
}

/// A character diff: common prefix / suffix, and the middle by longest common subsequence (a big
/// middle is replaced whole).
fn diff(old: &[char], new: &[char]) -> Vec<Op> {
    let pre = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suf = old[pre..].iter().rev().zip(new[pre..].iter().rev()).take_while(|(a, b)| a == b).count();
    let (o, n) = (&old[pre..old.len() - suf], &new[pre..new.len() - suf]);
    let mut ops: Vec<Op> = (0..pre).map(Op::Keep).collect();
    if o.len() * n.len() > 4_000_000 {
        ops.extend((0..o.len()).map(|i| Op::Del(pre + i)));
        ops.extend(n.iter().map(|c| Op::Ins(pre + o.len(), *c)));
    } else {
        let w = n.len() + 1;
        let mut t = vec![0u16; (o.len() + 1) * w];
        for i in (0..o.len()).rev() {
            for j in (0..n.len()).rev() {
                t[i * w + j] = if o[i] == n[j] { t[(i + 1) * w + j + 1] + 1 } else { t[(i + 1) * w + j].max(t[i * w + j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < o.len() || j < n.len() {
            if i < o.len() && j < n.len() && o[i] == n[j] {
                ops.push(Op::Keep(pre + i));
                i += 1;
                j += 1;
            } else if j < n.len() && (i == o.len() || t[i * w + j + 1] >= t[(i + 1) * w + j]) {
                ops.push(Op::Ins(pre + i, n[j]));
                j += 1;
            } else {
                ops.push(Op::Del(pre + i));
                i += 1;
            }
        }
    }
    ops.extend((old.len() - suf..old.len()).map(Op::Keep));
    ops
}

/// Each text node's new characters with their formatting; Err when a tab / break would change.
fn retext(item: &Item, new: &[(char, Flags)]) -> Result<Vec<Vec<(char, Flags)>>, String> {
    let old: Vec<char> = item.plain.chars().collect();
    let plain: Vec<char> = new.iter().map(|x| x.0).collect();
    let nodes: Vec<Vec<char>> = item.nodes.iter().map(|n| n.text.chars().collect()).collect();
    let run_flags = |n: usize| item.runs.get(item.nodes[n].run).map_or(Flags::default(), |r| r.flags);
    let mut kept: Vec<Vec<Option<Flags>>> = nodes.iter().map(|n| vec![None; n.len()]).collect();
    // per node: (insert before this char offset, char, formatting); offset = len: at its end
    let mut inserts: Vec<Vec<(usize, char, Flags)>> = vec![vec![]; nodes.len()];
    let mut j = 0;
    for op in diff(&old, &plain) {
        match op {
            Op::Keep(i) => {
                if let Slot::Text(n, k) = item.slots[i] {
                    kept[n][k] = Some(new[j].1);
                }
                j += 1;
            }
            Op::Del(i) => {
                if item.slots[i] == Slot::Fixed {
                    return Err("这段的改动碰到了制表符或换行，没法保留格式".into());
                }
            }
            Op::Ins(at, c) => {
                let f = new[j].1;
                j += 1;
                if c == '\t' || c == '\n' {
                    return Err("这段的改动加了制表符或换行，没法保留格式".into());
                }
                let at_text = |s: &Slot| if let Slot::Text(n, k) = s { Some((*n, *k)) } else { None };
                let prev = at.checked_sub(1).and_then(|i| at_text(&item.slots[i])).map(|(n, k)| (n, k + 1));
                let next = item.slots.get(at).and_then(at_text);
                // after the character before it (its formatting) — or before the next one when that
                // one's formatting is the one wanted and this is where two runs meet
                let spot = match (prev, next) {
                    (Some(p), Some(nx)) if p.1 == nodes[p.0].len() && nx.1 == 0 && run_flags(p.0) != f && run_flags(nx.0) == f => nx,
                    (Some(p), _) => p,
                    (None, Some(nx)) => nx,
                    (None, None) => item.slots[..at].iter().rev().find_map(at_text).map(|(n, k)| (n, k + 1)).ok_or("这一段没有文字可以改")?,
                };
                inserts[spot.0].push((spot.1, c, f));
            }
        }
    }
    Ok(nodes
        .iter()
        .enumerate()
        .map(|(n, chars)| {
            let mut out = vec![];
            for k in 0..=chars.len() {
                out.extend(inserts[n].iter().filter(|x| x.0 == k).map(|x| (x.1, x.2)));
                if let Some(f) = kept[n].get(k).copied().flatten() {
                    out.push((chars[k], f));
                }
            }
            out
        })
        .collect())
}

// The schema's order of paragraph and run properties (Word refuses some files when it's off).
const PPR_ORDER: &[&str] = &["pStyle", "keepNext", "keepLines", "pageBreakBefore", "framePr", "widowControl", "numPr", "suppressLineNumbers", "pBdr", "shd", "tabs", "suppressAutoHyphens", "kinsoku", "wordWrap", "overflowPunct", "topLinePunct", "autoSpaceDE", "autoSpaceDN", "bidi", "adjustRightInd", "snapToGrid", "spacing", "ind", "contextualSpacing", "mirrorIndents", "suppressOverlap", "jc", "textDirection", "textAlignment", "textboxTightWrap", "outlineLvl", "divId", "cnfStyle", "rPr", "sectPr", "pPrChange"];
const RPR_ORDER: &[&str] = &["rStyle", "rFonts", "b", "bCs", "i", "iCs", "caps", "smallCaps", "strike", "dstrike", "outline", "shadow", "emboss", "imprint", "noProof", "snapToGrid", "vanish", "webHidden", "color", "spacing", "w", "kern", "position", "sz", "szCs", "highlight", "u", "effect", "bdr", "shd", "fitText", "vertAlign", "rtl", "cs", "em", "lang", "eastAsianLayout", "specVanish", "oMath"];

/// The child elements of a properties element (`<w:pPr>…</w:pPr>`): (name, its XML).
fn children(props: &str) -> Vec<(String, String)> {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(props);
    let mut out = vec![];
    let mut depth = 0;
    let mut start: Option<(String, usize)> = None;
    loop {
        let before = r.buffer_position() as usize;
        let Ok(ev) = r.read_event() else { break };
        let after = r.buffer_position() as usize;
        match ev {
            Event::Start(e) => {
                depth += 1;
                if depth == 2 {
                    start = Some((AsRef::<str>::as_ref(&e.local_name()).to_string(), before));
                }
            }
            Event::Empty(e) if depth == 1 => out.push((AsRef::<str>::as_ref(&e.local_name()).to_string(), props[before..after].to_string())),
            Event::End(_) => {
                if depth == 2 {
                    if let Some((n, s)) = start.take() {
                        out.push((n, props[s..after].to_string()));
                    }
                }
                depth -= 1;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

/// Properties put back in the schema's order (an unknown one after the known ones).
fn props(tag: &str, mut kids: Vec<(String, String)>, order: &[&str]) -> String {
    if kids.is_empty() {
        return String::new();
    }
    kids.sort_by_key(|(n, _)| order.iter().position(|o| o == n).unwrap_or(order.len()));
    format!("<w:{tag}>{}</w:{tag}>", kids.into_iter().map(|k| k.1).collect::<String>())
}

/// Run properties with bold / italic / underline as `to` (what `was` already had stays as written).
fn run_props(rpr: &str, was: Flags, to: Flags) -> String {
    let mut kids = children(rpr);
    for (on, was, tag, add) in [(to.b, was.b, "b", "<w:b/>"), (to.i, was.i, "i", "<w:i/>"), (to.u, was.u, "u", r#"<w:u w:val="single"/>"#)] {
        if on == was {
            continue;
        }
        kids.retain(|(n, _)| n != tag);
        if on {
            kids.push((tag.to_string(), add.to_string()));
        }
    }
    props("rPr", kids, RPR_ORDER)
}

/// A new run: `text` (tabs and breaks as theirs) with `rpr` as its properties.
fn new_run(rpr: &str, text: &[char]) -> String {
    let mut out = format!("<w:r>{rpr}");
    let mut t = String::new();
    let flush = |out: &mut String, t: &mut String| {
        if !t.is_empty() {
            out.push_str(&format!(r#"<w:t xml:space="preserve">{}</w:t>"#, esc(t)));
            t.clear();
        }
    };
    for c in text {
        match c {
            '\t' => {
                flush(&mut out, &mut t);
                out.push_str("<w:tab/>");
            }
            '\n' => {
                flush(&mut out, &mut t);
                out.push_str("<w:br/>");
            }
            c => t.push(*c),
        }
    }
    flush(&mut out, &mut t);
    out + "</w:r>"
}

/// Runs for characters with their formatting, from base properties (`rpr`, formatting `was`).
fn runs_for(rpr: &str, was: Flags, chars: &[(char, Flags)]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let f = chars[i].1;
        let n = chars[i..].iter().take_while(|x| x.1 == f).count();
        let text: Vec<char> = chars[i..i + n].iter().map(|x| x.0).collect();
        out.push_str(&new_run(&run_props(rpr, was, f), &text));
        i += n;
    }
    out
}

/// The paragraph's properties as `para` wants them (heading level, list), in the document's own
/// styles; `numpr`: a list paragraph's numbering to copy when it becomes one.
fn para_props(ppr: &str, item_style: Option<&str>, was: (u8, Option<ListKind>), para: &NewPara, styles: &Styles, numpr: impl Fn(ListKind) -> Option<String>) -> Result<String, String> {
    let mut kids: Vec<(String, String)> = children(ppr).into_iter().filter(|(n, _)| n != "sectPr").collect();
    let set = |kids: &mut Vec<(String, String)>, name: &str, xml: Option<String>| {
        kids.retain(|(n, _)| n != name);
        if let Some(x) = xml {
            kids.push((name.to_string(), x));
        }
    };
    let style_xml = |id: &str| format!(r#"<w:pStyle w:val="{}"/>"#, esc(id));
    if para.heading != was.0 {
        if para.heading > 0 {
            let id = styles.heading_id(para.heading).or_else(|| (styles.headings.is_empty() && item_style.is_some_and(|s| s.to_lowercase().starts_with("heading"))).then(|| format!("Heading{}", para.heading)));
            let id = id.ok_or_else(|| format!("文档里没有「标题 {}」样式，没法改成标题", para.heading))?;
            set(&mut kids, "pStyle", Some(style_xml(&id)));
        } else {
            set(&mut kids, "pStyle", None);
        }
    }
    if para.list != was.1 {
        match para.list {
            Some(kind) => {
                let n = numpr(kind).ok_or_else(|| format!("文档里没有{}可以照着，没法改成列表", if kind == ListKind::Bullet { "项目符号列表" } else { "编号列表" }))?;
                set(&mut kids, "numPr", Some(n));
                if para.heading == 0 && !kids.iter().any(|(n, _)| n == "pStyle") {
                    if let Some(ls) = &styles.list_style {
                        set(&mut kids, "pStyle", Some(style_xml(ls)));
                    }
                }
            }
            None => {
                set(&mut kids, "numPr", None);
                if styles.list_style.is_some() && item_style == styles.list_style.as_deref() && para.heading == 0 {
                    set(&mut kids, "pStyle", None);
                }
            }
        }
    }
    Ok(props("pPr", kids, PPR_ORDER))
}

type Patch = ((usize, usize), String);

/// Bold / italic / underline a run's properties give.
fn flags_in(rpr: &str) -> Flags {
    let mut f = Flags::default();
    for (name, xml) in children(rpr) {
        let on = !["\"0\"", "\"false\"", "\"off\"", "\"none\""].iter().any(|v| xml.contains(&format!("w:val={v}")));
        match name.as_str() {
            "b" => f.b = on,
            "i" => f.i = on,
            "u" => f.u = on,
            _ => {}
        }
    }
    f
}

/// Patches for a paragraph's text becoming `chars` (`which`: how errors name it).
fn text_patches(xml: &str, item: &Item, chars: &[(char, Flags)], which: &str) -> Result<Vec<Patch>, String> {
    let mut out: Vec<Patch> = vec![];
    for (node, chars) in item.nodes.iter().zip(retext(item, chars)?) {
        let run = &item.runs[node.run];
        if chars.iter().all(|c| c.1 == run.flags) {
            let text: String = chars.iter().map(|c| c.0).collect();
            if text == node.text {
                continue;
            }
            let tag = &xml[node.tag.0..node.tag.1];
            if !tag.contains("xml:space") && (text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace)) {
                out.push(((node.tag.0, node.tag.1), tag.replacen("<w:t", r#"<w:t xml:space="preserve""#, 1)));
            }
            out.push((node.content, esc(&text)));
        } else {
            if !run.simple {
                return Err(format!("{which}有一处格式改不了（那里不只有文字），只能改文字"));
            }
            let rpr = run.rpr.map_or("", |(a, b)| &xml[a..b]);
            out.push((run.range, runs_for(rpr, run.flags, &chars)));
        }
    }
    Ok(out)
}

/// A table cell's paragraph becoming `new` (its text and bold / italic / underline only).
fn plan_cell(xml: &str, item: &Item, new: &str, which: &str) -> Result<Vec<Patch>, String> {
    let para = parse_para(new);
    if (para.heading, para.list) != (item.heading, item.list) || para.chars.iter().any(|c| c.0 == '\t' || c.0 == '\n') {
        return Err(format!("{which}只能改字和加粗、斜体、下划线"));
    }
    if !item.nodes.is_empty() {
        return text_patches(xml, item, &para.chars, which);
    }
    if para.chars.is_empty() {
        return Ok(vec![]);
    }
    // an empty cell: a run with the paragraph mark's look
    let mark = item.ppr.map(|(a, b)| &xml[a..b]).and_then(|p| children(p).into_iter().find(|(n, _)| n == "rPr")).map(|(_, x)| x).unwrap_or_default();
    let runs = runs_for(&mark, flags_in(&mark), &para.chars);
    let tag = &xml[item.open.0..item.open.1];
    Ok(vec![match item.ppr {
        Some((_, b)) => ((b, b), runs),
        None if tag.ends_with("/>") => (item.range, format!("{}>{runs}</w:p>", tag[..tag.len() - 2].trim_end())),
        None => ((item.open.1, item.open.1), runs),
    }])
}

/// Item `i`, a table, becoming `new`: the same rows and cells, the changed cells' text.
fn plan_table(xml: &str, item: &Item, rows: &[Vec<Cell>], i: usize, new: &str) -> Result<Vec<Patch>, String> {
    let n = i + 1;
    if new.trim().is_empty() {
        return Err(format!("第 {n} 段是表格，不能整个删掉"));
    }
    let (old, want) = (table_rows(&item.text), table_rows(new));
    let shape = |r: &[Vec<String>]| r.iter().map(Vec::len).collect::<Vec<_>>();
    if shape(&want) != shape(&old) {
        return Err(format!("第 {n} 段是表格：只能改格子里的字，行数、每行的格数不变（原来 {} 行，一行一行写，格子用 | 隔开）", old.len()));
    }
    let mut out = vec![];
    for (r, row) in want.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            if *text == old[r][c] {
                continue;
            }
            let which = format!("第 {n} 段表格第 {} 行第 {} 格", r + 1, c + 1);
            let cell = &rows[r][c];
            if let Some(why) = cell.locked {
                return Err(format!("{which}{why}，不能改"));
            }
            out.extend(plan_cell(xml, &cell.paras[0], text, &which)?);
        }
    }
    Ok(out)
}

/// The changes to document.xml for item `i` becoming `new` (an empty `new` deletes it).
fn plan(xml: &str, doc: &Docx, i: usize, new: &str) -> Result<Vec<Patch>, String> {
    let item = doc.items.get(i).ok_or("没有这一段")?;
    if let Some(why) = item.fixed {
        return Err(format!("第 {} 段是{why}，不能改", i + 1));
    }
    if let Some(rows) = &item.table {
        return plan_table(xml, item, rows, i, new);
    }
    if new.trim().is_empty() {
        if let Some(why) = item.keep {
            return Err(format!("第 {} 段有{why}，不能整段删", i + 1));
        }
        return Ok(vec![(item.range, String::new())]);
    }
    let paras = parse_new(item, new);
    let first = &paras[0];
    let numpr = |kind: ListKind| {
        let near = doc.items[..i].iter().rev().chain(doc.items[i + 1..].iter());
        near.filter(|x| x.list == Some(kind)).find_map(|x| x.numpr.clone()).or_else(|| (item.list == Some(kind)).then(|| item.numpr.clone()).flatten())
    };
    let ppr_xml = item.ppr.map_or("", |(a, b)| &xml[a..b]);
    // the paragraph's own text: changed characters only, a run split where its formatting changes
    let mut out: Vec<Patch> = text_patches(xml, item, &first.chars, &format!("第 {} 段", i + 1))?;
    if (first.heading, first.list) != (item.heading, item.list) {
        let p = para_props(ppr_xml, item.pstyle.as_deref(), (item.heading, item.list), first, &doc.styles, &numpr)?;
        match item.ppr {
            Some(r) => out.push((r, p)),
            None => out.push(((item.open.1, item.open.1), p)),
        }
    }
    // paragraphs after it: its properties (no section break), its first run's look
    if paras.len() > 1 {
        let base = item.nodes.first().and_then(|n| item.runs.get(n.run));
        let (rpr, was) = base.map_or(("", Flags::default()), |r| (r.rpr.map_or("", |(a, b)| &xml[a..b]), r.flags));
        let mut added = String::new();
        for para in &paras[1..] {
            let p = para_props(ppr_xml, item.pstyle.as_deref(), (item.heading, item.list), para, &doc.styles, &numpr)?;
            added.push_str(&format!("<w:p>{p}{}</w:p>", runs_for(rpr, was, &para.chars)));
        }
        out.push(((item.range.1, item.range.1), added));
    }
    Ok(out)
}

/// The document of a .docx: its numbered items (1-based as the model sees them) and its styles.
pub fn read_docx(data: &[u8]) -> Result<Docx, String> {
    let styles = styles_of(part(data, "word/styles.xml")?.as_deref(), part(data, "word/numbering.xml")?.as_deref());
    let xml = document_xml(data)?;
    Ok(Docx { items: items(&xml, &styles)?, styles, xml })
}

/// Whether item `i` can become `new` with the document's formatting kept (the reason when not).
pub fn check(doc: &Docx, i: usize, new: &str) -> Result<(), String> {
    plan(&doc.xml, doc, i, new).map(|_| ())
}

/// document.xml with `edits` ((item index, new text)) made.
fn edited_xml(doc: &Docx, edits: &[(usize, String)]) -> Result<String, String> {
    let xml = &doc.xml;
    let mut changes: Vec<Patch> = vec![];
    for (i, new) in edits {
        changes.extend(plan(xml, doc, *i, new)?);
    }
    // from the end, so earlier offsets stay right (an insertion right after a replaced range goes last)
    changes.sort_by(|a, b| b.0 .0.cmp(&a.0 .0).then(b.0 .1.cmp(&a.0 .1)));
    let mut out = xml.to_string();
    for ((a, b), text) in changes {
        out.replace_range(a..b, &text);
    }
    Ok(out)
}

/// The .docx with `edits` ((item index, new text)) made; every other part of the file as it was.
pub fn edit_docx(data: &[u8], edits: &[(usize, String)]) -> Result<Vec<u8>, String> {
    let xml = edited_xml(&read_docx(data)?, edits)?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|_| bad())?;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let name = zip.name_for_index(i).ok_or_else(bad)?.map_err(|_| bad())?.to_string();
        if name == DOCUMENT {
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            w.start_file(name, opts).map_err(|e| e.to_string())?;
            w.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
        } else {
            let f = zip.by_index_raw(i).map_err(|_| bad())?;
            w.raw_copy_file(f).map_err(|e| e.to_string())?;
        }
    }
    Ok(w.finish().map_err(|e| e.to_string())?.into_inner())
}

// ---------- Seeker: a resume from the user's template ----------

/// The template's paragraphs (None: an empty one, not to be chosen). Only plain paragraphs: a table,
/// content control, field, text box, image or section break in the body is refused.
pub fn resume_template_items(data: &[u8]) -> Result<Vec<Option<String>>, String> {
    let doc = read_docx(data)?;
    if doc.items.iter().all(|p| p.plain.trim().is_empty()) {
        return Err("模板里没有文字".into());
    }
    for p in &doc.items {
        let why = if p.is_table() { Some("表格") } else { p.fixed.filter(|f| *f != "空段").or(p.keep.filter(|k| matches!(*k, "图片" | "分节符"))) };
        if let Some(why) = why {
            return Err(format!("模板里有{why}，请换一份只用段落排版的模板"));
        }
    }
    Ok(doc.items.iter().map(|p| (!p.plain.trim().is_empty()).then(|| p.plain.clone())).collect())
}

/// How a template paragraph looks: its properties, a typed bullet before the text, and per column
/// (split by tabs) the run properties of its first character, of its first plain and first bold one.
struct Look {
    ppr: String,
    prefix: Option<(String, String)>,
    cols: Vec<Column>,
    bold: Option<String>,
}

#[derive(Default, Clone)]
struct Column {
    first: String,
    first_bold: bool,
    plain: Option<String>,
    bold: Option<String>,
}

fn look(doc: &Docx, item: &Item) -> Look {
    let rpr_of = |run: usize| item.runs.get(run).map_or((String::new(), false), |r| (r.rpr.map_or(String::new(), |(a, b)| doc.xml[a..b].to_string()), r.flags.b));
    let chars: Vec<(char, Option<usize>)> = item.plain.chars().zip(item.slots.iter()).map(|(c, s)| (c, if let Slot::Text(n, _) = s { Some(item.nodes[*n].run) } else { None })).collect();
    let mut i = chars.iter().take_while(|c| c.0.is_whitespace() && c.0 != '\t').count();
    let mut prefix = None;
    if let Some(&(_, Some(run))) = chars.get(i).filter(|c| matches!(c.0, '•' | '●' | '▪' | '◦' | '·' | '‣' | '-' | '–')) {
        let n = chars[i + 1..].iter().take_while(|c| c.0 == ' ' || c.0 == '\u{3000}' || c.0 == '\t').count();
        if n > 0 {
            prefix = Some((chars[i..=i + n].iter().map(|c| c.0).collect::<String>().replace('\t', " "), rpr_of(run).0));
            i += n + 1;
        }
    }
    let mut cols = vec![Column::default()];
    let mut started = false;
    let mut bold = None;
    for &(c, run) in &chars[i..] {
        if c == '\t' {
            cols.push(Column::default());
            started = false;
            continue;
        }
        let (Some(run), false) = (run, c.is_whitespace()) else { continue };
        let (rpr, b) = rpr_of(run);
        let col = cols.last_mut().unwrap();
        if !started {
            (col.first, col.first_bold, started) = (rpr.clone(), b, true);
        }
        if b {
            col.bold.get_or_insert_with(|| rpr.clone());
            bold.get_or_insert(rpr);
        } else {
            col.plain.get_or_insert(rpr);
        }
    }
    Look { ppr: item.ppr.map_or(String::new(), |(a, b)| doc.xml[a..b].to_string()), prefix, cols, bold }
}

fn text_run(rpr: &str, text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    format!(r#"<w:r>{rpr}<w:t xml:space="preserve">{}</w:t></w:r>"#, esc(text))
}

/// A paragraph like `look` with `text`: tabs and line breaks as theirs, a column's look per column;
/// with `**粗体**` marks, the marked parts bold and the rest plain.
fn styled(look: &Look, text: &str) -> String {
    let bold_on = Flags { b: true, ..Default::default() };
    let mut runs = String::new();
    if let Some((prefix, rpr)) = &look.prefix {
        runs.push_str(&text_run(rpr, prefix));
    }
    for (l, line) in text.split('\n').enumerate() {
        if l > 0 {
            runs.push_str("<w:r><w:br/></w:r>");
        }
        for (n, part) in line.split('\t').enumerate() {
            if n > 0 {
                runs.push_str("<w:r><w:tab/></w:r>");
            }
            let col = look.cols.get(n).or(look.cols.last()).cloned().unwrap_or_default();
            let marked = part.contains("**");
            let plain = col.plain.clone().unwrap_or_else(|| if col.first_bold { run_props(&col.first, bold_on, Flags::default()) } else { col.first.clone() });
            let bold = col.bold.clone().or(look.bold.clone()).unwrap_or_else(|| run_props(&col.first, Flags::default(), bold_on));
            for (k, piece) in part.split("**").enumerate() {
                let rpr = match (marked, k % 2 == 1) {
                    (true, true) => &bold,
                    (true, false) => &plain,
                    // unmarked: a column all in one look keeps it; a mixed one (小标题 + 正文) is its 正文
                    (false, _) if col.bold.is_some() && col.plain.is_some() => &plain,
                    (false, _) => &col.first,
                };
                runs.push_str(&text_run(rpr, piece));
            }
        }
    }
    format!("<w:p>{}{runs}</w:p>", look.ppr)
}

/// The template with its body replaced by `lines` (template paragraph, text), each written like
/// that paragraph; page setup, styles, numbering, headers and every other part stay as they are.
pub fn resume_from_template(data: &[u8], lines: &[(usize, String)]) -> Result<Vec<u8>, String> {
    resume_template_items(data)?;
    let doc = read_docx(data)?;
    let mut body = String::new();
    for (index, text) in lines {
        let item = doc.items.get(*index).filter(|p| !p.plain.trim().is_empty()).ok_or("模板刚刚改过，请重新选择")?;
        body.push_str(&styled(&look(&doc, item), text));
    }
    let (first, last) = (doc.items.first().ok_or("模板里没有文字")?, doc.items.last().ok_or("模板里没有文字")?);
    let mut xml = doc.xml.clone();
    xml.replace_range(first.range.0..last.range.1, &body);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|_| bad())?;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let name = zip.name_for_index(i).ok_or_else(bad)?.map_err(|_| bad())?.to_string();
        if name == DOCUMENT {
            writer.start_file(name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).map_err(|e| e.to_string())?;
            writer.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
        } else {
            writer.raw_copy_file(zip.by_index_raw(i).map_err(|_| bad())?).map_err(|e| e.to_string())?;
        }
    }
    Ok(writer.finish().map_err(|e| e.to_string())?.into_inner())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Text of a .docx through macOS's own reader (proves Word-like apps can open it).
    pub fn textutil_text(dir: &std::path::Path, data: &[u8]) -> String {
        let p = dir.join("check.docx");
        std::fs::write(&p, data).unwrap();
        let out = std::process::Command::new("/usr/bin/textutil").args(["-convert", "txt", "-stdout"]).arg(&p).output().unwrap();
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    #[test]
    fn spans_read_inline_marks() {
        let s = spans("先 **加粗** 再 *斜体* 和 `code`，链接 [官网](https://a.com)，snake_case 和 2*3 不算");
        assert!(s.iter().any(|x| x.bold && x.text == "加粗"));
        assert!(s.iter().any(|x| x.italic && x.text == "斜体"));
        assert!(s.iter().any(|x| x.code && x.text == "code"));
        let all: String = s.iter().map(|x| x.text.as_str()).collect();
        assert!(all.contains("官网（https://a.com）") && all.contains("snake_case") && all.contains("2*3"), "{all}");
    }

    #[test]
    fn a_report_opens_with_its_structure() {
        let dir = crate::db::tests::temp_dir("word-report");
        let md = "# 租房调研\n\n结论：选朝阳那套 [1]。\n第二行。\n\n## 比较\n\n| 房子 | 月租 |\n|---|---|\n| 朝阳 | 6500 |\n| 海淀 | 5200 |\n\n- 采光好\n  - 朝南\n- 通勤 35 分钟\n\n1. 先签合同\n2. 再搬家\n\n> 中介说可以再谈。\n\n```\nrent = 6500\n```\n";
        let data = markdown_to_docx(md, &["[1] 看房记录.md · 结论".into()]).unwrap();
        let text = textutil_text(&dir, &data);
        for want in ["租房调研", "结论：选朝阳那套 [1]。", "第二行。", "比较", "朝阳", "6500", "采光好", "朝南", "先签合同", "中介说可以再谈。", "rent = 6500", "出处", "[1] 看房记录.md · 结论"] {
            assert!(text.contains(want), "missing {want} in {text}");
        }
        // headings are real Word headings; the numbered list restarts
        let xml = document_xml(&data).unwrap();
        assert!(xml.contains(r#"<w:pStyle w:val="Heading1"/>"#) && xml.contains(r#"<w:pStyle w:val="Heading2"/>"#) && xml.contains("<w:tbl>"));
        assert!(xml.contains(r#"<w:numId w:val="2"/>"#) && xml.contains(r#"<w:ilvl w:val="1"/>"#));
    }

    /// A document with formatting inside paragraphs, a table, a tab and a field.
    pub fn sample_docx() -> Vec<u8> {
        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>求职信</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve">我对贵公司的 </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>产品经理</w:t></w:r><w:r><w:t xml:space="preserve"> 岗位很感兴趣。</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>技能</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Rust &amp; SQL</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>电话</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>123</w:t></w:r></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:t>1</w:t></w:r></w:p><w:p/><w:sectPr/></w:body></w:document>"#
        );
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, text) in [("[Content_Types].xml", CONTENT_TYPES), ("_rels/.rels", RELS), ("word/document.xml", document.as_str()), ("word/extra.bin", "keep me")] {
            w.start_file(name, opts).unwrap();
            w.write_all(text.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn a_document_reads_as_items_and_edits_keep_formatting() {
        let data = sample_docx();
        let list = read_docx(&data).unwrap().items;
        let shown: Vec<(&str, Option<&str>)> = list.iter().map(|i| (i.text.as_str(), i.fixed)).collect();
        assert_eq!(
            shown,
            [("# 求职信", None), ("我对贵公司的 **产品经理** 岗位很感兴趣。", None), ("| 技能 | Rust & SQL |", None), ("电话\t123", None), ("1", Some("域（页码、目录这类）")), ("", Some("空段"))]
        );
        // words changed inside the bold run stay bold; inserted text takes what comes before it
        let out = edit_docx(&data, &[(1, "我对贵公司的 高级**产品负责人** 岗位非常感兴趣。".into())]).unwrap();
        let xml = document_xml(&out).unwrap();
        assert!(xml.contains(r#"<w:t xml:space="preserve">我对贵公司的 高级</w:t>"#) && xml.contains(r#"<w:rPr><w:b/></w:rPr><w:t>产品负责人</w:t>"#), "{xml}");
        assert!(xml.contains("岗位非常感兴趣。"));
        assert_eq!(read_docx(&out).unwrap().items[1].text, "我对贵公司的 高级**产品负责人** 岗位非常感兴趣。");
        // everything else as it was: the table, the other paragraphs, the other parts of the file
        assert!(xml.contains("Rust &amp; SQL") && xml.contains("<w:instrText>PAGE</w:instrText>"));
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&out)).unwrap();
        let mut extra = String::new();
        z.by_name("word/extra.bin").unwrap().read_to_string(&mut extra).unwrap();
        assert_eq!(extra, "keep me");
        // a tab stays a tab; changing it is refused; fixed items are refused
        let out = edit_docx(&data, &[(3, "手机\t+86 123".into())]).unwrap();
        assert_eq!(read_docx(&out).unwrap().items[3].text, "手机\t+86 123");
        assert!(edit_docx(&data, &[(3, "手机 123".into())]).unwrap_err().contains("制表符"));
        assert!(edit_docx(&data, &[(2, "x".into())]).unwrap_err().contains("表格"));
        assert!(edit_docx(&data, &[(2, "".into())]).unwrap_err().contains("不能整个删掉"));
        assert!(edit_docx(&data, &[(4, "2".into())]).is_err());
        // special characters are escaped; leading spaces kept; a literal * reads as \*
        let out = edit_docx(&data, &[(0, "#  求职信 <草稿> & 2\\*3".into())]).unwrap();
        assert_eq!(read_docx(&out).unwrap().items[0].text, "# 求职信 <草稿> & 2\\*3");
        assert!(document_xml(&out).unwrap().contains("求职信 &lt;草稿&gt; &amp; 2*3</w:t>"));
    }

    /// A Word-made document: styles (标题 1–2, 列表段落), a bullet and a numbered list, a bookmark.
    pub fn styled_docx() -> Vec<u8> {
        let styles = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles {W}><w:style w:type="paragraph" w:styleId="a"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="2"><w:name w:val="heading 2"/></w:style><w:style w:type="paragraph" w:styleId="a3"><w:name w:val="List Paragraph"/></w:style></w:styles>"#
        );
        let numbering = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering {W}><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="5"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="6"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#
        );
        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body><w:p><w:pPr><w:pStyle w:val="1"/></w:pPr><w:r><w:t>周报</w:t></w:r></w:p><w:p><w:pPr><w:jc w:val="both"/></w:pPr><w:r><w:rPr><w:color w:val="FF0000"/></w:rPr><w:t>这周做完了知识库。</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="a3"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="5"/></w:numPr></w:pPr><w:r><w:t>测试通过</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="a3"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="6"/></w:numPr></w:pPr><w:r><w:t>第一步</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="0" w:name="x"/><w:r><w:t>有书签的一段</w:t></w:r><w:bookmarkEnd w:id="0"/></w:p><w:p><w:r><w:t>下周计划</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#
        );
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, text) in [("[Content_Types].xml", CONTENT_TYPES), ("_rels/.rels", RELS), ("word/document.xml", document.as_str()), ("word/styles.xml", styles.as_str()), ("word/numbering.xml", numbering.as_str())] {
            w.start_file(name, opts).unwrap();
            w.write_all(text.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// A table's cells change their text (and bold / italic / underline); its rows and cells stay.
    #[test]
    fn table_cells_change_their_text() {
        let cell = |inner: &str| format!("<w:tc>{inner}</w:tc>");
        let para = |t: &str| format!("<w:p><w:r><w:t>{t}</w:t></w:r></w:p>");
        let rows = [
            format!("<w:tr>{}{}</w:tr>", cell(&para("技能")), cell(&para("程度"))),
            format!("<w:tr>{}{}</w:tr>", cell(&para("SQL")), cell(r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>熟练</w:t></w:r></w:p>"#)),
            format!("<w:tr>{}{}</w:tr>", cell(&para("Figma")), cell(r#"<w:p><w:pPr><w:rPr><w:i/></w:rPr></w:pPr></w:p>"#)),
            format!("<w:tr>{}{}</w:tr>", cell(&format!("{}{}", para("Go"), para("（在学）"))), cell(r#"<w:tcPr><w:vMerge/></w:tcPr><w:p/>"#)),
        ];
        let document = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body><w:p><w:r><w:t>技能表</w:t></w:r></w:p><w:tbl>{}</w:tbl><w:sectPr/></w:body></w:document>"#, rows.concat());
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, text) in [("[Content_Types].xml", CONTENT_TYPES), ("_rels/.rels", RELS), ("word/document.xml", document.as_str())] {
            w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(text.as_bytes()).unwrap();
        }
        let data = w.finish().unwrap().into_inner();
        let doc = read_docx(&data).unwrap();
        assert_eq!((doc.items.len(), doc.items[1].fixed, doc.items[1].is_table()), (2, None, true));
        assert_eq!(doc.items[1].text, "| 技能 | 程度 |\n| SQL | **熟练** |\n| Figma |  |\n| Go （在学） |  |");
        // a cell's text (bold kept), an empty cell filled, the rest as written
        let new = "| 技能 | 程度 |\n| SQL | **精通** |\n| Figma | 一般 |\n| Go （在学） |  |";
        let out = edit_docx(&data, &[(1, new.into())]).unwrap();
        assert_eq!(read_docx(&out).unwrap().items[1].text, new);
        // the marks decide: bold off, bold on (a markdown rule line is fine)
        let out = edit_docx(&data, &[(1, "| 技能 | 程度 |\n|---|---|\n| SQL | 熟练 |\n| Figma | **一般** |\n| Go （在学） |  |".into())]).unwrap();
        assert_eq!(read_docx(&out).unwrap().items[1].text, "| 技能 | 程度 |\n| SQL | 熟练 |\n| Figma | **一般** |\n| Go （在学） |  |");
        let dir = crate::db::tests::temp_dir("word-table");
        assert!(textutil_text(&dir, &out).contains("一般"));
        // rows / cells added, a cell with two paragraphs, a merged one: refused
        assert!(edit_docx(&data, &[(1, format!("{new}\n| Rust | 一般 |"))]).unwrap_err().contains("行数"));
        assert!(edit_docx(&data, &[(1, new.replace("Go （在学）", "Go"))]).unwrap_err().contains("好几段"));
        assert!(edit_docx(&data, &[(1, new.replace("Go （在学） |  |", "Go （在学） | 入门 |"))]).unwrap_err().contains("合并"));
    }

    /// Formatting, headings and lists change through their marks, in the document's own styles.
    #[test]
    fn marks_change_formatting_headings_and_lists() {
        let data = styled_docx();
        let doc = read_docx(&data).unwrap();
        let shown: Vec<&str> = doc.items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(shown, ["# 周报", "这周做完了知识库。", "- 测试通过", "1. 第一步", "有书签的一段", "下周计划"]);
        // bold on part of a run: split there, its other properties (the color) kept
        let out = edit_docx(&data, &[(1, "这周做完了**知识库**。".into())]).unwrap();
        let xml = document_xml(&out).unwrap();
        assert!(xml.contains(r#"<w:r><w:rPr><w:color w:val="FF0000"/></w:rPr><w:t xml:space="preserve">这周做完了</w:t></w:r><w:r><w:rPr><w:b/><w:color w:val="FF0000"/></w:rPr><w:t xml:space="preserve">知识库</w:t></w:r>"#), "{xml}");
        assert_eq!(read_docx(&out).unwrap().items[1].text, "这周做完了**知识库**。");
        // and off again, italic and underline too
        let out2 = edit_docx(&out, &[(1, "这周*做完*了<u>知识库</u>。".into())]).unwrap();
        assert_eq!(read_docx(&out2).unwrap().items[1].text, "这周*做完*了<u>知识库</u>。");
        // a heading level by the document's own style; back to a plain paragraph
        let out = edit_docx(&data, &[(5, "## 下周计划".into()), (0, "周报".into())]).unwrap();
        let xml = document_xml(&out).unwrap();
        assert!(xml.contains(r#"<w:p><w:pPr><w:pStyle w:val="2"/></w:pPr><w:r><w:t>下周计划</w:t></w:r></w:p>"#) && xml.contains("<w:p><w:r><w:t>周报</w:t></w:r></w:p>"), "{xml}");
        assert!(edit_docx(&data, &[(5, "### 下周计划".into())]).unwrap_err().contains("标题 3"));
        // a list item like the nearest one of its kind (its numbering, 列表段落); and out of the list
        let out = edit_docx(&data, &[(5, "- 下周计划".into()), (3, "第一步".into())]).unwrap();
        let after = read_docx(&out).unwrap();
        assert_eq!((after.items[5].text.as_str(), after.items[3].text.as_str()), ("- 下周计划", "第一步"));
        assert!(document_xml(&out).unwrap().contains(r#"<w:p><w:pPr><w:pStyle w:val="a3"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="5"/></w:numPr></w:pPr><w:r><w:t>下周计划</w:t></w:r></w:p>"#));
        // new paragraphs after one (a blank line), with its properties; a heading among them
        let out = edit_docx(&data, &[(1, "这周做完了知识库。\n\n还修了两个 bug。\n\n## 小结".into())]).unwrap();
        let after = read_docx(&out).unwrap();
        assert_eq!(after.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), ["# 周报", "这周做完了知识库。", "还修了两个 bug。", "## 小结", "- 测试通过", "1. 第一步", "有书签的一段", "下周计划"]);
        assert!(document_xml(&out).unwrap().contains(r#"<w:p><w:pPr><w:jc w:val="both"/></w:pPr><w:r><w:rPr><w:color w:val="FF0000"/></w:rPr><w:t xml:space="preserve">还修了两个 bug。</w:t></w:r></w:p>"#));
        let text = textutil_text(&crate::db::tests::temp_dir("word-marks"), &out); // still a document Word-like apps open
        assert!(text.contains("还修了两个 bug。") && text.contains("小结") && text.contains("测试通过"), "{text}");
        // deleting a paragraph; not one with a bookmark
        let out = edit_docx(&data, &[(5, String::new())]).unwrap();
        assert!(!document_xml(&out).unwrap().contains("下周计划") && read_docx(&out).unwrap().items.len() == 5);
        assert!(edit_docx(&data, &[(4, String::new())]).unwrap_err().contains("书签"));
        // what check says matches what writing does
        assert!(check(&doc, 4, "").is_err() && check(&doc, 1, "**全部**改掉").is_ok());
    }

    #[test]
    fn the_diff_maps_back() {
        let a: Vec<char> = "abcdef".chars().collect();
        let b: Vec<char> = "abXdeYf".chars().collect();
        let ops = diff(&a, &b);
        let rebuilt: String = ops.iter().filter_map(|o| match o {
            Op::Keep(i) => Some(a[*i]),
            Op::Ins(_, c) => Some(*c),
            Op::Del(_) => None,
        }).collect();
        assert_eq!(rebuilt, "abXdeYf");
    }

    /// A one-page resume template like the user's (fictional placeholders): centred name, contact
    /// line, ruled section title, 教育抬头 with a right tab, grey note, bullet with a bold 小标题,
    /// 经历抬头 with three tab stops (time and name bold), a body-level bookmark, page setup.
    pub fn resume_template_docx() -> Vec<u8> {
        let numbering = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering {W}><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#);
        let bold = "<w:rPr><w:b/></w:rPr>";
        let tab = "<w:r><w:tab/></w:r>";
        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="36"/></w:rPr><w:t>[姓名]</w:t></w:r></w:p><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:color w:val="555555"/></w:rPr><w:t>[联系电话] · [电子邮箱] · [所在城市]</w:t></w:r></w:p><w:p><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="4" w:space="1" w:color="auto"/></w:pBdr></w:pPr><w:r>{bold}<w:t>教育背景</w:t></w:r></w:p><w:p><w:pPr><w:tabs><w:tab w:val="right" w:pos="10466"/></w:tabs></w:pPr><w:r>{bold}<w:t>[学校名称] · [专业名称]</w:t></w:r>{tab}<w:r><w:t>20XX.XX - 20XX.XX</w:t></w:r></w:p><w:p><w:r><w:rPr><w:color w:val="777777"/></w:rPr><w:t>[学历或学位] · [学院名称]；[主修课程]</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r>{bold}<w:t>编程开发：</w:t></w:r><w:r><w:t>[技术一]、[技术二]</w:t></w:r></w:p><w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="3000"/><w:tab w:val="left" w:pos="6500"/><w:tab w:val="right" w:pos="10466"/></w:tabs></w:pPr><w:r>{bold}<w:t>20XX.XX - 20XX.XX</w:t></w:r>{tab}<w:r>{bold}<w:t>[项目名称]</w:t></w:r>{tab}<w:r><w:t>[本人角色]</w:t></w:r>{tab}<w:r><w:t>[地点]</w:t></w:r></w:p><w:bookmarkStart w:id="0" w:name="_GoBack"/><w:bookmarkEnd w:id="0"/><w:p/><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720"/></w:sectPr></w:body></w:document>"#
        );
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, text) in [("[Content_Types].xml", CONTENT_TYPES), ("_rels/.rels", RELS), ("word/document.xml", document.as_str()), ("word/numbering.xml", numbering.as_str())] {
            w.start_file(name, opts).unwrap();
            w.write_all(text.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn a_resume_takes_each_template_paragraph_its_tab_stops_and_column_looks() {
        let data = resume_template_docx();
        let items = resume_template_items(&data).unwrap();
        assert_eq!(items.len(), 8);
        assert_eq!(items[3].as_deref(), Some("[学校名称] · [专业名称]\t20XX.XX - 20XX.XX"));
        assert_eq!(items[7], None);
        let lines: Vec<(usize, String)> = [
            (0, "张三"),
            (1, "a@example.com · 138 0000 0000"),
            (2, "教育背景"),
            (3, "示例大学 · 计算机\t2022.09 - 2026.06"),
            (4, "本科；主修课程：数据结构"),
            (5, "**编程开发**：Python、Rust"),
            (5, "没有小标题 & <一条>"),
            (6, "2025.01 - 至今\t示例项目\t独立开发\t北京"),
            (6, "2024.07 - 2024.08\t示例公司\t\t"),
        ]
        .into_iter()
        .map(|(i, t)| (i, t.to_string()))
        .collect();
        let out = resume_from_template(&data, &lines).unwrap();
        let shown: Vec<String> = read_docx(&out).unwrap().items.into_iter().map(|i| i.text).collect();
        assert_eq!(shown, ["**张三**", "a@example.com · 138 0000 0000", "**教育背景**", "**示例大学 · 计算机\t**2022.09 - 2026.06", "本科；主修课程：数据结构", "- **编程开发**：Python、Rust", "- 没有小标题 & <一条>", "**2025.01 - 至今\t示例项目\t**独立开发\t北京", "**2024.07 - 2024.08\t示例公司\t\t**"]);
        // (a tab has no look of its own: the bold marks read across it)
        let xml = document_xml(&out).unwrap();
        for kept in [r#"<w:jc w:val="center"/>"#, "<w:pBdr>", r#"<w:tab w:val="left" w:pos="6500"/>"#, r#"<w:color w:val="777777"/>"#, r#"<w:numId w:val="1"/>"#, r#"<w:pgSz w:w="11906""#, r#"<w:sz w:val="36"/>"#] {
            assert!(xml.contains(kept), "{kept}");
        }
        assert!(!xml.contains("[项目名称]") && !xml.contains("_GoBack"));
        // the numbering part comes along untouched
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&out)).unwrap();
        assert!(z.by_name("word/numbering.xml").is_ok());
        // a paragraph not in the template (any more)
        assert!(resume_from_template(&data, &[(7, "x".into())]).is_err());
        // tables, fields: refused
        assert!(resume_template_items(&sample_docx()).unwrap_err().contains("表格"));
    }
}
