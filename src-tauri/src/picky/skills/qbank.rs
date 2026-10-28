//! The three 题库 SKILLs in a 小拾 conversation (v0.7 阶段 ⑤): 「按柚子银行的 JD 出 5 道突击题」. The
//! conversation's model calls the tool with words (经历、岗位、题目里的字); Rust finds the records, runs
//! the same SKILL as Rater's page (assistants/rater/qbank.rs `generate`, with Rater's model) and puts
//! the questions — or the new answer — into the reply's card (act/card.rs `add_rater`). Confirming
//! writes them into the bank in one undo step. Hidden Rater, or a cloud model Seeker doesn't allow:
//! refused, like reading.
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
    let (description, props) = match name {
        RESUME_TOOL => ("按简历上的经历出面试题（或给已有简历题重写答案），进确认卡片", json!({ "materials": { "type": "array", "items": { "type": "string" }, "description": "经历名称，不写是简历草稿里的" }, "count": count, "question": question, "request": request })),
        GENERAL_TOOL => ("出自我介绍、行为题这类通用面试题（或给已有通用题重写答案），进确认卡片", json!({ "count": count, "question": question, "request": request })),
        _ => ("按投递岗位的 JD 出突击题（或给已有突击题重写答案），进确认卡片", json!({ "job": s("岗位的公司或岗位名"), "count": count, "question": question, "request": request })),
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

/// Runs a call: finds what it names, runs the SKILL, puts the result into run `run_id`'s card.
pub async fn run(app: &AppHandle, access: &Access, run_id: i64, tool: &str, arguments: &str) -> Answer {
    let Some(category) = category(tool) else { return failed("题库 · 没有这个技能", "没有这个技能。".into()) };
    let label = format!("题库 · {category}");
    if !access.allows(data::RATER) {
        return super::seeker::blocked(data::RATER, "Rater", access);
    }
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let text = |k: &str| args[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let st = app.state::<AppState>();
    let (w, bank) = match st.picky.with_db(|c| Ok((seeker::read(c)?, rater::read(c)?))) {
        Ok(x) => x,
        Err(e) => return failed(&label, format!("没读成：{e}")),
    };
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
        let mats = qbank::materials(&w);
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
    let material = |id: &Option<String>| id.as_ref().and_then(|m| w.catalog.records.iter().find(|r| &r.id == m)).map(|r| r.title.clone());
    let from = |id: &Option<String>| id.as_ref().and_then(|f| bank.questions.iter().find(|q| &q.id == f)).map(|q| q.question.clone());
    let mut first = None;
    let added: Result<Vec<String>, String> = st.picky.with_db(|c| {
        let mut lines = vec![];
        if let (Some(before), Some(after)) = (&proposal.before, &proposal.after) {
            let n = act::card::add_rater(c, run_id, tool, &before.question, &[Field { label: "答案".into(), before: Some(before.answer.clone()), after: after.answer.clone(), long: true }], after)?;
            first.get_or_insert(n);
            lines.push(format!("改答案「{}」：{}", before.question, after.answer.chars().take(120).collect::<String>()));
        }
        for q in &proposal.questions {
            let mut fields = vec![Field { label: "类别".into(), before: None, after: q.category.clone(), long: false }];
            if let Some(m) = material(&q.material) {
                fields.push(Field { label: "素材".into(), before: None, after: m, long: false });
            }
            if let Some(j) = &q.job {
                fields.push(Field { label: "岗位".into(), before: None, after: j.label.clone(), long: false });
            }
            if let Some(f) = from(&q.from) {
                fields.push(Field { label: "改自".into(), before: None, after: f, long: false });
            }
            if !q.tags.is_empty() {
                fields.push(Field { label: "标签".into(), before: None, after: q.tags.join("、"), long: false });
            }
            fields.push(Field { label: "答案".into(), before: None, after: q.answer.clone(), long: true });
            let n = act::card::add_rater(c, run_id, tool, &q.question, &fields, q)?;
            first.get_or_insert(n);
            lines.push(format!("{}. {}（约 {} 秒）", lines.len() + 1, q.question, rater::seconds(&q.answer)));
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
                text: format!("{head}（从第 {} 条起，等用户确认后才写进 Rater 题库）：\n{}\n（{note}）\n回答里简单说出了哪些、请用户在卡片里确认；不要把答案全文再抄一遍，也不要说已经写进题库。", first.unwrap_or(1), lines.join("\n")),
                records: vec![],
            }
        }
        Err(e) => failed(&format!("{label} · 没放进卡片"), format!("没放进卡片：{e}")),
    }
}
