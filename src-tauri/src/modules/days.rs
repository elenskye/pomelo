//! 来日方长 (v0.4.5, after Days Matter): days counted toward or since. Rules (all here):
//! - Kinds: `day` 纪念日和倒数日 (repeat none / year / month / week); `birthday` 亲友的生日和纪念日
//!   (every year; the year is optional → age; 礼物点子 text + 送过的, one line per gift); `document`
//!   证件 (a name and the expiry date only — never numbers or photos; no repeat).
//! - Date: YYYY-MM-DD; a birthday without the year is --MM-DD. 02-29 falls on 02-28 in other years; a
//!   monthly day past a month's end on its last day.
//! - Derived from today (local): `next` = the occurrence counted to — for repeating ones the first on
//!   or after today (the date itself while it's still ahead), else the date. `days` = next − today:
//!   ≥ 0 还有 N 天 (0 今天), < 0 已经 N 天 (证件: 已过期 N 天). `nth` = 第几周年 (a yearly day whose
//!   date has passed) / 几岁 (a birthday with its year), at `next`.
//! - Remind: up to 5 lead days (0–366, largest first). `soon` (首页「快到的日子」): from the largest
//!   lead until the day; expired 证件 too, until changed. `today` (今日简报): a lead day or the day.
//! - Order: expired 证件 first (longest expired first), then by days left, past one-offs last (most
//!   recent first). Pinned: at most one (the big card); pinning one unpins the other, and pinning
//!   alone keeps 修改于.

use super::common::{self, err, some, str_enum};
use crate::AppState;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

pub const KEY: &str = "days";
pub const LABEL: &str = "来日方长";
pub const MAX_REMIND: usize = 5;
pub const MAX_GIFTS: usize = 100;
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

fn gone() -> String {
    "找不到这个日子，可能已经被删除了".into()
}

str_enum!(Kind, default = Day, {
    Day => "day",
    Birthday => "birthday",
    Document => "document",
});

str_enum!(Repeat, default = None, {
    None => "none",
    Year => "year",
    Month => "month",
    Week => "week",
});

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Day => "纪念日",
            Kind::Birthday => "生日",
            Kind::Document => "证件",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Gift {
    /// YYYY
    pub year: String,
    pub gift: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Day {
    pub id: i64,
    pub kind: Kind,
    pub name: String,
    pub date: String,
    pub repeat: Repeat,
    /// Lead days, largest first.
    pub remind: Vec<i64>,
    pub pinned: bool,
    pub note: Option<String>,
    pub ideas: Option<String>,
    /// Newest year first.
    pub gifts: Vec<Gift>,
    pub created_at: String,
    pub updated_at: String,
    /// Derived (see the module doc).
    pub next: String,
    pub days: i64,
    pub nth: Option<i64>,
    pub soon: bool,
    pub today: bool,
}

// ---------- dates ----------

/// (year if any, month, day) of a stored date.
fn parts(date: &str) -> (Option<i64>, u32, u32) {
    let (y, rest) = match date.strip_prefix("--") {
        Some(md) => (None, md),
        None => (date[..4].parse().ok(), &date[5..]),
    };
    (y, rest[..2].parse().unwrap_or(1), rest[3..5].parse().unwrap_or(1))
}

/// That month-day in year `y` (clamped to the month's end: 02-29 → 02-28).
fn on(y: i64, m: u32, d: u32) -> String {
    format!("{y:04}-{m:02}-{:02}", d.min(common::days_in_month(y, m)))
}

fn year_of(d: &str) -> i64 {
    d[..4].parse().unwrap_or(0)
}

/// (next, nth) from today.
fn occurrence(kind: Kind, date: &str, repeat: Repeat, today: &str) -> (String, Option<i64>) {
    let (y0, m, d) = parts(date);
    let ty = year_of(today);
    let yearly = |from: i64| {
        let this = on(from, m, d);
        if this.as_str() >= today {
            this
        } else {
            on(from + 1, m, d)
        }
    };
    match (kind, repeat) {
        (Kind::Birthday, _) => {
            let next = yearly(ty);
            let age = y0.map(|b| year_of(&next) - b).filter(|a| *a >= 0);
            (next, age)
        }
        (Kind::Day, Repeat::Year) if date < today => {
            let next = yearly(ty);
            (next.clone(), y0.map(|b| year_of(&next) - b))
        }
        (Kind::Day, Repeat::Month) if date < today => {
            let mut k = 0;
            loop {
                let first = common::add_months(&format!("{}-01", &today[..7]), k);
                let cand = on(year_of(&first), first[5..7].parse().unwrap_or(1), d);
                if cand.as_str() >= today {
                    break (cand, None);
                }
                k += 1;
            }
        }
        (Kind::Day, Repeat::Week) if date < today => {
            let ahead = (common::weekday(date) as i64 - common::weekday(today) as i64).rem_euclid(7);
            (common::add_days(today, ahead), None)
        }
        _ => (date.to_string(), None),
    }
}

/// Fills in the derived fields for `today`.
fn derive(mut x: Day, today: &str) -> Day {
    let (next, nth) = occurrence(x.kind, &x.date, x.repeat, today);
    x.days = common::day_number(&next) - common::day_number(today);
    x.next = next;
    x.nth = nth;
    let expired = x.kind == Kind::Document && x.days < 0;
    let lead = x.remind.first().copied();
    x.soon = expired || lead.is_some_and(|l| x.days >= 0 && x.days <= l);
    x.today = !x.remind.is_empty() && x.days >= 0 && (x.days == 0 || x.remind.contains(&x.days));
    x
}

/// The list's order key: expired 证件, then ahead, then past one-offs.
fn order(x: &Day) -> (u8, i64) {
    match (x.kind == Kind::Document, x.days < 0) {
        (true, true) => (0, x.days),
        (_, false) => (1, x.days),
        (false, true) => (2, -x.days),
    }
}

// ---------- reading ----------

const COLS: &str = "id, kind, name, date, repeat, remind, pinned, note, ideas, created_at, updated_at";

fn row(r: &Row) -> rusqlite::Result<Day> {
    let remind: String = r.get(5)?;
    Ok(Day {
        id: r.get(0)?,
        kind: r.get(1)?,
        name: r.get(2)?,
        date: r.get(3)?,
        repeat: r.get(4)?,
        remind: remind.split(',').filter_map(|s| s.trim().parse().ok()).collect(),
        pinned: r.get(6)?,
        note: r.get(7)?,
        ideas: r.get(8)?,
        gifts: vec![],
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        next: String::new(),
        days: 0,
        nth: None,
        soon: false,
        today: false,
    })
}

fn gifts(conn: &Connection) -> Result<HashMap<i64, Vec<Gift>>, String> {
    let mut st = conn.prepare("SELECT day_id, year, gift FROM day_gifts ORDER BY day_id, year DESC, sort_order, id").map_err(err)?;
    let mut out: HashMap<i64, Vec<Gift>> = HashMap::new();
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, Gift { year: r.get(1)?, gift: r.get(2)? }))).map_err(err)?;
    for x in rows {
        let (id, g) = x.map_err(err)?;
        out.entry(id).or_default().push(g);
    }
    Ok(out)
}

/// All, in the list's order.
pub fn list(conn: &Connection, today: &str) -> Result<Vec<Day>, String> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM days ORDER BY id")).map_err(err)?;
    let rows = st.query_map([], row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let mut gs = gifts(conn)?;
    let mut out: Vec<Day> = rows.into_iter().map(|mut x| {
        x.gifts = gs.remove(&x.id).unwrap_or_default();
        derive(x, today)
    }).collect();
    out.sort_by_key(|x| (order(x), x.id));
    Ok(out)
}

pub fn get(conn: &Connection, today: &str, id: i64) -> Result<Day, String> {
    list(conn, today)?.into_iter().find(|x| x.id == id).ok_or_else(gone)
}

/// 首页「快到的日子」, in the list's order.
pub fn soon(conn: &Connection, today: &str) -> Result<Vec<Day>, String> {
    Ok(list(conn, today)?.into_iter().filter(|x| x.soon).collect())
}

// ---------- writing ----------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GiftInput {
    pub year: String,
    pub gift: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DayPatch {
    #[serde(default)]
    pub kind: Option<Kind>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub repeat: Option<Repeat>,
    #[serde(default)]
    pub remind: Option<Vec<i64>>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub ideas: Option<Option<String>>,
    /// Replaces the list (birthdays only).
    #[serde(default)]
    pub gifts: Option<Vec<GiftInput>>,
}

/// What a row holds, checked.
#[derive(Debug, Clone, PartialEq)]
struct Fields {
    kind: Kind,
    name: String,
    date: String,
    repeat: Repeat,
    remind: Vec<i64>,
    note: Option<String>,
    ideas: Option<String>,
    gifts: Vec<Gift>,
}

fn check_date_of(kind: Kind, date: &str) -> Result<String, String> {
    let d = date.trim();
    if let Some(md) = d.strip_prefix("--") {
        if kind != Kind::Birthday {
            return Err("只有生日可以不记年份".into());
        }
        {
            // any year with a 02-29 checks the month-day
            return common::check_date(&Some(format!("2000-{md}")), "日期").map(|_| d.to_string()).map_err(|_| format!("日期格式不对：{d}"));
        }
    }
    common::check_date(&Some(d.to_string()), "日期").map(|_| d.to_string())
}

fn clean_remind(r: &[i64]) -> Result<Vec<i64>, String> {
    let mut v: Vec<i64> = r.to_vec();
    v.sort_unstable_by(|a, b| b.cmp(a));
    v.dedup();
    if v.len() > MAX_REMIND {
        return Err(format!("提醒最多设 {MAX_REMIND} 个"));
    }
    if v.iter().any(|d| !(0..=366).contains(d)) {
        return Err("提前的天数要在 0～366 之间".into());
    }
    Ok(v)
}

fn clean_gifts(gs: &[Gift]) -> Result<Vec<Gift>, String> {
    if gs.len() > MAX_GIFTS {
        return Err(format!("送过的最多记 {MAX_GIFTS} 条"));
    }
    let mut out = vec![];
    for g in gs {
        let gift = common::require(&g.gift, "送了什么", 100)?;
        let year = g.year.trim().to_string();
        common::check_year(&Some(year.clone()))?;
        out.push(Gift { year, gift });
    }
    out.sort_by(|a, b| b.year.cmp(&a.year)); // stable: same year keeps its order
    Ok(out)
}

impl Fields {
    fn checked(self) -> Result<Fields, String> {
        let kind = self.kind;
        let f = Fields {
            name: common::require(&self.name, "名称", 50)?,
            date: check_date_of(kind, &self.date)?,
            repeat: match kind {
                Kind::Day => self.repeat,
                Kind::Birthday => Repeat::Year,
                Kind::Document => Repeat::None,
            },
            remind: clean_remind(&self.remind)?,
            note: common::clean(self.note),
            ideas: if kind == Kind::Birthday { common::clean(self.ideas) } else { None },
            gifts: if kind == Kind::Birthday { clean_gifts(&self.gifts)? } else { vec![] },
            kind,
        };
        common::check_len(&f.note, "备注", 2000)?;
        common::check_len(&f.ideas, "礼物点子", 2000)?;
        Ok(f)
    }

    fn apply(self, p: &DayPatch) -> Fields {
        Fields {
            kind: p.kind.unwrap_or(self.kind),
            name: p.name.clone().unwrap_or(self.name),
            date: p.date.clone().unwrap_or(self.date),
            repeat: p.repeat.unwrap_or(self.repeat),
            remind: p.remind.clone().unwrap_or(self.remind),
            note: p.note.clone().unwrap_or(self.note),
            ideas: p.ideas.clone().unwrap_or(self.ideas),
            gifts: p.gifts.as_ref().map(|gs| gs.iter().map(|g| Gift { year: g.year.clone(), gift: g.gift.clone() }).collect()).unwrap_or(self.gifts),
        }
    }
}

fn remind_text(r: &[i64]) -> String {
    r.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

fn write_gifts(conn: &Connection, id: i64, gs: &[Gift]) -> Result<(), String> {
    conn.execute("DELETE FROM day_gifts WHERE day_id = ?1", [id]).map_err(err)?;
    for (i, g) in gs.iter().enumerate() {
        conn.execute("INSERT INTO day_gifts (day_id, year, gift, sort_order) VALUES (?1, ?2, ?3, ?4)", params![id, g.year, g.gift, i as i64 + 1]).map_err(err)?;
    }
    Ok(())
}

fn pin(conn: &Connection, id: i64, pinned: bool) -> Result<(), String> {
    if pinned {
        conn.execute("UPDATE days SET pinned = 0 WHERE pinned = 1 AND id != ?1", [id]).map_err(err)?;
    }
    conn.execute("UPDATE days SET pinned = ?1 WHERE id = ?2", params![pinned, id]).map_err(err)?;
    Ok(())
}

pub fn create(conn: &Connection, p: DayPatch, today: &str) -> Result<Day, String> {
    let base = Fields { kind: Kind::Day, name: String::new(), date: String::new(), repeat: Repeat::None, remind: vec![], note: None, ideas: None, gifts: vec![] };
    let f = base.apply(&p).checked()?;
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        "INSERT INTO days (kind, name, date, repeat, remind, note, ideas) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![f.kind, f.name, f.date, f.repeat, remind_text(&f.remind), f.note, f.ideas],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    write_gifts(&tx, id, &f.gifts)?;
    if p.pinned == Some(true) {
        pin(&tx, id, true)?;
    }
    tx.commit()?;
    get(conn, today, id)
}

pub fn update(conn: &Connection, id: i64, p: DayPatch, today: &str) -> Result<Day, String> {
    let old = get(conn, today, id)?;
    let was = Fields { kind: old.kind, name: old.name.clone(), date: old.date.clone(), repeat: old.repeat, remind: old.remind.clone(), note: old.note.clone(), ideas: old.ideas.clone(), gifts: old.gifts.clone() };
    let f = was.clone().apply(&p).checked()?;
    let tx = common::Tx::begin(conn)?;
    if f != was {
        tx.execute(
            &format!("UPDATE days SET kind = ?1, name = ?2, date = ?3, repeat = ?4, remind = ?5, note = ?6, ideas = ?7, updated_at = {NOW} WHERE id = ?8"),
            params![f.kind, f.name, f.date, f.repeat, remind_text(&f.remind), f.note, f.ideas, id],
        )
        .map_err(err)?;
        if f.gifts != was.gifts {
            write_gifts(&tx, id, &f.gifts)?;
        }
    }
    if let Some(pinned) = p.pinned.filter(|x| *x != old.pinned) {
        pin(&tx, id, pinned)?;
    }
    tx.commit()?;
    get(conn, today, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM days WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(gone()),
        _ => Ok(()),
    }
}

// ---------- import / export: [{ kind, name, date, repeat?, remind?, pinned?, note?, ideas?, gifts? }] ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DayRecord {
    pub kind: Kind,
    pub name: String,
    pub date: String,
    #[serde(default, skip_serializing_if = "is_none")]
    pub repeat: Repeat,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remind: Vec<i64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ideas: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gifts: Vec<Gift>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

fn is_none(r: &Repeat) -> bool {
    *r == Repeat::None
}

impl DayRecord {
    fn fields(&self) -> Result<Fields, String> {
        Fields { kind: self.kind, name: self.name.clone(), date: self.date.clone(), repeat: self.repeat, remind: self.remind.clone(), note: self.note.clone(), ideas: self.ideas.clone(), gifts: self.gifts.clone() }.checked()
    }
}

pub fn validate_records(rec: &[DayRecord]) -> Result<(), String> {
    if rec.iter().filter(|r| r.pinned).count() > 1 {
        return Err(format!("{LABEL}：只能置顶一个"));
    }
    for (i, r) in rec.iter().enumerate() {
        r.fields().map_err(|e| format!("{LABEL}第 {} 条（{}）：{e}", i + 1, r.name.trim()))?;
    }
    Ok(())
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM days", [], |r| r.get(0)).map_err(err)
}

/// Replaces all. Caller owns the transaction.
pub fn replace_all(conn: &Connection, rec: &[DayRecord]) -> Result<usize, String> {
    validate_records(rec)?;
    conn.execute("DELETE FROM days", []).map_err(err)?;
    for r in rec {
        let f = r.fields()?;
        conn.execute(
            &format!(
                "INSERT INTO days (kind, name, date, repeat, remind, pinned, note, ideas, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, coalesce(?9, {NOW}), coalesce(?10, ?9, {NOW}))"
            ),
            params![f.kind, f.name, f.date, f.repeat, remind_text(&f.remind), r.pinned, f.note, f.ideas, r.created_at, r.updated_at],
        )
        .map_err(err)?;
        write_gifts(conn, conn.last_insert_rowid(), &f.gifts)?;
    }
    Ok(rec.len())
}

/// Oldest first.
pub fn export(conn: &Connection) -> Result<Vec<DayRecord>, String> {
    let mut all = list(conn, &super::todo::today(conn)?)?;
    all.sort_by_key(|x| x.id);
    Ok(all
        .into_iter()
        .map(|x| DayRecord {
            kind: x.kind,
            name: x.name,
            date: x.date,
            repeat: x.repeat,
            remind: x.remind,
            pinned: x.pinned,
            note: x.note,
            ideas: x.ideas,
            gifts: x.gifts,
            created_at: Some(x.created_at),
            updated_at: Some(x.updated_at),
        })
        .collect())
}

// ---------- commands ----------

fn today(conn: &Connection) -> Result<String, String> {
    super::todo::today(conn)
}

#[tauri::command]
pub fn days_list(state: State<AppState>) -> Result<Vec<Day>, String> {
    state.with_db(|c| list(c, &today(c)?))
}
#[tauri::command]
pub fn day_create(state: State<AppState>, patch: DayPatch) -> Result<Day, String> {
    state.with_undo("添加日子", |c| create(c, patch, &today(c)?))
}
#[tauri::command]
pub fn day_update(state: State<AppState>, id: i64, patch: DayPatch) -> Result<Day, String> {
    let label = if patch.pinned.is_some() && patch.name.is_none() && patch.date.is_none() { "置顶" } else { "修改日子" };
    state.with_undo(label, |c| update(c, id, patch, &today(c)?))
}
#[tauri::command]
pub fn day_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除日子", |c| delete(c, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    fn dp(v: serde_json::Value) -> DayPatch {
        serde_json::from_value(v).unwrap()
    }
    const T: &str = "2026-10-17"; // a Saturday

    #[test]
    fn next_occurrence_days_and_nth() {
        let o = |kind, date: &str, repeat| occurrence(kind, date, repeat, T);
        // one-off ahead / past
        assert_eq!(o(Kind::Day, "2026-12-31", Repeat::None), ("2026-12-31".into(), None));
        assert_eq!(o(Kind::Day, "2024-08-01", Repeat::None), ("2024-08-01".into(), None));
        // yearly: this year's still ahead / passed; a first date still ahead isn't an anniversary yet
        assert_eq!(o(Kind::Day, "2020-11-02", Repeat::Year), ("2026-11-02".into(), Some(6)));
        assert_eq!(o(Kind::Day, "2020-05-20", Repeat::Year), ("2027-05-20".into(), Some(7)));
        assert_eq!(o(Kind::Day, "2026-10-17", Repeat::Year), ("2026-10-17".into(), None)); // today
        assert_eq!(o(Kind::Day, "2027-03-01", Repeat::Year), ("2027-03-01".into(), None));
        // 02-29 in other years: 02-28
        assert_eq!(occurrence(Kind::Day, "2024-02-29", Repeat::Year, "2027-01-01").0, "2027-02-28");
        assert_eq!(occurrence(Kind::Day, "2024-02-29", Repeat::Year, "2028-01-01").0, "2028-02-29");
        // monthly (31st → the month's end), weekly
        assert_eq!(o(Kind::Day, "2026-01-31", Repeat::Month), ("2026-10-31".into(), None));
        assert_eq!(occurrence(Kind::Day, "2026-01-31", Repeat::Month, "2026-11-05").0, "2026-11-30");
        assert_eq!(o(Kind::Day, "2026-10-05", Repeat::Month), ("2026-11-05".into(), None));
        assert_eq!(o(Kind::Day, "2026-10-14", Repeat::Week), ("2026-10-21".into(), None)); // a Wednesday
        assert_eq!(o(Kind::Day, "2026-10-10", Repeat::Week), ("2026-10-17".into(), None));
        // birthdays: age with the year, none without
        assert_eq!(o(Kind::Birthday, "1966-11-02", Repeat::None), ("2026-11-02".into(), Some(60)));
        assert_eq!(o(Kind::Birthday, "--03-08", Repeat::None), ("2027-03-08".into(), None));
        // 证件: the date
        assert_eq!(o(Kind::Document, "2026-10-29", Repeat::Year), ("2026-10-29".into(), None));
    }

    #[test]
    fn soon_today_and_order() {
        let c = memory_db();
        let mk = |v: serde_json::Value| create(&c, dp(v), T).unwrap();
        let passport = mk(json!({"kind": "document", "name": "护照", "date": "2026-10-29", "remind": [7, 30]}));
        let visa = mk(json!({"kind": "document", "name": "签证", "date": "2026-10-01"}));
        let mom = mk(json!({"kind": "birthday", "name": "妈妈", "date": "1966-11-02", "remind": [7], "ideas": " 围巾 ", "gifts": [{"year": "2024", "gift": "丝巾"}, {"year": "2025", "gift": "茶具"}]}));
        let came = mk(json!({"name": "来新加坡", "date": "2024-08-01"}));
        let exam = mk(json!({"name": "考试", "date": "2026-10-24", "remind": [7, 0]}));
        assert_eq!((passport.days, passport.soon, passport.today), (12, true, false));
        assert_eq!(passport.remind, [30, 7]);
        assert_eq!((visa.days, visa.soon), (-16, true)); // expired: stays on 首页
        assert_eq!((mom.days, mom.nth, mom.soon), (16, Some(60), false));
        assert_eq!(mom.gifts[0], Gift { year: "2025".into(), gift: "茶具".into() });
        assert_eq!(mom.ideas.as_deref(), Some("围巾"));
        assert_eq!((came.days, came.soon), (-807, false));
        assert_eq!((exam.days, exam.soon, exam.today), (7, true, true)); // a lead day
        let order: Vec<String> = list(&c, T).unwrap().into_iter().map(|x| x.name).collect();
        assert_eq!(order, ["签证", "考试", "护照", "妈妈", "来新加坡"]);
        assert_eq!(soon(&c, T).unwrap().into_iter().map(|x| x.name).collect::<Vec<_>>(), ["签证", "考试", "护照"]);
        // the day itself counts for the brief
        assert!(get(&c, "2026-10-24", exam.id).unwrap().today);
    }

    #[test]
    fn rules_pinning_and_kinds() {
        let c = memory_db();
        assert!(create(&c, dp(json!({"name": " ", "date": "2026-01-01"})), T).is_err());
        assert!(create(&c, dp(json!({"name": "x", "date": "--05-20"})), T).unwrap_err().contains("生日"));
        assert!(create(&c, dp(json!({"name": "x", "date": "2026-01-01", "remind": [1, 2, 3, 4, 5, 6]})), T).is_err());
        assert!(create(&c, dp(json!({"name": "x", "date": "2026-01-01", "remind": [400]})), T).is_err());
        assert!(create(&c, dp(json!({"kind": "birthday", "name": "x", "date": "--02-30"})), T).is_err());
        let b = create(&c, dp(json!({"kind": "birthday", "name": "小明", "date": "--02-29", "repeat": "week"})), T).unwrap();
        assert_eq!((b.repeat, b.date.as_str()), (Repeat::Year, "--02-29"));
        // 证件 never repeat; switching kind drops what only a birthday has
        let d = create(&c, dp(json!({"kind": "document", "name": "驾照", "date": "2030-01-01", "repeat": "year", "ideas": "x"})), T).unwrap();
        assert_eq!((d.repeat, d.ideas), (Repeat::None, None));
        let b2 = update(&c, b.id, dp(json!({"kind": "day", "date": "2020-02-29", "gifts": []})), T).unwrap();
        assert_eq!((b2.kind, b2.repeat, b2.ideas.as_deref()), (Kind::Day, Repeat::Year, None));
        // one pinned at a time; pinning alone keeps 修改于
        let p1 = update(&c, d.id, dp(json!({"pinned": true})), T).unwrap();
        assert_eq!(p1.updated_at, d.updated_at);
        update(&c, b.id, dp(json!({"pinned": true})), T).unwrap();
        let pins: Vec<i64> = list(&c, T).unwrap().into_iter().filter(|x| x.pinned).map(|x| x.id).collect();
        assert_eq!(pins, [b.id]);
        delete(&c, b.id).unwrap();
        assert!(get(&c, T, b.id).is_err() && delete(&c, b.id).is_err());
    }

    #[test]
    fn export_import_roundtrip() {
        let c = memory_db();
        create(&c, dp(json!({"kind": "birthday", "name": "妈妈", "date": "1966-11-02", "remind": [7], "pinned": true, "ideas": "围巾", "gifts": [{"year": "2025", "gift": "茶具"}]})), T).unwrap();
        create(&c, dp(json!({"name": "纪念日", "date": "2020-05-20", "repeat": "year", "note": "第一次见面"})), T).unwrap();
        let rec = export(&c).unwrap();
        let text = serde_json::to_string(&rec).unwrap();
        assert!(!text.contains("\"repeat\":\"none\"") && text.contains("\"pinned\":true"));
        let back: Vec<DayRecord> = serde_json::from_str(&text).unwrap();
        let b = memory_db();
        replace_all(&b, &back).unwrap();
        assert_eq!(serde_json::to_string(&export(&b).unwrap()).unwrap(), text);
        let two = [back[0].clone(), DayRecord { pinned: true, ..back[1].clone() }];
        assert!(validate_records(&two).unwrap_err().contains("置顶"));
    }
}
