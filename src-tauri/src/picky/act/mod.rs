//! 小拾动手 (v0.4 phases 4–5): 小拾 proposes, the user confirms, Rust writes.
//! - A write tool only proposes. Its arguments are read like 「一句话填好」 (make.rs: names matched to
//!   existing 分类 / 分组 / 状态 / 模板, a date word in the question worked out by Rust); an edit names
//!   its record with words and Rust finds it (one → it, several → candidates to pick on the card, none
//!   → the model is told to ask). Each candidate runs through the module's own functions in a
//!   transaction that is rolled back (ops.rs `apply`): what passes is exactly what confirming writes.
//!   The proposal joins the reply's card (one card per reply, items in order).
//! - Confirming writes the ticked items (the picked candidate) with the same `apply` in one
//!   `with_undo` step (one ⌘Z); cancelling writes nothing. The card, what was ticked and what got
//!   written stay in picky.db (card.rs), so reopening and exporting show them; a record that is gone or
//!   (an edit) back to how it was shows as 已撤销.
//! - Same rules as reading: hidden modules are refused; for a cloud model, modules not allowed there
//!   (钱来钱往 by default) are refused with the sentence the reply starts with. Nothing deletes;
//!   芝麻开门, starting / stopping models, import / export and backups have no tools.
//! - Tool definitions carry no user data; the names to pick from and the calendar go in the system
//!   message (`instructions`), only for modules this model may write.

pub mod card;
mod make;
mod ops;

pub use card::{add, card_of, mark_gone, Card};
pub use ops::Op;

use super::assist;
use super::data::{self, Access, Answer};
use crate::modules::common::Tx;
use crate::modules::{drawing, finance, jobs, kinds, packing, todo};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const ADD_ENTRY: &str = "pomelo_add_entry";
pub const EDIT_ENTRY: &str = "pomelo_edit_entry";
pub const SET_BALANCE: &str = "pomelo_set_balance";
pub const ADD_TASK: &str = "pomelo_add_task";
pub const EDIT_TASK: &str = "pomelo_edit_task";
pub const ADD_JOB: &str = "pomelo_add_job";
pub const EDIT_JOB: &str = "pomelo_edit_job";
pub const ADD_WORK: &str = "pomelo_add_work";
pub const EDIT_WORK: &str = "pomelo_edit_work";
pub const FINISH_LESSON: &str = "pomelo_finish_lesson";
pub const ADD_TRIP: &str = "pomelo_add_trip";
pub const EDIT_TRIP: &str = "pomelo_edit_trip";
pub const ADD_NOTE: &str = "pomelo_add_note";
pub const EDIT_NOTE: &str = "pomelo_edit_note";

struct Act {
    tool: &'static str,
    /// 书影: books, or films by the `kind` argument.
    module: &'static str,
    /// 「记一笔收支」
    label: &'static str,
}

const ACTS: &[Act] = &[
    Act { tool: ADD_ENTRY, module: "finance", label: "记一笔收支" },
    Act { tool: EDIT_ENTRY, module: "finance", label: "改一笔收支" },
    Act { tool: SET_BALANCE, module: "finance", label: "更新余额" },
    Act { tool: ADD_TASK, module: "todo", label: "新任务" },
    Act { tool: EDIT_TASK, module: "todo", label: "改任务" },
    Act { tool: ADD_JOB, module: "jobs", label: "新投递" },
    Act { tool: EDIT_JOB, module: "jobs", label: "改投递" },
    Act { tool: ADD_WORK, module: "books", label: "新书影" },
    Act { tool: EDIT_WORK, module: "books", label: "改书影" },
    Act { tool: FINISH_LESSON, module: "drawing", label: "画完一节" },
    Act { tool: ADD_TRIP, module: "packing", label: "新出行" },
    Act { tool: EDIT_TRIP, module: "packing", label: "改出行清单" },
    Act { tool: ADD_NOTE, module: "notes", label: "记一条笔记" },
    Act { tool: EDIT_NOTE, module: "notes", label: "改笔记" },
];

fn act(tool: &str) -> Option<&'static Act> {
    ACTS.iter().find(|a| a.tool == tool)
}

pub fn is_act(tool: &str) -> bool {
    act(tool).is_some()
}

fn is_work(tool: &str) -> bool {
    tool == ADD_WORK || tool == EDIT_WORK
}

/// The module a call writes (书影 by its `kind`; None: not said yet).
fn module_for(tool: &str, args: &Value) -> Option<&'static str> {
    let a = act(tool)?;
    if !is_work(tool) {
        return Some(a.module);
    }
    match args.get("kind").and_then(Value::as_str) {
        Some("book" | "书") => Some("books"),
        Some("film" | "影视" | "电影" | "剧") => Some("films"),
        _ => None,
    }
}

/// What a card item is called: 「新任务」, 「加子任务」, 「新书」, 「改影视」.
fn label_of(tool: &str, module: &str, op: Option<&Op>) -> String {
    match (tool, module) {
        (ADD_TASK, _) if matches!(op, Some(Op::AddTask { patch }) if patch.get("parentId").is_some()) => "加子任务".into(),
        (ADD_WORK, "books") => "新书".into(),
        (ADD_WORK, _) => "新影视".into(),
        (EDIT_WORK, "books") => "改书".into(),
        (EDIT_WORK, _) => "改影视".into(),
        _ => act(tool).map_or("提议", |a| a.label).into(),
    }
}

// ---------- tools and instructions ----------

/// The write tools of modules this answer is offered (hidden ones aren't; cloud-refused ones are, to say so).
pub fn tools(access: &Access) -> Vec<Value> {
    let offered: Vec<&str> = access.offered().iter().map(|m| m.key).collect();
    let s = |d: &str| json!({ "type": "string", "description": d });
    let n = |d: &str| json!({ "type": "number", "description": d });
    let words = |d: &str| json!({ "type": "array", "items": { "type": "string" }, "description": d });
    let kinds: Vec<&str> = [("books", "book"), ("films", "film")].iter().filter(|(m, _)| offered.contains(m)).map(|(_, k)| *k).collect();
    let kind = json!({ "type": "string", "enum": kinds });
    ACTS.iter()
        .filter(|a| offered.contains(&a.module) || (is_work(a.tool) && !kinds.is_empty()))
        .map(|a| {
            let (description, props, required): (&str, Value, Vec<&str>) = match a.tool {
                ADD_ENTRY => (
                    "记一笔收支（几笔就调几次）",
                    json!({ "flow": { "type": "string", "enum": ["out", "in"] }, "amount": n("元"), "currency": s("CNY/SGD/HKD/USD，没说不写"), "day": s("没说不写"), "kind": s("分类"), "note": s("备注") }),
                    vec!["amount"],
                ),
                EDIT_ENTRY => (
                    "改记过的一笔收支",
                    json!({ "find": s("那笔的备注或分类，如 午饭"), "on": s("那笔的日期"), "was": n("那笔的金额"), "amount": n("新金额"), "kind": s("新分类"), "day": s("新日期") }),
                    vec![],
                ),
                SET_BALANCE => ("更新账户余额", json!({ "account": s("账户"), "amount": n("余额（元）"), "day": s("没说不写") }), vec!["account", "amount"]),
                ADD_TASK => (
                    "新任务，有 parent 就是子任务（几件就调几次）",
                    json!({ "title": s("去掉时间词"), "due_date": s("截止或要做的那天"), "start_date": s("说了起止才写"), "group": s("分组"), "goal": s("目标"), "parent": s("上一层任务名"), "note": s("说明") }),
                    vec!["title"],
                ),
                EDIT_TASK => (
                    "已有任务：完成、打卡、记进度，或改日期、分组、目标",
                    json!({ "task": s("任务名里的字"), "done": { "type": "boolean", "description": "完成 / 今天打卡 / 进度 +1" }, "amount": n("进度记几"), "due_date": s("新截止日"), "start_date": s("新开始日"), "group": s("新分组"), "goal": s("新目标") }),
                    vec!["task"],
                ),
                ADD_JOB => (
                    "新投递",
                    json!({ "company": s("公司"), "position": s("岗位"), "location": s("城市"), "channel": s("渠道"), "status": s("走到哪一步：投了是第一步，只是想投不写"), "day": s("那一步的日期"), "note": s("备注") }),
                    vec!["company"],
                ),
                EDIT_JOB => ("已有投递：勾上走到的那一步，或加备注", json!({ "job": s("公司或岗位"), "status": s("走到哪一步"), "day": s("那一步的日期"), "note": s("要加的备注") }), vec!["job"]),
                ADD_WORK => (
                    "书单 / 影视单里新加一部",
                    json!({ "kind": kind, "title": s("名称"), "creator": s("作者 / 导演，确定才写"), "country": s("国家，确定才写"), "type": s("类型"), "status": s("todo / doing / done"), "rating": n("1～5 星"), "year": s("读完看完的年份") }),
                    vec!["kind", "title"],
                ),
                EDIT_WORK => (
                    "书单 / 影视单里已有的：改状态、星级、年份",
                    json!({ "kind": kind, "title": s("名称"), "status": s("todo / doing / done"), "rating": n("1～5 星"), "year": s("读完看完的年份") }),
                    vec!["kind", "title"],
                ),
                FINISH_LESSON => ("勾上画完的一节课", json!({ "course": s("课程"), "lesson": s("课节编号或名称，不写是下一节") }), vec![]),
                ADD_TRIP => ("新出行", json!({ "name": s("目的地加天数"), "depart_on": s("出发日期"), "template": s("模板") }), vec!["name"]),
                EDIT_TRIP => ("出行清单：加上物品，或勾上装好的", json!({ "trip": s("出行，不写是还没出发的"), "add": words("要加的"), "pack": words("装好的") }), vec![]),
                ADD_NOTE => ("记一条笔记", json!({ "content": s("全文，用户原话") }), vec!["content"]),
                _ => ("改已有的笔记：给改好的全文，或把一段换掉", json!({ "find": s("笔记里的字"), "content": s("改好后的全文"), "old": s("要换掉的原文"), "new": s("换成") }), vec!["find"]),
            };
            json!({ "type": "function", "function": { "name": a.tool, "description": description, "parameters": { "type": "object", "properties": props, "required": required } } })
        })
        .collect()
}

fn joined(v: Vec<String>) -> String {
    if v.is_empty() { "（没有）".into() } else { v.join("、") }
}

/// The names a module's tools pick from.
fn names(c: &Connection, module: &str, today: &str) -> Result<String, String> {
    let kind_names = |m: &str| -> Result<String, String> { Ok(joined(kinds::list(c, m)?.into_iter().map(|k| k.name).collect())) };
    Ok(match module {
        "finance" => format!(
            "钱来钱往：支出分类 {}；收入分类 {}；账户 {}",
            kind_names(finance::OUT_KINDS)?,
            kind_names(finance::IN_KINDS)?,
            joined(finance::accounts(c)?.into_iter().filter(|a| !a.closed).map(|a| a.name).collect())
        ),
        "todo" => {
            let groups = todo::groups(c)?;
            let goals: Vec<String> = todo::goals(c)?
                .into_iter()
                .filter(|g| g.status == todo::GoalStatus::Active)
                .map(|g| format!("{}（{}）", g.name, groups.iter().find(|x| x.id == g.group_id).map_or("", |x| x.name.as_str())))
                .collect();
            format!("一步一步：分组 {}；进行中的目标 {}", joined(groups.iter().map(|g| g.name.clone()).collect()), joined(goals))
        }
        "jobs" => format!(
            "投石问路：时间线的状态（按先后）{}；渠道 {}",
            joined(jobs::statuses(c)?.into_iter().filter(|s| s.stage != jobs::Stage::Wish).map(|s| s.name).collect()),
            kind_names(jobs::CHANNELS)?
        ),
        "books" => format!("摸鱼读读：类型 {}", kind_names("books")?),
        "films" => format!("好奇看看：类型 {}", kind_names("films")?),
        "drawing" => format!("随意画画：课程 {}", joined(drawing::courses(c)?.into_iter().map(|x| x.name).collect())),
        "packing" => format!(
            "出门走走：模板 {}；还没出发的出行 {}",
            joined(packing::templates(c)?.into_iter().map(|t| t.name).collect()),
            joined(packing::trips(c)?.into_iter().filter(|t| t.depart_on.as_deref().is_none_or(|d| d >= today)).map(|t| t.name).collect())
        ),
        _ => String::new(),
    })
}

/// The part of the system message about writing: what may be proposed, the calendar and the names
/// of the modules this model may write (empty when none).
pub fn instructions(conn: &Connection, access: &Access, today: &str) -> Result<String, String> {
    let writable: Vec<&Act> = ACTS.iter().filter(|a| access.allows(a.module) || (is_work(a.tool) && access.allows("films"))).collect();
    if writable.is_empty() {
        return Ok(String::new());
    }
    let mut s = format!(
        "【记数据】你可以帮用户在柚儿园里新建、修改记录：{}。用对应的工具「提议」，柚儿园检查后放进回复下面的确认卡片，用户点「确认」才会写进去。\n\
         - 用户明确要记、要改时才提议（「午饭 35」「想读《三体》」「甲公司进面试了」「交房租做完了」都是要记）；一句话里有几件事就调用几次。缺了必须的信息（比如金额）就先问，不要编。\n\
         - 改、完成、勾上已有的记录时，用名称里的字说是哪一条（任务名、公司、书名、笔记里的字、收支的备注），柚儿园来找：找到几条会列在卡片里让用户选；一条都没找到会告诉你，那就问用户。\n\
         - 分类、分组、目标、状态、渠道、类型、课程、模板、账户的名称从下面列出的里选；日期写 YYYY-MM-DD，没说的不写。\n\
         - 提议之后告诉用户已经放进卡片、请确认，不要说已经记好、改好了。不能删除任何东西。\n{}",
        writable.iter().map(|a| a.label).collect::<Vec<_>>().join("、"),
        assist::calendar(today)
    );
    let mut modules: Vec<&str> = writable.iter().map(|a| a.module).collect();
    if writable.iter().any(|a| is_work(a.tool)) {
        modules.push("films");
    }
    modules.dedup();
    for m in modules.into_iter().filter(|m| access.allows(m)) {
        let line = names(conn, m, today)?;
        if !line.is_empty() {
            s.push_str(&format!("\n- {line}"));
        }
    }
    Ok(s)
}

// ---------- proposing ----------

/// One line of an item: a value, or 改前 → 改后.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    pub after: String,
    /// Long text: shown with its changes marked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub long: bool,
}

/// One way to do it: what the card shows and what confirming runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub title: String,
    pub fields: Vec<Field>,
    pub op: Op,
}

/// A checked proposal: one choice, or several records the words fit (the user picks).
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub tool: String,
    pub module: String,
    pub label: String,
    pub choices: Vec<Choice>,
    /// Candidates beyond the listed ones.
    pub more: usize,
    /// Told to the model (items it couldn't find…).
    pub note: Option<String>,
}

/// One candidate tried in a rolled-back transaction: its lines before and after.
fn trial(c: &Connection, op: &Op, extra: Vec<Field>, today: &str) -> Result<(Choice, String), String> {
    let target = op.before(c)?;
    let before = match &target {
        Some(w) => Some(ops::show(c, w, today, false)?),
        None => None,
    };
    let (w, after) = {
        let tx = Tx::begin(c)?; // never committed: a trial
        let w = ops::apply(&tx, op, today)?;
        let after = ops::show(&tx, &w, today, target.is_none())?;
        (w, after)
    };
    let mut fields = ops::fields(&w.kind, before.as_deref(), &after);
    let title = match &before {
        None => ops::name_of(&w.kind, &after),
        Some(b) => {
            let changed = ops::changes(&fields);
            if changed.is_empty() && extra.is_empty() {
                return Err(format!("「{}」已经是这样了，不用改", ops::name_of(&w.kind, b)));
            }
            let what = [changed, extra.iter().map(|f| format!("{} {}", f.label, f.after)).collect::<Vec<_>>().join("；")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("；");
            format!("{}：{what}", ops::name_of(&w.kind, b))
        }
    };
    fields.extend(extra);
    Ok((Choice { title, fields, op: op.clone() }, w.module))
}

/// Checks a call against the module's own rules (rolled-back trials); what the card will show.
pub fn propose(conn: &Connection, tool: &str, arguments: &str, question: &str, today: &str) -> Result<Proposal, String> {
    act(tool).ok_or("没有这个工具")?;
    let args: Value = serde_json::from_str(arguments).map_err(|_| "参数不是有效的 JSON".to_string())?;
    if !args.is_object() {
        return Err("参数要是一个 JSON 对象".into());
    }
    let mut plan = make::plan(conn, tool, &args, question, today)?;
    // a date the model worked out differently: say which one counts (else it argues with the card)
    if let Some(d) = assist::date_in(question, today) {
        let differs = args.as_object().is_some_and(|o| o.values().filter_map(Value::as_str).any(|v| v.len() == 10 && v.as_bytes()[4] == b'-' && v != d && v != today));
        if differs {
            let said = format!("问题里的日期是 {d}（按日历算的，卡片用的是它）");
            plan.note = Some(plan.note.map_or(said.clone(), |n| format!("{n}；{said}")));
        }
    }
    let (mut choices, mut module, mut first_err) = (vec![], None, None);
    for (op, extra) in plan.ops {
        match trial(conn, &op, extra, today) {
            Ok((ch, m)) => {
                module.get_or_insert(m);
                choices.push(ch);
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    let module = module.ok_or_else(|| first_err.unwrap_or_else(|| "没有可以提议的".into()))?;
    let label = label_of(tool, &module, choices.first().map(|c| &c.op));
    Ok(Proposal { tool: tool.into(), module, label, choices, more: plan.more, note: plan.note })
}

/// What the model gets back (and whether the proposal went into the card).
pub fn answer(tool: &str, result: Result<(usize, &Proposal), String>) -> Answer {
    let a = act(tool);
    match result {
        Ok((n, p)) => {
            let note = p.note.as_deref().map(|x| format!("（{x}）")).unwrap_or_default();
            let text = match p.choices.as_slice() {
                // the card's own lines, so the reply says what will really be written
                [one] => format!(
                    "已放进确认卡片（第 {n} 条，等用户确认）：{} · {}（{}）{note}",
                    p.label,
                    one.title,
                    one.fields.iter().map(|f| match &f.before {
                        Some(b) if !f.long => format!("{} {b} → {}", f.label, f.after),
                        _ if f.long => format!("{}：{}", f.label, f.after.chars().take(80).collect::<String>()),
                        _ => format!("{} {}", f.label, f.after),
                    }).collect::<Vec<_>>().join("；")
                ),
                many => format!(
                    "已放进确认卡片（第 {n} 条，等用户在卡片里选是哪一条再确认）：找到 {} 条可能是的{}：{}{note}",
                    many.len() + p.more,
                    if p.more > 0 { format!("，列了最相关的 {} 条", many.len()) } else { String::new() },
                    many.iter().map(|c| c.title.as_str()).collect::<Vec<_>>().join("；")
                ),
            };
            Answer { module: a.map(|_| static_module(&p.module).to_string()), label: format!("提议{} · {}", p.label, p.choices[0].title), state: "ok", text, records: vec![] }
        }
        Err(e) => Answer { module: a.map(|a| a.module.to_string()), label: format!("提议{} · 没成", a.map_or("", |a| a.label)), state: "failed", text: format!("没放进卡片：{e}"), records: vec![] },
    }
}

fn static_module(m: &str) -> &'static str {
    data::MODULES.iter().find(|x| x.key == m).map_or("", |x| x.key)
}

/// Hidden / cloud rules for a write call: None = allowed, else the answer that refuses.
pub fn refused(access: &Access, tool: &str, arguments: &str) -> Option<Answer> {
    let args: Value = serde_json::from_str(arguments).unwrap_or_default();
    let module = module_for(tool, &args)?;
    let m = data::MODULES.iter().find(|m| m.key == module)?;
    let label = label_of(tool, module, None);
    if access.hidden.iter().any(|h| h == module) {
        return Some(Answer { module: Some(m.key.into()), label: format!("提议{label} · 已隐藏"), state: "blocked", text: format!("没有提议：{}在模块坞里隐藏了，小拾不改它的数据。", m.label), records: vec![] });
    }
    if !access.allows(module) {
        return Some(Answer {
            module: Some(m.key.into()),
            label: format!("提议{label} · 不发给云端"),
            state: "blocked",
            text: format!("没有提议：{0}的数据不发给云端模型，所以用云端模型时不能记、不能改。请在回答开头写明「{0}不发给云端，这次不能记」，并提醒可以换本地模型再说。", m.label),
            records: vec![],
        });
    }
    None
}

/// A 新书影 call to look up first (书单 / 影单检索): (module, title, creator hint).
pub fn work_to_look_up(tool: &str, arguments: &str) -> Option<(&'static str, String, Option<String>)> {
    if tool != ADD_WORK {
        return None;
    }
    let args: Value = serde_json::from_str(arguments).ok()?;
    let module = module_for(tool, &args)?;
    let title = args["title"].as_str()?.trim().trim_matches(|c| c == '《' || c == '》').to_string();
    (!title.is_empty()).then(|| (module, title, args["creator"].as_str().map(String::from)))
}

/// The call's arguments with what the lookup found (it wins over the model's own knowledge).
pub fn with_work(arguments: &str, w: Option<&super::skills::works::Work>) -> String {
    let Some(w) = w else { return arguments.to_string() };
    let mut args: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
    for (k, v) in [("creator", &w.creator), ("country", &w.country), ("type", &w.kind)] {
        if let Some(v) = v {
            args[k] = json!(v);
        }
    }
    args.to_string()
}

// ---------- mock model ----------

/// Leading date words of a mock title (「明天」「下周五前」).
fn without_date_words(s: &str) -> &str {
    let mut s = s.trim_start_matches(['：', ':', '，', ',', ' ']);
    for w in ["大后天", "后天", "明天", "今天", "昨天", "前天"] {
        s = s.strip_prefix(w).unwrap_or(s);
    }
    for w in ["下下周", "下周", "这周", "本周", "周", "星期"] {
        if let Some(r) = s.strip_prefix(w).and_then(|rest| rest.strip_prefix(|c: char| "一二三四五六日天".contains(c))) {
            s = r;
            break;
        }
    }
    s.trim_start_matches(['前', '之', '的', '：', ':', ' ']).trim()
}

fn items_in(s: &str) -> Vec<String> {
    s.split(['、', '，', ',', '和']).map(|x| x.trim().trim_end_matches(['。', '了']).to_string()).filter(|x| !x.is_empty()).collect()
}

fn after<'a>(q: &'a str, w: &str) -> Option<&'a str> {
    q.split_once(w).map(|(_, r)| r.trim()).filter(|r| !r.is_empty())
}

/// The mock model's write calls, by a few plain patterns (mockIpc.ts mirrors them). `system` lists the
/// 分类 (an entry picks one by a few words).
pub fn mock_calls(question: &str, system: &str, today: &str) -> Vec<(&'static str, Value)> {
    let q = question.trim().trim_end_matches(['。', '！', '!']);
    let date = assist::date_in(q, today);
    let with_day = |mut v: Value, k: &str| {
        if let Some(d) = &date {
            v[k] = json!(d);
        }
        v
    };
    // 一步一步
    if let (Some(parent), Some(title)) = (q.split_once("加个子任务").and_then(|(h, _)| h.strip_prefix('给')), after(q, "加个子任务")) {
        return vec![(ADD_TASK, json!({ "parent": parent.trim(), "title": title.trim_start_matches(['：', ':']) }))];
    }
    for w in ["提醒我", "新任务", "加个任务", "加一个任务", "添加任务", "加个待办", "记个待办"] {
        if let Some(rest) = after(q, w) {
            let title = without_date_words(rest);
            return if title.is_empty() { vec![] } else { vec![(ADD_TASK, with_day(json!({ "title": title }), "due_date"))] };
        }
    }
    if let Some(t) = q.strip_prefix("完成了").or_else(|| q.strip_suffix("做完了")).or_else(|| q.strip_suffix("打卡了")).or_else(|| q.strip_suffix("打卡")) {
        return vec![(EDIT_TASK, json!({ "task": without_date_words(t), "done": true }))];
    }
    if let Some((t, _)) = q.split_once("推到") {
        return vec![(EDIT_TASK, with_day(json!({ "task": t.trim_start_matches('把').trim() }), "due_date"))];
    }
    // 投石问路
    if let Some(rest) = after(q, "投了") {
        let (company, position) = rest.split_once('的').unwrap_or((rest, ""));
        return vec![(ADD_JOB, json!({ "company": company, "position": position, "status": "已投递" }))];
    }
    if let (Some(job), Some(note)) = (q.split_once("加个备注").and_then(|(h, _)| h.strip_prefix('给')), after(q, "加个备注")) {
        return vec![(EDIT_JOB, json!({ "job": job.trim_end_matches("的投递").trim(), "note": note.trim_start_matches(['：', ':']) }))];
    }
    if let Some((job, status)) = q.strip_suffix('了').and_then(|x| x.rsplit_once('进')) {
        if !job.is_empty() && !status.is_empty() && !job.contains(['把', '给']) {
            return vec![(EDIT_JOB, json!({ "job": job.trim_start_matches("这个"), "status": status }))];
        }
    }
    // 书影
    let title = q.split_once('《').and_then(|(_, r)| r.split_once('》')).map(|(t, _)| t).filter(|_| ["想读", "想看", "在读", "在看", "完了", "星"].iter().any(|w| q.contains(w)));
    if let Some(t) = title {
        let kind = if q.contains('看') { "film" } else { "book" };
        let rating = q.chars().find(|c| "12345一二三四五两".contains(*c) && q.contains(&format!("{c}星"))).map(|c| "12345一二三四五两".chars().position(|x| x == c).map_or(0, |i| [1, 2, 3, 4, 5, 1, 2, 3, 4, 5, 2][i]));
        let mut v = json!({ "kind": kind, "title": t });
        if let Some(r) = rating {
            v["rating"] = json!(r);
        }
        if q.contains("想读") || q.contains("想看") {
            v["status"] = json!("todo");
            return vec![(ADD_WORK, v)];
        }
        if q.contains("完了") {
            v["status"] = json!("done");
        }
        return vec![(EDIT_WORK, v)];
    }
    // 随意画画
    if let Some(course) = after(q, "画完了") {
        let (course, lesson) = course.split_once("的").map_or((course, None), |(c, l)| (c, Some(l)));
        let mut v = json!({ "course": course });
        if let Some(l) = lesson {
            v["lesson"] = json!(l);
        }
        return vec![(FINISH_LESSON, v)];
    }
    // 出门走走
    if let Some(name) = after(q, "新出行") {
        return vec![(ADD_TRIP, with_day(json!({ "name": without_date_words(name) }), "depart_on"))];
    }
    if let Some(rest) = after(q, "装好了") {
        return vec![(EDIT_TRIP, json!({ "pack": items_in(rest) }))];
    }
    if let Some(rest) = after(q, "加上").or_else(|| after(q, "带上")) {
        return vec![(EDIT_TRIP, json!({ "add": items_in(rest) }))];
    }
    // 念念不忘
    for w in ["记一下", "记个笔记", "记笔记"] {
        if let Some(rest) = after(q, w) {
            return vec![(ADD_NOTE, json!({ "content": rest.trim_start_matches(['：', ':']) }))];
        }
    }
    if let Some((old, new)) = after(q, "笔记里的").and_then(|r| r.split_once("改成")) {
        return vec![(EDIT_NOTE, json!({ "find": old, "old": old, "new": new }))];
    }
    // 钱来钱往
    if let Some((account, amount)) = q.split_once("余额") {
        if let Ok(n) = amount.trim_start_matches(['是', '：', ':', ' ']).trim_end_matches(['元', '块']).parse::<f64>() {
            return vec![(SET_BALANCE, json!({ "account": account.trim(), "amount": n }))];
        }
    }
    if let Some((what, to)) = q.trim_start_matches('把').split_once("改成") {
        if let Ok(n) = to.trim().trim_end_matches(['元', '块']).parse::<f64>() {
            return vec![(EDIT_ENTRY, with_day(json!({ "find": without_date_words(what), "amount": n }), "on"))];
        }
    }
    let kinds: Vec<&str> = system.split("支出分类 ").nth(1).map(|r| r.split(['；', '\n']).next().unwrap_or("").split('、').collect()).unwrap_or_default();
    let mut out = vec![];
    for part in q.split(['，', ',', '；', ';', '、', '\n']) {
        let mut rest = part.trim();
        let mut currency = None;
        for (word, code) in [("新币", "SGD"), ("港币", "HKD"), ("美元", "USD"), ("人民币", "CNY"), ("块钱", ""), ("块", ""), ("元", "")] {
            if let Some(r) = rest.strip_suffix(word) {
                rest = r.trim_end();
                if !code.is_empty() {
                    currency = Some(code);
                }
            }
        }
        let digits = rest.chars().rev().take_while(|c| c.is_ascii_digit() || *c == '.').count();
        let (head, number) = rest.split_at(rest.len() - digits); // ASCII digits: byte count = char count
        let note = without_date_words(head.trim()).trim();
        let Ok(amount) = number.parse::<f64>() else { continue };
        if note.is_empty() || note.ends_with(['？', '?']) {
            continue;
        }
        let income = ["工资", "收入", "报销", "退款", "红包", "奖学金"].iter().any(|w| note.contains(w));
        let mut args = json!({ "flow": if income { "in" } else { "out" }, "amount": amount, "note": note });
        let kind = [("餐饮", &["饭", "餐", "咖啡", "奶茶", "菜", "水果"][..]), ("交通", &["车", "地铁", "公交", "机票"][..])].iter().find(|(_, words)| words.iter().any(|w| note.contains(w))).map(|(k, _)| *k);
        if let Some(k) = kind.filter(|k| !income && kinds.contains(k)) {
            args["kind"] = json!(k);
        }
        if let Some(c) = currency {
            args["currency"] = json!(c);
        }
        if let Some(d) = assist::date_in(part, today) {
            args["day"] = json!(d);
        }
        out.push((ADD_ENTRY, args));
    }
    out
}

#[cfg(test)]
mod tests;
