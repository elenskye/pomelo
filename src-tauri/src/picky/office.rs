//! More kinds of files for 柚子 (v0.4.5), turned into what files.rs stores:
//! - Word (.docx / .doc): macOS `textutil` → plain text.
//! - PowerPoint (.pptx): each slide's text (the zip's `ppt/slides/slideN.xml`, `<a:t>` runs, one line
//!   per paragraph), one segment per slide.
//! - Excel (.xlsx / .xlsm / .xls / .ods: calamine) and CSV (UTF-8 / UTF-16): sheets of rows, the first
//!   row the header. Up to `BIG_ROWS` data rows the whole table is sent; a bigger one stores only its
//!   header and first `PREVIEW_ROWS` rows, and the question gets `pomelo_table` to look up the rest
//!   (read again from the stored copy each time).
//! - Web pages: the article's text as Markdown (dom_smoothie, a Readability port); the copy is that
//!   text, with the address kept.
//! - Images (PNG / JPEG / GIF / WebP / HEIC): no text; sent to cloud models only (local ones are told
//!   they can't see it). Shrunk to `MAX_SIDE` px on the long side first (macOS `sips`); HEIC → JPEG.
//! - Images inside documents (v0.8): a Word / PowerPoint file's own images (from its zip, placed by the text near
//!   them or their slide; vector ones can't be sent); a PDF is looked at page by page, drawn whole (`image_pages` says
//!   which hold images). Listed by `images_text`, sent by `look_at`; skills/files.rs and knowledge.rs offer them.

use super::files::Segment;
use std::io::Read;
use std::path::Path;
use std::process::Command;

pub const BIG_ROWS: usize = 200;
pub const PREVIEW_ROWS: usize = 50;
/// Rows one 查表 call returns at most.
pub const MAX_QUERY_ROWS: usize = 200;
pub const TABLE_TOOL: &str = "pomelo_table";

/// 查表, offered when the question carries a big table (no user data in it).
pub fn table_tool() -> serde_json::Value {
    let text = |d: &str| serde_json::json!({ "type": "string", "description": d });
    serde_json::json!({ "type": "function", "function": {
        "name": TABLE_TOOL,
        "description": "查这次附上的大表格（Excel / CSV）里没有给出的行。view：rows 按行号取（from、to，一次最多 200 行）；find 含某些字的行（keyword）；column 某一列等于某值的行（column 填列名，value 填值）。file 填文件名；有好几张表时 sheet 填表名。",
        "parameters": { "type": "object", "properties": {
            "file": text("表格的文件名"),
            "sheet": text("表名（不填就是第一张）"),
            "view": { "type": "string", "enum": ["rows", "find", "column"] },
            "from": { "type": "integer", "description": "rows：从第几行" },
            "to": { "type": "integer", "description": "rows：到第几行" },
            "keyword": text("find：要找的字"),
            "column": text("column：列名"),
            "value": text("column：这一列的值"),
        }, "required": ["view"] },
    } })
}
/// Long side of an image as sent.
pub const MAX_SIDE: u32 = 1568;
const SEGMENT_CHARS: usize = 6000;
const MAX_CELL: usize = 200;

// ---------- Word ----------

pub fn word_text(path: &Path) -> Result<String, String> {
    let out = Command::new("/usr/bin/textutil").args(["-convert", "txt", "-stdout", "--"]).arg(path).output().map_err(|e| format!("读不了这个 Word 文件：{e}"))?;
    if !out.status.success() {
        return Err("读不了这个 Word 文件，可能已损坏或加了密码".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n").replace('\r', "\n").replace('\u{2028}', "\n").replace('\u{000c}', "\n"))
}

// ---------- PowerPoint ----------

/// Each slide's text, in slide order (an empty slide is an empty string).
pub fn pptx_slides(data: &[u8]) -> Result<Vec<String>, String> {
    let bad = || "读不了这个 PowerPoint 文件，可能已损坏或加了密码".to_string();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|_| bad())?;
    let mut slides: Vec<(u32, String)> = (0..zip.len())
        .filter_map(|i| {
            let name = zip.name_for_index(i)?.ok()?.to_string();
            let n: u32 = name.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse().ok()?;
            Some((n, name))
        })
        .collect();
    if slides.is_empty() {
        return Err(bad());
    }
    slides.sort();
    let mut out = vec![];
    for (_, name) in slides {
        let mut xml = String::new();
        zip.by_name(&name).map_err(|_| bad())?.take(20 * 1024 * 1024).read_to_string(&mut xml).map_err(|_| bad())?;
        out.push(slide_text(&xml));
    }
    Ok(out)
}

/// `<a:t>` runs joined, one line per `<a:p>` paragraph.
fn slide_text(xml: &str) -> String {
    use quick_xml::events::Event;
    let is = |n: quick_xml::name::LocalName, s: &str| AsRef::<str>::as_ref(&n) == s;
    let mut r = quick_xml::Reader::from_str(xml);
    let (mut lines, mut line, mut in_text) = (vec![], String::new(), false);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if is(e.local_name(), "t") => in_text = true,
            Ok(Event::End(e)) if is(e.local_name(), "t") => in_text = false,
            Ok(Event::End(e)) if is(e.local_name(), "p") => {
                let l = std::mem::take(&mut line);
                if !l.trim().is_empty() {
                    lines.push(l.trim().to_string());
                }
            }
            Ok(Event::Empty(e)) if is(e.local_name(), "br") => line.push(' '),
            Ok(Event::Text(t)) if in_text => line.push_str(&t.into_inner()),
            Ok(Event::GeneralRef(e)) if in_text => {
                let name = e.into_inner();
                let num = |s: &str| if let Some(h) = s.strip_prefix('x').or_else(|| s.strip_prefix('X')) { u32::from_str_radix(h, 16).ok() } else { s.parse().ok() };
                match name.as_ref() {
                    "amp" => line.push('&'),
                    "lt" => line.push('<'),
                    "gt" => line.push('>'),
                    "quot" => line.push('"'),
                    "apos" => line.push('\''),
                    n => line.extend(n.strip_prefix('#').and_then(num).and_then(char::from_u32)),
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    if !line.trim().is_empty() {
        lines.push(line.trim().to_string());
    }
    lines.join("\n")
}

// ---------- tables ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub name: String,
    /// The header first; trailing empty rows and columns dropped.
    pub rows: Vec<Vec<String>>,
}

impl Sheet {
    pub fn data_rows(&self) -> usize {
        self.rows.len().saturating_sub(1)
    }
}

pub fn is_table(ext: &str) -> bool {
    matches!(ext, "xlsx" | "xlsm" | "xls" | "ods" | "csv")
}

/// Every non-empty sheet of a spreadsheet or CSV file.
pub fn read_table(path: &Path, ext: &str) -> Result<Vec<Sheet>, String> {
    let sheets = if ext == "csv" {
        let data = std::fs::read(path).map_err(|e| format!("无法读取文件：{e}"))?;
        let text = super::files::decode(&data).map_err(|_| "认不出 CSV 的文字编码，请另存为 UTF-8 的 CSV 再添加".to_string())?;
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "表".into());
        vec![Sheet { name, rows: parse_csv(&text) }]
    } else {
        use calamine::{open_workbook_auto, Reader};
        let mut wb = open_workbook_auto(path).map_err(|_| "读不了这个表格文件，可能已损坏或加了密码".to_string())?;
        let mut out = vec![];
        for name in wb.sheet_names() {
            let Ok(range) = wb.worksheet_range(&name) else { continue };
            let rows = range.rows().map(|r| r.iter().map(|c| cell(&c.to_string())).collect()).collect();
            out.push(Sheet { name, rows });
        }
        out
    };
    let sheets: Vec<Sheet> = sheets.into_iter().map(trimmed).filter(|s| !s.rows.is_empty()).collect();
    if sheets.is_empty() {
        return Err("表格是空的，没有可以读取的内容".into());
    }
    Ok(sheets)
}

fn cell(s: &str) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ").replace('|', "/");
    if flat.chars().count() > MAX_CELL {
        format!("{}…", flat.chars().take(MAX_CELL).collect::<String>())
    } else {
        flat
    }
}

/// Without trailing empty rows / columns (and fully empty rows anywhere).
fn trimmed(mut s: Sheet) -> Sheet {
    s.rows.retain(|r| r.iter().any(|c| !c.is_empty()));
    let width = s.rows.iter().map(|r| r.iter().rposition(|c| !c.is_empty()).map_or(0, |i| i + 1)).max().unwrap_or(0);
    for r in &mut s.rows {
        r.resize(width, String::new());
    }
    s
}

/// RFC 4180-ish: commas (or tabs when a line has no comma but tabs), quotes with "" inside, CRLF.
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let first = text.lines().next().unwrap_or("");
    let sep = if !first.contains(',') && first.contains('\t') { '\t' } else { ',' };
    let (mut rows, mut row, mut field, mut quoted) = (vec![], vec![], String::new(), false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' if quoted => quoted = false,
            '"' if field.is_empty() => quoted = true,
            c if c == sep && !quoted => row.push(cell(&std::mem::take(&mut field))),
            '\n' if !quoted => {
                row.push(cell(&std::mem::take(&mut field)));
                rows.push(std::mem::take(&mut row));
            }
            '\r' if !quoted => {}
            c => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(cell(&field));
        rows.push(row);
    }
    rows
}

fn line(cells: &[String]) -> String {
    format!("| {} |", cells.join(" | "))
}

fn header(s: &Sheet) -> String {
    let h = &s.rows[0];
    format!("{}\n|{}", line(h), " --- |".repeat(h.len().max(1)))
}

/// Data row `i` (1-based) with its number in front.
fn numbered(s: &Sheet, i: usize) -> String {
    format!("{i}. {}", line(&s.rows[i]))
}

/// Segments for the stored copy and the total data rows. Small tables: every row (cut every
/// `SEGMENT_CHARS`, each piece with the header). Big ones: the header and the first rows only.
/// `whole`: every row even of a big table (知识库).
pub fn table_segments(sheets: &[Sheet], file: &str, whole: bool) -> (Vec<Segment>, i64) {
    let total: usize = sheets.iter().map(Sheet::data_rows).sum();
    let mut out = vec![];
    let mut push = |heading: String, content: String| out.push(Segment { ordinal: out.len() as i64, page_start: None, page_end: None, heading: Some(heading), content });
    for s in sheets {
        let n = s.data_rows();
        let cols = s.rows[0].len();
        if total > BIG_ROWS && !whole {
            let shown = n.min(PREVIEW_ROWS);
            let rows: Vec<String> = (1..=shown).map(|i| numbered(s, i)).collect();
            let more = if n > shown { format!("其他行没有给出：要看用 pomelo_table 查（file 填「{file}」，sheet 填「{}」）。", s.name) } else { String::new() };
            push(format!("表「{}」", s.name), format!("表「{}」共 {n} 行 {cols} 列，这里是表头和前 {shown} 行（行号在前）。{more}\n{}\n{}", s.name, header(s), rows.join("\n")));
            continue;
        }
        let mut start = 1;
        while start <= n.max(1) {
            let mut rows = vec![];
            let mut size = header(s).chars().count();
            let mut i = start;
            while i <= n {
                let l = numbered(s, i);
                if !rows.is_empty() && size + l.chars().count() > SEGMENT_CHARS {
                    break;
                }
                size += l.chars().count() + 1;
                rows.push(l);
                i += 1;
            }
            let end = (i - 1).max(start);
            let range = if n == 0 { "只有表头".to_string() } else if start == 1 && end == n { format!("共 {n} 行") } else { format!("第 {start}–{end} 行，共 {n} 行") };
            push(format!("表「{}」", s.name), format!("表「{}」（{range}，{cols} 列）\n{}\n{}", s.name, header(s), rows.join("\n")));
            start = end + 1;
        }
    }
    (out, total as i64)
}

/// pomelo_table: rows of one sheet by number range, by a word, or by a column's value.
pub fn table_query(sheets: &[Sheet], args: &serde_json::Value) -> Result<String, String> {
    let text = |k: &str| args[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let num = |k: &str| args[k].as_u64().or_else(|| args[k].as_str().and_then(|s| s.trim().parse().ok())).map(|n| n as usize);
    let s = match text("sheet") {
        Some(name) => sheets.iter().find(|s| s.name == name).or_else(|| sheets.iter().find(|s| s.name.contains(&name))).ok_or_else(|| format!("没有叫「{name}」的表（有：{}）", sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("、")))?,
        None => &sheets[0],
    };
    let n = s.data_rows();
    let picked: Vec<usize> = match text("view").as_deref().unwrap_or("rows") {
        "find" => {
            let w = text("keyword").ok_or("find 要填 keyword")?.to_lowercase();
            (1..=n).filter(|&i| s.rows[i].iter().any(|c| c.to_lowercase().contains(&w))).collect()
        }
        "column" => {
            let col = text("column").ok_or("column 要填列名")?;
            let j = s.rows[0].iter().position(|h| *h == col).or_else(|| s.rows[0].iter().position(|h| h.contains(&col))).ok_or_else(|| format!("没有「{col}」这一列（有：{}）", s.rows[0].join("、")))?;
            let v = text("value").ok_or("column 要填 value")?.to_lowercase();
            (1..=n).filter(|&i| s.rows[i][j].to_lowercase() == v || s.rows[i][j].to_lowercase().contains(&v)).collect()
        }
        "rows" => {
            let from = num("from").unwrap_or(1).max(1);
            let to = num("to").unwrap_or(from + MAX_QUERY_ROWS - 1).min(n);
            if from > n {
                return Err(format!("表「{}」只有 {n} 行", s.name));
            }
            (from..=to).collect()
        }
        _ => return Err("view 只能是 rows / find / column".into()),
    };
    let shown: Vec<String> = picked.iter().take(MAX_QUERY_ROWS).map(|&i| numbered(s, i)).collect();
    let more = if picked.len() > shown.len() { format!("\n……还有 {} 行没列出（可以缩小范围再查）", picked.len() - shown.len()) } else { String::new() };
    Ok(format!("表「{}」（共 {n} 行）里找到 {} 行（行号在前）：\n{}\n{}{more}", s.name, picked.len(), header(s), shown.join("\n")))
}

// ---------- web pages ----------

/// (title, the article as Markdown) of a page.
pub fn web_article(html: &str, url: &str) -> Result<(String, String), String> {
    let cfg = dom_smoothie::Config { text_mode: dom_smoothie::TextMode::Markdown, ..Default::default() };
    let mut r = dom_smoothie::Readability::new(html, Some(url), Some(cfg)).map_err(|_| "网址不对".to_string())?;
    let a = r.parse().map_err(|_| "这个网页找不到正文（可能要登录，或者内容是脚本生成的）".to_string())?;
    let text = a.text_content.trim().to_string();
    if text.chars().count() < 20 {
        return Err("这个网页找不到正文（可能要登录，或者内容是脚本生成的）".into());
    }
    Ok((a.title.trim().to_string(), text))
}

// ---------- images ----------

pub fn image_type(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" | "heif" => "image/heic",
        _ => return None,
    })
}

/// (width, height) by `sips`.
pub(crate) fn size_of(path: &Path) -> Option<(u32, u32)> {
    let out = Command::new("/usr/bin/sips").args(["-g", "pixelWidth", "-g", "pixelHeight"]).arg(path).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let get = |k: &str| text.lines().find_map(|l| l.trim().strip_prefix(k).map(|v| v.trim().parse::<u32>().ok())).flatten();
    Some((get("pixelWidth:")?, get("pixelHeight:")?))
}

/// The image as sent: (media type, base64). Shrunk to `MAX_SIDE` and HEIC / big ones made JPEG, in
/// a temporary copy (`tmp_dir`), never the stored one.
pub fn image_for_model(path: &Path, media: &str, tmp_dir: &Path) -> Result<(String, String), String> {
    let (w, h) = size_of(path).unwrap_or((0, 0));
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let heic = media == "image/heic";
    // kinds a document may hold that models don't take
    let converted = media == "image/tiff" || media == "image/bmp";
    let shrink = w.max(h) > MAX_SIDE;
    if !heic && !converted && !shrink && bytes <= 4 * 1024 * 1024 {
        let data = std::fs::read(path).map_err(|e| format!("读不了图片：{e}"))?;
        return Ok((media.to_string(), base64(&data)));
    }
    std::fs::create_dir_all(tmp_dir).map_err(|e| format!("没能准备图片：{e}"))?;
    let jpeg = heic || converted || bytes > 4 * 1024 * 1024 || media == "image/gif" || media == "image/webp";
    let out = tmp_dir.join(format!("send-{}.{}", std::process::id(), if jpeg { "jpg" } else { "png" }));
    let mut cmd = Command::new("/usr/bin/sips");
    if shrink {
        cmd.args(["-Z", &MAX_SIDE.to_string()]);
    }
    if jpeg {
        cmd.args(["-s", "format", "jpeg", "-s", "formatOptions", "85"]);
    }
    // paths are absolute (never taken for an option)
    let ok = cmd.arg(path).arg("--out").arg(&out).output().map(|o| o.status.success()).unwrap_or(false);
    let data = if ok { std::fs::read(&out).ok() } else { None };
    let _ = std::fs::remove_file(&out);
    let data = data.ok_or("没能缩小这张图片")?;
    Ok((if jpeg { "image/jpeg" } else { media }.to_string(), base64(&data)))
}

// ---------- images inside documents ----------

/// An image a Word or PowerPoint file holds, or a PDF page with images.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedded {
    /// Where it is, for the model and the citation.
    pub place: String,
    /// Its part of the zip (Word, PowerPoint).
    pub part: String,
    /// Its page (1-based; PDF).
    pub page: Option<usize>,
    /// None: a kind that can't be sent (EMF, WMF, SVG…).
    pub media: Option<&'static str>,
}

/// Word, PowerPoint or PDF: the kinds whose images can be looked at.
pub fn holds_images(ext: &str) -> bool {
    matches!(ext, "docx" | "pptx" | "pdf")
}

fn embedded_media(part: &str) -> Option<&'static str> {
    let ext = part.rsplit_once('.').map(|(_, x)| x.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "tif" | "tiff" => Some("image/tiff"),
        "bmp" => Some("image/bmp"),
        e => image_type(e),
    }
}

fn zip_text(data: &[u8], name: &str) -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).ok()?;
    let mut text = String::new();
    zip.by_name(name).ok()?.take(50 * 1024 * 1024).read_to_string(&mut text).ok()?;
    Some(text)
}

/// A part's relationships: id → its path in the zip (`dir`: the folder of the part they belong to).
fn relations(data: &[u8], rels: &str, dir: &str) -> std::collections::HashMap<String, String> {
    use quick_xml::events::Event;
    let mut out = std::collections::HashMap::new();
    let Some(xml) = zip_text(data, rels) else { return out };
    let mut r = quick_xml::Reader::from_str(xml.as_str());
    loop {
        match r.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == "Relationship" => {
                if let (Some(id), Some(target)) = (super::word::attr(&e, "Id"), super::word::attr(&e, "Target")) {
                    let mut path: Vec<&str> = if target.starts_with('/') { vec![] } else { dir.split('/').collect() };
                    for piece in target.trim_start_matches('/').split('/') {
                        match piece {
                            ".." => {
                                path.pop();
                            }
                            "." => {}
                            p => path.push(p),
                        }
                    }
                    out.insert(id, path.join("/"));
                }
            }
            Ok(Event::Eof) | Err(_) => return out,
            _ => {}
        }
    }
}

fn short(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 24 { format!("{}…", flat.chars().take(24).collect::<String>()) } else { flat }
}

/// The images of a Word document in reading order, each placed by the text near it.
fn word_images(data: &[u8]) -> Result<Vec<Embedded>, String> {
    use quick_xml::events::Event;
    let bad = || "读不了这个 Word 文件，可能已损坏或加了密码".to_string();
    let xml = zip_text(data, "word/document.xml").ok_or_else(bad)?;
    let rels = relations(data, "word/_rels/document.xml.rels", "word");
    let mut r = quick_xml::Reader::from_str(xml.as_str());
    let (mut para, mut last, mut in_text) = (String::new(), String::new(), false);
    let mut out: Vec<Embedded> = vec![];
    let mut add = |id: Option<String>, near: &str| {
        let Some(part) = id.and_then(|i| rels.get(&i).cloned()) else { return };
        if !out.iter().any(|e| e.part == part) {
            let media = embedded_media(&part);
            out.push(Embedded { place: if near.is_empty() { "在文档开头".into() } else { format!("在「{}」那里", short(near)) }, part, page: None, media });
        }
    };
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == "t" => in_text = true,
            Ok(Event::End(e)) if e.local_name().as_ref() == "t" => in_text = false,
            Ok(Event::Text(t)) if in_text => para.push_str(&t.into_inner()),
            Ok(Event::End(e)) if e.local_name().as_ref() == "p" => {
                if !para.trim().is_empty() {
                    last = std::mem::take(&mut para);
                }
                para.clear();
            }
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == "blip" => add(super::word::attr(&e, "embed"), if para.trim().is_empty() { &last } else { &para }),
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == "imagedata" => add(super::word::attr(&e, "id"), if para.trim().is_empty() { &last } else { &para }),
            Ok(Event::Eof) | Err(_) => return Ok(out),
            _ => {}
        }
    }
}

/// The images of a PowerPoint file, slide by slide.
fn slide_images(data: &[u8]) -> Result<Vec<Embedded>, String> {
    use quick_xml::events::Event;
    let zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|_| "读不了这个 PowerPoint 文件，可能已损坏或加了密码".to_string())?;
    let mut slides: Vec<u32> = (0..zip.len()).filter_map(|i| zip.name_for_index(i)?.ok()?.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse().ok()).collect();
    slides.sort();
    let mut out = vec![];
    for n in slides {
        let Some(xml) = zip_text(data, &format!("ppt/slides/slide{n}.xml")) else { continue };
        let rels = relations(data, &format!("ppt/slides/_rels/slide{n}.xml.rels"), "ppt/slides");
        let mut seen: Vec<String> = vec![];
        let mut r = quick_xml::Reader::from_str(xml.as_str());
        loop {
            match r.read_event() {
                Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == "blip" => {
                    if let Some(part) = super::word::attr(&e, "embed").and_then(|i| rels.get(&i).cloned()) {
                        if !seen.contains(&part) {
                            seen.push(part.clone());
                            let media = embedded_media(&part);
                            out.push(Embedded { place: format!("第 {n} 页"), part, page: None, media });
                        }
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
    }
    Ok(out)
}

/// What a document holds: its images, a PDF's pages with images.
pub fn embedded_images(data: &[u8], ext: &str) -> Result<Vec<Embedded>, String> {
    match ext {
        "docx" => word_images(data),
        "pptx" => slide_images(data),
        "pdf" => Ok(super::pdf::image_pages(data)?.into_iter().map(|(page, n)| Embedded { place: if n > 1 { format!("第 {page} 页（{n} 张图）") } else { format!("第 {page} 页") }, part: String::new(), page: Some(page), media: Some("image/jpeg") }).collect()),
        _ => Err("这种文件里的图片看不了（能看 .docx、.pptx、.pdf 的）".into()),
    }
}

/// The list the model reads (`view=images`).
pub fn images_text(name: &str, data: &[u8], ext: &str) -> Result<String, String> {
    let list = embedded_images(data, ext)?;
    if ext == "pdf" {
        let pages = super::pdf::pages(data)?;
        let blank: Vec<String> = pages.iter().enumerate().filter(|(_, t)| t.trim().is_empty()).map(|(i, _)| (i + 1).to_string()).collect();
        let mut lines = vec![format!("《{name}》共 {} 页，view=look 用 page 看某一页的样子（包括它的图和图表）。", pages.len())];
        lines.push(if list.is_empty() { "没有页面嵌着图片。".to_string() } else { format!("有图片的页：{}", list.iter().map(|e| e.place.as_str()).collect::<Vec<_>>().join("、")) });
        if !blank.is_empty() {
            lines.push(format!("没有文字的页（可能是扫描件）：第 {} 页", blank.join("、")));
        }
        return Ok(lines.join("\n"));
    }
    if list.is_empty() {
        return Ok(format!("《{name}》里没有图片。"));
    }
    let lines: Vec<String> = list.iter().enumerate().map(|(i, e)| format!("图 {}：{}{}", i + 1, e.place, if e.media.is_none() { "（这种矢量图看不了）" } else { "" })).collect();
    Ok(format!("《{name}》里有 {} 张图片，view=look 用 image 写图号：\n{}", list.len(), lines.join("\n")))
}

/// One image of a document as sent (`view=look`: `image`, its number; a PDF's `page`): (where it is, media type, base64).
pub fn look_at(data: &[u8], ext: &str, args: &serde_json::Value, tmp_dir: &Path) -> Result<(String, String, String), String> {
    let num = |k: &str| args[k].as_u64().or_else(|| args[k].as_str().and_then(|s| s.trim().parse().ok())).map(|n| n as usize);
    if ext == "pdf" {
        let page = num("page").or_else(|| num("image")).ok_or("page 写要看第几页")?;
        let count = super::pdf::pages(data)?.len();
        if page == 0 || page > count {
            return Err(format!("只有 {count} 页"));
        }
        let jpeg = super::pdf::page_jpeg(data, page - 1, MAX_SIDE as f64)?;
        return Ok((format!("第 {page} 页"), "image/jpeg".into(), base64(&jpeg)));
    }
    let list = embedded_images(data, ext)?;
    if list.is_empty() {
        return Err("这个文件里没有图片".into());
    }
    let n = num("image").ok_or("image 写图号（view=images 列出的）")?;
    let e = n.checked_sub(1).and_then(|i| list.get(i)).ok_or_else(|| format!("只有 {} 张图片", list.len()))?;
    let media = e.media.ok_or("这张是矢量图，看不了")?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| e.to_string())?;
    let mut bytes = vec![];
    zip.by_name(&e.part).map_err(|_| "这张图片在文件里找不到".to_string())?.take(200 * 1024 * 1024).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(tmp_dir).map_err(|e| format!("没能准备图片：{e}"))?;
    let ext_of = e.part.rsplit_once('.').map_or("png", |(_, x)| x);
    let file = tmp_dir.join(format!("embedded-{}-{n}.{ext_of}", std::process::id()));
    std::fs::write(&file, &bytes).map_err(|e| format!("没能准备图片：{e}"))?;
    let sent = image_for_model(&file, media, tmp_dir);
    let _ = std::fs::remove_file(&file);
    let (media_type, b64) = sent?;
    Ok((format!("第 {n} 张图（{}）", e.place), media_type, b64))
}

/// How many images a Word or PowerPoint file holds, by its media folder (quick, for the system message).
pub fn media_count(path: &Path, ext: &str) -> usize {
    let folder = match ext {
        "docx" => "word/media/",
        "pptx" => "ppt/media/",
        _ => return 0,
    };
    let Ok(file) = std::fs::File::open(path) else { return 0 };
    let Ok(zip) = zip::ZipArchive::new(file) else { return 0 };
    zip.file_names().filter(|n| n.as_ref().is_ok_and(|n| n.starts_with(folder))).count()
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// A 1×1 PNG (tests).
#[cfg(test)]
pub fn tests_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
        0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
        0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn csv_quotes_tabs_and_trims() {
        let rows = parse_csv("名称,备注\r\n\"苹果, 红\",\"说 \"\"好\"\"\"\n香蕉,\n,\n");
        assert_eq!(rows, [vec!["名称", "备注"], vec!["苹果, 红", "说 \"好\""], vec!["香蕉", ""], vec!["", ""]]);
        let s = trimmed(Sheet { name: "t".into(), rows });
        assert_eq!(s.data_rows(), 2);
        assert_eq!(parse_csv("a\tb\n1\t2")[1], ["1", "2"]);
    }

    fn sheet(n: usize) -> Sheet {
        let mut rows = vec![vec!["城市".to_string(), "人口".to_string()]];
        rows.extend((1..=n).map(|i| vec![format!("城{i}"), format!("{}", i * 10)]));
        Sheet { name: "Sheet1".into(), rows }
    }

    #[test]
    fn small_tables_whole_big_ones_previewed_and_queried() {
        let (segs, total) = table_segments(&[sheet(3)], "城市.xlsx", false);
        assert_eq!(total, 3);
        assert!(segs[0].content.contains("共 3 行") && segs[0].content.contains("3. | 城3 | 30 |"), "{}", segs[0].content);
        let big = [sheet(500)];
        let (segs, total) = table_segments(&big, "城市.xlsx", false);
        assert_eq!((segs.len(), total), (1, 500));
        assert!(segs[0].content.contains("前 50 行") && segs[0].content.contains("50. | 城50") && !segs[0].content.contains("51. |"));
        // 知识库: every row
        let (segs, _) = table_segments(&big, "城市.xlsx", true);
        assert!(segs.iter().any(|s| s.content.contains("500. | 城500")) && !segs[0].content.contains("pomelo_table"));
        let r = table_query(&big, &json!({ "view": "rows", "from": 499 })).unwrap();
        assert!(r.contains("找到 2 行") && r.contains("500. | 城500 | 5000 |"), "{r}");
        let r = table_query(&big, &json!({ "view": "column", "column": "人口", "value": "4990" })).unwrap();
        assert!(r.contains("499. | 城499"), "{r}");
        let r = table_query(&big, &json!({ "view": "find", "keyword": "城4" })).unwrap();
        assert!(r.contains("找到 111 行"), "{r}"); // 城4, 城40–49, 城400–499
        assert!(table_query(&big, &json!({ "view": "column", "column": "面积", "value": "1" })).unwrap_err().contains("城市、人口"));
    }

    #[test]
    fn slide_text_by_paragraph() {
        let xml = r#"<p:sld xmlns:a="a" xmlns:p="p"><p:txBody><a:p><a:r><a:t>标题 &amp; 副</a:t></a:r><a:r><a:t>题</a:t></a:r></a:p><a:p><a:r><a:t>第二段</a:t></a:r></a:p><a:p></a:p></p:txBody></p:sld>"#;
        assert_eq!(slide_text(xml), "标题 & 副题\n第二段");
    }

    #[test]
    fn web_article_markdown() {
        let html = format!("<html><head><title>租房笔记</title></head><body><nav>菜单 首页</nav><article><h1>租房笔记</h1><p>{}</p><p>押一付三，合同看清楚。</p></article></body></html>", "看房要注意采光、噪音和通勤时间。".repeat(20));
        let (title, text) = web_article(&html, "https://example.com/a").unwrap();
        assert_eq!(title, "租房笔记");
        assert!(text.contains("押一付三") && !text.contains("菜单"), "{text}");
        assert!(web_article("<html><body></body></html>", "https://example.com").is_err());
    }

    fn zipped(parts: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in parts {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    const RELS: &str = r#"<Relationships xmlns="p"><Relationship Id="rId1" Type="t" Target="media/image1.png"/><Relationship Id="rId2" Type="t" Target="/word/media/chart.emf"/><Relationship Id="rId3" Type="t" Target="../media/image1.png"/></Relationships>"#;

    #[test]
    fn images_inside_word_and_powerpoint() {
        let png = tests_png();
        let doc = r#"<w:document xmlns:w="w" xmlns:a="a" xmlns:r="r"><w:body><w:p><w:r><w:t>第一段说明这张图的文字</w:t></w:r></w:p><w:p><w:r><w:drawing><a:blip r:embed="rId1"/></w:drawing></w:r></w:p><w:p><w:r><w:t>第二段</w:t></w:r><w:r><w:drawing><a:blip r:embed="rId2"/></w:drawing></w:r></w:p></w:body></w:document>"#;
        let docx = zipped(&[("word/document.xml", doc.as_bytes()), ("word/_rels/document.xml.rels", RELS.as_bytes()), ("word/media/image1.png", &png), ("word/media/chart.emf", b"emf")]);
        let list = embedded_images(&docx, "docx").unwrap();
        assert_eq!(list.iter().map(|e| (e.place.as_str(), e.part.as_str(), e.media)).collect::<Vec<_>>(), [("在「第一段说明这张图的文字」那里", "word/media/image1.png", Some("image/png")), ("在「第二段」那里", "word/media/chart.emf", None)]);
        let text = images_text("报告.docx", &docx, "docx").unwrap();
        assert!(text.contains("有 2 张图片") && text.contains("图 2：在「第二段」那里（这种矢量图看不了）"), "{text}");
        let dir = crate::db::tests::temp_dir("office-embedded");
        let (place, media, data) = look_at(&docx, "docx", &json!({ "image": 1 }), &dir).unwrap();
        assert_eq!((place.as_str(), media.as_str(), data.is_empty()), ("第 1 张图（在「第一段说明这张图的文字」那里）", "image/png", false));
        assert!(look_at(&docx, "docx", &json!({ "image": 2 }), &dir).unwrap_err().contains("矢量图"));
        assert!(look_at(&docx, "docx", &json!({ "image": 9 }), &dir).unwrap_err().contains("只有 2 张"));
        assert!(look_at(&docx, "docx", &json!({}), &dir).unwrap_err().contains("图号"));
        // PowerPoint: by slide, in slide order
        let slide = r#"<p:sld xmlns:p="p" xmlns:a="a" xmlns:r="r"><a:blip r:embed="rId3"/><a:blip r:embed="rId3"/></p:sld>"#;
        let pptx = zipped(&[("ppt/slides/slide2.xml", slide.as_bytes()), ("ppt/slides/_rels/slide2.xml.rels", RELS.as_bytes()), ("ppt/slides/slide1.xml", b"<p:sld xmlns:p=\"p\"/>"), ("ppt/media/image1.png", &png)]);
        let list = embedded_images(&pptx, "pptx").unwrap();
        assert_eq!(list.iter().map(|e| (e.place.as_str(), e.part.as_str())).collect::<Vec<_>>(), [("第 2 页", "ppt/media/image1.png")]);
        assert_eq!(look_at(&pptx, "pptx", &json!({ "image": 1 }), &dir).unwrap().0, "第 1 张图（第 2 页）");
        let f = dir.join("a.pptx");
        std::fs::write(&f, &pptx).unwrap();
        assert_eq!(media_count(&f, "pptx"), 1);
        // none, and a kind that holds none
        let plain = zipped(&[("word/document.xml", b"<w:document xmlns:w=\"w\"/>")]);
        assert!(images_text("空.docx", &plain, "docx").unwrap().contains("没有图片"));
        assert!(embedded_images(&plain, "xlsx").is_err());
    }

    #[test]
    fn a_pdf_is_looked_at_page_by_page() {
        let pdf = crate::picky::pdf::tests::sample_pdf(&["Page one", ""]);
        let text = images_text("扫描.pdf", &pdf, "pdf").unwrap();
        assert!(text.contains("共 2 页") && text.contains("没有页面嵌着图片") && text.contains("没有文字的页（可能是扫描件）：第 2 页"), "{text}");
        let dir = crate::db::tests::temp_dir("office-pdf");
        let (place, media, data) = look_at(&pdf, "pdf", &json!({ "page": 1 }), &dir).unwrap();
        assert_eq!((place.as_str(), media.as_str()), ("第 1 页", "image/jpeg"));
        assert!(data.starts_with("/9j/")); // JPEG
        assert!(look_at(&pdf, "pdf", &json!({ "page": 3 }), &dir).unwrap_err().contains("只有 2 页"));
    }

    #[test]
    fn base64_like_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
