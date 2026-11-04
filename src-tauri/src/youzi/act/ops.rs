//! What a card item does when confirmed (`Op`, saved as `card_items.input`) and how a record reads on
//! the card. `apply` is the only way a card writes: the same call runs in the rolled-back trial when
//! proposing and in the real write, through each module's own functions (the rules of doing it by hand).
//! - `fingerprint`: an edited record (with its logs / steps / items) as it is, to tell later whether the
//!   edit was taken back (⌘Z restores it exactly). A new record that is gone was taken back too.
//! - `show`: the record's lines on the card; an edit shows the changed ones as 改前 → 改后 and the
//!   lines that name the record (`IDENTITY`), a new one shows everything filled in.

use super::Field;
use crate::modules::common::{err, Tx};
use crate::modules::{books, drawing, films, finance, jobs, kinds, notes, packing, todo};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    AddEntry { patch: Value },
    EditEntry { id: i64, patch: Value },
    SetBalance { account: i64, day: String, amount: i64 },
    /// A subtask when the patch has `parentId`.
    AddTask { patch: Value },
    /// `done`: the list's circle (完成 / 打卡 / 进度 +1); `log`: progress + n today.
    EditTask {
        id: i64,
        #[serde(default)]
        patch: Option<Value>,
        #[serde(default)]
        done: bool,
        #[serde(default)]
        log: Option<i64>,
    },
    AddJob { patch: Value },
    EditJob { id: i64, patch: Value },
    AddWork { module: String, patch: Value },
    EditWork { module: String, id: i64, patch: Value },
    EditLesson { id: i64, patch: Value },
    AddTrip { patch: Value, template: Option<i64> },
    /// New items by name; trip item ids to mark packed.
    EditTrip { id: i64, add: Vec<String>, pack: Vec<i64> },
    AddNote { patch: Value },
    EditNote { id: i64, patch: Value },
}

/// A record a card wrote or will change, opened like a 全局搜索 hit.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    pub module: String,
    pub kind: String,
    pub id: i64,
    pub parent: Option<i64>,
    pub day: Option<String>,
    /// Not there any more, or (an edit) back to how it was: ⌘Z, deleted.
    pub gone: bool,
    /// An edit: the record's fingerprint before it.
    #[serde(skip)]
    pub before: Option<String>,
}

fn rec(module: &str, kind: &str, id: i64, parent: Option<i64>, day: Option<String>) -> Written {
    Written { module: module.into(), kind: kind.into(), id, parent, day, gone: false, before: None }
}

fn task_rec(t: &todo::Task) -> Written {
    rec("todo", "task", t.id, Some(t.goal_id.map_or(t.group_id, |g| -g)), t.due_date.clone())
}

pub fn work_kind(module: &str) -> &'static str {
    if module == "books" { "book" } else { "film" }
}

fn patch<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("参数不对：{e}"))
}

impl Op {
    /// The record an edit changes (kind, id); None for a new one.
    pub fn target(&self) -> Option<(&'static str, i64)> {
        Some(match self {
            Op::EditEntry { id, .. } => ("entry", *id),
            Op::SetBalance { account, .. } => ("account", *account),
            Op::EditTask { id, .. } => ("task", *id),
            Op::EditJob { id, .. } => ("job", *id),
            Op::EditWork { module, id, .. } => (work_kind(module), *id),
            Op::EditLesson { id, .. } => ("lesson", *id),
            Op::EditTrip { id, .. } => ("trip", *id),
            Op::EditNote { id, .. } => ("note", *id),
            _ => return None,
        })
    }

    /// The edited record as a card line source (a balance reads on its day).
    pub fn before(&self, c: &Connection) -> Result<Option<Written>, String> {
        let Some((kind, id)) = self.target() else { return Ok(None) };
        Ok(Some(match self {
            Op::SetBalance { day, .. } => rec("finance", "account", id, None, Some(day.clone())),
            Op::EditTask { .. } => task_rec(&todo::task(c, id)?),
            Op::EditLesson { .. } => rec("drawing", "lesson", id, Some(drawing::lesson(c, id)?.course_id), None),
            Op::EditWork { module, .. } => rec(module, kind, id, None, None),
            _ => rec(module_of(kind), kind, id, None, None),
        }))
    }
}

fn module_of(kind: &str) -> &'static str {
    match kind {
        "entry" | "account" => "finance",
        "task" => "todo",
        "job" => "jobs",
        "book" => "books",
        "film" => "films",
        "lesson" => "drawing",
        "trip" => "packing",
        _ => "notes",
    }
}

/// Writes one item with the module's own functions; the record it wrote.
pub fn apply(c: &Connection, op: &Op, today: &str) -> Result<Written, String> {
    Ok(match op {
        Op::AddEntry { patch: p } => {
            let e = finance::create_entry(c, patch(p)?)?;
            rec("finance", "entry", e.id, None, Some(e.day))
        }
        Op::EditEntry { id, patch: p } => {
            let e = finance::update_entry(c, *id, patch(p)?)?;
            rec("finance", "entry", e.id, None, Some(e.day))
        }
        Op::SetBalance { account, day, amount } => {
            finance::save_balances(c, day, &[finance::BalanceInput { account_id: *account, amount: Some(*amount) }], &[])?;
            rec("finance", "account", *account, None, Some(day.clone()))
        }
        Op::AddTask { patch: p } => task_rec(&todo::create_task_on(c, patch(p)?, today)?),
        Op::EditTask { id, patch: p, done, log } => {
            let tx = Tx::begin(c)?;
            if let Some(p) = p {
                todo::update_task_on(&tx, *id, patch(p)?, today)?;
            }
            if *done {
                todo::set_done_on(&tx, *id, true, today)?;
            }
            if let Some(n) = log {
                todo::log_on(&tx, *id, today, *n, today)?;
            }
            tx.commit()?;
            task_rec(&todo::task(c, *id)?)
        }
        Op::AddJob { patch: p } => rec("jobs", "job", jobs::create_job(c, patch(p)?, today)?.id, None, None),
        Op::EditJob { id, patch: p } => rec("jobs", "job", jobs::update_job(c, *id, patch(p)?, today)?.id, None, None),
        Op::AddWork { module, patch: p } => {
            let id = if module == "books" { books::create(c, patch(p)?)?.id } else { films::create(c, patch(p)?)?.id };
            rec(module, work_kind(module), id, None, None)
        }
        Op::EditWork { module, id, patch: p } => {
            if module == "books" {
                books::update(c, *id, patch(p)?)?;
            } else {
                films::update(c, *id, patch(p)?)?;
            }
            rec(module, work_kind(module), *id, None, None)
        }
        Op::EditLesson { id, patch: p } => {
            let l = drawing::update_lesson(c, *id, patch(p)?)?;
            rec("drawing", "lesson", l.id, Some(l.course_id), None)
        }
        Op::AddTrip { patch: p, template } => rec("packing", "trip", packing::create_trip(c, patch(p)?, *template)?.id, None, None),
        Op::EditTrip { id, add, pack } => {
            packing::trip(c, *id)?;
            let tx = Tx::begin(c)?;
            for name in add {
                packing::create_item(&tx, packing::List::Trip, *id, packing::ItemPatch { name: Some(name.clone()), ..Default::default() })?;
            }
            for item in pack {
                if packing::item(&tx, packing::List::Trip, *item)?.parent_id != *id {
                    return Err("这件物品不在这次出行里".into());
                }
                packing::update_item(&tx, packing::List::Trip, *item, packing::ItemPatch { packed: Some(true), ..Default::default() })?;
            }
            tx.commit()?;
            rec("packing", "trip", *id, None, None)
        }
        Op::AddNote { patch: p } => rec("notes", "note", notes::create(c, patch(p)?)?.id, None, None),
        Op::EditNote { id, patch: p } => rec("notes", "note", notes::update(c, *id, patch(p)?)?.id, None, None),
    })
}

// ---------- fingerprints ----------

/// The record's rows (first query: the record itself; None when it is gone), hashed (FNV-1a).
pub fn fingerprint(c: &Connection, kind: &str, id: i64) -> Option<String> {
    let sqls: &[&str] = match kind {
        "entry" => &["SELECT * FROM fin_entries WHERE id = ?1"],
        "account" => &["SELECT id FROM fin_accounts WHERE id = ?1", "SELECT * FROM fin_balances WHERE account_id = ?1 ORDER BY day"],
        "task" => &["SELECT * FROM todo_tasks WHERE id = ?1", "SELECT * FROM todo_logs WHERE task_id = ?1 ORDER BY id"],
        "job" => &["SELECT * FROM jobs WHERE id = ?1", "SELECT * FROM job_steps WHERE job_id = ?1 ORDER BY id"],
        "book" => &["SELECT * FROM books WHERE id = ?1", "SELECT * FROM book_rereads WHERE book_id = ?1 ORDER BY id"],
        "film" => &["SELECT * FROM films WHERE id = ?1", "SELECT * FROM film_rereads WHERE film_id = ?1 ORDER BY id"],
        "lesson" => &["SELECT * FROM drawing_lessons WHERE id = ?1"],
        "trip" => &["SELECT * FROM trips WHERE id = ?1", "SELECT * FROM trip_items WHERE trip_id = ?1 ORDER BY id"],
        "note" => &["SELECT * FROM notes WHERE id = ?1"],
        _ => return None,
    };
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: &[u8]| {
        for x in b {
            h = (h ^ *x as u64).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for (i, sql) in sqls.iter().enumerate() {
        let mut st = c.prepare(sql).ok()?;
        let n = st.column_count();
        let mut rows = st.query([id]).ok()?;
        let mut any = false;
        while let Ok(Some(r)) = rows.next() {
            any = true;
            for k in 0..n {
                eat(format!("{:?}\u{1f}", r.get_ref(k).ok()?).as_bytes());
            }
            eat(b"\n");
        }
        if i == 0 && !any {
            return None;
        }
    }
    Some(format!("{h:016x}"))
}

// ---------- how a record reads ----------

/// One line of a record on the card (None = empty).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub label: &'static str,
    pub value: Option<String>,
    /// Long text (shown with its changes marked).
    pub long: bool,
}

fn line(label: &'static str, value: Option<String>) -> Line {
    Line { label, value, long: false }
}

fn slash(d: &str) -> String {
    d.replace('-', "/")
}

fn status_label(module: &str, s: crate::modules::common::Status) -> Option<String> {
    use crate::modules::common::Status;
    let verb = match module {
        "books" => "读",
        "films" => "看",
        _ => "画",
    };
    match s {
        Status::Unmarked => None,
        Status::Todo => Some(format!("想{verb}")),
        Status::Doing => Some(format!("在{verb}")),
        Status::Done => Some(format!("{verb}完了")),
    }
}

fn when(t: &todo::Task) -> Option<String> {
    match (&t.task_type, &t.start_date, &t.due_date) {
        (todo::TaskType::Repeat, _, Some(d)) => Some(format!("下一次 {}", slash(d))),
        (_, Some(s), Some(d)) => Some(format!("{} 至 {}", slash(s), slash(d))),
        (_, Some(s), None) => Some(format!("{} 起", slash(s))),
        (_, None, Some(d)) => Some(format!("截止 {}", slash(d))),
        _ => None,
    }
}

fn task_state(t: &todo::Task) -> String {
    match t.task_type {
        _ if t.done => "已完成".into(),
        todo::TaskType::Normal => "没完成".into(),
        todo::TaskType::Repeat => format!("做了 {} 次", t.count),
        todo::TaskType::Checkin => format!("打卡 {} 天{}", t.count, if t.logged_today { "，今天打过了" } else { "" }),
        todo::TaskType::Progress => match t.target {
            Some(n) => format!("{} / {n}{}", t.count, t.unit.as_deref().unwrap_or("")),
            None => format!("{}{}", t.count, t.unit.as_deref().unwrap_or("")),
        },
    }
}

/// The record's lines on the card (`fresh`: just made — a new task's 没完成 isn't worth a line).
pub fn show(c: &Connection, w: &Written, today: &str, fresh: bool) -> Result<Vec<Line>, String> {
    Ok(match w.kind.as_str() {
        "entry" => {
            let e = finance::entry(c, w.id)?;
            let inflow = e.flow == finance::Flow::In;
            let names = kinds::name_map(c, if inflow { finance::IN_KINDS } else { finance::OUT_KINDS })?;
            let day = match &e.end_day {
                Some(end) => format!("{} 至 {}", slash(&e.day), slash(end)),
                None => slash(&e.day),
            };
            vec![
                line("收支", Some(if inflow { "收入" } else { "支出" }.into())),
                line("金额", Some(super::super::data::money(e.amount, e.currency))),
                line("日期", Some(day)),
                line("分类", Some(e.kind_id.and_then(|k| names.get(&k).cloned()).unwrap_or_else(|| "未分类".into()))),
                line("备注", e.note),
            ]
        }
        "account" => {
            let a = finance::account(c, w.id)?;
            let day = w.day.clone().unwrap_or_else(|| today.to_string());
            let last: Option<(String, i64)> = c
                .query_row("SELECT day, amount FROM fin_balances WHERE account_id = ?1 AND day <= ?2 ORDER BY day DESC LIMIT 1", rusqlite::params![w.id, day], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()
                .map_err(err)?;
            let balance = last.map(|(d, n)| {
                let m = super::super::data::money(n, a.currency);
                if d == day { m } else { format!("{m}（{}）", slash(&d)) }
            });
            vec![line("账户", Some(a.name)), line("日期", Some(slash(&day))), line("余额", balance)]
        }
        "task" => {
            let t = todo::task_view(c, w.id)?;
            let mut place = vec![todo::group(c, t.group_id)?.name];
            if let Some(g) = t.goal_id {
                place.push(todo::goal(c, g)?.name);
            }
            if let Some(p) = t.parent_id {
                place.push(todo::task(c, p)?.title);
            }
            let mut v = vec![line("任务", Some(t.title.clone())), line("在", Some(place.join(" / "))), line("时间", when(&t)), Line { label: "内容", value: t.note.clone(), long: true }];
            if !fresh {
                v.push(line("状态", Some(task_state(&t))));
            }
            v
        }
        "job" => {
            let j = jobs::job(c, w.id, today)?;
            let sts = jobs::statuses(c)?;
            let channels = kinds::name_map(c, jobs::CHANNELS)?;
            let progress = j.status_id.and_then(|s| sts.iter().find(|x| x.id == s)).map(|s| {
                let day = j.steps.iter().find(|x| x.status_id == s.id && x.done).and_then(|x| x.done_on.clone());
                [Some(s.name.clone()), day.map(|d| slash(&d))].into_iter().flatten().collect::<Vec<_>>().join(" · ")
            });
            vec![
                line("公司", Some(j.company)),
                line("岗位", j.position),
                line("地点", j.location),
                line("渠道", j.kind_id.and_then(|k| channels.get(&k).cloned())),
                line("进度", progress),
                Line { label: "备注", value: j.note, long: true },
            ]
        }
        "book" | "film" => {
            let module = if w.kind == "book" { "books" } else { "films" };
            let (title, creator, country, kind_id, status, rating, year) = if w.kind == "book" {
                let b = books::get(c, w.id)?;
                (b.title, b.author, b.country, b.kind_id, b.status, b.rating, b.finished_year)
            } else {
                let f = films::get(c, w.id)?;
                (f.title, f.director, f.country, f.kind_id, f.status, f.rating, f.finished_year)
            };
            let names = kinds::name_map(c, module)?;
            let book = w.kind == "book";
            vec![
                line("名称", Some(title)),
                line(if book { "作者" } else { "导演" }, creator),
                line("国家", country),
                line("类型", kind_id.and_then(|k| names.get(&k).cloned())),
                line("状态", status_label(module, status)),
                line("星级", rating.map(|r| format!("{r} 星"))),
                line(if book { "读完" } else { "看完" }, year.map(|y| format!("{y} 年"))),
            ]
        }
        "lesson" => {
            let l = drawing::lesson(c, w.id)?;
            let name = [l.code.clone(), Some(l.title.clone())].into_iter().flatten().collect::<Vec<_>>().join(" ");
            vec![
                line("课程", Some(drawing::course(c, l.course_id)?.name)),
                line("课节", Some(name)),
                line("状态", status_label("drawing", l.status)),
                line("画完", l.finished_month.map(|m| slash(&m))),
            ]
        }
        "trip" => {
            let t = packing::trip(c, w.id)?;
            vec![
                line("出行", Some(t.name)),
                line("出发", t.depart_on.map(|d| slash(&d))),
                line("模板", t.template_name),
                line("物品", Some(format!("共 {} 件 · 装好 {} 件", t.total, t.packed))),
            ]
        }
        "note" => vec![Line { label: "内容", value: Some(notes::get(c, w.id)?.content), long: true }],
        k => return Err(format!("不认识的记录：{k}")),
    })
}

/// The lines that name a record (kept on an edit even when unchanged).
const IDENTITY: &[(&str, &[&str])] = &[
    ("entry", &["日期", "备注", "金额"]),
    ("account", &["账户"]),
    ("task", &["任务"]),
    ("job", &["公司", "岗位"]),
    ("book", &["名称"]),
    ("film", &["名称"]),
    ("lesson", &["课程", "课节"]),
    ("trip", &["出行"]),
];

fn value<'a>(lines: &'a [Line], label: &str) -> Option<&'a str> {
    lines.iter().find(|l| l.label == label).and_then(|l| l.value.as_deref())
}

/// A short name for the record (option labels, the model's answer, exports).
pub fn name_of(kind: &str, lines: &[Line]) -> String {
    let v = |l: &str| value(lines, l);
    let s = match kind {
        "entry" => {
            let day = v("日期").map(|d| d.get(5..).unwrap_or(d).to_string());
            [day.as_deref(), v("备注").or(v("分类")), v("金额")].into_iter().flatten().collect::<Vec<_>>().join(" ")
        }
        "account" => v("账户").unwrap_or_default().to_string(),
        "task" => v("任务").unwrap_or_default().to_string(),
        "job" => [v("公司"), v("岗位")].into_iter().flatten().collect::<Vec<_>>().join(" · "),
        "lesson" => [v("课程"), v("课节")].into_iter().flatten().collect::<Vec<_>>().join(" · "),
        "trip" => v("出行").unwrap_or_default().to_string(),
        "note" => {
            let t = v("内容").unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
            let short: String = t.chars().take(30).collect();
            if short.chars().count() < t.chars().count() { format!("{short}…") } else { short }
        }
        _ => v("名称").map(|n| if kind == "book" { format!("《{n}》") } else { n.to_string() }).unwrap_or_default(),
    };
    s.replace('\n', " ")
}

const EMPTY: &str = "（空）";

/// The card's fields: everything filled in (new), or what changes plus what names it (edit).
pub fn fields(kind: &str, before: Option<&[Line]>, after: &[Line]) -> Vec<Field> {
    let Some(before) = before else {
        return after.iter().filter_map(|l| l.value.clone().map(|v| Field { label: l.label.into(), before: None, after: v, long: l.long })).collect();
    };
    let names = IDENTITY.iter().find(|(k, _)| *k == kind).map_or(&[][..], |(_, n)| *n);
    after
        .iter()
        .filter_map(|l| {
            let b = value(before, l.label);
            let a = l.value.as_deref();
            if b != a {
                Some(Field { label: l.label.into(), before: Some(b.unwrap_or(EMPTY).into()), after: a.unwrap_or(EMPTY).into(), long: l.long })
            } else {
                a.filter(|_| names.contains(&l.label)).map(|a| Field { label: l.label.into(), before: None, after: a.into(), long: l.long })
            }
        })
        .collect()
}

/// What changes, in a line: 「金额 ¥35 → ¥40；分类 餐饮 → 交通」 (long text: 「内容改了」).
pub fn changes(fields: &[Field]) -> String {
    fields
        .iter()
        .filter_map(|f| {
            let b = f.before.as_deref()?;
            Some(if f.long { format!("{}改了", f.label) } else { format!("{} {b} → {}", f.label, f.after) })
        })
        .collect::<Vec<_>>()
        .join("；")
}
