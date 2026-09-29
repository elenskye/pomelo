//! Shared domain helpers: string enums, unified status, field validation, patch deserialization.

use rusqlite::Connection;
use serde::{Deserialize, Deserializer};

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

/// Shared rule: becoming `done` with an empty month fills the current month, unless the
/// caller set the month explicitly (their value, even null, wins).
pub fn auto_month(
    conn: &Connection,
    status: Status,
    was_done: bool,
    month: &mut Option<String>,
    month_given: bool,
) -> Result<(), String> {
    if status == Status::Done && !was_done && month.is_none() && !month_given {
        *month = Some(current_month(conn)?);
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

pub fn current_month(conn: &Connection) -> Result<String, String> {
    conn.query_row("SELECT strftime('%Y-%m', 'now', 'localtime')", [], |r| r.get(0))
        .map_err(|e| e.to_string())
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
    fn text_helpers() {
        assert_eq!(clean(Some("  a ".into())), Some("a".into()));
        assert_eq!(clean(Some("   ".into())), None);
        assert!(require("  ", "书名", 10).is_err());
        assert!(require("一二三", "书名", 2).is_err());
        assert!(check_rating(Some(0)).is_err() && check_rating(Some(6)).is_err());
        assert!(check_rating(Some(5)).is_ok() && check_rating(None).is_ok());
    }
}
