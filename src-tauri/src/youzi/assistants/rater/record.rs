//! 复盘 · 记录 (Rater, v0.7 R8): a conversation fills a review. youzi.db `rater_records` (migration
//! 0031): one row per review being recorded, on a 柚子 session of its own (`private`, and left out of
//! 柚子's list, 对话搜索, ⌘K and the 模块坞 count; deleted with the session).
//! - Start (`start`): the job, the date and the round (第 N 轮) → a session (its 思考 Rater's, 知识库 on) and a table
//!   with only those. The conversation is 柚子's own (chat.rs): its model is Rater's 「复盘 · 记录」
//!   (chat.rs `prepare` → `route`), its system message gets the task, the table so far (JSON), the JD,
//!   the job's newest 速查卡, its earlier reviews and the rules 复盘 (`system`), and it may call `TOOL`
//!   (`fill`) besides 柚子's tools (Seeker 资料 reads preps and reviews; 知识库, 记忆, skills as anywhere).
//! - `fill`: the whole table as the model has it now (a field it leaves out stays); `ready` once the user
//!   said it's complete: the page then asks 继续记录 / 确认. Filling and the user's edits (`edit`) aren't
//!   undone (they are the conversation's).
//! - 确认 (`save`): the table as the user last saw it becomes a review and the conversation is deleted, one
//!   undo step. An unfinished one stays under its job to go on with; 放弃 (`discard`) is one undo step.
//! - Not in the data file (conversations aren't).
use super::interview::{self, Asked, Interviews, Review, RESULTS};
use super::qbank;
use super::*;
use crate::youzi::assistants::seeker::{self, output::JobSnapshot, Workspace};
use crate::youzi::cloud::Level;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

pub const TOOL: &str = "pomelo_review_fill";
/// The rule groups it reads.
pub const READS: [&str; 1] = [REVIEW];
/// How a fill's result starts (the mock model answers to it).
pub const FILLED: &str = "复盘表已更新";
const TABLE: &str = "现在的表：";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Draft {
    pub session: i64,
    /// The table so far: job, date and round from the start; no id yet.
    pub review: Review,
    /// The model said it's complete.
    pub ready: bool,
    /// How many times the model filled it (the page asks again after a `ready` fill).
    pub fills: i64,
    pub created_at: String,
}

pub fn is_record(c: &Connection, session: i64) -> Result<bool, String> {
    c.query_row("SELECT EXISTS (SELECT 1 FROM rater_records WHERE session_id = ?1)", [session], |r| r.get(0)).map_err(err)
}

pub fn get(c: &Connection, session: i64) -> Result<Draft, String> {
    let body: String = c.query_row("SELECT body FROM rater_records WHERE session_id = ?1", [session], |r| r.get(0)).map_err(|_| "这次记录已经存好或放弃了".to_string())?;
    Ok(Draft { session, ..json(&body)? })
}

/// The unfinished ones, newest first.
pub fn list(c: &Connection) -> Result<Vec<Draft>, String> {
    let mut st = c.prepare("SELECT session_id, body FROM rater_records").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let mut out: Vec<Draft> = rows.into_iter().map(|(session, body)| Ok(Draft { session, ..json(&body)? })).collect::<Result<_, String>>()?;
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.session.cmp(&a.session)));
    Ok(out)
}

fn put(c: &Connection, d: &Draft) -> Result<(), String> {
    c.execute("UPDATE rater_records SET body = ?1 WHERE session_id = ?2", params![serde_json::to_string(&Draft { session: 0, ..d.clone() }).map_err(err)?, d.session]).map_err(err)?;
    Ok(())
}

/// A new record: its session (private, 思考 as Rater's, 知识库 on: every library) and a table with the job,
/// date and round.
pub fn start(c: &Connection, job: JobLink, date: &str, round: &str, level: &Level) -> Result<Draft, String> {
    let (date, round) = (date.trim(), round.trim());
    if date.is_empty() {
        return Err("请填写面试日期".into());
    }
    if round.is_empty() || round.chars().count() > 40 {
        return Err("请选第几轮".into());
    }
    let tx = Tx::begin(c)?;
    tx.execute("INSERT INTO sessions (title, title_source, private, thinking, effort, knowledge) VALUES (?1, 'user', 1, ?2, ?3, 1)", params![format!("复盘 · {} · {round}", job.label), level.thinking(), level.effort()]).map_err(err)?;
    let session = tx.last_insert_rowid();
    let d = Draft { session, review: Review { job, date: date.into(), round: round.into(), result: RESULTS[1].into(), ..Default::default() }, ready: false, fills: 0, created_at: interview::now(&tx)? };
    tx.execute("INSERT INTO rater_records (session_id, body) VALUES (?1, ?2)", params![session, serde_json::to_string(&Draft { session: 0, ..d.clone() }).map_err(err)?]).map_err(err)?;
    tx.commit()?;
    Ok(d)
}

/// The table cleaned: trimmed, empty questions dropped, a known result (else 待定), lengths as a review's.
fn clean(mut r: Review) -> Review {
    r.form = r.form.trim().chars().take(60).collect();
    r.round = r.round.trim().chars().take(40).collect();
    if r.result.is_empty() || !RESULTS.contains(&r.result.as_str()) {
        r.result = RESULTS[1].into();
    }
    r.items = r.items.into_iter().map(|a| Asked { question: a.question.trim().into(), answer: a.answer.trim().into(), feedback: a.feedback.trim().into() }).filter(|a| !a.question.is_empty()).collect();
    (r.feedback, r.thoughts) = (r.feedback.trim().into(), r.thoughts.trim().into());
    r
}

/// The model's `TOOL` call: the fields it gives replace the table's (items: the whole list). What it
/// gets back: the table now and what's still empty.
pub fn fill(c: &Connection, session: i64, args: &str) -> Result<(Draft, String), String> {
    let v: Value = serde_json::from_str(args).map_err(|_| "参数不是 JSON".to_string())?;
    let mut d = get(c, session)?;
    let text = |x: &Value, k: &str| x[k].as_str().map(|s| s.trim().to_string());
    let mut r = d.review.clone();
    if let Some(x) = text(&v, "form") {
        r.form = x;
    }
    if let Some(x) = text(&v, "result") {
        r.result = x;
    }
    if let Some(items) = v["items"].as_array() {
        r.items = items.iter().map(|a| Asked { question: text(a, "question").unwrap_or_default(), answer: text(a, "answer").unwrap_or_default(), feedback: text(a, "feedback").unwrap_or_default() }).collect();
    }
    if let Some(x) = text(&v, "feedback") {
        r.feedback = x;
    }
    if let Some(x) = text(&v, "thoughts") {
        r.thoughts = x;
    }
    d.review = clean(r);
    d.ready = v["ready"].as_bool().unwrap_or(false) && !d.review.items.is_empty();
    d.fills += 1;
    put(c, &d)?;
    let empty: Vec<&str> = [("形式", d.review.form.is_empty()), ("问了什么", d.review.items.is_empty()), ("面试官的反馈", d.review.feedback.is_empty()), ("我的判断", d.review.thoughts.is_empty())].into_iter().filter(|(_, e)| *e).map(|(k, _)| k).collect();
    let mut out = format!("{FILLED}：{} 个问题。{TABLE}{}", d.review.items.len(), table_json(&d.review));
    if !empty.is_empty() {
        out.push_str(&format!("\n还空着：{}。", empty.join("、")));
    }
    out.push_str(if d.ready { "\n已标为填好，用户会看一遍再保存。" } else { "\n用户确认填齐了再把 ready 设成 true。" });
    Ok((d, out))
}

/// The user's edits to the table (the page's 「返回对话」): kept for the conversation; not `ready` any more.
pub fn edit(c: &Connection, session: i64, review: &Review) -> Result<Draft, String> {
    let mut d = get(c, session)?;
    d.review = clean(Review { id: String::new(), job: d.review.job.clone(), analysis: None, created_at: String::new(), ..review.clone() });
    d.ready = false;
    put(c, &d)?;
    Ok(d)
}

/// 确认: the table (as the user last edited it) becomes a review of the record's job; the conversation goes.
pub fn save(c: &Connection, session: i64, review: &Review) -> Result<Review, String> {
    let tx = Tx::begin(c)?;
    let d = get(&tx, session)?;
    let r = Review { id: String::new(), job: d.review.job.clone(), analysis: None, created_at: String::new(), ..clean(review.clone()) };
    if r.items.is_empty() {
        return Err("还没记下问了什么".into());
    }
    let saved = interview::save_review(&tx, &r, Some(d.review.job.clone()))?;
    tx.execute("DELETE FROM sessions WHERE id = ?1", [session]).map_err(err)?;
    tx.commit()?;
    Ok(saved)
}

/// 放弃: the conversation and its table go.
pub fn discard(c: &Connection, session: i64) -> Result<(), String> {
    get(c, session)?;
    c.execute("DELETE FROM sessions WHERE id = ?1", [session]).map_err(err)?;
    Ok(())
}

/// The table as the model fills it (the shape of `TOOL`'s arguments).
fn table_json(r: &Review) -> String {
    json!({ "form": r.form, "result": r.result, "items": r.items.iter().map(|a| json!({ "question": a.question, "answer": a.answer, "feedback": a.feedback })).collect::<Vec<_>>(), "feedback": r.feedback, "thoughts": r.thoughts }).to_string()
}

pub fn tool() -> Value {
    let s = |d: &str| json!({ "type": "string", "description": d });
    json!({ "type": "function", "function": { "name": TOOL, "description": "把这场面试的复盘表写进去（整张表：已记的照抄，items 写全部问题，按问的顺序）。用户确认表填齐了，ready 写 true。", "parameters": { "type": "object", "properties": {
        "form": s("形式，如「视频 · 技术面」"),
        "result": { "type": "string", "enum": [RESULTS[1], RESULTS[2], RESULTS[3]], "description": "结果，还不知道写待定" },
        "items": { "type": "array", "description": "问了什么，按顺序", "items": { "type": "object", "properties": { "question": s("问题"), "answer": s("用户当时怎么答的，尽量原话"), "feedback": s("面试官当场的反应") }, "required": ["question"] } },
        "feedback": s("面试官的整体反馈（发生的事实）"),
        "thoughts": s("用户自己的判断（个人看法）"),
        "ready": { "type": "boolean", "description": "用户确认表填齐了" }
    }, "required": ["items"] } } })
}

/// The system message's part for a record's conversation: the task, the table so far, the JD, the
/// job's newest 速查卡, its earlier reviews, the rules 复盘.
pub fn system(w: &Workspace, all: &Interviews, d: &Draft, job: Option<&JobSnapshot>) -> String {
    let r = &d.review;
    let same = |j: &JobLink| j.id == r.job.id && j.created_at == r.job.created_at;
    let mut sources = vec![];
    if let Some(j) = job {
        sources.push(interview::jd_block(j));
    }
    if let Some(p) = all.preps.iter().find(|p| same(&p.job)) {
        let sheet: String = p.sheet.chars().take(1500).collect();
        sources.push(format!("这个岗位最近的速查卡（{}）：\n{sheet}", &p.created_at[..10.min(p.created_at.len())]));
    }
    let earlier: Vec<String> = all.reviews.iter().filter(|x| same(&x.job)).map(|x| format!("- {}{}：{}", x.date, if x.round.is_empty() { String::new() } else { format!(" · {}", x.round) }, x.items.iter().map(|a| a.question.as_str()).collect::<Vec<_>>().join("；"))).collect();
    if !earlier.is_empty() {
        sources.push(format!("这个岗位以前记过的复盘（问了什么）：\n{}", earlier.join("\n")));
    }
    format!(
        "# Rater · 记录复盘\n你在陪用户记一场面试的复盘，说明用中文，口语、简短。\n\n工作原则：\n{}\n\n任务：照规则里的「复盘记录」，通过对话把这张复盘表填好。每听到新内容就调用 {TOOL} 写进整张表（已记的照抄，items 写全部问题）；用户确认表填齐了，再调用一次并把 ready 写成 true。表里只写用户说过或确认过的。\n\n这场面试：{} · {} · {}（日期和第几轮用户已经选好了）\n{TABLE}{}\n\n资料：\n{}\n\n规则：\n{}",
        seeker::model::principles(&[]),
        r.job.label,
        r.date,
        r.round,
        table_json(r),
        if sources.is_empty() { "（没有）".into() } else { sources.join("\n\n") },
        qbank::rules_of(w, &READS, false)
    )
}

/// `system` for a record's session, read now (the job from 投石问路: gone → without the JD).
pub fn system_now(state: &AppState, session: i64) -> Result<String, String> {
    let (w, all, d) = state.youzi.with_db(|c| Ok((seeker::read(c)?, interview::read_all(c)?, get(c, session)?)))?;
    let job = seeker::output::job(state, d.review.job.id).ok().filter(|j| j.created_at == d.review.job.created_at);
    Ok(system(&w, &all, &d, job.as_ref()))
}

/// Where a record's reply goes: Rater's 「复盘 · 记录」 model; the session's 思考 follows Rater's.
pub async fn route(app: &AppHandle, session: i64) -> Result<crate::youzi::Route, String> {
    let state = app.state::<AppState>();
    let (w, opts) = state.youzi.with_db(|c| Ok((seeker::read(c)?, qbank::options(c)?)))?;
    let (model, level) = qbank::chosen_model(&w, &opts, "record")?;
    state.youzi.with_db(|c| c.execute("UPDATE sessions SET thinking = ?1, effort = ?3 WHERE id = ?2", params![level.thinking(), session, level.effort()]).map(|_| ()).map_err(err))?;
    Ok(crate::youzi::schedule::route(&state.youzi, Some(&model)).await?.0)
}

/// The mock model's fill: the table in the system message, the user's words as one more question; 「就这些」
/// marks it ready.
pub fn mock_fill(question: &str, system: &str) -> Option<Value> {
    let table = system.split(TABLE).nth(1)?.lines().next()?;
    let mut v: Value = serde_json::from_str(table).ok()?;
    let done = ["就这些", "没了", "填好了"].iter().any(|w| question.contains(w));
    if !done {
        let q: String = question.trim().chars().take(40).collect();
        if let Some(items) = v["items"].as_array_mut() {
            items.push(json!({ "question": q, "answer": "（模拟）我先讲了结论，再说了做法。", "feedback": "（模拟）点头，追问了数据" }));
        }
        v["form"] = json!("视频 · 技术面（模拟）");
    } else {
        v["feedback"] = json!("（模拟）整体不错，细节再准备");
        v["thoughts"] = json!("（模拟）项目题答得还行，数据口径没讲清");
    }
    v["ready"] = json!(done);
    Some(v)
}

#[tauri::command]
pub async fn youzi_rater_records(state: tauri::State<'_, AppState>) -> Result<Vec<Draft>, String> {
    state.youzi.with_db(|c| list(c))
}

#[tauri::command]
pub async fn youzi_rater_record(state: tauri::State<'_, AppState>, session: i64) -> Result<Draft, String> {
    state.youzi.with_db(|c| get(c, session))
}

/// A new record (like starting a conversation: not undone). Rater's 「复盘 · 记录」 needs a model.
#[tauri::command]
pub async fn youzi_rater_record_start(state: tauri::State<'_, AppState>, job: i64, date: String, round: String) -> Result<Draft, String> {
    let link = qbank::job_link(&seeker::output::job(&state, job)?);
    state.youzi.with_db(|c| {
        let (_, level) = qbank::chosen_model(&seeker::read(c)?, &qbank::options(c)?, "record")?;
        start(c, link, &date, &round, &level)
    })
}

#[tauri::command]
pub async fn youzi_rater_record_edit(state: tauri::State<'_, AppState>, session: i64, review: Review) -> Result<Draft, String> {
    state.youzi.with_db(|c| edit(c, session, &review))
}

#[tauri::command]
pub async fn youzi_rater_record_save(state: tauri::State<'_, AppState>, session: i64, review: Review) -> Result<Interviews, String> {
    if state.youzi.runs.busy_in(session) {
        return Err("柚子还在回复，等它说完再存".into());
    }
    state.with_youzi_undo("记复盘", |c| {
        save(c, session, &review)?;
        interview::read_all(c)
    })
}

#[tauri::command]
pub async fn youzi_rater_record_discard(state: tauri::State<'_, AppState>, session: i64) -> Result<Vec<Draft>, String> {
    if state.youzi.runs.busy_in(session) {
        return Err("柚子还在回复，等它说完或先停下".into());
    }
    state.with_youzi_undo("放弃这次复盘记录", |c| {
        discard(c, session)?;
        list(c)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::youzi::store::tests::memory_db;

    fn job() -> JobLink {
        JobLink { id: 4, created_at: "2026-10-01T00:00:00.000Z".into(), label: "柚子银行 · 数据分析师".into() }
    }

    #[test]
    fn a_record_is_filled_by_the_model_and_saved_as_a_review_without_its_conversation() {
        let c = memory_db();
        let d = start(&c, job(), "2026-10-20", "第 1 轮", &Level::word("high")).unwrap();
        // its session is private, left out of 柚子's list
        let (private, thinking): (bool, bool) = c.query_row("SELECT private, thinking FROM sessions WHERE id = ?1", [d.session], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert!(private && thinking && is_record(&c, d.session).unwrap());
        assert_eq!(c.query_row("SELECT effort FROM sessions WHERE id = ?1", [d.session], |r| r.get::<_, Option<String>>(0)).unwrap().as_deref(), Some("high"));
        assert!(crate::youzi::library::library(&c).unwrap().recent.iter().all(|s| s.id != d.session));
        assert_eq!(start(&c, job(), " ", "第 1 轮", &Level::off()).unwrap_err(), "请填写面试日期");

        // the model fills the whole table; a field it leaves out stays, an unknown result is 待定
        let (_, said) = fill(&c, d.session, &json!({ "form": " 视频 · 技术面 ", "result": "不知道", "items": [{ "question": "自我介绍", "answer": "我是林小柚。" }, { "question": " " }] }).to_string()).unwrap();
        assert!(said.starts_with(FILLED) && said.contains("还空着：面试官的反馈、我的判断"));
        let (now, _) = fill(&c, d.session, &json!({ "feedback": "节奏不错", "ready": true }).to_string()).unwrap();
        assert_eq!((now.review.form.as_str(), now.review.result.as_str(), now.review.items.len(), now.ready, now.fills), ("视频 · 技术面", "待定", 1, true, 2));
        // the user's edits: kept, not ready any more
        let edited = edit(&c, d.session, &Review { thoughts: "数据口径没讲清".into(), ..now.review.clone() }).unwrap();
        assert!(!edited.ready && edited.review.thoughts == "数据口径没讲清");
        assert_eq!(list(&c).unwrap().len(), 1);

        // 确认: one undo step — a review of the job, the conversation gone; undo brings the record back
        let mut h = crate::undo::History::default();
        crate::undo::tests::step(&c, &mut h, "记复盘", |c| save(c, d.session, &edited.review).map(|_| ()));
        let all = interview::read_all(&c).unwrap();
        assert_eq!((all.reviews.len(), all.reviews[0].job.id, all.reviews[0].round.as_str(), all.reviews[0].feedback.as_str()), (1, 4, "第 1 轮", "节奏不错"));
        assert!(list(&c).unwrap().is_empty());
        assert_eq!(c.query_row("SELECT count(*) FROM sessions", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        crate::undo::undo(&c, &mut h).unwrap();
        assert!(interview::read_all(&c).unwrap().reviews.is_empty() && is_record(&c, d.session).unwrap());

        // 放弃 takes it away; a table without questions isn't saved
        let e = start(&c, job(), "2026-10-27", "第 2 轮", &Level::off()).unwrap();
        assert!(save(&c, e.session, &e.review).unwrap_err().contains("问了什么"));
        discard(&c, e.session).unwrap();
        assert!(get(&c, e.session).is_err());
    }

    #[test]
    fn the_conversation_reads_the_table_jd_sheet_and_earlier_rounds_and_rules() {
        let w = seeker::read(&memory_db()).unwrap();
        let earlier = Review { id: "r1".into(), job: job(), date: "2026-10-10".into(), round: "第 1 轮".into(), items: vec![Asked { question: "为什么投我们？".into(), ..Default::default() }], ..Default::default() };
        let prep = interview::Prep { id: "p".into(), job: job(), sheet: "# 柚子银行 速查卡".into(), created_at: "2026-10-09T00:00:00Z".into(), ..Default::default() };
        let all = Interviews { preps: vec![prep], reviews: vec![earlier], summaries: vec![] };
        let d = Draft { review: Review { job: job(), date: "2026-10-20".into(), round: "第 2 轮".into(), result: "待定".into(), ..Default::default() }, ..Default::default() };
        let snap = JobSnapshot { id: 4, company: "柚子银行".into(), position: Some("数据分析师".into()), location: None, link: None, note: Some("JD：会 SQL".into()), created_at: job().created_at };
        let s = system(&w, &all, &d, Some(&snap));
        for want in [TOOL, "第 2 轮", "\"result\":\"待定\"", "JD：会 SQL", "# 柚子银行 速查卡", "为什么投我们？", "## 复盘记录"] {
            assert!(s.contains(want), "{want} missing");
        }
        // the mock model appends what the user said; 「就这些」 marks it ready
        let v = mock_fill("先问了自我介绍", &s).unwrap();
        assert_eq!((v["items"].as_array().unwrap().len(), v["ready"].as_bool()), (1, Some(false)));
        assert_eq!(mock_fill("就这些", &s).unwrap()["ready"], json!(true));
    }
}
