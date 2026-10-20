//! More kinds of files for 小拾 (v0.4.5), turned into what files.rs stores:
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
    let shrink = w.max(h) > MAX_SIDE;
    if !heic && !shrink && bytes <= 4 * 1024 * 1024 {
        let data = std::fs::read(path).map_err(|e| format!("读不了图片：{e}"))?;
        return Ok((media.to_string(), base64(&data)));
    }
    std::fs::create_dir_all(tmp_dir).map_err(|e| format!("没能准备图片：{e}"))?;
    let jpeg = heic || bytes > 4 * 1024 * 1024 || media == "image/gif" || media == "image/webp";
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

    #[test]
    fn base64_like_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
