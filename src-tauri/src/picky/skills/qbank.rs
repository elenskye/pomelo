//! The three 题库 SKILLs in a 小拾 conversation (v0.7 阶段 ⑤): 「按柚子银行的 JD 出 5 道突击题」. The
//! conversation's model calls the tool with words (经历、岗位、题目里的字); Rust finds the records, runs
//! the same SKILL as Rater's page (assistants/rater/qbank.rs `generate`, with Rater's model) and puts
//! the questions — or the new answer — into the reply's card (act/card.rs `add_rater`). Confirming
//! writes them into the bank in one undo step. Hidden Rater, or a cloud model Seeker doesn't allow:
//! refused, like reading. With `replace` (a pending card item's 编号, act/card.rs): that question's answer
//! is rewritten by the same SKILL (润色, the request as the user's words) in its place.
use crate::picky::act::{self, Field};
use crate::picky::assistants::rater::{self, qbank, Question};
use crate::picky::assistants::seeker;
use crate::picky::data::{self, Access, Answer};
use crate::AppState;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

pub const RESUME_TOOL: &str = "pomelo_qbank_resume";
pub const GENERAL_TOOL: &str = "pomelo_qbank_general";
pub const JOB_TOOL: &str = "pomelo_qbank_job";

/// The question category a tool makes.
pub fn category(tool: &str) -> Option<&'static str> {
    match tool {
        RESUME_TOOL => Some("简历题"),
        GENERAL_TOOL => Some("通用题"),
        JOB_TOOL => Some("突击题"),
        _ => None,
    }
}

pub fn tool(name: &str) -> Value {
    let s = |d: &str| json!({ "type": "string", "description": d });
    let count = json!({ "type": "integer" });
    let question = s("改答案：那道题里的字");
    let request = s("用户原话，不加别的");
    let replace = s("重写卡片里还没确认的那道题的答案时写它的编号（如 12-1），按 request 改；出新题不写");
    let (description, props) = match name {
        RESUME_TOOL => ("按简历上的经历出面试题（或给已有简历题重写答案），进确认卡片", json!({ "materials": { "type": "array", "items": { "type": "string" }, "description": "经历名称，不写是简历草稿里的" }, "count": count, "question": question, "request": request, "replace": replace })),
        GENERAL_TOOL => ("出自我介绍、行为题这类通用面试题（或给已有通用题重写答案），进确认卡片", json!({ "count": count, "question": question, "request": request, "replace": replace })),
        _ => ("按投递岗位的 JD 出突击题（或给已有突击题重写答案），进确认卡片", json!({ "job": s("岗位的公司或岗位名"), "count": count, "question": question, "request": request, "replace": replace })),
    };
    json!({ "type": "function", "function": { "name": name, "description": description, "parameters": { "type": "object", "properties": props, "required": [] } } })
}

fn failed(label: &str, text: String) -> Answer {
    Answer { module: Some(data::RATER.into()), label: label.into(), state: "failed", text, records: vec![] }
}

fn words_in(text: &str, words: &str) -> bool {
    let t = text.to_lowercase();
    let ws: Vec<String> = words.split_whitespace().map(str::to_lowercase).collect();
    !ws.is_empty() && ws.iter().all(|w| t.contains(w.as_str()))
}

/// The card lines of a new question: 类别, 素材, 岗位, 改自, 标签, 答案.
fn new_fields(q: &Question, w: &seeker::Workspace, bank: &rater::Bank) -> Vec<Field> {
    let line = |label: &str, after: String| Field { label: label.into(), before: None, after, long: false };
    let mut fields = vec![line("类别", q.category.clone())];
    if let Some(m) = q.material.as_ref().and_then(|m| w.catalog.records.iter().find(|r| &r.id == m)) {
        fields.push(line("素材", m.title.clone()));
    }
    if let Some(j) = &q.job {
        fields.push(line("岗位", j.label.clone()));
    }
    if let Some(f) = q.from.as_ref().and_then(|f| bank.questions.iter().find(|x| &x.id == f)) {
        fields.push(line("改自", f.question.clone()));
    }
    if !q.tags.is_empty() {
        fields.push(line("标签", q.tags.join("、")));
    }
    fields.push(Field { label: "答案".into(), before: None, after: q.answer.clone(), long: true });
    fields
}

/// Runs a call: finds what it names, runs the SKILL, puts the result into run `run_id`'s card — or
/// with `replace`, rewrites that pending question's answer in its place (`cards`: the earlier
/// replies on the shown path). Also returns the run whose card changed.
pub async fn run(app: &AppHandle, access: &Access, run_id: i64, cards: &[i64], tool: &str, arguments: &str) -> (Answer, i64) {
    let Some(category) = category(tool) else { return (failed("题库 · 没有这个技能", "没有这个技能。".into()), run_id) };
    let label = format!("题库 · {category}");
    if !access.allows(data::RATER) {
        return (super::seeker::blocked(data::RATER, "Rater", access), run_id);
    }
    let (replace, arguments) = act::replacing(arguments);
    let args: Value = serde_json::from_str(&arguments).unwrap_or(Value::Null);
    let text = |k: &str| args[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let st = app.state::<AppState>();
    let (w, bank) = match st.picky.with_db(|c| Ok((seeker::read(c)?, rater::read(c)?))) {
        Ok(x) => x,
        Err(e) => return (failed(&label, format!("没读成：{e}")), run_id),
    };
    if let Some(r) = replace {
        return (rewrite(app, cards, run_id, &r, text("request").unwrap_or_default(), &w, &bank).await, run_id_of(&r, run_id));
    }
    (new_or_polish(app, run_id, tool, category, &label, &args, &w, &bank).await, run_id)
}

/// The run a 编号 names (a bare number: this reply).
fn run_id_of(r: &str, this: i64) -> i64 {
    r.trim().trim_start_matches("编号").split_once('-').and_then(|(a, _)| a.trim().parse().ok()).unwrap_or(this)
}

/// `replace`: the pending question's answer rewritten (润色 with the user's words) in its place.
#[allow(clippy::too_many_arguments)]
async fn rewrite(app: &AppHandle, cards: &[i64], run_id: i64, r: &str, request: String, w: &seeker::Workspace, bank: &rater::Bank) -> Answer {
    let st = app.state::<AppState>();
    let label = "题库 · 改卡片里的题";
    let found = st.picky.with_db(|c| {
        let (run, pos, item_tool) = act::card::find_ref(c, cards, run_id, r)?;
        if category(&item_tool).is_none() {
            return Err(format!("编号 {r} 不是题库的题：用原来的工具加 replace 改"));
        }
        Ok((run, pos, item_tool, act::card::rater_item(c, run, pos)?))
    });
    let (run, pos, item_tool, pending) = match found {
        Ok(x) => x,
        Err(e) => return failed(&format!("{label} · 没改成"), e),
    };
    let ask = qbank::Ask { kind: qbank::POLISH.into(), question: Some(pending.id.clone()), pending: Some(pending.clone()), request, ..Default::default() };
    let proposal = match qbank::generate(app, ask).await {
        Ok(p) => p,
        Err(e) => return failed(&format!("{label} · 没改成"), format!("没改成：{e}")),
    };
    let Some(after) = proposal.after else { return failed(&format!("{label} · 没改成"), "模型没有给出新的答案。".into()) };
    // a new question keeps its lines; a new answer of a bank question shows the bank's answer before
    let fields = match bank.questions.iter().find(|q| q.id == after.id) {
        Some(saved) if pending.revision > 0 => vec![Field { label: "答案".into(), before: Some(saved.answer.clone()), after: after.answer.clone(), long: true }],
        _ => new_fields(&after, w, bank),
    };
    match st.picky.with_db(|c| act::card::replace_rater(c, run, pos, &item_tool, &after.question, &fields, &after)) {
        Ok(()) => Answer {
            module: Some(data::RATER.into()),
            label: format!("{label} · 编号 {r}"),
            state: "ok",
            text: format!("已重写卡片里编号 {r} 那道题的答案（等用户确认后才写进 Rater 题库）：「{}」{}（约 {} 秒）\n（{}）\n回答里简单说改了什么、请用户在卡片里确认；不要把答案全文再抄一遍。", after.question, after.answer.chars().take(120).collect::<String>(), rater::seconds(&after.answer), [proposal.advice, proposal.note, format!("用的模型：{}", proposal.model)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("；")),
            records: vec![],
        },
        Err(e) => failed(&format!("{label} · 没改成"), format!("没改成：{e}")),
    }
}

/// New questions, or a bank question's new answer (`question`), into this reply's card.
#[allow(clippy::too_many_arguments)]
async fn new_or_polish(app: &AppHandle, run_id: i64, tool: &str, category: &str, label: &str, args: &Value, w: &seeker::Workspace, bank: &rater::Bank) -> Answer {
    let label = label.to_string();
    let text = |k: &str| args[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let st = app.state::<AppState>();
    let mut ask = qbank::Ask { kind: category.into(), count: args["count"].as_u64().unwrap_or(0) as usize, request: text("request").unwrap_or_default(), ..Default::default() };
    // a question to rewrite the answer of
    if let Some(words) = text("question") {
        let found: Vec<&Question> = bank.questions.iter().filter(|q| q.category == category && words_in(&q.question, &words)).collect();
        match found.as_slice() {
            [one] => {
                ask.kind = qbank::POLISH.into();
                ask.question = Some(one.id.clone());
            }
            [] => return failed(&format!("{label} · 没找到这道题"), format!("{category}里没有含「{words}」的题。请问用户是哪一道，或者不写 question 出新题。")),
            many => return failed(&format!("{label} · 有几道"), format!("含「{words}」的{category}有 {} 道：{}。请问用户是哪一道。", many.len(), many.iter().take(5).map(|q| format!("「{}」", q.question)).collect::<Vec<_>>().join("、"))),
        }
    }
    if ask.kind == "简历题" {
        let mats = qbank::materials(w);
        for name in args["materials"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
            let hit: Vec<&&seeker::catalog::Record> = mats.iter().filter(|r| words_in(&r.title, name) || r.title.to_lowercase().split_whitespace().next().is_some_and(|t| name.to_lowercase().contains(t))).collect();
            match hit.as_slice() {
                [] => return failed(&format!("{label} · 没找到经历"), format!("中文素材里没有「{name}」。有的经历：{}。", mats.iter().filter(|r| r.category != "技能" && r.category != "教育").map(|r| format!("「{}」", r.title)).collect::<Vec<_>>().join("、"))),
                found => ask.materials.extend(found.iter().map(|r| r.id.clone())),
            }
        }
        ask.materials.dedup();
    }
    if ask.kind == "突击题" {
        let Some(words) = text("job") else { return failed(&format!("{label} · 没说岗位"), "突击题要说是哪个岗位（公司或岗位名）。".into()) };
        let jobs: Vec<(i64, String)> = st.with_db(|c| {
            let mut s = c.prepare("SELECT id, company || ' · ' || coalesce(position, '') FROM jobs ORDER BY id DESC").map_err(crate::modules::common::err)?;
            let rows = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(crate::modules::common::err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(crate::modules::common::err)
        }).unwrap_or_default();
        let found: Vec<&(i64, String)> = jobs.iter().filter(|(_, name)| words_in(name, &words)).collect();
        match found.as_slice() {
            [one] => ask.job = Some(one.0),
            [] => return failed(&format!("{label} · 没找到岗位"), format!("投石问路里没有含「{words}」的岗位。请问用户是哪个。")),
            many => return failed(&format!("{label} · 有几个岗位"), format!("含「{words}」的岗位有 {} 个：{}。请问用户是哪一个。", many.len(), many.iter().take(5).map(|(_, n)| format!("「{}」", n.trim_end_matches(" · "))).collect::<Vec<_>>().join("、"))),
        }
    }
    let proposal = match qbank::generate(app, ask.clone()).await {
        Ok(p) => p,
        Err(e) => return failed(&format!("{label} · 没出成"), format!("没出成：{e}")),
    };
    let mut first = None;
    let added: Result<Vec<String>, String> = st.picky.with_db(|c| {
        let mut lines = vec![];
        if let (Some(before), Some(after)) = (&proposal.before, &proposal.after) {
            let n = act::card::add_rater(c, run_id, tool, &before.question, &[Field { label: "答案".into(), before: Some(before.answer.clone()), after: after.answer.clone(), long: true }], after)?;
            first.get_or_insert(n);
            lines.push(format!("编号 {} · 改答案「{}」：{}", act::card::reference(run_id, n as i64 - 1), before.question, after.answer.chars().take(120).collect::<String>()));
        }
        for q in &proposal.questions {
            let n = act::card::add_rater(c, run_id, tool, &q.question, &new_fields(q, w, bank), q)?;
            first.get_or_insert(n);
            lines.push(format!("编号 {} · {}（约 {} 秒）", act::card::reference(run_id, n as i64 - 1), q.question, rater::seconds(&q.answer)));
        }
        Ok(lines)
    });
    match added {
        Ok(lines) => {
            let n = lines.len();
            let head = if proposal.after.is_some() { "已把新的答案放进确认卡片".to_string() } else { format!("已把 {n} 道{category}放进确认卡片") };
            let note = [proposal.advice.clone(), proposal.note.clone(), format!("用的模型：{}", proposal.model)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("；");
            Answer {
                module: Some(data::RATER.into()),
                label: format!("{label} · {}", if proposal.after.is_some() { "改答案".to_string() } else { format!("{n} 道") }),
                state: "ok",
                text: format!("{head}（从第 {} 条起，等用户确认后才写进 Rater 题库）：\n{}\n（{note}）\n回答里简单说出了哪些、请用户在卡片里确认；不要把答案全文再抄一遍，也不要说已经写进题库；编号只用来改卡片，回答里不用提。", first.unwrap_or(1), lines.join("\n")),
                records: vec![],
            }
        }
        Err(e) => failed(&format!("{label} · 没放进卡片"), format!("没放进卡片：{e}")),
    }
}
