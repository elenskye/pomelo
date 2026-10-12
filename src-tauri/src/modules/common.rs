//! Shared domain helpers: string enums, unified status, field validation, patch deserialization.

use rusqlite::{params, Connection};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Closed set of codes stored as TEXT (validated here, not by SQL CHECK, so adding a value
/// needs no table rebuild). Generates serde (code strings), Default, and rusqlite To/FromSql.
/// `str_enum!(Name, default = Variant, { Variant => "code", ... });`
macro_rules! str_enum {
    ($name:ident, default = $def:ident, { $($variant:ident => $code:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        pub enum $name {
            $(#[serde(rename = $code)] $variant),+
        }
        impl Default for $name {
            fn default() -> Self {
                Self::$def
            }
        }
        impl $name {
            #[allow(dead_code)]
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $code),+
                }
            }
            pub fn parse(s: &str) -> Option<Self> {
                match s {
                    $($code => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
        impl rusqlite::types::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(rusqlite::types::ToSqlOutput::from(self.as_str()))
            }
        }
        impl rusqlite::types::FromSql for $name {
            fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                let s = v.as_str()?;
                Self::parse(s).ok_or_else(|| {
                    rusqlite::types::FromSqlError::Other(format!("unknown {} {s}", stringify!($name)).into())
                })
            }
        }
    };
}
#[allow(unused_imports)] // for future fixed-code enums in other modules
pub(crate) use str_enum;

// Unified status for books / films / drawing lessons. UI maps to module-specific labels.
str_enum!(Status, default = Unmarked, {
    Unmarked => "unmarked",
    Todo => "todo",
    Doing => "doing",
    Done => "done",
});

/// Shared rule: becoming `done` with an empty 完成时间 fills the current one — month `YYYY-MM`
/// (`len` 7, 随意画画) or year `YYYY` (`len` 4, 书 / 影) — unless the caller set it explicitly
/// (their value, even null, wins).
pub fn auto_finished(
    conn: &Connection,
    status: Status,
    was_done: bool,
    when: &mut Option<String>,
    given: bool,
    len: usize,
) -> Result<(), String> {
    if status == Status::Done && !was_done && when.is_none() && !given {
        *when = Some(current_month(conn)?[..len].to_string());
    }
    Ok(())
}

/// For `Option<Option<T>>` patch fields: absent → None, null → Some(None), value → Some(Some(v)).
/// Use with `#[serde(default, deserialize_with = "some")]`.
pub fn some<'de, T: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

pub fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Nestable transaction (a SAVEPOINT): module functions call each other (投石问路 → 一步一步) and
/// run inside an import's transaction, where a plain BEGIN would fail. Rolls back unless committed.
pub struct Tx<'c> {
    conn: &'c Connection,
    open: bool,
}

impl<'c> Tx<'c> {
    pub fn begin(conn: &'c Connection) -> Result<Self, String> {
        conn.execute_batch("SAVEPOINT pomelo_tx").map_err(err)?;
        Ok(Tx { conn, open: true })
    }

    pub fn commit(mut self) -> Result<(), String> {
        self.open = false;
        self.conn.execute_batch("RELEASE pomelo_tx").map_err(err)
    }
}

impl std::ops::Deref for Tx<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn
    }
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        if self.open {
            let _ = self.conn.execute_batch("ROLLBACK TO pomelo_tx; RELEASE pomelo_tx");
        }
    }
}

/// Trim; empty → None.
pub fn clean(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn require(s: &str, label: &str, max_chars: usize) -> Result<String, String> {
    let v = s.trim();
    if v.is_empty() {
        return Err(format!("{label}不能为空"));
    }
    if v.chars().count() > max_chars {
        return Err(format!("{label}太长了（最多 {max_chars} 个字）"));
    }
    Ok(v.to_string())
}

pub fn check_len(s: &Option<String>, label: &str, max_chars: usize) -> Result<(), String> {
    match s {
        Some(v) if v.chars().count() > max_chars => Err(format!("{label}太长了（最多 {max_chars} 个字）")),
        _ => Ok(()),
    }
}

pub fn check_rating(r: Option<i64>) -> Result<(), String> {
    match r {
        Some(n) if !(1..=5).contains(&n) => Err("评分只能是 1～5 星".into()),
        _ => Ok(()),
    }
}

/// `YYYY-MM`, year 1900–2100.
pub fn check_month(m: &Option<String>) -> Result<(), String> {
    let Some(m) = m else { return Ok(()) };
    let ok = m.len() == 7
        && m.as_bytes()[4] == b'-'
        && m[..4].parse::<u32>().is_ok_and(|y| (1900..=2100).contains(&y))
        && m[5..].parse::<u32>().is_ok_and(|mo| (1..=12).contains(&mo));
    if ok {
        Ok(())
    } else {
        Err(format!("完成时间格式不对：{m}（应为 2026-09 这样的年月）"))
    }
}

/// `YYYY-MM-DD`, a real calendar date, year 1900–2100.
pub fn check_year(y: &Option<String>) -> Result<(), String> {
    let Some(y) = y else { return Ok(()) };
    if y.len() == 4 && y.parse::<u32>().is_ok_and(|y| (1900..=2100).contains(&y)) {
        Ok(())
    } else {
        Err(format!("完成时间格式不对：{y}（应为 2026 这样的年份）"))
    }
}

/// 重读 / 重看 (books.rs, films.rs): a later time — its year and rating. Stored in `<table>` rows keyed
/// by `<fk>`, in list order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reread {
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub rating: Option<i64>,
}

pub const MAX_REREADS: usize = 50;

pub fn clean_rereads(rs: Vec<Reread>) -> Result<Vec<Reread>, String> {
    if rs.len() > MAX_REREADS {
        return Err(format!("最多记 {MAX_REREADS} 次"));
    }
    rs.into_iter()
        .map(|r| {
            let year = year_of(r.year);
            check_year(&year)?;
            check_rating(r.rating)?;
            Ok(Reread { year, rating: r.rating })
        })
        .collect()
}

/// All rows (`id` = None) or one item's, by item id.
pub fn load_rereads(conn: &Connection, table: &str, fk: &str, id: Option<i64>) -> Result<HashMap<i64, Vec<Reread>>, String> {
    let mut st = conn
        .prepare(&format!("SELECT {fk}, year, rating FROM {table} WHERE ?1 IS NULL OR {fk} = ?1 ORDER BY {fk}, sort_order, id"))
        .map_err(err)?;
    let rows = st.query_map([id], |r| Ok((r.get::<_, i64>(0)?, Reread { year: r.get(1)?, rating: r.get(2)? }))).map_err(err)?;
    let mut out: HashMap<i64, Vec<Reread>> = HashMap::new();
    for row in rows {
        let (k, r) = row.map_err(err)?;
        out.entry(k).or_default().push(r);
    }
    Ok(out)
}

/// Replaces one item's list (already cleaned).
pub fn save_rereads(conn: &Connection, table: &str, fk: &str, id: i64, rs: &[Reread]) -> Result<(), String> {
    conn.execute(&format!("DELETE FROM {table} WHERE {fk} = ?1"), [id]).map_err(err)?;
    for (i, r) in rs.iter().enumerate() {
        conn.execute(&format!("INSERT INTO {table} ({fk}, year, rating, sort_order) VALUES (?1, ?2, ?3, ?4)"), params![id, r.year, r.rating, i as i64 + 1])
            .map_err(err)?;
    }
    Ok(())
}

/// Old files wrote 完成时间 as `YYYY-MM`; a year-only field keeps the year.
pub fn year_of(y: Option<String>) -> Option<String> {
    clean(y).map(|y| if y.len() == 7 && y.as_bytes()[4] == b'-' { y[..4].to_string() } else { y })
}

pub fn check_date(d: &Option<String>, label: &str) -> Result<(), String> {
    let Some(d) = d else { return Ok(()) };
    let parts: Vec<u32> = d.split('-').filter_map(|p| p.parse().ok()).collect();
    let ok = d.len() == 10
        && parts.len() == 3
        && (1900..=2100).contains(&parts[0])
        && (1..=12).contains(&parts[1])
        && {
            let (y, m, day) = (parts[0], parts[1], parts[2]);
            let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
            let max = match m {
                2 if leap => 29,
                2 => 28,
                4 | 6 | 9 | 11 => 30,
                _ => 31,
            };
            (1..=max).contains(&day)
        };
    if ok {
        Ok(())
    } else {
        Err(format!("{label}格式不对：{d}（应为 2026-10-01 这样的日期）"))
    }
}

// ---------- civil dates ("YYYY-MM-DD", proleptic Gregorian; H. Hinnant's algorithms) ----------

/// Days since 1970-01-01. Input must already be a valid date (`check_date`).
pub fn day_number(d: &str) -> i64 {
    let (y, m, dd) = (d[..4].parse::<i64>().unwrap(), d[5..7].parse::<i64>().unwrap(), d[8..10].parse::<i64>().unwrap());
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + dd - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn from_day_number(n: i64) -> String {
    let z = n + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn add_days(d: &str, n: i64) -> String {
    from_day_number(day_number(d) + n)
}

/// Monday = 1 … Sunday = 7.
pub fn weekday(d: &str) -> u32 {
    ((day_number(d) + 3).rem_euclid(7) + 1) as u32 // 1970-01-01 was a Thursday
}

/// `d` moved by `n` months, the day kept or clamped to the month's end.
pub fn add_months(d: &str, n: i64) -> String {
    let (y, m, day) = (d[..4].parse::<i64>().unwrap(), d[5..7].parse::<i64>().unwrap(), d[8..10].parse::<u32>().unwrap());
    let k = y * 12 + (m - 1) + n;
    let (y, m) = (k.div_euclid(12), (k.rem_euclid(12) + 1) as u32);
    format!("{y:04}-{m:02}-{:02}", day.min(days_in_month(y, m)))
}

pub fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `scope` = WHERE clause of one list. `ids` = some rows of the scope (e.g. the visible ones: undone tasks, a filtered list) in their new
/// order; they swap among the slots they already hold and the other rows stay put.
pub fn reorder(conn: &Connection, table: &str, scope: &str, args: &[&dyn rusqlite::ToSql], ids: &[i64]) -> Result<(), String> {
    let mut st = conn.prepare(&format!("SELECT id FROM {table} WHERE {scope} ORDER BY sort_order, id")).map_err(err)?;
    let mut order: Vec<i64> = st.query_map(args, |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let wanted: std::collections::HashSet<i64> = ids.iter().copied().collect();
    if wanted.len() != ids.len() || !ids.iter().all(|id| order.contains(id)) {
        return Err("列表已经变了，请刷新后再拖".into());
    }
    let mut next = ids.iter();
    for slot in order.iter_mut().filter(|id| wanted.contains(id)) {
        *slot = *next.next().unwrap();
    }
    let tx = Tx::begin(conn)?;
    for (i, id) in order.iter().enumerate() {
        tx.execute(&format!("UPDATE {table} SET sort_order = ?1 WHERE id = ?2"), rusqlite::params![i as i64 + 1, id])
            .map_err(err)?;
    }
    tx.commit().map_err(err)
}

pub fn current_month(conn: &Connection) -> Result<String, String> {
    conn.query_row("SELECT strftime('%Y-%m', 'now', 'localtime')", [], |r| r.get(0))
        .map_err(|e| e.to_string())
}

#[derive(Debug, PartialEq)]
pub enum LinkTarget {
    Url(String),
    Path(std::path::PathBuf),
}

/// What a stored link opens: a URL with any scheme (https:, mailto:, bear:…), a local path
/// (`/…` or `~/…`), or a bare address like `www.x.com` (→ https).
pub fn link_target(link: &str, home: &std::path::Path) -> Result<LinkTarget, String> {
    let s = link.trim();
    if s.is_empty() {
        return Err("没有链接".into());
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return Ok(LinkTarget::Path(home.join(rest)));
    }
    if s.starts_with('/') {
        return Ok(LinkTarget::Path(s.into()));
    }
    // `host:8080` is not a scheme
    let has_scheme = s.split_once(':').is_some_and(|(a, rest)| {
        a.len() > 1
            && a.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && a.chars().all(|c| c.is_ascii_alphanumeric() || "+-".contains(c))
            && !rest.starts_with(|c: char| c.is_ascii_digit())
    });
    Ok(LinkTarget::Url(if has_scheme { s.to_string() } else { format!("https://{s}") }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_validation() {
        for ok in [None, Some("2026-09"), Some("1990-01"), Some("2100-12")] {
            assert!(check_month(&ok.map(String::from)).is_ok(), "{ok:?}");
        }
        for bad in ["2026-9", "2026-13", "2026-00", "26-09-01", "abcd-ef", "1800-01", "2026/09"] {
            assert!(check_month(&Some(bad.into())).is_err(), "{bad}");
        }
    }

    #[test]
    fn date_validation() {
        for ok in [None, Some("2026-10-01"), Some("2024-02-29"), Some("2026-12-31")] {
            assert!(check_date(&ok.map(String::from), "日期").is_ok(), "{ok:?}");
        }
        for bad in ["2026-02-29", "2026-04-31", "2026-13-01", "2026-1-01", "26-10-01", "2026/10/01", "abc"] {
            assert!(check_date(&Some(bad.into()), "日期").is_err(), "{bad}");
        }
    }

    #[test]
    fn civil_dates() {
        for d in ["1970-01-01", "2000-02-29", "2026-09-30", "2027-01-01", "1900-03-01", "2100-12-31"] {
            assert_eq!(from_day_number(day_number(d)), d);
        }
        assert_eq!(day_number("1970-01-01"), 0);
        assert_eq!(add_days("2026-12-31", 1), "2027-01-01");
        assert_eq!(add_days("2024-03-01", -1), "2024-02-29");
        assert_eq!((weekday("2026-09-30"), weekday("2026-10-03"), weekday("2026-10-04")), (3, 6, 7));
        assert_eq!((days_in_month(2024, 2), days_in_month(2026, 2), days_in_month(2026, 9)), (29, 28, 30));
    }

    #[test]
    fn link_targets() {
        let home = std::path::Path::new("/Users/me");
        let url = |s: &str| Ok(LinkTarget::Url(s.into()));
        assert_eq!(link_target(" https://a.com/x?y=1 ", home), url("https://a.com/x?y=1"));
        assert_eq!(link_target("www.a.com/x", home), url("https://www.a.com/x"));
        assert_eq!(link_target("a.com:8080/x", home), url("https://a.com:8080/x"));
        assert_eq!(link_target("localhost:3000", home), url("https://localhost:3000"));
        assert_eq!(link_target("bear://x-callback-url/open-note?id=1", home), url("bear://x-callback-url/open-note?id=1"));
        assert_eq!(link_target("mailto:a@b.c", home), url("mailto:a@b.c"));
        assert_eq!(link_target("~/Documents/Seeker", home), Ok(LinkTarget::Path("/Users/me/Documents/Seeker".into())));
        assert_eq!(link_target("/tmp/a.pdf", home), Ok(LinkTarget::Path("/tmp/a.pdf".into())));
        assert!(link_target("  ", home).is_err());
    }

    #[test]
    fn reorder_part_of_a_list() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, k INTEGER, sort_order INTEGER); INSERT INTO t VALUES (1,0,1),(2,0,2),(3,0,3),(4,0,4),(5,1,1);")
            .unwrap();
        let order = |c: &Connection| -> Vec<i64> {
            c.prepare("SELECT id FROM t WHERE k = 0 ORDER BY sort_order").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        reorder(&c, "t", "k = ?1", &[&0], &[4, 2, 1, 3]).unwrap();
        assert_eq!(order(&c), [4, 2, 1, 3]);
        reorder(&c, "t", "k = ?1", &[&0], &[3, 4]).unwrap(); // 2 and 1 (e.g. done / filtered out) keep their slots
        assert_eq!(order(&c), [3, 2, 1, 4]);
        assert!(reorder(&c, "t", "k = ?1", &[&0], &[1, 5]).is_err()); // 5 is in another list
        assert!(reorder(&c, "t", "k = ?1", &[&0], &[1, 1]).is_err());
    }

    #[test]
    fn text_helpers() {
        assert_eq!(clean(Some("  a ".into())), Some("a".into()));
        assert_eq!(clean(Some("   ".into())), None);
        assert!(require("  ", "书名", 10).is_err());
        assert!(require("一二三", "书名", 2).is_err());
        assert!(check_rating(Some(0)).is_err() && check_rating(Some(6)).is_err());
        assert!(check_rating(Some(5)).is_ok() && check_rating(None).is_ok());
    }
}
