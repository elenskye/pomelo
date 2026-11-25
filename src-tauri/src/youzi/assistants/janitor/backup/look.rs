//! 柚子 on 备份 (1.1.0 Phase 2; requirements「DevJanitor › 备份」): DevJanitor's Agent sees the backup through
//! the system message (`situation`: the drive, each task's comparison, the clues of hints.rs) and `BACKUP`
//! (read only: names and metadata), and proposes the cards of cards.rs.
//! - 自动看一眼: when the watcher has compared a drive just plugged in (or I picked it, or pressed 重新对比) and the clues'
//!   fingerprint isn't the one last looked at, a question (`MARK`) goes into the Agent's conversation, answered
//!   by its model without 问我. The look (its reply, `PREF`) shows on 备份 with its cards; `janitor_backup_look`
//!   asks again on demand. Not when nothing's to say, a backup is running or the conversation is busy.
//! - The mock model proposes what the clues' `可以：` lines say (`mock_calls`).

use super::super::agent::{self, Card};
use super::super::{rules, Janitor};
use super::diff::{self, Diff, Skip};
use super::hints::{tilde, Suggest};
use super::{cards, compare_all, drive_of, gb, load, Settings};
use crate::youzi::data::Answer;
use crate::youzi::store::{pref_get, pref_set};
use crate::AppState;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, State};

pub const BACKUP: &str = "pomelo_janitor_backup";
/// The automatic look's question starts with it (no 问我 for it; the mock model answers by it).
pub const MARK: &str = "【插上硬盘后自动看一眼】";
/// The last look: youzi.db preference (not undone).
pub const PREF: &str = "assistant.janitor.backup.look";
const ROWS: usize = 80;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LookState {
    pub fingerprint: String,
    /// The reply (None: it couldn't start).
    pub reply: Option<i64>,
    pub at: i64,
    pub error: Option<String>,
}

/// The look for 备份.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookView {
    pub at: i64,
    /// running | done | failed
    pub state: String,
    pub text: String,
    pub error: Option<String>,
    pub cards: Vec<Card>,
    /// The clues changed since.
    pub stale: bool,
}

pub fn tools() -> Vec<Value> {
    let s = |d: &str| json!({ "type": "string", "description": d });
    let items = |props: Value, req: Value| json!({ "type": "array", "items": { "type": "object", "properties": props, "required": req } });
    vec![
        json!({ "type": "function", "function": { "name": BACKUP, "description": "看备份（只读，只看名字、大小、修改时间，看不到文件内容）。硬盘上的路径都相对移动硬盘的根目录写。", "parameters": { "type": "object", "properties": {
            "view": { "type": "string", "enum": ["overview", "diff", "drive", "compare"], "description": "overview 移动硬盘、每个任务的对比和程序对出来的线索；diff 一个任务对比出来的一组；drive 移动硬盘上一个文件夹里有什么；compare 电脑上一个文件夹和硬盘上一个文件夹比一比（不改任何东西）" },
            "task": { "type": "integer", "description": "diff：任务的编号" },
            "group": { "type": "string", "enum": ["new", "changed", "moved", "held", "recent", "archived"], "description": "diff：new 新加的、changed 改过的、moved 改名或挪了的、held 不覆盖的、recent 电脑上刚删的、archived 只在硬盘上的" },
            "path": s("drive：硬盘上的文件夹（相对根目录，空是根目录）"),
            "mac": s("compare：电脑上的文件夹（完整路径，可以用 ~）"),
            "drive": s("compare：硬盘上的文件夹（相对根目录）")
        }, "required": ["view"] } } }),
        json!({ "type": "function", "function": { "name": cards::MOVE, "description": "提议在移动硬盘上改名或挪位置（用户勾了、点了才做；不复制、不删；挪空的旧文件夹会删掉）。整个文件夹改了名就提这个文件夹，不用一个个文件提。", "parameters": { "type": "object", "properties": {
            "title": s("这张卡片说什么，如「照片里改了名的文件夹」"),
            "items": items(json!({ "from": s("硬盘上现在的路径（相对根目录）"), "to": s("改成的路径（相对根目录，照电脑上的名字和位置）"), "why": s("为什么") }), json!(["from", "to", "why"]))
        }, "required": ["items"] } } }),
        json!({ "type": "function", "function": { "name": cards::TRASH, "description": "提议把移动硬盘上重复的文件或文件夹移到硬盘的废纸篓（用户勾了、点了才移；清空废纸篓前还能放回去）。只给别处有一模一样一份的；任务的硬盘文件夹本身不行。", "parameters": { "type": "object", "properties": {
            "title": s("这张卡片说什么，如「重复的旧照片」"),
            "items": items(json!({ "path": s("硬盘上的路径（相对根目录）"), "why": s("为什么能删：一样的那份在哪") }), json!(["path", "why"]))
        }, "required": ["items"] } } }),
        json!({ "type": "function", "function": { "name": cards::TASK, "description": "提议新建或修改一个备份任务：电脑上的一个文件夹 → 硬盘上的一个文件夹（用户点了才存）。", "parameters": { "type": "object", "properties": {
            "id": { "type": "integer", "description": "要改的任务的编号（新建不写）" },
            "source": s("电脑上的文件夹（完整路径，可以用 ~）"),
            "target": s("硬盘上的文件夹（相对根目录）"),
            "why": s("为什么")
        }, "required": ["source", "target", "why"] } } }),
    ]
}

/// The comparison now (compares first when it's missing or old; waits for one running).
pub fn fresh(j: &Janitor, s: &Settings) -> Option<super::Snapshot> {
    // a cold drive takes a few seconds to read (twice: the tasks, then the clues)
    for _ in 0..600 {
        if let Some(sn) = super::fresh_snapshot(j, s) {
            return Some(sn);
        }
        drive_of(j, s)?;
        if !j.backup.comparing.load(Ordering::SeqCst) {
            compare_all(j, s);
            return super::fresh_snapshot(j, s);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

fn counts(d: &Diff) -> String {
    let parts: Vec<String> = [("一样", d.same as usize), ("新加", d.new.len()), ("改过", d.changed.len()), ("改名或挪了", d.moved.len()), ("不覆盖", d.held.len()), ("电脑上刚删的", d.recent.len()), ("只在硬盘上", d.archived.len())].iter().filter(|(_, n)| *n > 0).map(|(k, n)| format!("{k} {n}")).collect();
    if parts.is_empty() { "两边都是空的".into() } else { parts.join(" · ") }
}

/// The backup part of the Agent's 现在的情况.
pub fn situation(j: &Janitor, s: &Settings) -> String {
    let Some(d) = &s.drive else { return "还没选移动硬盘（用户在「备份」页选）。\n".into() };
    let Some(v) = drive_of(j, s) else { return format!("移动硬盘「{}」没插上。{}\n", d.name, if s.tasks.is_empty() { "还没有任务。".to_string() } else { format!("有 {} 个任务。", s.tasks.len()) }) };
    let mut out = format!("移动硬盘「{}」插着（{}，共 {}，还剩 {}）。硬盘上的路径都相对它的根目录写；「旧版」是放被覆盖的旧版本的地方，由用户自己清。\n", v.name, v.fs, gb(v.total), gb(v.free));
    let snap = super::fresh_snapshot(j, s);
    if s.tasks.is_empty() {
        out.push_str("还没有任务。\n");
    } else {
        out.push_str("任务（电脑上的文件夹 → 硬盘上的文件夹）：\n");
        for t in &s.tasks {
            let state = match snap.as_ref().and_then(|sn| sn.diffs.get(&t.id)) {
                Some(Ok(d)) => counts(d),
                Some(Err(e)) => format!("没对成：{e}"),
                None => "正在对比".into(),
            };
            let ex = if t.excludes.is_empty() { String::new() } else { format!("（另外不备份：{}）", t.excludes.join("、")) };
            out.push_str(&format!("- #{} {} → {}{ex}：{state}\n", t.id, tilde(&j.home, &t.source), t.target));
        }
    }
    match snap.as_ref().and_then(|sn| sn.hints.as_ref()) {
        Some(h) => {
            let text = h.text(&j.home, gb);
            if !text.is_empty() {
                out.push_str(&format!("程序按名字、大小、修改时间对出来的线索（没看文件内容；「{}」后面是可以提议的做法，要不要提你来判断）：\n{text}", super::hints::CAN));
            }
        }
        None if snap.is_none() => out.push_str("正在对比，线索等对完才有。\n"),
        None => {}
    }
    out
}

fn answer(label: impl Into<String>, ok: bool, text: impl Into<String>) -> Answer {
    Answer { module: None, label: label.into(), state: if ok { "ok" } else { "failed" }, text: text.into(), records: vec![] }
}

fn rows_text(rows: &[(String, u64, i64)], total: usize) -> String {
    let mut out: String = rows.iter().take(ROWS).map(|(p, size, t)| format!("- {p} · {} · 改于 {}\n", gb(*size), super::super::scan::day_of(*t))).collect();
    if total > ROWS.min(rows.len()) {
        out.push_str(&format!("（还有 {} 个没列）\n", total - ROWS.min(rows.len())));
    }
    out
}

/// Runs `BACKUP` (read only).
pub fn tool(app: &AppHandle, args: &str, budget: i64) -> Answer {
    let j = app.state::<Janitor>();
    let s = load(&app.state::<AppState>());
    tool_with(&j, &s, args, budget)
}

pub fn tool_with(j: &Janitor, s: &Settings, args: &str, budget: i64) -> Answer {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    if rules::mentions_sealed(args) {
        return answer("备份 · 没看成", false, "没看成：这个地方不能看");
    }
    let view = v["view"].as_str().unwrap_or("overview");
    if view == "overview" {
        return answer("备份 · 总的情况", true, agent::fit(situation(j, s), budget));
    }
    let Some(dv) = drive_of(j, s) else { return answer("备份 · 没看成", false, "没看成：移动硬盘没插上") };
    let r: Result<(String, String), String> = (|| match view {
        "diff" => {
            let id = v["task"].as_i64().ok_or("要写 task（任务的编号）")?;
            let t = s.tasks.iter().find(|t| t.id == id).ok_or(format!("没有任务 #{id}"))?;
            let sn = fresh(j, s).ok_or("还没对比完，等一下再看")?;
            let d = sn.diffs.get(&id).ok_or("还没对比完")?.as_ref().map_err(|e| e.clone())?;
            let group = v["group"].as_str().unwrap_or("new");
            let row = |r: &diff::Row| (r.path.clone(), r.size, r.mtime);
            let (name, body) = match group {
                "changed" => ("改过的（电脑上的更新，硬盘上那份会挪进旧版）", rows_text(&d.changed.iter().map(row).collect::<Vec<_>>(), d.changed.len())),
                "moved" => ("改名或挪了的（硬盘上的 → 电脑上的）", d.moved.iter().take(ROWS).map(|m| format!("- {} → {} · {}\n", m.from, m.to, gb(m.size))).collect::<String>() + &if d.moved.len() > ROWS { format!("（还有 {} 个没列）\n", d.moved.len() - ROWS) } else { String::new() }),
                "held" => ("不覆盖的", d.held.iter().take(ROWS).map(|h| format!("- {} · {} · {}\n", h.path, gb(h.size), h.why)).collect()),
                "recent" => ("电脑上刚删的（上次备份时电脑上还有）", rows_text(&d.recent.iter().map(row).collect::<Vec<_>>(), d.recent.len())),
                "archived" => ("只在硬盘上的", rows_text(&d.archived.iter().map(row).collect::<Vec<_>>(), d.archived.len())),
                _ => ("新加的", rows_text(&d.new.iter().map(row).collect::<Vec<_>>(), d.new.len())),
            };
            Ok((format!("备份 · 任务 #{id} 的{}", name.split('（').next().unwrap_or(name)), format!("任务 #{id} {} → {}：{}\n{name}：\n{body}", tilde(&j.home, &t.source), t.target, counts(d))))
        }
        "drive" => {
            let rel = v["path"].as_str().unwrap_or("").trim().trim_matches('/').to_string();
            if rel.split('/').any(|p| p == ".." || p.starts_with('.')) {
                return Err("路径不对".into());
            }
            let real = if rel.is_empty() { dv.path.clone() } else { dv.path.join(&rel) };
            let l = diff::list(&real, &Skip::default(), &AtomicBool::new(false))?;
            let mut dirs: std::collections::BTreeMap<String, (u64, u64, i64)> = std::collections::BTreeMap::new();
            let mut files = Vec::new();
            for (k, (_, m)) in &l.files {
                match k.split_once('/') {
                    Some((top, _)) => {
                        let e = dirs.entry(top.to_string()).or_default();
                        (e.0, e.1, e.2) = (e.0 + 1, e.1 + m.size, e.2.max(m.mtime));
                    }
                    None => files.push((k.clone(), m.size, m.mtime)),
                }
            }
            for k in l.dirs.keys().filter(|k| !k.contains('/')) {
                dirs.entry(k.clone()).or_default();
            }
            let shown = if rel.is_empty() { "根目录".to_string() } else { rel.clone() };
            let mut out = format!("硬盘上的 {shown}（{} 个文件夹、{} 个文件）\n", dirs.len(), files.len());
            let mut dl: Vec<_> = dirs.into_iter().collect();
            dl.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then(a.0.cmp(&b.0)));
            for (name, (n, b, t)) in dl.iter().take(ROWS) {
                let note = if rel.is_empty() && name == super::copy::OLD { " · 放被覆盖的旧版本（用户自己清，不提议）" } else { "" };
                out.push_str(&format!("- {name}/ · {n} 个文件 · {} · 最近改于 {}{note}\n", gb(*b), if *t > 0 { super::super::scan::day_of(*t) } else { "—".into() }));
            }
            files.sort_by(|a, b| b.1.cmp(&a.1));
            out.push_str(&rows_text(&files, files.len()));
            Ok((format!("备份 · 看硬盘上的 {shown}"), out))
        }
        "compare" => {
            let mac = agent::virt_of(&j.home, v["mac"].as_str().unwrap_or(""))?;
            if rules::sealed(&mac) || !rules::within(&mac, &j.home) {
                return Err("电脑上的文件夹要在个人文件夹里".into());
            }
            let rel = v["drive"].as_str().unwrap_or("").trim().trim_matches('/').to_string();
            if rel.is_empty() || rel.split('/').any(|p| p == ".." || p.starts_with('.')) {
                return Err("硬盘上的文件夹不对".into());
            }
            let skip = Skip { extra: s.tasks.iter().find(|t| t.source == mac).map(|t| t.excludes.clone()).unwrap_or_default() };
            let stop = AtomicBool::new(false);
            let (m, d) = (diff::list(&j.real_of(&mac), &skip, &stop)?, diff::list(&dv.path.join(&rel), &Skip::default(), &stop)?);
            if m.files.is_empty() && m.dirs.is_empty() {
                return Err(format!("电脑上的 {} 是空的或者不在", tilde(&j.home, &mac)));
            }
            let x = diff::compare(&m, &d, &Default::default());
            let pick = |rows: Vec<String>| rows.into_iter().take(15).map(|r| format!("  - {r}\n")).collect::<String>();
            let mut out = format!("电脑上的 {}（{} 个文件）对硬盘上的 {rel}（{} 个文件）：{}\n", tilde(&j.home, &mac), m.files.len(), d.files.len(), counts(&x));
            for (name, rows) in [("新加的", x.new.iter().map(|r| r.path.clone()).collect::<Vec<_>>()), ("改名或挪了的", x.moved.iter().map(|m| format!("{} → {}", m.from, m.to)).collect()), ("只在硬盘上的", x.archived.iter().map(|r| r.path.clone()).collect())] {
                if !rows.is_empty() {
                    out.push_str(&format!("{name}：\n{}", pick(rows)));
                }
            }
            Ok((format!("备份 · 比一比 {}", tilde(&j.home, &mac)), out))
        }
        _ => Err("没有这个 view".into()),
    })();
    match r {
        Ok((label, text)) => answer(label, true, agent::fit(text, budget)),
        Err(e) => answer("备份 · 没看成", false, format!("没看成：{e}")),
    }
}

// ---- 自动看一眼 ----

pub fn state(c: &Connection) -> LookState {
    pref_get::<LookState>(c, PREF).ok().flatten().unwrap_or_default()
}

/// What a look would be about now (empty: nothing to say).
fn fingerprint(j: &Janitor, sn: &super::Snapshot) -> String {
    let mut held: Vec<(i64, usize)> = sn.diffs.iter().filter_map(|(id, d)| d.as_ref().ok().map(|d| (*id, d.held.len()))).collect();
    held.sort();
    sn.hints.as_ref().map(|h| h.fingerprint(&j.home, &held)).unwrap_or_default()
}

pub fn question(drive: &str) -> String {
    format!("{MARK}移动硬盘「{drive}」插上了，两边对完了一遍。照「现在的情况」里备份的部分看一眼差异：该在硬盘上改名或挪的、该把重复的移到硬盘的废纸篓的、该新建或改任务的，各提议卡片；只在硬盘上、像是归档的说一句留着就行，不用提议；拿不准的写出来让用户定。回答简短。")
}

/// Asks the Agent to look (when the clues changed since the last look, or `force`). Ok(false): nothing asked.
pub async fn maybe(app: &AppHandle, force: bool) -> Result<bool, String> {
    let st = app.state::<AppState>();
    let j = app.state::<Janitor>().inner().clone();
    let s = load(&st);
    let Some(d) = s.drive.clone() else { return Ok(false) };
    let sn = {
        let (j, s) = (j.clone(), s.clone());
        tauri::async_runtime::spawn_blocking(move || fresh(&j, &s)).await.map_err(|e| e.to_string())?
    };
    let Some(sn) = sn else { return Ok(false) };
    let fp = fingerprint(&j, &sn);
    let last = st.youzi.with_db(|c| Ok(state(c)))?;
    if !force && (fp.is_empty() || fp == last.fingerprint) {
        return Ok(false);
    }
    if j.backup.running() {
        return Err("正在备份，等它做完".into());
    }
    let level = st.youzi.with_db(|c| Ok(super::super::settings(c).agent.level()))?;
    let session = st.youzi.with_db(|c| agent::open_chat(c, &level))?;
    if st.youzi.runs.busy_in(session) {
        return Err("Agent 正在回答，等它说完".into());
    }
    let at = crate::modules::common::unix_now();
    let route = match agent::route(app, session).await {
        Ok(r) => r,
        Err(e) => {
            st.youzi.with_db(|c| pref_set(c, PREF, &LookState { fingerprint: fp, reply: None, at, error: Some(e.clone()) }))?;
            return Err(e);
        }
    };
    let b = st.youzi.with_db(|c| {
        let b = crate::youzi::chat::begin_run_in(c, Some(session), &Default::default(), &question(&d.name), &route.model, &route.name, &Default::default())?;
        pref_set(c, PREF, &LookState { fingerprint: fp, reply: Some(b.reply_id), at, error: None })?;
        Ok(b)
    })?;
    crate::youzi::chat::start(app, b, route, tauri::ipc::Channel::new(|_| Ok(())));
    Ok(true)
}

/// From a thread (the watcher): looks in the background; a failure only shows on 备份.
pub fn maybe_later(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = maybe(&app, false).await;
    });
}

/// What the clues are about now (None: no fresh comparison). Off any database lock (it asks the drives).
pub fn fingerprint_now(j: &Janitor, s: &Settings) -> Option<String> {
    super::fresh_snapshot(j, s).map(|sn| fingerprint(j, &sn))
}

/// The last look for 备份 (None: never, or its reply is gone with an old conversation). `now`: `fingerprint_now`.
pub fn view(c: &Connection, now: Option<&str>) -> Result<Option<LookView>, String> {
    let l = state(c);
    if l.at == 0 {
        return Ok(None);
    }
    let stale = now.is_some_and(|f| f != l.fingerprint);
    let Some(reply) = l.reply else { return Ok(Some(LookView { at: l.at, state: "failed".into(), text: String::new(), error: l.error, cards: vec![], stale })) };
    let row: Option<(String, String, Option<String>)> = c.query_row("SELECT m.content, m.state, r.error FROM messages m LEFT JOIN runs r ON r.id = m.run_id WHERE m.id = ?1", [reply], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().map_err(crate::modules::common::err)?;
    let Some((text, st, error)) = row else { return Ok(None) };
    let state = match st.as_str() {
        "streaming" => "running",
        "complete" => "done",
        _ => "failed",
    };
    Ok(Some(LookView { at: l.at, state: state.into(), text, error: error.or_else(|| (state == "failed" && st == "cancelled").then(|| "停下了".into())), cards: agent::of_message(c, reply)?, stale }))
}

/// 柚子看一眼: asks the Agent now (even when the clues are the ones it saw).
#[tauri::command]
pub async fn janitor_backup_look(app: AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>) -> Result<super::Overview, String> {
    maybe(&app, true).await?;
    super::overview_now(&state, &janitor)
}

// ---- the mock model ----

/// The mock Agent on 备份: a look, then the clues' suggestions as cards (`system` holds them).
pub fn mock_calls(system: &str) -> Vec<crate::youzi::provider::ToolCall> {
    use crate::youzi::provider::ToolCall;
    let call = |id: String, name: &str, args: Value| ToolCall { id, name: name.into(), arguments: args.to_string() };
    let mut out = vec![call("b0".into(), BACKUP, json!({ "view": "overview" }))];
    let (mut moves, mut trash) = (Vec::new(), Vec::new());
    for (i, s) in Suggest::all_in(system).into_iter().enumerate() {
        match s {
            Suggest::Task { id, source, target } => out.push(call(format!("bt{i}"), cards::TASK, json!({ "id": id, "source": source, "target": target, "why": "（模拟）程序对出来的线索" }))),
            Suggest::Rename { from, to } => moves.push(json!({ "from": from, "to": to, "why": "（模拟）电脑上改了名" })),
            Suggest::Trash { path } => trash.push(json!({ "path": path, "why": "（模拟）别处有一模一样的一份" })),
        }
    }
    if !moves.is_empty() {
        out.push(call("bm".into(), cards::MOVE, json!({ "title": "改了名的文件夹", "items": moves })));
    }
    if !trash.is_empty() {
        out.push(call("bx".into(), cards::TRASH, json!({ "title": "硬盘里重复的", "items": trash })));
    }
    out
}
