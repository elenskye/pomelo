//! The website's front matter. Read with yaml-rust2 (keys keep their order); written exactly in the
//! site's existing format: js-yaml 4.3's `dump` with its defaults (indent 2, line width 80, single quotes,
//! block styles; strings another type would read as are quoted, long lines folded with `>-`, multi-line
//! text as `|-`). `dump` below is a port of js-yaml's lib/dumper.js for these values — widths and
//! positions counted in UTF-16 units as JavaScript does; tests hold its own output for many cases.

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Y {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Seq(Vec<Y>),
    Map(Vec<(String, Y)>),
}

impl Y {
    pub fn get(&self, key: &str) -> Option<&Y> {
        match self {
            Y::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    /// Text as the site reads it: strings, and numbers / booleans written plain.
    pub fn text(&self) -> Option<String> {
        match self {
            Y::Str(s) => Some(s.clone()),
            Y::Int(i) => Some(i.to_string()),
            Y::Float(f) => Some(float(*f)),
            Y::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
    pub fn seq(&self) -> &[Y] {
        match self {
            Y::Seq(v) => v,
            _ => &[],
        }
    }
    /// Sets a key in place (a new one goes before the first of `after` that is present, else last).
    pub fn set(&mut self, key: &str, value: Y, before: &[&str]) {
        let Y::Map(m) = self else { return };
        if let Some(slot) = m.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value;
            return;
        }
        let at = m.iter().position(|(k, _)| before.contains(&k.as_str())).unwrap_or(m.len());
        m.insert(at, (key.to_string(), value));
    }
    pub fn remove(&mut self, key: &str) {
        if let Y::Map(m) = self {
            m.retain(|(k, _)| k != key);
        }
    }
}

// ---------- reading ----------

fn from_yaml(y: yaml_rust2::Yaml) -> Result<Y, String> {
    use yaml_rust2::Yaml as R;
    Ok(match y {
        R::Null => Y::Null,
        R::Boolean(b) => Y::Bool(b),
        R::Integer(i) => Y::Int(i),
        R::Real(s) => Y::Float(parse_real(&s)),
        R::String(s) => Y::Str(s),
        R::Array(v) => Y::Seq(v.into_iter().map(from_yaml).collect::<Result<_, _>>()?),
        R::Hash(h) => Y::Map(
            h.into_iter()
                .map(|(k, v)| {
                    let key = match k {
                        R::String(s) => s,
                        R::Integer(i) => i.to_string(),
                        R::Boolean(b) => b.to_string(),
                        _ => return Err("开头字段里有读不懂的键".to_string()),
                    };
                    Ok((key, from_yaml(v)?))
                })
                .collect::<Result<_, _>>()?,
        ),
        R::Alias(_) | R::BadValue => return Err("开头字段里有读不懂的写法".into()),
    })
}

fn parse_real(s: &str) -> f64 {
    match s.trim_start_matches(['+', '-']) {
        ".inf" | ".Inf" | ".INF" => {
            if s.starts_with('-') {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        }
        ".nan" | ".NaN" | ".NAN" => f64::NAN,
        _ => s.parse().unwrap_or(0.0),
    }
}

/// One YAML document (empty = an empty mapping).
pub fn parse(text: &str) -> Result<Y, String> {
    let docs = yaml_rust2::YamlLoader::load_from_str(text).map_err(|e| format!("开头字段的格式不对：{e}"))?;
    match docs.into_iter().next() {
        None => Ok(Y::Map(vec![])),
        Some(d) => from_yaml(d),
    }
}

/// (front matter, body) of a `---` … `---` Markdown file.
pub fn split_front(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    if let Some(body) = rest.strip_prefix("---\n") {
        return Some(("", body));
    }
    let end = rest.find("\n---\n").map(|i| (i + 1, i + 5)).or_else(|| rest.strip_suffix("\n---").map(|r| (r.len() + 1, rest.len())))?;
    Some((&rest[..end.0], &rest[end.1..]))
}

/// A Markdown file in the site's format.
pub fn join_front(front: &Y, body: &str) -> String {
    format!("---\n{}---\n{body}", dump(front))
}

// ---------- writing (js-yaml's dumper) ----------

const INDENT: usize = 2;
const LINE_WIDTH: usize = 80;

/// js-yaml `dump(value)`.
pub fn dump(y: &Y) -> String {
    node(y, 0, true, true, false) + "\n"
}

fn next_line(level: usize) -> String {
    format!("\n{}", " ".repeat(INDENT * level))
}

fn node(y: &Y, level: usize, block: bool, compact: bool, iskey: bool) -> String {
    match y {
        Y::Null => "null".into(),
        Y::Bool(b) => b.to_string(),
        Y::Int(i) => i.to_string(),
        Y::Float(f) => float(*f),
        Y::Str(s) => scalar(s, level, iskey, block),
        Y::Map(m) if block && !m.is_empty() => block_map(m, level, compact),
        Y::Map(m) => {
            let pairs: Vec<String> = m.iter().map(|(k, v)| format!("{}: {}", node(&Y::Str(k.clone()), level, false, false, false), node(v, level, false, false, false))).collect();
            format!("{{{}}}", pairs.join(", "))
        }
        Y::Seq(v) if block && !v.is_empty() => block_seq(v, level, compact),
        Y::Seq(v) => format!("[{}]", v.iter().map(|x| node(x, level, false, false, false)).collect::<Vec<_>>().join(", ")),
    }
}

/// JavaScript's number → text (integers without a point).
fn float(f: f64) -> String {
    if f.is_nan() {
        ".nan".into()
    } else if f.is_infinite() {
        if f > 0.0 { ".inf" } else { "-.inf" }.into()
    } else if f == 0.0 && f.is_sign_negative() {
        "-0.0".into()
    } else if f.fract() == 0.0 && f.abs() < 1e21 {
        format!("{f:.0}")
    } else {
        format!("{f}")
    }
}

fn block_map(m: &[(String, Y)], level: usize, compact: bool) -> String {
    let mut out = String::new();
    for (k, v) in m {
        if !compact || !out.is_empty() {
            out += &next_line(level);
        }
        out += &node(&Y::Str(k.clone()), level + 1, true, true, true);
        let value = node(v, level + 1, true, false, false);
        out += if value.starts_with('\n') { ":" } else { ": " };
        out += &value;
    }
    out
}

fn block_seq(v: &[Y], level: usize, compact: bool) -> String {
    let mut out = String::new();
    for item in v {
        let value = node(item, level + 1, true, true, false);
        if !compact || !out.is_empty() {
            out += &next_line(level);
        }
        out += if value.starts_with('\n') { "-" } else { "- " };
        out += &value;
    }
    out
}

const DEPRECATED_BOOLEANS: [&str; 16] = ["y", "Y", "yes", "Yes", "YES", "on", "On", "ON", "n", "N", "no", "No", "NO", "off", "Off", "OFF"];

#[derive(Debug, PartialEq)]
enum Style {
    Plain,
    Single,
    Literal,
    Folded,
    Double,
}

fn scalar(s: &str, level: usize, iskey: bool, inblock: bool) -> String {
    if s.is_empty() {
        return "''".into();
    }
    if DEPRECATED_BOOLEANS.contains(&s) || base60(s) {
        return format!("'{s}'");
    }
    let indent = INDENT * level.max(1);
    let width = LINE_WIDTH.min(40).max(LINE_WIDTH.saturating_sub(indent));
    match choose(s, iskey, width, inblock) {
        Style::Plain => s.into(),
        Style::Single => format!("'{}'", s.replace('\'', "''")),
        Style::Literal => format!("|{}{}", header(s), drop_end_newline(indent_string(s, indent))),
        Style::Folded => format!(">{}{}", header(s), drop_end_newline(indent_string(&fold_string(s, width), indent))),
        Style::Double => format!("\"{}\"", escape(s)),
    }
}

fn printable(c: u32) -> bool {
    (0x20..=0x7E).contains(&c) || ((0xA1..=0xD7FF).contains(&c) && c != 0x2028 && c != 0x2029) || ((0xE000..=0xFFFD).contains(&c) && c != 0xFEFF) || (0x10000..=0x10FFFF).contains(&c)
}

const fn ws(c: u32) -> bool {
    c == 0x20 || c == 0x09
}

fn ns_char_or_ws(c: u32) -> bool {
    printable(c) && c != 0xFEFF && c != 0x0D && c != 0x0A
}

fn plain_safe(c: u32, prev: Option<u32>, inblock: bool) -> bool {
    let c_any = ns_char_or_ws(c);
    let c_ns = c_any && !ws(c);
    let colon = prev == Some(':' as u32);
    ((if inblock { c_any } else { c_any && !",[]{}".contains(char::from_u32(c).unwrap_or('\0')) }) && c != '#' as u32 && !(colon && !c_ns))
        || (prev.is_some_and(|p| ns_char_or_ws(p) && !ws(p)) && c == '#' as u32)
        || (colon && c_ns)
}

fn plain_safe_first(c: u32) -> bool {
    printable(c) && c != 0xFEFF && !ws(c) && !"-?:,[]{}#&*!|=>'\"%@`".contains(char::from_u32(c).unwrap_or('\0'))
}

/// js-yaml `chooseScalarStyle` (string non-empty; positions in UTF-16 units).
fn choose(s: &str, single_line_only: bool, width: usize, inblock: bool) -> Style {
    let u: Vec<u16> = s.encode_utf16().collect();
    let first = s.chars().next().map_or(0, |c| c as u32);
    let last = *u.last().unwrap_or(&0) as u32;
    let mut plain = plain_safe_first(first) && !ws(last) && last != ':' as u32;
    let mut prev: Option<u32> = None;
    let (mut line_break, mut foldable) = (false, false);
    let mut prev_lb: i64 = -1;
    let mut i: i64 = 0;
    let foldable_line = |i: i64, prev_lb: i64| i - prev_lb - 1 > width as i64 && u.get((prev_lb + 1) as usize) != Some(&0x20);
    for ch in s.chars() {
        let c = ch as u32;
        if single_line_only {
            if !printable(c) {
                return Style::Double;
            }
        } else if c == 0x0A {
            line_break = true;
            foldable = foldable || foldable_line(i, prev_lb);
            prev_lb = i;
        } else if !printable(c) {
            return Style::Double;
        }
        plain = plain && plain_safe(c, prev, inblock);
        prev = Some(c);
        i += ch.len_utf16() as i64;
    }
    if !single_line_only {
        foldable = foldable || foldable_line(i, prev_lb);
    }
    if !line_break && !foldable {
        return if plain && !ambiguous(s) { Style::Plain } else { Style::Single };
    }
    if foldable {
        Style::Folded
    } else {
        Style::Literal
    }
}

fn header(s: &str) -> String {
    let indicator = if s.trim_start_matches('\n').starts_with(' ') { INDENT.to_string() } else { String::new() };
    let clip = s.ends_with('\n');
    let keep = clip && (s.ends_with("\n\n") || s == "\n");
    let chomp = if keep { "+" } else if clip { "" } else { "-" };
    format!("{indicator}{chomp}\n")
}

fn drop_end_newline(s: String) -> String {
    match s.strip_suffix('\n') {
        Some(t) => t.to_string(),
        None => s,
    }
}

fn indent_string(s: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    let mut out = String::new();
    for line in s.split_inclusive('\n') {
        if !line.is_empty() && line != "\n" {
            out += &pad;
        }
        out += line;
    }
    out
}

const NL: u16 = 0x0A;
const SP: u16 = 0x20;

fn fold_string(s: &str, width: usize) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    let first_lf = u.iter().position(|&c| c == NL).unwrap_or(u.len());
    let mut out = fold_line(&u[..first_lf], width);
    let mut prev_more = matches!(u.first(), Some(&NL) | Some(&SP));
    let mut i = first_lf;
    while i < u.len() {
        let mut j = i;
        while j < u.len() && u[j] == NL {
            j += 1;
        }
        let mut k = j;
        while k < u.len() && u[k] != NL {
            k += 1;
        }
        let line = &u[j..k];
        let more = line.first() == Some(&SP);
        out.extend_from_slice(&u[i..j]);
        if !prev_more && !more && !line.is_empty() {
            out.push(NL);
        }
        out.extend(fold_line(line, width));
        prev_more = more;
        i = k;
    }
    String::from_utf16_lossy(&out)
}

/// Greedy breaking at spaces (a break never followed by a space).
fn fold_line(line: &[u16], width: usize) -> Vec<u16> {
    if line.is_empty() || line[0] == SP {
        return line.to_vec();
    }
    let (mut start, mut curr) = (0usize, 0usize);
    let mut out: Vec<u16> = vec![];
    let mut i = 0;
    while i + 1 < line.len() {
        if line[i] == SP && line[i + 1] != SP {
            let next = i;
            if next - start > width {
                let end = if curr > start { curr } else { next };
                out.push(NL);
                out.extend_from_slice(&line[start..end]);
                start = end + 1;
            }
            curr = next;
            i += 2;
        } else {
            i += 1;
        }
    }
    out.push(NL);
    if line.len() - start > width && curr > start {
        out.extend_from_slice(&line[start..curr]);
        out.push(NL);
        out.extend_from_slice(&line[curr + 1..]);
    } else {
        out.extend_from_slice(&line[start..]);
    }
    out.remove(0);
    out
}

fn escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        let c = ch as u32;
        let seq = match c {
            0x00 => Some("\\0"),
            0x07 => Some("\\a"),
            0x08 => Some("\\b"),
            0x09 => Some("\\t"),
            0x0A => Some("\\n"),
            0x0B => Some("\\v"),
            0x0C => Some("\\f"),
            0x0D => Some("\\r"),
            0x1B => Some("\\e"),
            0x22 => Some("\\\""),
            0x5C => Some("\\\\"),
            0x85 => Some("\\N"),
            0xA0 => Some("\\_"),
            0x2028 => Some("\\L"),
            0x2029 => Some("\\P"),
            _ => None,
        };
        match seq {
            Some(e) => out += e,
            None if printable(c) => out.push(ch),
            None if c <= 0xFF => _ = write!(out, "\\x{c:02X}"),
            None if c <= 0xFFFF => _ = write!(out, "\\u{c:04X}"),
            None => _ = write!(out, "\\U{c:08X}"),
        }
    }
    out
}

// js-yaml's implicit types (default schema): a string reading as one of them must be quoted.

fn ambiguous(s: &str) -> bool {
    matches!(s, "~" | "null" | "Null" | "NULL" | "true" | "True" | "TRUE" | "false" | "False" | "FALSE" | "<<") || int_like(s) || float_like(s) || timestamp_like(s)
}

fn digits(b: &[u8], from: usize, ok: impl Fn(u8) -> bool) -> usize {
    b[from..].iter().take_while(|&&c| ok(c)).count()
}

fn int_like(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'-') | Some(b'+')) {
        i = 1;
    }
    if b.get(i) == Some(&b'0') {
        if i + 1 == b.len() {
            return true;
        }
        i += 1;
        let base: Option<fn(u8) -> bool> = match b.get(i) {
            Some(b'b') => Some(|c| c == b'0' || c == b'1'),
            Some(b'x') => Some(|c: u8| c.is_ascii_hexdigit()),
            Some(b'o') => Some(|c| (b'0'..=b'7').contains(&c)),
            _ => None,
        };
        if let Some(ok) = base {
            let n = digits(b, i + 1, ok);
            return n > 0 && i + 1 + n == b.len();
        }
    }
    let n = digits(b, i, |c| c.is_ascii_digit());
    n > 0 && i + n == b.len() && n < 309
}

fn float_like(s: &str) -> bool {
    let b = s.as_bytes();
    let unsigned = s.strip_prefix('-').or_else(|| s.strip_prefix('+')).unwrap_or(s);
    if matches!(s, ".nan" | ".NaN" | ".NAN") || matches!(unsigned, ".inf" | ".Inf" | ".INF") {
        return true;
    }
    let exp = |i: usize| -> Option<usize> {
        if !matches!(b.get(i), Some(b'e') | Some(b'E')) {
            return Some(i);
        }
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'-') | Some(b'+')) {
            j += 1;
        }
        let n = digits(b, j, |c| c.is_ascii_digit());
        (n > 0).then_some(j + n)
    };
    let end = if b.first() == Some(&b'.') {
        let n = digits(b, 1, |c| c.is_ascii_digit());
        if n == 0 {
            return false;
        }
        exp(1 + n)
    } else {
        let mut i = usize::from(matches!(b.first(), Some(b'-') | Some(b'+')));
        let n = digits(b, i, |c| c.is_ascii_digit());
        if n == 0 {
            return false;
        }
        i += n;
        if b.get(i) == Some(&b'.') {
            i += 1 + digits(b, i + 1, |c| c.is_ascii_digit());
        }
        exp(i)
    };
    end == Some(b.len()) && s.parse::<f64>().map_or(true, f64::is_finite)
}

fn timestamp_like(s: &str) -> bool {
    let b = s.as_bytes();
    let d = |i: usize, n: usize| b.len() >= i + n && b[i..i + n].iter().all(u8::is_ascii_digit);
    // YYYY-MM-DD
    if b.len() == 10 && d(0, 4) && b[4] == b'-' && d(5, 2) && b[7] == b'-' && d(8, 2) {
        return true;
    }
    // YYYY-M(M)-D(D)(T| +)H(H):MM:SS(.f*)?( *(Z|[-+]H(H)(:MM)?))?
    let mut i = 0;
    let take = |i: &mut usize, min: usize, max: usize| -> bool {
        let n = digits(b, *i, |c| c.is_ascii_digit()).min(max);
        *i += n;
        n >= min
    };
    if !(take(&mut i, 4, 4) && b.get(i) == Some(&b'-')) {
        return false;
    }
    i += 1;
    if !(take(&mut i, 1, 2) && b.get(i) == Some(&b'-')) {
        return false;
    }
    i += 1;
    if !take(&mut i, 1, 2) {
        return false;
    }
    match b.get(i) {
        Some(b'T') | Some(b't') => i += 1,
        Some(b' ') | Some(b'\t') => {
            while matches!(b.get(i), Some(b' ') | Some(b'\t')) {
                i += 1;
            }
        }
        _ => return false,
    }
    if !(take(&mut i, 1, 2) && b.get(i) == Some(&b':')) {
        return false;
    }
    i += 1;
    if !(take(&mut i, 2, 2) && b.get(i) == Some(&b':')) {
        return false;
    }
    i += 1;
    if !take(&mut i, 2, 2) {
        return false;
    }
    if b.get(i) == Some(&b'.') {
        i += 1 + digits(b, i + 1, |c| c.is_ascii_digit());
    }
    if i == b.len() {
        return true;
    }
    while matches!(b.get(i), Some(b' ') | Some(b'\t')) {
        i += 1;
    }
    match b.get(i) {
        Some(b'Z') => i + 1 == b.len(),
        Some(b'-') | Some(b'+') => {
            i += 1;
            if !take(&mut i, 1, 2) {
                return false;
            }
            if b.get(i) == Some(&b':') {
                i += 1;
                if !take(&mut i, 2, 2) {
                    return false;
                }
            }
            i == b.len()
        }
        _ => false,
    }
}

/// js-yaml's DEPRECATED_BASE60_SYNTAX: [-+]?[0-9_]+(:[0-9_]+)+(\.[0-9_]*)?
fn base60(s: &str) -> bool {
    let b = s.as_bytes();
    let num = |c: u8| c.is_ascii_digit() || c == b'_';
    let mut i = usize::from(matches!(b.first(), Some(b'-') | Some(b'+')));
    let n = digits(b, i, num);
    if n == 0 {
        return false;
    }
    i += n;
    let mut groups = 0;
    while b.get(i) == Some(&b':') {
        let n = digits(b, i + 1, num);
        if n == 0 {
            return false;
        }
        i += 1 + n;
        groups += 1;
    }
    if groups == 0 {
        return false;
    }
    if b.get(i) == Some(&b'.') {
        i += 1 + digits(b, i + 1, num);
    }
    i == b.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    include!("yaml_cases.rs");

    /// (js-yaml `dump(v, { forceQuotes, quotingType: '"' })`, js-yaml `dump(v)`) for the same value.
    #[test]
    fn writes_what_js_yaml_writes() {
        for (input, want) in CASES {
            let y = parse(input).unwrap();
            assert_eq!(dump(&y), *want, "from {input:?}");
            assert_eq!(dump(&parse(want).unwrap()), *want, "again {want:?}");
        }
    }

    /// Every content file of the real site (read only): written back by Pomelo exactly as js-yaml
    /// (the site's own, run with Node) writes it after reading it.
    /// `cargo test real_yaml_like_js_yaml -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_yaml_like_js_yaml() {
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
        let site = home.join("Documents/GitHub/personal-website");
        let node = crate::site::tools::node_bin(&site, &home).unwrap().join("node");
        let mut files = vec![];
        for dir in ["src/content/wander", "src/content/now", "src/content/about", "src/content/career"] {
            for e in std::fs::read_dir(site.join(dir)).unwrap() {
                files.push(e.unwrap().path());
            }
        }
        files.sort();
        let (mut same, mut changed) = (0, 0);
        for f in &files {
            let text = std::fs::read_to_string(f).unwrap();
            let (front, _) = split_front(&text).unwrap();
            let script = "const y=require(process.argv[1]);let s='';process.stdin.on('data',d=>s+=d).on('end',()=>process.stdout.write(y.dump(y.load(s))))";
            let js = std::process::Command::new(&node).args(["-e", script, &site.join("node_modules/js-yaml").to_string_lossy()]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().map(|mut c| {
                use std::io::Write;
                c.stdin.take().unwrap().write_all(front.as_bytes()).unwrap();
                String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap()
            }).unwrap();
            let ours = dump(&parse(front).unwrap());
            assert_eq!(ours, js, "{}", f.display());
            if ours == front { same += 1 } else { changed += 1 }
        }
        println!("{} 个文件和 js-yaml 一样；其中 {same} 个原样不变，{changed} 个是旧写法（保存时会按 js-yaml 现在的写法写）", files.len());
    }

    #[test]
    fn front_matter() {
        assert_eq!(split_front("---\ntitle: a\n---\nbody\n"), Some(("title: a\n", "body\n")));
        assert_eq!(split_front("---\ntitle: a\n---\n"), Some(("title: a\n", "")));
        assert_eq!(split_front("---\ntitle: a\n---"), Some(("title: a\n", "")));
        assert_eq!(split_front("---\n---\nbody"), Some(("", "body")));
        assert_eq!(split_front("no front"), None);
        let y = parse("title: a\ntopics: []\n").unwrap();
        assert_eq!(join_front(&y, "body\n"), "---\ntitle: a\ntopics: []\n---\nbody\n");
        assert_eq!(parse("").unwrap(), Y::Map(vec![]));
        assert!(parse("a: [").is_err());
    }

    #[test]
    fn keys_keep_their_place() {
        let mut y = parse("title: a\nlanguage: zh\nmonth: 2025-05\n").unwrap();
        y.set("category", Y::Str("notes".into()), &["month", "summary"]);
        y.set("title", Y::Str("b".into()), &[]);
        y.set("gallery", Y::Seq(vec![]), &[]);
        y.remove("language");
        assert_eq!(dump(&y), "title: b\ncategory: notes\nmonth: 2025-05\ngallery: []\n");
        assert_eq!(y.get("month").and_then(Y::text).as_deref(), Some("2025-05"));
        assert_eq!(parse("year: 2026").unwrap().get("year").and_then(Y::text).as_deref(), Some("2026"));
    }

    #[test]
    fn implicit_types() {
        for s in ["0", "-0", "12", "0x1F", "0b1", "0o7", "1.5", ".5", "1e3", "1.e3", "-.inf", ".NaN", "2025-05-01", "2025-5-1 1:00:00", "2025-05-01T10:00:00.5Z", "2025-05-01 10:00:00 +08:00", "null", "~", "<<"] {
            assert!(ambiguous(s), "{s}");
        }
        for s in ["2025-05", "1_000", "0x", "+", "e3", "1.5.2", "2025-05-01 10:00", "Null value", "--1", "1e999"] {
            assert!(!ambiguous(s), "{s}");
        }
        assert!(base60("1:20") && base60("-1_0:2.5") && !base60("1:") && !base60("12"));
    }
}
