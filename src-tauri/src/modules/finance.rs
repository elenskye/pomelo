//! 钱来钱往 (finance). Money is integer cents in the row's currency; currencies are never mixed
//! except for total assets, which converts each month's balances with that month's CNY rates.
//! - Accounts hold one currency each; balances (and a broker's holdings) are monthly snapshots.
//! - 收支 entries: flow out / in, amount, currency (= the account's, if one is set), day, 分类.
//! - 固定支出 (`fin_fixed`): on `next_day` an entry is posted and `next_day` moves on by the rule
//!   (`post_due`: on create / update, and from the daily sweep). Entries keep `fixed_id`.
//! - Budgets: monthly amount per 支出分类 + currency.
//! 分类 = kinds.rs modules "fin_out" (支出) / "fin_in" (收入).

use super::common::{self, err, some, str_enum};
use super::kinds;
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

pub const KEY: &str = "finance";
pub const LABEL: &str = "钱来钱往";
pub const OUT_KINDS: &str = "fin_out";
pub const IN_KINDS: &str = "fin_in";
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";
const MAX_CENTS: i64 = 1_000_000_000_000; // 100 亿

fn gone(what: &str) -> String {
    format!("找不到这个{what}，可能已经被删除了")
}

str_enum!(Currency, default = Cny, {
    Cny => "CNY",
    Sgd => "SGD",
    Hkd => "HKD",
    Usd => "USD",
});

str_enum!(AccountKind, default = Bank, {
    Bank => "bank",
    Wallet => "wallet",
    Broker => "broker",
});

str_enum!(Flow, default = Out, {
    Out => "out",
    In => "in",
});

impl Flow {
    fn kinds_module(self) -> &'static str {
        match self {
            Flow::Out => OUT_KINDS,
            Flow::In => IN_KINDS,
        }
    }
}

fn check_amount(n: i64, what: &str) -> Result<(), String> {
    if !(1..=MAX_CENTS).contains(&n) {
        return Err(format!("{what}要大于 0"));
    }
    Ok(())
}

fn check_month(m: &str) -> Result<(), String> {
    common::check_month(&Some(m.to_string()))
}

fn check_day(d: &str, what: &str) -> Result<(), String> {
    common::check_date(&Some(d.to_string()), what)
}

// ---------- accounts ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub kind: AccountKind,
    pub currency: Currency,
    pub closed: bool,
    pub note: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    /// Latest snapshot.
    pub last_month: Option<String>,
    pub last_amount: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<AccountKind>,
    #[serde(default)]
    pub currency: Option<Currency>,
    #[serde(default)]
    pub closed: Option<bool>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
}

const ACCOUNT_SQL: &str = "SELECT a.id, a.name, a.kind, a.currency, a.closed, a.note, a.sort_order, a.created_at, a.updated_at,
    b.month, b.amount
  FROM fin_accounts a
  LEFT JOIN fin_balances b ON b.id = (SELECT id FROM fin_balances WHERE account_id = a.id ORDER BY month DESC LIMIT 1)";

fn account_row(r: &Row) -> rusqlite::Result<Account> {
    Ok(Account {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        currency: r.get(3)?,
        closed: r.get(4)?,
        note: r.get(5)?,
        sort_order: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
        last_month: r.get(9)?,
        last_amount: r.get(10)?,
    })
}

pub fn accounts(conn: &Connection) -> Result<Vec<Account>, String> {
    let mut st = conn.prepare(&format!("{ACCOUNT_SQL} ORDER BY a.sort_order, a.id")).map_err(err)?;
    let rows = st.query_map([], account_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn account(conn: &Connection, id: i64) -> Result<Account, String> {
    conn.query_row(&format!("{ACCOUNT_SQL} WHERE a.id = ?1"), [id], account_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("账户"))
}

fn clean_account_name(conn: &Connection, name: &str, except: Option<i64>) -> Result<String, String> {
    let name = common::require(name, "账户名", 30)?;
    let dup: i64 = conn
        .query_row("SELECT count(*) FROM fin_accounts WHERE name = ?1 AND id != coalesce(?2, -1)", params![name, except], |r| r.get(0))
        .map_err(err)?;
    if dup > 0 {
        return Err(format!("已经有叫「{name}」的账户了"));
    }
    Ok(name)
}

pub fn create_account(conn: &Connection, p: AccountPatch) -> Result<Account, String> {
    let name = clean_account_name(conn, p.name.as_deref().unwrap_or_default(), None)?;
    let currency = p.currency.ok_or("请选择币种")?;
    let note = common::clean(p.note.flatten());
    common::check_len(&note, "备注", 1000)?;
    conn.execute(
        "INSERT INTO fin_accounts (name, kind, currency, closed, note, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, (SELECT coalesce(max(sort_order), 0) + 1 FROM fin_accounts))",
        params![name, p.kind.unwrap_or_default(), currency, p.closed.unwrap_or(false), note],
    )
    .map_err(err)?;
    account(conn, conn.last_insert_rowid())
}

fn account_used(conn: &Connection, id: i64) -> Result<bool, String> {
    let n: i64 = conn
        .query_row(
            "SELECT (SELECT count(*) FROM fin_balances WHERE account_id = ?1) + (SELECT count(*) FROM fin_entries WHERE account_id = ?1)
              + (SELECT count(*) FROM fin_fixed WHERE account_id = ?1) + (SELECT count(*) FROM fin_holdings WHERE account_id = ?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    Ok(n > 0)
}

pub fn update_account(conn: &Connection, id: i64, p: AccountPatch) -> Result<Account, String> {
    let old = account(conn, id)?;
    let name = match &p.name {
        Some(n) => clean_account_name(conn, n, Some(id))?,
        None => old.name.clone(),
    };
    let currency = p.currency.unwrap_or(old.currency);
    if currency != old.currency && account_used(conn, id)? {
        return Err("这个账户已经有记录了，不能改币种；可以新建一个账户".into());
    }
    let kind = p.kind.unwrap_or(old.kind);
    if old.kind == AccountKind::Broker && kind != AccountKind::Broker {
        let n: i64 = conn.query_row("SELECT count(*) FROM fin_holdings WHERE account_id = ?1", [id], |r| r.get(0)).map_err(err)?;
        if n > 0 {
            return Err("这个账户记过持仓，不能改成其他类型".into());
        }
    }
    let note = match p.note {
        Some(n) => common::clean(n),
        None => old.note.clone(),
    };
    common::check_len(&note, "备注", 1000)?;
    conn.execute(
        &format!("UPDATE fin_accounts SET name = ?1, kind = ?2, currency = ?3, closed = ?4, note = ?5, updated_at = {NOW} WHERE id = ?6"),
        params![name, kind, currency, p.closed.unwrap_or(old.closed), note, id],
    )
    .map_err(err)?;
    account(conn, id)
}

/// Deletes the account with its balances and holdings; entries and 固定支出 keep their rows
/// (account → 不填).
pub fn delete_account(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM fin_accounts WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(gone("账户")),
        _ => Ok(()),
    }
}

// ---------- balances, rates, total assets ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Balance {
    pub account_id: i64,
    pub month: String,
    pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rate {
    pub month: String,
    pub currency: Currency,
    pub rate: f64,
}

/// One month of total assets.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthTotal {
    pub month: String,
    /// Sum of balances × that month's rates, in CNY cents; None while a rate is missing.
    pub total_cny: Option<i64>,
    /// Currencies with balances but no rate that month.
    pub missing: Vec<Currency>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assets {
    pub balances: Vec<Balance>,
    pub rates: Vec<Rate>,
    /// Months with any balance, oldest first.
    pub totals: Vec<MonthTotal>,
}

pub fn assets(conn: &Connection) -> Result<Assets, String> {
    let mut st = conn.prepare("SELECT account_id, month, amount FROM fin_balances ORDER BY month, account_id").map_err(err)?;
    let balances: Vec<Balance> = st
        .query_map([], |r| Ok(Balance { account_id: r.get(0)?, month: r.get(1)?, amount: r.get(2)? }))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut st = conn.prepare("SELECT month, currency, rate FROM fin_rates ORDER BY month, currency").map_err(err)?;
    let rates: Vec<Rate> = st
        .query_map([], |r| Ok(Rate { month: r.get(0)?, currency: r.get(1)?, rate: r.get(2)? }))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let currency: HashMap<i64, Currency> = accounts(conn)?.into_iter().map(|a| (a.id, a.currency)).collect();
    let rate_of: HashMap<(String, &str), f64> = rates.iter().map(|r| ((r.month.clone(), r.currency.as_str()), r.rate)).collect();
    let mut totals: Vec<MonthTotal> = Vec::new();
    for b in &balances {
        if totals.last().is_none_or(|t| t.month != b.month) {
            totals.push(MonthTotal { month: b.month.clone(), total_cny: Some(0), missing: Vec::new() });
        }
        let t = totals.last_mut().unwrap();
        let c = currency[&b.account_id];
        let rate = if c == Currency::Cny { Some(1.0) } else { rate_of.get(&(b.month.clone(), c.as_str())).copied() };
        match rate {
            Some(r) => t.total_cny = t.total_cny.map(|s| s + (b.amount as f64 * r).round() as i64),
            None => {
                if !t.missing.contains(&c) {
                    t.missing.push(c);
                }
            }
        }
    }
    for t in &mut totals {
        if !t.missing.is_empty() {
            t.total_cny = None;
        }
    }
    Ok(Assets { balances, rates, totals })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BalanceInput {
    pub account_id: i64,
    /// None = no snapshot for this account this month.
    pub amount: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RateInput {
    pub currency: Currency,
    pub rate: Option<f64>,
}

fn check_rate(rate: f64) -> Result<(), String> {
    if !(rate.is_finite() && rate > 0.0 && rate < 10_000.0) {
        return Err("汇率要大于 0".into());
    }
    Ok(())
}

/// 更新余额: a month's balances and rates in one go (None clears that row).
pub fn save_month(conn: &Connection, month: &str, balances: &[BalanceInput], rates: &[RateInput]) -> Result<(), String> {
    check_month(month)?;
    let tx = conn.unchecked_transaction().map_err(err)?;
    for b in balances {
        account(&tx, b.account_id)?;
        match b.amount {
            Some(a) => {
                if a.abs() > MAX_CENTS {
                    return Err("余额太大了".into());
                }
                tx.execute(
                    &format!(
                        "INSERT INTO fin_balances (account_id, month, amount) VALUES (?1, ?2, ?3)
                         ON CONFLICT(account_id, month) DO UPDATE SET amount = excluded.amount, updated_at = {NOW}"
                    ),
                    params![b.account_id, month, a],
                )
                .map_err(err)?;
            }
            None => {
                tx.execute("DELETE FROM fin_balances WHERE account_id = ?1 AND month = ?2", params![b.account_id, month]).map_err(err)?;
            }
        }
    }
    for r in rates {
        if r.currency == Currency::Cny {
            continue;
        }
        match r.rate {
            Some(v) => {
                check_rate(v)?;
                tx.execute(
                    &format!(
                        "INSERT INTO fin_rates (month, currency, rate) VALUES (?1, ?2, ?3)
                         ON CONFLICT(month, currency) DO UPDATE SET rate = excluded.rate, updated_at = {NOW}"
                    ),
                    params![month, r.currency, v],
                )
                .map_err(err)?;
            }
            None => {
                tx.execute("DELETE FROM fin_rates WHERE month = ?1 AND currency = ?2", params![month, r.currency]).map_err(err)?;
            }
        }
    }
    tx.commit().map_err(err)
}

// ---------- holdings ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Holding {
    pub id: i64,
    pub account_id: i64,
    pub month: String,
    pub name: String,
    pub quantity: Option<f64>,
    pub value: i64,
}

/// All broker holdings, by account, month, then row order.
pub fn holdings(conn: &Connection) -> Result<Vec<Holding>, String> {
    let mut st = conn
        .prepare("SELECT id, account_id, month, name, quantity, value FROM fin_holdings ORDER BY account_id, month, sort_order, id")
        .map_err(err)?;
    let rows = st
        .query_map([], |r| {
            Ok(Holding { id: r.get(0)?, account_id: r.get(1)?, month: r.get(2)?, name: r.get(3)?, quantity: r.get(4)?, value: r.get(5)? })
        })
        .map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HoldingInput {
    pub name: String,
    #[serde(default)]
    pub quantity: Option<f64>,
    pub value: i64,
}

fn check_holding(h: &HoldingInput) -> Result<String, String> {
    let name = common::require(&h.name, "持仓名称", 50)?;
    if h.quantity.is_some_and(|q| !(q.is_finite() && q >= 0.0)) {
        return Err(format!("「{name}」的数量不对"));
    }
    if h.value.abs() > MAX_CENTS {
        return Err(format!("「{name}」的市值太大了"));
    }
    Ok(name)
}

/// Replaces one broker account's holdings for one month.
pub fn save_holdings(conn: &Connection, account_id: i64, month: &str, rows: &[HoldingInput]) -> Result<(), String> {
    check_month(month)?;
    if account(conn, account_id)?.kind != AccountKind::Broker {
        return Err("只有券商账户能记持仓".into());
    }
    let tx = conn.unchecked_transaction().map_err(err)?;
    tx.execute("DELETE FROM fin_holdings WHERE account_id = ?1 AND month = ?2", params![account_id, month]).map_err(err)?;
    for (i, h) in rows.iter().enumerate() {
        let name = check_holding(h)?;
        tx.execute(
            "INSERT INTO fin_holdings (account_id, month, name, quantity, value, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![account_id, month, name, h.quantity, h.value, i as i64 + 1],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)
}

// ---------- 收支 entries ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: i64,
    pub flow: Flow,
    pub amount: i64,
    pub currency: Currency,
    pub day: String,
    pub kind_id: Option<i64>,
    pub account_id: Option<i64>,
    pub fixed_id: Option<i64>,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryPatch {
    #[serde(default)]
    pub flow: Option<Flow>,
    #[serde(default)]
    pub amount: Option<i64>,
    #[serde(default)]
    pub currency: Option<Currency>,
    #[serde(default)]
    pub day: Option<String>,
    #[serde(default, deserialize_with = "some")]
    pub kind_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub account_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
}

const ENTRY_COLS: &str = "id, flow, amount, currency, day, kind_id, account_id, fixed_id, note, created_at, updated_at";

fn entry_row(r: &Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: r.get(0)?,
        flow: r.get(1)?,
        amount: r.get(2)?,
        currency: r.get(3)?,
        day: r.get(4)?,
        kind_id: r.get(5)?,
        account_id: r.get(6)?,
        fixed_id: r.get(7)?,
        note: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

/// One month (YYYY-MM), newest first.
pub fn entries(conn: &Connection, month: &str) -> Result<Vec<Entry>, String> {
    check_month(month)?;
    let mut st = conn
        .prepare(&format!("SELECT {ENTRY_COLS} FROM fin_entries WHERE substr(day, 1, 7) = ?1 ORDER BY day DESC, id DESC"))
        .map_err(err)?;
    let rows = st.query_map([month], entry_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn entry(conn: &Connection, id: i64) -> Result<Entry, String> {
    conn.query_row(&format!("SELECT {ENTRY_COLS} FROM fin_entries WHERE id = ?1"), [id], entry_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("记录"))
}

/// Shared by entries and 固定支出: the 分类 matches the flow, the currency matches the account.
fn check_money(conn: &Connection, flow: Flow, currency: Currency, kind_id: Option<i64>, account_id: Option<i64>) -> Result<(), String> {
    kinds::check(conn, flow.kinds_module(), kind_id).map_err(|_| "选择的分类不存在，或者不是这一类（支出 / 收入）的".to_string())?;
    if let Some(a) = account_id {
        let a = account(conn, a)?;
        if a.currency != currency {
            return Err(format!("「{}」是 {} 账户，币种要一致", a.name, a.currency.as_str()));
        }
    }
    Ok(())
}

struct EntryFields {
    flow: Flow,
    amount: i64,
    currency: Currency,
    day: String,
    kind_id: Option<i64>,
    account_id: Option<i64>,
    note: Option<String>,
}

impl EntryFields {
    fn from_patch(old: Option<&Entry>, p: EntryPatch) -> Result<Self, String> {
        let flow = p.flow.or(old.map(|e| e.flow)).unwrap_or_default();
        let amount = p.amount.or(old.map(|e| e.amount)).ok_or("请填金额")?;
        let currency = p.currency.or(old.map(|e| e.currency)).ok_or("请选择币种")?;
        let day = p.day.or(old.map(|e| e.day.clone())).ok_or("请选择日期")?;
        let kind_id = p.kind_id.unwrap_or(old.and_then(|e| e.kind_id));
        let account_id = p.account_id.unwrap_or(old.and_then(|e| e.account_id));
        let note = common::clean(p.note.unwrap_or(old.and_then(|e| e.note.clone())));
        Ok(Self { flow, amount, currency, day, kind_id, account_id, note })
    }

    fn check(&self, conn: &Connection) -> Result<(), String> {
        check_amount(self.amount, "金额")?;
        check_day(&self.day, "日期")?;
        common::check_len(&self.note, "备注", 1000)?;
        check_money(conn, self.flow, self.currency, self.kind_id, self.account_id)
    }
}

fn insert_entry(conn: &Connection, f: &EntryFields, fixed_id: Option<i64>) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO fin_entries (flow, amount, currency, day, kind_id, account_id, fixed_id, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![f.flow, f.amount, f.currency, f.day, f.kind_id, f.account_id, fixed_id, f.note],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

pub fn create_entry(conn: &Connection, p: EntryPatch) -> Result<Entry, String> {
    let f = EntryFields::from_patch(None, p)?;
    f.check(conn)?;
    let id = insert_entry(conn, &f, None)?;
    entry(conn, id)
}

/// 记一周小额 etc.: several entries in one step (all or nothing).
pub fn create_entries(conn: &Connection, patches: Vec<EntryPatch>) -> Result<usize, String> {
    if patches.is_empty() {
        return Err("没有要记的".into());
    }
    let n = patches.len();
    let tx = conn.unchecked_transaction().map_err(err)?;
    for p in patches {
        let f = EntryFields::from_patch(None, p)?;
        f.check(&tx)?;
        insert_entry(&tx, &f, None)?;
    }
    tx.commit().map_err(err)?;
    Ok(n)
}

pub fn update_entry(conn: &Connection, id: i64, p: EntryPatch) -> Result<Entry, String> {
    let old = entry(conn, id)?;
    let f = EntryFields::from_patch(Some(&old), p)?;
    f.check(conn)?;
    conn.execute(
        &format!(
            "UPDATE fin_entries SET flow = ?1, amount = ?2, currency = ?3, day = ?4, kind_id = ?5, account_id = ?6, note = ?7,
               updated_at = {NOW} WHERE id = ?8"
        ),
        params![f.flow, f.amount, f.currency, f.day, f.kind_id, f.account_id, f.note, id],
    )
    .map_err(err)?;
    entry(conn, id)
}

pub fn delete_entry(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM fin_entries WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(gone("记录")),
        _ => Ok(()),
    }
}

// ---------- 固定支出 ----------

/// `monthly:<1-31>` (past a month's end → its last day) | `yearly:<MM-DD>` (02-29 → 02-28 off leap years).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FixedRule {
    Monthly(u32),
    Yearly(u32, u32),
}

impl FixedRule {
    pub fn parse(s: &str) -> Result<Self, String> {
        let bad = || format!("周期不对：{s}");
        match s.split_once(':') {
            Some(("monthly", d)) => d.parse::<u32>().ok().filter(|d| (1..=31).contains(d)).map(FixedRule::Monthly).ok_or_else(bad),
            Some(("yearly", md)) => {
                let (m, d) = md.split_once('-').ok_or_else(bad)?;
                let (m, d) = (m.parse::<u32>().map_err(|_| bad())?, d.parse::<u32>().map_err(|_| bad())?);
                if !(1..=12).contains(&m) || d < 1 || d > common::days_in_month(2024, m) {
                    return Err(bad());
                }
                Ok(FixedRule::Yearly(m, d))
            }
            _ => Err(bad()),
        }
    }

    /// First occurrence on or after `day`.
    pub fn on_or_after(&self, day: &str) -> String {
        let (mut y, mut m) = (day[..4].parse::<i64>().unwrap(), day[5..7].parse::<u32>().unwrap());
        loop {
            let d = match *self {
                FixedRule::Monthly(d) => Some(format!("{y:04}-{m:02}-{:02}", d.min(common::days_in_month(y, m)))),
                FixedRule::Yearly(fm, fd) if fm == m => Some(format!("{y:04}-{m:02}-{:02}", fd.min(common::days_in_month(y, m)))),
                FixedRule::Yearly(..) => None,
            };
            if let Some(d) = d.filter(|d| d.as_str() >= day) {
                return d;
            }
            (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
        }
    }

    pub fn after(&self, day: &str) -> String {
        self.on_or_after(&common::add_days(day, 1))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fixed {
    pub id: i64,
    pub name: String,
    pub flow: Flow,
    pub amount: i64,
    pub currency: Currency,
    pub kind_id: Option<i64>,
    pub account_id: Option<i64>,
    pub rule: String,
    pub next_day: String,
    pub stopped: bool,
    pub note: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub flow: Option<Flow>,
    #[serde(default)]
    pub amount: Option<i64>,
    #[serde(default)]
    pub currency: Option<Currency>,
    #[serde(default, deserialize_with = "some")]
    pub kind_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub account_id: Option<Option<i64>>,
    #[serde(default)]
    pub rule: Option<String>,
    /// Next day to post; omitted on create / rule change = the rule's first day from today.
    #[serde(default)]
    pub next_day: Option<String>,
    #[serde(default)]
    pub stopped: Option<bool>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
}

const FIXED_COLS: &str =
    "id, name, flow, amount, currency, kind_id, account_id, rule, next_day, stopped, note, sort_order, created_at, updated_at";

fn fixed_row(r: &Row) -> rusqlite::Result<Fixed> {
    Ok(Fixed {
        id: r.get(0)?,
        name: r.get(1)?,
        flow: r.get(2)?,
        amount: r.get(3)?,
        currency: r.get(4)?,
        kind_id: r.get(5)?,
        account_id: r.get(6)?,
        rule: r.get(7)?,
        next_day: r.get(8)?,
        stopped: r.get(9)?,
        note: r.get(10)?,
        sort_order: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
    })
}

pub fn fixed_list(conn: &Connection) -> Result<Vec<Fixed>, String> {
    let mut st = conn.prepare(&format!("SELECT {FIXED_COLS} FROM fin_fixed ORDER BY sort_order, id")).map_err(err)?;
    let rows = st.query_map([], fixed_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn fixed(conn: &Connection, id: i64) -> Result<Fixed, String> {
    conn.query_row(&format!("SELECT {FIXED_COLS} FROM fin_fixed WHERE id = ?1"), [id], fixed_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| gone("固定支出"))
}

struct FixedFields {
    name: String,
    flow: Flow,
    amount: i64,
    currency: Currency,
    kind_id: Option<i64>,
    account_id: Option<i64>,
    rule: String,
    next_day: String,
    stopped: bool,
    note: Option<String>,
}

fn fixed_fields(conn: &Connection, old: Option<&Fixed>, p: FixedPatch, today: &str) -> Result<FixedFields, String> {
    let name = common::require(p.name.as_deref().or(old.map(|f| f.name.as_str())).unwrap_or_default(), "名称", 50)?;
    let dup: i64 = conn
        .query_row("SELECT count(*) FROM fin_fixed WHERE name = ?1 AND id != coalesce(?2, -1)", params![name, old.map(|f| f.id)], |r| r.get(0))
        .map_err(err)?;
    if dup > 0 {
        return Err(format!("已经有叫「{name}」的固定支出了"));
    }
    let rule_text = p.rule.clone().or(old.map(|f| f.rule.clone())).ok_or("请设置周期")?;
    let rule = FixedRule::parse(&rule_text)?;
    let next_day = match (&p.next_day, old) {
        (Some(d), _) => d.clone(),
        (None, Some(o)) if p.rule.as_deref().is_none_or(|r| r == o.rule) => o.next_day.clone(),
        _ => rule.on_or_after(today),
    };
    check_day(&next_day, "下次日期")?;
    let f = FixedFields {
        name,
        flow: p.flow.or(old.map(|f| f.flow)).unwrap_or_default(),
        amount: p.amount.or(old.map(|f| f.amount)).ok_or("请填金额")?,
        currency: p.currency.or(old.map(|f| f.currency)).ok_or("请选择币种")?,
        kind_id: p.kind_id.unwrap_or(old.and_then(|f| f.kind_id)),
        account_id: p.account_id.unwrap_or(old.and_then(|f| f.account_id)),
        rule: rule_text,
        next_day,
        stopped: p.stopped.or(old.map(|f| f.stopped)).unwrap_or(false),
        note: common::clean(p.note.unwrap_or(old.and_then(|f| f.note.clone()))),
    };
    check_amount(f.amount, "金额")?;
    common::check_len(&f.note, "备注", 1000)?;
    check_money(conn, f.flow, f.currency, f.kind_id, f.account_id)?;
    Ok(f)
}

/// Posts every running 固定支出 due on or before `today`, moving `next_day` past today.
pub fn post_due(conn: &Connection, today: &str) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(err)?;
    let n = post_due_in(&tx, today)?;
    tx.commit().map_err(err)?;
    Ok(n)
}

/// `post_due` inside the caller's transaction.
fn post_due_in(conn: &Connection, today: &str) -> Result<usize, String> {
    let mut st = conn
        .prepare(&format!("SELECT {FIXED_COLS} FROM fin_fixed WHERE stopped = 0 AND next_day <= ?1 ORDER BY sort_order, id"))
        .map_err(err)?;
    let due: Vec<Fixed> = st.query_map([today], fixed_row).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut posted = 0;
    for f in due {
        let rule = FixedRule::parse(&f.rule)?;
        let mut day = f.next_day.clone();
        for _ in 0..240 {
            if day.as_str() > today {
                break;
            }
            let e = EntryFields {
                flow: f.flow,
                amount: f.amount,
                currency: f.currency,
                day: day.clone(),
                kind_id: f.kind_id,
                account_id: f.account_id,
                note: Some(f.name.clone()),
            };
            insert_entry(conn, &e, Some(f.id))?;
            posted += 1;
            day = rule.after(&day);
        }
        conn.execute("UPDATE fin_fixed SET next_day = ?1 WHERE id = ?2", params![day, f.id]).map_err(err)?;
    }
    Ok(posted)
}

pub fn create_fixed(conn: &Connection, p: FixedPatch, today: &str) -> Result<Fixed, String> {
    let f = fixed_fields(conn, None, p, today)?;
    let tx = conn.unchecked_transaction().map_err(err)?;
    tx.execute(
        "INSERT INTO fin_fixed (name, flow, amount, currency, kind_id, account_id, rule, next_day, stopped, note, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, (SELECT coalesce(max(sort_order), 0) + 1 FROM fin_fixed))",
        params![f.name, f.flow, f.amount, f.currency, f.kind_id, f.account_id, f.rule, f.next_day, f.stopped, f.note],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    post_due_in(&tx, today)?;
    tx.commit().map_err(err)?;
    fixed(conn, id)
}

pub fn update_fixed(conn: &Connection, id: i64, p: FixedPatch, today: &str) -> Result<Fixed, String> {
    let old = fixed(conn, id)?;
    let f = fixed_fields(conn, Some(&old), p, today)?;
    let tx = conn.unchecked_transaction().map_err(err)?;
    tx.execute(
        &format!(
            "UPDATE fin_fixed SET name = ?1, flow = ?2, amount = ?3, currency = ?4, kind_id = ?5, account_id = ?6, rule = ?7,
               next_day = ?8, stopped = ?9, note = ?10, updated_at = {NOW} WHERE id = ?11"
        ),
        params![f.name, f.flow, f.amount, f.currency, f.kind_id, f.account_id, f.rule, f.next_day, f.stopped, f.note, id],
    )
    .map_err(err)?;
    post_due_in(&tx, today)?;
    tx.commit().map_err(err)?;
    fixed(conn, id)
}

/// Entries it already posted stay (as plain entries).
pub fn delete_fixed(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM fin_fixed WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(gone("固定支出")),
        _ => Ok(()),
    }
}

// ---------- budgets & reports ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    pub kind_id: i64,
    pub currency: Currency,
    pub amount: i64,
}

pub fn budgets(conn: &Connection) -> Result<Vec<Budget>, String> {
    let mut st = conn
        .prepare("SELECT b.kind_id, b.currency, b.amount FROM fin_budgets b JOIN kinds k ON k.id = b.kind_id ORDER BY k.sort_order, b.currency")
        .map_err(err)?;
    let rows = st.query_map([], |r| Ok(Budget { kind_id: r.get(0)?, currency: r.get(1)?, amount: r.get(2)? })).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetInput {
    pub kind_id: i64,
    /// None = no budget.
    pub amount: Option<i64>,
}

/// Sets one currency's monthly budgets (listed 支出分类 only; others unchanged).
pub fn save_budgets(conn: &Connection, currency: Currency, rows: &[BudgetInput]) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(err)?;
    for b in rows {
        kinds::check(&tx, OUT_KINDS, Some(b.kind_id)).map_err(|_| "预算只能按支出分类设".to_string())?;
        match b.amount {
            Some(a) => {
                check_amount(a, "预算")?;
                tx.execute(
                    &format!(
                        "INSERT INTO fin_budgets (kind_id, currency, amount) VALUES (?1, ?2, ?3)
                         ON CONFLICT(kind_id, currency) DO UPDATE SET amount = excluded.amount, updated_at = {NOW}"
                    ),
                    params![b.kind_id, currency, a],
                )
                .map_err(err)?;
            }
            None => {
                tx.execute("DELETE FROM fin_budgets WHERE kind_id = ?1 AND currency = ?2", params![b.kind_id, currency]).map_err(err)?;
            }
        }
    }
    tx.commit().map_err(err)
}

/// Entry sums by month × currency × flow × 分类 (reports pick what they need).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Total {
    pub month: String,
    pub currency: Currency,
    pub flow: Flow,
    pub kind_id: Option<i64>,
    pub amount: i64,
    pub count: i64,
}

pub fn totals(conn: &Connection) -> Result<Vec<Total>, String> {
    let mut st = conn
        .prepare(
            "SELECT substr(day, 1, 7) AS m, currency, flow, kind_id, sum(amount), count(*) FROM fin_entries
             GROUP BY m, currency, flow, kind_id ORDER BY m, currency, flow, kind_id",
        )
        .map_err(err)?;
    let rows = st
        .query_map([], |r| {
            Ok(Total { month: r.get(0)?, currency: r.get(1)?, flow: r.get(2)?, kind_id: r.get(3)?, amount: r.get(4)?, count: r.get(5)? })
        })
        .map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

// ---------- import / export: accounts (with balances, holdings), rates, fixed, entries, budgets ----------

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BalanceRecord {
    pub month: String,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HoldingRecord {
    pub month: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f64>,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountRecord {
    pub name: String,
    #[serde(default)]
    pub kind: AccountKind,
    pub currency: Currency,
    #[serde(default, skip_serializing_if = "is_false")]
    pub closed: bool,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub balances: Vec<BalanceRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holdings: Vec<HoldingRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RateRecord {
    pub month: String,
    pub currency: Currency,
    pub rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedRecord {
    pub name: String,
    #[serde(default)]
    pub flow: Flow,
    pub amount: i64,
    pub currency: Currency,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    pub rule: String,
    pub next_day: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stopped: bool,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryRecord {
    #[serde(default)]
    pub flow: Flow,
    pub amount: i64,
    pub currency: Currency,
    pub day: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    /// Name of the 固定支出 that posted it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetRecord {
    pub category: String,
    pub currency: Currency,
    pub amount: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FinanceRecord {
    #[serde(default)]
    pub accounts: Vec<AccountRecord>,
    #[serde(default)]
    pub rates: Vec<RateRecord>,
    #[serde(default)]
    pub fixed: Vec<FixedRecord>,
    #[serde(default)]
    pub entries: Vec<EntryRecord>,
    #[serde(default)]
    pub budgets: Vec<BudgetRecord>,
}

impl FinanceRecord {
    /// Records counted in import summaries: accounts, 固定支出, entries, budgets.
    pub fn len(&self) -> usize {
        self.accounts.len() + self.fixed.len() + self.entries.len() + self.budgets.len()
    }
}

pub fn validate_records(rec: &FinanceRecord) -> Result<(), String> {
    let e = |what: String| move |msg: String| format!("{LABEL}{what}：{msg}");
    let mut names = std::collections::HashSet::new();
    let mut currency: HashMap<String, Currency> = HashMap::new();
    for a in &rec.accounts {
        let name = common::require(&a.name, "账户名", 30).map_err(e("的账户".into()))?;
        if !names.insert(name.clone()) {
            return Err(format!("{LABEL}：账户「{name}」重复了"));
        }
        currency.insert(name.clone(), a.currency);
        let mut months = std::collections::HashSet::new();
        for b in &a.balances {
            check_month(&b.month).map_err(e(format!("「{name}」的余额")))?;
            if !months.insert(&b.month) {
                return Err(format!("{LABEL}「{name}」：{} 的余额重复了", b.month));
            }
        }
        for h in &a.holdings {
            check_month(&h.month).map_err(e(format!("「{name}」的持仓")))?;
            check_holding(&HoldingInput { name: h.name.clone(), quantity: h.quantity, value: h.value }).map_err(e(format!("「{name}」的持仓")))?;
        }
        if !a.holdings.is_empty() && a.kind != AccountKind::Broker {
            return Err(format!("{LABEL}「{name}」：只有券商账户能记持仓"));
        }
    }
    let mut rate_keys = std::collections::HashSet::new();
    for r in &rec.rates {
        check_month(&r.month).map_err(e("的汇率".into()))?;
        check_rate(r.rate).map_err(e("的汇率".into()))?;
        if !rate_keys.insert((r.month.clone(), r.currency.as_str())) {
            return Err(format!("{LABEL}：{} {} 的汇率重复了", r.month, r.currency.as_str()));
        }
    }
    let account_ok = |acc: &Option<String>, cur: Currency| -> Result<(), String> {
        match acc.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => Ok(()),
            Some(n) => match currency.get(n) {
                None => Err(format!("找不到账户「{n}」")),
                Some(c) if *c != cur => Err(format!("「{n}」是 {} 账户，币种要一致", c.as_str())),
                Some(_) => Ok(()),
            },
        }
    };
    let mut fixed_names = std::collections::HashSet::new();
    for f in &rec.fixed {
        let name = common::require(&f.name, "名称", 50).map_err(e("的固定支出".into()))?;
        if !fixed_names.insert(name.clone()) {
            return Err(format!("{LABEL}：固定支出「{name}」重复了"));
        }
        let ctx = e(format!("的固定支出「{name}」"));
        check_amount(f.amount, "金额").map_err(&ctx)?;
        FixedRule::parse(&f.rule).map_err(&ctx)?;
        check_day(&f.next_day, "下次日期").map_err(&ctx)?;
        kinds::check_name(&f.category).map_err(&ctx)?;
        account_ok(&f.account, f.currency).map_err(&ctx)?;
    }
    for x in &rec.entries {
        let ctx = e(format!("{} 的记录", x.day));
        check_amount(x.amount, "金额").map_err(&ctx)?;
        check_day(&x.day, "日期").map_err(&ctx)?;
        kinds::check_name(&x.category).map_err(&ctx)?;
        account_ok(&x.account, x.currency).map_err(&ctx)?;
        if let Some(f) = x.fixed.as_deref().filter(|f| !fixed_names.contains(f.trim())) {
            return Err(ctx(format!("找不到固定支出「{f}」")));
        }
    }
    let mut budget_keys = std::collections::HashSet::new();
    for b in &rec.budgets {
        let name = common::require(&b.category, "分类", 20).map_err(e("的预算".into()))?;
        check_amount(b.amount, "预算").map_err(e(format!("「{name}」的预算")))?;
        if !budget_keys.insert((name.clone(), b.currency.as_str())) {
            return Err(format!("{LABEL}：「{name}」{} 的预算重复了", b.currency.as_str()));
        }
    }
    Ok(())
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        "SELECT (SELECT count(*) FROM fin_accounts) + (SELECT count(*) FROM fin_fixed) + (SELECT count(*) FROM fin_entries)
           + (SELECT count(*) FROM fin_budgets)",
        [],
        |r| r.get(0),
    )
    .map_err(err)
}

/// Replaces all finance data (kinds lists are handled by transfer.rs). Caller owns the transaction.
pub fn replace_all(conn: &Connection, rec: &FinanceRecord) -> Result<usize, String> {
    validate_records(rec)?;
    for t in ["fin_entries", "fin_fixed", "fin_budgets", "fin_holdings", "fin_balances", "fin_rates", "fin_accounts"] {
        conn.execute(&format!("DELETE FROM {t}"), []).map_err(err)?;
    }
    let mut account_ids: HashMap<String, i64> = HashMap::new();
    for (i, a) in rec.accounts.iter().enumerate() {
        conn.execute(
            &format!(
                "INSERT INTO fin_accounts (name, kind, currency, closed, note, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, coalesce(?7, {NOW}), coalesce(?8, ?7, {NOW}))"
            ),
            params![a.name.trim(), a.kind, a.currency, a.closed, common::clean(a.note.clone()), i as i64 + 1, a.created_at, a.updated_at],
        )
        .map_err(err)?;
        let id = conn.last_insert_rowid();
        account_ids.insert(a.name.trim().to_string(), id);
        for b in &a.balances {
            conn.execute("INSERT INTO fin_balances (account_id, month, amount) VALUES (?1, ?2, ?3)", params![id, b.month, b.amount])
                .map_err(err)?;
        }
        for (j, h) in a.holdings.iter().enumerate() {
            conn.execute(
                "INSERT INTO fin_holdings (account_id, month, name, quantity, value, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, h.month, h.name.trim(), h.quantity, h.value, j as i64 + 1],
            )
            .map_err(err)?;
        }
    }
    for r in &rec.rates {
        conn.execute("INSERT INTO fin_rates (month, currency, rate) VALUES (?1, ?2, ?3)", params![r.month, r.currency, r.rate])
            .map_err(err)?;
    }
    let acc = |n: &Option<String>| n.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(|n| account_ids[n]);
    let mut fixed_ids: HashMap<String, i64> = HashMap::new();
    for (i, f) in rec.fixed.iter().enumerate() {
        let kind_id = kinds::resolve(conn, f.flow.kinds_module(), f.category.as_deref())?;
        conn.execute(
            &format!(
                "INSERT INTO fin_fixed (name, flow, amount, currency, kind_id, account_id, rule, next_day, stopped, note, sort_order,
                   created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, coalesce(?12, {NOW}), coalesce(?13, ?12, {NOW}))"
            ),
            params![
                f.name.trim(), f.flow, f.amount, f.currency, kind_id, acc(&f.account), f.rule, f.next_day, f.stopped,
                common::clean(f.note.clone()), i as i64 + 1, f.created_at, f.updated_at
            ],
        )
        .map_err(err)?;
        fixed_ids.insert(f.name.trim().to_string(), conn.last_insert_rowid());
    }
    for x in &rec.entries {
        let kind_id = kinds::resolve(conn, x.flow.kinds_module(), x.category.as_deref())?;
        conn.execute(
            &format!(
                "INSERT INTO fin_entries (flow, amount, currency, day, kind_id, account_id, fixed_id, note, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, coalesce(?9, {NOW}), coalesce(?10, ?9, {NOW}))"
            ),
            params![
                x.flow, x.amount, x.currency, x.day, kind_id, acc(&x.account), x.fixed.as_deref().map(|f| fixed_ids[f.trim()]),
                common::clean(x.note.clone()), x.created_at, x.updated_at
            ],
        )
        .map_err(err)?;
    }
    for b in &rec.budgets {
        let kind_id = kinds::resolve(conn, OUT_KINDS, Some(&b.category))?.ok_or("预算缺少分类")?;
        conn.execute("INSERT INTO fin_budgets (kind_id, currency, amount) VALUES (?1, ?2, ?3)", params![kind_id, b.currency, b.amount])
            .map_err(err)?;
    }
    Ok(rec.len())
}

pub fn export(conn: &Connection) -> Result<FinanceRecord, String> {
    let out_names = kinds::name_map(conn, OUT_KINDS)?;
    let in_names = kinds::name_map(conn, IN_KINDS)?;
    let cat = |flow: Flow, id: Option<i64>| id.and_then(|id| if flow == Flow::Out { &out_names } else { &in_names }.get(&id).cloned());
    let accs = accounts(conn)?;
    let acc_name: HashMap<i64, String> = accs.iter().map(|a| (a.id, a.name.clone())).collect();
    let all_balances = assets(conn)?.balances;
    let all_holdings = holdings(conn)?;
    let accounts = accs
        .iter()
        .map(|a| AccountRecord {
            name: a.name.clone(),
            kind: a.kind,
            currency: a.currency,
            closed: a.closed,
            note: a.note.clone(),
            balances: all_balances
                .iter()
                .filter(|b| b.account_id == a.id)
                .map(|b| BalanceRecord { month: b.month.clone(), amount: b.amount })
                .collect(),
            holdings: all_holdings
                .iter()
                .filter(|h| h.account_id == a.id)
                .map(|h| HoldingRecord { month: h.month.clone(), name: h.name.clone(), quantity: h.quantity, value: h.value })
                .collect(),
            created_at: Some(a.created_at.clone()),
            updated_at: Some(a.updated_at.clone()),
        })
        .collect();
    let rates = assets(conn)?.rates.into_iter().map(|r| RateRecord { month: r.month, currency: r.currency, rate: r.rate }).collect();
    let fixed_all = fixed_list(conn)?;
    let fixed_name: HashMap<i64, String> = fixed_all.iter().map(|f| (f.id, f.name.clone())).collect();
    let fixed = fixed_all
        .iter()
        .map(|f| FixedRecord {
            name: f.name.clone(),
            flow: f.flow,
            amount: f.amount,
            currency: f.currency,
            category: cat(f.flow, f.kind_id),
            account: f.account_id.and_then(|id| acc_name.get(&id).cloned()),
            rule: f.rule.clone(),
            next_day: f.next_day.clone(),
            stopped: f.stopped,
            note: f.note.clone(),
            created_at: Some(f.created_at.clone()),
            updated_at: Some(f.updated_at.clone()),
        })
        .collect();
    let mut st = conn.prepare(&format!("SELECT {ENTRY_COLS} FROM fin_entries ORDER BY day, id")).map_err(err)?;
    let entries = st
        .query_map([], entry_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?
        .into_iter()
        .map(|x| EntryRecord {
            flow: x.flow,
            amount: x.amount,
            currency: x.currency,
            day: x.day,
            category: cat(x.flow, x.kind_id),
            account: x.account_id.and_then(|id| acc_name.get(&id).cloned()),
            fixed: x.fixed_id.and_then(|id| fixed_name.get(&id).cloned()),
            note: x.note,
            created_at: Some(x.created_at),
            updated_at: Some(x.updated_at),
        })
        .collect();
    let budgets = budgets(conn)?
        .into_iter()
        .filter_map(|b| out_names.get(&b.kind_id).map(|n| BudgetRecord { category: n.clone(), currency: b.currency, amount: b.amount }))
        .collect();
    Ok(FinanceRecord { accounts, rates, fixed, entries, budgets })
}

// ---------- commands ----------

fn today(conn: &Connection) -> Result<String, String> {
    super::todo::today(conn)
}

#[tauri::command]
pub fn fin_accounts(state: State<AppState>) -> Result<Vec<Account>, String> {
    state.with_db(|c| accounts(c))
}
#[tauri::command]
pub fn fin_account_create(state: State<AppState>, patch: AccountPatch) -> Result<Account, String> {
    state.with_undo("添加账户", |c| create_account(c, patch))
}
#[tauri::command]
pub fn fin_account_update(state: State<AppState>, id: i64, patch: AccountPatch) -> Result<Account, String> {
    state.with_undo("修改账户", |c| update_account(c, id, patch))
}
#[tauri::command]
pub fn fin_account_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除账户", |c| delete_account(c, id))
}
#[tauri::command]
pub fn fin_accounts_reorder(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| common::reorder(c, "fin_accounts", "1 = 1", &[], &ids))
}
#[tauri::command]
pub fn fin_assets(state: State<AppState>) -> Result<Assets, String> {
    state.with_db(|c| assets(c))
}
#[tauri::command]
pub fn fin_month_save(state: State<AppState>, month: String, balances: Vec<BalanceInput>, rates: Vec<RateInput>) -> Result<(), String> {
    state.with_undo("更新余额", |c| save_month(c, &month, &balances, &rates))
}
#[tauri::command]
pub fn fin_holdings(state: State<AppState>) -> Result<Vec<Holding>, String> {
    state.with_db(|c| holdings(c))
}
#[tauri::command]
pub fn fin_holdings_save(state: State<AppState>, account_id: i64, month: String, rows: Vec<HoldingInput>) -> Result<(), String> {
    state.with_undo("更新持仓", |c| save_holdings(c, account_id, &month, &rows))
}
#[tauri::command]
pub fn fin_entries(state: State<AppState>, month: String) -> Result<Vec<Entry>, String> {
    state.with_db(|c| entries(c, &month))
}
#[tauri::command]
pub fn fin_entry_create(state: State<AppState>, patch: EntryPatch) -> Result<Entry, String> {
    state.with_undo("记一笔", |c| create_entry(c, patch))
}
#[tauri::command]
pub fn fin_entries_create(state: State<AppState>, patches: Vec<EntryPatch>) -> Result<usize, String> {
    state.with_undo("记一周小额", |c| create_entries(c, patches))
}
#[tauri::command]
pub fn fin_entry_update(state: State<AppState>, id: i64, patch: EntryPatch) -> Result<Entry, String> {
    state.with_undo("修改记录", |c| update_entry(c, id, patch))
}
#[tauri::command]
pub fn fin_entry_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除记录", |c| delete_entry(c, id))
}
#[tauri::command]
pub fn fin_fixed(state: State<AppState>) -> Result<Vec<Fixed>, String> {
    state.with_db(|c| fixed_list(c))
}
#[tauri::command]
pub fn fin_fixed_create(state: State<AppState>, patch: FixedPatch) -> Result<Fixed, String> {
    state.with_undo("添加固定支出", |c| create_fixed(c, patch, &today(c)?))
}
#[tauri::command]
pub fn fin_fixed_update(state: State<AppState>, id: i64, patch: FixedPatch) -> Result<Fixed, String> {
    state.with_undo("修改固定支出", |c| update_fixed(c, id, patch, &today(c)?))
}
#[tauri::command]
pub fn fin_fixed_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除固定支出", |c| delete_fixed(c, id))
}
#[tauri::command]
pub fn fin_fixed_reorder(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| common::reorder(c, "fin_fixed", "1 = 1", &[], &ids))
}
#[tauri::command]
pub fn fin_budgets(state: State<AppState>) -> Result<Vec<Budget>, String> {
    state.with_db(|c| budgets(c))
}
#[tauri::command]
pub fn fin_budgets_save(state: State<AppState>, currency: Currency, rows: Vec<BudgetInput>) -> Result<(), String> {
    state.with_undo("设预算", |c| save_budgets(c, currency, &rows))
}
#[tauri::command]
pub fn fin_totals(state: State<AppState>) -> Result<Vec<Total>, String> {
    state.with_db(|c| totals(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::modules::kinds::tests::kind_id;
    use serde_json::json;

    fn ep(v: serde_json::Value) -> EntryPatch {
        serde_json::from_value(v).unwrap()
    }
    fn fp(v: serde_json::Value) -> FixedPatch {
        serde_json::from_value(v).unwrap()
    }
    fn ap(v: serde_json::Value) -> AccountPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn fixed_rules() {
        let m = FixedRule::parse("monthly:31").unwrap();
        assert_eq!(m.on_or_after("2026-02-10"), "2026-02-28");
        assert_eq!(m.after("2026-02-28"), "2026-03-31");
        assert_eq!(FixedRule::parse("monthly:15").unwrap().on_or_after("2026-10-15"), "2026-10-15");
        let y = FixedRule::parse("yearly:02-29").unwrap();
        assert_eq!(y.on_or_after("2026-10-02"), "2027-02-28");
        assert_eq!(y.after("2027-02-28"), "2028-02-29");
        assert_eq!(FixedRule::parse("yearly:12-01").unwrap().on_or_after("2026-12-02"), "2027-12-01");
        for bad in ["monthly:0", "monthly:32", "yearly:13-01", "yearly:02-30", "weekly:1", "monthly", ""] {
            assert!(FixedRule::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn accounts_and_currency_rules() {
        let c = memory_db();
        let sg = create_account(&c, ap(json!({"name": "DBS", "currency": "SGD"}))).unwrap();
        assert!(create_account(&c, ap(json!({"name": " DBS ", "currency": "CNY"}))).is_err());
        assert!(create_account(&c, ap(json!({"name": "x"}))).is_err()); // currency required
        let food = kind_id(&c, OUT_KINDS, "餐饮");
        // currency must match the account; 分类 must match the flow
        assert!(create_entry(&c, ep(json!({"amount": 1000, "currency": "CNY", "day": "2026-10-01", "accountId": sg.id}))).is_err());
        assert!(create_entry(&c, ep(json!({"flow": "in", "amount": 1000, "currency": "SGD", "day": "2026-10-01", "kindId": food}))).is_err());
        assert!(create_entry(&c, ep(json!({"amount": 0, "currency": "SGD", "day": "2026-10-01"}))).is_err());
        let e = create_entry(&c, ep(json!({"amount": 1250, "currency": "SGD", "day": "2026-10-01", "accountId": sg.id, "kindId": food, "note": " 午饭 "})))
            .unwrap();
        assert_eq!((e.flow, e.note.as_deref()), (Flow::Out, Some("午饭")));
        assert!(update_account(&c, sg.id, ap(json!({"currency": "USD"}))).unwrap_err().contains("不能改币种"));
        assert_eq!(update_account(&c, sg.id, ap(json!({"closed": true, "name": "DBS 储蓄"}))).unwrap().name, "DBS 储蓄");
        delete_account(&c, sg.id).unwrap();
        assert_eq!(entry(&c, e.id).unwrap().account_id, None); // entry stays
    }

    #[test]
    fn month_totals_need_rates() {
        let c = memory_db();
        let cn = create_account(&c, ap(json!({"name": "招行", "currency": "CNY"}))).unwrap().id;
        let sg = create_account(&c, ap(json!({"name": "DBS", "currency": "SGD"}))).unwrap().id;
        let b = |id: i64, amount: i64| BalanceInput { account_id: id, amount: Some(amount) };
        save_month(&c, "2026-09", &[b(cn, 1_000_00), b(sg, 200_00)], &[RateInput { currency: Currency::Sgd, rate: Some(5.5) }]).unwrap();
        save_month(&c, "2026-10", &[b(cn, 1_200_00), b(sg, 300_00)], &[]).unwrap();
        let a = assets(&c).unwrap();
        assert_eq!(a.totals[0], MonthTotal { month: "2026-09".into(), total_cny: Some(2_100_00), missing: vec![] });
        assert_eq!((a.totals[1].total_cny, a.totals[1].missing.clone()), (None, vec![Currency::Sgd]));
        save_month(&c, "2026-10", &[BalanceInput { account_id: sg, amount: None }], &[]).unwrap(); // cleared
        assert_eq!(assets(&c).unwrap().totals[1].total_cny, Some(1_200_00));
        assert_eq!(account(&c, cn).unwrap().last_month.as_deref(), Some("2026-10"));
        assert!(save_month(&c, "2026-13", &[], &[]).is_err());
        assert!(save_month(&c, "2026-10", &[], &[RateInput { currency: Currency::Usd, rate: Some(0.0) }]).is_err());
    }

    #[test]
    fn fixed_posts_on_its_day() {
        let c = memory_db();
        let rent = kind_id(&c, OUT_KINDS, "住房");
        let f = create_fixed(&c, fp(json!({"name": "房租", "amount": 150000, "currency": "SGD", "kindId": rent, "rule": "monthly:1"})), "2026-10-02")
            .unwrap();
        assert_eq!(f.next_day, "2026-11-01"); // first day from today
        assert_eq!(post_due(&c, "2026-10-31").unwrap(), 0);
        assert_eq!(post_due(&c, "2026-12-05").unwrap(), 2); // 11-01 and 12-01, catching up
        let posted = entries(&c, "2026-12").unwrap();
        assert_eq!((posted[0].fixed_id, posted[0].note.as_deref(), posted[0].kind_id), (Some(f.id), Some("房租"), Some(rent)));
        assert_eq!(fixed(&c, f.id).unwrap().next_day, "2027-01-01");
        // a next day in the past posts right away; stopped ones don't post
        let phone = create_fixed(&c, fp(json!({"name": "话费", "amount": 3000, "currency": "CNY", "rule": "monthly:20", "nextDay": "2026-09-20"})), "2026-10-02")
            .unwrap();
        assert_eq!((phone.next_day.as_str(), entries(&c, "2026-09").unwrap().len()), ("2026-10-20", 1));
        update_fixed(&c, phone.id, fp(json!({"stopped": true})), "2026-10-02").unwrap();
        assert_eq!(post_due(&c, "2027-03-01").unwrap(), 3); // only 房租 (01-01 … 03-01)
        // changing the rule re-plans from today
        assert_eq!(update_fixed(&c, phone.id, fp(json!({"rule": "yearly:03-15"})), "2026-10-02").unwrap().next_day, "2027-03-15");
        assert!(create_fixed(&c, fp(json!({"name": "房租", "amount": 1, "currency": "SGD", "rule": "monthly:1"})), "2026-10-02").is_err());
        delete_fixed(&c, f.id).unwrap();
        assert_eq!(entries(&c, "2026-12").unwrap()[0].fixed_id, None);
    }

    #[test]
    fn budgets_and_totals() {
        let c = memory_db();
        let food = kind_id(&c, OUT_KINDS, "餐饮");
        let pay = kind_id(&c, IN_KINDS, "津贴");
        save_budgets(&c, Currency::Sgd, &[BudgetInput { kind_id: food, amount: Some(60000) }]).unwrap();
        assert!(save_budgets(&c, Currency::Sgd, &[BudgetInput { kind_id: pay, amount: Some(1) }]).is_err());
        assert_eq!(budgets(&c).unwrap(), [Budget { kind_id: food, currency: Currency::Sgd, amount: 60000 }]);
        create_entries(
            &c,
            vec![
                ep(json!({"amount": 32000, "currency": "SGD", "day": "2026-10-04", "kindId": food})),
                ep(json!({"amount": 8000, "currency": "SGD", "day": "2026-10-11", "kindId": food})),
                ep(json!({"flow": "in", "amount": 200000, "currency": "SGD", "day": "2026-10-01", "kindId": pay})),
            ],
        )
        .unwrap();
        assert!(create_entries(&c, vec![ep(json!({"amount": 1, "currency": "SGD", "day": "bad"}))]).is_err());
        let t = totals(&c).unwrap();
        let food_total = t.iter().find(|x| x.kind_id == Some(food)).unwrap();
        assert_eq!((food_total.amount, food_total.count, food_total.month.as_str()), (40000, 2, "2026-10"));
        save_budgets(&c, Currency::Sgd, &[BudgetInput { kind_id: food, amount: None }]).unwrap();
        assert!(budgets(&c).unwrap().is_empty());
    }

    #[test]
    fn holdings_only_for_brokers() {
        let c = memory_db();
        let bank = create_account(&c, ap(json!({"name": "DBS", "currency": "SGD"}))).unwrap().id;
        let ib = create_account(&c, ap(json!({"name": "嘉信", "currency": "USD", "kind": "broker"}))).unwrap().id;
        let rows = vec![HoldingInput { name: "VOO".into(), quantity: Some(3.5), value: 180000 }];
        assert!(save_holdings(&c, bank, "2026-10", &rows).is_err());
        save_holdings(&c, ib, "2026-10", &rows).unwrap();
        save_holdings(&c, ib, "2026-10", &rows).unwrap(); // replaces, not appends
        assert_eq!(holdings(&c).unwrap().len(), 1);
        assert!(update_account(&c, ib, ap(json!({"kind": "bank"}))).is_err());
    }

    #[test]
    fn records_roundtrip() {
        let c = memory_db();
        let rec: FinanceRecord = serde_json::from_value(json!({
            "accounts": [
                {"name": "招行", "currency": "CNY", "balances": [{"month": "2026-09", "amount": 100000}]},
                {"name": "嘉信", "kind": "broker", "currency": "USD", "balances": [{"month": "2026-09", "amount": 500000}],
                 "holdings": [{"month": "2026-09", "name": "VOO", "quantity": 1.5, "value": 400000}]}
            ],
            "rates": [{"month": "2026-09", "currency": "USD", "rate": 7.1}],
            "fixed": [{"name": "话费", "amount": 3000, "currency": "CNY", "category": "通讯", "account": "招行", "rule": "monthly:20", "nextDay": "2026-10-20"}],
            "entries": [
                {"amount": 3000, "currency": "CNY", "day": "2026-09-20", "category": "通讯", "account": "招行", "fixed": "话费", "note": "话费"},
                {"flow": "in", "amount": 200000, "currency": "CNY", "day": "2026-09-01", "category": "家里"},
                {"amount": 999, "currency": "SGD", "day": "2026-09-02", "category": "新分类"}
            ],
            "budgets": [{"category": "餐饮", "currency": "CNY", "amount": 100000}]
        }))
        .unwrap();
        assert_eq!(replace_all(&c, &rec).unwrap(), 2 + 1 + 3 + 1);
        let out = export(&c).unwrap();
        let a = serde_json::to_value(&out).unwrap();
        let c2 = memory_db();
        replace_all(&c2, &out).unwrap();
        assert_eq!(a, serde_json::to_value(export(&c2).unwrap()).unwrap());
        assert_eq!(out.entries.iter().find(|e| e.day == "2026-09-20").unwrap().fixed.as_deref(), Some("话费"));
        assert!(kinds::list(&c, OUT_KINDS).unwrap().iter().any(|k| k.name == "新分类")); // created on import
        assert_eq!(assets(&c).unwrap().totals[0].total_cny, Some(100000 + 3550000));

        let bad = |v: serde_json::Value| validate_records(&serde_json::from_value(v).unwrap()).is_err();
        assert!(bad(json!({"entries": [{"amount": 1, "currency": "CNY", "day": "2026-09-01", "account": "无"}]})));
        assert!(bad(json!({"accounts": [{"name": "a", "currency": "SGD"}], "entries": [{"amount": 1, "currency": "CNY", "day": "2026-09-01", "account": "a"}]})));
        assert!(bad(json!({"accounts": [{"name": "a", "currency": "SGD", "holdings": [{"month": "2026-09", "name": "x", "value": 1}]}]})));
        assert!(bad(json!({"fixed": [{"name": "f", "amount": 1, "currency": "CNY", "rule": "weekly:1", "nextDay": "2026-09-01"}]})));
        assert!(bad(json!({"entries": [{"amount": 1, "currency": "CNY", "day": "2026-09-01", "fixed": "无"}]})));
    }
}
