//! 见字如面 as Word (.docx, v1.0 Phase 4), its look following the PDF's two styles: 柚儿园 (paper colour, the hand
//! font for titles, 苹方 body, the palette's colours) and 印刷 (white, 宋体 body, 黑体 headings, fixed greys). Rules:
//! - The print page sends the article as blocks (headings, paragraphs, list items with their marks as written,
//!   quotes / callouts, code lines, tables, rules, pictures, footnote lines) and runs (bold, italic, underline,
//!   strike, code, highlight colour, link, superscript, inline picture). Formulas and diagrams come as pictures the
//!   page had drawn; the article's own pictures are read from its attachments (HEIC / WebP… become JPEG).
//! - A4, the PDF's margins; the title (not in the contents), then 标签和日期, then 目录 (a Word TOC field: its entries
//!   link to the headings; Word fills in page numbers when it updates the field), then the body. 页眉和页码: from
//!   the second page the title and 「见字如面」 on top; the page number at the bottom of every page.

use super::export::Options;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Write;

#[derive(Debug, Clone)]
pub struct Picture {
    pub bytes: Vec<u8>,
    pub ext: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Run {
    #[serde(default)]
    pub t: String,
    #[serde(default)]
    pub b: bool,
    #[serde(default)]
    pub i: bool,
    #[serde(default)]
    pub u: bool,
    #[serde(default)]
    pub s: bool,
    #[serde(default)]
    pub code: bool,
    #[serde(default)]
    pub sup: bool,
    /// def | mint | pink | sky | yuzu | grape
    #[serde(default)]
    pub hl: Option<String>,
    #[serde(default)]
    pub href: Option<String>,
    /// A picture the page drew (print_picture's number) or an attachment, w × h pt.
    #[serde(default)]
    pub pic: Option<usize>,
    #[serde(default)]
    pub file: Option<i64>,
    #[serde(default)]
    pub w: f64,
    #[serde(default)]
    pub h: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "k", rename_all = "camelCase")]
pub enum Block {
    #[serde(rename = "h")]
    H { level: u8, runs: Vec<Run> },
    #[serde(rename = "p")]
    P { runs: Vec<Run> },
    /// `mark`: as written (•, 1., ☐ / ☑).
    #[serde(rename = "li")]
    Li { level: u8, mark: String, #[serde(default)] done: bool, runs: Vec<Run> },
    #[serde(rename = "quote")]
    Quote { #[serde(default)] tint: Option<String>, #[serde(default)] label: Option<String>, runs: Vec<Run> },
    #[serde(rename = "code")]
    Code { lines: Vec<String> },
    #[serde(rename = "table")]
    Table { rows: Vec<Vec<Vec<Run>>> },
    #[serde(rename = "hr")]
    Hr {},
    #[serde(rename = "pic")]
    Pic { #[serde(default)] pic: Option<usize>, #[serde(default)] file: Option<i64>, w: f64, h: f64 },
    #[serde(rename = "note")]
    Note { runs: Vec<Run> },
}

/// Colours as RRGGBB (the page reads them from the palette; 印刷's are fixed).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Theme {
    pub ink: String,
    pub soft: String,
    pub link: String,
    pub pen: String,
    pub line: String,
    #[serde(default)]
    pub paper: Option<String>,
    pub code: String,
    /// Highlight colours by name (def, mint, …).
    #[serde(default)]
    pub hl: HashMap<String, String>,
    /// Callout tints by name (sky, mint, …).
    #[serde(default)]
    pub tint: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Doc {
    pub title: String,
    /// 标签和日期 (when on): the date as shown, the tags.
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub blocks: Vec<Block>,
    pub theme: Theme,
}

const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing""#;
const EMU: f64 = 12_700.0;

fn esc(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn hex(s: &str) -> String {
    let h: String = s.trim().trim_start_matches('#').chars().filter(char::is_ascii_hexdigit).take(6).collect();
    if h.len() == 6 {
        h.to_uppercase()
    } else {
        "000000".into()
    }
}

struct Look {
    print: bool,
    body: &'static str,
    head: &'static str,
    /// twips: top, right, bottom, left
    margins: (u32, u32, u32, u32),
    header: u32,
    footer: u32,
    /// line height (twips, at least)
    line: u32,
    title: u32,
    sizes: [u32; 6],
}

impl Look {
    fn of(style: &str) -> Look {
        if style == "print" {
            Look { print: true, body: "Songti SC", head: "PingFang SC", margins: (1520, 1440, 1520, 1520), header: 720, footer: 680, line: 418, title: 48, sizes: [36, 28, 24, 22, 22, 22] }
        } else {
            Look { print: false, body: "PingFang SC", head: "Hannotate SC", margins: (1280, 1240, 1280, 1320), header: 600, footer: 520, line: 407, title: 60, sizes: [44, 34, 28, 24, 22, 22] }
        }
    }
    /// Content width in twips (A4 11906 wide).
    fn width(&self) -> u32 {
        11_906 - self.margins.1 - self.margins.3
    }
}

struct Out<'a> {
    theme: &'a Theme,
    look: &'a Look,
    pictures: &'a [Picture],
    files: &'a HashMap<i64, (Vec<u8>, String)>,
    media: Vec<(String, Vec<u8>)>,
    /// (id, target, external)
    rels: Vec<(String, String, bool)>,
    links: HashMap<String, String>,
    shapes: u32,
}

impl Out<'_> {
    fn link(&mut self, url: &str) -> String {
        if let Some(id) = self.links.get(url) {
            return id.clone();
        }
        let id = format!("rL{}", self.links.len() + 1);
        self.rels.push((id.clone(), url.to_string(), true));
        self.links.insert(url.to_string(), id.clone());
        id
    }
    /// A picture as a drawing run (None: unreadable).
    fn drawing(&mut self, pic: Option<usize>, file: Option<i64>, w: f64, h: f64) -> Option<String> {
        let (bytes, ext) = match (pic, file) {
            (Some(i), _) => {
                let p = self.pictures.get(i)?;
                (p.bytes.clone(), p.ext.clone())
            }
            (None, Some(id)) => {
                let (b, name) = self.files.get(&id)?;
                let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
                if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "bmp") {
                    (b.clone(), if ext == "jpg" { "jpeg".into() } else { ext })
                } else {
                    (crate::print::jpeg_of_image(b)?, "jpeg".into())
                }
            }
            _ => return None,
        };
        let max_w = self.look.width() as f64 / 20.0;
        let (w, h) = if w > max_w && w > 0.0 { (max_w, h * max_w / w) } else { (w.max(1.0), h.max(1.0)) };
        self.shapes += 1;
        let n = self.shapes;
        let file_name = format!("image{n}.{ext}");
        let rid = format!("rI{n}");
        self.rels.push((rid.clone(), format!("media/{file_name}"), false));
        self.media.push((file_name, bytes));
        let (cx, cy) = ((w * EMU).round() as i64, (h * EMU).round() as i64);
        Some(format!(
            r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{n}" name="图 {n}"/><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:nvPicPr><pic:cNvPr id="{n}" name="图 {n}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
        ))
    }

    fn text(t: &str) -> String {
        let mut out = String::new();
        for (k, part) in t.split('\t').enumerate() {
            if k > 0 {
                out.push_str("<w:tab/>");
            }
            if !part.is_empty() {
                out.push_str(&format!(r#"<w:t xml:space="preserve">{}</w:t>"#, esc(part)));
            }
        }
        out
    }

    fn run(&mut self, r: &Run, base: &str) -> String {
        if r.pic.is_some() || r.file.is_some() {
            return self.drawing(r.pic, r.file, r.w, r.h).unwrap_or_else(|| r#"<w:r><w:t>[图]</w:t></w:r>"#.to_string());
        }
        if r.t.is_empty() {
            return String::new();
        }
        let mut p = String::from(base);
        if r.b {
            p.push_str("<w:b/>");
        }
        if r.i {
            p.push_str("<w:i/>");
        }
        if r.s {
            p.push_str("<w:strike/>");
        }
        if r.code {
            p.push_str(r#"<w:rFonts w:ascii="Menlo" w:hAnsi="Menlo"/>"#);
            p.push_str(&format!(r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#, hex(&self.theme.code)));
        }
        if let Some(h) = &r.hl {
            let fill = self.theme.hl.get(h).or_else(|| self.theme.hl.get("def")).cloned().unwrap_or_else(|| "FFF3A3".into());
            p.push_str(&format!(r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#, hex(&fill)));
        }
        if r.href.is_some() {
            p.push_str(&format!(r#"<w:color w:val="{}"/><w:u w:val="single"/>"#, hex(&self.theme.link)));
        } else if r.u {
            p.push_str(&format!(r#"<w:u w:val="single" w:color="{}"/>"#, hex(&self.theme.pen)));
        }
        if r.sup {
            p.push_str(r#"<w:vertAlign w:val="superscript"/>"#);
        }
        let run = format!("<w:r>{}{}</w:r>", if p.is_empty() { String::new() } else { format!("<w:rPr>{p}</w:rPr>") }, Self::text(&r.t));
        match &r.href {
            Some(u) if u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:") => {
                let id = self.link(u);
                format!(r#"<w:hyperlink r:id="{id}" w:history="1">{run}</w:hyperlink>"#)
            }
            _ => run,
        }
    }

    fn runs(&mut self, rs: &[Run], base: &str) -> String {
        rs.iter().map(|r| self.run(r, base)).collect()
    }
}

fn para(ppr: &str, body: &str) -> String {
    if ppr.is_empty() {
        format!("<w:p>{body}</w:p>")
    } else {
        format!("<w:p><w:pPr>{ppr}</w:pPr>{body}</w:p>")
    }
}

fn styles(look: &Look, theme: &Theme) -> String {
    let ink = hex(&theme.ink);
    let soft = hex(&theme.soft);
    let fonts = |f: &str| format!(r#"<w:rFonts w:ascii="{f}" w:hAnsi="{f}" w:eastAsia="{f}" w:cs="{f}"/>"#);
    let headings: String = (1..=6)
        .map(|n| {
            let size = look.sizes[n - 1];
            let color = if n >= 5 { &soft } else { &ink };
            format!(
                r#"<w:style w:type="paragraph" w:styleId="Heading{n}"><w:name w:val="heading {n}"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="{}" w:after="40" w:line="{}" w:lineRule="atLeast"/><w:outlineLvl w:val="{}"/></w:pPr><w:rPr>{}<w:b/><w:color w:val="{color}"/><w:sz w:val="{size}"/></w:rPr></w:style>"#,
                if n <= 2 { 280 } else { 200 },
                size * 14,
                n - 1,
                fonts(look.head)
            )
        })
        .collect();
    let tocs: String = (1..=6)
        .map(|n| format!(r#"<w:style w:type="paragraph" w:styleId="TOC{n}"><w:name w:val="toc {n}"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:tabs><w:tab w:val="right" w:leader="dot" w:pos="{}"/></w:tabs><w:spacing w:before="0" w:after="0"/><w:ind w:left="{}"/></w:pPr><w:rPr>{}</w:rPr></w:style>"#, look.width(), (n - 1) * 280, if look.print { String::new() } else { fonts(look.head) }))
        .collect();
    let title_jc = if look.print { r#"<w:jc w:val="center"/>"# } else { "" };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles {W_NS}><w:docDefaults><w:rPrDefault><w:rPr>{}<w:color w:val="{ink}"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US" w:eastAsia="zh-CN"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before="140" w:after="0" w:line="{}" w:lineRule="atLeast"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/><w:rPr>{}<w:color w:val="{ink}"/></w:rPr></w:style>{headings}<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:before="0" w:after="60" w:line="{}" w:lineRule="atLeast"/>{title_jc}</w:pPr><w:rPr>{}<w:b/><w:sz w:val="{}"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Meta"><w:name w:val="Meta"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="40" w:after="120"/>{title_jc}</w:pPr><w:rPr>{}<w:color w:val="{soft}"/><w:sz w:val="{}"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="TOCHeading"><w:name w:val="TOC Heading"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="200" w:after="60"/>{title_jc}</w:pPr><w:rPr>{}<w:b/><w:sz w:val="{}"/></w:rPr></w:style>{tocs}<w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="0" w:after="0" w:line="300" w:lineRule="atLeast"/><w:ind w:left="160" w:right="160"/><w:shd w:val="clear" w:color="auto" w:fill="{}"/></w:pPr><w:rPr><w:rFonts w:ascii="Menlo" w:hAnsi="Menlo" w:eastAsia="{}" w:cs="Menlo"/><w:sz w:val="18"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Note"><w:name w:val="Note"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="60"/></w:pPr><w:rPr><w:color w:val="{soft}"/><w:sz w:val="19"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Header"><w:name w:val="header"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="0"/></w:pPr><w:rPr>{}<w:color w:val="{soft}"/><w:sz w:val="18"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Footer"><w:name w:val="footer"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="0"/><w:jc w:val="center"/></w:pPr><w:rPr>{}<w:color w:val="{soft}"/><w:sz w:val="20"/></w:rPr></w:style><w:style w:type="table" w:styleId="Table"><w:name w:val="Table"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="8" w:space="0" w:color="{}"/><w:bottom w:val="single" w:sz="8" w:space="0" w:color="{}"/><w:insideH w:val="{}" w:sz="4" w:space="0" w:color="{}"/></w:tblBorders><w:tblCellMar><w:left w:w="110" w:type="dxa"/><w:right w:w="110" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style></w:styles>"#,
        fonts(look.body),
        look.line,
        fonts(look.body),
        look.title * 14,
        fonts(look.head),
        look.title,
        fonts(look.head),
        if look.print { 19 } else { 22 },
        fonts(look.head),
        if look.print { 24 } else { 28 },
        hex(&theme.code),
        look.body,
        fonts(if look.print { "PingFang SC" } else { look.head }),
        fonts(if look.print { look.body } else { look.head }),
        hex(&theme.line),
        hex(&theme.line),
        if look.print { "single" } else { "dashed" },
        hex(&theme.line),
    )
}

/// The article as a .docx.
pub fn write(doc: &Doc, options: &Options, pictures: &[Picture], files: &HashMap<i64, (Vec<u8>, String)>) -> Result<Vec<u8>, String> {
    let look = Look::of(&options.style);
    let theme = &doc.theme;
    let mut out = Out { theme, look: &look, pictures, files, media: vec![], rels: vec![], links: HashMap::new(), shapes: 0 };
    let mut body = String::new();
    // title, then 标签和日期
    body.push_str(&para(r#"<w:pStyle w:val="Title"/>"#, &format!("<w:r>{}</w:r>", Out::text(&doc.title))));
    if doc.date.is_some() || !doc.tags.is_empty() {
        let mut parts: Vec<String> = doc.date.iter().cloned().collect();
        parts.extend(doc.tags.iter().map(|t| format!("#{t}")));
        let sep = if look.print { " · " } else { "　" };
        body.push_str(&para(r#"<w:pStyle w:val="Meta"/>"#, &format!("<w:r>{}</w:r>", Out::text(&parts.join(sep)))));
    }
    // headings: bookmarks for the contents
    let heads: Vec<(usize, u8, String)> = doc
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(i, b)| match b {
            Block::H { level, runs } => Some((i, *level, runs.iter().map(|r| r.t.as_str()).collect::<String>())),
            _ => None,
        })
        .filter(|(_, _, t)| !t.trim().is_empty())
        .collect();
    if options.toc && !heads.is_empty() {
        body.push_str(&para(r#"<w:pStyle w:val="TOCHeading"/>"#, &format!("<w:r>{}</w:r>", Out::text(if look.print { "目　录" } else { "目录" }))));
        let top = heads.iter().map(|h| h.1).min().unwrap_or(1);
        let n = heads.len();
        for (k, (i, level, t)) in heads.iter().enumerate() {
            let begin = if k == 0 { r#"<w:r><w:fldChar w:fldCharType="begin" w:dirty="true"/></w:r><w:r><w:instrText xml:space="preserve"> TOC \o "1-6" \h \z \u </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>"# } else { "" };
            let end = if k + 1 == n { r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"# } else { "" };
            let style = format!(r#"<w:pStyle w:val="TOC{}"/>"#, (level - top + 1).clamp(1, 6));
            body.push_str(&para(&style, &format!(r#"{begin}<w:hyperlink w:anchor="_Toc{i}" w:history="1"><w:r><w:rPr><w:color w:val="{}"/><w:u w:val="none"/></w:rPr>{}</w:r></w:hyperlink>{end}"#, hex(&theme.ink), Out::text(t.trim()))));
        }
    }
    let soft = hex(&theme.soft);
    let pen = hex(&theme.pen);
    let line = hex(&theme.line);
    for (i, b) in doc.blocks.iter().enumerate() {
        match b {
            Block::H { level, runs } => {
                let lv = (*level).clamp(1, 6);
                body.push_str(&para(&format!(r#"<w:pStyle w:val="Heading{lv}"/>"#), &format!(r#"<w:bookmarkStart w:id="{i}" w:name="_Toc{i}"/>{}<w:bookmarkEnd w:id="{i}"/>"#, out.runs(runs, ""))));
            }
            Block::P { runs } => body.push_str(&para("", &out.runs(runs, ""))),
            Block::Li { level, mark, done, runs } => {
                let left = 400 * (*level as u32 + 1);
                let base = if *done { format!(r#"<w:strike/><w:color w:val="{soft}"/>"#) } else { String::new() };
                let marker = format!(r#"<w:r><w:rPr><w:color w:val="{pen}"/></w:rPr>{}</w:r><w:r><w:tab/></w:r>"#, Out::text(mark));
                body.push_str(&para(&format!(r#"<w:spacing w:before="40"/><w:ind w:left="{left}" w:hanging="400"/>"#), &format!("{marker}{}", out.runs(runs, &base))));
            }
            Block::Quote { tint, label, runs } => {
                let fill = tint.as_ref().and_then(|t| theme.tint.get(t)).map(|f| format!(r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#, hex(f))).unwrap_or_default();
                let lab = label.as_ref().map(|l| format!(r#"<w:r><w:rPr><w:b/></w:rPr>{}</w:r>"#, Out::text(&format!("{l}　")))).unwrap_or_default();
                let base = if tint.is_some() { String::new() } else { format!(r#"<w:color w:val="{soft}"/>"#) };
                body.push_str(&para(&format!(r#"<w:pBdr><w:left w:val="single" w:sz="18" w:space="8" w:color="{pen}"/></w:pBdr>{fill}<w:spacing w:before="40"/><w:ind w:left="200"/>"#), &format!("{lab}{}", out.runs(runs, &base))));
            }
            Block::Code { lines } => {
                for (k, l) in lines.iter().enumerate() {
                    let gap = if k == 0 { r#"<w:spacing w:before="140" w:after="0"/>"# } else { "" };
                    body.push_str(&para(&format!(r#"<w:pStyle w:val="Code"/>{gap}"#), &format!("<w:r>{}</w:r>", Out::text(l))));
                }
            }
            Block::Table { rows } => {
                let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                let w = look.width() / cols as u32;
                let grid: String = (0..cols).map(|_| format!(r#"<w:gridCol w:w="{w}"/>"#)).collect();
                let rule = if look.print { "single" } else { "dashed" };
                let mut t = format!(r#"<w:tbl><w:tblPr><w:tblStyle w:val="Table"/><w:tblW w:w="5000" w:type="pct"/><w:tblBorders><w:top w:val="single" w:sz="8" w:space="0" w:color="{line}"/><w:bottom w:val="single" w:sz="8" w:space="0" w:color="{line}"/><w:insideH w:val="{rule}" w:sz="4" w:space="0" w:color="{line}"/></w:tblBorders><w:tblCellMar><w:left w:w="110" w:type="dxa"/><w:right w:w="110" w:type="dxa"/></w:tblCellMar></w:tblPr><w:tblGrid>{grid}</w:tblGrid>"#);
                for (r, row) in rows.iter().enumerate() {
                    t.push_str("<w:tr>");
                    for c in 0..cols {
                        let cell = row.get(c).map(|rs| out.runs(rs, if r == 0 { "<w:b/>" } else { "" })).unwrap_or_default();
                        t.push_str(&format!(r#"<w:tc><w:tcPr><w:tcW w:w="{w}" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:spacing w:before="40" w:after="40"/></w:pPr>{cell}</w:p></w:tc>"#));
                    }
                    t.push_str("</w:tr>");
                }
                t.push_str("</w:tbl>");
                body.push_str(&t);
                body.push_str(&para(r#"<w:spacing w:before="0" w:line="120" w:lineRule="exact"/>"#, ""));
            }
            Block::Hr {} => body.push_str(&para(&format!(r#"<w:pBdr><w:bottom w:val="{}" w:sz="8" w:space="1" w:color="{line}"/></w:pBdr>"#, if look.print { "single" } else { "dashed" }), "")),
            Block::Pic { pic, file, w, h } => {
                let d = out.drawing(*pic, *file, *w, *h).unwrap_or_else(|| "<w:r><w:t>[图]</w:t></w:r>".into());
                body.push_str(&para(r#"<w:jc w:val="center"/>"#, &d));
            }
            Block::Note { runs } => body.push_str(&para(r#"<w:pStyle w:val="Note"/>"#, &out.runs(runs, ""))),
        }
    }
    // the section: A4, margins, header (from page 2) and page numbers
    let (mt, mr, mb, ml) = look.margins;
    let mut refs = String::new();
    if options.header {
        refs.push_str(r#"<w:headerReference w:type="default" r:id="rH1"/><w:footerReference w:type="default" r:id="rF1"/><w:footerReference w:type="first" r:id="rF1"/>"#);
        out.rels.push(("rH1".into(), "header1.xml".into(), false));
        out.rels.push(("rF1".into(), "footer1.xml".into(), false));
    }
    let background = match (&theme.paper, look.print) {
        (Some(p), false) => format!(r#"<w:background w:color="{}"/>"#, hex(p)),
        _ => String::new(),
    };
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document {W_NS}>{background}<w:body>{body}<w:sectPr>{refs}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="{mt}" w:right="{mr}" w:bottom="{mb}" w:left="{ml}" w:header="{}" w:footer="{}" w:gutter="0"/>{}</w:sectPr></w:body></w:document>"#,
        look.header,
        look.footer,
        if options.header { "<w:titlePg/>" } else { "" }
    );
    let rule = if look.print { "single" } else { "dashed" };
    let header = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:hdr {W_NS}><w:p><w:pPr><w:pStyle w:val="Header"/><w:pBdr><w:bottom w:val="{rule}" w:sz="6" w:space="4" w:color="{line}"/></w:pBdr><w:tabs><w:tab w:val="right" w:pos="{}"/></w:tabs></w:pPr><w:r>{}</w:r><w:r><w:tab/></w:r><w:r>{}</w:r></w:p></w:hdr>"#,
        look.width(),
        Out::text(&doc.title),
        Out::text("见字如面")
    );
    let field = r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#;
    let footer = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:ftr {W_NS}><w:p><w:pPr><w:pStyle w:val="Footer"/></w:pPr>{}</w:p></w:ftr>"#,
        if look.print { format!(r#"<w:r><w:t xml:space="preserve">— </w:t></w:r>{field}<w:r><w:t xml:space="preserve"> —</w:t></w:r>"#) } else { field.to_string() }
    );
    let settings = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings {W_NS}>{}<w:defaultTabStop w:val="420"/><w:characterSpacingControl w:val="compressPunctuation"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#,
        if background.is_empty() { "" } else { "<w:displayBackgroundShape/>" }
    );
    let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rSet" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/>"#);
    for (id, target, external) in &out.rels {
        let kind = if *external {
            "hyperlink"
        } else if target.starts_with("media/") {
            "image"
        } else if target.starts_with("header") {
            "header"
        } else {
            "footer"
        };
        rels.push_str(&format!(r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{}"{}/>"#, esc(target), if *external { r#" TargetMode="External""# } else { "" }));
    }
    rels.push_str("</Relationships>");
    let types = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="gif" ContentType="image/gif"/><Default Extension="bmp" ContentType="image/bmp"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>{}</Types>"#,
        if options.header { r#"<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/>"# } else { "" }
    );
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let styles_xml = styles(&look, theme);
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut parts: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), types.into_bytes()),
        ("_rels/.rels".into(), root_rels.as_bytes().to_vec()),
        ("word/_rels/document.xml.rels".into(), rels.into_bytes()),
        ("word/document.xml".into(), document.into_bytes()),
        ("word/styles.xml".into(), styles_xml.into_bytes()),
        ("word/settings.xml".into(), settings.into_bytes()),
    ];
    if options.header {
        parts.push(("word/header1.xml".into(), header.into_bytes()));
        parts.push(("word/footer1.xml".into(), footer.into_bytes()));
    }
    for (name, bytes) in std::mem::take(&mut out.media) {
        parts.push((format!("word/media/{name}"), bytes));
    }
    for (name, bytes) in parts {
        w.start_file(name, opts).map_err(|e| e.to_string())?;
        w.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    Ok(w.finish().map_err(|e| e.to_string())?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Read;

    fn part(docx: &[u8], name: &str) -> String {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(docx)).unwrap();
        let mut s = String::new();
        z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn an_article_as_word() {
        let doc: Doc = serde_json::from_value(json!({
            "title": "东京的第三天", "date": "2026 年 11 月 6 日 · 周四", "tags": ["旅行/2026东京"],
            "theme": {"ink": "#2b2a26", "soft": "#6d6a60", "link": "#3a4a8a", "pen": "#555555", "line": "#c9c9c2", "paper": "#fbf7ee", "code": "#eeeeea", "hl": {"def": "#f3e7a3", "mint": "#d8f0e4"}, "tint": {"mint": "#e6f5ee"}},
            "blocks": [
                {"k": "p", "runs": [{"t": "早上 "}, {"t": "谷中", "b": true}, {"t": "高亮", "hl": "mint"}, {"t": "链接", "href": "https://example.com"}, {"t": "1", "sup": true}]},
                {"k": "h", "level": 2, "runs": [{"t": "上午 · 谷中银座"}]},
                {"k": "li", "level": 0, "mark": "☑", "done": true, "runs": [{"t": "明信片 × 3"}]},
                {"k": "li", "level": 1, "mark": "1.", "runs": [{"t": "第二层"}]},
                {"k": "quote", "tint": "mint", "label": "提示", "runs": [{"t": "周四一半的店休息"}]},
                {"k": "code", "lines": ["fn main() {", "\tprintln!(\"<hi>\");", "}"]},
                {"k": "table", "rows": [[[{"t": "什么"}], [{"t": "日元"}]], [[{"t": "地铁"}], [{"t": "¥600"}]]]},
                {"k": "hr"},
                {"k": "pic", "pic": 0, "w": 900, "h": 300},
                {"k": "p", "runs": [{"t": "公式 "}, {"pic": 0, "w": 30, "h": 12}]},
                {"k": "note", "runs": [{"t": "1. 后来查了"}]}
            ]
        }))
        .unwrap();
        let opts = Options { format: "docx".into(), style: "pomelo".into(), toc: true, header: true, meta: true, dest: "/tmp".into() };
        let png = Picture { bytes: b"\x89PNG fake".to_vec(), ext: "png".into() };
        let bytes = write(&doc, &opts, &[png], &HashMap::new()).unwrap();
        let d = part(&bytes, "word/document.xml");
        assert!(d.contains(r#"<w:pStyle w:val="Title"/>"#) && d.contains("东京的第三天"));
        assert!(d.contains("#旅行/2026东京") && d.contains("TOC \\o") && d.contains(r#"w:anchor="_Toc1""#) && d.contains(r#"w:name="_Toc1""#));
        assert!(d.contains("&lt;hi&gt;") && d.contains("<w:tab/>") && d.contains("<w:titlePg/>") && d.contains(r#"<w:background w:color="FBF7EE"/>"#));
        assert!(d.contains(r#"w:fill="D8F0E4""#) && d.contains("<w:strike/>") && d.contains("<w:vertAlign w:val=\"superscript\"/>"));
        // a picture wider than the text is scaled to it (A4 − margins = 467 pt)
        assert!(d.contains(&format!("cx=\"{}\"", (9346.0 / 20.0 * 12_700.0f64).round() as i64)), "{d}");
        let rels = part(&bytes, "word/_rels/document.xml.rels");
        assert!(rels.contains("https://example.com") && rels.contains("media/image1.png") && rels.contains("header1.xml"));
        assert!(part(&bytes, "word/footer1.xml").contains(" PAGE "));
        assert!(part(&bytes, "word/styles.xml").contains("Hannotate SC"));
        // 印刷: no paper colour, 宋体, no header when off
        let opts = Options { style: "print".into(), header: false, toc: false, ..opts };
        let bytes = write(&doc, &opts, &[], &HashMap::new()).unwrap();
        let d = part(&bytes, "word/document.xml");
        assert!(!d.contains("w:background") && !d.contains("TOC \\o") && !d.contains("headerReference") && d.contains("[图]"));
        assert!(part(&bytes, "word/styles.xml").contains("Songti SC"));
    }
}
