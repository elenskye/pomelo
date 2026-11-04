//! Atlas's groundwork (v0.8; the journey itself is v0.9 — 第十四轮问卷, requirements「以后 · Atlas」): each
//! 年度总结 keeps its 「呈现结果」 (youzi.db `year_atlas`), written once right after the letter:
//! - `Snapshot` (Rust): the year's numbers and last year's up to the same day, the 之最, each day's
//!   records (heatmap / star map), check-ins, 钱来钱往 (data.rs `year_money`), the records the scenes
//!   may show (`Record`, keyed `note:12` …: notes in full, goals done, trips, offers, 来日方长's days,
//!   books / films, courses, conversations with 柚子) and the days both years have notes on (`Echo`).
//!   Only what this run could read (hidden modules, 「发给云端」), kept as it was then.
//! - The scenes (the same model, after the letter): which of the offered scenes to tell (a scene is
//!   offered only when there's something for it; 序章 and 终章 always), a line or two each and the
//!   records it quotes, only from the snapshot (回声: one note of each year, same day). Words only:
//!   numbers, dates and ranks are Rust's. Unknown scenes / records are dropped; scenes keep SCENES' order.
//! - Never fails the letter: a failed scenes step keeps the snapshot with the error.
//! - Until Atlas's page exists, shown folded under the letter (`preview`).

use super::context::{self, Counter};
use super::data::{self, Access, YearMoney};
use super::provider::{Msg, Target};
use super::yearly::{Facts, Sink};
use crate::modules::common::err;
use crate::modules::{days, jobs, todo};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Starts the scenes' system message (the mock model answers by it).
pub const MARK: &str = "【Atlas · 各幕】";

/// (key, title, what it shows), in the journey's order.
pub const SCENES: &[(&str, &str, &str)] = &[
    ("prologue", "序章", "生活星图、年份、一句开场"),
    ("days", "留下的日子", "一年的笔记热力图，配一句原话"),
    ("works", "书影相遇", "读完的书、看完的电影和剧"),
    ("moments", "几个具体时刻", "完成的目标、出行、拿到的 Offer、来日方长的重要日子，沿时间排"),
    ("echo", "去年的回声", "两年同一天写下的话，和去年比"),
    ("drawing", "创作", "画完的课程和课节"),
    ("jobs", "求职这一年", "投了几份、走到了哪一步"),
    ("habits", "身体和习惯", "打卡的天数、最长的连续"),
    ("talks", "和柚子聊的事", "这一年常聊的话题"),
    ("money", "钱", "这一年花在哪、总资产怎么变"),
    ("finale", "终章", "柚子的来信"),
];

/// The reply's room (thinking extra).
const SCENE_TOKENS: i64 = 4000;
/// Characters of a note the model sees (the snapshot keeps it whole).
const NOTE_CHARS: usize = 100;
/// Same-day pairs offered at most.
const ECHO_DAYS: usize = 40;
/// Lines per scene, characters per line.
const LINES: usize = 3;
const LINE_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Number {
    pub label: String,
    pub unit: String,
    pub n: i64,
    /// Last year up to the same day (None: nothing to compare with).
    pub last: Option<i64>,
}

/// A day with records: notes, and everything (notes, things done, goals, application steps, trips, conversations).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Heat {
    pub day: String,
    pub notes: i64,
    pub all: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Habit {
    pub title: String,
    pub days: i64,
    pub longest: i64,
}

/// A record a scene may show: `kind` note / goal / trip / offer / day / book / film / course / talk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub key: String,
    pub kind: String,
    pub day: Option<String>,
    pub text: String,
}

/// A month-day (MM-DD) with notes in both years.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Echo {
    pub day: String,
    pub this: Vec<String>,
    pub last: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub year: String,
    pub until: Option<String>,
    pub numbers: Vec<Number>,
    pub extras: Vec<String>,
    pub heat: Vec<Heat>,
    pub habits: Vec<Habit>,
    pub money: Option<YearMoney>,
    pub records: Vec<Record>,
    pub echo: Vec<Echo>,
    /// The scenes there's something for (keys, SCENES' order).
    pub offered: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub key: String,
    pub lines: Vec<String>,
    pub refs: Vec<String>,
}

fn rows<T>(c: &Connection, sql: &str, args: impl rusqlite::Params, f: impl FnMut(&rusqlite::Row) -> rusqlite::Result<T>) -> Result<Vec<T>, String> {
    let mut st = c.prepare(sql).map_err(err)?;
    let out = st.query_map(args, f).map_err(err)?.collect::<Result<_, _>>().map_err(err);
    out
}

/// What pomelo.db gives: records, each day's counts, check-ins, money, same-day pairs.
#[derive(Default)]
struct Pomelo {
    records: Vec<Record>,
    heat: BTreeMap<String, (i64, i64)>,
    habits: Vec<Habit>,
    money: Option<YearMoney>,
    echo: Vec<Echo>,
}

fn rec(kind: &str, id: i64, day: Option<String>, text: String) -> Record {
    Record { key: format!("{kind}:{id}"), kind: kind.into(), day, text }
}

fn pomelo_part(c: &Connection, access: &Access, year: &str, from: &str, to: &str) -> Result<Pomelo, String> {
    let mut p = Pomelo::default();
    let bump = |p: &mut Pomelo, day: &str, note: bool| {
        let e = p.heat.entry(day.to_string()).or_default();
        e.0 += note as i64;
        e.1 += 1;
    };
    if access.allows("notes") {
        let mine = rows(c, "SELECT id, date(created_at, 'localtime'), content FROM notes WHERE date(created_at, 'localtime') BETWEEN ?1 AND ?2 ORDER BY created_at, id", [from, to], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        let prev = (year.parse::<i64>().unwrap_or(1971) - 1).to_string();
        let before = rows(c, "SELECT id, date(created_at, 'localtime'), content FROM notes WHERE date(created_at, 'localtime') BETWEEN ?1 AND ?2 ORDER BY created_at, id", [format!("{prev}-01-01"), format!("{prev}-12-31")], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        let mut pairs: BTreeMap<String, (Vec<String>, Vec<String>)> = BTreeMap::new();
        for (id, day, _) in &mine {
            pairs.entry(day[5..].to_string()).or_default().0.push(format!("note:{id}"));
        }
        let mut last_used = HashSet::new();
        for (id, day, _) in &before {
            if let Some(x) = pairs.get_mut(&day[5..]) {
                x.1.push(format!("note:{id}"));
                last_used.insert(*id);
            }
        }
        p.echo = pairs.into_iter().filter(|(_, (a, b))| !a.is_empty() && !b.is_empty()).take(ECHO_DAYS).map(|(day, (this, last))| Echo { day, this, last }).collect();
        for (id, day, content) in mine {
            bump(&mut p, &day, true);
            p.records.push(rec("note", id, Some(day), content));
        }
        for (id, day, content) in before.into_iter().filter(|x| last_used.contains(&x.0)) {
            p.records.push(rec("note", id, Some(day), content));
        }
    }
    if access.allows("todo") {
        for d in todo::done_between(c, from, to)? {
            bump(&mut p, &d.day, false);
        }
        for (id, day, name) in rows(c, "SELECT id, end_date, name FROM todo_goals WHERE status = 'done' AND end_date BETWEEN ?1 AND ?2 ORDER BY end_date, id", [from, to], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })? {
            bump(&mut p, &day, false);
            p.records.push(rec("goal", id, Some(day), name));
        }
        p.habits = todo::checkins_between(c, from, to)?.into_iter().map(|h| Habit { title: h.title, days: h.days, longest: h.longest }).collect();
    }
    if access.allows("jobs") {
        let statuses = jobs::statuses(c)?;
        for j in jobs::jobs(c, to)? {
            let inside: Vec<(&str, bool)> = j
                .steps
                .iter()
                .filter(|s| s.done)
                .filter_map(|s| s.done_on.as_deref().filter(|d| *d >= from && *d <= to).map(|d| (d, statuses.iter().any(|x| x.id == s.status_id && x.stage == jobs::Stage::Got))))
                .collect();
            for (d, _) in &inside {
                bump(&mut p, d, false);
            }
            if let Some((d, _)) = inside.iter().filter(|x| x.1).max() {
                let title = [Some(j.company.trim()), j.position.as_deref().map(str::trim).filter(|x| !x.is_empty())].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                p.records.push(rec("offer", j.id, Some(d.to_string()), title));
            }
        }
    }
    if access.allows("packing") {
        for (id, day, name) in rows(c, "SELECT id, depart_on, name FROM trips WHERE depart_on BETWEEN ?1 AND ?2 ORDER BY depart_on, id", [from, to], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })? {
            bump(&mut p, &day, false);
            p.records.push(rec("trip", id, Some(day), name));
        }
    }
    if access.allows("days") {
        for h in days::happened(c, from, to)? {
            let nth = data::nth_text(&h);
            let text = if nth.is_empty() { h.name.clone() } else { format!("{}（{nth}）", h.name) };
            p.records.push(rec("day", h.id, Some(h.on.clone()), text));
        }
    }
    for (kind, table, rereads, fk) in [("book", "books", "book_rereads", "book_id"), ("film", "films", "film_rereads", "film_id")] {
        if access.allows(&format!("{kind}s")) {
            let sql = format!("SELECT x.id, x.title FROM {table} x WHERE (x.status = 'done' AND x.finished_year = ?1) OR EXISTS (SELECT 1 FROM {rereads} r WHERE r.{fk} = x.id AND r.year = ?1) ORDER BY x.id");
            for (id, title) in rows(c, &sql, [year], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
                p.records.push(rec(kind, id, None, format!("《{title}》")));
            }
        }
    }
    if access.allows("drawing") {
        let sql = "SELECT c.id, c.name, count(*) FROM drawing_lessons l JOIN drawing_courses c ON c.id = l.course_id
                   WHERE l.status = 'done' AND l.finished_month BETWEEN ?1 AND ?2 GROUP BY c.id ORDER BY c.sort_order, c.id";
        for (id, name, n) in rows(c, sql, [&from[..7], &to[..7]], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))? {
            p.records.push(rec("course", id, None, format!("{name}（这一年画完 {n} 节）")));
        }
    }
    p.money = data::year_money(c, access, from, to)?;
    Ok(p)
}

/// The snapshot of `year` (up to `facts.until`) as this run could read it.
pub(crate) fn snapshot(sink: &impl Sink, access: &Access, year: &str, facts: &Facts) -> Result<Snapshot, String> {
    let (from, to) = (format!("{year}-01-01"), facts.until.clone().unwrap_or_else(|| format!("{year}-12-31")));
    let mut p = sink.with_pomelo(|c| pomelo_part(c, access, year, &from, &to))?;
    let talks = sink.with_youzi(|c| {
        rows(c, "SELECT id, date(created_at, 'localtime'), title FROM sessions WHERE NOT private AND NOT scheduled AND date(created_at, 'localtime') BETWEEN ?1 AND ?2 ORDER BY created_at, id", [&from, &to], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })
    })?;
    for (id, day, title) in talks {
        let e = p.heat.entry(day.clone()).or_default();
        e.1 += 1;
        p.records.push(rec("talk", id, Some(day), title));
    }
    let numbers: Vec<Number> = facts
        .this
        .iter()
        .map(|c| Number { label: c.label.into(), unit: c.unit.into(), n: c.n, last: facts.compare.as_ref().map(|_| facts.last.iter().find(|l| l.label == c.label).map_or(0, |l| l.n)) })
        .collect();
    let has = |kinds: &[&str]| p.records.iter().any(|r| kinds.contains(&r.kind.as_str()) && r.day.as_deref().is_none_or(|d| d >= from.as_str()));
    let count = |label: &str| numbers.iter().find(|n| n.label == label).is_some_and(|n| n.n > 0);
    let offered = SCENES
        .iter()
        .map(|s| s.0)
        .filter(|k| match *k {
            "days" => has(&["note"]),
            "works" => has(&["book", "film"]),
            "moments" => has(&["goal", "trip", "offer", "day"]),
            "echo" => !p.echo.is_empty() || facts.compare.is_some(),
            "drawing" => has(&["course"]),
            "jobs" => count("有进展的投递"),
            "habits" => !p.habits.is_empty(),
            "talks" => has(&["talk"]),
            "money" => p.money.is_some(),
            _ => true,
        })
        .map(String::from)
        .collect();
    Ok(Snapshot {
        year: year.into(),
        until: facts.until.clone(),
        numbers,
        extras: facts.extras.clone(),
        heat: p.heat.into_iter().map(|(day, (notes, all))| Heat { day, notes, all }).collect(),
        habits: p.habits,
        money: p.money,
        records: p.records,
        echo: p.echo,
        offered,
    })
}

/// 「3 月 12 日」
fn md(d: &str) -> String {
    format!("{} 月 {} 日", d[5..7].trim_start_matches('0'), d[8..10].trim_start_matches('0'))
}

fn clip(s: &str, n: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > n { format!("{}…", flat.chars().take(n).collect::<String>()) } else { flat }
}

/// A record as the model sees it.
fn record_line(r: &Record) -> String {
    let day = r.day.as_deref().map(|d| format!("（{} 年 {}）", &d[..4], md(d))).unwrap_or_default();
    format!("{}{day} {}", r.key, if r.kind == "note" { clip(&r.text, NOTE_CHARS) } else { r.text.clone() })
}

/// The request; notes thinned (every `stride`-th, the same-day ones kept) when the year doesn't fit.
fn messages(snap: &Snapshot, facts: &Facts, letter: &str, stride: usize) -> Vec<Msg> {
    let system = format!(
        "{MARK}你是柚子，用户自己的生活记录 App「柚儿园」里的助手。你刚给用户写完 {} 年的年度总结（在最后）。Atlas 会把这一年做成一段往下走的视觉旅程，一幕讲一件事；每一幕的数字、日期、图和列表都由程序画，你只写话、挑记录。\n\
         - 从【可以写的幕】里挑这一年值得讲的，按给的顺序；序章和终章一定要有，没什么可说的幕就不写。\n\
         - 每一幕写 1～2 句话，每句不超过 40 个字，口吻和信一样，对着「你」说；不写数字、日期、排名和百分比（程序会显示），只根据给你的内容，不要编。\n\
         - 每一幕可以从【可以引用的记录】里挑 0～3 条，写编号（像 note:12）：「留下的日子」挑 1 条最能代表这一年的笔记；「几个具体时刻」挑 2～6 条；「去年的回声」从【两年同一天】里挑同一天的今年 1 条、去年 1 条；别的幕挑和它有关的。\n\
         - 只输出 JSON，不要别的文字：{{\"scenes\":[{{\"key\":\"prologue\",\"lines\":[\"……\"],\"refs\":[]}}]}}",
        snap.year
    );
    let echo_keys: HashSet<&str> = snap.echo.iter().flat_map(|e| e.this.iter().chain(&e.last)).map(String::as_str).collect();
    let mut n = 0;
    let lines: Vec<String> = snap
        .records
        .iter()
        .filter(|r| {
            if r.kind != "note" || echo_keys.contains(r.key.as_str()) {
                return true;
            }
            n += 1;
            (n - 1) % stride == 0
        })
        .map(|r| format!("- {}", record_line(r)))
        .collect();
    let scenes: Vec<String> = SCENES.iter().filter(|s| snap.offered.iter().any(|k| k == s.0)).map(|(k, t, what)| format!("- {k} {t}：{what}")).collect();
    let mut parts = vec![format!("【可以写的幕】\n{}", scenes.join("\n"))];
    if !facts.numbers.is_empty() {
        parts.push(format!("【这一年的数字】（程序会显示，你不用写）\n{}", facts.numbers));
    }
    parts.push(format!("【可以引用的记录】\n{}", if lines.is_empty() { "（没有）".into() } else { lines.join("\n") }));
    if !snap.echo.is_empty() {
        parts.push(format!("【两年同一天】\n{}", snap.echo.iter().map(|e| format!("- {}：今年 {}；去年 {}", md(&format!("0000-{}", e.day)), e.this.join("、"), e.last.join("、"))).collect::<Vec<_>>().join("\n")));
    }
    parts.push(format!("【刚写的信】\n{letter}"));
    vec![Msg::new("system", &system), Msg::new("user", &parts.join("\n\n"))]
}

/// The model's scenes, checked against the snapshot.
pub fn parse(reply: &str, snap: &Snapshot) -> Result<Vec<Scene>, String> {
    #[derive(Deserialize)]
    struct Raw {
        scenes: Vec<RawScene>,
    }
    #[derive(Deserialize)]
    struct RawScene {
        key: String,
        #[serde(default)]
        lines: Vec<String>,
        #[serde(default)]
        refs: Vec<String>,
    }
    let bad = || "模型写的各幕读不出来（不是要的 JSON）".to_string();
    let (a, b) = (reply.find('{').ok_or_else(bad)?, reply.rfind('}').ok_or_else(bad)?);
    let raw: Raw = serde_json::from_str(reply.get(a..=b).ok_or_else(bad)?).map_err(|_| bad())?;
    let records: HashMap<&str, &Record> = snap.records.iter().map(|r| (r.key.as_str(), r)).collect();
    let mut out = vec![];
    for (key, _, _) in SCENES.iter().filter(|s| snap.offered.iter().any(|k| k == s.0)) {
        let Some(s) = raw.scenes.iter().find(|s| s.key.trim() == *key) else { continue };
        let lines: Vec<String> = s.lines.iter().map(|l| clip(l.trim(), LINE_CHARS)).filter(|l| !l.is_empty()).take(LINES).collect();
        let mut seen = HashSet::new();
        let mut refs: Vec<String> = s.refs.iter().map(|r| r.trim().to_string()).filter(|r| records.contains_key(r.as_str()) && seen.insert(r.clone())).collect();
        match *key {
            // one note of each year, from the same day
            "echo" => {
                refs = snap
                    .echo
                    .iter()
                    .find_map(|e| {
                        let this = refs.iter().find(|r| e.this.contains(r))?;
                        let last = refs.iter().find(|r| e.last.contains(r))?;
                        Some(vec![this.clone(), last.clone()])
                    })
                    .unwrap_or_default();
            }
            "moments" => {
                refs.retain(|r| ["goal", "trip", "offer", "day"].contains(&records[r.as_str()].kind.as_str()));
                refs.truncate(6);
                refs.sort_by(|x, y| records[x.as_str()].day.cmp(&records[y.as_str()].day));
            }
            _ => refs.truncate(3),
        }
        if !lines.is_empty() || !refs.is_empty() {
            out.push(Scene { key: key.to_string(), lines, refs });
        }
    }
    if out.is_empty() {
        return Err("模型没写出任何一幕。".into());
    }
    Ok(out)
}

async fn size(counter: &Counter, messages: &[Msg]) -> i64 {
    let mut used = context::PROMPT_OVERHEAD;
    for m in messages {
        used += counter.count(&m.content).await + context::PER_MESSAGE;
    }
    used
}

/// Asks the model for the scenes (thinning the notes it sees until the request fits).
async fn ask(sink: &impl Sink, target: &Target, window: i64, thinking: bool, snap: &Snapshot, facts: &Facts, letter: &str) -> Result<Vec<Scene>, String> {
    let counter = Counter::for_target(target);
    let room = SCENE_TOKENS + if thinking { context::think_room(window) } else { 0 };
    let notes = snap.records.iter().filter(|r| r.kind == "note").count();
    let mut stride = 1;
    let request = loop {
        let m = messages(snap, facts, letter, stride);
        if size(&counter, &m).await + room <= window {
            break m;
        }
        if notes / stride < 10 {
            return Err("这一年的记录放不下这个模型的上下文，各幕没写。".into());
        }
        stride *= 2;
    };
    let (out, usage) = super::brief::ask(target, request, room, thinking, None).await?;
    sink.asked(usage);
    parse(&out, snap)
}

/// After the letter: the snapshot, then the scenes; saved either way (a failed step with its error).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn write(sink: &mut impl Sink, target: &Target, model: (&str, &str), window: i64, thinking: bool, access: &Access, year: &str, facts: &Facts, letter: &str) {
    if letter.trim().is_empty() {
        return;
    }
    let snap = match snapshot(sink, access, year, facts) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Atlas snapshot of {year}: {e}");
            return;
        }
    };
    sink.progress(Some((1, 1, "正在给 Atlas 写各幕".into())));
    let scenes = ask(sink, target, window, thinking, &snap, facts, letter).await;
    sink.progress(None);
    let run = sink.run_id();
    if let Err(e) = sink.with_youzi(|c| save(c, run, &snap, letter, &scenes, model)) {
        eprintln!("Atlas of {year}: {e}");
    }
}

pub fn save(c: &Connection, run_id: Option<i64>, snap: &Snapshot, letter: &str, scenes: &Result<Vec<Scene>, String>, model: (&str, &str)) -> Result<(), String> {
    let snapshot = serde_json::to_string(snap).map_err(err)?;
    let (scenes, error) = match scenes {
        Ok(s) => (Some(serde_json::to_string(s).map_err(err)?), None),
        Err(e) => (None, Some(e.clone())),
    };
    c.execute(
        "INSERT INTO year_atlas (run_id, year, until, letter, snapshot, scenes, error, model, model_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (run_id) DO UPDATE SET year = excluded.year, until = excluded.until, letter = excluded.letter, snapshot = excluded.snapshot,
           scenes = excluded.scenes, error = excluded.error, model = excluded.model, model_name = excluded.model_name, created_at = excluded.created_at",
        params![run_id, snap.year, snap.until, letter, snapshot, scenes, error, model.0, model.1],
    )
    .map(|_| ())
    .map_err(err)
}

/// A saved 「呈现结果」.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub snapshot: Snapshot,
    pub scenes: Option<Vec<Scene>>,
    pub error: Option<String>,
}

pub fn of_run(c: &Connection, run_id: i64) -> Result<Option<Saved>, String> {
    let row: Option<(String, Option<String>, Option<String>)> =
        c.query_row("SELECT snapshot, scenes, error FROM year_atlas WHERE run_id = ?1", [run_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().map_err(err)?;
    row.map(|(snap, scenes, error)| {
        Ok(Saved {
            snapshot: serde_json::from_str(&snap).map_err(|e| format!("Atlas 的快照读不出来：{e}"))?,
            scenes: scenes.map(|s| serde_json::from_str(&s).map_err(|e| format!("Atlas 的各幕读不出来：{e}"))).transpose()?,
            error,
        })
    })
    .transpose()
}

/// Folded under the letter until Atlas's page exists: (label, Markdown).
pub fn preview(c: &Connection, run_id: i64) -> Result<Option<(String, String)>, String> {
    let Some(saved) = of_run(c, run_id)? else { return Ok(None) };
    let Some(scenes) = saved.scenes else {
        return Ok(Some(("Atlas · 各幕没写成".into(), format!("{}\n\n数字和记录的快照已经存下。", saved.error.unwrap_or_default()))));
    };
    let records: HashMap<&str, &Record> = saved.snapshot.records.iter().map(|r| (r.key.as_str(), r)).collect();
    let blocks: Vec<String> = scenes
        .iter()
        .map(|s| {
            let title = SCENES.iter().find(|x| x.0 == s.key).map_or(s.key.as_str(), |x| x.1);
            let mut out = format!("**{title}**　{}", s.lines.join(""));
            for r in s.refs.iter().filter_map(|r| records.get(r.as_str())) {
                let day = r.day.as_deref().map(|d| format!("（{} 年 {}）", &d[..4], md(d))).unwrap_or_default();
                out.push_str(&format!("\n> {}{day}", if r.kind == "note" { clip(&r.text, 60) } else { r.text.clone() }));
            }
            out
        })
        .collect();
    Ok(Some((format!("Atlas · {} 幕", scenes.len()), blocks.join("\n\n"))))
}

/// The mock model's scenes: every offered one, quoting the first records that fit.
pub fn mock_reply(user: &str) -> String {
    let section = |name: &str| user.split(&format!("【{name}】")).nth(1).map(|s| s.split("\n\n【").next().unwrap_or("").to_string()).unwrap_or_default();
    let keys: Vec<String> = section("可以写的幕").lines().filter_map(|l| l.strip_prefix("- ")?.split(' ').next().map(String::from)).collect();
    let refs: Vec<String> = section("可以引用的记录").lines().filter_map(|l| l.strip_prefix("- ")?.split(['（', ' ']).next().map(String::from)).collect();
    let first = |kinds: &[&str], n: usize| refs.iter().filter(|r| kinds.iter().any(|k| r.starts_with(&format!("{k}:")))).take(n).cloned().collect::<Vec<_>>();
    let echo: Vec<String> = section("两年同一天").lines().find(|l| l.starts_with("- ")).map(|l| l.split(['：', '；', '、', ' ']).filter(|x| x.contains(':')).map(|x| x.trim_start_matches("今年").trim_start_matches("去年").to_string()).collect()).unwrap_or_default();
    let scenes: Vec<serde_json::Value> = keys
        .iter()
        .map(|k| {
            let refs = match k.as_str() {
                "days" => first(&["note"], 1),
                "moments" => first(&["goal", "trip", "offer", "day"], 4),
                "works" => first(&["book", "film"], 2),
                "drawing" => first(&["course"], 1),
                "talks" => first(&["talk"], 2),
                "echo" => echo.iter().take(2).cloned().collect(),
                _ => vec![],
            };
            serde_json::json!({ "key": k, "lines": [format!("（模拟）这是「{k}」这一幕的话。")], "refs": refs })
        })
        .collect();
    serde_json::json!({ "scenes": scenes }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::youzi::yearly::tests::{seeded_for_atlas, sink};

    #[test]
    fn snapshot_and_scenes_after_the_letter() {
        let mut s = sink(seeded_for_atlas(), Target::Mock { thinking: false });
        let target = s.target.clone();
        tauri::async_runtime::block_on(crate::youzi::yearly::compose(&mut s, &target, ("local:x", "X"), 32_768, false, None, &Access::default(), "2026", "2026-10-12", "", "写我这一年")).unwrap();
        let (snap, scenes, error): (String, Option<String>, Option<String>) =
            s.youzi.query_row("SELECT snapshot, scenes, error FROM year_atlas WHERE year = '2026'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(error, None);
        let snap: Snapshot = serde_json::from_str(&snap).unwrap();
        assert_eq!(snap.until.as_deref(), Some("2026-10-12"));
        // what each scene can draw on
        let keys = |kind: &str| snap.records.iter().filter(|r| r.kind == kind).map(|r| r.text.as_str()).collect::<Vec<_>>();
        assert_eq!(keys("goal"), ["学会游泳"]);
        assert_eq!(keys("trip"), ["东京看樱花"]);
        assert_eq!(keys("book"), ["《三体》"]);
        assert_eq!(keys("day"), ["在一起（第 2 周年）", "妈妈生日（58 岁）"]); // by date; not December's (after the day), not the monthly one
        assert_eq!(snap.echo.len(), 1);
        assert_eq!(snap.echo[0].day, "03-10");
        assert!(snap.numbers.iter().any(|n| n.label == "笔记" && n.n == 3 && n.last == Some(1)));
        assert!(snap.heat.iter().any(|h| h.day == "2026-03-02" && h.all >= 2)); // a task done + a check-in
        assert_eq!(snap.money.as_ref().map(|m| m.spent.len()), Some(1));
        assert!(snap.offered.contains(&"money".to_string()) && !snap.offered.contains(&"jobs".to_string()));
        let scenes: Vec<Scene> = serde_json::from_str(&scenes.unwrap()).unwrap();
        assert_eq!(scenes.first().map(|x| x.key.as_str()), Some("prologue"));
        assert_eq!(scenes.last().map(|x| x.key.as_str()), Some("finale"));
        let echo = scenes.iter().find(|x| x.key == "echo").unwrap();
        assert_eq!(echo.refs.len(), 2);
        // the letter's reading of money and 来日方长
        assert!(s.request.1.contains("【钱来钱往 · 收支】") && s.request.1.contains("【来日方长 · 这一年的重要日子】"), "{}", s.request.1);
        assert!(!s.request.1.contains("十二月的生日"));
    }

    #[test]
    fn hidden_and_cloud_refused_parts_stay_out() {
        let mut s = sink(seeded_for_atlas(), Target::Mock { thinking: false });
        let target = s.target.clone();
        let access = Access { hidden: vec!["days".into()], cloud: true, cloud_allowed: ["notes", "todo", "books", "films", "drawing", "packing", "jobs", "days", data::FIN_FLOW].map(String::from).to_vec(), made: vec![] };
        tauri::async_runtime::block_on(crate::youzi::yearly::compose(&mut s, &target, ("local:x", "X"), 32_768, false, None, &access, "2026", "2026-10-12", "", "写")).unwrap();
        let snap: String = s.youzi.query_row("SELECT snapshot FROM year_atlas", [], |r| r.get(0)).unwrap();
        let snap: Snapshot = serde_json::from_str(&snap).unwrap();
        assert!(!snap.records.iter().any(|r| r.kind == "day"));
        let m = snap.money.unwrap();
        assert!(!m.spent.is_empty() && m.assets.is_none()); // 资产 isn't sent
        assert!(s.said.borrow().contains("钱来钱往 · 资产"), "{}", s.said.borrow());
    }

    #[test]
    fn parse_keeps_only_what_the_snapshot_has() {
        let snap = Snapshot {
            year: "2026".into(),
            offered: ["prologue", "moments", "echo", "finale"].map(String::from).to_vec(),
            records: vec![
                rec("note", 1, Some("2026-03-10".into()), "今年".into()),
                rec("note", 2, Some("2025-03-10".into()), "去年".into()),
                rec("trip", 3, Some("2026-05-01".into()), "东京".into()),
                rec("goal", 4, Some("2026-02-01".into()), "游泳".into()),
            ],
            echo: vec![Echo { day: "03-10".into(), this: vec!["note:1".into()], last: vec!["note:2".into()] }],
            ..Default::default()
        };
        let reply = r#"好的：```json
{"scenes":[{"key":"finale","lines":["信"]},{"key":"moments","lines":["几件事"],"refs":["trip:3","note:1","goal:4","goal:4","x:9"]},{"key":"works","lines":["没给这一幕"]},{"key":"echo","lines":["回声"],"refs":["note:2","note:1"]},{"key":"prologue","lines":["  ",""]}]}
```"#;
        let out = parse(reply, &snap).unwrap();
        assert_eq!(out.iter().map(|x| x.key.as_str()).collect::<Vec<_>>(), ["moments", "echo", "finale"]); // the journey's order; empty ones go
        assert_eq!(out[0].refs, ["goal:4", "trip:3"]); // only moments' kinds, by date
        assert_eq!(out[1].refs, ["note:1", "note:2"]);
        assert!(parse("不是 JSON", &snap).is_err());
        assert!(parse(r#"{"scenes":[{"key":"works","lines":["x"]}]}"#, &snap).is_err());
    }
}
