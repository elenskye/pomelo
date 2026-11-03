//! 体检 (设置 → 体检, v0.8): records worth a look, listed by what's wrong, each opening its record the way
//! ⌘K does (`Hit`). Read only, worked out each time; modules hidden in the 模块坞 are left out (made
//! lists one by one). Never 芝麻开门.
//! - 没分类的收支: no `kind_id`; a refund of a known expense follows that expense, so it isn't listed.
//! - 找不到的附件: a note / task / goal hangs a file that isn't on this Mac (`stored` empty) or whose copy is gone.
//! - 可能重复: books / films with the same title (case, spaces and punctuation aside) whose author /
//!   director is the same or missing on one side; items of one made list with the same name.
//! - 过期很久的待办: not done, due more than `OVERDUE_DAYS` ago, not inside a paused or finished goal.
//! - 过期的证件: a 来日方长 证件 whose date has passed.
//! - 等回音很久的投递: stage 等回音, `STALE_JOB_DAYS` or more since it last moved or was followed up.
//! - 很久没记余额的账户: open account, last balance over `STALE_BALANCE_DAYS` ago (or none).

use crate::modules::attachments;
use crate::modules::common::err;
use crate::modules::jobs::{self, Stage};
use crate::modules::todo;
use crate::picky::data;
use crate::search::Hit;
use crate::AppState;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use tauri::State;

pub const OVERDUE_DAYS: i64 = 30;
pub const STALE_JOB_DAYS: i64 = 60;
pub const STALE_BALANCE_DAYS: i64 = 90;
/// Rows a group shows at most (`total` says how many there are).
const SHOWN: usize = 100;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub key: &'static str,
    pub title: String,
    pub total: usize,
    pub items: Vec<Hit>,
}

fn group(key: &'static str, title: String, mut items: Vec<Hit>) -> Group {
    let total = items.len();
    items.truncate(SHOWN);
    Group { key, title, total, items }
}

fn hit(module: &str, kind: &'static str, id: i64, title: String, detail: Option<String>, parent: Option<i64>, day: Option<String>) -> Hit {
    Hit { module: module.into(), kind, id, title, detail, parent, day, amount: None, currency: None, flow: None }
}

/// The first non-empty line of a note, short.
fn first_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("（空笔记）");
    let short: String = line.chars().take(40).collect();
    if short.chars().count() < line.chars().count() { format!("{short}…") } else { short }
}

fn uncategorized(conn: &Connection) -> Result<Vec<Hit>, String> {
    let mut st = conn
        .prepare("SELECT id, coalesce(note, '未分类'), day, amount, currency, flow, is_refund FROM fin_entries WHERE kind_id IS NULL AND refund_of IS NULL ORDER BY day DESC, id DESC")
        .map_err(err)?;
    let rows = st
        .query_map([], |r| {
            let (id, title, day, amount, currency, flow, refund): (i64, String, String, i64, String, String, bool) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?);
            let what = if refund { "退款" } else if flow == "in" { "收入" } else { "支出" };
            let detail = format!("{day} · {what} {}.{:02} {currency}", amount / 100, amount % 100);
            Ok(Hit { module: "finance".into(), kind: "entry", id, title, detail: Some(detail), parent: None, day: Some(day), amount: Some(amount), currency: Some(currency), flow: Some(flow) })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

fn missing_attachments(conn: &Connection, dir: &Path, shown: &dyn Fn(&str) -> bool) -> Result<Vec<Hit>, String> {
    let mut st = conn
        .prepare("SELECT a.owner, a.record_id, f.name, f.stored FROM attachments a JOIN attachment_files f ON f.id = a.file_id ORDER BY a.owner, a.record_id, a.sort_order")
        .map_err(err)?;
    let rows: Vec<(String, i64, String, Option<String>)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut out = vec![];
    for (owner, record, name, stored) in rows {
        if stored.is_some_and(|s| !s.contains('/') && !s.contains("..") && dir.join(s).is_file()) {
            continue;
        }
        let detail = Some(format!("附件「{name}」"));
        let found = match owner.as_str() {
            "note" if shown("notes") => conn.query_row("SELECT content, date(created_at, 'localtime') FROM notes WHERE id = ?1", [record], |r| Ok(hit("notes", "note", record, first_line(&r.get::<_, String>(0)?), detail, None, r.get(1)?))),
            "task" if shown("todo") => conn.query_row("SELECT title, CASE WHEN goal_id IS NULL THEN group_id ELSE -goal_id END, due_date FROM todo_tasks WHERE id = ?1", [record], |r| Ok(hit("todo", "task", record, r.get(0)?, detail, r.get(1)?, r.get(2)?))),
            "goal" if shown("todo") => conn.query_row("SELECT name, group_id, end_date FROM todo_goals WHERE id = ?1", [record], |r| Ok(hit("todo", "goal", record, r.get(0)?, detail, r.get(1)?, r.get(2)?))),
            _ => continue,
        };
        match found {
            Ok(h) => out.push(h),
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(err(e)),
        }
    }
    Ok(out)
}

/// A title to compare: lower case, without spaces and punctuation.
fn same_form(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Groups with the same key, two or more each, in the order first met.
fn repeats<T>(rows: Vec<(String, T)>) -> Vec<Vec<T>> {
    let mut order: Vec<String> = vec![];
    let mut by: HashMap<String, Vec<T>> = HashMap::new();
    for (k, v) in rows {
        if k.is_empty() {
            continue;
        }
        if !by.contains_key(&k) {
            order.push(k.clone());
        }
        by.entry(k).or_default().push(v);
    }
    order.into_iter().filter_map(|k| by.remove(&k)).filter(|g| g.len() > 1).collect()
}

fn duplicate_works(conn: &Connection, module: &str, kind: &'static str, table: &str, person: &str) -> Result<Vec<Hit>, String> {
    let mut st = conn.prepare(&format!("SELECT id, title, {person} FROM {table} ORDER BY title, id")).map_err(err)?;
    let rows: Vec<(i64, String, Option<String>)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut out = vec![];
    for same in repeats(rows.into_iter().map(|w| (same_form(&w.1), w)).collect()) {
        // two named people who differ are two works: only those matching another (or with no name) count
        let form = |w: &(i64, String, Option<String>)| w.2.as_deref().map(same_form).filter(|p| !p.is_empty());
        for w in same.iter().filter(|w| same.iter().any(|o| o.0 != w.0 && (form(o).is_none() || form(w).is_none() || form(o) == form(w)))) {
            out.push(hit(module, kind, w.0, w.1.clone(), w.2.clone(), None, None));
        }
    }
    Ok(out)
}

fn duplicate_list_items(conn: &Connection, shown: &dyn Fn(&str) -> bool) -> Result<Vec<Hit>, String> {
    let mut st = conn.prepare("SELECT i.id, i.name, i.module_id, m.name FROM list_items i JOIN custom_modules m ON m.id = i.module_id ORDER BY i.module_id, i.name, i.id").map_err(err)?;
    let rows: Vec<(i64, String, i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    let mut out = vec![];
    for same in repeats(rows.into_iter().filter(|i| shown(&format!("list/{}", i.2))).map(|i| (format!("{}:{}", i.2, same_form(&i.1)), i)).collect()) {
        out.extend(same.into_iter().map(|i| hit(&format!("list/{}", i.2), "list_item", i.0, i.1, Some(i.3), None, None)));
    }
    Ok(out)
}

fn overdue_tasks(conn: &Connection, today: &str) -> Result<Vec<Hit>, String> {
    let mut st = conn
        .prepare(
            "SELECT t.id, t.title, coalesce(go.name, gr.name), CASE WHEN t.goal_id IS NULL THEN t.group_id ELSE -t.goal_id END, t.due_date, CAST(julianday(?1) - julianday(t.due_date) AS INTEGER)
             FROM todo_tasks t JOIN todo_groups gr ON gr.id = t.group_id LEFT JOIN todo_goals go ON go.id = t.goal_id
             WHERE NOT t.done AND t.due_date IS NOT NULL AND julianday(?1) - julianday(t.due_date) > ?2 AND (go.id IS NULL OR go.status = 'active')
             ORDER BY t.due_date, t.id",
        )
        .map_err(err)?;
    let rows = st
        .query_map(rusqlite::params![today, OVERDUE_DAYS], |r| {
            let (where_, days): (String, i64) = (r.get(2)?, r.get(5)?);
            Ok(hit("todo", "task", r.get(0)?, r.get(1)?, Some(format!("{where_} · 过期 {days} 天")), r.get(3)?, r.get(4)?))
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

fn expired_documents(conn: &Connection, today: &str) -> Result<Vec<Hit>, String> {
    let mut st = conn.prepare("SELECT id, name, date FROM days WHERE kind = 'document' AND date < ?1 AND date NOT LIKE '--%' ORDER BY date, id").map_err(err)?;
    let rows = st
        .query_map([today], |r| {
            let date: String = r.get(2)?;
            Ok(hit("days", "day", r.get(0)?, r.get(1)?, Some(format!("{date} 到期")), None, None))
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

fn stale_jobs(conn: &Connection, today: &str) -> Result<Vec<Hit>, String> {
    let mut list: Vec<jobs::Job> = jobs::jobs(conn, today)?.into_iter().filter(|j| j.stage == Stage::Applied && j.waiting_days.is_some_and(|d| d >= STALE_JOB_DAYS)).collect();
    list.sort_by_key(|j| std::cmp::Reverse(j.waiting_days));
    Ok(list
        .into_iter()
        .map(|j| {
            let title = match &j.position {
                Some(p) => format!("{} · {p}", j.company),
                None => j.company.clone(),
            };
            hit("jobs", "job", j.id, title, Some(format!("等了 {} 天", j.waiting_days.unwrap_or(0))), None, None)
        })
        .collect())
}

fn stale_accounts(conn: &Connection, today: &str) -> Result<Vec<Hit>, String> {
    let mut st = conn
        .prepare(
            "SELECT a.id, a.name, b.day FROM fin_accounts a LEFT JOIN (SELECT account_id, max(day) AS day FROM fin_balances GROUP BY account_id) b ON b.account_id = a.id
             WHERE NOT a.closed AND (b.day IS NULL OR julianday(?1) - julianday(b.day) > ?2) ORDER BY b.day, a.sort_order",
        )
        .map_err(err)?;
    let rows = st
        .query_map(rusqlite::params![today, STALE_BALANCE_DAYS], |r| {
            let day: Option<String> = r.get(2)?;
            Ok(hit("finance", "account", r.get(0)?, r.get(1)?, Some(day.map_or("还没记过余额".into(), |d| format!("上次记在 {d}"))), None, None))
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Every check that has a module shown (empty groups too: the page says it found nothing).
pub fn check(conn: &Connection, dir: &Path, today: &str, hidden: &[String]) -> Result<Vec<Group>, String> {
    let shown = |m: &str| !hidden.iter().any(|h| h == m);
    let mut out = vec![];
    if shown("finance") {
        out.push(group("uncategorized", "没分类的收支".into(), uncategorized(conn)?));
    }
    if shown("notes") || shown("todo") {
        out.push(group("attachments", "找不到的附件".into(), missing_attachments(conn, dir, &shown)?));
    }
    let mut twins = vec![];
    if shown("books") {
        twins.extend(duplicate_works(conn, "books", "book", "books", "author")?);
    }
    if shown("films") {
        twins.extend(duplicate_works(conn, "films", "film", "films", "director")?);
    }
    twins.extend(duplicate_list_items(conn, &shown)?);
    out.push(group("duplicates", "可能重复的书影和清单条目".into(), twins));
    if shown("todo") {
        out.push(group("overdue", format!("过期 {OVERDUE_DAYS} 天以上的待办"), overdue_tasks(conn, today)?));
    }
    if shown("days") {
        out.push(group("documents", "过期的证件".into(), expired_documents(conn, today)?));
    }
    if shown("jobs") {
        out.push(group("jobs", format!("等回音 {STALE_JOB_DAYS} 天以上的投递"), stale_jobs(conn, today)?));
    }
    if shown("finance") {
        out.push(group("accounts", format!("{STALE_BALANCE_DAYS} 天没记余额的账户"), stale_accounts(conn, today)?));
    }
    Ok(out)
}

#[tauri::command]
pub fn health_check(state: State<AppState>) -> Result<Vec<Group>, String> {
    let dir = attachments::folder_of(&state.paths.root);
    state.with_db(|c| check(c, &dir, &todo::today(c)?, &data::hidden(c)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::{memory_db, temp_dir};
    use rusqlite::params;

    const TODAY: &str = "2026-11-03";

    fn items<'a>(gs: &'a [Group], key: &str) -> Vec<&'a str> {
        gs.iter().find(|g| g.key == key).unwrap().items.iter().map(|h| h.title.as_str()).collect()
    }

    #[test]
    fn each_check_finds_only_what_is_wrong() {
        let c = memory_db();
        let dir = temp_dir("health");
        // 收支: no 分类 yes; with 分类 no; a refund of a known expense no
        c.execute("INSERT INTO fin_entries (id, flow, amount, currency, day, note) VALUES (1, 'out', 1250, 'CNY', '2026-10-02', '没分类的午饭')", []).unwrap();
        let k: i64 = c.query_row("SELECT id FROM kinds WHERE module = 'fin_out' LIMIT 1", [], |r| r.get(0)).unwrap();
        c.execute("INSERT INTO fin_entries (id, flow, amount, currency, day, kind_id, note) VALUES (2, 'out', 100, 'CNY', '2026-10-02', ?1, '有分类')", [k]).unwrap();
        c.execute("INSERT INTO fin_entries (id, flow, amount, currency, day, is_refund, refund_of, note) VALUES (3, 'out', 50, 'CNY', '2026-10-03', 1, 2, '退的')", []).unwrap();
        // 附件: a note's copy gone, another's still there
        c.execute("INSERT INTO notes (id, content, created_at) VALUES (1, '第一条笔记\n第二行', '2026-10-01T00:00:00.000Z'), (2, '好的笔记', '2026-10-01T00:00:00.001Z')", []).unwrap();
        std::fs::write(dir.join("here.png"), b"x").unwrap();
        c.execute("INSERT INTO attachment_files (id, name, stored, kind, bytes, sha256) VALUES (1, '不见了.png', 'gone.png', 'image', 1, 'a'), (2, '在.png', 'here.png', 'image', 1, 'b'), (3, '别处.pdf', NULL, 'file', 0, 'c')", []).unwrap();
        c.execute("INSERT INTO attachments (owner, record_id, file_id, sort_order) VALUES ('note', 1, 1, 0), ('note', 2, 2, 0), ('note', 2, 3, 1)", []).unwrap();
        // 书影: the same book twice (spaces, case and a missing author aside); two authors, two books
        c.execute("INSERT INTO books (title, author, status) VALUES ('三体', '刘慈欣', 'done'), ('三 体', NULL, 'todo'), ('Hamlet', 'Shakespeare', 'done'), ('hamlet', 'Someone Else', 'done'), ('红楼梦', '曹雪芹', 'done')", []).unwrap();
        c.execute("INSERT INTO films (title, director, status) VALUES ('Heat', 'Michael Mann', 'done'), ('HEAT!', 'michael mann', 'done')", []).unwrap();
        // 清单: same name inside one list; the same name in another list doesn't count
        c.execute("INSERT INTO custom_modules (id, template, name, icon) VALUES (1, 'list', '餐厅', 'star'), (2, 'list', '别的', 'star')", []).unwrap();
        c.execute("INSERT INTO list_items (module_id, name, data, sort_order) VALUES (1, '小馆', '{}', 0), (1, '小馆', '{}', 1), (2, '小馆', '{}', 0)", []).unwrap();
        // 待办: long overdue yes; a little overdue, done, no date, in a paused goal no
        c.execute("INSERT INTO todo_groups (id, name, sort_order) VALUES (1, '生活', 0)", []).unwrap();
        c.execute("INSERT INTO todo_goals (id, group_id, name, status, sort_order) VALUES (1, 1, '暂停的目标', 'paused', 0)", []).unwrap();
        for (title, due, done, goal) in [("拖了很久", Some("2026-09-01"), false, None), ("刚过期", Some("2026-10-30"), false, None), ("做完了", Some("2026-01-01"), true, None), ("没日期", None, false, None), ("暂停的", Some("2026-01-01"), false, Some(1i64))] {
            c.execute("INSERT INTO todo_tasks (group_id, goal_id, title, task_type, due_date, done, sort_order) VALUES (1, ?1, ?2, 'normal', ?3, ?4, 0)", params![goal, title, due, done]).unwrap();
        }
        // 证件, 账户
        c.execute("INSERT INTO days (kind, name, date, repeat, remind) VALUES ('document', '过期的护照', '2026-01-01', 'none', ''), ('document', '有效的签证', '2027-01-01', 'none', ''), ('day', '纪念日', '2020-01-01', 'year', '')", []).unwrap();
        c.execute("INSERT INTO fin_accounts (id, name, kind, currency, closed, sort_order) VALUES (1, '老账户', 'bank', 'CNY', 0, 0), (2, '新账户', 'bank', 'CNY', 0, 1), (3, '没记过', 'bank', 'CNY', 0, 2), (4, '停用的', 'bank', 'CNY', 1, 3)", []).unwrap();
        c.execute("INSERT INTO fin_balances (account_id, day, amount) VALUES (1, '2026-01-01', 1), (2, '2026-10-30', 1)", []).unwrap();
        // 投递: waiting long, waiting a little
        let applied: i64 = c.query_row("SELECT id FROM job_statuses WHERE stage = 'applied' LIMIT 1", [], |r| r.get(0)).unwrap();
        c.execute("INSERT INTO jobs (id, company, position) VALUES (1, '久等的公司', '产品'), (2, '刚投的公司', NULL)", []).unwrap();
        c.execute("INSERT INTO job_steps (job_id, status_id, done, done_on) VALUES (1, ?1, 1, '2026-08-01'), (2, ?1, 1, '2026-10-30')", [applied]).unwrap();

        let g = check(&c, &dir, TODAY, &[]).unwrap();
        assert_eq!(items(&g, "uncategorized"), ["没分类的午饭"]);
        let e = &g[0].items[0];
        assert_eq!((e.module.as_str(), e.kind, e.day.as_deref(), e.detail.as_deref()), ("finance", "entry", Some("2026-10-02"), Some("2026-10-02 · 支出 12.50 CNY")));
        assert_eq!(items(&g, "attachments"), ["第一条笔记", "好的笔记"]); // its copy is gone; a file not on this Mac
        assert_eq!(items(&g, "duplicates"), ["三 体", "三体", "HEAT!", "Heat", "小馆", "小馆"]);
        assert_eq!(items(&g, "overdue"), ["拖了很久"]);
        assert_eq!(g.iter().find(|x| x.key == "overdue").unwrap().items[0].detail.as_deref(), Some("生活 · 过期 63 天"));
        assert_eq!(items(&g, "documents"), ["过期的护照"]);
        assert_eq!(items(&g, "jobs"), ["久等的公司 · 产品"]);
        assert_eq!(items(&g, "accounts"), ["没记过", "老账户"]);
        // hidden modules are left out, a made list by its own key
        let hidden: Vec<String> = ["finance", "books", "todo", "list/1"].map(String::from).to_vec();
        let g = check(&c, &dir, TODAY, &hidden).unwrap();
        assert!(!g.iter().any(|x| ["uncategorized", "overdue", "accounts"].contains(&x.key)));
        assert_eq!(items(&g, "duplicates"), ["HEAT!", "Heat"]);
        assert_eq!(items(&g, "attachments"), ["第一条笔记", "好的笔记"]);
    }

    #[test]
    fn a_long_group_says_how_many_it_cut() {
        let c = memory_db();
        c.execute("INSERT INTO todo_groups (id, name, sort_order) VALUES (1, '生活', 0)", []).unwrap();
        for i in 0..120 {
            c.execute("INSERT INTO todo_tasks (group_id, title, task_type, due_date, sort_order) VALUES (1, ?1, 'normal', '2020-01-01', 0)", [format!("任务 {i}")]).unwrap();
        }
        let g = check(&c, &temp_dir("health-long"), TODAY, &[]).unwrap();
        let o = g.iter().find(|x| x.key == "overdue").unwrap();
        assert_eq!((o.total, o.items.len()), (120, SHOWN));
    }
}
