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
//! - 和柚子聊的事: the model also sorts the conversations into topics, each with a kind from TOPIC_KINDS
//!   (else `other`). It never picks a drawing: the design draws a kind's own bubble when it has one and a
//!   plain one otherwise, so any year's topics can be drawn. How often is counted here (the refs).
//! - Never fails the letter: a failed scenes step keeps the snapshot with the error.
//! - Every run keeps its own row (history): `design` is the design it was made for (DESIGN then), and
//!   Atlas always draws it with that one, also after a newer design exists; a year done again (补做) is one
//!   more row. Only the user deletes rows (`atlas_delete`, undoable): no AI tool, no schedule.
//! - Also shown folded under the letter (`preview`).

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

/// The design new runs are made for: `<id>@<version>` of a release in src/vendor/pomelo/atlas/ (synced from
/// visual-atelier schemes/pomelo/atlas/releases/). Changed when a newer design is adopted; old rows keep theirs.
pub const DESIGN: &str = "tabloid@1.0.1";

/// What a year's talks with 柚子 are sorted into (key, what belongs). A design draws each its own way.
pub const TOPIC_KINDS: &[(&str, &str)] = &[
    ("cook", "做饭、吃喝"),
    ("work", "求职、面试、工作"),
    ("code", "代码、技术"),
    ("travel", "旅行、出门"),
    ("pet", "宠物"),
    ("sleep", "睡眠、熬夜"),
    ("love", "感情、亲密关系"),
    ("money", "钱、理财、消费"),
    ("study", "学习、读书、考试"),
    ("health", "身体、运动、健康"),
    ("mood", "心情、烦恼"),
    ("family", "家人、朋友"),
];
/// Topics a year shows at most; characters of a topic's name.
const TOPICS: usize = 6;
const TOPIC_CHARS: usize = 6;

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

/// The reply's room (thinking extra): the scenes, and for every conversation the topics name (up to TALKS_MOST).
const SCENE_TOKENS: i64 = 6000;
const TALK_TOKENS: i64 = 10;
const TALKS_MOST: i64 = 20_000;
/// A cloud model's room for thinking on the first try (doubled on each later one).
const CLOUD_THINK: i64 = 24_576;
/// How often the scenes are asked for before giving up (a reply that can't be read is asked for again).
const TRIES: usize = 3;
const ANSWER_LIMIT: std::time::Duration = std::time::Duration::from_secs(600);
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
    /// The day the longest run starts on.
    #[serde(default)]
    pub longest_from: Option<String>,
    /// Days checked in each week of the year (week 0 = January 1–7 …).
    #[serde(default)]
    pub weeks: Vec<i64>,
}

/// A stone of 求职这一年: how many applications got that far (投出 → the 进行中 statuses most reached → Offer).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStage {
    pub label: String,
    pub n: i64,
    pub unit: String,
    #[serde(default)]
    pub got: bool,
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
#[serde(default, rename_all = "camelCase")]
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
    pub job_stages: Vec<JobStage>,
}

/// A topic of the year's talks: `kind` from TOPIC_KINDS or "other"; `n` conversations (`refs`, talk keys).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub name: String,
    pub kind: String,
    pub n: i64,
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub key: String,
    pub lines: Vec<String>,
    pub refs: Vec<String>,
    /// 和柚子聊的事 only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<Topic>,
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
    job_stages: Vec<JobStage>,
}

/// Days checked per week of the year (week 0 = January 1–7).
fn weeks_of(days: &[String]) -> Vec<i64> {
    let mut out = vec![0; 53];
    for d in days {
        let (y, m, day): (i64, u32, usize) = (d[..4].parse().unwrap_or(1970), d[5..7].parse().unwrap_or(1), d[8..10].parse().unwrap_or(1));
        let before: usize = (1..m).map(|x| crate::modules::common::days_in_month(y, x) as usize).sum();
        out[((before + day - 1) / 7).min(52)] += 1;
    }
    out
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
        p.habits = todo::checkins_between(c, from, to)?.into_iter().map(|h| Habit { title: h.title, days: h.days, longest: h.longest, longest_from: h.longest_from, weeks: weeks_of(&h.on) }).collect();
    }
    if access.allows("jobs") {
        let statuses = jobs::statuses(c)?;
        let (mut moved, mut got) = (0, 0);
        let mut reached: HashMap<i64, i64> = HashMap::new();
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
            if !inside.is_empty() {
                moved += 1;
                got += inside.iter().any(|x| x.1) as i64;
                let mut seen = HashSet::new();
                for s in j.steps.iter().filter(|s| s.done && s.done_on.as_deref().is_some_and(|d| d >= from && d <= to)) {
                    if seen.insert(s.status_id) {
                        *reached.entry(s.status_id).or_default() += 1;
                    }
                }
            }
            if let Some((d, _)) = inside.iter().filter(|x| x.1).max() {
                let title = [Some(j.company.trim()), j.position.as_deref().map(str::trim).filter(|x| !x.is_empty())].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                p.records.push(rec("offer", j.id, Some(d.to_string()), title));
            }
        }
        // 投出 → the two 进行中 statuses most applications reached (in the status list's order) → Offer
        if moved > 0 {
            p.job_stages.push(JobStage { label: "有进展".into(), n: moved, unit: "份".into(), got: false });
            let mut active: Vec<&jobs::Status> = statuses.iter().filter(|s| s.stage == jobs::Stage::Active && reached.get(&s.id).is_some_and(|n| *n > 0)).collect();
            active.sort_by_key(|s| std::cmp::Reverse(reached[&s.id]));
            active.truncate(2);
            active.sort_by_key(|s| s.sort_order);
            p.job_stages.extend(active.into_iter().map(|s| JobStage { label: s.name.clone(), n: reached[&s.id], unit: "份".into(), got: false }));
            if got > 0 {
                p.job_stages.push(JobStage { label: "Offer".into(), n: got, unit: "个".into(), got: true });
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
        job_stages: p.job_stages,
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
         - 每一幕可以从【可以引用的记录】里挑几条，写编号（像 note:12）。页上只画这一幕认的那几类，别的类挑了也不出现，所以话也只说挑中的这几条：「留下的日子」（days）挑 1 条最能代表这一年的笔记 note；「书影相遇」（works）挑 1～3 条 book、film；「几个具体时刻」（moments）只挑 goal、trip、offer、day，2～6 条，笔记里写的事不算；「去年的回声」（echo）从【两年同一天】里挑同一天的今年 1 条、去年 1 条；「创作」（drawing）挑 course；「求职这一年」（jobs）挑 offer；「和柚子聊的事」（talks）挑 talk；别的幕不用挑。\n\
         - 「和柚子聊的事」（talks）的话印在柚子的一个小泡泡里：只写 1 句，不超过 28 个字。\n\
         - 写了「和柚子聊的事」（talks）时，再给它一个 topics：把【可以引用的记录】里 talk 开头的对话按话题归成 2～{TOPICS} 组（每个对话最多进一组，归不进去的不管），每组写名字 name（不超过 {TOPIC_CHARS} 个字，像「做饭」「面试」）、类别 kind（只能从这些里挑：{kinds}；都不合适就写 other）、这一组的对话编号 refs。\n\
         - 「终章」可以写 2～4 句，像一封短信的开头、中间和结尾。\n\
         - 话里要引用原话、书名时用「」或《》，不要用英文双引号（会把 JSON 弄坏）。\n\
         - 只输出一个 JSON 对象，从 {{ 开始、到 }} 结束，不要代码块、不要别的文字：{{\"scenes\":[{{\"key\":\"prologue\",\"lines\":[\"……\"],\"refs\":[]}},{{\"key\":\"talks\",\"lines\":[\"……\"],\"refs\":[],\"topics\":[{{\"name\":\"做饭\",\"kind\":\"cook\",\"refs\":[\"talk:3\",\"talk:8\"]}}]}}]}}",
        snap.year,
        kinds = TOPIC_KINDS.iter().map(|(k, what)| format!("{k}（{what}）")).collect::<Vec<_>>().join("、")
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

/// A scene as the model wrote it, whatever shape it chose (read leniently: `raw_scenes`).
struct RawScene {
    key: String,
    lines: Vec<String>,
    refs: Vec<String>,
    topics: Vec<RawTopic>,
}
struct RawTopic {
    name: String,
    kind: String,
    refs: Vec<String>,
}

/// A scene's key from what the model called it: the key, or its title (「序章」).
fn scene_key(s: &str) -> Option<&'static str> {
    let s = s.trim();
    SCENES.iter().find(|x| x.0.eq_ignore_ascii_case(s) || x.1 == s).map(|x| x.0)
}

/// A string, a number, or a list of them, as strings (anything else: nothing).
fn strings(v: &serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Number(n) => vec![n.to_string()],
        serde_json::Value::Array(a) => a.iter().flat_map(strings).collect(),
        _ => vec![],
    }
}

fn field<'a>(o: &'a serde_json::Map<String, serde_json::Value>, names: &[&str]) -> Option<&'a serde_json::Value> {
    names.iter().find_map(|n| o.get(*n))
}

fn raw_scene(key: Option<&str>, v: &serde_json::Value) -> Option<RawScene> {
    let o = match v {
        serde_json::Value::Object(o) => o,
        // `"prologue": "一句话"` / `"prologue": ["一句", "两句"]`
        serde_json::Value::String(_) | serde_json::Value::Array(_) => return key.map(|k| RawScene { key: k.into(), lines: strings(v), refs: vec![], topics: vec![] }),
        _ => return None,
    };
    let key = key.map(String::from).or_else(|| field(o, &["key", "scene", "id"]).and_then(|k| k.as_str()).map(String::from))?;
    let topics = field(o, &["topics"])
        .and_then(|t| t.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_object())
                .map(|t| RawTopic {
                    name: field(t, &["name", "topic", "title"]).map(strings).unwrap_or_default().join(""),
                    kind: field(t, &["kind", "type", "category"]).and_then(|k| k.as_str()).unwrap_or_default().trim().to_lowercase(),
                    // `3` for `talk:3`
                    refs: field(t, &["refs", "talks", "ids"]).map(strings).unwrap_or_default().into_iter().map(|r| if !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()) { format!("talk:{r}") } else { r }).collect(),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(RawScene {
        key,
        lines: field(o, &["lines", "line", "text", "words"]).map(strings).unwrap_or_default().iter().flat_map(|l| l.lines().map(String::from).collect::<Vec<_>>()).collect(),
        refs: field(o, &["refs", "ref", "records"]).map(strings).unwrap_or_default(),
        topics,
    })
}

/// The scenes in a JSON value: `{"scenes": [...]}` (what's asked for), the list alone, or scenes by their key.
/// A lone scene, a list of words or of topics holds none.
fn raw_scenes(v: &serde_json::Value) -> Vec<RawScene> {
    match v {
        serde_json::Value::Object(o) => match o.get("scenes") {
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|x| raw_scene(None, x)).collect(),
            Some(serde_json::Value::Object(m)) => m.iter().filter_map(|(k, x)| raw_scene(Some(k), x)).collect(),
            _ => o.iter().filter(|(k, _)| scene_key(k).is_some()).filter_map(|(k, x)| raw_scene(Some(k), x)).collect(),
        },
        serde_json::Value::Array(a) if a.iter().any(|x| x.get("key").is_some_and(|k| k.is_string())) => a.iter().filter_map(|x| raw_scene(None, x)).collect(),
        _ => vec![],
    }
}

/// Mends what models most often get wrong in JSON: a `"` inside a sentence (it closes the string only before
/// `,` `}` `]` `:` or the end), a line break inside a string, a comma before a closing bracket, 「“ ”」 around a string.
fn repair(text: &str) -> String {
    let cs: Vec<char> = text.chars().collect();
    let solid = |from: usize| cs.get(from..).and_then(|rest| rest.iter().copied().find(|c| !c.is_whitespace()));
    let mut out = String::with_capacity(text.len() + 16);
    // the character that closes the string we're in
    let mut closer: Option<char> = None;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match closer {
            Some(end) => match c {
                '\\' => {
                    out.push(c);
                    if let Some(n) = cs.get(i + 1) {
                        out.push(*n);
                        i += 1;
                    }
                }
                c if c == end && matches!(solid(i + 1), None | Some(',' | '}' | ']' | ':')) => {
                    closer = None;
                    out.push('"');
                }
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => {}
                _ => out.push(c),
            },
            None => match c {
                '"' => {
                    closer = Some('"');
                    out.push('"');
                }
                '“' => {
                    closer = Some('”');
                    out.push('"');
                }
                ',' if matches!(solid(i + 1), Some('}' | ']')) => {}
                _ => out.push(c),
            },
        }
        i += 1;
    }
    out
}

/// The first JSON value in `text` that holds scenes: models put words or a code fence around it, and a `{` may
/// come before it. Err: why the first try at it didn't read.
fn scenes_in(text: &str) -> Result<Vec<RawScene>, String> {
    let mut why = None;
    for (i, _) in text.char_indices().filter(|(_, c)| *c == '{' || *c == '[').take(60) {
        match serde_json::Deserializer::from_str(&text[i..]).into_iter::<serde_json::Value>().next() {
            Some(Ok(v)) => {
                let found = raw_scenes(&v);
                if !found.is_empty() {
                    return Ok(found);
                }
            }
            Some(Err(e)) if why.is_none() => why = Some(e.to_string()),
            _ => {}
        }
    }
    Err(why.unwrap_or_else(|| "里面没有各幕".into()))
}

pub(crate) const NOT_JSON: &str = "模型写的各幕读不出来（不是要的 JSON）";

/// The model's scenes, checked against the snapshot.
pub fn parse(reply: &str, snap: &Snapshot) -> Result<Vec<Scene>, String> {
    let raw = scenes_in(reply).or_else(|_| scenes_in(&repair(reply))).map_err(|_| NOT_JSON.to_string())?;
    let records: HashMap<&str, &Record> = snap.records.iter().map(|r| (r.key.as_str(), r)).collect();
    let mut out = vec![];
    for (key, _, _) in SCENES.iter().filter(|s| snap.offered.iter().any(|k| k == s.0)) {
        let Some(s) = raw.iter().find(|s| scene_key(&s.key) == Some(*key)) else { continue };
        let lines: Vec<String> = s.lines.iter().map(|l| clip(l.trim(), LINE_CHARS)).filter(|l| !l.is_empty()).take(if *key == "finale" { LINES + 1 } else { LINES }).collect();
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
        // topics: only talks, each conversation in one topic; how often = the conversations it names
        let mut topics = vec![];
        if *key == "talks" {
            let mut taken = HashSet::new();
            for t in &s.topics {
                let name: String = t.name.split_whitespace().collect::<String>().chars().take(TOPIC_CHARS).collect();
                let refs: Vec<String> = t.refs.iter().map(|r| r.trim().to_string()).filter(|r| records.get(r.as_str()).is_some_and(|x| x.kind == "talk") && taken.insert(r.clone())).collect();
                if name.is_empty() || refs.is_empty() {
                    continue;
                }
                let kind = TOPIC_KINDS.iter().find(|k| k.0 == t.kind).map_or("other", |k| k.0);
                topics.push(Topic { name, kind: kind.into(), n: refs.len() as i64, refs });
            }
            topics.sort_by_key(|t| std::cmp::Reverse(t.n));
            topics.truncate(TOPICS);
        }
        if !lines.is_empty() || !refs.is_empty() || !topics.is_empty() {
            out.push(Scene { key: key.to_string(), lines, refs, topics });
        }
    }
    if out.is_empty() {
        return Err("模型没写出任何一幕。".into());
    }
    Ok(out)
}

/// A reply read, or why not: (what the user is told, what the model is told before it tries again).
fn read(reply: &str, finish: &str, snap: &Snapshot) -> Result<Vec<Scene>, (String, String)> {
    if reply.trim().is_empty() {
        let cut = if finish == "length" { "，回复的长度都用在思考上了" } else { "" };
        return Err((format!("模型没有写出各幕（回复是空的{cut}）。"), "上一次你什么都没输出。这次直接输出 JSON。".into()));
    }
    parse(reply, snap).map_err(|e| {
        if finish == "length" {
            ("模型写各幕时到了回复长度的上限，没写完。".into(), "上一次写到一半就到了回复长度的上限。这次写短一些：每一幕一句话，不要解释，直接输出 JSON。".into())
        } else {
            let hint = format!("上一次的输出读不出来：{}。这次只输出一个合法的 JSON 对象，从 {{ 开始、到 }} 结束，不要代码块和别的文字；话里要引用原话时用「」，不要用英文双引号。", e.trim_end_matches('。'));
            // what it did write, so the reason can be told from Atlas's page
            let shown = if e == NOT_JSON { format!("{NOT_JSON}，它回的开头是：{}", clip(reply, 60)) } else { e };
            (shown, hint)
        }
    })
}

async fn size(counter: &Counter, messages: &[Msg]) -> i64 {
    let mut used = context::PROMPT_OVERHEAD;
    for m in messages {
        used += counter.count(&m.content).await + context::PER_MESSAGE;
    }
    used
}

/// One answer, whole: the text, why it stopped (`stop`, `length`…) and what it used.
async fn say(target: &Target, messages: Vec<Msg>, max_tokens: i64, thinking: bool, effort: Option<String>) -> Result<(String, String, Option<(i64, i64)>), String> {
    let (mut out, mut finish, mut usage) = (String::new(), String::new(), None);
    // room_fixed: the cap already has the thinking's room
    let streamed = super::provider::stream(target.clone(), messages, super::provider::Options { max_tokens: Some(max_tokens), thinking: Some(thinking), effort, room_fixed: true, ..Default::default() }, |e| match e {
        super::provider::Event::Content(t) => out.push_str(&t),
        super::provider::Event::Finished(r) => finish = r,
        super::provider::Event::Usage { prompt, completion } => usage = Some((prompt, completion)),
        _ => {}
    });
    match tokio::time::timeout(ANSWER_LIMIT, streamed).await {
        Ok(r) => r?,
        Err(_) => return Err("模型 10 分钟都没写完各幕（云端可能正忙）。".into()),
    }
    Ok((out.trim().to_string(), finish, usage))
}

/// Testing with real models (docs/testing.md「Atlas」): POMELO_ATLAS_DUMP=<folder> keeps every request and what
/// came back. Never set in everyday use.
fn dump(attempt: usize, messages: &[Msg], reply: &Result<(String, String, Option<(i64, i64)>), String>) {
    let Some(dir) = std::env::var_os("POMELO_ATLAS_DUMP").map(std::path::PathBuf::from) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let _ = std::fs::write(dir.join(format!("{at}-{attempt}-request.txt")), messages.iter().map(|m| format!("[{}]\n{}", m.role, m.content)).collect::<Vec<_>>().join("\n\n"));
    let text = match reply {
        Ok((out, finish, usage)) => format!("finish: {finish}\nusage: {usage:?}\n\n{out}"),
        Err(e) => format!("error: {e}"),
    };
    let _ = std::fs::write(dir.join(format!("{at}-{attempt}-reply.txt")), text);
}

/// Asks the model for the scenes (thinning the notes it sees until the request fits). A reply that can't be read
/// (empty, cut off, not JSON) is asked for again, told what went wrong and without thinking, `TRIES` times in all.
async fn ask(sink: &impl Sink, target: &Target, window: i64, thinking: bool, snap: &Snapshot, facts: &Facts, letter: &str) -> Result<Vec<Scene>, String> {
    let counter = Counter::for_target(target);
    // the topics name every conversation they hold
    let talks = snap.records.iter().filter(|r| r.kind == "talk").count() as i64;
    // (a small window — a local model — gives a third of itself at most: the year has to fit too)
    let answer = (SCENE_TOKENS + (talks * TALK_TOKENS).min(TALKS_MOST)).min((window / 3).max(2048));
    // Thinking is counted in the reply's cap. A cloud model that can't switch it off (GLM, GPT) thinks as long as it
    // likes: with a small cap the whole reply went on thinking and nothing was written. So: room for it on every try
    // that thinks, more on the later ones, which also ask for as little thinking as the model can do.
    let thinks = |on: bool| match target {
        Target::Cloud { model, .. } => model.thinking.thinks(on),
        _ => on,
    };
    let think_room = |attempt: usize| match (thinks(thinking && attempt == 0), target) {
        (false, _) => 0,
        (true, Target::Cloud { .. }) => CLOUD_THINK << attempt.min(2),
        (true, _) => context::think_room(window),
    };
    let notes = snap.records.iter().filter(|r| r.kind == "note").count();
    let mut stride = 1;
    let request = loop {
        let m = messages(snap, facts, letter, stride);
        if size(&counter, &m).await + answer + think_room(0) <= window {
            break m;
        }
        if notes / stride < 10 {
            return Err("这一年的记录放不下这个模型的上下文，各幕没写。".into());
        }
        stride *= 2;
    };
    let mut missed = String::new();
    for attempt in 0..TRIES {
        let mut m = request.clone();
        let think = thinking && attempt == 0;
        if attempt > 0 {
            m[1].content.push_str(&format!("\n\n【再写一次】{missed}"));
        }
        let reply = say(target, m.clone(), answer + think_room(attempt), think, (attempt > 0).then(|| "low".to_string())).await;
        dump(attempt, &m, &reply);
        let (out, finish, usage) = reply?;
        sink.asked(usage);
        match read(&out, &finish, snap) {
            Ok(scenes) => return Ok(scenes),
            Err((shown, hint)) => {
                eprintln!("Atlas scenes of {}, try {}: {shown}", snap.year, attempt + 1);
                missed = hint;
                if attempt + 1 == TRIES {
                    return Err(format!("{}（试了 {TRIES} 次）", shown.trim_end_matches('。')));
                }
            }
        }
    }
    Err(NOT_JSON.into())
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

/// One more row of history (the same run writing again replaces its own).
pub fn save(c: &Connection, run_id: Option<i64>, snap: &Snapshot, letter: &str, scenes: &Result<Vec<Scene>, String>, model: (&str, &str)) -> Result<(), String> {
    let snapshot = serde_json::to_string(snap).map_err(err)?;
    let (scenes, error) = match scenes {
        Ok(s) => (Some(serde_json::to_string(s).map_err(err)?), None),
        Err(e) => (None, Some(e.clone())),
    };
    c.execute(
        "INSERT INTO year_atlas (run_id, year, until, letter, snapshot, scenes, error, model, model_name, design) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT (run_id) DO UPDATE SET year = excluded.year, until = excluded.until, letter = excluded.letter, snapshot = excluded.snapshot,
           scenes = excluded.scenes, error = excluded.error, model = excluded.model, model_name = excluded.model_name, design = excluded.design, created_at = excluded.created_at",
        params![run_id, snap.year, snap.until, letter, snapshot, scenes, error, model.0, model.1, DESIGN],
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

// ---------- Atlas's page: every run kept (history), opened with the design it was made for ----------

/// A row of the history.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: i64,
    pub year: String,
    pub until: Option<String>,
    pub created_at: String,
    /// `<id>@<version>`; rows from before designs had names count as the first one.
    pub design: String,
    /// Scenes told (0: that step failed, `error`).
    pub scenes: i64,
    pub error: Option<String>,
    pub model_name: String,
    /// Its conversation under 定时 (None once deleted).
    pub session_id: Option<i64>,
}

/// Rows written before `design` existed were made for the first design.
const FIRST_DESIGN: &str = "tabloid@1.0.0";

pub fn list(c: &Connection) -> Result<Vec<Entry>, String> {
    rows(
        c,
        "SELECT a.id, a.year, a.until, a.created_at, a.design, a.scenes, a.error, a.model_name, r.session_id
         FROM year_atlas a LEFT JOIN runs r ON r.id = a.run_id ORDER BY a.year DESC, a.id DESC",
        [],
        |r| {
            let scenes: Option<String> = r.get(5)?;
            Ok(Entry {
                id: r.get(0)?,
                year: r.get(1)?,
                until: r.get(2)?,
                created_at: r.get(3)?,
                design: r.get::<_, Option<String>>(4)?.unwrap_or_else(|| FIRST_DESIGN.into()),
                scenes: scenes.and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok()).map_or(0, |v| v.len() as i64),
                error: r.get(6)?,
                model_name: r.get(7)?,
                session_id: r.get(8)?,
            })
        },
    )
}

/// One run in full, as a design draws it (visual-atelier schemes/pomelo/atlas/README.md「数据」).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Full {
    pub id: i64,
    pub year: String,
    pub until: Option<String>,
    pub created_at: String,
    pub design: String,
    pub model_name: String,
    pub letter: String,
    pub snapshot: Snapshot,
    pub scenes: Option<Vec<Scene>>,
    pub error: Option<String>,
}

pub fn get(c: &Connection, id: i64) -> Result<Full, String> {
    let row = c
        .query_row("SELECT year, until, created_at, design, model_name, letter, snapshot, scenes, error FROM year_atlas WHERE id = ?1", [id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?, r.get::<_, Option<String>>(7)?, r.get::<_, Option<String>>(8)?))
        })
        .optional()
        .map_err(err)?
        .ok_or("找不到这一份年度小报，可能已经被删除了")?;
    Ok(Full {
        id,
        year: row.0,
        until: row.1,
        created_at: row.2,
        design: row.3.unwrap_or_else(|| FIRST_DESIGN.into()),
        model_name: row.4,
        letter: row.5,
        snapshot: serde_json::from_str(&row.6).map_err(|e| format!("Atlas 的快照读不出来：{e}"))?,
        scenes: row.7.map(|s| serde_json::from_str(&s).map_err(|e| format!("Atlas 的各幕读不出来：{e}"))).transpose()?,
        error: row.8,
    })
}

pub fn delete(c: &Connection, id: i64) -> Result<(), String> {
    match c.execute("DELETE FROM year_atlas WHERE id = ?1", [id]).map_err(err)? {
        0 => Err("找不到这一份年度小报，可能已经被删除了".into()),
        _ => Ok(()),
    }
}

/// A year with records and how many (notes, things done, trips): what 补做 can be asked for.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YearCount {
    pub year: String,
    pub records: i64,
}

pub fn years(c: &Connection) -> Result<Vec<YearCount>, String> {
    rows(
        c,
        "SELECT y, count(*) FROM (
           SELECT strftime('%Y', created_at, 'localtime') AS y FROM notes
           UNION ALL SELECT strftime('%Y', done_at, 'localtime') FROM todo_tasks WHERE done = 1 AND done_at IS NOT NULL
           UNION ALL SELECT substr(depart_on, 1, 4) FROM trips WHERE depart_on IS NOT NULL)
         WHERE y IS NOT NULL GROUP BY y ORDER BY y DESC",
        [],
        |r| Ok(YearCount { year: r.get(0)?, records: r.get(1)? }),
    )
}

/// 首页's envelope in the new year's first week (1 月 1～7 日): the newest whole-year run of the year just ended
/// (id, year). None on other days, or when that year has none.
pub fn envelope(c: &Connection, today: &str) -> Result<Option<(i64, String)>, String> {
    if &today[5..7] != "01" || &today[8..10] > "07" {
        return Ok(None);
    }
    let last = (today[..4].parse::<i64>().unwrap_or(1970) - 1).to_string();
    c.query_row("SELECT id, year FROM year_atlas WHERE year = ?1 AND until IS NULL ORDER BY id DESC LIMIT 1", [&last], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(err)
}

/// The year a 年度总结 is being written for right now (schedule.rs sets it around the run).
static WRITING: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub(crate) fn set_writing(year: Option<String>) {
    *WRITING.lock().unwrap_or_else(|e| e.into_inner()) = year;
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub entries: Vec<Entry>,
    pub years: Vec<YearCount>,
    pub writing: Option<String>,
    pub today: String,
    /// The design new runs are made for.
    pub design: String,
    /// The run 首页's envelope opens (the new year's first week).
    pub envelope: Option<i64>,
}

#[tauri::command]
pub fn atlas_overview(state: tauri::State<crate::AppState>) -> Result<Overview, String> {
    let (today, years) = state.with_db(|c| Ok((todo::today(c)?, years(c)?)))?;
    let (entries, envelope) = state.youzi.with_db(|c| Ok((list(c)?, envelope(c, &today)?.map(|e| e.0))))?;
    Ok(Overview { entries, years, writing: WRITING.lock().unwrap_or_else(|e| e.into_inner()).clone(), today, design: DESIGN.into(), envelope })
}

#[tauri::command]
pub fn atlas_get(state: tauri::State<crate::AppState>, id: i64) -> Result<Full, String> {
    state.youzi.with_db(|c| get(c, id))
}

/// Only from Atlas's page (the user): never an AI tool, never a schedule.
#[tauri::command]
pub fn atlas_delete(state: tauri::State<crate::AppState>, id: i64) -> Result<(), String> {
    state.with_youzi_undo("删除年度小报", |c| delete(c, id))
}

/// Folded under the letter: (label, Markdown).
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
            let mut scene = serde_json::json!({ "key": k, "lines": [format!("（模拟）这是「{k}」这一幕的话。")], "refs": refs });
            if k == "talks" {
                // the conversations dealt out to three topics: one with its own bubble, one unknown kind, one other
                let talks = first(&["talk"], usize::MAX);
                let group = |i: usize| talks.iter().enumerate().filter(|(j, _)| j % 3 == i).map(|(_, r)| r.clone()).collect::<Vec<_>>();
                scene["topics"] = serde_json::json!([{ "name": "做饭", "kind": "cook", "refs": group(0) }, { "name": "塔罗", "kind": "tarot", "refs": group(1) }, { "name": "闲聊", "kind": "other", "refs": group(2) }]);
            }
            scene
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

    #[test]
    fn topics_get_a_known_kind_and_are_counted_here() {
        let talk = |id: i64, t: &str| rec("talk", id, Some("2026-04-01".into()), t.into());
        let snap = Snapshot {
            year: "2026".into(),
            offered: ["prologue", "talks", "finale"].map(String::from).to_vec(),
            records: vec![talk(1, "番茄炒蛋"), talk(2, "便当"), talk(3, "面试"), talk(4, "塔罗"), talk(5, "搬家"), rec("note", 9, Some("2026-04-02".into()), "笔记".into())],
            ..Default::default()
        };
        // an unknown kind → other; a note isn't a conversation; a conversation counts once; an empty topic goes;
        // the model's own count is ignored; the name is cut
        let reply = r#"{"scenes":[{"key":"talks","lines":["聊了不少"],"topics":[
            {"name":"塔罗","kind":"tarot","refs":["talk:4"],"n":99},
            {"name":"做饭","kind":"cook","refs":["talk:1","talk:2","note:9","talk:1"]},
            {"name":"求职和面试的那些事","kind":" work ","refs":["talk:3","talk:2"]},
            {"name":"空的","kind":"pet","refs":["talk:77"]}]},{"key":"finale","lines":["一","二","三","四","五"]}]}"#;
        let out = parse(reply, &snap).unwrap();
        let t = &out[0].topics;
        assert_eq!(t.iter().map(|x| (x.name.as_str(), x.kind.as_str(), x.n)).collect::<Vec<_>>(), [("做饭", "cook", 2), ("塔罗", "other", 1), ("求职和面试的", "work", 1)]);
        assert_eq!(t[0].refs, ["talk:1", "talk:2"]);
        assert_eq!(out[1].lines.len(), 4); // 终章 may say a little more
        assert!(out[1].topics.is_empty());
        // the mock model's topics come through the same check
        let user = "【可以写的幕】\n- prologue 序章：x\n- talks 和柚子聊的事：x\n- finale 终章：x\n\n【可以引用的记录】\n- talk:1（2026 年 4 月 1 日） 番茄炒蛋\n- talk:2（2026 年 4 月 1 日） 便当\n- talk:3（2026 年 4 月 1 日） 面试\n\n【刚写的信】\n信";
        let mock = parse(&mock_reply(user), &snap).unwrap();
        let topics = &mock.iter().find(|s| s.key == "talks").unwrap().topics;
        assert_eq!(topics.iter().map(|x| (x.kind.as_str(), x.n)).collect::<Vec<_>>(), [("cook", 1), ("other", 1), ("other", 1)]);
    }

    #[test]
    fn replies_in_other_shapes_are_still_read() {
        let talk = |id: i64, t: &str| rec("talk", id, Some("2026-04-01".into()), t.into());
        let snap = Snapshot { year: "2026".into(), offered: ["prologue", "talks", "finale"].map(String::from).to_vec(), records: vec![talk(1, "面"), talk(2, "汤")], ..Default::default() };
        // words and a stray `{` around it, a code fence, a `"` inside a sentence, null refs, a line break inside a
        // string, a comma before the closing bracket
        let broken = "想了想 {先看看}\n```json\n{\"scenes\":[{\"key\":\"prologue\",\"lines\":[\"你说\"慢慢来\"，就真的慢下来了\"],\"refs\":null},{\"key\":\"finale\",\"lines\":\"第一句\n第二句\",}]}\n```\n就这些 }";
        let out = parse(broken, &snap).unwrap();
        assert_eq!(out[0].lines, ["你说\"慢慢来\"，就真的慢下来了"]);
        assert_eq!(out[1].lines, ["第一句", "第二句"]);
        // scenes by key (titles too), a sentence instead of a list, numbers for the conversations, other names
        let keyed = r#"{"序章":"开场","talks":{"lines":["聊了不少"],"topics":[{"topic":"吃的","type":"Cook","talks":[1,2]}]},"终章":["再见"]}"#;
        let out = parse(keyed, &snap).unwrap();
        assert_eq!(out.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), ["prologue", "talks", "finale"]);
        assert_eq!(out[1].topics, [Topic { name: "吃的".into(), kind: "cook".into(), n: 2, refs: vec!["talk:1".into(), "talk:2".into()] }]);
        // the list alone; 「“ ”」 around the strings
        assert_eq!(parse(r#"[{"key":"prologue","lines":["开场"]}]"#, &snap).unwrap().len(), 1);
        assert_eq!(parse("{“scenes”:[{“key”:“finale”,“lines”:[“再见”]}]}", &snap).unwrap()[0].lines, ["再见"]);
        // one scene alone, or a list of topics, isn't the scenes
        assert_eq!(parse(r#"{"key":"prologue","lines":["开场"]}"#, &snap).unwrap_err(), NOT_JSON);
        // why a reply gave nothing, as the user is told
        assert!(read("", "length", &snap).unwrap_err().0.contains("回复是空的，回复的长度都用在思考上了"));
        assert!(read(r#"{"scenes":[{"key":"prologue","lines":["开"#, "length", &snap).unwrap_err().0.contains("上限"));
        let (shown, hint) = read("我写不出来", "stop", &snap).unwrap_err();
        assert!(shown == format!("{NOT_JSON}，它回的开头是：我写不出来") && hint.contains("英文双引号"));
        assert!(read(keyed, "stop", &snap).is_ok());
    }

    #[test]
    fn every_run_is_kept_and_only_deleted_by_id() {
        let c = crate::youzi::store::tests::memory_db();
        let snap = Snapshot { year: "2025".into(), ..Default::default() };
        let scenes = Ok(vec![Scene { key: "prologue".into(), lines: vec!["开场".into()], refs: vec![], topics: vec![] }]);
        save(&c, None, &snap, "第一封", &scenes, ("cloud:x", "X")).unwrap();
        // the same year done again (补做) is one more row
        save(&c, None, &snap, "第二封", &Err("各幕没写成".into()), ("cloud:x", "X")).unwrap();
        // a row from before designs had names, in the old shapes (no topics, no weeks, no job stages)
        c.execute(
            "INSERT INTO year_atlas (year, letter, snapshot, scenes, model, model_name) VALUES ('2024', '旧的', ?1, ?2, 'local:x', 'X')",
            params![r#"{"year":"2024","until":null,"numbers":[],"extras":[],"heat":[],"habits":[{"title":"拉伸","days":3,"longest":2}],"money":null,"records":[],"echo":[],"offered":["prologue"]}"#, r#"[{"key":"prologue","lines":["旧"],"refs":[]}]"#],
        )
        .unwrap();
        let all = list(&c).unwrap();
        assert_eq!(all.iter().map(|e| (e.year.as_str(), e.scenes, e.design.as_str())).collect::<Vec<_>>(), [("2025", 0, DESIGN), ("2025", 1, DESIGN), ("2024", 1, FIRST_DESIGN)]);
        assert_eq!(all[0].error.as_deref(), Some("各幕没写成"));
        let old = get(&c, all[2].id).unwrap();
        assert_eq!(old.letter, "旧的");
        assert_eq!(old.snapshot.habits[0].weeks, Vec::<i64>::new());
        assert!(old.snapshot.job_stages.is_empty() && old.scenes.unwrap()[0].topics.is_empty());
        // the envelope: only in the new year's first week, the newest whole-year run of the year just ended
        assert_eq!(envelope(&c, "2026-01-03").unwrap(), Some((all[0].id, "2025".into())));
        assert_eq!(envelope(&c, "2026-01-08").unwrap(), None);
        assert_eq!(envelope(&c, "2027-01-01").unwrap(), None);
        delete(&c, all[1].id).unwrap();
        assert_eq!(list(&c).unwrap().len(), 2);
        assert!(delete(&c, all[1].id).is_err() && get(&c, all[1].id).is_err());
    }

    #[test]
    fn weeks_and_job_stages_in_the_snapshot() {
        assert_eq!(weeks_of(&["2026-01-01".into(), "2026-01-07".into(), "2026-01-08".into(), "2026-12-31".into()])[..2], [2, 1]);
        assert_eq!(weeks_of(&["2026-12-31".into()])[52], 1);
        let mut s = sink(seeded_for_atlas(), Target::Mock { thinking: false });
        let target = s.target.clone();
        tauri::async_runtime::block_on(crate::youzi::yearly::compose(&mut s, &target, ("local:x", "X"), 32_768, false, None, &Access::default(), "2026", "2026-10-12", "", "写")).unwrap();
        let full = get(&s.youzi, list(&s.youzi).unwrap()[0].id).unwrap();
        assert_eq!(full.design, DESIGN);
        for h in &full.snapshot.habits {
            assert_eq!(h.weeks.iter().sum::<i64>(), h.days);
            assert!(h.longest_from.is_some());
        }
        // what the page gets is camelCase, with the letter
        let json = serde_json::to_value(&full).unwrap();
        assert!(json["createdAt"].is_string() && json["snapshot"]["jobStages"].is_array() && json["letter"].is_string());
    }
}
