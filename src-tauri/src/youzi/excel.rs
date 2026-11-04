//! Excel (.xlsx) for the Excel 工具箱 (v0.8), no library beyond zip / quick-xml (calamine only reads):
//! - `Book::open` / `read_view`: sheets as rows of addressed cells for the model: text, numbers, dates
//!   (a date-formatted serial as 2026-10-01), percents (a %-formatted number as 12.5%), formulas (=A1*2 with
//!   their last result; a shared formula's child shown as its own). What can't change is said up front:
//!   a protected sheet, merged cells (only the top-left), array formulas.
//! - `Book::plan`: one proposed change checked (the card shows its before and after): a cell's new value,
//!   rows added after a row, a row deleted. Numbering is always the file's as read.
//! - `apply`: the ticked changes written. Only the sheets concerned are rebuilt (unchanged cells keep their
//!   source XML; everything else in the file is copied as it is). Values: `=…` a formula (checked: quotes and
//!   brackets balance; newer functions get the `_xlfn.` Excel wants, dynamic arrays are refused), `'…` text as
//!   written, numbers / percents / dates by what they look like, keeping the cell's own style (a date or percent
//!   in a plain cell gets a copy of its style with that format); a cell of numeric-looking text (a code, 001)
//!   stays text. Text goes in as an inline string: sharedStrings.xml stays as it is.
//! - Adding and deleting rows moves what's below: row numbers, formulas in every sheet (A1 references with $,
//!   ranges, whole rows, Sheet!refs; a deleted cell becomes #REF!, a range shrinks), shared formulas (expanded
//!   first), array ranges, merged cells, conditional formats, data validations, hyperlinks, filters, defined
//!   names. A sheet with what can't be moved safely (a chart or picture, a table object, comments, a pivot table,
//!   extension lists) can't add or delete rows, nor can a formula that spans sheets (Sheet1:Sheet3!A1) move.
//!   A new row copies the look of the row above it (its height and each cell's style).
//! - Any write drops calcChain.xml (Excel rebuilds it) and asks for a full recalculation on load: a formula's
//!   stored result is stale until Excel opens the file.
//! - `new_xlsx`: a new workbook from rows of values (the first row bold, columns as wide as their text).

use super::word::attr;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};

const MAX_ROW: u32 = 1_048_576;
const MAX_COL: u32 = 16_384;
const WORKBOOK: &str = "xl/workbook.xml";
const WORKBOOK_RELS: &str = "xl/_rels/workbook.xml.rels";
const STYLES: &str = "xl/styles.xml";
const STRINGS: &str = "xl/sharedStrings.xml";
const CONTENT_TYPES: &str = "[Content_Types].xml";
const CALC_CHAIN: &str = "xl/calcChain.xml";
/// numFmtId of a plain date and of 0.00%.
const FMT_DATE: u32 = 14;
const FMT_PERCENT: u32 = 10;
/// Excel's name for functions newer than the file format (else they show #NAME?).
const NEWER: &[&str] = &["TEXTJOIN", "CONCAT", "IFS", "SWITCH", "MAXIFS", "MINIFS", "XLOOKUP", "XMATCH", "LET"];
/// These spill over several cells, which takes metadata a plain formula doesn't have.
const SPILLING: &[&str] = &["FILTER", "SORT", "SORTBY", "UNIQUE", "SEQUENCE", "RANDARRAY", "LAMBDA"];

fn bad() -> String {
    "读不了这个 Excel 文件，可能已损坏或加了密码".into()
}

fn part(data: &[u8], name: &str) -> Result<Option<String>, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|_| bad())?;
    let Ok(f) = zip.by_name(name) else { return Ok(None) };
    let mut xml = String::new();
    f.take(500 * 1024 * 1024).read_to_string(&mut xml).map_err(|_| bad())?;
    Ok(Some(xml))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn esc_attr(s: &str) -> String {
    esc(s).replace('"', "&quot;")
}

fn unesc(s: &str) -> String {
    quick_xml::escape::unescape(s).map_or_else(|_| s.to_string(), |c| c.to_string())
}

/// `<t>` text inside some XML (a shared string, an inline one), phonetic hints left out.
fn t_text(xml: &str) -> String {
    let mut r = Reader::from_str(xml);
    let mut out = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == "t" => {
                let end = e.to_end().into_owned();
                if let Ok(span) = r.read_to_end(end.name()) {
                    out.push_str(&unesc(&xml[span.start as usize..span.end as usize]));
                }
            }
            Ok(Event::Start(e)) if e.local_name().as_ref() == "rPh" => {
                let end = e.to_end().into_owned();
                let _ = r.read_to_end(end.name());
            }
            Ok(Event::Eof) | Err(_) => return out,
            _ => {}
        }
    }
}

fn raw_attrs(e: &BytesStart, skip: &[&str]) -> String {
    let mut out = String::new();
    for a in e.attributes().flatten() {
        let key = a.key.as_ref().to_string();
        if !skip.contains(&key.as_str()) {
            out.push_str(&format!(" {key}=\"{}\"", a.value));
        }
    }
    out
}

// ---------- addresses ----------

pub fn col_name(mut col: u32) -> String {
    let mut out = vec![];
    while col > 0 {
        out.push((b'A' + ((col - 1) % 26) as u8) as char);
        col = (col - 1) / 26;
    }
    out.iter().rev().collect()
}

fn col_number(letters: &str) -> Option<u32> {
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut n = 0u32;
    for c in letters.chars() {
        if !c.is_ascii_uppercase() {
            return None;
        }
        n = n * 26 + (c as u32 - 'A' as u32 + 1);
    }
    (n <= MAX_COL).then_some(n)
}

/// "B12" → (2, 12).
pub fn parse_addr(s: &str) -> Option<(u32, u32)> {
    let s = s.trim().to_uppercase().replace('$', "");
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (col, row) = s.split_at(split);
    let (c, r) = (col_number(col)?, row.parse::<u32>().ok()?);
    (r >= 1 && r <= MAX_ROW).then_some((c, r))
}

fn parse_range(s: &str) -> Option<(u32, u32, u32, u32)> {
    let (a, b) = s.split_once(':').unwrap_or((s, s));
    let (c1, r1) = parse_addr(a)?;
    let (c2, r2) = parse_addr(b)?;
    Some((c1.min(c2), r1.min(r2), c1.max(c2), r1.max(r2)))
}

// ---------- rewriting references ----------

#[derive(Debug, Clone, Copy, PartialEq)]
struct End {
    col: u32,
    col_abs: bool,
    row: u32,
    row_abs: bool,
}

impl End {
    fn text(&self) -> String {
        format!("{}{}{}{}", if self.col_abs { "$" } else { "" }, col_name(self.col), if self.row_abs { "$" } else { "" }, self.row)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ref {
    One(End),
    Two(End, End),
    Rows((u32, bool), (u32, bool)),
    Cols((u32, bool), (u32, bool)),
}

impl Ref {
    fn text(&self) -> String {
        let d = |abs: bool| if abs { "$" } else { "" };
        match self {
            Ref::One(e) => e.text(),
            Ref::Two(a, b) => format!("{}:{}", a.text(), b.text()),
            Ref::Rows(a, b) => format!("{}{}:{}{}", d(a.1), a.0, d(b.1), b.0),
            Ref::Cols(a, b) => format!("{}{}:{}{}", d(a.1), col_name(a.0), d(b.1), col_name(b.0)),
        }
    }
}

/// `$A$1`, `A1` at the start of `chars[at..]`: the end and its length.
fn cell_at(chars: &[char], at: usize) -> Option<(End, usize)> {
    let mut i = at;
    let col_abs = chars.get(i) == Some(&'$');
    i += col_abs as usize;
    let c0 = i;
    while chars.get(i).is_some_and(char::is_ascii_uppercase) {
        i += 1;
    }
    let col = col_number(&chars[c0..i].iter().collect::<String>())?;
    let row_abs = chars.get(i) == Some(&'$');
    i += row_abs as usize;
    let r0 = i;
    while chars.get(i).is_some_and(char::is_ascii_digit) {
        i += 1;
    }
    let row: u32 = chars[r0..i].iter().collect::<String>().parse().ok()?;
    if row == 0 || row > MAX_ROW {
        return None;
    }
    Some((End { col, col_abs, row, row_abs }, i - at))
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.' || c == '\\' || !c.is_ascii() && !c.is_whitespace()
}

/// A reference at `at`: a cell, a range, whole rows (`3:5`) or columns (`A:C`).
fn ref_at(chars: &[char], at: usize) -> Option<(Ref, usize)> {
    let ends = |i: usize| !chars.get(i).is_some_and(|c| word_char(*c) || *c == '(' || *c == '!');
    if let Some((a, n)) = cell_at(chars, at) {
        if chars.get(at + n) == Some(&':') {
            if let Some((b, m)) = cell_at(chars, at + n + 1) {
                if ends(at + n + 1 + m) {
                    return Some((Ref::Two(a, b), n + 1 + m));
                }
            }
        }
        return ends(at + n).then_some((Ref::One(a), n));
    }
    let part = |mut i: usize, digits: bool| -> Option<((u32, bool), usize)> {
        let start = i;
        let abs = chars.get(i) == Some(&'$');
        i += abs as usize;
        let s = i;
        while chars.get(i).is_some_and(|c| if digits { c.is_ascii_digit() } else { c.is_ascii_uppercase() }) {
            i += 1;
        }
        let text: String = chars[s..i].iter().collect();
        let n = if digits { text.parse::<u32>().ok().filter(|r| *r >= 1 && *r <= MAX_ROW)? } else { col_number(&text)? };
        Some(((n, abs), i - start))
    };
    for digits in [true, false] {
        if let Some((a, n)) = part(at, digits) {
            if chars.get(at + n) == Some(&':') {
                if let Some((b, m)) = part(at + n + 1, digits) {
                    if ends(at + n + 1 + m) {
                        return Some((if digits { Ref::Rows(a, b) } else { Ref::Cols(a, b) }, n + 1 + m));
                    }
                }
            }
        }
    }
    None
}

/// Rewrites every reference of `formula` by `f` (the sheet it names, if any; None = this sheet; a reference
/// into another workbook isn't passed). Strings, names, functions and structured references stay.
fn rewrite(formula: &str, f: &mut dyn FnMut(Option<&str>, Ref) -> Result<String, String>) -> Result<String, String> {
    let chars: Vec<char> = formula.chars().collect();
    let mut out = String::with_capacity(formula.len());
    let mut i = 0;
    let mut sheet: Option<String> = None;
    let mut external = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                out.push(c);
                i += 1;
                while i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                    if chars[i - 1] == '"' {
                        if chars.get(i) == Some(&'"') {
                            out.push('"');
                            i += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
            '\'' => {
                // a quoted sheet name, then !
                let mut name = String::new();
                let mut j = i + 1;
                while j < chars.len() {
                    if chars[j] == '\'' {
                        if chars.get(j + 1) == Some(&'\'') {
                            name.push('\'');
                            j += 2;
                            continue;
                        }
                        break;
                    }
                    name.push(chars[j]);
                    j += 1;
                }
                let text: String = chars[i..=j.min(chars.len() - 1)].iter().collect();
                out.push_str(&text);
                i = j + 1;
                if chars.get(i) == Some(&'!') {
                    out.push('!');
                    i += 1;
                    if name.contains(':') {
                        return Err("有跨几张表的公式".into());
                    }
                    external = name.starts_with('[');
                    sheet = Some(name);
                }
            }
            '[' => {
                let mut depth = 0;
                while i < chars.len() {
                    out.push(chars[i]);
                    depth += (chars[i] == '[') as i32 - (chars[i] == ']') as i32;
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                }
                external = true;
            }
            _ => {
                if let Some((r, n)) = ref_at(&chars, i) {
                    if external {
                        out.push_str(&r.text());
                    } else {
                        out.push_str(&f(sheet.as_deref(), r)?);
                    }
                    i += n;
                    sheet = None;
                    external = false;
                } else if word_char(c) || c == '$' {
                    let s = i;
                    while i < chars.len() && (word_char(chars[i]) || chars[i] == '$') {
                        i += 1;
                    }
                    let word: String = chars[s..i].iter().collect();
                    out.push_str(&word);
                    if chars.get(i) == Some(&'!') {
                        out.push('!');
                        i += 1;
                        sheet = Some(word);
                    } else if chars.get(i) == Some(&':') && chars[i + 1..].iter().take_while(|c| word_char(**c)).count() > 0 && chars[i + 1..].iter().find(|c| !word_char(**c)) == Some(&'!') {
                        return Err("有跨几张表的公式".into());
                    } else {
                        sheet = None;
                        external = false;
                    }
                } else {
                    out.push(c);
                    i += 1;
                    if c != ':' {
                        sheet = None;
                        external = false;
                    }
                }
            }
        }
    }
    Ok(out)
}

/// A shared formula as its child at `(dcol, drow)` from the master has it.
fn translate(formula: &str, dcol: i64, drow: i64) -> String {
    let col = |n: u32, abs: bool| if abs { n } else { (n as i64 + dcol).clamp(1, MAX_COL as i64) as u32 };
    let row = |n: u32, abs: bool| if abs { n } else { (n as i64 + drow).clamp(1, MAX_ROW as i64) as u32 };
    let end = |e: End| End { col: col(e.col, e.col_abs), row: row(e.row, e.row_abs), ..e };
    rewrite(formula, &mut |_, r| {
        Ok(match r {
            Ref::One(e) => Ref::One(end(e)),
            Ref::Two(a, b) => Ref::Two(end(a), end(b)),
            Ref::Rows(a, b) => Ref::Rows((row(a.0, a.1), a.1), (row(b.0, b.1), b.1)),
            Ref::Cols(a, b) => Ref::Cols((col(a.0, a.1), a.1), (col(b.0, b.1), b.1)),
        }
        .text())
    })
    .unwrap_or_else(|_| formula.to_string())
}

/// Rows inserted or one deleted.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Move {
    Insert { after: u32, count: u32 },
    Delete { row: u32 },
}

impl Move {
    fn one(self, row: u32) -> Option<u32> {
        match self {
            Move::Insert { after, count } => Some(if row > after { row + count } else { row }),
            Move::Delete { row: gone } => match row.cmp(&gone) {
                std::cmp::Ordering::Less => Some(row),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(row - 1),
            },
        }
    }
    /// A range's rows; None when all of them went.
    fn span(self, a: u32, b: u32) -> Option<(u32, u32)> {
        match self {
            Move::Insert { .. } => Some((self.one(a)?, self.one(b)?)),
            Move::Delete { row } => {
                if a == row && b == row {
                    return None;
                }
                let a = if a == row { row } else { self.one(a)? };
                let b = if b == row { row - 1 } else { self.one(b)? };
                Some((a, b))
            }
        }
    }
}

/// A formula with `sheet`'s rows moved (`here`: the sheet the formula is on).
fn moved_formula(formula: &str, here: &str, sheet: &str, m: Move) -> Result<String, String> {
    rewrite(formula, &mut |named, r| {
        if named.unwrap_or(here) != sheet {
            return Ok(r.text());
        }
        Ok(match r {
            Ref::One(e) => match m.one(e.row) {
                Some(row) => Ref::One(End { row, ..e }).text(),
                None => "#REF!".into(),
            },
            Ref::Two(a, b) => match m.span(a.row.min(b.row), a.row.max(b.row)) {
                Some((x, y)) => {
                    let (x, y) = if a.row <= b.row { (x, y) } else { (y, x) };
                    Ref::Two(End { row: x, ..a }, End { row: y, ..b }).text()
                }
                None => "#REF!".into(),
            },
            Ref::Rows(a, b) => match m.span(a.0.min(b.0), a.0.max(b.0)) {
                Some((x, y)) => {
                    let (x, y) = if a.0 <= b.0 { (x, y) } else { (y, x) };
                    Ref::Rows((x, a.1), (y, b.1)).text()
                }
                None => "#REF!".into(),
            },
            Ref::Cols(..) => r.text(),
        })
    })
}

/// Rows added right after the last row of a range, by a formula outside the range's rows (a 合计 under its
/// items, or above them): the range takes the new rows in. Excel leaves it short; the toolbox adds rows to a
/// list, so it grows. `row`: the formula's row; `after`, `count`: the rows added (numbers before the move).
fn grows(formula: &str, here: &str, row: u32, after: u32) -> bool {
    let mut hit = false;
    let _ = rewrite(formula, &mut |named, r| {
        if let Ref::Two(a, b) = r {
            let (lo, hi) = (a.row.min(b.row), a.row.max(b.row));
            hit |= named.is_none_or(|n| n == here) && hi == after && lo < after && (row > after || row < lo);
        }
        Ok(r.text())
    });
    hit
}

/// `grows`' ranges taken to the last added row, in a formula already moved (they still end at `after`).
fn grown_formula(formula: &str, here: &str, row: u32, after: u32, count: u32) -> String {
    rewrite(formula, &mut |named, r| {
        Ok(match r {
            Ref::Two(a, b) if named.is_none_or(|n| n == here) && a.row.max(b.row) == after && a.row.min(b.row) < after && (row > after || row < a.row.min(b.row)) => {
                if a.row > b.row { Ref::Two(End { row: after + count, ..a }, b) } else { Ref::Two(a, End { row: after + count, ..b }) }.text()
            }
            r => r.text(),
        })
    })
    .unwrap_or_else(|_| formula.to_string())
}

/// Whether a formula at (col, row) of `here` names that cell itself (a loop Excel can't work out).
fn names_itself(formula: &str, here: &str, col: u32, row: u32) -> bool {
    let mut hit = false;
    let _ = rewrite(formula, &mut |named, r| {
        if named.is_none_or(|n| n == here) {
            hit |= match r {
                Ref::One(e) => (e.col, e.row) == (col, row),
                Ref::Two(a, b) => col >= a.col.min(b.col) && col <= a.col.max(b.col) && row >= a.row.min(b.row) && row <= a.row.max(b.row),
                Ref::Rows(a, b) => row >= a.0.min(b.0) && row <= a.0.max(b.0),
                Ref::Cols(a, b) => col >= a.0.min(b.0) && col <= a.0.max(b.0),
            };
        }
        Ok(r.text())
    });
    hit
}

/// A space-separated list of ranges moved; None when any of it went.
fn moved_sqref(sqref: &str, m: Move) -> Option<String> {
    let mut out = vec![];
    for piece in sqref.split_whitespace() {
        let (a, b) = piece.split_once(':').map_or((piece, None), |(a, b)| (a, Some(b)));
        let (ca, ra) = parse_addr(a)?;
        match b {
            None => out.push(format!("{}{}", col_name(ca), m.one(ra)?)),
            Some(b) => {
                let (cb, rb) = parse_addr(b)?;
                let (x, y) = m.span(ra, rb)?;
                out.push(format!("{}{x}:{}{y}", col_name(ca), col_name(cb)));
            }
        }
    }
    Some(out.join(" "))
}

// ---------- the file as read ----------

#[derive(Debug, Clone, PartialEq)]
enum FKind {
    Normal,
    Shared,
    Array,
    Table,
}

#[derive(Debug, Clone)]
struct Formula {
    kind: FKind,
    /// Its text (a shared child has none).
    text: String,
    si: Option<u32>,
    reference: Option<String>,
    /// Other attributes, as they were.
    attrs: String,
}

#[derive(Debug, Clone)]
struct Cell {
    col: u32,
    s: Option<u32>,
    t: Option<String>,
    other: String,
    f: Option<Formula>,
    /// `<v>` and `<is>` as they were.
    v: Option<String>,
    is_raw: Option<String>,
    rest: String,
    /// Its XML as read; None once changed.
    raw: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct Row {
    r: u32,
    /// Its attributes without r and spans.
    attrs: String,
    cells: Vec<Cell>,
}

#[derive(Debug, Clone)]
struct Sheet {
    name: String,
    part: String,
    hidden: bool,
    protected: bool,
    /// Prefix of its elements (`x:` or none).
    pfx: String,
    head: String,
    rows: Vec<Row>,
    tail: String,
    merged: Vec<(u32, u32, u32, u32)>,
    /// Why rows can't be added or deleted (empty: they can).
    fixed: Vec<&'static str>,
    /// (first col, last col, style) of `<cols>`.
    cols: Vec<(u32, u32, u32)>,
    changed: bool,
}

#[derive(Debug, Clone, Default)]
struct Styles {
    /// Each cell format: its numFmtId and XML.
    xfs: Vec<(u32, String)>,
    custom: HashMap<u32, String>,
    /// How many were in the file.
    read: usize,
}

fn is_date_code(code: &str) -> bool {
    let mut plain = String::new();
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for x in chars.by_ref() {
                    if x == '"' {
                        break;
                    }
                }
            }
            '[' => {
                for x in chars.by_ref() {
                    if x == ']' {
                        break;
                    }
                }
            }
            '\\' => {
                chars.next();
            }
            c => plain.push(c.to_ascii_lowercase()),
        }
    }
    plain != "general" && plain.contains(['y', 'm', 'd', 'h', 's'])
}

impl Styles {
    fn parse(xml: &str) -> Styles {
        let mut out = Styles::default();
        let mut r = Reader::from_str(xml);
        let mut in_xfs = false;
        loop {
            let at = r.buffer_position() as usize;
            match r.read_event() {
                Ok(Event::Start(e)) if e.local_name().as_ref() == "cellXfs" => in_xfs = true,
                Ok(Event::End(e)) if e.local_name().as_ref() == "cellXfs" => in_xfs = false,
                Ok(Event::Empty(e)) if e.local_name().as_ref() == "numFmt" => {
                    if let (Some(id), Some(code)) = (attr(&e, "numFmtId").and_then(|v| v.parse().ok()), attr(&e, "formatCode")) {
                        out.custom.insert(id, code);
                    }
                }
                Ok(Event::Empty(e)) if in_xfs && e.local_name().as_ref() == "xf" => {
                    let id = attr(&e, "numFmtId").and_then(|v| v.parse().ok()).unwrap_or(0);
                    out.xfs.push((id, xml[at..r.buffer_position() as usize].to_string()));
                }
                Ok(Event::Start(e)) if in_xfs && e.local_name().as_ref() == "xf" => {
                    let id = attr(&e, "numFmtId").and_then(|v| v.parse().ok()).unwrap_or(0);
                    let end = e.to_end().into_owned();
                    let _ = r.read_to_end(end.name());
                    out.xfs.push((id, xml[at..r.buffer_position() as usize].to_string()));
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        out.read = out.xfs.len();
        out
    }
    fn fmt(&self, s: Option<u32>) -> u32 {
        self.xfs.get(s.unwrap_or(0) as usize).map_or(0, |x| x.0)
    }
    fn is_date(&self, s: Option<u32>) -> bool {
        let id = self.fmt(s);
        matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58) || self.custom.get(&id).is_some_and(|c| is_date_code(c))
    }
    fn is_percent(&self, s: Option<u32>) -> bool {
        let id = self.fmt(s);
        matches!(id, 9 | 10) || self.custom.get(&id).is_some_and(|c| c.contains('%'))
    }
    /// A copy of format `base` with number format `fmt` (the same one again if already made).
    fn with_fmt(&mut self, base: Option<u32>, fmt: u32) -> Option<u32> {
        let xml = self.xfs.get(base.unwrap_or(0) as usize)?.1.clone();
        let mut r = Reader::from_str(xml.as_str());
        let Ok(Event::Start(e) | Event::Empty(e)) = r.read_event() else { return None };
        let start_end = r.buffer_position() as usize;
        let attrs = raw_attrs(&e, &["numFmtId", "applyNumberFormat"]);
        let closes = xml[..start_end].ends_with("/>");
        let tag = e.name().as_ref().to_string();
        let new = format!("<{tag} numFmtId=\"{fmt}\" applyNumberFormat=\"1\"{attrs}{}{}", if closes { "/>" } else { ">" }, &xml[start_end..]);
        if let Some(i) = self.xfs.iter().position(|x| x.1 == new) {
            return Some(i as u32);
        }
        self.xfs.push((fmt, new));
        Some(self.xfs.len() as u32 - 1)
    }
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    // days since 1970-01-01 → (y, m, d)
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - (m <= 2) as i64;
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Days from 1970-01-01 to Excel's serial 0 (1899-12-30, which counts its leap day that never was).
const EPOCH_1900: i64 = -25_569;
const EPOCH_1904: i64 = -24_107;

fn serial_text(x: f64, date1904: bool) -> String {
    let day = x.floor() as i64;
    let secs = ((x - x.floor()) * 86_400.0).round() as i64;
    let (hh, mm, ss) = (secs / 3600, secs % 3600 / 60, secs % 60);
    let time = if ss != 0 { format!("{hh:02}:{mm:02}:{ss:02}") } else { format!("{hh:02}:{mm:02}") };
    if day == 0 {
        return time;
    }
    let (y, m, d) = civil_from_days(day + if date1904 { EPOCH_1904 } else { EPOCH_1900 });
    if secs == 0 { format!("{y}-{m:02}-{d:02}") } else { format!("{y}-{m:02}-{d:02} {time}") }
}

/// 2026-10-01 / 2026/10/1, with a time or not → Excel's serial.
fn parse_date(s: &str, date1904: bool) -> Option<f64> {
    let s = s.trim();
    let (date, time) = s.split_once(' ').map_or((s, None), |(a, b)| (a, Some(b)));
    let parts: Vec<&str> = date.split(['-', '/']).collect();
    if parts.len() != 3 || parts[0].len() != 4 {
        return None;
    }
    let (y, m, d) = (parts[0].parse::<i64>().ok()?, parts[1].parse::<u32>().ok()?, parts[2].parse::<u32>().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || civil_from_days(days_from_civil(y, m, d)) != (y, m, d) {
        return None;
    }
    let mut x = (days_from_civil(y, m, d) - if date1904 { EPOCH_1904 } else { EPOCH_1900 }) as f64;
    if let Some(t) = time {
        let bits: Vec<u32> = t.trim().split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
        if !(2..=3).contains(&bits.len()) || bits[0] > 23 || bits[1] > 59 || bits.get(2).is_some_and(|s| *s > 59) {
            return None;
        }
        x += (bits[0] * 3600 + bits[1] * 60 + bits.get(2).copied().unwrap_or(0)) as f64 / 86_400.0;
    }
    Some(x)
}

fn number_text(raw: &str) -> String {
    match raw.parse::<f64>() {
        Ok(x) if raw.len() > 16 => {
            let r = (x * 1e10).round() / 1e10;
            r.to_string()
        }
        _ => raw.to_string(),
    }
}

fn plain_number(s: &str) -> Option<f64> {
    let digits = s.trim_start_matches(['-', '+']);
    let leading_zero = digits.len() > 1 && digits.starts_with('0') && !digits.starts_with("0.");
    if s.is_empty() || leading_zero || digits.chars().filter(char::is_ascii_digit).count() > 15 || !digits.chars().all(|c| c.is_ascii_digit() || c == '.') || digits.is_empty() {
        return None;
    }
    s.parse::<f64>().ok().filter(|x| x.is_finite())
}

fn no_prefix(formula: &str) -> String {
    formula.replace("_xlfn._xlws.", "").replace("_xlfn.", "")
}

/// A formula as it goes into the file: checked, newer functions named as Excel wants them.
fn formula_for_file(text: &str) -> Result<String, String> {
    let (mut depth, mut quoted, mut sheet_quoted) = (0i32, false, false);
    for c in text.chars() {
        match c {
            '"' if !sheet_quoted => quoted = !quoted,
            '\'' if !quoted => sheet_quoted = !sheet_quoted,
            '(' if !quoted && !sheet_quoted => depth += 1,
            ')' if !quoted && !sheet_quoted => {
                depth -= 1;
                if depth < 0 {
                    return Err("公式的括号对不上".into());
                }
            }
            _ => {}
        }
    }
    if depth != 0 || quoted || sheet_quoted {
        return Err("公式的括号或引号对不上".into());
    }
    if text.trim().is_empty() || text.trim_end().ends_with(['+', '-', '*', '/', '^', '&', '=', '<', '>', ',']) {
        return Err("公式没写完".into());
    }
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut in_str = false;
    while i < chars.len() {
        if chars[i] == '"' {
            in_str = !in_str;
        }
        if !in_str && chars[i].is_ascii_alphabetic() && (i == 0 || !word_char(chars[i - 1])) {
            let s = i;
            while i < chars.len() && word_char(chars[i]) {
                i += 1;
            }
            let word: String = chars[s..i].iter().collect();
            if chars.get(i) == Some(&'(') {
                let upper = word.to_uppercase();
                if SPILLING.contains(&upper.as_str()) {
                    return Err(format!("{upper} 这种会占好几格的函数写不了，换成普通公式"));
                }
                if NEWER.contains(&upper.as_str()) {
                    out.push_str("_xlfn.");
                }
            }
            out.push_str(&word);
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    Ok(out)
}

fn parse_sheet(name: String, part_name: String, hidden: bool, xml: &str, rels: Option<&str>) -> Result<Sheet, String> {
    let mut r = Reader::from_str(xml);
    let mut rows: Vec<Row> = vec![];
    let mut cols = vec![];
    let (head_end, pfx, self_closed) = loop {
        match r.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == "sheetData" => break (r.buffer_position() as usize, e.name().as_ref().trim_end_matches("sheetData").to_string(), false),
            Ok(Event::Empty(e)) if e.local_name().as_ref() == "sheetData" => break (r.buffer_position() as usize, e.name().as_ref().trim_end_matches("sheetData").to_string(), true),
            Ok(Event::Empty(e)) if e.local_name().as_ref() == "col" => {
                let num = |k: &str| attr(&e, k).and_then(|v| v.parse::<u32>().ok());
                if let (Some(a), Some(b), Some(s)) = (num("min"), num("max"), num("style")) {
                    cols.push((a, b, s));
                }
            }
            Ok(Event::Eof) => return Err(bad()),
            Err(_) => return Err(bad()),
            _ => {}
        }
    };
    let mut tail_start = head_end;
    if !self_closed {
        loop {
            let at = r.buffer_position() as usize;
            match r.read_event().map_err(|_| bad())? {
                Event::Start(e) if e.local_name().as_ref() == "row" => {
                    let row = row_of(&e, rows.last().map_or(0, |x: &Row| x.r))?;
                    rows.push(row);
                    let n = rows.len() - 1;
                    read_cells(&mut r, xml, &mut rows[n])?;
                }
                Event::Empty(e) if e.local_name().as_ref() == "row" => {
                    let row = row_of(&e, rows.last().map_or(0, |x: &Row| x.r))?;
                    rows.push(row);
                }
                Event::End(e) if e.local_name().as_ref() == "sheetData" => {
                    tail_start = at;
                    break;
                }
                Event::Eof => return Err(bad()),
                _ => {}
            }
        }
    }
    let close = format!("</{pfx}sheetData>");
    let (head, tail) = if self_closed {
        let before = xml[..head_end].trim_end_matches("/>").trim_end();
        (format!("{before}>"), format!("{close}{}", &xml[head_end..]))
    } else {
        (xml[..head_end].to_string(), xml[tail_start..].to_string())
    };
    let mut merged = vec![];
    let mut protected = false;
    let mut fixed = vec![];
    let mut t = tail_reader(&tail);
    loop {
        match t.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) => match e.local_name().as_ref() {
                "mergeCell" => merged.extend(attr(&e, "ref").and_then(|v| parse_range(&v))),
                "sheetProtection" => protected = matches!(attr(&e, "sheet").as_deref(), Some("1" | "true")),
                "drawing" => fixed.push("图表或图片"),
                "legacyDrawing" => fixed.push("批注"),
                "tableParts" => fixed.push("表格对象"),
                "extLst" => fixed.push("扩展的条件格式"),
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    if rels.is_some_and(|x| x.contains("relationships/pivotTable")) {
        fixed.push("数据透视表");
    }
    if rels.is_some_and(|x| x.contains("relationships/comments")) && !fixed.contains(&"批注") {
        fixed.push("批注");
    }
    Ok(Sheet { name, part: part_name, hidden, protected, pfx, head, rows, tail, merged, fixed, cols, changed: false })
}

fn row_of(e: &BytesStart, last: u32) -> Result<Row, String> {
    let r = attr(e, "r").and_then(|v| v.parse().ok()).unwrap_or(last + 1);
    Ok(Row { r, attrs: raw_attrs(e, &["r", "spans"]), cells: vec![] })
}

fn read_cells(r: &mut Reader<&[u8]>, xml: &str, row: &mut Row) -> Result<(), String> {
    loop {
        let at = r.buffer_position() as usize;
        match r.read_event().map_err(|_| bad())? {
            Event::Start(e) if e.local_name().as_ref() == "c" => {
                let mut c = cell_of(&e, row)?;
                read_children(r, xml, &mut c)?;
                c.raw = Some(xml[at..r.buffer_position() as usize].to_string());
                row.cells.push(c);
            }
            Event::Empty(e) if e.local_name().as_ref() == "c" => {
                let mut c = cell_of(&e, row)?;
                c.raw = Some(xml[at..r.buffer_position() as usize].to_string());
                row.cells.push(c);
            }
            Event::End(e) if e.local_name().as_ref() == "row" => return Ok(()),
            Event::Eof => return Err(bad()),
            _ => {}
        }
    }
}

fn cell_of(e: &BytesStart, row: &Row) -> Result<Cell, String> {
    let col = attr(e, "r").and_then(|v| parse_addr(&v)).map(|(c, _)| c).unwrap_or_else(|| row.cells.last().map_or(1, |c| c.col + 1));
    Ok(Cell { col, s: attr(e, "s").and_then(|v| v.parse().ok()), t: attr(e, "t"), other: raw_attrs(e, &["r", "s", "t"]), f: None, v: None, is_raw: None, rest: String::new(), raw: None })
}

fn read_children(r: &mut Reader<&[u8]>, xml: &str, c: &mut Cell) -> Result<(), String> {
    loop {
        let at = r.buffer_position() as usize;
        match r.read_event().map_err(|_| bad())? {
            Event::Start(e) => {
                let end = e.to_end().into_owned();
                let name = e.local_name().as_ref().to_string();
                let f = (name == "f").then(|| formula_of(&e));
                let span = r.read_to_end(end.name()).map_err(|_| bad())?;
                let inner = &xml[span.start as usize..span.end as usize];
                match name.as_str() {
                    "f" => c.f = f.map(|mut x| {
                        x.text = unesc(inner);
                        x
                    }),
                    "v" => c.v = Some(inner.to_string()),
                    "is" => c.is_raw = Some(inner.to_string()),
                    _ => c.rest.push_str(&xml[at..r.buffer_position() as usize]),
                }
            }
            Event::Empty(e) => match e.local_name().as_ref() {
                "f" => c.f = Some(formula_of(&e)),
                "v" => c.v = Some(String::new()),
                _ => c.rest.push_str(&xml[at..r.buffer_position() as usize]),
            },
            Event::End(e) if e.local_name().as_ref() == "c" => return Ok(()),
            Event::Eof => return Err(bad()),
            _ => {}
        }
    }
}

fn formula_of(e: &BytesStart) -> Formula {
    let kind = match attr(e, "t").as_deref() {
        Some("shared") => FKind::Shared,
        Some("array") => FKind::Array,
        Some("dataTable") => FKind::Table,
        _ => FKind::Normal,
    };
    Formula { kind, text: String::new(), si: attr(e, "si").and_then(|v| v.parse().ok()), reference: attr(e, "ref"), attrs: raw_attrs(e, &["t", "si", "ref"]) }
}

// ---------- a proposal ----------

/// One change, in the file's numbering as read (kept with the card).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    Cell { sheet: String, cell: String, value: String },
    AddRows { sheet: String, after: u32, rows: Vec<Vec<String>> },
    DeleteRow { sheet: String, row: u32 },
}

/// A checked change as the card shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    /// Same key: the same place (a newer change replaces it); also the card's order.
    pub key: i64,
    pub label: String,
    pub before: String,
    pub after: String,
    pub op: Op,
}

#[derive(Debug, Clone)]
pub struct Book {
    sheets: Vec<Sheet>,
    strings: Vec<String>,
    styles: Styles,
    has_styles: bool,
    date1904: bool,
}

impl Book {
    pub fn open(data: &[u8]) -> Result<Book, String> {
        let workbook = part(data, WORKBOOK)?.ok_or_else(bad)?;
        let rels = part(data, WORKBOOK_RELS)?.ok_or_else(bad)?;
        let mut targets: HashMap<String, String> = HashMap::new();
        let mut r = Reader::from_str(rels.as_str());
        loop {
            match r.read_event() {
                Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == "Relationship" => {
                    if let (Some(id), Some(target)) = (attr(&e, "Id"), attr(&e, "Target")) {
                        targets.insert(id, target.strip_prefix('/').map_or_else(|| format!("xl/{target}"), String::from));
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        let (mut refs, mut date1904) = (vec![], false);
        let mut r = Reader::from_str(workbook.as_str());
        loop {
            match r.read_event() {
                Ok(Event::Empty(e) | Event::Start(e)) => match e.local_name().as_ref() {
                    "workbookPr" => date1904 = matches!(attr(&e, "date1904").as_deref(), Some("1" | "true")),
                    "sheet" => {
                        // r:id: `attr` goes by the local name
                        if let (Some(name), Some(target)) = (attr(&e, "name"), attr(&e, "id").and_then(|id| targets.get(&id).cloned())) {
                            refs.push((name, target, attr(&e, "state").is_some_and(|s| s != "visible")));
                        }
                    }
                    _ => {}
                },
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        if refs.is_empty() {
            return Err(bad());
        }
        let mut sheets = vec![];
        for (name, target, hidden) in refs {
            let Some(xml) = part(data, &target)? else { continue }; // a chart sheet and the like
            let (dir, file) = target.rsplit_once('/').unwrap_or(("", &target));
            let rels = part(data, &format!("{dir}/_rels/{file}.rels"))?;
            sheets.push(parse_sheet(name, target.clone(), hidden, &xml, rels.as_deref())?);
        }
        let strings = match part(data, STRINGS)? {
            Some(xml) => {
                let mut out = vec![];
                let mut r = Reader::from_str(xml.as_str());
                loop {
                    match r.read_event() {
                        Ok(Event::Start(e)) if e.local_name().as_ref() == "si" => {
                            let end = e.to_end().into_owned();
                            let span = r.read_to_end(end.name()).map_err(|_| bad())?;
                            out.push(t_text(&xml[span.start as usize..span.end as usize]));
                        }
                        Ok(Event::Empty(e)) if e.local_name().as_ref() == "si" => out.push(String::new()),
                        Ok(Event::Eof) | Err(_) => break,
                        _ => {}
                    }
                }
                out
            }
            None => vec![],
        };
        let styles_xml = part(data, STYLES)?;
        let styles = styles_xml.as_deref().map(Styles::parse).unwrap_or_default();
        Ok(Book { sheets, strings, styles, has_styles: styles_xml.is_some(), date1904 })
    }

    fn sheet_index(&self, want: &str) -> Result<usize, String> {
        let want = want.trim();
        if want.is_empty() {
            return if self.sheets.len() == 1 { Ok(0) } else { Err(format!("sheet 写表名（有：{}）", self.sheet_names())) };
        }
        self.sheets.iter().position(|s| s.name == want).or_else(|| self.sheets.iter().position(|s| s.name.to_lowercase() == want.to_lowercase())).ok_or_else(|| format!("没有叫「{want}」的表（有：{}）", self.sheet_names()))
    }

    pub fn sheet_count(&self) -> usize {
        self.sheets.len()
    }

    pub fn first_sheet(&self) -> &str {
        self.sheets.first().map_or("", |s| s.name.as_str())
    }

    fn sheet_names(&self) -> String {
        self.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("、")
    }

    /// A shared formula's text at this cell.
    fn formula_text(&self, s: &Sheet, row: u32, c: &Cell) -> Option<String> {
        let f = c.f.as_ref()?;
        if f.kind == FKind::Shared && f.text.is_empty() {
            let si = f.si?;
            for r in &s.rows {
                for m in &r.cells {
                    if let Some(mf) = &m.f {
                        if mf.kind == FKind::Shared && mf.si == Some(si) && !mf.text.is_empty() {
                            return Some(translate(&mf.text, c.col as i64 - m.col as i64, row as i64 - r.r as i64));
                        }
                    }
                }
            }
            return None;
        }
        Some(f.text.clone())
    }

    /// The stored value as the model and the card read it.
    fn value_text(&self, c: &Cell) -> String {
        let v = c.v.as_deref().unwrap_or("");
        match c.t.as_deref() {
            Some("s") => v.trim().parse::<usize>().ok().and_then(|i| self.strings.get(i)).cloned().unwrap_or_default(),
            Some("inlineStr") => c.is_raw.as_deref().map(t_text).unwrap_or_default(),
            Some("str") | Some("e") => unesc(v),
            Some("b") => if v.trim() == "1" { "TRUE".into() } else { "FALSE".into() },
            _ if v.is_empty() => String::new(),
            _ => match v.parse::<f64>() {
                Ok(x) if self.styles.is_date(c.s) => serial_text(x, self.date1904),
                Ok(x) if self.styles.is_percent(c.s) => format!("{}%", ((x * 1e8).round() / 1e6)),
                _ => number_text(v),
            },
        }
    }

    fn cell_text(&self, s: &Sheet, row: u32, c: &Cell) -> String {
        match self.formula_text(s, row, c) {
            Some(f) => {
                let v = self.value_text(c);
                let f = no_prefix(&f);
                if v.is_empty() { format!("={f}") } else { format!("={f}（结果 {v}）") }
            }
            None => self.value_text(c),
        }
    }

    fn row_text(&self, s: &Sheet, row: &Row) -> Vec<(u32, String)> {
        row.cells.iter().map(|c| (c.col, self.cell_text(s, row.r, c))).filter(|(_, t)| !t.is_empty()).collect()
    }

    /// The sheets in one line each.
    pub fn outline(&self) -> String {
        self.sheets
            .iter()
            .map(|s| {
                let rows = s.rows.iter().filter(|r| !self.row_text(s, r).is_empty()).count();
                let last = s.rows.iter().flat_map(|r| r.cells.iter().map(|c| c.col)).max().unwrap_or(1);
                let mut notes = vec![];
                if s.hidden {
                    notes.push("隐藏的表".to_string());
                }
                if s.protected {
                    notes.push("设了保护，不能改".to_string());
                }
                if !s.fixed.is_empty() {
                    notes.push(format!("有{}，不能加行删行", s.fixed.join("、")));
                }
                format!("- 「{}」{rows} 行，到第 {} 行、{} 列{}", s.name, s.rows.last().map_or(0, |r| r.r), col_name(last), if notes.is_empty() { String::new() } else { format!("（{}）", notes.join("，")) })
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Rows `from`–`to` of a sheet as lines (those with `keyword` only, when given), while they fit `room`:
    /// the text and the last row given (None: all of them fit).
    pub fn read_rows(&self, sheet: &str, from: u32, to: u32, keyword: &str, room: i64) -> Result<(String, String, Option<u32>), String> {
        let s = &self.sheets[self.sheet_index(sheet)?];
        let word = keyword.trim().to_lowercase();
        let mut lines = vec![];
        let mut used = 0;
        let mut stopped = None;
        for r in s.rows.iter().filter(|r| r.r >= from && r.r <= to) {
            let cells = self.row_text(s, r);
            if cells.is_empty() || (!word.is_empty() && !cells.iter().any(|(_, t)| t.to_lowercase().contains(&word))) {
                continue;
            }
            let hidden = r.attrs.contains("hidden=\"1\"");
            let line = format!("{}{}：{}", r.r, if hidden { "（隐藏的行）" } else { "" }, cells.iter().map(|(c, t)| format!("{}={}", col_name(*c), t.replace('\n', " "))).collect::<Vec<_>>().join(" | "));
            let cost = super::context::estimate(&line) + 1;
            if used + cost > room && !lines.is_empty() {
                stopped = Some(r.r);
                break;
            }
            used += cost;
            lines.push(line);
        }
        Ok((s.name.clone(), lines.join("\n"), stopped))
    }

    /// What's merged in a sheet, for the model.
    pub fn merged_text(&self, sheet: &str) -> Result<String, String> {
        let s = &self.sheets[self.sheet_index(sheet)?];
        Ok(s.merged.iter().map(|(a, b, c, d)| format!("{}{b}:{}{d}", col_name(*a), col_name(*c))).collect::<Vec<_>>().join("、"))
    }

    pub fn plan_cell(&self, sheet: &str, cell: &str, value: &str) -> Result<Planned, String> {
        let i = self.sheet_index(sheet)?;
        let s = &self.sheets[i];
        if s.protected {
            return Err(format!("表「{}」设了保护，不能改", s.name));
        }
        let (col, row) = parse_addr(cell).ok_or_else(|| format!("「{cell}」不是格子的位置（写 B3 这样的）"))?;
        let addr = format!("{}{row}", col_name(col));
        if let Some((a, b, _, _)) = s.merged.iter().find(|(a, b, c, d)| col >= *a && col <= *c && row >= *b && row <= *d && (col, row) != (*a, *b)) {
            return Err(format!("{addr} 合并在别的格子里：改左上角的 {}{b}", col_name(*a)));
        }
        let old = s.rows.iter().find(|r| r.r == row).and_then(|r| r.cells.iter().find(|c| c.col == col));
        if let Some(f) = old.and_then(|c| c.f.as_ref()) {
            if f.kind == FKind::Array || f.kind == FKind::Table {
                return Err(format!("{addr} 是数组公式，不能改"));
            }
        }
        // inside another cell's array formula
        for r in &s.rows {
            for c in &r.cells {
                if let Some((a, b, e, d)) = c.f.as_ref().filter(|f| f.kind == FKind::Array || f.kind == FKind::Table).and_then(|f| f.reference.as_deref()).and_then(parse_range) {
                    if col >= a && col <= e && row >= b && row <= d {
                        return Err(format!("{addr} 在数组公式 {}{} 的范围里，不能改", col_name(c.col), r.r));
                    }
                }
            }
        }
        if let Some(f) = value.trim().strip_prefix('=') {
            formula_for_file(f)?;
            if names_itself(f, &s.name, col, row) {
                return Err(format!("公式用到了 {addr} 自己（循环引用）：行号照现在读到的写；加在合计这类范围最后一行后面的行，写入时范围会自己扩进来，不用改公式"));
            }
        }
        let before = old.map(|c| self.cell_text(s, row, c)).unwrap_or_default();
        let after = value.trim_end().to_string();
        let label = format!("表「{}」{addr}", s.name);
        Ok(Planned { key: key(i, row, col), label, before, after: after.clone(), op: Op::Cell { sheet: s.name.clone(), cell: addr, value: after } })
    }

    fn check_row_ops(&self, i: usize, m: Move) -> Result<(), String> {
        let s = &self.sheets[i];
        if s.protected {
            return Err(format!("表「{}」设了保护，不能改", s.name));
        }
        if !s.fixed.is_empty() {
            return Err(format!("表「{}」有{}，加行删行会错位：只能改格子", s.name, s.fixed.join("、")));
        }
        // every formula that names it must still make sense
        for other in &self.sheets {
            for r in &other.rows {
                for c in &r.cells {
                    if let Some(text) = c.f.as_ref().map(|f| f.text.as_str()).filter(|t| !t.is_empty()) {
                        moved_formula(text, &other.name, &s.name, m).map_err(|e| format!("表「{}」{}{}{e}，加行删行算不对", other.name, col_name(c.col), r.r))?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn plan_add(&self, sheet: &str, after: u32, rows: &[Vec<String>]) -> Result<Planned, String> {
        let i = self.sheet_index(sheet)?;
        let s = &self.sheets[i];
        if rows.is_empty() || rows.iter().all(|r| r.iter().all(|v| v.trim().is_empty())) {
            return Err("要加的行是空的".into());
        }
        if after > MAX_ROW - rows.len() as u32 {
            return Err("超出了 Excel 的行数".into());
        }
        self.check_row_ops(i, Move::Insert { after, count: rows.len() as u32 })?;
        if let Some((a, b, c, d)) = s.merged.iter().find(|(_, b, _, d)| after >= *b && after < *d) {
            return Err(format!("第 {after} 行在合并的 {}{b}:{}{d} 中间，不能在这里加行", col_name(*a), col_name(*c)));
        }
        for v in rows.iter().flatten() {
            if let Some(f) = v.trim().strip_prefix('=') {
                formula_for_file(f)?;
            }
        }
        let text = rows.iter().map(|r| r.iter().map(|v| v.trim()).collect::<Vec<_>>().join(" | ")).collect::<Vec<_>>().join("\n");
        let label = format!("表「{}」第 {after} 行后加 {} 行", s.name, rows.len());
        Ok(Planned { key: key(i, after, MAX_COL + 1), label, before: String::new(), after: text, op: Op::AddRows { sheet: s.name.clone(), after, rows: rows.to_vec() } })
    }

    /// The formulas on `sheet` whose range rows added after `row` join (`grows`: =SUM(B3:B5) in B6 for
    /// row 5). Each as 「B6（=SUM(B3:B5)）」.
    pub fn ranges_ending_at(&self, sheet: &str, row: u32) -> Vec<String> {
        let Ok(i) = self.sheet_index(sheet) else { return vec![] };
        let s = &self.sheets[i];
        let mut out = vec![];
        for r in &s.rows {
            for c in &r.cells {
                let Some(f) = self.formula_text(s, r.r, c) else { continue };
                if grows(&f, &s.name, r.r, row) {
                    out.push(format!("{}{}（={}）", col_name(c.col), r.r, no_prefix(&f)));
                }
            }
        }
        out
    }

    pub fn plan_delete(&self, sheet: &str, row: u32) -> Result<Planned, String> {
        let i = self.sheet_index(sheet)?;
        let s = &self.sheets[i];
        let found = s.rows.iter().find(|r| r.r == row).ok_or_else(|| format!("表「{}」第 {row} 行是空的", s.name))?;
        self.check_row_ops(i, Move::Delete { row })?;
        if let Some((a, b, c, d)) = s.merged.iter().find(|(_, b, _, d)| row >= *b && row <= *d) {
            return Err(format!("第 {row} 行有合并的格子（{}{b}:{}{d}），不能删", col_name(*a), col_name(*c)));
        }
        if vanishes(&s.tail, Move::Delete { row }) {
            return Err(format!("第 {row} 行有条件格式、数据验证或超链接，不能删"));
        }
        let before = self.row_text(s, found).into_iter().map(|(c, t)| format!("{}={t}", col_name(c))).collect::<Vec<_>>().join(" | ");
        Ok(Planned { key: key(i, row, 0), label: format!("表「{}」删第 {row} 行", s.name), before, after: String::new(), op: Op::DeleteRow { sheet: s.name.clone(), row } })
    }
}

fn key(sheet: usize, row: u32, col: u32) -> i64 {
    (sheet as i64 + 1) * 10_000_000_000_000 + row as i64 * 100_000 + col as i64
}

// ---------- writing ----------

/// What a new value becomes.
#[derive(Debug, Clone, PartialEq)]
enum Typed {
    Empty,
    Num(f64),
    Text(String),
    Bool(bool),
    Formula(String),
    Date(f64),
    Percent(f64),
}

/// `had_text`: what the cell held was text (numeric-looking text stays text); `had_date`: it's date-formatted.
fn typed(value: &str, old: Option<&str>, had_text: bool, had_date: bool, date1904: bool) -> Result<Typed, String> {
    let v = value.trim_end();
    if v.trim().is_empty() {
        return Ok(Typed::Empty);
    }
    if let Some(text) = v.strip_prefix('\'') {
        return Ok(Typed::Text(text.to_string()));
    }
    if let Some(f) = v.strip_prefix('=') {
        return Ok(Typed::Formula(formula_for_file(f.trim())?));
    }
    let code_like = had_text && old.is_some_and(|o| o.parse::<f64>().is_ok() || o.chars().all(|c| c.is_ascii_digit() || c == '-'));
    if v == "TRUE" || v == "FALSE" {
        return Ok(if had_text { Typed::Text(v.into()) } else { Typed::Bool(v == "TRUE") });
    }
    if let Some(x) = v.strip_suffix('%').and_then(plain_number) {
        return Ok(if code_like { Typed::Text(v.into()) } else { Typed::Percent(x / 100.0) });
    }
    if let Some(x) = plain_number(v) {
        return Ok(if code_like { Typed::Text(v.into()) } else { Typed::Num(x) });
    }
    if let Some(x) = parse_date(v, date1904) {
        return Ok(if had_text && !had_date { Typed::Text(v.into()) } else { Typed::Date(x) });
    }
    Ok(Typed::Text(v.to_string()))
}

fn num_text(x: f64) -> String {
    x.to_string()
}

/// Fills `cell` with `value` (its style kept; a date or percent in a cell without that format gets one).
fn fill(cell: &mut Cell, styles: &mut Styles, has_styles: bool, value: Typed, pfx: &str) {
    cell.raw = None;
    cell.other.clear();
    cell.rest.clear();
    cell.f = None;
    cell.v = None;
    cell.is_raw = None;
    cell.t = None;
    match value {
        Typed::Empty => {}
        Typed::Num(x) => cell.v = Some(num_text(x)),
        Typed::Bool(b) => {
            cell.t = Some("b".into());
            cell.v = Some(if b { "1".into() } else { "0".into() });
        }
        Typed::Text(t) => {
            cell.t = Some("inlineStr".into());
            let space = if t != t.trim() || t.contains('\n') { " xml:space=\"preserve\"" } else { "" };
            cell.is_raw = Some(format!("<{pfx}t{space}>{}</{pfx}t>", esc(&t)));
        }
        Typed::Formula(f) => cell.f = Some(Formula { kind: FKind::Normal, text: f, si: None, reference: None, attrs: String::new() }),
        Typed::Date(x) => {
            if has_styles && !styles.is_date(cell.s) {
                cell.s = styles.with_fmt(cell.s, FMT_DATE).or(cell.s);
            }
            cell.v = Some(num_text(x));
        }
        Typed::Percent(x) => {
            if has_styles && !styles.is_percent(cell.s) {
                cell.s = styles.with_fmt(cell.s, FMT_PERCENT).or(cell.s);
            }
            cell.v = Some(num_text(x));
        }
    }
}

impl Sheet {
    /// The style a new cell at (col, row) starts with: its row's, else its column's.
    fn new_cell_style(&self, row: &Row, col: u32) -> Option<u32> {
        if row.attrs.contains("customFormat=\"1\"") {
            if let Some(s) = row.attrs.split(" s=\"").nth(1).and_then(|x| x.split('"').next()).and_then(|v| v.parse().ok()) {
                return Some(s);
            }
        }
        self.cols.iter().find(|(a, b, _)| col >= *a && col <= *b).map(|x| x.2)
    }

    fn expand_shared(&mut self, only: Option<u32>) {
        let mut masters: HashMap<u32, (u32, u32, String)> = HashMap::new();
        for r in &self.rows {
            for c in &r.cells {
                if let Some(f) = &c.f {
                    if f.kind == FKind::Shared && !f.text.is_empty() {
                        if let Some(si) = f.si {
                            masters.insert(si, (c.col, r.r, f.text.clone()));
                        }
                    }
                }
            }
        }
        for r in &mut self.rows {
            let row = r.r;
            for c in &mut r.cells {
                let Some(f) = c.f.as_mut() else { continue };
                if f.kind != FKind::Shared || only.is_some_and(|o| f.si != Some(o)) {
                    continue;
                }
                if let Some((mc, mr, text)) = f.si.and_then(|si| masters.get(&si)) {
                    f.text = translate(text, c.col as i64 - *mc as i64, row as i64 - *mr as i64);
                    f.kind = FKind::Normal;
                    f.si = None;
                    f.reference = None;
                    c.raw = None;
                }
            }
        }
    }

    fn set(&mut self, styles: &mut Styles, has_styles: bool, date1904: bool, strings: &[String], col: u32, row: u32, value: &str) -> Result<(), String> {
        // a shared formula's master going away: its children get their own
        let master_si = self.rows.iter().find(|r| r.r == row).and_then(|r| r.cells.iter().find(|c| c.col == col)).and_then(|c| c.f.as_ref()).filter(|f| f.kind == FKind::Shared && !f.text.is_empty()).and_then(|f| f.si);
        if let Some(si) = master_si {
            self.expand_shared(Some(si));
        }
        let at = match self.rows.binary_search_by_key(&row, |r| r.r) {
            Ok(i) => i,
            Err(i) => {
                self.rows.insert(i, Row { r: row, ..Row::default() });
                i
            }
        };
        let style = self.new_cell_style(&self.rows[at].clone(), col);
        let pfx = self.pfx.clone();
        let r = &mut self.rows[at];
        let ci = match r.cells.binary_search_by_key(&col, |c| c.col) {
            Ok(i) => i,
            Err(i) => {
                r.cells.insert(i, Cell { col, s: style, t: None, other: String::new(), f: None, v: None, is_raw: None, rest: String::new(), raw: None });
                i
            }
        };
        let c = &mut r.cells[ci];
        let was_text = matches!(c.t.as_deref(), Some("s" | "inlineStr" | "str"));
        let old = match c.t.as_deref() {
            Some("s") => c.v.as_deref().and_then(|v| v.trim().parse::<usize>().ok()).and_then(|i| strings.get(i)).cloned(),
            Some("inlineStr") => c.is_raw.as_deref().map(t_text),
            _ => None,
        };
        let v = typed(value, old.as_deref(), was_text, styles.is_date(c.s), date1904)?;
        fill(c, styles, has_styles, v, &pfx);
        self.changed = true;
        Ok(())
    }

    /// New rows after `after`, their look copied from it.
    fn insert_rows(&mut self, styles: &mut Styles, has_styles: bool, date1904: bool, after: u32, rows: &[Vec<String>]) -> Result<(), String> {
        let count = rows.len() as u32;
        let model = self.rows.iter().find(|r| r.r == after).cloned();
        self.move_rows(Move::Insert { after, count })?;
        let at = self.rows.partition_point(|r| r.r <= after);
        for (k, values) in rows.iter().enumerate() {
            let number = after + k as u32 + 1;
            let mut row = Row { r: number, attrs: model.as_ref().map(|m| m.attrs.clone()).unwrap_or_default(), cells: vec![] };
            let width = values.len() as u32;
            for col in 1..=width.max(model.as_ref().and_then(|m| m.cells.last().map(|c| c.col)).unwrap_or(0)) {
                let value = values.get(col as usize - 1).map_or("", |v| v.as_str());
                let s = model.as_ref().and_then(|m| m.cells.iter().find(|c| c.col == col)).and_then(|c| c.s);
                if s.is_none() && value.trim().is_empty() {
                    continue;
                }
                let mut c = Cell { col, s, t: None, other: String::new(), f: None, v: None, is_raw: None, rest: String::new(), raw: None };
                let v = typed(value, None, false, styles.is_date(s), date1904)?;
                fill(&mut c, styles, has_styles, v, &self.pfx.clone());
                row.cells.push(c);
            }
            self.rows.insert(at + k, row);
        }
        self.changed = true;
        Ok(())
    }

    /// Renumbers this sheet's rows and what points at them (formulas elsewhere: `move_formulas`).
    fn move_rows(&mut self, m: Move) -> Result<(), String> {
        self.expand_shared(None);
        if let Move::Delete { row } = m {
            self.rows.retain(|r| r.r != row);
        }
        for r in &mut self.rows {
            if let Some(n) = m.one(r.r) {
                if n != r.r {
                    r.r = n;
                    for c in &mut r.cells {
                        c.raw = None;
                    }
                }
            }
            for c in &mut r.cells {
                if let Some(f) = c.f.as_mut().filter(|f| f.kind == FKind::Array || f.kind == FKind::Table) {
                    if let Some(reference) = &f.reference {
                        if let Some(moved) = moved_sqref(reference, m) {
                            f.reference = Some(moved);
                            c.raw = None;
                        }
                    }
                }
            }
        }
        self.merged = self.merged.iter().filter_map(|(a, b, c, d)| m.span(*b, *d).map(|(x, y)| (*a, x, *c, y))).collect();
        self.tail = moved_tail(&self.tail, &self.name, m)?;
        self.changed = true;
        Ok(())
    }

    fn formulas_moved(&mut self, sheet: &str, m: Move) -> Result<(), String> {
        let needs_expanding = self.name == sheet || self.rows.iter().any(|r| r.cells.iter().any(|c| c.f.as_ref().is_some_and(|f| f.kind == FKind::Shared && f.text.contains(sheet))));
        if needs_expanding {
            self.expand_shared(None);
        }
        let here = self.name.clone();
        let insert = match m {
            Move::Insert { after, count } if here == sheet => Some((after, count)),
            _ => None,
        };
        for r in &mut self.rows {
            for c in &mut r.cells {
                let Some(f) = c.f.as_mut() else { continue };
                if f.text.is_empty() {
                    continue;
                }
                let mut new = moved_formula(&f.text, &here, sheet, m)?;
                if let Some((after, count)) = insert.filter(|(after, _)| grows(&f.text, &here, r.r, *after)) {
                    new = grown_formula(&new, &here, r.r, after, count);
                }
                if new != f.text {
                    f.text = new;
                    c.raw = None;
                    self.changed = true;
                }
            }
        }
        Ok(())
    }

    fn xml(&self) -> String {
        let p = &self.pfx;
        let mut out = String::with_capacity(self.head.len() + self.tail.len() + self.rows.len() * 64);
        let (mut first, mut last_col, mut last_row, mut first_row) = (MAX_COL, 0, 0, 0);
        let mut body = String::new();
        for r in &self.rows {
            if !r.cells.is_empty() {
                first_row = if first_row == 0 { r.r } else { first_row };
                last_row = r.r;
                first = first.min(r.cells[0].col);
                last_col = last_col.max(r.cells[r.cells.len() - 1].col);
            }
            body.push_str(&format!("<{p}row r=\"{}\"{}", r.r, r.attrs));
            if r.cells.is_empty() {
                body.push_str("/>");
                continue;
            }
            body.push('>');
            for c in &r.cells {
                match &c.raw {
                    Some(raw) => body.push_str(raw),
                    None => body.push_str(&cell_xml(p, r.r, c)),
                }
            }
            body.push_str(&format!("</{p}row>"));
        }
        // the used range
        let head = if first_row > 0 { with_dimension(&self.head, &format!("{}{first_row}:{}{last_row}", col_name(first), col_name(last_col))) } else { self.head.clone() };
        out.push_str(&head);
        out.push_str(&body);
        out.push_str(&self.tail);
        out
    }
}

fn cell_xml(p: &str, row: u32, c: &Cell) -> String {
    let mut out = format!("<{p}c r=\"{}{row}\"", col_name(c.col));
    if let Some(s) = c.s {
        out.push_str(&format!(" s=\"{s}\""));
    }
    if let Some(t) = &c.t {
        out.push_str(&format!(" t=\"{t}\""));
    }
    out.push_str(&c.other);
    let mut inner = String::new();
    if let Some(f) = &c.f {
        inner.push_str(&format!("<{p}f"));
        match f.kind {
            FKind::Shared => inner.push_str(" t=\"shared\""),
            FKind::Array => inner.push_str(" t=\"array\""),
            FKind::Table => inner.push_str(" t=\"dataTable\""),
            FKind::Normal => {}
        }
        if let Some(reference) = &f.reference {
            inner.push_str(&format!(" ref=\"{}\"", esc_attr(reference)));
        }
        if let Some(si) = f.si {
            inner.push_str(&format!(" si=\"{si}\""));
        }
        inner.push_str(&f.attrs);
        if f.text.is_empty() {
            inner.push_str("/>");
        } else {
            inner.push_str(&format!(">{}</{p}f>", esc(&f.text)));
        }
    }
    if let Some(v) = &c.v {
        inner.push_str(&format!("<{p}v>{v}</{p}v>"));
    }
    if let Some(is) = &c.is_raw {
        inner.push_str(&format!("<{p}is>{is}</{p}is>"));
    }
    inner.push_str(&c.rest);
    if inner.is_empty() {
        out.push_str("/>");
    } else {
        out.push_str(&format!(">{inner}</{p}c>"));
    }
    out
}

fn with_dimension(head: &str, range: &str) -> String {
    let Some(at) = head.find("dimension ") else { return head.to_string() };
    let Some(open) = head[at..].find("ref=\"").map(|i| at + i + 5) else { return head.to_string() };
    let Some(close) = head[open..].find('"').map(|i| open + i) else { return head.to_string() };
    format!("{}{range}{}", &head[..open], &head[close..])
}

/// A reader for what comes after sheetData (it starts with an end tag).
fn tail_reader(tail: &str) -> Reader<&[u8]> {
    let mut r = Reader::from_str(tail);
    r.config_mut().allow_unmatched_ends = true;
    r
}

/// Elements after sheetData that point at rows: (element, its attribute).
const POINTING: &[(&str, &str)] = &[("mergeCell", "ref"), ("conditionalFormatting", "sqref"), ("dataValidation", "sqref"), ("hyperlink", "ref"), ("autoFilter", "ref"), ("sortState", "ref"), ("sortCondition", "ref")];

/// Whether moving these rows would leave one of them with nothing to point at.
fn vanishes(tail: &str, m: Move) -> bool {
    let mut r = tail_reader(tail);
    loop {
        match r.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) => {
                if let Some((_, key)) = POINTING.iter().find(|(n, _)| *n == e.local_name().as_ref()) {
                    if attr(&e, key).is_some_and(|v| moved_sqref(&v, m).is_none()) {
                        return true;
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => return false,
            _ => {}
        }
    }
}

/// What comes after sheetData with its references to the rows moved (attributes and rule formulas).
fn moved_tail(tail: &str, sheet: &str, m: Move) -> Result<String, String> {
    let mut patches: Vec<(usize, usize, String)> = vec![];
    let mut r = tail_reader(tail);
    loop {
        let at = r.buffer_position() as usize;
        match r.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) => {
                let name = e.local_name().as_ref().to_string();
                if let Some((_, key)) = POINTING.iter().find(|(n, _)| *n == name.as_str()) {
                    if let Some(old) = attr(&e, key) {
                        if let Some(new) = moved_sqref(&old, m) {
                            let end = r.buffer_position() as usize;
                            let raw = &tail[at..end];
                            let needle = format!("{key}=\"");
                            if let Some(p) = raw.find(&needle) {
                                let s = at + p + needle.len();
                                if let Some(q) = tail[s..].find('"') {
                                    patches.push((s, s + q, esc_attr(&new)));
                                }
                            }
                        }
                    }
                }
                if matches!(name.as_str(), "formula" | "formula1" | "formula2") {
                    // Start only: an Empty one has no text
                    if tail[at..r.buffer_position() as usize].ends_with("/>") {
                        continue;
                    }
                    let end = e.to_end().into_owned();
                    let span = r.read_to_end(end.name()).map_err(|_| bad())?;
                    let (a, b) = (span.start as usize, span.end as usize);
                    let new = moved_formula(&unesc(&tail[a..b]), sheet, sheet, m)?;
                    patches.push((a, b, esc(&new)));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    patches.sort_by_key(|p| std::cmp::Reverse(p.0));
    let mut out = tail.to_string();
    for (a, b, t) in patches {
        out.replace_range(a..b, &t);
    }
    Ok(out)
}

/// Defined names with `sheet`'s rows moved.
fn moved_names(workbook: &str, sheet: &str, m: Move) -> Result<String, String> {
    let mut patches = vec![];
    let mut r = Reader::from_str(workbook);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == "definedName" => {
                let end = e.to_end().into_owned();
                let span = r.read_to_end(end.name()).map_err(|_| bad())?;
                let (a, b) = (span.start as usize, span.end as usize);
                let new = moved_formula(&unesc(&workbook[a..b]), "", sheet, m)?;
                patches.push((a, b, esc(&new)));
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let mut out = workbook.to_string();
    for (a, b, t) in patches.into_iter().rev() {
        out.replace_range(a..b, &t);
    }
    Ok(out)
}

/// The workbook asks Excel for a full recalculation when it opens.
fn with_recalc(workbook: &str) -> String {
    let mut r = Reader::from_str(workbook);
    // where a new calcPr may go: after whatever of these comes last
    let mut after = None;
    let mut pfx = String::new();
    loop {
        let at = r.buffer_position() as usize;
        match r.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == "calcPr" => {
                let end = r.buffer_position() as usize;
                let tag = &workbook[at..end];
                if tag.contains("fullCalcOnLoad") {
                    return workbook.to_string();
                }
                let close = if tag.ends_with("/>") { end - 2 } else { end - 1 };
                return format!("{} fullCalcOnLoad=\"1\"{}", &workbook[..close], &workbook[close..]);
            }
            Ok(Event::End(e)) if matches!(e.local_name().as_ref(), "sheets" | "externalReferences" | "definedNames" | "functionGroups") => {
                after = Some(r.buffer_position() as usize);
                pfx = e.name().as_ref().rsplit_once(':').map_or(String::new(), |(p, _)| format!("{p}:"));
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    match after {
        Some(at) => format!("{}<{pfx}calcPr fullCalcOnLoad=\"1\"/>{}", &workbook[..at], &workbook[at..]),
        None => workbook.to_string(),
    }
}

/// Without the element naming calcChain.
fn without_calc_chain(xml: &str) -> String {
    let Some(at) = xml.find("calcChain") else { return xml.to_string() };
    let Some(start) = xml[..at].rfind('<') else { return xml.to_string() };
    let Some(end) = xml[at..].find('>').map(|i| at + i + 1) else { return xml.to_string() };
    format!("{}{}", &xml[..start], &xml[end..])
}

/// The ticked changes written: the file's bytes.
pub fn apply(data: &[u8], ops: &[Op]) -> Result<Vec<u8>, String> {
    let mut book = Book::open(data)?;
    let mut workbook = part(data, WORKBOOK)?.ok_or_else(bad)?;
    let (date1904, has_styles) = (book.date1904, book.has_styles);
    let strings = std::mem::take(&mut book.strings);
    // cells first, in the numbering as read
    for op in ops {
        if let Op::Cell { sheet, cell, value } = op {
            let i = book.sheet_index(sheet)?;
            let (col, row) = parse_addr(cell).ok_or_else(|| format!("「{cell}」不是格子的位置"))?;
            let styles = &mut book.styles;
            book.sheets[i].set(styles, has_styles, date1904, &strings, col, row, value)?;
        }
    }
    // then rows, from the bottom up so the numbers above stay right
    let mut moves: Vec<&Op> = ops.iter().filter(|o| !matches!(o, Op::Cell { .. })).collect();
    let place = |o: &Op| match o {
        Op::AddRows { after, .. } => (*after, 1),
        Op::DeleteRow { row, .. } => (*row, 0),
        Op::Cell { .. } => (0, 0),
    };
    moves.sort_by_key(|o| std::cmp::Reverse(place(o)));
    for op in moves {
        let (sheet, m) = match op {
            Op::AddRows { sheet, after, rows } => (sheet, Move::Insert { after: *after, count: rows.len() as u32 }),
            Op::DeleteRow { sheet, row } => (sheet, Move::Delete { row: *row }),
            Op::Cell { .. } => continue,
        };
        let i = book.sheet_index(sheet)?;
        let name = book.sheets[i].name.clone();
        if !book.sheets[i].fixed.is_empty() {
            return Err(format!("表「{name}」有{}，加行删行会错位", book.sheets[i].fixed.join("、")));
        }
        for other in book.sheets.iter_mut() {
            other.formulas_moved(&name, m)?;
        }
        match op {
            Op::AddRows { after, rows, .. } => {
                let styles = &mut book.styles;
                book.sheets[i].insert_rows(styles, has_styles, date1904, *after, rows)?;
            }
            _ => book.sheets[i].move_rows(m)?,
        }
        workbook = moved_names(&workbook, &name, m)?;
    }
    workbook = with_recalc(&workbook);
    let changed: HashMap<String, String> = book.sheets.iter().filter(|s| s.changed).map(|s| (s.part.clone(), s.xml())).collect();
    let styles_xml = if book.styles.xfs.len() > book.styles.read { part(data, STYLES)?.map(|x| with_new_xfs(&x, &book.styles)) } else { None };
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|_| bad())?;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..zip.len() {
        let name = zip.name_for_index(i).ok_or_else(bad)?.map_err(|_| bad())?.to_string();
        let new: Option<String> = match name.as_str() {
            CALC_CHAIN => continue,
            WORKBOOK => Some(workbook.clone()),
            WORKBOOK_RELS | CONTENT_TYPES => part(data, &name)?.map(|x| without_calc_chain(&x)),
            STYLES => styles_xml.clone(),
            n => changed.get(n).cloned(),
        };
        match new {
            Some(text) => {
                w.start_file(name, opts).map_err(|e| e.to_string())?;
                w.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
            }
            None => {
                let f = zip.by_index_raw(i).map_err(|_| bad())?;
                w.raw_copy_file(f).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(w.finish().map_err(|e| e.to_string())?.into_inner())
}

fn with_new_xfs(xml: &str, styles: &Styles) -> String {
    let added: String = styles.xfs[styles.read..].iter().map(|x| x.1.as_str()).collect();
    let Some(close) = xml.find("</cellXfs>").or_else(|| xml.find(":cellXfs>").and_then(|i| xml[..i].rfind("</"))) else { return xml.to_string() };
    let mut out = format!("{}{added}{}", &xml[..close], &xml[close..]);
    if let Some(at) = out.find("cellXfs ") {
        if let Some(c) = out[at..].find("count=\"").map(|i| at + i + 7) {
            if let Some(e) = out[c..].find('"').map(|i| c + i) {
                out.replace_range(c..e, &styles.xfs.len().to_string());
            }
        }
    }
    out
}

// ---------- a new workbook ----------

/// A sheet of a new workbook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewSheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

/// A sheet name Excel takes: no : \ / ? * [ ], at most 31 characters, not repeated.
pub fn clean_sheets(sheets: Vec<NewSheet>) -> Vec<NewSheet> {
    let mut seen: Vec<String> = vec![];
    sheets
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let clean: String = s.name.chars().map(|c| if ":\\/?*[]".contains(c) || c.is_control() { ' ' } else { c }).collect();
            let clean = clean.trim().trim_matches('\'').trim().chars().take(28).collect::<String>();
            let base = if clean.is_empty() { format!("Sheet{}", i + 1) } else { clean };
            let mut name = base.clone();
            let mut k = 2;
            while seen.iter().any(|x| x.to_lowercase() == name.to_lowercase()) {
                name = format!("{base} {k}");
                k += 1;
            }
            seen.push(name.clone());
            NewSheet { name, rows: s.rows }
        })
        .collect()
}

const NEW_STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="12"/><name val="PingFang SC"/></font><font><b/><sz val="12"/><name val="PingFang SC"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="4"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="10" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;

fn text_width(s: &str) -> f64 {
    s.chars().map(|c| if c.is_ascii() { 1.0 } else { 2.0 }).sum::<f64>().clamp(6.0, 60.0) + 2.0
}

/// A new workbook from rows of values (typed like a cell's new value; the first row bold).
pub fn new_xlsx(sheets: &[NewSheet]) -> Result<Vec<u8>, String> {
    if sheets.is_empty() || sheets.iter().all(|s| s.rows.is_empty()) {
        return Err("没有内容".into());
    }
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut put = |name: &str, text: &str| -> Result<(), String> {
        w.start_file(name, opts).map_err(|e| e.to_string())?;
        w.write_all(text.as_bytes()).map_err(|e| e.to_string())
    };
    let overrides: String = (1..=sheets.len()).map(|i| format!(r#"<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#)).collect();
    put(CONTENT_TYPES, &format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>{overrides}</Types>"#))?;
    put("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#)?;
    let list: String = sheets.iter().enumerate().map(|(i, s)| format!(r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#, esc_attr(&s.name), i + 1, i + 1)).collect();
    put(WORKBOOK, &format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{list}</sheets><calcPr fullCalcOnLoad="1"/></workbook>"#))?;
    let rels: String = (1..=sheets.len()).map(|i| format!(r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>"#)).collect();
    put(WORKBOOK_RELS, &format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#, sheets.len() + 1))?;
    put(STYLES, NEW_STYLES)?;
    for (i, s) in sheets.iter().enumerate() {
        let mut body = String::new();
        let mut widths: Vec<f64> = vec![];
        for (ri, row) in s.rows.iter().enumerate() {
            let n = ri as u32 + 1;
            body.push_str(&format!("<row r=\"{n}\">"));
            for (ci, value) in row.iter().enumerate() {
                let col = ci as u32 + 1;
                if widths.len() <= ci {
                    widths.resize(ci + 1, 6.0);
                }
                widths[ci] = widths[ci].max(text_width(value));
                let v = typed(value, None, false, false, false)?;
                if v == Typed::Empty {
                    continue;
                }
                let s = match (&v, ri) {
                    (_, 0) => Some(1),
                    (Typed::Date(_), _) => Some(2),
                    (Typed::Percent(_), _) => Some(3),
                    _ => None,
                };
                let mut c = Cell { col, s, t: None, other: String::new(), f: None, v: None, is_raw: None, rest: String::new(), raw: None };
                let mut none = Styles::default();
                fill(&mut c, &mut none, false, v, "");
                body.push_str(&cell_xml("", n, &c));
            }
            body.push_str("</row>");
        }
        let cols = if widths.is_empty() { String::new() } else { format!("<cols>{}</cols>", widths.iter().enumerate().map(|(k, w)| format!(r#"<col min="{0}" max="{0}" width="{1:.1}" customWidth="1"/>"#, k + 1, w)).collect::<String>()) };
        put(&format!("xl/worksheets/sheet{}.xml", i + 1), &format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{cols}<sheetData>{body}</sheetData></worksheet>"#))?;
    }
    Ok(w.finish().map_err(|e| e.to_string())?.into_inner())
}

/// A new workbook as the card shows it (the first rows of each sheet as Markdown tables).
pub fn preview(sheets: &[NewSheet]) -> String {
    const ROWS: usize = 30;
    sheets
        .iter()
        .map(|s| {
            let width = s.rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
            let line = |r: &[String]| format!("| {} |", (0..width).map(|i| r.get(i).map_or("", |v| v.as_str()).replace('|', "\\|").replace('\n', " ")).collect::<Vec<_>>().join(" | "));
            let mut out = vec![format!("**{}**", s.name), String::new()];
            if let Some(h) = s.rows.first() {
                out.push(line(h));
                out.push(format!("|{}", " --- |".repeat(width)));
            }
            out.extend(s.rows.iter().skip(1).take(ROWS).map(|r| line(r)));
            if s.rows.len() > ROWS + 1 {
                out.push(String::new());
                out.push(format!("……还有 {} 行", s.rows.len() - ROWS - 1));
            }
            out.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use calamine::{Data, Reader as _};

    fn zipped(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in parts {
            w.start_file(*name, opts).unwrap();
            w.write_all(text.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// A workbook shaped as Excel writes them: shared strings, styles (a date, a percent), a shared formula, a
    /// merged title, a total, a second sheet and a defined name pointing at the first, a calc chain.
    pub fn sample_xlsx() -> Vec<u8> {
        let sheet1 = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:E6"/><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/><cols><col min="1" max="1" width="12" customWidth="1"/></cols><sheetData><row r="1" spans="1:5" ht="20" customHeight="1"><c r="A1" s="1" t="s"><v>0</v></c><c r="B1" s="1"/></row><row r="2" spans="1:5"><c r="A2" t="s"><v>1</v></c><c r="B2" t="s"><v>2</v></c><c r="C2" t="s"><v>3</v></c><c r="D2" t="s"><v>4</v></c><c r="E2" t="s"><v>5</v></c></row><row r="3" spans="1:5"><c r="A3" t="s"><v>6</v></c><c r="B3"><v>3000</v></c><c r="C3" s="2"><v>46296</v></c><c r="D3"><f t="shared" ref="D3:D5" si="0">B3*2</f><v>6000</v></c><c r="E3" s="3"><v>0.125</v></c></row><row r="4" spans="1:5"><c r="A4" t="s"><v>7</v></c><c r="B4"><v>120.5</v></c><c r="C4" s="2"><v>46300</v></c><c r="D4"><f t="shared" si="0"/><v>241</v></c><c r="E4" t="s"><v>8</v></c></row><row r="5" spans="1:5"><c r="A5" t="s"><v>9</v></c><c r="B5"><v>80</v></c><c r="C5" s="2"><v>46301</v></c><c r="D5"><f t="shared" si="0"/><v>160</v></c></row><row r="6" spans="1:5"><c r="A6" t="s"><v>10</v></c><c r="B6"><f>SUM(B3:B5)</f><v>3200.5</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells><conditionalFormatting sqref="B3:B5"><cfRule type="expression" dxfId="0" priority="1"><formula>B3&gt;100</formula></cfRule></conditionalFormatting><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>"#;
        let sheet2 = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><f>'预算 表'!B6*2</f><v>6401</v></c><c r="B1"><f>'预算 表'!B4</f><v>120.5</v></c><c r="C1"><v>7</v></c></row></sheetData></worksheet>"#;
        let strings = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="11" uniqueCount="11"><si><t>十月预算</t></si><si><t>项目</t></si><si><t>金额</t></si><si><t>日期</t></si><si><t>两倍</t></si><si><t>比例</t></si><si><t>房租</t></si><si><r><rPr><b/></rPr><t>水</t></r><r><t>电</t></r></si><si><t>007</t></si><si><t>网费</t></si><si><t>合计</t></si></sst>"#;
        let styles = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy&quot;年&quot;m&quot;月&quot;d&quot;日&quot;"/></numFmts><fonts count="1"><font><sz val="12"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="4"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"><alignment horizontal="center"/></xf><xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="10" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs></styleSheet>"#;
        let workbook = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><bookViews><workbookView/></bookViews><sheets><sheet name="预算 表" sheetId="1" r:id="rId1"/><sheet name="引用" sheetId="2" r:id="rId2"/></sheets><definedNames><definedName name="合计格">'预算 表'!$B$6</definedName></definedNames><calcPr calcId="191029"/></workbook>"#;
        let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/></Relationships>"#;
        let types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/><Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/></Types>"#;
        let root = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
        zipped(&[(CONTENT_TYPES, types), ("_rels/.rels", root), (WORKBOOK, workbook), (WORKBOOK_RELS, rels), ("xl/worksheets/sheet1.xml", sheet1), ("xl/worksheets/sheet2.xml", sheet2), (STRINGS, strings), (STYLES, styles), (CALC_CHAIN, r#"<calcChain xmlns="x"><c r="B6" i="1"/></calcChain>"#)])
    }

    /// A sheet as calamine (another reader) sees it: values by (row, col), 1-based.
    fn sheet_of(data: &[u8], name: &str) -> calamine::Range<Data> {
        let mut wb = calamine::open_workbook_auto_from_rs(std::io::Cursor::new(data.to_vec())).unwrap();
        wb.worksheet_range(name).unwrap()
    }

    fn value(r: &calamine::Range<Data>, row: u32, col: u32) -> Data {
        r.get_value((row - 1, col - 1)).cloned().unwrap_or(Data::Empty)
    }

    fn formula_of_cell(data: &[u8], name: &str, addr: &str) -> String {
        let mut wb = calamine::open_workbook_auto_from_rs(std::io::Cursor::new(data.to_vec())).unwrap();
        let (c, r) = parse_addr(addr).unwrap();
        wb.worksheet_formula(name).unwrap().get_value((r - 1, c - 1)).cloned().unwrap_or_default()
    }

    #[test]
    fn reads_values_as_a_person_would() {
        let b = Book::open(&sample_xlsx()).unwrap();
        let (name, text, stopped) = b.read_rows("预算 表", 1, 100, "", 10_000).unwrap();
        assert_eq!((name.as_str(), stopped), ("预算 表", None));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "1：A=十月预算");
        assert_eq!(lines[2], "3：A=房租 | B=3000 | C=2026-10-01 | D==B3*2（结果 6000） | E=12.5%");
        assert_eq!(lines[3], "4：A=水电 | B=120.5 | C=2026-10-05 | D==B4*2（结果 241） | E=007");
        assert_eq!(lines[5], "6：A=合计 | B==SUM(B3:B5)（结果 3200.5）");
        assert!(b.outline().contains("「预算 表」6 行，到第 6 行、E 列"), "{}", b.outline());
        assert_eq!(b.merged_text("预算 表").unwrap(), "A1:B1");
        let (_, found, _) = b.read_rows("预算 表", 1, 100, "网费", 10_000).unwrap();
        assert_eq!(found.lines().count(), 1);
        assert!(b.read_rows("没有", 1, 9, "", 100).unwrap_err().contains("预算 表、引用"));
        // cut to the room: says where to go on
        let (_, _, stopped) = b.read_rows("预算 表", 1, 100, "", 12).unwrap();
        assert!(stopped.is_some());
    }

    #[test]
    fn cells_change_keeping_everything_else() {
        let data = sample_xlsx();
        let b = Book::open(&data).unwrap();
        // numbers, a date in a date cell, a percent, text, a formula, emptied, a code that stays text, a new cell
        let ops: Vec<Op> = [("B3", "3200"), ("C4", "2026-11-05"), ("E3", "20%"), ("A5", "宽带 <100M> & 电话"), ("B5", "=B4+1"), ("B4", ""), ("E4", "008"), ("G9", "新的")]
            .iter()
            .map(|(c, v)| b.plan_cell("预算 表", c, v).unwrap().op)
            .collect();
        let out = apply(&data, &ops).unwrap();
        let r = sheet_of(&out, "预算 表");
        assert_eq!(value(&r, 3, 2), Data::Float(3200.0));
        assert_eq!(value(&r, 3, 5), Data::Float(0.2));
        assert_eq!(value(&r, 5, 1), Data::String("宽带 <100M> & 电话".into()));
        assert_eq!(value(&r, 4, 5), Data::String("008".into()));
        assert_eq!(value(&r, 9, 7), Data::String("新的".into()));
        assert_eq!(value(&r, 4, 2), Data::Empty);
        assert_eq!(formula_of_cell(&out, "预算 表", "B5"), "B4+1");
        let again = Book::open(&out).unwrap();
        let (_, text, _) = again.read_rows("预算 表", 1, 100, "", 10_000).unwrap();
        assert!(text.contains("C=2026-11-05"), "{text}"); // read back as a date: the format stayed
        assert!(text.contains("A=合计"), "{text}");
        // untouched: the other sheet's bytes, the strings; a full recalc asked, the calc chain gone
        assert_eq!(part(&out, "xl/worksheets/sheet2.xml").unwrap(), part(&data, "xl/worksheets/sheet2.xml").unwrap());
        assert_eq!(part(&out, STRINGS).unwrap(), part(&data, STRINGS).unwrap());
        assert!(part(&out, WORKBOOK).unwrap().unwrap().contains(r#"<calcPr calcId="191029" fullCalcOnLoad="1"/>"#));
        assert!(part(&out, CALC_CHAIN).unwrap().is_none());
        assert!(!part(&out, CONTENT_TYPES).unwrap().unwrap().contains("calcChain") && !part(&out, WORKBOOK_RELS).unwrap().unwrap().contains("calcChain"));
        // the dimension follows; a row kept its look
        let xml = part(&out, "xl/worksheets/sheet1.xml").unwrap().unwrap();
        assert!(xml.contains(r#"<dimension ref="A1:G9"/>"#) && xml.contains(r#"<row r="1" ht="20" customHeight="1"><c r="A1" s="1" t="s"><v>0</v></c>"#), "{xml}");
    }

    #[test]
    fn a_shared_master_going_keeps_its_children() {
        let data = sample_xlsx();
        let b = Book::open(&data).unwrap();
        let op = b.plan_cell("预算 表", "D3", "1").unwrap().op;
        let out = apply(&data, &[op]).unwrap();
        assert_eq!(formula_of_cell(&out, "预算 表", "D4"), "B4*2");
        assert_eq!(formula_of_cell(&out, "预算 表", "D5"), "B5*2");
        assert_eq!(value(&sheet_of(&out, "预算 表"), 3, 4), Data::Float(1.0));
    }

    #[test]
    fn a_plain_cell_gets_a_date_or_percent_format() {
        let data = sample_xlsx();
        let b = Book::open(&data).unwrap();
        let ops = vec![b.plan_cell("预算 表", "F3", "2026-12-25").unwrap().op, b.plan_cell("预算 表", "F4", "50%").unwrap().op, b.plan_cell("预算 表", "C3", "'2026-01-01").unwrap().op];
        let out = apply(&data, &ops).unwrap();
        let again = Book::open(&out).unwrap();
        let (_, text, _) = again.read_rows("预算 表", 1, 100, "", 10_000).unwrap();
        assert!(text.contains("F=2026-12-25") && text.contains("F=50%") && text.contains("C=2026-01-01"), "{text}");
        let styles = part(&out, STYLES).unwrap().unwrap();
        assert!(styles.contains(r#"<cellXfs count="6">"#), "{styles}");
        assert_eq!(value(&sheet_of(&out, "预算 表"), 3, 3), Data::String("2026-01-01".into()));
    }

    #[test]
    fn what_cant_change_is_refused() {
        let b = Book::open(&sample_xlsx()).unwrap();
        assert!(b.plan_cell("预算 表", "B1", "x").unwrap_err().contains("改左上角的 A1"));
        assert!(b.plan_cell("预算 表", "什么", "x").unwrap_err().contains("不是格子"));
        assert!(b.plan_cell("预算 表", "B3", "=SUM(A1").unwrap_err().contains("括号"));
        assert!(b.plan_cell("预算 表", "B3", "=A1+").unwrap_err().contains("没写完"));
        assert!(b.plan_cell("预算 表", "B3", "=FILTER(A1:A3,B1:B3>0)").unwrap_err().contains("FILTER"));
        // a formula on its own cell (the 合计 taken past itself)
        assert!(b.plan_cell("预算 表", "B6", "=SUM(B3:B6)").unwrap_err().contains("循环引用"));
        assert!(b.plan_cell("预算 表", "B6", "=SUM(B3:B5)+'预算 表'!B6").unwrap_err().contains("循环引用"));
        assert!(b.plan_cell("预算 表", "B6", "=SUM(B3:B5)+引用!B6").is_ok());
        assert!(b.plan_delete("预算 表", 1).unwrap_err().contains("合并"));
        assert!(b.plan_delete("预算 表", 99).unwrap_err().contains("空的"));
    }

    #[test]
    fn newer_functions_get_excels_name() {
        let data = sample_xlsx();
        let b = Book::open(&data).unwrap();
        let out = apply(&data, &[b.plan_cell("预算 表", "F3", "=xlookup(A3,A3:A5,B3:B5)").unwrap().op, b.plan_cell("预算 表", "F4", r#"=IF(A3="XLOOKUP(",1,2)"#).unwrap().op]).unwrap();
        let xml = part(&out, "xl/worksheets/sheet1.xml").unwrap().unwrap();
        assert!(xml.contains("<f>_xlfn.xlookup(A3,A3:A5,B3:B5)</f>") && xml.contains(r#"IF(A3="XLOOKUP(",1,2)"#), "{xml}");
        let (_, text, _) = Book::open(&out).unwrap().read_rows("预算 表", 3, 3, "", 10_000).unwrap();
        assert!(text.contains("F==xlookup(A3,A3:A5,B3:B5)"), "{text}");
    }

    #[test]
    fn adding_rows_moves_whats_below() {
        let data = sample_xlsx();
        let b = Book::open(&data).unwrap();
        let p = b.plan_add("预算 表", 4, &[vec!["水费".into(), "60".into(), "2026-10-20".into(), "=B5*2".into()], vec!["电费".into(), "70".into()]]).unwrap();
        assert_eq!((p.label.as_str(), p.after.as_str()), ("表「预算 表」第 4 行后加 2 行", "水费 | 60 | 2026-10-20 | =B5*2\n电费 | 70"));
        let out = apply(&data, &[p.op]).unwrap();
        let r = sheet_of(&out, "预算 表");
        assert_eq!(value(&r, 5, 1), Data::String("水费".into()));
        assert_eq!(value(&r, 6, 2), Data::Float(70.0));
        assert_eq!(value(&r, 7, 1), Data::String("网费".into()));
        assert_eq!(value(&r, 8, 1), Data::String("合计".into()));
        // the new date got the look of the one above; formulas follow, the total's range grew
        let again = Book::open(&out).unwrap();
        let (_, text, _) = again.read_rows("预算 表", 5, 5, "", 1000).unwrap();
        assert!(text.contains("C=2026-10-20"), "{text}");
        assert_eq!(formula_of_cell(&out, "预算 表", "B8"), "SUM(B3:B7)");
        assert_eq!(formula_of_cell(&out, "预算 表", "D7"), "B7*2");
        assert_eq!(formula_of_cell(&out, "预算 表", "D4"), "B4*2");
        // other sheets and the defined name too; a reference above stays
        assert_eq!(formula_of_cell(&out, "引用", "A1"), "'预算 表'!B8*2");
        assert_eq!(formula_of_cell(&out, "引用", "B1"), "'预算 表'!B4");
        assert!(part(&out, WORKBOOK).unwrap().unwrap().contains("'预算 表'!$B$8"));
        // conditional format and its rule
        let xml = part(&out, "xl/worksheets/sheet1.xml").unwrap().unwrap();
        assert!(xml.contains(r#"sqref="B3:B7""#) && xml.contains("<formula>B3&gt;100</formula>"), "{xml}");
        // added right after the last item: the 合计 below takes it in (Excel would not); another sheet's range stays
        assert_eq!(b.ranges_ending_at("预算 表", 5), ["B6（=SUM(B3:B5)）"]);
        assert!(b.ranges_ending_at("预算 表", 4).is_empty());
        let out = apply(&data, &[b.plan_add("预算 表", 5, &[vec!["物业费".into(), "150".into()]]).unwrap().op]).unwrap();
        assert_eq!(formula_of_cell(&out, "预算 表", "B7"), "SUM(B3:B6)");
        assert_eq!(formula_of_cell(&out, "预算 表", "D5"), "B5*2");
        assert_eq!(formula_of_cell(&out, "引用", "A1"), "'预算 表'!B7*2");
    }

    #[test]
    fn a_range_grows_only_from_outside_its_rows() {
        assert_eq!(grown_formula("SUM(B3:B5)", "S", 9, 5, 2), "SUM(B3:B7)"); // a total below
        assert_eq!(grown_formula("SUM(B5:B3)", "S", 1, 5, 1), "SUM(B6:B3)"); // above, written backwards
        assert!(!grows("SUM(B3:B5)", "S", 5, 5)); // a running total inside the rows
        assert!(!grows("SUM(B3:D3)", "S", 9, 3) && !grows("B5*2", "S", 9, 5) && !grows("其他!B3:B5", "S", 9, 5));
        assert!(grows("'S'!B3:B5", "S", 9, 5));
    }

    #[test]
    fn deleting_rows_moves_whats_below() {
        let data = sample_xlsx();
        let b = Book::open(&data).unwrap();
        let p = b.plan_delete("预算 表", 4).unwrap();
        assert!(p.before.starts_with("A=水电 | B=120.5"), "{}", p.before);
        let out = apply(&data, &[p.op]).unwrap();
        let r = sheet_of(&out, "预算 表");
        assert_eq!(value(&r, 4, 1), Data::String("网费".into()));
        assert_eq!(value(&r, 5, 1), Data::String("合计".into()));
        assert_eq!(formula_of_cell(&out, "预算 表", "B5"), "SUM(B3:B4)");
        assert_eq!(formula_of_cell(&out, "预算 表", "D4"), "B4*2");
        // a cell that pointed at the deleted row
        assert_eq!(formula_of_cell(&out, "引用", "B1"), "'预算 表'!#REF!");
        assert_eq!(formula_of_cell(&out, "引用", "A1"), "'预算 表'!B5*2");
        let xml = part(&out, "xl/worksheets/sheet1.xml").unwrap().unwrap();
        assert!(xml.contains(r#"sqref="B3:B4""#), "{xml}");
    }

    #[test]
    fn several_changes_use_the_numbers_as_read() {
        let data = sample_xlsx();
        let b = Book::open(&data).unwrap();
        let ops = vec![
            b.plan_delete("预算 表", 3).unwrap().op,
            b.plan_add("预算 表", 5, &[vec!["加的".into()]]).unwrap().op,
            b.plan_cell("预算 表", "B5", "90").unwrap().op,
            b.plan_delete("预算 表", 4).unwrap().op,
        ];
        let out = apply(&data, &ops).unwrap();
        let r = sheet_of(&out, "预算 表");
        assert_eq!(value(&r, 3, 1), Data::String("网费".into()));
        assert_eq!(value(&r, 3, 2), Data::Float(90.0));
        assert_eq!(value(&r, 4, 1), Data::String("加的".into()));
        assert_eq!(value(&r, 5, 1), Data::String("合计".into()));
        // 加一行、删一行 around the total: it sums what is left
        let out = apply(&data, &[b.plan_delete("预算 表", 4).unwrap().op, b.plan_add("预算 表", 5, &[vec!["物业费".into(), "150".into()]]).unwrap().op]).unwrap();
        assert_eq!((value(&sheet_of(&out, "预算 表"), 5, 1), formula_of_cell(&out, "预算 表", "B6")), (Data::String("物业费".into()), "SUM(B3:B5)".into()));
    }

    #[test]
    fn a_sheet_with_a_chart_keeps_its_rows() {
        let chart = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="r"><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c></row></sheetData><drawing r:id="rId1"/></worksheet>"#;
        let locked = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData><sheetProtection sheet="1"/></worksheet>"#;
        let spanning = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><f>SUM(a:b!A1)</f></c></row></sheetData></worksheet>"#;
        let workbook = r#"<workbook xmlns="m" xmlns:r="r"><sheets><sheet name="图" sheetId="1" r:id="rId1"/><sheet name="锁" sheetId="2" r:id="rId2"/><sheet name="跨" sheetId="3" r:id="rId3"/></sheets></workbook>"#;
        let rels = r#"<Relationships xmlns="p"><Relationship Id="rId1" Type="t" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="t" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="t" Target="/xl/worksheets/sheet3.xml"/></Relationships>"#;
        let data = zipped(&[(WORKBOOK, workbook), (WORKBOOK_RELS, rels), ("xl/worksheets/sheet1.xml", chart), ("xl/worksheets/sheet2.xml", locked), ("xl/worksheets/sheet3.xml", spanning)]);
        let b = Book::open(&data).unwrap();
        assert!(b.plan_cell("图", "A1", "9").is_ok());
        assert!(b.plan_delete("图", 1).unwrap_err().contains("图表或图片"));
        assert!(b.plan_cell("锁", "A1", "9").unwrap_err().contains("保护"));
        assert!(b.plan_add("跨", 1, &[vec!["x".into()]]).unwrap_err().contains("跨几张表"));
        assert!(b.outline().contains("设了保护") && b.outline().contains("不能加行删行"));
        // no calcPr yet: one is added after the sheets
        let out = apply(&data, &[b.plan_cell("图", "A1", "9").unwrap().op]).unwrap();
        assert!(part(&out, WORKBOOK).unwrap().unwrap().contains(r#"</sheets><calcPr fullCalcOnLoad="1"/>"#));
    }

    #[test]
    fn references_move_the_way_excel_moves_them() {
        let ins = Move::Insert { after: 5, count: 2 };
        let del = Move::Delete { row: 5 };
        let f = |s: &str, m| moved_formula(s, "S", "S", m).unwrap();
        assert_eq!(f("A5+A6+$B$7", ins), "A5+A8+$B$9");
        assert_eq!(f("SUM(A1:A10)", ins), "SUM(A1:A12)");
        assert_eq!(f("A5+A6", del), "#REF!+A5");
        assert_eq!(f("SUM(A5:A9)", del), "SUM(A5:A8)");
        assert_eq!(f("SUM(A1:A5)", del), "SUM(A1:A4)");
        assert_eq!(f("SUM(A5:A5)", del), "SUM(#REF!)");
        assert_eq!(f("SUM(6:9)+SUM(A:A)", ins), "SUM(8:11)+SUM(A:A)");
        assert_eq!(f(r#"IF(A9="A9",LOG10(A9),TAXES2024)"#, del), r#"IF(A8="A9",LOG10(A8),TAXES2024)"#);
        assert_eq!(moved_formula("别的!A9+A9", "S", "S", del).unwrap(), "别的!A9+A8");
        assert_eq!(moved_formula("'S'!A9+A9", "别的", "S", del).unwrap(), "'S'!A8+A9");
        assert_eq!(moved_formula("[1]S!A9+A9", "S", "S", del).unwrap(), "[1]S!A9+A8");
        assert_eq!(translate("$A$1+A1+B$2+$C3+SUM(A1:A2)", 1, 2), "$A$1+B3+C$2+$C5+SUM(B3:B4)");
        assert_eq!(moved_sqref("A1:B3 D9", del).unwrap(), "A1:B3 D8");
        assert_eq!(moved_sqref("A5", del), None);
    }

    /// Writes sample files to look at with another program: `POMELO_EXCEL_PREVIEW=<文件夹> cargo test excel_samples -- --ignored`.
    #[test]
    #[ignore]
    fn excel_samples() {
        let dir = std::path::PathBuf::from(std::env::var("POMELO_EXCEL_PREVIEW").unwrap());
        let data = sample_xlsx();
        let b = Book::open(&data).unwrap();
        let ops = vec![
            b.plan_cell("预算 表", "B3", "3200").unwrap().op,
            b.plan_cell("预算 表", "F3", "2026-12-25").unwrap().op,
            b.plan_cell("预算 表", "A5", "宽带 <100M> & 电话").unwrap().op,
            b.plan_cell("预算 表", "F4", "=XLOOKUP(A3,A3:A5,B3:B5)").unwrap().op,
            b.plan_add("预算 表", 4, &[vec!["水费".into(), "60".into(), "2026-10-20".into(), "=B5*2".into()]]).unwrap().op,
            b.plan_delete("预算 表", 3).unwrap().op,
        ];
        std::fs::write(dir.join("改过的.xlsx"), apply(&data, &ops).unwrap()).unwrap();
        let new = clean_sheets(vec![NewSheet { name: "十月开销".into(), rows: vec![vec!["项目".into(), "金额".into(), "日期".into(), "比例".into(), "两倍".into()], vec!["房租".into(), "3000".into(), "2026-10-01".into(), "12.5%".into(), "=B2*2".into()], vec!["编号".into(), "'007".into()]] }]);
        std::fs::write(dir.join("新建的.xlsx"), new_xlsx(&new).unwrap()).unwrap();
    }

    #[test]
    fn dates_both_ways() {
        assert_eq!(serial_text(46296.0, false), "2026-10-01");
        assert_eq!(serial_text(46296.5, false), "2026-10-01 12:00");
        assert_eq!(serial_text(44835.0, true), "2026-10-02");
        assert_eq!(serial_text(0.25, false), "06:00");
        assert_eq!(parse_date("2026-10-01", false), Some(46296.0));
        assert_eq!(parse_date("2026/10/1 12:00", false), Some(46296.5));
        assert_eq!(parse_date("2026-02-30", false), None);
        assert_eq!(parse_date("26-1-1", false), None);
    }

    #[test]
    fn a_new_workbook() {
        let sheets = clean_sheets(vec![
            NewSheet { name: "预算/十月".into(), rows: vec![vec!["项目".into(), "金额".into(), "日期".into(), "比例".into(), "两倍".into()], vec!["房租".into(), "3000".into(), "2026-10-01".into(), "12.5%".into(), "=B2*2".into()], vec!["编号".into(), "'007".into()]] },
            NewSheet { name: "预算 十月".into(), rows: vec![vec!["x".into()]] },
        ]);
        assert_eq!(sheets[0].name, "预算 十月");
        assert_eq!(sheets[1].name, "预算 十月 2");
        let data = new_xlsx(&sheets).unwrap();
        let r = sheet_of(&data, "预算 十月");
        assert_eq!(value(&r, 1, 1), Data::String("项目".into()));
        assert_eq!(value(&r, 2, 2), Data::Float(3000.0));
        assert_eq!(value(&r, 2, 4), Data::Float(0.125));
        assert_eq!(value(&r, 3, 2), Data::String("007".into()));
        assert_eq!(formula_of_cell(&data, "预算 十月", "E2"), "B2*2");
        let b = Book::open(&data).unwrap();
        let (_, text, _) = b.read_rows("预算 十月", 1, 9, "", 1000).unwrap();
        assert!(text.contains("C=2026-10-01") && text.contains("D=12.5%"), "{text}");
        // what it made can be changed like any other
        let out = apply(&data, &[b.plan_add("预算 十月", 2, &[vec!["水费".into(), "60".into()]]).unwrap().op]).unwrap();
        assert_eq!(value(&sheet_of(&out, "预算 十月"), 3, 1), Data::String("水费".into()));
        assert!(preview(&sheets).contains("| 房租 | 3000 | 2026-10-01 | 12.5% | =B2*2 |"));
        assert!(new_xlsx(&[]).is_err());
    }
}
