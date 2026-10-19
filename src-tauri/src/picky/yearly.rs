//! 年度总结 (v0.4 phase 8; how it reads and writes: 第六轮问卷, requirements「小拾 · 定时任务、今日简报、年度总结」):
//! the ready-made 定时任务 (builtin `yearly`, 12 月 31 日 21:00, picky migration 13; editable, can be
//! switched off / deleted) writes its year in a conversation, without looking things up.
//! - Reads everything, January up to its month (立即运行 mid-year: so far): data.rs `month_part` per
//!   month (notes in full, goals and tasks done, lessons finished, applications' steps) plus the
//!   month's conversations with 小拾 (`talks`: title and first question; not 私密, not 定时); data.rs
//!   `year_part` (books / films, courses, applications, check-ins, trips); the numbers Rust counts
//!   (`year_counts` / `year_extras`) and the same numbers for last year up to the same day with the
//!   differences worked out; last year's 年度总结 when there is one. 钱来钱往 and 芝麻开门 never
//!   (money talked about in notes is fine).
//! - The whole year in one request when it fits the model's window with room for the letter
//!   (`out_room`; a cloud model nearly always). Otherwise month readers first: each month with
//!   something gets notes that keep dates, names and the user's own words (picky.db
//!   `month_summaries`, with a fingerprint of what it read: an unchanged month is reused, a changed
//!   one written again; a month too big for the window is read in pieces), 4 at a time on a cloud
//!   model, one by one on a local one; then the letter from the notes. The notes are kept with the
//!   run (`run_month_notes`) and shown folded under it.
//! - Thinking as the schedule says (default on: month readers and the letter, each with room for it).
//! - The letter: to 「你」 from 小拾; 关键词 line, 总述, 数字 (copied), themes with
//!   content, 3～5 quotes with dates, 之最, last year woven in, 写给明年; length by how much there is;
//!   empty months / parts in a sentence; nothing about how it was read. The schedule's 要做什么 comes
//!   last and wins.
//! - The year: the run's time (one in January → the year just ended, schedule.rs `year_of`); kept
//!   with the run (`year_runs`) for 首页's new-year card (`card`) and next year's comparison.
//! - Model rules as other 定时任务; for a cloud model the modules not sent are left out and said at
//!   the top. Stopped midway: the months done are kept and reused next time.

use super::context::{self, Counter};
use super::data::{self, month_label, Access, Count};
use super::provider::{Msg, Options, Target};
use crate::modules::common::err;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::VecDeque;

pub const BUILTIN: &str = "yearly";
/// Start the system messages (the mock model answers by them).
pub const MONTH_MARK: &str = "【年度总结 · 每月要点】";
pub const PIECE_MARK: &str = "【年度总结 · 分段要点】";
pub const YEAR_MARK: &str = "【年度总结】";
const MONTH_TOKENS: i64 = 1600;
const PIECE_TOKENS: i64 = 900;
/// Bumped when what a month reads or how it's summarised changes (old summaries are written again).
const RULES: u32 = 2;
/// Months read at once by a cloud model (a local server answers one at a time).
const LANES: usize = 4;
/// Last year's letter goes along up to this many characters.
const LAST_LETTER_CHARS: usize = 6000;
/// The least room the letter needs after what it reads.
const MIN_LETTER: i64 = 2048;
/// Conversations listed per month.
const TALK_LINES: usize = 40;

/// The months to read: January up to December, or up to this month for the current year.
pub fn months_of(year: &str, today: &str) -> Vec<String> {
    let last = if &today[..4] == year { today[5..7].parse::<u32>().unwrap_or(12) } else { 12 };
    (1..=last).map(|m| format!("{year}-{m:02}")).collect()
}

/// FNV-1a over what a month read (and the rules).
pub fn fingerprint(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in format!("{RULES}\n{text}").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// The saved summary of `month` if it was written from exactly this.
pub fn reusable(conn: &Connection, month: &str, fp: &str) -> Result<Option<String>, String> {
    conn.query_row("SELECT content FROM month_summaries WHERE month = ?1 AND fingerprint = ?2", [month, fp], |r| r.get(0)).optional().map_err(err)
}

pub fn save(conn: &Connection, month: &str, content: &str, fp: &str, model: &str, model_name: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO month_summaries (month, content, fingerprint, model, model_name) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (month) DO UPDATE SET content = excluded.content, fingerprint = excluded.fingerprint, model = excluded.model, model_name = excluded.model_name,
           created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![month, content, fp, model, model_name],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn record(conn: &Connection, run_id: i64, month: &str, content: &str) -> Result<(), String> {
    conn.execute("INSERT OR REPLACE INTO run_month_notes (run_id, month, content) VALUES (?1, ?2, ?3)", params![run_id, month, content]).map(|_| ()).map_err(err)
}

/// A run's monthly summaries as shown under its reply: (label, content).
pub fn notes_of(conn: &Connection, run_id: i64) -> Result<Vec<(String, String)>, String> {
    let mut st = conn.prepare("SELECT month, content FROM run_month_notes WHERE run_id = ?1 ORDER BY month").map_err(err)?;
    let rows = st.query_map([run_id], |r| Ok((month_label(&r.get::<_, String>(0)?), r.get(1)?))).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// The run writes `year`'s 年度总结.
pub fn mark_year(conn: &Connection, run_id: i64, year: &str) -> Result<(), String> {
    conn.execute("INSERT OR REPLACE INTO year_runs (run_id, year) VALUES (?1, ?2)", params![run_id, year]).map(|_| ()).map_err(err)
}

/// A finished 年度总结.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Letter {
    pub year: String,
    pub session_id: i64,
    pub content: String,
}

/// The latest finished 年度总结 of `year`. `whole`: only one written in its December or later (the
/// year was over); `with_private`: also from a 私密 conversation.
pub fn letter_of(conn: &Connection, year: &str, whole: bool, with_private: bool) -> Result<Option<Letter>, String> {
    conn.query_row(
        "SELECT m.session_id, m.content FROM year_runs y JOIN runs r ON r.id = y.run_id JOIN messages m ON m.run_id = r.id AND m.role = 'assistant'
          JOIN sessions s ON s.id = m.session_id
         WHERE y.year = ?1 AND r.state = 'completed' AND m.state = 'complete' AND trim(m.content) <> ''
           AND (NOT ?2 OR date(r.started_at, 'localtime') >= ?1 || '-12-01') AND (?3 OR NOT s.private)
         ORDER BY r.id DESC LIMIT 1",
        params![year, whole, with_private],
        |r| Ok(Letter { year: year.into(), session_id: r.get(0)?, content: r.get(1)? }),
    )
    .optional()
    .map_err(err)
}

/// 首页's card in the new year's first week (1 月 1～7 日): last year's whole-year 年度总结.
pub fn card(conn: &Connection, today: &str) -> Result<Option<Letter>, String> {
    if &today[5..7] != "01" || &today[8..10] > "07" {
        return Ok(None);
    }
    let last = (today[..4].parse::<i64>().unwrap_or(1970) - 1).to_string();
    letter_of(conn, &last, true, true)
}

/// Conversations with 小拾 started in [from, to] (local days): not 私密, not written by a 定时任务.
pub fn talk_count(conn: &Connection, from: &str, to: &str) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM sessions WHERE NOT private AND NOT scheduled AND date(created_at, 'localtime') BETWEEN ?1 AND ?2", [from, to], |r| r.get(0)).map_err(err)
}

/// 【和小拾的对话】 of a month: each one's day, title and first question (clipped). Empty: none.
pub fn talks(conn: &Connection, month: &str) -> Result<String, String> {
    let mut st = conn
        .prepare(
            "SELECT strftime('%m-%d', s.created_at, 'localtime'), s.title, (SELECT m.content FROM messages m WHERE m.session_id = s.id AND m.role = 'user' ORDER BY m.id LIMIT 1)
             FROM sessions s WHERE NOT s.private AND NOT s.scheduled AND strftime('%Y-%m', s.created_at, 'localtime') = ?1 ORDER BY s.created_at, s.id",
        )
        .map_err(err)?;
    let all: Vec<(String, String, Option<String>)> = st.query_map([month], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
    if all.is_empty() {
        return Ok(String::new());
    }
    let flat = |s: &str, n: usize| {
        let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.chars().count() > n { format!("{}…", t.chars().take(n).collect::<String>()) } else { t }
    };
    let mut lines: Vec<String> = all
        .iter()
        .take(TALK_LINES)
        .map(|(day, title, first)| match first.as_deref().map(|f| flat(f, 60)).filter(|f| !f.is_empty() && !f.starts_with(title.trim_end_matches('…'))) {
            Some(q) => format!("- {day} {}：{q}", flat(title, 30)),
            None => format!("- {day} {}", flat(title, 60)),
        })
        .collect();
    if all.len() > TALK_LINES {
        lines.push(format!("……还有 {} 个没列出", all.len() - TALK_LINES));
    }
    Ok(format!("【和小拾的对话 {} 个】\n{}", all.len(), lines.join("\n")))
}

/// 「3～5 月、7 月」
pub fn month_spans(months: &[String]) -> String {
    let n: Vec<u32> = months.iter().filter_map(|m| m[5..7].parse().ok()).collect();
    let mut out: Vec<String> = vec![];
    let mut i = 0;
    while i < n.len() {
        let mut j = i;
        while j + 1 < n.len() && n[j + 1] == n[j] + 1 {
            j += 1;
        }
        out.push(if j == i { format!("{} 月", n[i]) } else { format!("{}～{} 月", n[i], n[j]) });
        i = j + 1;
    }
    out.join("、")
}

fn month_messages(year: &str, month: &str, text: &str, pieces: bool) -> Vec<Msg> {
    let system = format!(
        "{MONTH_MARK}你在帮小拾准备用户 {year} 年的年度总结，现在读 {m} 的全部记录（来自用户自己的生活记录 App「柚儿园」）。把它整理成这个月的要点，交给之后写全年的人：他看不到原始记录，所以尽量留下原样的信息。\n\
         - 这个月做了什么、做成了什么、发生了什么、在想什么、心情怎样：一条一条写，带上日期；人名、地名、书名和事情的名称照原样写。\n\
         - 原话：从笔记里摘 2～4 句最能代表这个月的原话，照抄不改字，写成「原话」（{m}几日）这样。没有笔记就不摘。\n\
         - 有和小拾的对话的，用一句话说聊了哪些话题。\n\
         - 只根据{}，不要编，不要评价；不要标题、开场白和总结语。不超过 800 字。",
        if pieces { "下面分段读出来的要点" } else { "下面的记录" },
        m = month_label(month),
    );
    vec![Msg::new("system", &system), Msg::new("user", text)]
}

fn piece_messages(year: &str, month: &str, i: usize, n: usize, text: &str) -> Vec<Msg> {
    let system = format!(
        "{PIECE_MARK}你在帮小拾准备用户 {year} 年的年度总结。{} 的记录太多，分 {n} 段读，这是第 {i} 段。记下这一段的要点：做了什么、发生了什么、在想什么（带日期，名称照原样写），再摘 1～3 句原话（照抄不改字，带日期）。只根据记录，不要编；不超过 500 字。",
        month_label(month)
    );
    vec![Msg::new("system", &system), Msg::new("user", text)]
}

/// What the letter is written from besides the months.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// 【这一年的数字】
    pub numbers: String,
    /// 【和去年比】 and how far each year was counted.
    pub compare: Option<(String, String)>,
    /// data.rs `year_part`
    pub whole: String,
    /// Months without any record.
    pub quiet: Vec<String>,
    pub last_letter: Option<String>,
    /// The year isn't over: written up to this day.
    pub until: Option<String>,
}

/// How the months are given: all their records, or the month readers' notes.
#[derive(Clone, Copy)]
pub enum Read<'a> {
    Whole(&'a [(String, String)]),
    Notes(&'a [(String, String)]),
}

fn year_system(year: &str, facts: &Facts) -> String {
    let until = facts.until.as_deref().map(|d| format!("这一年还没过完，写到 {}为止。", data::span(d, d))).unwrap_or_default();
    let compare = if facts.compare.is_some() {
        "\n  7. 和去年比：自然地写进相关的块里（「比去年多读了 4 本」「去年你在想……，今年……」），数字照【和去年比】写，不用单独成块。"
    } else {
        ""
    };
    format!(
        "{YEAR_MARK}你是小拾，用户自己的生活记录 App「柚儿园」里的助手。现在给用户写 {year} 年的年度总结：一封写给用户的信，像一个熟悉用户的朋友，陪用户回看这一年。{until}\n\
         \n\
         怎么写：\n\
         - 称呼用户「你」。口吻温暖、具体、真诚，像朋友聊天；不要写成报告，不要堆形容词和空泛的鼓励，不要说教。\n\
         - 用 Markdown，按这个顺序：\n\
         \x20 1. 第一行：**这一年的关键词**：词 · 词 · 词（3～5 个，从这一年的内容里提炼）\n\
         \x20 2. 一段总述：这一年整体是什么样的。\n\
         \x20 3. 「## 这一年的数字」：从【这一年的数字】里挑有意义的，写成几行或一小段。数字照抄，不要自己加减、估算或换算。\n\
         \x20 4. 按主题分几块，每块一个「## 小标题」：比如读书观影、创作、学业和工作、感情和家人、身体和习惯、和小拾聊的事……只写这一年真有内容的主题，标题按内容起。每块写具体的事：书名、人名、地名、日期照原样写，说说它们在这一年里的分量和变化。\n\
         \x20 5. 「## 你写下的话」：摘 3～5 句用户在笔记里写下的原话，照抄不改字（可以只截一句里的一段），每句一行，写成「> 原话（3 月 12 日）」。\n\
         \x20 6. 「## 这一年之最」：几条，有依据的才写，比如写笔记最多的月份、最常出现的人和事、打卡坚持最久的、评分最高的书影。{compare}\n\
         \x20 8. 最后「## 写给明年」：几句对明年的期待，或者留给明年的一两个问题。\n\
         - 篇幅看这一年的内容：内容多就写长、写细，内容少就写短，不要硬凑。\n\
         - 没有记录的月份、没有内容的事（比如这一年没画完课），一句带过（像「3～9 月几乎没有记录」），不要单独成块。\n\
         - 只根据下面给你的内容写，不要编；拿不准的就不写。\n\
         - 只写信本身：不要提你是怎么读的、读了什么没读什么、用了哪些要点或小结，结尾不要加说明或备注。\n\
         - 用户的要求（最后一段）和上面冲突时，照用户的要求。"
    )
}

/// The request for the letter: the rules first, then everything read, the user's 要做什么 last.
pub fn year_messages(year: &str, system: &str, read: Read, facts: &Facts, instruction: &str) -> Vec<Msg> {
    let rules = year_system(year, facts);
    let system = [system.trim(), rules.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
    let mut parts = vec![format!("【这一年的数字】\n{}", if facts.numbers.is_empty() { "（没有）" } else { &facts.numbers })];
    if let Some((text, counted)) = &facts.compare {
        parts.push(format!("【和去年比】（{counted}）\n{text}"));
    }
    if !facts.whole.is_empty() {
        parts.push(format!("【这一年】\n{}", facts.whole));
    }
    if !facts.quiet.is_empty() {
        parts.push(format!("【没有任何记录的月份】{}", month_spans(&facts.quiet)));
    }
    let months = |m: &[(String, String)]| m.iter().map(|(m, s)| format!("### {}\n{s}", month_label(m))).collect::<Vec<_>>().join("\n\n");
    parts.push(match read {
        Read::Whole([]) | Read::Notes([]) => "【每个月的记录】（这一年还没有可读的记录。）".into(),
        Read::Whole(m) => format!("【每个月的记录】\n{}", months(m)),
        Read::Notes(m) => format!("【每个月的要点】（读完那个月全部记录后整理的，「」里是用户的原话）\n{}", months(m)),
    });
    if let Some(last) = &facts.last_letter {
        parts.push(format!("【去年的年度总结】（只用来和去年比，不要照抄）\n{last}"));
    }
    parts.push(format!("用户的要求：{}", instruction.trim()));
    vec![Msg::new("system", &system), Msg::new("user", &parts.join("\n\n"))]
}

/// Lines of `text` in pieces of at most `max` tokens (a line longer than that is its own piece).
async fn split(text: &str, max: i64, counter: &Counter) -> Vec<String> {
    let (mut pieces, mut cur, mut used) = (vec![], String::new(), 0);
    for line in text.lines() {
        let n = counter.count(line).await + 1;
        if used + n > max && !cur.is_empty() {
            pieces.push(std::mem::take(&mut cur));
            used = 0;
        }
        cur.push_str(line);
        cur.push('\n');
        used += n;
    }
    if !cur.trim().is_empty() {
        pieces.push(cur);
    }
    pieces
}

/// One request of a month reader (`max` has room for thinking); its usage joins `usage`.
async fn ask_month(target: &Target, messages: Vec<Msg>, max: i64, thinking: bool, month: &str, usage: &mut Vec<Option<(i64, i64)>>) -> Result<String, String> {
    let (out, u) = super::brief::ask(target, messages, max, thinking).await?;
    usage.push(u);
    if out.is_empty() {
        return Err(format!("模型没有写出{}的要点。", month_label(month)));
    }
    Ok(out)
}

/// One month's notes (a month reader): one request, or pieces first when it doesn't fit. Also the
/// usage of each request.
async fn summarise(target: Target, window: i64, thinking: bool, year: String, month: String, text: String) -> Result<(String, Vec<Option<(i64, i64)>>), String> {
    let counter = Counter::for_target(&target);
    let extra = if thinking { context::think_room(window) } else { 0 };
    let room = window - context::reserve(window).max(MONTH_TOKENS + extra) - context::PROMPT_OVERHEAD - 800;
    let mut usage = vec![];
    let notes = if counter.count(&text).await <= room {
        ask_month(&target, month_messages(&year, &month, &text, false), MONTH_TOKENS + extra, thinking, &month, &mut usage).await?
    } else {
        let pieces = split(&text, room, &counter).await;
        let mut got = vec![];
        for (i, piece) in pieces.iter().enumerate() {
            let n = ask_month(&target, piece_messages(&year, &month, i + 1, pieces.len(), piece), PIECE_TOKENS + extra, thinking, &month, &mut usage).await?;
            got.push(format!("（第 {} 段）{n}", i + 1));
        }
        ask_month(&target, month_messages(&year, &month, &got.join("\n\n"), true), MONTH_TOKENS + extra, thinking, &month, &mut usage).await?
    };
    Ok((notes, usage))
}

/// Room the letter needs (thinking on): a cloud model up to its own cap, a local one a third of
/// its window.
fn out_room(target: &Target, window: i64) -> i64 {
    match target {
        Target::Cloud { model, .. } => model.max_output_tokens.min(32_768),
        _ => (window / 3).clamp(MIN_LETTER, 16_384),
    }
}

async fn size(counter: &Counter, messages: &[Msg]) -> i64 {
    let mut used = context::PROMPT_OVERHEAD;
    for m in messages {
        used += counter.count(&m.content).await + context::PER_MESSAGE;
    }
    used
}

fn letter_opts(max_tokens: i64, thinking: bool) -> Options {
    Options { max_tokens: Some(max_tokens), thinking: Some(thinking), ..Default::default() }
}

/// Where the letter goes and what it may use (the app: the run's reply; tests: memory).
pub(crate) trait Sink {
    fn with_pomelo<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String>;
    fn with_picky<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String>;
    /// Reading month `i` of `n` (None: done reading).
    fn progress(&self, at: Option<(usize, usize, String)>);
    /// A month's notes are ready (shown folded under the reply).
    fn month(&self, month: &str, content: &str);
    /// A month / piece request finished (its usage, if the service said).
    fn asked(&self, usage: Option<(i64, i64)>);
    /// Text at the top of the reply (not from the model).
    fn say(&self, text: &str);
    /// Streams the letter into the reply.
    async fn article(&mut self, messages: Vec<Msg>, opts: Options) -> Result<(), String>;
}

/// 「10 月 15 日」
fn month_day(d: &str) -> String {
    format!("{} 月 {} 日", d[5..7].trim_start_matches('0'), d[8..10].trim_start_matches('0'))
}

/// Everything besides the months: the numbers (and last year's up to the same day), the year as a
/// whole, last year's letter.
fn facts(sink: &impl Sink, access: &Access, year: &str, today: &str, quiet: Vec<String>) -> Result<Facts, String> {
    let until = (&today[..4] == year).then(|| today.to_string());
    let (from, to) = (format!("{year}-01-01"), until.clone().unwrap_or_else(|| format!("{year}-12-31")));
    let prev = (year.parse::<i64>().unwrap_or(1971) - 1).to_string();
    let (prev_from, prev_to) = (format!("{prev}-01-01"), format!("{prev}{}", if &to[4..] == "-02-29" { "-02-28" } else { &to[4..] }));
    let counts = |f: &str, t: &str| -> Result<Vec<Count>, String> {
        let mut c = sink.with_pomelo(|c| data::year_counts(c, access, f, t))?;
        c.push(Count { label: "和小拾的对话", unit: "个", n: sink.with_picky(|c| talk_count(c, f, t))? });
        Ok(c)
    };
    let (this, last) = (counts(&from, &to)?, counts(&prev_from, &prev_to)?);
    let extras = sink.with_pomelo(|c| data::year_extras(c, access, &from, &to))?;
    let counted = match &until {
        Some(d) => format!("两年都算到 {}；书影只记了哪一年读完看完，按全年算", month_day(d)),
        None => "两年都算全年".into(),
    };
    let last_letter = sink.with_picky(|c| letter_of(c, &prev, false, false))?.map(|l| {
        let n = l.content.chars().count();
        if n > LAST_LETTER_CHARS { format!("{}……（后面略）", l.content.chars().take(LAST_LETTER_CHARS).collect::<String>()) } else { l.content }
    });
    Ok(Facts {
        numbers: data::numbers_text(&this, &extras),
        compare: data::compare_text(&this, &last).map(|t| (t, counted)),
        whole: sink.with_pomelo(|c| data::year_part(c, access, year))?,
        quiet,
        last_letter,
        until,
    })
}

type Reader = tauri::async_runtime::JoinHandle<Result<(String, Vec<Option<(i64, i64)>>), String>>;

/// Month readers still going; stopped when the letter is (dropped).
struct Readers(VecDeque<(usize, String, Reader)>);

impl Drop for Readers {
    fn drop(&mut self) {
        for (_, _, h) in &self.0 {
            h.abort();
        }
    }
}

/// The month readers: each month's notes, reused when its records haven't changed, otherwise
/// written (several at once on a cloud model) and saved as each is done.
async fn read_months(sink: &impl Sink, target: &Target, model: (&str, &str), window: i64, thinking: bool, year: &str, months: &[(String, String)]) -> Result<Vec<(String, String)>, String> {
    let mut notes: Vec<Option<String>> = vec![None; months.len()];
    let mut waiting = VecDeque::new();
    for (i, (month, text)) in months.iter().enumerate() {
        let fp = fingerprint(text);
        match sink.with_picky(|c| reusable(c, month, &fp))? {
            Some(s) => notes[i] = Some(s),
            None => waiting.push_back((i, fp)),
        }
    }
    let lanes = if matches!(target, Target::Local { .. }) { 1 } else { LANES };
    let mut running = Readers(VecDeque::new());
    loop {
        while running.0.len() < lanes {
            let Some((i, fp)) = waiting.pop_front() else { break };
            let (t, y, m, x) = (target.clone(), year.to_string(), months[i].0.clone(), months[i].1.clone());
            running.0.push_back((i, fp, tauri::async_runtime::spawn(summarise(t, window, thinking, y, m, x))));
        }
        let Some((i, fp, reader)) = running.0.pop_front() else { break };
        let done = notes.iter().filter(|n| n.is_some()).count();
        sink.progress(Some((done + 1, months.len(), format!("正在读 {}", month_label(&months[i].0)))));
        let (s, usage) = reader.await.map_err(|_| format!("读{}时出错了。", month_label(&months[i].0)))??;
        for u in usage {
            sink.asked(u);
        }
        sink.with_picky(|c| save(c, &months[i].0, &s, &fp, model.0, model.1))?;
        notes[i] = Some(s);
    }
    sink.progress(None);
    let out: Vec<(String, String)> = months.iter().zip(notes).filter_map(|((m, _), n)| n.map(|n| (m.clone(), n))).collect();
    for (m, n) in &out {
        sink.month(m, n);
    }
    Ok(out)
}

/// The whole 年度总结 for `year`: everything read, then the letter streamed — from all the records
/// when they fit, else from the month readers' notes.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn compose(sink: &mut impl Sink, target: &Target, model: (&str, &str), window: i64, thinking: bool, access: &Access, year: &str, today: &str, system: &str, instruction: &str) -> Result<(), String> {
    let counter = Counter::for_target(target);
    let left_out = data::year_left_out(access);
    if !left_out.is_empty() {
        sink.say(&format!("（{}不发给云端，这次没读。）", left_out.join("、")));
    }
    let (mut months, mut quiet) = (vec![], vec![]);
    for month in months_of(year, today) {
        let mut text = sink.with_pomelo(|c| data::month_part(c, access, &month))?;
        let talk = sink.with_picky(|c| talks(c, &month))?;
        if !talk.is_empty() {
            text = [text, talk].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n\n");
        }
        if text.trim().is_empty() {
            quiet.push(month);
        } else {
            months.push((month, text));
        }
    }
    let mut facts = facts(sink, access, year, today, quiet)?;
    let whole = year_messages(year, system, Read::Whole(&months), &facts, instruction);
    let used = size(&counter, &whole).await;
    if used + out_room(target, window) <= window {
        return sink.article(whole, letter_opts(window - used, thinking)).await;
    }
    let notes = read_months(sink, target, model, window, thinking, year, &months).await?;
    let mut messages = year_messages(year, system, Read::Notes(&notes), &facts, instruction);
    let mut used = size(&counter, &messages).await;
    if used + MIN_LETTER > window && facts.last_letter.take().is_some() {
        messages = year_messages(year, system, Read::Notes(&notes), &facts, instruction);
        used = size(&counter, &messages).await;
    }
    if used + MIN_LETTER > window {
        return Err("这一年的要点加起来放不下这个模型的上下文，换一个上下文大的模型（比如云端）再运行。".into());
    }
    sink.article(messages, letter_opts(window - used, thinking)).await
}

// ---------- commands ----------

/// 首页: last year's 年度总结 in the new year's first week.
#[tauri::command]
pub fn picky_year_card(state: tauri::State<crate::AppState>) -> Result<Option<Letter>, String> {
    let today = state.with_db(|c| crate::modules::todo::today(c))?;
    state.picky.with_db(|c| card(c, &today))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Memory databases; what the app would show, kept.
    pub(crate) struct TestSink {
        pub pomelo: Connection,
        pub picky: Connection,
        pub target: Target,
        pub said: RefCell<String>,
        pub months: RefCell<Vec<(String, String)>>,
        pub calls: RefCell<usize>,
        /// The letter's request (system, user) and the letter.
        pub request: (String, String),
        pub thinking: Option<bool>,
        pub article: String,
    }

    impl Sink for TestSink {
        fn with_pomelo<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
            f(&self.pomelo)
        }
        fn with_picky<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
            f(&self.picky)
        }
        fn progress(&self, at: Option<(usize, usize, String)>) {
            if let Some((i, n, label)) = at {
                if std::env::var("POMELO_SHOW").is_ok() {
                    println!("  [{i}/{n}] {label}");
                }
            }
        }
        fn month(&self, month: &str, content: &str) {
            self.months.borrow_mut().push((month.into(), content.into()));
        }
        fn asked(&self, _: Option<(i64, i64)>) {
            *self.calls.borrow_mut() += 1;
        }
        fn say(&self, text: &str) {
            self.said.borrow_mut().push_str(text);
        }
        async fn article(&mut self, messages: Vec<Msg>, opts: Options) -> Result<(), String> {
            self.request = (messages[0].content.clone(), messages[1].content.clone());
            self.thinking = opts.thinking;
            let mut out = String::new();
            super::super::provider::stream(self.target.clone(), messages, opts, |e| {
                if let super::super::provider::Event::Content(t) = e {
                    out.push_str(&t);
                }
            })
            .await?;
            self.article = out;
            Ok(())
        }
    }

    pub(crate) fn sink(pomelo: Connection, target: Target) -> TestSink {
        TestSink {
            pomelo,
            picky: crate::picky::store::tests::memory_db(),
            target,
            said: RefCell::default(),
            months: RefCell::default(),
            calls: RefCell::default(),
            request: Default::default(),
            thinking: None,
            article: String::new(),
        }
    }

    fn seeded() -> Connection {
        let c = crate::db::tests::memory_db();
        c.execute_batch(
            "INSERT INTO notes (content, created_at) VALUES ('一月的计划：读完三本书 #读书', '2026-01-05T02:00:00.000Z'), ('想换工作了 #工作', '2026-03-10T02:00:00.000Z'), ('三月又看了一遍旧笔记 #读书', '2026-03-20T02:00:00.000Z'), ('去年的笔记', '2025-12-20T02:00:00.000Z');
             INSERT INTO note_tags (note_id, tag) VALUES (1, '读书'), (2, '工作'), (3, '读书');
             INSERT INTO todo_groups (name, sort_order) VALUES ('生活', 1);
             INSERT INTO todo_goals (group_id, name, status, start_date, end_date, review, sort_order) VALUES (1, '学会游泳', 'done', '2026-01-01', '2026-03-31', '能游 200 米了', 1);
             INSERT INTO todo_tasks (group_id, title, task_type, done, done_at, sort_order) VALUES (1, '办游泳卡', 'normal', 1, '2026-03-02T02:00:00.000Z', 1);
             INSERT INTO todo_tasks (group_id, title, task_type, repeat_rule, start_date, sort_order) VALUES (1, '背单词', 'checkin', 'daily', '2026-01-01', 2);
             INSERT INTO todo_logs (task_id, day) VALUES (2, '2026-03-01'), (2, '2026-03-02'), (2, '2026-03-03'), (2, '2026-03-05'), (2, '2025-06-01');
             INSERT INTO drawing_courses (name, sort_order) VALUES ('人体速写', 1);
             INSERT INTO drawing_lessons (course_id, code, title, status, finished_month, sort_order) VALUES (1, '1A', '比例', 'done', '2026-03', 1), (1, '1B', '动态', 'todo', NULL, 2);
             INSERT INTO trips (name, depart_on, note) VALUES ('东京看樱花', '2026-03-28', '五天'), ('老家', '2025-02-01', NULL);
             INSERT INTO books (title, author, status, finished_year, rating) VALUES ('三体', '刘慈欣', 'done', '2026', 5), ('旧书', '某人', 'done', '2025', 3), ('更旧的书', '某人', 'done', '2024', 3);",
        )
        .unwrap();
        c
    }

    /// Thinking off unless said: the counts below assume the plain room.
    fn run(s: &mut TestSink, window: i64, today: &str) {
        run_thinking(s, window, today, false);
    }
    fn run_thinking(s: &mut TestSink, window: i64, today: &str, thinking: bool) {
        let target = s.target.clone();
        tauri::async_runtime::block_on(compose(s, &target, ("local:x", "X"), window, thinking, &Access::default(), &today[..4], today, "", "写我这一年的年度总结")).unwrap();
    }

    #[test]
    fn months_and_years_read_only_their_own() {
        assert_eq!(months_of("2026", "2026-03-15"), ["2026-01", "2026-02", "2026-03"]);
        assert_eq!(months_of("2025", "2026-01-02").len(), 12);
        assert_eq!(crate::picky::schedule::year_of("2026-12-31 21:00"), "2026");
        assert_eq!(crate::picky::schedule::year_of("2027-01-02 09:00"), "2026");
        let c = seeded();
        let all = Access::default();
        let march = data::month_part(&c, &all, "2026-03").unwrap();
        assert!(march.contains("想换工作了") && march.contains("三月又看了一遍旧笔记") && march.contains("学会游泳") && march.contains("能游 200 米了") && march.contains("人体速写 · 1A 比例"));
        assert!(march.contains("【做完的事 5 件】（每次重复、打卡、进度记录各算一件）\n- 背单词 ×4（生活）\n- 03-02 办游泳卡（生活）"), "{march}");
        assert!(!march.contains("一月的计划"));
        assert!(data::month_part(&c, &all, "2026-02").unwrap().is_empty());
        let whole = data::year_part(&c, &all, "2026").unwrap();
        assert!(whole.contains("《三体》（刘慈欣，5 星）") && !whole.contains("旧书") && whole.contains("这一年画完 1 节，一共画完 1 / 2 节"));
        assert!(whole.contains("- 背单词：这一年打卡 4 天，最长连续打了 3 次") && whole.contains("【出行 1 次】\n- 3 月 28 日 出发：东京看樱花（五天）"), "{whole}");
        // 投石问路: the steps taken that month, and the year's applications
        data::tests::seed_jobs(&c);
        let sept = data::month_part(&c, &all, "2026-09").unwrap();
        assert!(sept.contains("【求职进展") && sept.contains("甲公司"), "{sept}");
        assert!(data::year_part(&c, &all, "2026").unwrap().contains("【求职】这一年有进展的投递"));
        // a cloud model without 念念不忘: notes left out and said
        let cloud = Access { cloud: true, cloud_allowed: vec!["todo".into(), "drawing".into(), "jobs".into(), "packing".into(), "books".into(), "films".into()], ..Default::default() };
        assert!(!data::month_part(&c, &cloud, "2026-03").unwrap().contains("想换工作了"));
        assert_eq!(data::year_left_out(&cloud), ["念念不忘"]);
    }

    #[test]
    fn numbers_are_counted_and_compared_with_last_year_to_the_same_day() {
        let c = seeded();
        let all = Access::default();
        let this = data::year_counts(&c, &all, "2026-01-01", "2026-12-31").unwrap();
        let n = |label: &str| this.iter().find(|x| x.label == label).unwrap().n;
        assert_eq!((n("笔记"), n("写了笔记的日子"), n("做完的事"), n("做完的目标"), n("打卡"), n("读完的书"), n("画完的课节"), n("出行")), (3, 3, 5, 1, 4, 1, 1, 1));
        let extras = data::year_extras(&c, &all, "2026-01-01", "2026-12-31").unwrap();
        assert!(extras.contains(&"写笔记最多的月份：3 月（2 条）".to_string()) && extras.contains(&"最常用的标签：#读书（2 条）、#工作（1 条）".to_string()), "{extras:?}");
        assert!(extras.iter().any(|e| e.starts_with("每月做完的事：1 月 0 件 · 2 月 0 件 · 3 月 5 件")));
        let text = data::numbers_text(&this, &extras);
        assert!(text.contains("- 读完的书：1 本") && !text.contains("拿到的工作"), "{text}");
        let last = data::year_counts(&c, &all, "2025-01-01", "2025-12-31").unwrap();
        let cmp = data::compare_text(&this, &last).unwrap();
        assert!(cmp.contains("- 笔记：今年 3 条，去年 1 条，多 2 条") && cmp.contains("- 读完的书：今年 1 本，去年 1 本，一样多") && cmp.contains("- 打卡：今年 4 天，去年 1 天，多 3 天"), "{cmp}");
        // nothing two years ago: no comparison
        assert!(data::compare_text(&last, &data::year_counts(&c, &all, "2024-01-01", "2024-12-31").unwrap().into_iter().map(|x| Count { n: 0, ..x }).collect::<Vec<_>>()).is_none());
        // the year so far: last year up to the same day (the 2025-12-20 note is after 03-31)
        let s = sink(seeded(), Target::Mock { thinking: false });
        let f = facts(&s, &all, "2026", "2026-03-31", vec![]).unwrap();
        let (cmp, counted) = f.compare.unwrap();
        assert!(cmp.contains("- 笔记：今年 3 条，去年 0 条，多 3 条") && counted.starts_with("两年都算到 3 月 31 日"), "{cmp}");
        assert_eq!(f.until.as_deref(), Some("2026-03-31"));
    }

    #[test]
    fn the_whole_year_in_one_go_when_it_fits() {
        let mut s = sink(seeded(), Target::Mock { thinking: false });
        run_thinking(&mut s, 16384, "2026-03-31", true);
        assert_eq!((*s.calls.borrow(), s.months.borrow().len()), (0, 0), "no month readers");
        assert!(s.article.contains("一次读完了全年") && s.article.contains("这一年的关键词"), "{}", s.article);
        assert_eq!(s.thinking, Some(true));
        let (system, user) = &s.request;
        assert!(system.contains(YEAR_MARK) && system.contains("写给明年") && system.contains("写到 2026 年 3 月 31 日为止"));
        assert!(user.contains("【这一年的数字】\n- 笔记：3 条") && user.contains("【每个月的记录】\n### 1 月") && user.contains("想换工作了"), "{user}");
        assert!(user.contains("【没有任何记录的月份】2 月") && user.ends_with("用户的要求：写我这一年的年度总结"));
        assert!(!user.contains("【和去年比】") || user.contains("（两年都算到 3 月 31 日"));
    }

    /// Notes in January and March, too many for a small window at once.
    fn busy() -> Connection {
        let c = seeded();
        for i in 0..14 {
            for m in ["01", "03"] {
                c.execute("INSERT INTO notes (content, created_at) VALUES (?1, ?2)", params![format!("{m} 月第 {i} 条：{}", "今天想了很多事情。".repeat(14)), format!("2026-{m}-{:02}T03:00:00.000Z", i + 1)]).unwrap();
            }
        }
        c
    }

    #[test]
    fn month_readers_when_it_doesnt_fit_and_months_are_reused_until_they_change() {
        let mut s = sink(busy(), Target::Mock { thinking: false });
        run(&mut s, 6144, "2026-03-31");
        assert_eq!(s.months.borrow().iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["2026-01", "2026-03"]); // February had nothing
        assert!(s.article.contains("读了 2 个月的要点"), "{}", s.article);
        assert!(s.request.1.contains("【每个月的要点】") && s.request.1.contains("模拟要点"));
        let first = *s.calls.borrow();
        assert_eq!(first, 2);
        // again: nothing changed, no month written
        s.months.borrow_mut().clear();
        run(&mut s, 6144, "2026-03-31");
        assert_eq!((*s.calls.borrow(), s.months.borrow().len()), (first, 2));
        // a new note in March: only March again
        s.pomelo.execute("INSERT INTO notes (content, created_at) VALUES ('三月最后一条', '2026-03-30T02:00:00.000Z')", []).unwrap();
        run(&mut s, 6144, "2026-03-31");
        assert_eq!(*s.calls.borrow(), first + 1);
    }

    #[test]
    fn a_month_too_big_is_read_in_pieces() {
        let c = crate::db::tests::memory_db();
        for i in 0..40 {
            c.execute("INSERT INTO notes (content, created_at) VALUES (?1, ?2)", params![format!("第 {i} 条：{}", "很长的一段话。".repeat(60)), format!("2026-05-{:02}T0{}:00:00.000Z", i % 28 + 1, i % 9)]).unwrap();
        }
        let mut s = sink(c, Target::Mock { thinking: false });
        // a small window: May doesn't fit in one request
        run(&mut s, 8192, "2026-05-31");
        assert!(*s.calls.borrow() > 2, "pieces, then the month");
        assert_eq!(s.months.borrow().len(), 1);
    }

    /// A finished 年度总结 of `year` started at `at` (UTC).
    fn letter(p: &Connection, year: &str, at: &str, text: &str) -> i64 {
        p.execute("INSERT INTO sessions (title, scheduled) VALUES ('年度总结', 1)", []).unwrap();
        let s = p.last_insert_rowid();
        p.execute("INSERT INTO runs (session_id, model, model_name, state, started_at) VALUES (?1, 'local:x', 'X', 'completed', ?2)", params![s, at]).unwrap();
        let r = p.last_insert_rowid();
        p.execute("INSERT INTO messages (session_id, run_id, role, content, state) VALUES (?1, ?2, 'assistant', ?3, 'complete')", params![s, r, text]).unwrap();
        mark_year(p, r, year).unwrap();
        s
    }

    #[test]
    fn last_years_letter_goes_along_and_the_new_year_card_shows_a_whole_year() {
        let mut s = sink(seeded(), Target::Mock { thinking: false });
        let mid = letter(&s.picky, "2025", "2025-10-01T02:00:00.000Z", "**这一年的关键词**：十月写的");
        run(&mut s, 16384, "2026-03-31");
        assert!(s.request.1.contains("【去年的年度总结】（只用来和去年比，不要照抄）\n**这一年的关键词**：十月写的"));
        // the card: only 1 月 1～7 日, only a letter written in December or later
        assert_eq!(card(&s.picky, "2026-01-03").unwrap(), None);
        let whole = letter(&s.picky, "2025", "2025-12-31T13:00:00.000Z", "**这一年的关键词**：全年");
        assert_eq!(card(&s.picky, "2026-01-03").unwrap().map(|l| (l.year, l.session_id)), Some(("2025".to_string(), whole)));
        assert_eq!(card(&s.picky, "2026-01-08").unwrap(), None);
        assert_eq!(card(&s.picky, "2025-12-31").unwrap(), None);
        // a 私密 one still shows on the card, but isn't read for the comparison
        crate::picky::library::set_private(&s.picky, &[whole], true).unwrap();
        assert!(card(&s.picky, "2026-01-07").unwrap().is_some());
        assert_eq!(letter_of(&s.picky, "2025", false, false).unwrap().map(|l| l.session_id), Some(mid));
    }

    #[test]
    fn conversations_are_read_but_not_private_or_scheduled_ones() {
        let p = crate::picky::store::tests::memory_db();
        p.execute_batch(
            "INSERT INTO sessions (title, created_at) VALUES ('租房', '2026-03-02T02:00:00.000Z'), ('秘密', '2026-03-03T02:00:00.000Z'), ('三月第一句很长的问题', '2026-03-04T02:00:00.000Z');
             INSERT INTO sessions (title, created_at, scheduled) VALUES ('每周回顾 · 3/9', '2026-03-09T01:00:00.000Z', 1);
             UPDATE sessions SET private = 1 WHERE title = '秘密';
             INSERT INTO messages (session_id, role, content, state) VALUES (1, 'user', '上海哪里租房便宜？', 'complete'), (2, 'user', '不能说的事', 'complete'), (3, 'user', '三月第一句很长的问题，后面还有', 'complete');",
        )
        .unwrap();
        let t = talks(&p, "2026-03").unwrap();
        assert_eq!(t, "【和小拾的对话 2 个】\n- 03-02 租房：上海哪里租房便宜？\n- 03-04 三月第一句很长的问题");
        assert!(talks(&p, "2026-04").unwrap().is_empty());
        assert_eq!(talk_count(&p, "2026-01-01", "2026-12-31").unwrap(), 2);
        assert_eq!(month_spans(&["2026-02".into(), "2026-03".into(), "2026-04".into(), "2026-07".into(), "2026-09".into(), "2026-10".into()]), "2～4 月、7 月、9～10 月");
    }
}
