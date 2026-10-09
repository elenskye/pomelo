//! 钱来钱往 (finance). Money is integer cents in the row's currency; currencies are never mixed
//! except for total assets, which converts balances with CNY rates.
//! - Accounts hold one currency each. Balances, rates and a broker's holdings are dated snapshots
//!   (更新余额 / 更新持仓 on some day). Total assets on a day with any balance = each account's latest
//!   balance up to that day (a closed account stops counting after its last one) × the latest rate
//!   up to that day.
//! - Holdings: quantity × unit price (currency units, not cents) = market value.
//! - 收支 entries: flow out / in, amount, currency, day (… `end_day` for a period, e.g. 记小额), 分类.
//!   Monthly sums spread a period's amount evenly over its days (`spread`). No account.
//! - 固定支出 (`fin_fixed`): on `next_day` an entry is posted and `next_day` moves on by the rule
//!   (`post_due`: on create / update, and from the daily sweep). Entries keep `fixed_id`.
//! - Budgets: monthly amount per 支出分类 + currency.
//! 分类 = kinds.rs modules "fin_out" (支出) / "fin_in" (收入).

use super::common::{self, err, some, str_enum};
use super::kinds;
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
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

/// v7 and older files dated snapshots by month (`YYYY-MM`): read as that month's last day.
fn day_or_month_end(s: &str) -> String {
    let s = s.trim();
    if s.len() == 7 && s.as_bytes()[4] == b'-' {
        if let (Ok(y), Ok(m)) = (s[..4].parse::<i64>(), s[5..].parse::<u32>()) {
            if (1..=12).contains(&m) {
                return format!("{s}-{:02}", common::days_in_month(y, m));
            }
        }
    }
    s.to_string()
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
    pub last_day: Option<String>,
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
    b.day, b.amount
  FROM fin_accounts a
  LEFT JOIN fin_balances b ON b.id = (SELECT id FROM fin_balances WHERE account_id = a.id ORDER BY day DESC LIMIT 1)";

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
        last_day: r.get(9)?,
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
            "SELECT (SELECT count(*) FROM fin_balances WHERE account_id = ?1) + (SELECT count(*) FROM fin_holdings WHERE account_id = ?1)",
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

/// Deletes the account with its balances and holdings.
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
    pub day: String,
    pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rate {
    pub day: String,
    pub currency: Currency,
    pub rate: f64,
}

/// Total assets on one day that has any balance.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayTotal {
    pub day: String,
    /// In CNY cents; None while a currency has no rate yet.
    pub total_cny: Option<i64>,
    /// Currencies with balances but no rate up to that day.
    pub missing: Vec<Currency>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assets {
    pub balances: Vec<Balance>,
    pub rates: Vec<Rate>,
    /// Days with any balance, oldest first.
    pub totals: Vec<DayTotal>,
}

pub fn assets(conn: &Connection) -> Result<Assets, String> {
    let mut st = conn.prepare("SELECT account_id, day, amount FROM fin_balances ORDER BY day, account_id").map_err(err)?;
    let balances: Vec<Balance> = st
        .query_map([], |r| Ok(Balance { account_id: r.get(0)?, day: r.get(1)?, amount: r.get(2)? }))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut st = conn.prepare("SELECT day, currency, rate FROM fin_rates ORDER BY day, currency").map_err(err)?;
    let rates: Vec<Rate> = st
        .query_map([], |r| Ok(Rate { day: r.get(0)?, currency: r.get(1)?, rate: r.get(2)? }))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let accs = accounts(conn)?;
    let mut days: Vec<&str> = balances.iter().map(|b| b.day.as_str()).collect();
    days.dedup();
    let (mut bi, mut ri) = (0, 0);
    let mut balance: HashMap<i64, i64> = HashMap::new();
    let mut rate: HashMap<&str, f64> = HashMap::new();
    let mut totals = Vec::with_capacity(days.len());
    for day in days {
        while bi < balances.len() && balances[bi].day.as_str() <= day {
            balance.insert(balances[bi].account_id, balances[bi].amount);
            bi += 1;
        }
        while ri < rates.len() && rates[ri].day.as_str() <= day {
            rate.insert(rates[ri].currency.as_str(), rates[ri].rate);
            ri += 1;
        }
        let mut sum = 0;
        let mut missing = Vec::new();
        for a in &accs {
            let Some(&amount) = balance.get(&a.id) else { continue };
            if a.closed && a.last_day.as_deref().is_some_and(|last| last < day) {
                continue;
            }
            match if a.currency == Currency::Cny { Some(1.0) } else { rate.get(a.currency.as_str()).copied() } {
                Some(r) => sum += (amount as f64 * r).round() as i64,
                None if !missing.contains(&a.currency) => missing.push(a.currency),
                None => {}
            }
        }
        totals.push(DayTotal { day: day.to_string(), total_cny: missing.is_empty().then_some(sum), missing });
    }
    Ok(Assets { balances, rates, totals })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BalanceInput {
    pub account_id: i64,
    /// None = no snapshot for this account that day.
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

/// 更新余额: one day's balances and rates in one go (None clears that row).
pub fn save_balances(conn: &Connection, day: &str, balances: &[BalanceInput], rates: &[RateInput]) -> Result<(), String> {
    check_day(day, "日期")?;
    let tx = common::Tx::begin(conn)?;
    for b in balances {
        account(&tx, b.account_id)?;
        match b.amount {
            Some(a) => {
                if a.abs() > MAX_CENTS {
                    return Err("余额太大了".into());
                }
                tx.execute(
                    &format!(
                        "INSERT INTO fin_balances (account_id, day, amount) VALUES (?1, ?2, ?3)
                         ON CONFLICT(account_id, day) DO UPDATE SET amount = excluded.amount, updated_at = {NOW}"
                    ),
                    params![b.account_id, day, a],
                )
                .map_err(err)?;
            }
            None => {
                tx.execute("DELETE FROM fin_balances WHERE account_id = ?1 AND day = ?2", params![b.account_id, day]).map_err(err)?;
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
                        "INSERT INTO fin_rates (day, currency, rate) VALUES (?1, ?2, ?3)
                         ON CONFLICT(day, currency) DO UPDATE SET rate = excluded.rate, updated_at = {NOW}"
                    ),
                    params![day, r.currency, v],
                )
                .map_err(err)?;
            }
            None => {
                tx.execute("DELETE FROM fin_rates WHERE day = ?1 AND currency = ?2", params![day, r.currency]).map_err(err)?;
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
    pub day: String,
    pub name: String,
    pub quantity: f64,
    /// Unit price in the account's currency (not cents).
    pub price: f64,
    /// Market value in cents = quantity × price.
    pub value: i64,
}

fn market_value(quantity: f64, price: f64) -> i64 {
    (quantity * price * 100.0).round() as i64
}

/// All broker holdings, by account, day, then row order.
pub fn holdings(conn: &Connection) -> Result<Vec<Holding>, String> {
    let mut st = conn
        .prepare("SELECT id, account_id, day, name, quantity, price FROM fin_holdings ORDER BY account_id, day, sort_order, id")
        .map_err(err)?;
    let rows = st
        .query_map([], |r| {
            let (quantity, price): (f64, f64) = (r.get(4)?, r.get(5)?);
            Ok(Holding { id: r.get(0)?, account_id: r.get(1)?, day: r.get(2)?, name: r.get(3)?, quantity, price, value: market_value(quantity, price) })
        })
        .map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HoldingInput {
    pub name: String,
    pub quantity: f64,
    pub price: f64,
}

fn check_holding(h: &HoldingInput) -> Result<String, String> {
    let name = common::require(&h.name, "持仓名称", 50)?;
    if !(h.quantity.is_finite() && h.quantity > 0.0) {
        return Err(format!("「{name}」的数量要大于 0"));
    }
    if !(h.price.is_finite() && h.price >= 0.0) {
        return Err(format!("「{name}」的单价不对"));
    }
    if market_value(h.quantity, h.price) > MAX_CENTS {
        return Err(format!("「{name}」的市值太大了"));
    }
    Ok(name)
}

/// Replaces one broker account's holdings on one day.
pub fn save_holdings(conn: &Connection, account_id: i64, day: &str, rows: &[HoldingInput]) -> Result<(), String> {
    check_day(day, "日期")?;
    if account(conn, account_id)?.kind != AccountKind::Broker {
        return Err("只有券商账户能记持仓".into());
    }
    let tx = common::Tx::begin(conn)?;
    tx.execute("DELETE FROM fin_holdings WHERE account_id = ?1 AND day = ?2", params![account_id, day]).map_err(err)?;
    for (i, h) in rows.iter().enumerate() {
        let name = check_holding(h)?;
        tx.execute(
            "INSERT INTO fin_holdings (account_id, day, name, quantity, price, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![account_id, day, name, h.quantity, h.price, i as i64 + 1],
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
    /// The day, or a period's first day.
    pub day: String,
    /// A period's last day (after `day`).
    pub end_day: Option<String>,
    pub kind_id: Option<i64>,
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
    /// null / same as `day` = a single day.
    #[serde(default, deserialize_with = "some")]
    pub end_day: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub kind_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
}

const ENTRY_COLS: &str = "id, flow, amount, currency, day, end_day, kind_id, fixed_id, note, created_at, updated_at";

fn entry_row(r: &Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: r.get(0)?,
        flow: r.get(1)?,
        amount: r.get(2)?,
        currency: r.get(3)?,
        day: r.get(4)?,
        end_day: r.get(5)?,
        kind_id: r.get(6)?,
        fixed_id: r.get(7)?,
        note: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

/// Entries touching one month (YYYY-MM; a period shows in every month it covers), latest first.
pub fn entries(conn: &Connection, month: &str) -> Result<Vec<Entry>, String> {
    check_month(month)?;
    let mut st = conn
        .prepare(&format!(
            "SELECT {ENTRY_COLS} FROM fin_entries WHERE day <= ?1 || '-31' AND coalesce(end_day, day) >= ?1 || '-01'
             ORDER BY coalesce(end_day, day) DESC, day DESC, id DESC"
        ))
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

/// Shared by entries and 固定支出: the 分类 matches the flow.
fn check_kind(conn: &Connection, flow: Flow, kind_id: Option<i64>) -> Result<(), String> {
    kinds::check(conn, flow.kinds_module(), kind_id).map_err(|_| "选择的分类不存在，或者不是这一类（支出 / 收入）的".to_string())
}

fn check_period(day: &str, end_day: &Option<String>) -> Result<(), String> {
    check_day(day, "日期")?;
    if let Some(e) = end_day {
        check_day(e, "结束日期")?;
        if e.as_str() < day {
            return Err("结束日期不能早于开始日期".into());
        }
    }
    Ok(())
}

struct EntryFields {
    flow: Flow,
    amount: i64,
    currency: Currency,
    day: String,
    end_day: Option<String>,
    kind_id: Option<i64>,
    note: Option<String>,
}

impl EntryFields {
    fn from_patch(old: Option<&Entry>, p: EntryPatch) -> Result<Self, String> {
        let flow = p.flow.or(old.map(|e| e.flow)).unwrap_or_default();
        let amount = p.amount.or(old.map(|e| e.amount)).ok_or("请填金额")?;
        let currency = p.currency.or(old.map(|e| e.currency)).ok_or("请选择币种")?;
        let day = p.day.or(old.map(|e| e.day.clone())).ok_or("请选择日期")?;
        let end_day = common::clean(p.end_day.unwrap_or(old.and_then(|e| e.end_day.clone()))).filter(|e| *e != day);
        let kind_id = p.kind_id.unwrap_or(old.and_then(|e| e.kind_id));
        let note = common::clean(p.note.unwrap_or(old.and_then(|e| e.note.clone())));
        Ok(Self { flow, amount, currency, day, end_day, kind_id, note })
    }

    fn check(&self, conn: &Connection) -> Result<(), String> {
        check_amount(self.amount, "金额")?;
        check_period(&self.day, &self.end_day)?;
        common::check_len(&self.note, "备注", 1000)?;
        check_kind(conn, self.flow, self.kind_id)
    }
}

fn insert_entry(conn: &Connection, f: &EntryFields, fixed_id: Option<i64>) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO fin_entries (flow, amount, currency, day, end_day, kind_id, fixed_id, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![f.flow, f.amount, f.currency, f.day, f.end_day, f.kind_id, fixed_id, f.note],
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

/// 记小额 etc.: several entries in one step (all or nothing).
pub fn create_entries(conn: &Connection, patches: Vec<EntryPatch>) -> Result<usize, String> {
    if patches.is_empty() {
        return Err("没有要记的".into());
    }
    let n = patches.len();
    let tx = common::Tx::begin(conn)?;
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
            "UPDATE fin_entries SET flow = ?1, amount = ?2, currency = ?3, day = ?4, end_day = ?5, kind_id = ?6, note = ?7,
               updated_at = {NOW} WHERE id = ?8"
        ),
        params![f.flow, f.amount, f.currency, f.day, f.end_day, f.kind_id, f.note, id],
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

/// 多选: the same change (e.g. 分类 or 币种) on several entries, all or nothing.
pub fn update_entries(conn: &Connection, ids: &[i64], p: EntryPatch) -> Result<usize, String> {
    if ids.is_empty() {
        return Err("没有选中的记录".into());
    }
    let tx = common::Tx::begin(conn)?;
    for &id in ids {
        update_entry(&tx, id, p.clone())?;
    }
    tx.commit().map_err(err)?;
    Ok(ids.len())
}

pub fn delete_entries(conn: &Connection, ids: &[i64]) -> Result<usize, String> {
    if ids.is_empty() {
        return Err("没有选中的记录".into());
    }
    let tx = common::Tx::begin(conn)?;
    for &id in ids {
        delete_entry(&tx, id)?;
    }
    tx.commit().map_err(err)?;
    Ok(ids.len())
}

/// Where the latest period ends (记小额 starts the next one the day after).
pub fn last_period_end(conn: &Connection) -> Result<Option<String>, String> {
    conn.query_row("SELECT max(end_day) FROM fin_entries", [], |r| r.get(0)).map_err(err)
}

/// A period's amount by month, in proportion to its days there (each day = amount ÷ days); the
/// parts are rounded so they add up to the amount exactly.
pub fn spread(amount: i64, day: &str, end_day: &str) -> Vec<(String, i64)> {
    let (first, last) = (common::day_number(day), common::day_number(end_day));
    let days = last - first + 1;
    let mut out = Vec::new();
    let (mut d, mut covered, mut given) = (first, 0, 0);
    while d <= last {
        let date = common::from_day_number(d);
        let (y, m) = (date[..4].parse::<i64>().unwrap(), date[5..7].parse::<u32>().unwrap());
        let to = last.min(d - date[8..10].parse::<i64>().unwrap() + common::days_in_month(y, m) as i64);
        covered += to - d + 1;
        let upto = (2 * amount * covered + days) / (2 * days); // amount × covered ÷ days, rounded
        out.push((date[..7].to_string(), upto - given));
        given = upto;
        d = to + 1;
    }
    out
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

const FIXED_COLS: &str = "id, name, flow, amount, currency, kind_id, rule, next_day, stopped, note, sort_order, created_at, updated_at";

fn fixed_row(r: &Row) -> rusqlite::Result<Fixed> {
    Ok(Fixed {
        id: r.get(0)?,
        name: r.get(1)?,
        flow: r.get(2)?,
        amount: r.get(3)?,
        currency: r.get(4)?,
        kind_id: r.get(5)?,
        rule: r.get(6)?,
        next_day: r.get(7)?,
        stopped: r.get(8)?,
        note: r.get(9)?,
        sort_order: r.get(10)?,
        created_at: r.get(11)?,
        updated_at: r.get(12)?,
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
        rule: rule_text,
        next_day,
        stopped: p.stopped.or(old.map(|f| f.stopped)).unwrap_or(false),
        note: common::clean(p.note.unwrap_or(old.and_then(|f| f.note.clone()))),
    };
    check_amount(f.amount, "金额")?;
    common::check_len(&f.note, "备注", 1000)?;
    check_kind(conn, f.flow, f.kind_id)?;
    Ok(f)
}

/// Posts every running 固定支出 due on or before `today`, moving `next_day` past today.
pub fn post_due(conn: &Connection, today: &str) -> Result<usize, String> {
    let tx = common::Tx::begin(conn)?;
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
                end_day: None,
                kind_id: f.kind_id,
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
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        "INSERT INTO fin_fixed (name, flow, amount, currency, kind_id, rule, next_day, stopped, note, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, (SELECT coalesce(max(sort_order), 0) + 1 FROM fin_fixed))",
        params![f.name, f.flow, f.amount, f.currency, f.kind_id, f.rule, f.next_day, f.stopped, f.note],
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
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        &format!(
            "UPDATE fin_fixed SET name = ?1, flow = ?2, amount = ?3, currency = ?4, kind_id = ?5, rule = ?6,
               next_day = ?7, stopped = ?8, note = ?9, updated_at = {NOW} WHERE id = ?10"
        ),
        params![f.name, f.flow, f.amount, f.currency, f.kind_id, f.rule, f.next_day, f.stopped, f.note, id],
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
    let tx = common::Tx::begin(conn)?;
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

/// Entry sums by month × currency × flow × 分类 (reports pick what they need). A period counts in
/// every month it covers, with that month's share of its amount (`spread`).
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
    let mut st = conn.prepare("SELECT day, end_day, currency, flow, kind_id, amount FROM fin_entries").map_err(err)?;
    let rows = st
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Currency>(2)?, r.get::<_, Flow>(3)?, r.get::<_, Option<i64>>(4)?, r.get::<_, i64>(5)?))
        })
        .map_err(err)?;
    // keyed by codes so the output sorts like the old GROUP BY (month, currency, flow, kind)
    let mut sums: BTreeMap<(String, &str, &str, Option<i64>), (Currency, Flow, i64, i64)> = BTreeMap::new();
    for row in rows {
        let (day, end_day, currency, flow, kind_id, amount) = row.map_err(err)?;
        let parts = match &end_day {
            Some(e) => spread(amount, &day, e),
            None => vec![(day[..7].to_string(), amount)],
        };
        for (month, part) in parts {
            let s = sums.entry((month, currency.as_str(), flow.as_str(), kind_id)).or_insert((currency, flow, 0, 0));
            s.2 += part;
            s.3 += 1;
        }
    }
    Ok(sums
        .into_iter()
        .map(|((month, _, _, kind_id), (currency, flow, amount, count))| Total { month, currency, flow, kind_id, amount, count })
        .collect())
}

// ---------- import / export: accounts (with balances, holdings), rates, fixed, entries, budgets ----------
// v7 and older files: snapshots by `month`, holdings as `value` (+ optional quantity), entries and
// 固定支出 with an `account` (ignored now).

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BalanceRecord {
    #[serde(alias = "month")]
    pub day: String,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HoldingRecord {
    #[serde(alias = "month")]
    pub day: String,
    pub name: String,
    #[serde(default)]
    pub quantity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    /// v7 and older: market value in cents.
    #[serde(default, skip_serializing)]
    pub value: Option<i64>,
}

impl HoldingRecord {
    /// Old rows: price = value ÷ quantity (no quantity → 1 × value), as migration 18 did.
    fn input(&self) -> Result<HoldingInput, String> {
        let (quantity, price) = match (self.price, self.value) {
            (Some(p), _) => (self.quantity.ok_or_else(|| format!("「{}」缺少数量", self.name.trim()))?, p),
            (None, Some(v)) => match self.quantity.filter(|q| *q > 0.0) {
                Some(q) => (q, v as f64 / 100.0 / q),
                None => (1.0, v as f64 / 100.0),
            },
            (None, None) => return Err(format!("「{}」缺少单价", self.name.trim())),
        };
        Ok(HoldingInput { name: self.name.clone(), quantity, price })
    }
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
    #[serde(alias = "month")]
    pub day: String,
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
    /// v7 and older; ignored.
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_day: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    /// v7 and older; ignored.
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
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
    let mut names = HashSet::new();
    for a in &rec.accounts {
        let name = common::require(&a.name, "账户名", 30).map_err(e("的账户".into()))?;
        if !names.insert(name.clone()) {
            return Err(format!("{LABEL}：账户「{name}」重复了"));
        }
        let mut days = HashSet::new();
        for b in &a.balances {
            let day = day_or_month_end(&b.day);
            check_day(&day, "日期").map_err(e(format!("「{name}」的余额")))?;
            if !days.insert(day.clone()) {
                return Err(format!("{LABEL}「{name}」：{day} 的余额重复了"));
            }
        }
        for h in &a.holdings {
            check_day(&day_or_month_end(&h.day), "日期").map_err(e(format!("「{name}」的持仓")))?;
            h.input().and_then(|i| check_holding(&i)).map_err(e(format!("「{name}」的持仓")))?;
        }
        if !a.holdings.is_empty() && a.kind != AccountKind::Broker {
            return Err(format!("{LABEL}「{name}」：只有券商账户能记持仓"));
        }
    }
    let mut rate_keys = HashSet::new();
    for r in &rec.rates {
        let day = day_or_month_end(&r.day);
        check_day(&day, "日期").map_err(e("的汇率".into()))?;
        check_rate(r.rate).map_err(e("的汇率".into()))?;
        if !rate_keys.insert((day.clone(), r.currency.as_str())) {
            return Err(format!("{LABEL}：{day} {} 的汇率重复了", r.currency.as_str()));
        }
    }
    let mut fixed_names = HashSet::new();
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
    }
    for x in &rec.entries {
        let ctx = e(format!("{} 的记录", x.day));
        check_amount(x.amount, "金额").map_err(&ctx)?;
        check_period(&x.day, &x.end_day).map_err(&ctx)?;
        kinds::check_name(&x.category).map_err(&ctx)?;
        if let Some(f) = x.fixed.as_deref().filter(|f| !fixed_names.contains(f.trim())) {
            return Err(ctx(format!("找不到固定支出「{f}」")));
        }
    }
    let mut budget_keys = HashSet::new();
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
        for b in &a.balances {
            conn.execute("INSERT INTO fin_balances (account_id, day, amount) VALUES (?1, ?2, ?3)", params![id, day_or_month_end(&b.day), b.amount])
                .map_err(err)?;
        }
        for (j, h) in a.holdings.iter().enumerate() {
            let input = h.input()?;
            conn.execute(
                "INSERT INTO fin_holdings (account_id, day, name, quantity, price, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, day_or_month_end(&h.day), h.name.trim(), input.quantity, input.price, j as i64 + 1],
            )
            .map_err(err)?;
        }
    }
    for r in &rec.rates {
        conn.execute("INSERT INTO fin_rates (day, currency, rate) VALUES (?1, ?2, ?3)", params![day_or_month_end(&r.day), r.currency, r.rate])
            .map_err(err)?;
    }
    let mut fixed_ids: HashMap<String, i64> = HashMap::new();
    for (i, f) in rec.fixed.iter().enumerate() {
        let kind_id = kinds::resolve(conn, f.flow.kinds_module(), f.category.as_deref())?;
        conn.execute(
            &format!(
                "INSERT INTO fin_fixed (name, flow, amount, currency, kind_id, rule, next_day, stopped, note, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, coalesce(?11, {NOW}), coalesce(?12, ?11, {NOW}))"
            ),
            params![
                f.name.trim(), f.flow, f.amount, f.currency, kind_id, f.rule, f.next_day, f.stopped, common::clean(f.note.clone()), i as i64 + 1,
                f.created_at, f.updated_at
            ],
        )
        .map_err(err)?;
        fixed_ids.insert(f.name.trim().to_string(), conn.last_insert_rowid());
    }
    for x in &rec.entries {
        let kind_id = kinds::resolve(conn, x.flow.kinds_module(), x.category.as_deref())?;
        conn.execute(
            &format!(
                "INSERT INTO fin_entries (flow, amount, currency, day, end_day, kind_id, fixed_id, note, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, coalesce(?9, {NOW}), coalesce(?10, ?9, {NOW}))"
            ),
            params![
                x.flow, x.amount, x.currency, x.day, x.end_day.as_deref().filter(|e| *e != x.day), kind_id,
                x.fixed.as_deref().map(|f| fixed_ids[f.trim()]), common::clean(x.note.clone()), x.created_at, x.updated_at
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
    let all = assets(conn)?;
    let all_holdings = holdings(conn)?;
    let accounts = accounts(conn)?
        .iter()
        .map(|a| AccountRecord {
            name: a.name.clone(),
            kind: a.kind,
            currency: a.currency,
            closed: a.closed,
            note: a.note.clone(),
            balances: all.balances.iter().filter(|b| b.account_id == a.id).map(|b| BalanceRecord { day: b.day.clone(), amount: b.amount }).collect(),
            holdings: all_holdings
                .iter()
                .filter(|h| h.account_id == a.id)
                .map(|h| HoldingRecord { day: h.day.clone(), name: h.name.clone(), quantity: Some(h.quantity), price: Some(h.price), value: None })
                .collect(),
            created_at: Some(a.created_at.clone()),
            updated_at: Some(a.updated_at.clone()),
        })
        .collect();
    let rates = all.rates.into_iter().map(|r| RateRecord { day: r.day, currency: r.currency, rate: r.rate }).collect();
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
            account: None,
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
            end_day: x.end_day,
            category: cat(x.flow, x.kind_id),
            account: None,
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
pub fn fin_balances_save(state: State<AppState>, day: String, balances: Vec<BalanceInput>, rates: Vec<RateInput>) -> Result<(), String> {
    state.with_undo("更新余额", |c| save_balances(c, &day, &balances, &rates))
}
#[tauri::command]
pub fn fin_holdings(state: State<AppState>) -> Result<Vec<Holding>, String> {
    state.with_db(|c| holdings(c))
}
#[tauri::command]
pub fn fin_holdings_save(state: State<AppState>, account_id: i64, day: String, rows: Vec<HoldingInput>) -> Result<(), String> {
    state.with_undo("更新持仓", |c| save_holdings(c, account_id, &day, &rows))
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
    state.with_undo("记小额", |c| create_entries(c, patches))
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
pub fn fin_entries_update(state: State<AppState>, ids: Vec<i64>, patch: EntryPatch) -> Result<usize, String> {
    state.with_undo(&format!("修改 {} 笔记录", ids.len()), |c| update_entries(c, &ids, patch))
}
#[tauri::command]
pub fn fin_entries_delete(state: State<AppState>, ids: Vec<i64>) -> Result<usize, String> {
    state.with_undo(&format!("删除 {} 笔记录", ids.len()), |c| delete_entries(c, &ids))
}
#[tauri::command]
pub fn fin_period_last(state: State<AppState>) -> Result<Option<String>, String> {
    state.with_db(|c| last_period_end(c))
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
    fn bal(id: i64, amount: i64) -> BalanceInput {
        BalanceInput { account_id: id, amount: Some(amount) }
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
        // 分类 must match the flow; entries have no account
        assert!(create_entry(&c, ep(json!({"flow": "in", "amount": 1000, "currency": "SGD", "day": "2026-10-01", "kindId": food}))).is_err());
        assert!(create_entry(&c, ep(json!({"amount": 0, "currency": "SGD", "day": "2026-10-01"}))).is_err());
        assert!(serde_json::from_value::<EntryPatch>(json!({"accountId": sg.id})).is_err());
        let e = create_entry(&c, ep(json!({"amount": 1250, "currency": "SGD", "day": "2026-10-01", "kindId": food, "note": " 午饭 "}))).unwrap();
        assert_eq!((e.flow, e.note.as_deref(), e.end_day.as_deref()), (Flow::Out, Some("午饭"), None));
        // currency is free until the account has balances or holdings
        assert_eq!(update_account(&c, sg.id, ap(json!({"currency": "USD"}))).unwrap().currency, Currency::Usd);
        save_balances(&c, "2026-10-01", &[bal(sg.id, 100)], &[]).unwrap();
        assert!(update_account(&c, sg.id, ap(json!({"currency": "SGD"}))).unwrap_err().contains("不能改币种"));
        assert_eq!(update_account(&c, sg.id, ap(json!({"closed": true, "name": "DBS 储蓄"}))).unwrap().name, "DBS 储蓄");
        delete_account(&c, sg.id).unwrap();
        assert!(assets(&c).unwrap().balances.is_empty());
    }

    #[test]
    fn day_totals_carry_balances_and_rates_forward() {
        let c = memory_db();
        let cn = create_account(&c, ap(json!({"name": "招行", "currency": "CNY"}))).unwrap().id;
        let sg = create_account(&c, ap(json!({"name": "DBS", "currency": "SGD"}))).unwrap().id;
        let usd = create_account(&c, ap(json!({"name": "嘉信", "currency": "USD", "kind": "broker"}))).unwrap().id;
        let sgd = |r: f64| RateInput { currency: Currency::Sgd, rate: Some(r) };
        save_balances(&c, "2026-09-03", &[bal(cn, 1_000_00), bal(sg, 200_00)], &[sgd(5.5)]).unwrap();
        save_balances(&c, "2026-09-10", &[bal(cn, 1_100_00)], &[]).unwrap(); // only one account that day
        save_balances(&c, "2026-09-20", &[bal(usd, 10_00)], &[]).unwrap(); // USD has no rate yet
        save_balances(&c, "2026-09-25", &[bal(sg, 300_00)], &[sgd(5.4), RateInput { currency: Currency::Usd, rate: Some(7.0) }]).unwrap();
        let t = assets(&c).unwrap().totals;
        let days: Vec<&str> = t.iter().map(|t| t.day.as_str()).collect();
        assert_eq!(days, ["2026-09-03", "2026-09-10", "2026-09-20", "2026-09-25"]); // only days with updates
        assert_eq!(t[0].total_cny, Some(1_000_00 + 1_100_00));
        assert_eq!(t[1].total_cny, Some(1_100_00 + 1_100_00)); // DBS and its rate carried forward
        assert_eq!((t[2].total_cny, t[2].missing.clone()), (None, vec![Currency::Usd]));
        assert_eq!(t[3].total_cny, Some(1_100_00 + 1_620_00 + 70_00));
        // a closed account stops counting after its last balance (09-20)
        update_account(&c, usd, ap(json!({"closed": true}))).unwrap();
        save_balances(&c, "2026-10-01", &[bal(cn, 900_00)], &[]).unwrap();
        let t = assets(&c).unwrap().totals;
        assert_eq!((t[2].total_cny, t[3].total_cny, t[4].total_cny), (None, Some(1_100_00 + 1_620_00), Some(900_00 + 1_620_00)));
        // clearing a balance; account shows its latest snapshot
        save_balances(&c, "2026-10-01", &[BalanceInput { account_id: cn, amount: None }], &[]).unwrap();
        assert_eq!(account(&c, cn).unwrap().last_day.as_deref(), Some("2026-09-10"));
        assert!(save_balances(&c, "2026-10", &[], &[]).is_err());
        assert!(save_balances(&c, "2026-10-02", &[], &[RateInput { currency: Currency::Usd, rate: Some(0.0) }]).is_err());
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
        assert!(serde_json::from_value::<FixedPatch>(json!({"accountId": 1})).is_err());
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
    fn periods_spread_over_their_days() {
        assert_eq!(spread(7000, "2026-09-28", "2026-10-04"), [("2026-09".to_string(), 3000), ("2026-10".to_string(), 4000)]);
        assert_eq!(spread(100, "2026-09-30", "2026-10-02"), [("2026-09".to_string(), 33), ("2026-10".to_string(), 67)]); // adds up
        assert_eq!(spread(100, "2026-12-31", "2027-02-01").iter().map(|x| x.1).sum::<i64>(), 100);
        assert_eq!(spread(5, "2026-10-03", "2026-10-03"), [("2026-10".to_string(), 5)]);

        let c = memory_db();
        let food = kind_id(&c, OUT_KINDS, "餐饮");
        assert_eq!(last_period_end(&c).unwrap(), None);
        let e = create_entry(&c, ep(json!({"amount": 7000, "currency": "SGD", "day": "2026-09-28", "endDay": "2026-10-04", "kindId": food}))).unwrap();
        assert_eq!(e.end_day.as_deref(), Some("2026-10-04"));
        assert_eq!(last_period_end(&c).unwrap().as_deref(), Some("2026-10-04"));
        // listed in both months, summed by share
        assert_eq!((entries(&c, "2026-09").unwrap().len(), entries(&c, "2026-10").unwrap().len(), entries(&c, "2026-11").unwrap().len()), (1, 1, 0));
        let t = totals(&c).unwrap();
        assert_eq!(t.iter().map(|x| (x.month.as_str(), x.amount)).collect::<Vec<_>>(), [("2026-09", 3000), ("2026-10", 4000)]);
        // end = start → a single day; end before start → error
        assert_eq!(update_entry(&c, e.id, ep(json!({"endDay": "2026-09-28"}))).unwrap().end_day, None);
        assert!(update_entry(&c, e.id, ep(json!({"endDay": "2026-09-01"}))).unwrap_err().contains("结束日期"));
    }

    #[test]
    fn batch_edit_and_delete() {
        let c = memory_db();
        let food = kind_id(&c, OUT_KINDS, "餐饮");
        let fun = kind_id(&c, OUT_KINDS, "娱乐");
        let pay = kind_id(&c, IN_KINDS, "津贴");
        let a = create_entry(&c, ep(json!({"amount": 100, "currency": "SGD", "day": "2026-10-01", "kindId": food}))).unwrap().id;
        let b = create_entry(&c, ep(json!({"amount": 200, "currency": "SGD", "day": "2026-10-02"}))).unwrap().id;
        let i = create_entry(&c, ep(json!({"flow": "in", "amount": 300, "currency": "SGD", "day": "2026-10-03", "kindId": pay}))).unwrap().id;
        assert_eq!(update_entries(&c, &[a, b], ep(json!({"kindId": fun}))).unwrap(), 2);
        assert_eq!((entry(&c, a).unwrap().kind_id, entry(&c, b).unwrap().kind_id), (Some(fun), Some(fun)));
        // an out 分类 on an income: nothing changes
        assert!(update_entries(&c, &[a, i], ep(json!({"kindId": food}))).is_err());
        assert_eq!(entry(&c, a).unwrap().kind_id, Some(fun));
        update_entries(&c, &[a, i], ep(json!({"currency": "CNY"}))).unwrap();
        assert_eq!((entry(&c, a).unwrap().currency, entry(&c, i).unwrap().amount), (Currency::Cny, 300));
        assert!(delete_entries(&c, &[a, 999]).is_err()); // all or nothing
        assert_eq!(delete_entries(&c, &[a, b]).unwrap(), 2);
        assert_eq!(entries(&c, "2026-10").unwrap().len(), 1);
        assert!(update_entries(&c, &[], ep(json!({"currency": "CNY"}))).is_err());
    }

    #[test]
    fn holdings_only_for_brokers() {
        let c = memory_db();
        let bank = create_account(&c, ap(json!({"name": "DBS", "currency": "SGD"}))).unwrap().id;
        let ib = create_account(&c, ap(json!({"name": "嘉信", "currency": "USD", "kind": "broker"}))).unwrap().id;
        let rows = vec![HoldingInput { name: "VOO".into(), quantity: 3.5, price: 512.5 }];
        assert!(save_holdings(&c, bank, "2026-10-07", &rows).is_err());
        save_holdings(&c, ib, "2026-10-07", &rows).unwrap();
        save_holdings(&c, ib, "2026-10-07", &rows).unwrap(); // replaces, not appends
        let h = holdings(&c).unwrap();
        assert_eq!((h.len(), h[0].value, h[0].day.as_str()), (1, 179375, "2026-10-07")); // 3.5 × 512.5
        for bad in [(0.0, 1.0), (1.0, -1.0), (f64::NAN, 1.0)] {
            assert!(save_holdings(&c, ib, "2026-10-08", &[HoldingInput { name: "x".into(), quantity: bad.0, price: bad.1 }]).is_err());
        }
        assert!(update_account(&c, ib, ap(json!({"kind": "bank"}))).is_err());
    }

    #[test]
    fn records_roundtrip() {
        let c = memory_db();
        let rec: FinanceRecord = serde_json::from_value(json!({
            "accounts": [
                {"name": "招行", "currency": "CNY", "balances": [{"day": "2026-09-30", "amount": 100000}]},
                {"name": "嘉信", "kind": "broker", "currency": "USD", "balances": [{"day": "2026-09-30", "amount": 500000}],
                 "holdings": [{"day": "2026-09-30", "name": "VOO", "quantity": 1.5, "price": 2666.67}]}
            ],
            "rates": [{"day": "2026-09-30", "currency": "USD", "rate": 7.1}],
            "fixed": [{"name": "话费", "amount": 3000, "currency": "CNY", "category": "通讯", "rule": "monthly:20", "nextDay": "2026-10-20"}],
            "entries": [
                {"amount": 3000, "currency": "CNY", "day": "2026-09-20", "category": "通讯", "fixed": "话费", "note": "话费"},
                {"flow": "in", "amount": 200000, "currency": "CNY", "day": "2026-09-01", "category": "家里"},
                {"amount": 999, "currency": "SGD", "day": "2026-09-02", "endDay": "2026-09-08", "category": "新分类"}
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
        assert_eq!(a["entries"][1]["endDay"], "2026-09-08");
        assert!(a["entries"][0].get("account").is_none());
        assert!(kinds::list(&c, OUT_KINDS).unwrap().iter().any(|k| k.name == "新分类")); // created on import
        assert_eq!(assets(&c).unwrap().totals[0].total_cny, Some(100000 + 3550000));

        let bad = |v: serde_json::Value| validate_records(&serde_json::from_value(v).unwrap()).is_err();
        assert!(bad(json!({"accounts": [{"name": "a", "currency": "SGD", "holdings": [{"day": "2026-09-01", "name": "x", "quantity": 1, "price": 1}]}]})));
        assert!(bad(json!({"accounts": [{"name": "a", "kind": "broker", "currency": "SGD", "holdings": [{"day": "2026-09-01", "name": "x", "price": 1}]}]})));
        assert!(bad(json!({"fixed": [{"name": "f", "amount": 1, "currency": "CNY", "rule": "weekly:1", "nextDay": "2026-09-01"}]})));
        assert!(bad(json!({"entries": [{"amount": 1, "currency": "CNY", "day": "2026-09-01", "fixed": "无"}]})));
        assert!(bad(json!({"entries": [{"amount": 1, "currency": "CNY", "day": "2026-09-05", "endDay": "2026-09-01"}]})));
    }

    #[test]
    fn reads_v7_records() {
        // monthly snapshots, holdings by value, entries / 固定支出 with an account
        let rec: FinanceRecord = serde_json::from_value(json!({
            "accounts": [
                {"name": "招行", "currency": "CNY", "balances": [{"month": "2026-02", "amount": 100000}]},
                {"name": "嘉信", "kind": "broker", "currency": "USD",
                 "holdings": [{"month": "2026-09", "name": "VOO", "quantity": 4, "value": 200000}, {"month": "2026-09", "name": "现金", "value": 12345}]}
            ],
            "rates": [{"month": "2026-02", "currency": "USD", "rate": 7.1}],
            "fixed": [{"name": "话费", "amount": 3000, "currency": "CNY", "account": "招行", "rule": "monthly:20", "nextDay": "2026-10-20"}],
            "entries": [{"amount": 3000, "currency": "CNY", "day": "2026-09-20", "account": "招行", "fixed": "话费"}]
        }))
        .unwrap();
        let c = memory_db();
        replace_all(&c, &rec).unwrap();
        let a = assets(&c).unwrap();
        assert_eq!((a.balances[0].day.as_str(), a.rates[0].day.as_str()), ("2026-02-28", "2026-02-28"));
        let h = holdings(&c).unwrap();
        assert_eq!(h.iter().map(|h| (h.day.as_str(), h.quantity, h.price, h.value)).collect::<Vec<_>>(), [("2026-09-30", 4.0, 500.0, 200000), ("2026-09-30", 1.0, 123.45, 12345)]);
        assert_eq!(entries(&c, "2026-09").unwrap()[0].fixed_id, Some(fixed_list(&c).unwrap()[0].id));
    }
}
