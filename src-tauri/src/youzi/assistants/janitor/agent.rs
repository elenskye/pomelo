//! DevJanitor's Agent (v0.8 阶段 8; requirements「DevJanitor · Agent 页」): the detailed layer. A 柚子
//! conversation of its own (private, left out of 柚子's list, search, ⌘K and the 模块坞: preference `CHAT`,
//! library.rs `unlisted!`), with DevJanitor's model (`Settings::agent`), its SKILL and the scan's situation
//! in the system message (`system`), and its own tools instead of Pomelo's data:
//! - `LOOK` (read only): the overview, a folder's contents (names, sizes, times; sizes from janitor.db or a
//!   bounded walk), `find` under a folder (name, extension, size, untouched days), the 开发环境. Never a
//!   file's contents.
//! - Proposals, each a card in the reply (`janitor_cards`): `TRASH` (paths to the Trash), `COMMAND` (one
//!   shell command: what it does, what it deletes, whether it comes back, about how much), `RULE` (a folder
//!   kind for 一键清理). Rust checks them first: paths exist, a protected place is marked (the page asks
//!   twice), a command with sudo is only shown (no 执行), a rule only under home, outside personal folders
//!   and protected places.
//! - `RUN`: a read-only command (readonly.rs's list: du, ls, brew list, conda env list…) run right away, its output
//!   back to the model (shown as a 「查了」 line); anything else is a `COMMAND` card. The fake Mac doesn't run it.
//! - Plus 联网搜索 on its own judgment (on and with its Key) and 问我 (cloud models).
//! - The SKILL is mine to rewrite (preference `SKILL_PREF`, None = `SKILL`); the situation is written each time,
//!   the 一键清理 list only on request (`LOOK` cleanup). Cards I ran go back with their reply (`in_history`).
//! - Running a card: the Trash (sandbox: the fake Mac's), a command in my login shell (`zsh -lic`, from
//!   home; sandbox: not run, a made-up result), a rule into `janitor_rules` (one undo step, then a rescan).
//!   None of the first two is undone. A command runs in the background (output kept in memory while it
//!   runs, the last part saved with the card) and can be stopped (its process group).
//! - 柚子 reads DevJanitor through `INFO` (read only), where it may (hidden, 发给云端).

use super::rules::{self, Protected};
use super::{store, Janitor, Mode};
use crate::modules::common::{err, Tx};
use crate::youzi::data::Answer;
use crate::youzi::store::{pref_get, pref_set};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

/// The conversation's preference (must match library.rs `unlisted!`).
pub const CHAT: &str = "janitor.chat";
/// The system message says this (the mock model answers by it).
pub const MARK: &str = "你是 DevJanitor";
pub const LOOK: &str = "pomelo_janitor_look";
pub const TRASH: &str = "pomelo_janitor_trash";
pub const COMMAND: &str = "pomelo_janitor_command";
pub const RULE: &str = "pomelo_janitor_rule";
/// A read-only command, run right away (readonly.rs).
pub const RUN: &str = "pomelo_janitor_run";
/// My own SKILL (None: `SKILL`).
pub const SKILL_PREF: &str = "janitor.skill";
/// 柚子's read-only look at DevJanitor.
pub const INFO: &str = "pomelo_janitor_info";
/// Lookup rounds a reply may take (a folder at a time needs more than 柚子's).
pub const ROUNDS: usize = 12;
/// 发给云端's key.
pub const MODULE: &str = "janitor";

pub fn is_proposal(name: &str) -> bool {
    [TRASH, COMMAND, RULE].contains(&name)
}

fn answer(label: impl Into<String>, ok: bool, text: impl Into<String>) -> Answer {
    Answer { module: None, label: label.into(), state: if ok { "ok" } else { "failed" }, text: text.into(), records: vec![] }
}

/// Text cut to what `budget` tokens hold (about two characters a token, said where it was cut).
fn fit(text: String, budget: i64) -> String {
    let max = (budget.max(200) as usize) * 2;
    if text.chars().count() <= max {
        return text;
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}\n（太长了，后面的没给）")
}

pub fn gb(bytes: u64) -> String {
    if bytes >= rules::GB {
        format!("{:.1} GB", bytes as f64 / rules::GB as f64)
    } else if bytes >= rules::MB {
        format!("{} MB", bytes / rules::MB)
    } else {
        format!("{} KB", (bytes / 1000).max(1))
    }
}

fn day(t: i64) -> String {
    if t <= 0 {
        "—".into()
    } else {
        super::scan::day_of(t)
    }
}

// ---- the conversation ----

/// The conversation, if it's still there.
pub fn chat(c: &Connection) -> Result<Option<i64>, String> {
    let Some(id) = pref_get::<i64>(c, CHAT).ok().flatten() else { return Ok(None) };
    let there: bool = c.query_row("SELECT EXISTS (SELECT 1 FROM sessions WHERE id = ?1)", [id], |r| r.get(0)).map_err(err)?;
    Ok(there.then_some(id))
}

pub fn is_chat(c: &Connection, session: i64) -> Result<bool, String> {
    Ok(chat(c)? == Some(session))
}

/// The conversation (made when missing): private, thinking as DevJanitor's model is set.
pub fn open_chat(c: &Connection, level: &crate::youzi::cloud::Level) -> Result<i64, String> {
    if let Some(id) = chat(c)? {
        return Ok(id);
    }
    c.execute("INSERT INTO sessions (title, title_source, private, thinking, effort) VALUES ('DevJanitor', 'user', 1, ?1, ?2)", params![level.thinking(), level.effort()]).map_err(err)?;
    let id = c.last_insert_rowid();
    pref_set(c, CHAT, &id)?;
    Ok(id)
}

/// 新对话: the old one goes (not undone: it never was in 柚子's list), a new one starts.
/// New row first: deleting the highest rowid first would hand the same id back, and the page, keyed on it, wouldn't switch.
pub fn new_chat(c: &Connection, level: &crate::youzi::cloud::Level) -> Result<i64, String> {
    let old = chat(c)?;
    pref_set(c, CHAT, &Value::Null)?;
    let id = open_chat(c, level)?;
    if let Some(old) = old {
        c.execute("DELETE FROM sessions WHERE id = ?1", [old]).map_err(err)?;
    }
    Ok(id)
}

/// Where a reply goes: DevJanitor's model (None: 柚子's selection); the session's 思考 follows it.
pub async fn route(app: &AppHandle, session: i64) -> Result<crate::youzi::Route, String> {
    let st = app.state::<AppState>();
    let s = st.youzi.with_db(|c| Ok(super::settings(c)))?;
    let level = s.agent.level();
    st.youzi.with_db(|c| c.execute("UPDATE sessions SET thinking = ?1, effort = ?3 WHERE id = ?2", params![level.thinking(), session, level.effort()]).map(|_| ()).map_err(err))?;
    Ok(crate::youzi::schedule::route(&st.youzi, s.agent.model.as_deref()).await?.0)
}

// ---- rules (Agent-proposed, added to 一键清理) ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rule {
    pub id: i64,
    pub name: String,
    /// A virtual path; `*` matches within one folder name.
    pub pattern: String,
    pub recognize: String,
    pub rebuild: String,
    pub checked: bool,
    pub created_at: String,
}

pub fn rules(c: &Connection) -> Result<Vec<Rule>, String> {
    let mut s = c.prepare("SELECT id, name, pattern, recognize, rebuild, checked, created_at FROM janitor_rules ORDER BY id").map_err(err)?;
    let rows = s
        .query_map([], |r| Ok(Rule { id: r.get(0)?, name: r.get(1)?, pattern: r.get(2)?, recognize: r.get(3)?, rebuild: r.get(4)?, checked: r.get(5)?, created_at: r.get(6)? }))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}

pub fn rename_rule(c: &Connection, id: i64, name: &str) -> Result<(), String> {
    let name: String = name.trim().chars().take(30).collect();
    if name.is_empty() {
        return Err("名字不能是空的".into());
    }
    let n = c.execute("UPDATE janitor_rules SET name = ?1 WHERE id = ?2 AND name <> ?1", params![name, id]).map_err(err)?;
    if n == 0 && c.query_row("SELECT count(*) FROM janitor_rules WHERE id = ?1", [id], |r| r.get::<_, i64>(0)).map_err(err)? == 0 {
        return Err("这条规则已经不在了".into());
    }
    Ok(())
}

pub fn delete_rule(c: &Connection, id: i64) -> Result<(), String> {
    let n = c.execute("DELETE FROM janitor_rules WHERE id = ?1", [id]).map_err(err)?;
    if n == 0 {
        return Err("这条规则已经不在了".into());
    }
    Ok(())
}

/// One folder name against a pattern with `*`.
pub fn glob(pat: &str, s: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == s;
    }
    let mut rest = s;
    for (i, p) in parts.iter().enumerate() {
        if i == 0 {
            let Some(r) = rest.strip_prefix(p) else { return false };
            rest = r;
        } else if i == parts.len() - 1 {
            return rest.ends_with(p);
        } else {
            match rest.find(p) {
                Some(k) => rest = &rest[k + p.len()..],
                None => return false,
            }
        }
    }
    true
}

/// A virtual path against a rule's pattern (same depth, each name matching).
pub fn matches(pattern: &str, virt: &str) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), virt.split('/').collect());
    a.len() == b.len() && a.iter().zip(&b).all(|(p, s)| glob(p, s))
}

/// Folders under home that hold my own things: a rule never goes in them.
const PERSONAL: [&str; 12] = ["Documents", "Desktop", "Downloads", "Pictures", "Movies", "Music", "Public", "Applications", "Library/Mobile Documents", "Library/CloudStorage", "Library/Mail", "Library/Messages"];

/// A rule's pattern, checked: under home, two names deep at least, outside personal folders and protected places.
pub fn check_pattern(home: &str, protected: &Protected, pattern: &str) -> Result<String, String> {
    let p = virt_of(home, pattern)?;
    let Some(rel) = p.strip_prefix(&format!("{home}/")) else { return Err("一键清理的规则只能在个人文件夹里（工具自己的地方）".into()) };
    let segs: Vec<&str> = rel.split('/').collect();
    if segs.len() < 2 || segs.iter().any(|s| s.is_empty() || *s == "*" && segs.len() < 3) {
        return Err("规则太宽了：至少写到个人文件夹下面两层（比如 ~/.gradle/caches）".into());
    }
    if segs.iter().any(|s| *s == ".." || *s == ".") {
        return Err("路径里不能有 . 或 ..".into());
    }
    if let Some(dir) = PERSONAL.iter().find(|d| rules::within(rel, d)) {
        return Err(format!("{dir} 里是你自己的东西，不能放进一键清理"));
    }
    let fixed: String = p.split('*').next().unwrap_or(&p).trim_end_matches('/').to_string();
    if let Some(why) = protected.of(&fixed).or_else(|| protected.of(&p)) {
        return Err(format!("{why}是受保护的地方，不能放进一键清理"));
    }
    Ok(p)
}

/// "~/x", "$HOME/x", "/x" → a virtual path (no trailing slash).
pub fn virt_of(home: &str, p: &str) -> Result<String, String> {
    let p = p.trim().trim_matches(['"', '\'']);
    let v = if p == "~" || p == "$HOME" || p == "${HOME}" {
        home.to_string()
    } else if let Some(r) = p.strip_prefix("~/").or_else(|| p.strip_prefix("$HOME/")).or_else(|| p.strip_prefix("${HOME}/")) {
        format!("{home}/{r}")
    } else if p.starts_with('/') {
        p.to_string()
    } else {
        return Err(format!("「{p}」不是完整的路径（要从 / 或 ~ 开始）"));
    };
    let v = v.trim_end_matches('/').to_string();
    if v.is_empty() {
        return Err("不能是整块磁盘".into());
    }
    Ok(v)
}

// ---- sizes and listings, live ----

/// A folder's size by walking it, at most `left` entries (false: it stopped early).
fn du(real: &Path, allocated: bool, left: &mut usize) -> (u64, bool) {
    let mut total = 0u64;
    let mut stack = vec![real.to_path_buf()];
    let dev = std::fs::symlink_metadata(real).map(|m| m.dev()).unwrap_or(0);
    let mut seen = std::collections::HashSet::new();
    while let Some(d) = stack.pop() {
        let Ok(r) = std::fs::read_dir(&d) else { continue };
        for e in r.flatten() {
            if *left == 0 {
                return (total, false);
            }
            *left -= 1;
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                if m.dev() == dev {
                    stack.push(e.path());
                }
            } else if m.nlink() <= 1 || seen.insert((m.dev(), m.ino())) {
                total += if allocated { m.blocks() * 512 } else { m.len() };
            }
        }
    }
    (total, true)
}

fn size_of(j: &Janitor, virt: &str, left: &mut usize) -> (u64, bool) {
    if let Ok(Some(n)) = j.with_db(|c| store::node_at(c, virt)) {
        return (n.size, true);
    }
    let real = j.real_of(virt);
    match std::fs::symlink_metadata(&real) {
        Ok(m) if m.is_dir() => du(&real, j.mode == Mode::Real, left),
        Ok(m) => (if j.mode == Mode::Real { m.blocks() * 512 } else { m.len() }, true),
        Err(_) => (0, true),
    }
}

fn mtime(real: &Path) -> i64 {
    std::fs::symlink_metadata(real).map(|m| m.mtime()).unwrap_or(0)
}

fn folder(j: &Janitor, protected: &Protected, virt: &str) -> Result<String, String> {
    let real = j.real_of(virt);
    let rd = std::fs::read_dir(&real).map_err(|e| if e.kind() == std::io::ErrorKind::PermissionDenied { "没权限读这个文件夹（需要完全磁盘访问权限）".to_string() } else { format!("打不开：{e}") })?;
    let mut left = 300_000usize;
    let mut rows: Vec<(String, u64, bool, i64, &'static str)> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let child = if virt == "/" { format!("/{name}") } else { format!("{virt}/{name}") };
        let kind = match e.file_type() {
            Ok(t) if t.is_symlink() => "替身",
            Ok(t) if t.is_dir() => "文件夹",
            _ => "文件",
        };
        let (size, whole) = if kind == "替身" { (0, true) } else { size_of(j, &child, &mut left) };
        rows.push((name, size, whole, mtime(&e.path()), kind));
    }
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let total: u64 = rows.iter().map(|r| r.1).sum();
    let mut out = format!("{virt}（{} 项，共约 {}{}）\n", rows.len(), gb(total), protected.of(virt).map(|p| format!("，受保护：{p}")).unwrap_or_default());
    for (name, size, whole, t, kind) in rows.iter().take(80) {
        out.push_str(&format!("- {name}{} · {kind} · {}{} · 改于 {}\n", if *kind == "文件夹" { "/" } else { "" }, if *whole { "" } else { "至少 " }, gb(*size), day(*t)));
    }
    if rows.len() > 80 {
        out.push_str(&format!("（还有 {} 项更小的没列）\n", rows.len() - 80));
    }
    Ok(out)
}

fn find(j: &Janitor, protected: &Protected, virt: &str, args: &Value) -> Result<String, String> {
    let name = args["name"].as_str().map(str::to_lowercase).filter(|s| !s.is_empty());
    let ext = args["ext"].as_str().map(|s| s.trim_start_matches('.').to_lowercase()).filter(|s| !s.is_empty());
    let min = (args["minMb"].as_f64().unwrap_or(0.0).max(0.0) * rules::MB as f64) as u64;
    let days = args["olderDays"].as_i64().unwrap_or(0);
    let before = super::now() - days * 86400;
    let real = j.real_of(virt);
    let allocated = j.mode == Mode::Real;
    let mut left = 500_000usize;
    let mut hits: Vec<(String, u64, i64)> = Vec::new();
    let mut stack = vec![(real.clone(), virt.to_string())];
    let mut complete = true;
    while let Some((d, v)) = stack.pop() {
        let Ok(r) = std::fs::read_dir(&d) else { continue };
        for e in r.flatten() {
            if left == 0 {
                complete = false;
                break;
            }
            left -= 1;
            let Ok(m) = e.metadata() else { continue };
            let n = e.file_name().to_string_lossy().into_owned();
            let cv = format!("{v}/{n}");
            if m.is_dir() {
                stack.push((e.path(), cv));
                continue;
            }
            let size = if allocated { m.blocks() * 512 } else { m.len() };
            let low = n.to_lowercase();
            if size < min || m.mtime() > before && days > 0 || name.as_ref().is_some_and(|w| !low.contains(w.as_str())) || ext.as_ref().is_some_and(|x| !low.ends_with(&format!(".{x}"))) {
                continue;
            }
            hits.push((cv, size, m.mtime()));
        }
    }
    hits.sort_by(|a, b| b.1.cmp(&a.1));
    let total: u64 = hits.iter().map(|h| h.1).sum();
    let mut out = format!("在 {virt} 里找到 {} 个文件，共 {}{}\n", hits.len(), gb(total), if complete { "" } else { "（文件太多，只找了一部分）" });
    for (p, size, t) in hits.iter().take(60) {
        out.push_str(&format!("- {p} · {} · 改于 {}{}\n", gb(*size), day(*t), protected.of(p).map(|w| format!(" · 受保护：{w}")).unwrap_or_default()));
    }
    Ok(out)
}

// ---- the situation (system message, overview, 柚子's look) ----

/// The scan's situation. `list`: the 一键清理 categories and the listed groups line by line (柚子's INFO);
/// the Agent gets their totals and looks at the list itself when it needs it (`LOOK` cleanup).
fn situation(j: &Janitor, s: &super::Settings, rules: &[Rule], list: bool) -> String {
    let ov = super::overview(j, s, rules).ok();
    let Some(ov) = ov.filter(|o| o.summary.is_some()) else { return "还没扫描过：先跟用户说去「磁盘」页等扫描做完（或点重新扫描）。".into() };
    let sum = ov.summary.as_ref().expect("checked");
    let mut out = String::new();
    out.push_str(&format!("扫描于 {}。", day(sum["scannedAt"].as_i64().unwrap_or(0))));
    if let (Some(t), Some(f)) = (sum["disk"]["total"].as_u64(), sum["disk"]["free"].as_u64()) {
        out.push_str(&format!("磁盘 {}，还剩 {}；系统和其他 {}。", gb(t), gb(f), gb(sum["disk"]["system"].as_u64().unwrap_or(0))));
    }
    if sum["fullDiskAccess"] == false {
        out.push_str(&format!("没有完全磁盘访问权限，{} 处没扫。", sum["denied"].as_u64().unwrap_or(0)));
    }
    if list {
        out.push_str(&format!("\n一键清理里能清 {}（用户在「清理」页点）：\n", gb(ov.cleanable)));
        for g in &ov.categories {
            out.push_str(&format!("- {} {}（{} 项）\n", g.name, gb(g.size), g.items.len()));
        }
        if !ov.groups.is_empty() {
            out.push_str("只列出来、等用户挑的：\n");
            for g in &ov.groups {
                out.push_str(&format!("- {} {}（{} 项）\n", g.name, gb(g.size), g.items.len()));
            }
        }
    } else {
        let items: usize = ov.categories.iter().map(|g| g.items.len()).sum();
        let listed: u64 = ov.groups.iter().map(|g| g.size).sum();
        out.push_str(&format!("\n清理页有一份清单：一键清理 {} 类 {items} 项，能清 {}（用户自己点）；只列出来、等用户挑的 {} 组，共 {}。要看清单里有什么，用 pomelo_janitor_look 的 cleanup。\n", ov.categories.len(), gb(ov.cleanable), ov.groups.len(), gb(listed)));
    }
    if let Ok(Some(view)) = super::ring_view(j, &j.home.clone()) {
        let top: Vec<String> = view.node.children.iter().take(10).map(|k| format!("{} {}", k.name, gb(k.size))).collect();
        out.push_str(&format!("个人文件夹（{}）里最大的：{}\n", j.home, top.join("、")));
    }
    let envs = j.envs();
    if !envs.is_empty() {
        out.push_str(&format!("开发环境：{}\n", envs.iter().map(|e| format!("{} {}", e.name, gb(e.size))).collect::<Vec<_>>().join("、")));
    }
    if let Some(d) = sum["docker"].as_object() {
        out.push_str(&format!("Docker 一共 {}。\n", gb(d.get("size").and_then(Value::as_u64).unwrap_or(0))));
    }
    out
}

/// The 清理 page's list (`LOOK` cleanup): each 一键清理 category and listed group with its items.
fn cleanup_text(j: &Janitor, s: &super::Settings, rules: &[Rule]) -> String {
    let Ok(ov) = super::overview(j, s, rules) else { return "清单读不出来（等下一次扫描）。".into() };
    if ov.summary.is_none() {
        return "还没扫描过：先跟用户说去「磁盘」页等扫描做完（或点重新扫描）。".into();
    }
    let mut out = format!("## 一键清理（能清 {}，用户在「清理」页勾、点）\n", gb(ov.cleanable));
    let row = |i: &super::Item| {
        let mut bits = vec![gb(i.size), i.paths.first().cloned().unwrap_or_default()];
        if i.checked {
            bits.push("默认勾上".into());
        }
        for t in [&i.in_use, &i.protected, &i.note, &i.tag].into_iter().flatten() {
            bits.push(t.clone());
        }
        format!("- {} · {}\n", i.title, bits.into_iter().filter(|b| !b.is_empty()).collect::<Vec<_>>().join(" · "))
    };
    for g in &ov.categories {
        out.push_str(&format!("### {} · {}（{} 项）\n", g.name, gb(g.size), g.items.len()));
        if let Some(r) = &g.rebuild {
            out.push_str(&format!("怎么重建：{r}\n"));
        }
        for i in g.items.iter().take(30) {
            out.push_str(&row(i));
        }
        if g.items.len() > 30 {
            out.push_str(&format!("- …还有 {} 项\n", g.items.len() - 30));
        }
    }
    if ov.categories.is_empty() {
        out.push_str("（没有）\n");
    }
    out.push_str("\n## 只列出来、等用户挑的（选了移到废纸篓）\n");
    for g in &ov.groups {
        out.push_str(&format!("### {} · {}（{} 项）\n", g.name, gb(g.size), g.items.len()));
        for i in g.items.iter().take(20) {
            out.push_str(&row(i));
        }
        if g.items.len() > 20 {
            out.push_str(&format!("- …还有 {} 项\n", g.items.len() - 20));
        }
    }
    if ov.groups.is_empty() {
        out.push_str("（没有）\n");
    }
    out
}

fn envs_text(j: &Janitor) -> String {
    let envs = j.envs();
    if envs.is_empty() {
        return "还没有开发环境的结果（等下一次扫描）。".into();
    }
    let mut out = String::new();
    for e in envs {
        out.push_str(&format!("## {} · {}{}\n", e.name, gb(e.size), e.note.map(|n| format!(" · {n}")).unwrap_or_default()));
        for i in e.items {
            let mut bits = vec![gb(i.size)];
            if let Some(v) = &i.version {
                bits.push(v.clone());
            }
            if let Some(b) = &i.by {
                bits.push(format!("{b}装的"));
            }
            if i.default {
                bits.push("默认".into());
            }
            if let Some(n) = i.packages {
                bits.push(format!("{n} 个包"));
            }
            if !i.top.is_empty() {
                bits.push(format!("最大：{}", i.top.iter().map(|(n, s)| format!("{n} {}", gb(*s))).collect::<Vec<_>>().join("、")));
            }
            for t in [&i.tag, &i.protected].into_iter().flatten() {
                bits.push(t.clone());
            }
            if i.admin {
                bits.push("删要管理员密码".into());
            }
            out.push_str(&format!("- {}{} · {}\n", if i.part { "  " } else { "" }, i.title, [i.path.clone().unwrap_or_default()].into_iter().chain(bits).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")));
        }
    }
    out
}

const SKILL: &str = "## 工作方式（DevJanitor 的 SKILL）
1. 先看下面「现在的情况」；要细节用 pomelo_janitor_look：folder 看一个文件夹里有什么（名字、大小、改于何时），find 在一个文件夹里按名字、扩展名、大小、多久没改找文件，envs 看开发环境，cleanup 看清理页的清单（一键清理每一类有哪些、多大、在哪，和只列出来、等用户挑的），overview 看总的。只能看名字、大小和时间，看不到文件内容。
2. 想用命令看情况（du、ls、find、mdls、brew list、conda env list、pip list、npm ls -g、docker system df、java -version 这类只读的），用 pomelo_janitor_run 直接跑、马上看到输出，不用提议卡片。只能是一条普通的命令：不接管道、重定向、变量；不在只读清单里的会被拒，真要跑就提议卡片。
3. 判断一样东西能不能删：是谁留下的、删了会怎样、能不能重建、多久没用了。自己不确定它是什么，就联网搜索（有这个工具时）；还拿不准，或要用户拿主意，就用「问我」问（有这个工具时）。
4. 只能提议，用户在卡片上点了才执行：
   - 删一些文件或文件夹：pomelo_janitor_trash（移到废纸篓，还能放回去），一次可以列好几项，每项写为什么能删。
   - 用工具自己的命令清更稳妥时（conda clean、conda env remove、brew cleanup、nvm uninstall、npm cache clean、docker system prune…）：pomelo_janitor_command，一条命令调一次，写清楚做什么、会删掉什么、能不能恢复、大概腾出多少。要管理员密码的（sudo，比如删 /Library/Java 里的 JDK）照样写出来，用户自己去终端跑。
   - 发现一类以后每次都能放心直接删、删了能重建的（某个工具的缓存），而一键清理里还没有：pomelo_janitor_rule 提议加进一键清理（只能是个人文件夹里工具自己的地方，比如 ~/.something/cache、~/Library/Caches/某个 App）。
   - 卡片只能用这三个工具提：没调用工具就不要说「放进卡片了」。工具说「没提议成」时，照原因改了再提，或者跟用户说明。
   - 用户点过的卡片，结果写在那条回答后面（「卡片的结果」）；接着聊时照结果说。
5. 受保护的地方：Pomelo 的数据目录、钥匙串、本地模型（~/Documents/LLM）、Seeker 文件夹、个人网站里除了依赖和构建产物的东西、系统本身。能看；真要动也能提议，但说清楚风险（卡片会标「受保护」，用户要再确认一次）。
6. 文稿、下载、桌面、图片这些是用户自己的东西：用户没问就只说、不提议删。已经在一键清理里的东西，叫用户去「清理」页点，不用再提议。
7. 邮件（Mail）：~/Library/Mail 是 Mail 存在这台 Mac 上的邮件，~/Library/Containers/com.apple.mail 里有打开过的附件和缓存（清理页「只列出来」里的「邮件（Mail）的数据」）。用户不用 Mail 时：先请用户在「系统设置 → 互联网账户」里把每个账户的「邮件」关掉（不然打开 Mail 会重新下载）、退出 Mail，再提议移到废纸篓；「我的 Mac 上」的邮箱只存在本机，删之前问清楚。
8. 回答用中文、简洁：先说结论（能腾出多少、建议怎么做），再说为什么；卡片里已经写了路径和大小，回答里不用一一重复。";

/// The SKILL in use: mine, or the built-in one (and whether it's mine).
pub fn skill(c: &Connection) -> (String, bool) {
    match pref_get::<Option<String>>(c, SKILL_PREF).ok().flatten().flatten().filter(|t| !t.trim().is_empty()) {
        Some(t) => (t, true),
        None => (SKILL.to_string(), false),
    }
}

/// My SKILL (None or blank: back to the built-in one). A setting: not undone, not exported.
pub fn set_skill(c: &Connection, text: Option<&str>) -> Result<(), String> {
    let t = text.map(str::trim).filter(|t| !t.is_empty() && *t != SKILL.trim()).map(|t| t.chars().take(20_000).collect::<String>());
    pref_set(c, SKILL_PREF, &t)
}

/// What 设置 shows of the system message: the SKILL (mine to rewrite) and the rest, written each time.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub skill: String,
    pub custom: bool,
    pub head: String,
    pub situation: String,
}

pub fn prompt(state: &AppState, j: &Janitor) -> Prompt {
    let (s, rs, (skill, custom)) = state.youzi.with_db(|c| Ok((super::settings(c), rules(c)?, skill(c)))).unwrap_or_else(|_| (Default::default(), vec![], (SKILL.to_string(), false)));
    let added = if rs.is_empty() { String::new() } else { format!("\n已经加进一键清理的规则：{}\n", rs.iter().map(|r| format!("{}（{}）", r.name, r.pattern)).collect::<Vec<_>>().join("、")) };
    let head = format!("{MARK}，柚儿园里帮用户管这台 Mac 磁盘的助手（柚子的一个助手）。程序已经扫过一遍磁盘（粗的一层：各处多大、大文件、明显能清的）；细的——一个文件夹里哪些留哪些删——是你的事。个人文件夹是 {}。", j.home);
    Prompt { skill, custom, head, situation: format!("{}{added}", situation(j, &s, &rs, false)) }
}

/// The system message's part for DevJanitor's conversation: the SKILL and the situation now.
pub fn system(state: &AppState, j: &Janitor) -> String {
    let p = prompt(state, j);
    format!("{}\n\n{}\n\n## 现在的情况\n{}", p.head, p.skill.trim(), p.situation)
}

/// The tools of DevJanitor's conversation.
pub fn tools() -> Vec<Value> {
    let s = |d: &str| json!({ "type": "string", "description": d });
    vec![
        json!({ "type": "function", "function": { "name": LOOK, "description": "看这台 Mac 的磁盘（只读，只看名字、大小、时间）。", "parameters": { "type": "object", "properties": {
            "view": { "type": "string", "enum": ["overview", "folder", "find", "envs", "cleanup"], "description": "overview 总的情况；folder 一个文件夹里的东西；find 在一个文件夹里找文件；envs 开发环境；cleanup 清理页的清单（一键清理每一类有哪些、只列出来的）" },
            "path": s("folder / find 的文件夹（完整路径，可以用 ~）"),
            "name": s("find：文件名里有这些字"),
            "ext": s("find：扩展名，如 dmg"),
            "minMb": { "type": "number", "description": "find：至少多少 MB" },
            "olderDays": { "type": "integer", "description": "find：至少这么多天没改过" }
        }, "required": ["view"] } } }),
        json!({ "type": "function", "function": { "name": RUN, "description": "跑一条只读的命令看情况（不改任何东西），马上看到输出。只认只读清单里的：du、ls、stat、find（不带 -delete、-exec）、mdls、mdfind、df、ps、lsof、brew list / info / outdated / leaves / deps / --cache / cleanup -n、conda info / list / env list / clean --dry-run、pip list / show / cache dir、npm ls / root / config get、pnpm store path、docker system df / images / ps、java -version、/usr/libexec/java_home -V、xcrun simctl list、rustup show、nvm ls 这类。一条普通命令，不接管道、重定向、变量。会改东西的用 pomelo_janitor_command 提议。", "parameters": { "type": "object", "properties": {
            "command": s("一条只读命令（不换行）")
        }, "required": ["command"] } } }),
        json!({ "type": "function", "function": { "name": TRASH, "description": "提议把一些文件或文件夹移到废纸篓（用户在卡片上勾选、点了才移；能从废纸篓放回去）。", "parameters": { "type": "object", "properties": {
            "title": s("这张卡片说什么，如「旧的安装包」"),
            "items": { "type": "array", "items": { "type": "object", "properties": { "path": s("完整路径（可以用 ~）"), "why": s("为什么能删") }, "required": ["path", "why"] } }
        }, "required": ["items"] } } }),
        json!({ "type": "function", "function": { "name": COMMAND, "description": "提议跑一条终端命令（一次一条；用户在卡片上点了才跑，用用户的终端环境，nvm、conda 能用）。", "parameters": { "type": "object", "properties": {
            "command": s("一条命令（不换行）"),
            "what": s("做什么"),
            "deletes": s("会删掉什么"),
            "recoverable": s("能不能恢复、怎么恢复"),
            "frees": s("大概腾出多少，如「约 3 GB」")
        }, "required": ["command", "what", "deletes", "recoverable"] } } }),
        json!({ "type": "function", "function": { "name": RULE, "description": "提议把一类以后每次都能直接删、删了能重建的文件夹加进一键清理（用户确认后才加）。", "parameters": { "type": "object", "properties": {
            "name": s("这一类叫什么，如「Whisper 的模型缓存」"),
            "path": s("在哪：完整路径，* 代表一个文件夹名里的任意字（如 ~/Library/Caches/com.foo.*）"),
            "recognize": s("怎么认出来"),
            "rebuild": s("删了怎么重建"),
            "checked": { "type": "boolean", "description": "在一键清理里要不要默认勾上" }
        }, "required": ["name", "path", "recognize", "rebuild"] } } }),
    ]
}

/// Runs `LOOK` (read only).
pub fn look(app: &AppHandle, args: &str, budget: i64) -> Answer {
    let j = app.state::<Janitor>();
    let st = app.state::<AppState>();
    let (s, rs) = st.youzi.with_db(|c| Ok((super::settings(c), rules(c)?))).unwrap_or_default();
    look_with(&j, &s, &rs, &site_folder(&st, &j), args, budget)
}

pub fn look_with(j: &Janitor, s: &super::Settings, rs: &[Rule], site: &str, args: &str, budget: i64) -> Answer {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let view = v["view"].as_str().unwrap_or("overview");
    let protected = Protected::new(&j.home, site);
    let path = v["path"].as_str().map(|p| virt_of(&j.home, p));
    let r = match (view, path) {
        ("folder", Some(Ok(p))) => folder(j, &protected, &p).map(|t| (format!("DevJanitor · 看 {p}"), t)),
        ("find", Some(Ok(p))) => find(j, &protected, &p, &v).map(|t| (format!("DevJanitor · 在 {p} 里找"), t)),
        ("folder" | "find", Some(Err(e))) => Err(e),
        ("folder" | "find", None) => Err("要写 path".into()),
        ("envs", _) => Ok(("DevJanitor · 开发环境".into(), envs_text(j))),
        ("cleanup", _) => Ok(("DevJanitor · 清理页的清单".into(), cleanup_text(j, s, rs))),
        _ => Ok(("DevJanitor · 总的情况".into(), situation(j, s, rs, false))),
    };
    match r {
        Ok((label, text)) => answer(label, true, fit(text, budget)),
        Err(e) => answer("DevJanitor · 没看成", false, format!("没看成：{e}")),
    }
}

fn site_folder(st: &AppState, j: &Janitor) -> String {
    super::site_folder(st, j)
}

// ---- read-only commands ----

/// Runs `RUN`: a command on the read-only list, right away (the fake Mac: not run, said so).
pub fn run_readonly(app: &AppHandle, args: &str, budget: i64) -> Answer {
    let j = app.state::<Janitor>();
    run_readonly_with(&j, args, budget)
}

pub fn run_readonly_with(j: &Janitor, args: &str, budget: i64) -> Answer {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let cmd = v["command"].as_str().unwrap_or("").trim().to_string();
    let short: String = cmd.chars().take(60).collect();
    if let Err(e) = super::readonly::check(&cmd) {
        return answer(format!("DevJanitor · 没跑 {short}"), false, format!("没跑：{e}。要跑就用 pomelo_janitor_command 提议卡片，用户点了才跑。"));
    }
    let out = match j.mode {
        Mode::Sandbox => format!("$ {cmd}\n（假 Mac：开发版和测试不真跑命令，这是编的结果）\n12M\t{}/Library/Caches/com.example.tool", j.home),
        Mode::Real => format!("$ {cmd}\n{}", super::readonly::run(&cmd, &j.real_of(&j.home))),
    };
    answer(format!("DevJanitor · 跑了 {short}"), true, fit(out, budget))
}

/// What a reply's cards came to, for the model's later turns (only cards I clicked or turned down).
pub fn in_history(cards: &[Card]) -> String {
    let lines: Vec<String> = cards
        .iter()
        .filter(|c| c.state != "pending")
        .map(|c| {
            let what = match c.kind.as_str() {
                "trash" => format!("移到废纸篓「{}」", c.title),
                "command" => format!("跑命令 {}", c.command),
                _ => format!("加进一键清理「{}」", c.title),
            };
            let how = match (c.kind.as_str(), c.state.as_str()) {
                (_, "cancelled") => "用户点了「不要」".to_string(),
                (_, "running") => "还在跑".to_string(),
                (_, "stopped") => format!("用户停下了{}", tail_of(&c.output)),
                ("trash", _) => {
                    let done = c.items.iter().filter(|i| i.done).count();
                    let not: Vec<String> = c.items.iter().filter(|i| !i.done).map(|i| format!("{}（{}）", i.path, i.error.as_deref().unwrap_or("没勾"))).collect();
                    if not.is_empty() { format!("移了 {done} 项") } else { format!("移了 {done} 项；没移：{}", not.join("、")) }
                }
                ("command", s) => format!("{}（退出码 {}）{}", if s == "done" { "跑完了" } else { "没成功" }, c.code.map_or("—".into(), |x| x.to_string()), tail_of(&c.output)),
                (_, "done") => "加上了".to_string(),
                _ => c.error.clone().unwrap_or_else(|| c.state.clone()),
            };
            format!("- {what}：{how}")
        })
        .collect();
    if lines.is_empty() {
        String::new()
    } else {
        format!("\n\n（卡片的结果：\n{}）", lines.join("\n"))
    }
}

/// A command's output, its last part, for the history.
fn tail_of(output: &str) -> String {
    let t = output.trim();
    if t.is_empty() {
        return String::new();
    }
    let n = t.chars().count();
    let cut: String = if n > 1500 { t.chars().skip(n - 1500).collect() } else { t.to_string() };
    format!("，输出{}：\n{cut}", if n > 1500 { "的最后一段" } else { "" })
}

// ---- cards ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrashItem {
    pub path: String,
    pub why: String,
    pub size: u64,
    pub protected: Option<String>,
    pub kind: String,
    /// After 移到废纸篓: moved, or why not.
    pub done: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Card {
    pub id: i64,
    pub run_id: i64,
    /// trash | command | rule
    pub kind: String,
    pub state: String,
    pub title: String,
    pub items: Vec<TrashItem>,
    pub command: String,
    pub what: String,
    pub deletes: String,
    pub recoverable: String,
    pub frees: String,
    /// The command needs sudo: shown only, run in Terminal.
    pub sudo: bool,
    pub protected: Option<String>,
    pub rule: Option<Rule>,
    /// A rule: what it matches now.
    pub matches: u64,
    pub match_size: u64,
    /// A command: its output (the last part), exit code.
    pub output: String,
    pub code: Option<i32>,
    pub error: Option<String>,
}

fn card_row(r: &rusqlite::Row) -> rusqlite::Result<Card> {
    let body: String = r.get(3)?;
    let result: Option<String> = r.get(5)?;
    let mut c: Card = serde_json::from_str(&body).unwrap_or_default();
    c.id = r.get(0)?;
    c.run_id = r.get(1)?;
    c.kind = r.get(2)?;
    c.state = r.get(4)?;
    if let Some(res) = result.and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        if let Some(o) = res["output"].as_str() {
            c.output = o.to_string();
        }
        c.code = res["code"].as_i64().map(|x| x as i32);
        c.error = res["error"].as_str().map(str::to_string);
        if let Some(items) = res["items"].as_array() {
            for (it, done) in c.items.iter_mut().zip(items) {
                it.done = done["done"].as_bool().unwrap_or(false);
                it.error = done["error"].as_str().map(str::to_string);
            }
        }
        if let Some(id) = res["rule"].as_i64() {
            if let Some(r) = c.rule.as_mut() {
                r.id = id;
            }
        }
    }
    Ok(c)
}

const CARD_COLS: &str = "id, run_id, kind, body, state, result";

pub fn cards_of(c: &Connection, run: i64) -> Result<Vec<Card>, String> {
    let mut s = c.prepare_cached(&format!("SELECT {CARD_COLS} FROM janitor_cards WHERE run_id = ?1 ORDER BY id")).map_err(err)?;
    let rows = s.query_map([run], card_row).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    Ok(rows)
}

/// The cards of the reply `message_id` (its run's).
pub fn of_message(c: &Connection, message_id: i64) -> Result<Vec<Card>, String> {
    let run: Option<i64> = c.query_row("SELECT run_id FROM messages WHERE id = ?1", [message_id], |r| r.get(0)).optional().map_err(err)?.flatten();
    run.map_or(Ok(vec![]), |r| cards_of(c, r))
}

pub fn card(c: &Connection, id: i64) -> Result<Card, String> {
    c.query_row(&format!("SELECT {CARD_COLS} FROM janitor_cards WHERE id = ?1"), [id], card_row).optional().map_err(err)?.ok_or_else(|| "这张卡片已经不在了".to_string())
}

pub fn save_card(c: &Connection, run: i64, card: &Card) -> Result<i64, String> {
    let body = serde_json::to_string(&Card { id: 0, run_id: 0, state: String::new(), output: String::new(), code: None, error: None, ..card.clone() }).map_err(err)?;
    c.execute("INSERT INTO janitor_cards (run_id, kind, body) VALUES (?1, ?2, ?3)", params![run, card.kind, body]).map_err(err)?;
    Ok(c.last_insert_rowid())
}

pub fn set_state(c: &Connection, id: i64, state: &str, result: Option<&Value>) -> Result<(), String> {
    c.execute("UPDATE janitor_cards SET state = ?1, result = ?2 WHERE id = ?3", params![state, result.map(Value::to_string), id]).map_err(err)?;
    Ok(())
}

/// The shell words that name a path (`~/x`, `$HOME/x`, `/x`), as virtual paths.
pub fn command_paths(home: &str, cmd: &str) -> Vec<String> {
    cmd.split(|c: char| c.is_whitespace() || ";&|()<>=".contains(c))
        .map(|w| w.trim_matches(['"', '\'']))
        .filter(|w| w.starts_with('/') || w.starts_with('~') || w.starts_with("$HOME") || w.starts_with("${HOME}"))
        .filter_map(|w| virt_of(home, w.split('*').next().unwrap_or(w)).ok())
        .collect()
}

/// sudo / su / doas as a command word.
pub fn needs_admin(cmd: &str) -> bool {
    cmd.split(|c: char| c.is_whitespace() || ";&|(".contains(c)).any(|w| matches!(w, "sudo" | "su" | "doas"))
}

/// A command touching a protected place: which.
pub fn command_protected(home: &str, protected: &Protected, cmd: &str) -> Option<String> {
    if cmd.split_whitespace().any(|w| w == "security") {
        return Some("钥匙串".into());
    }
    command_paths(home, cmd).iter().find_map(|p| protected.of(p)).map(str::to_string)
}

/// A proposal tool: checked, then a card in the reply (`run`).
pub fn propose(app: &AppHandle, run: i64, name: &str, args: &str) -> Answer {
    let j = app.state::<Janitor>();
    let st = app.state::<AppState>();
    // the card is checked off the youzi.db lock (sizes come from janitor.db and the disk), then saved
    let card = match build_card(&j, &site_folder(&st, &j), name, args) {
        Ok(c) => c,
        Err(e) => return answer("DevJanitor · 没提议成", false, e),
    };
    match st.youzi.with_db(|c| save_card(c, run, &card)) {
        Ok(_) => proposed(&card),
        Err(e) => answer("DevJanitor · 没提议成", false, e),
    }
}

/// What the model hears back about a card it proposed.
pub fn proposed(card: &Card) -> Answer {
    let what = match card.kind.as_str() {
        "trash" => format!("移到废纸篓 · {} 项 · {}", card.items.len(), gb(card.items.iter().map(|i| i.size).sum())),
        "command" => format!("跑命令 · {}", card.command.chars().take(40).collect::<String>()),
        _ => format!("加进一键清理 · {}（现在有 {} 处，{}）", card.title, card.matches, gb(card.match_size)),
    };
    let note = if card.sudo { "（要管理员密码：卡片上只给命令，用户自己去终端跑）" } else if card.protected.is_some() { "（碰到受保护的地方，用户要再确认一次）" } else { "" };
    answer(format!("提议：{what}"), true, format!("放进卡片了：{what}{note}。等用户在卡片上点，回答里不用重复路径。"))
}

/// A proposal checked into a card: paths exist and get their sizes, protected places marked, sudo found,
/// a rule's pattern checked and expanded.
pub fn build_card(j: &Janitor, site: &str, name: &str, args: &str) -> Result<Card, String> {
    let protected = Protected::new(&j.home, site);
    let v: Value = serde_json::from_str(args).map_err(|_| "参数不是 JSON".to_string())?;
    let text = |k: &str, max: usize| -> String { v[k].as_str().unwrap_or("").trim().chars().take(max).collect() };
    let card = match name {
        TRASH => {
            let mut items = Vec::new();
            let mut left = 300_000usize;
            for it in v["items"].as_array().cloned().unwrap_or_default().iter().take(50) {
                let p = match virt_of(&j.home, it["path"].as_str().unwrap_or("")) {
                    Ok(p) => p,
                    Err(e) => return Err(e),
                };
                if p == j.home || p == "/" {
                    return Err(format!("{p} 不能整个移到废纸篓"));
                }
                let real = j.real_of(&p);
                let Ok(m) = std::fs::symlink_metadata(&real) else { return Err(format!("{p} 不在（先用 pomelo_janitor_look 看清楚路径）")) };
                if items.iter().any(|x: &TrashItem| x.path == p) {
                    continue;
                }
                let (size, _) = size_of(j, &p, &mut left);
                items.push(TrashItem { path: p.clone(), why: it["why"].as_str().unwrap_or("").trim().chars().take(200).collect(), size, protected: protected.of(&p).map(str::to_string), kind: if m.is_dir() { "文件夹".into() } else { "文件".into() }, ..Default::default() });
            }
            if items.is_empty() {
                return Err("items 是空的".into());
            }
            let protected = items.iter().find_map(|i| i.protected.clone());
            Card { kind: "trash".into(), title: Some(text("title", 40)).filter(|t| !t.is_empty()).unwrap_or_else(|| "移到废纸篓".into()), items, protected, ..Default::default() }
        }
        COMMAND => {
            let cmd = v["command"].as_str().unwrap_or("").trim().to_string();
            if cmd.is_empty() {
                return Err("command 是空的".into());
            }
            if cmd.contains('\n') {
                return Err("一条命令一张卡片：不要换行（要几条就调几次）".into());
            }
            if cmd.chars().count() > 2000 {
                return Err("命令太长了".into());
            }
            if super::readonly::check(&cmd).is_ok() {
                return Err("这是只读命令：直接用 pomelo_janitor_run 跑、看输出，不用提议卡片".into());
            }
            Card { kind: "command".into(), title: text("what", 80), sudo: needs_admin(&cmd), protected: command_protected(&j.home, &protected, &cmd), command: cmd, what: text("what", 300), deletes: text("deletes", 300), recoverable: text("recoverable", 300), frees: text("frees", 40), ..Default::default() }
        }
        RULE => {
            let pattern = match check_pattern(&j.home, &protected, v["path"].as_str().unwrap_or("")) {
                Ok(p) => p,
                Err(e) => return Err(e),
            };
            let name = text("name", 30);
            if name.is_empty() {
                return Err("要写 name".into());
            }
            let (matches, match_size) = expand(j, &pattern);
            let rule = Rule { name: name.clone(), pattern, recognize: text("recognize", 200), rebuild: text("rebuild", 100), checked: v["checked"].as_bool().unwrap_or(false), ..Default::default() };
            Card { kind: "rule".into(), title: name, rule: Some(rule), matches, match_size, ..Default::default() }
        }
        _ => return Err("没有这个工具".into()),
    };
    Ok(card)
}

/// A rule's matches now (live: a folder name at a time), and their size.
pub fn expand(j: &Janitor, pattern: &str) -> (u64, u64) {
    let segs: Vec<&str> = pattern.trim_start_matches('/').split('/').collect();
    let mut at = vec![String::new()];
    for s in &segs {
        let mut next = Vec::new();
        for base in &at {
            let dir = if base.is_empty() { "/".to_string() } else { base.clone() };
            if s.contains('*') {
                if let Ok(r) = std::fs::read_dir(j.real_of(&dir)) {
                    for e in r.flatten() {
                        let n = e.file_name().to_string_lossy().into_owned();
                        if glob(s, &n) {
                            next.push(format!("{base}/{n}"));
                        }
                    }
                }
            } else if j.real_of(&format!("{base}/{s}")).exists() {
                next.push(format!("{base}/{s}"));
            }
        }
        at = next;
        if at.len() > 200 {
            break;
        }
    }
    let mut left = 300_000usize;
    let size = at.iter().map(|p| size_of(j, p, &mut left).0).sum();
    (at.len() as u64, size)
}

// ---- running cards ----

/// A command running (in memory).
#[derive(Default)]
pub struct Cmd {
    pub output: Arc<Mutex<String>>,
    pub pid: Mutex<Option<u32>>,
    pub stopped: AtomicBool,
}

const KEEP_OUTPUT: usize = 20_000;

fn push_output(out: &Mutex<String>, text: &str) {
    let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
    o.push_str(text);
    if o.len() > KEEP_OUTPUT * 2 {
        let cut = o.len() - KEEP_OUTPUT;
        let at = (cut..o.len()).find(|i| o.is_char_boundary(*i)).unwrap_or(o.len());
        *o = format!("…{}", &o[at..]);
    }
}

fn tail(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let cut = s.len() - n;
    let at = (cut..s.len()).find(|i| s.is_char_boundary(*i)).unwrap_or(s.len());
    format!("…{}", &s[at..])
}

/// The card as the page should see it now (a running command's output is live).
pub fn card_now(app: &AppHandle, id: i64) -> Result<Card, String> {
    let mut c = app.state::<AppState>().youzi.with_db(|c| card(c, id))?;
    if let Some(run) = app.state::<Janitor>().cmds.lock().ok().and_then(|m| m.get(&id).cloned()) {
        c.output = run.output.lock().map(|o| o.clone()).unwrap_or_default();
    }
    Ok(c)
}

/// 移到废纸篓 the picked items (a protected one only when `protected_ok`).
pub fn run_trash(app: &AppHandle, id: i64, picked: &[usize], protected_ok: bool) -> Result<Card, String> {
    let st = app.state::<AppState>();
    let j = app.state::<Janitor>();
    let c = st.youzi.with_db(|c| card(c, id))?;
    let (state, result) = trash_card(&j, &c, picked, protected_ok)?;
    st.youzi.with_db(|c| set_state(c, id, state, Some(&result)))?;
    st.youzi.with_db(|c| card(c, id))
}

/// Moves a trash card's picked items to the Trash; janitor.db forgets them. (state, result) to save.
pub fn trash_card(j: &Janitor, c: &Card, picked: &[usize], protected_ok: bool) -> Result<(&'static str, Value), String> {
    if c.kind != "trash" || c.state != "pending" {
        return Err("这张卡片已经处理过了".into());
    }
    if picked.is_empty() {
        return Err("至少勾上一项".into());
    }
    let sys = j.sys();
    let home_real = j.real_of(&j.home);
    let mut results = Vec::new();
    let mut gone = Vec::new();
    let mut freed = 0;
    for (k, it) in c.items.iter().enumerate() {
        let r: Result<(), String> = if !picked.contains(&k) {
            Err("没勾".into())
        } else if it.protected.is_some() && !protected_ok {
            Err("受保护，要再确认一次".into())
        } else {
            let real = j.real_of(&it.path);
            if std::fs::symlink_metadata(&real).is_err() { Err("已经不在了".into()) } else { sys.trash(&real, &home_real) }
        };
        if r.is_ok() {
            freed += it.size;
            gone.push((0, vec![it.path.clone()], it.size));
        }
        results.push(json!({ "done": r.is_ok(), "error": r.err().filter(|e| e != "没勾") }));
    }
    let done = results.iter().filter(|r| r["done"] == true).count();
    let _ = j.with_db(|c| {
        store::forget(c, &gone)?;
        // the items it took are no longer listed
        for (_, paths, _) in &gone {
            c.execute("DELETE FROM items WHERE paths = ?1", [serde_json::to_string(paths).map_err(err)?]).map_err(err)?;
        }
        Ok(())
    });
    if done > 0 {
        let _ = j.with_db(|c| super::bump_trash(c, freed as i64, done as i64));
    }
    Ok((if done > 0 { "done" } else { "failed" }, json!({ "items": results, "freed": freed })))
}

/// 执行 a command card: in the background (sandbox: not run, a made-up result).
pub fn run_command(app: &AppHandle, id: i64, protected_ok: bool) -> Result<Card, String> {
    let st = app.state::<AppState>();
    let j = app.state::<Janitor>();
    let c = st.youzi.with_db(|c| card(c, id))?;
    may_run(&c, protected_ok)?;
    st.youzi.with_db(|cn| set_state(cn, id, "running", None))?;
    let cmd = Arc::new(Cmd::default());
    if let Ok(mut m) = j.cmds.lock() {
        m.insert(id, cmd.clone());
    }
    let (app2, command, mode, home) = (app.clone(), c.command.clone(), j.mode, j.real_of(&j.home));
    std::thread::spawn(move || {
        let (state, code, error) = match mode {
            Mode::Sandbox => {
                std::thread::sleep(std::time::Duration::from_millis(600));
                push_output(&cmd.output, &format!("$ {command}\n（假 Mac：这条命令没有真的跑，开发版和测试都不跑命令）\n"));
                ("done", Some(0), None)
            }
            Mode::Real => shell(&command, &home, &cmd),
        };
        let output = cmd.output.lock().map(|o| tail(&o, 8_000)).unwrap_or_default();
        let state = if cmd.stopped.load(Ordering::SeqCst) { "stopped" } else { state };
        let _ = app2.state::<AppState>().youzi.with_db(|c| set_state(c, id, state, Some(&json!({ "output": output, "code": code, "error": error }))));
        if let Ok(mut m) = app2.state::<Janitor>().cmds.lock() {
            m.remove(&id);
        }
    });
    card_now(app, id)
}

/// A command card may run: pending, no sudo, a protected place only confirmed twice.
pub fn may_run(c: &Card, protected_ok: bool) -> Result<(), String> {
    if c.kind != "command" || c.state != "pending" {
        return Err("这张卡片已经处理过了".into());
    }
    if c.sudo {
        return Err("这条命令要管理员密码：复制到终端里自己跑".into());
    }
    if c.protected.is_some() && !protected_ok {
        return Err("这条命令碰到受保护的地方，要再确认一次".into());
    }
    Ok(())
}

/// Runs a command in my login shell from home: output into `cmd`, the exit code.
fn shell(command: &str, home: &Path, cmd: &Cmd) -> (&'static str, Option<i32>, Option<String>) {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    let child = std::process::Command::new("/bin/zsh")
        .args(["-lic", command])
        .current_dir(home)
        .env("TERM", "dumb")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return ("failed", None, Some(format!("没跑起来：{e}"))),
    };
    if let Ok(mut p) = cmd.pid.lock() {
        *p = Some(child.id());
    }
    let mut readers = Vec::new();
    for mut pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)].into_iter().flatten() {
        let out = cmd.output.clone();
        readers.push(std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                push_output(&out, &String::from_utf8_lossy(&buf[..n]));
            }
        }));
    }
    let status = child.wait();
    for r in readers {
        let _ = r.join();
    }
    match status {
        Ok(s) if s.success() => ("done", s.code(), None),
        Ok(s) => ("failed", s.code(), Some(format!("命令没成功（退出码 {}）", s.code().map_or("—".into(), |c| c.to_string())))),
        Err(e) => ("failed", None, Some(e.to_string())),
    }
}

/// 停下 a running command (its whole process group).
pub fn stop_command(app: &AppHandle, id: i64) -> Result<Card, String> {
    let run = app.state::<Janitor>().cmds.lock().ok().and_then(|m| m.get(&id).cloned());
    let Some(run) = run else { return card_now(app, id) };
    run.stopped.store(true, Ordering::SeqCst);
    if let Some(pid) = run.pid.lock().ok().and_then(|p| *p) {
        let _ = std::process::Command::new("/bin/kill").args(["-TERM", "--", &format!("-{pid}")]).status();
    }
    card_now(app, id)
}

/// 加进一键清理: one undo step (the rule and the card), then a rescan picks it up.
pub fn add_rule(app: &AppHandle, id: i64) -> Result<Card, String> {
    let st = app.state::<AppState>();
    let j = app.state::<Janitor>();
    let protected = Protected::new(&j.home, &site_folder(&st, &j));
    st.with_youzi_undo("加进一键清理", |c| rule_in(c, &j.home, &protected, id).map(|_| ()))?;
    let site = site_folder(&st, &j);
    let rs = st.youzi.with_db(|c| rules(c)).unwrap_or_default();
    j.start(site, rs, true);
    st.youzi.with_db(|c| card(c, id))
}

/// A rule card's rule into janitor_rules (checked again), the card done: the rule's id.
pub fn rule_in(c: &Connection, home: &str, protected: &Protected, id: i64) -> Result<i64, String> {
    let card = card(c, id)?;
    if card.kind != "rule" || card.state != "pending" {
        return Err("这张卡片已经处理过了".into());
    }
    let r = card.rule.clone().ok_or("卡片里没有规则")?;
    let pattern = check_pattern(home, protected, &r.pattern)?;
    let tx = Tx::begin(c)?;
    tx.execute("INSERT INTO janitor_rules (name, pattern, recognize, rebuild, checked) VALUES (?1, ?2, ?3, ?4, ?5)", params![r.name, pattern, r.recognize, r.rebuild, r.checked]).map_err(err)?;
    let rule_id = tx.last_insert_rowid();
    set_state(&tx, id, "done", Some(&json!({ "rule": rule_id })))?;
    tx.commit()?;
    Ok(rule_id)
}

/// 不要 a card.
pub fn cancel(app: &AppHandle, id: i64) -> Result<Card, String> {
    let st = app.state::<AppState>();
    st.youzi.with_db(|c| {
        let card = card(c, id)?;
        if card.state != "pending" {
            return Err("这张卡片已经处理过了".into());
        }
        set_state(c, id, "cancelled", None)?;
        self::card(c, id)
    })
}

// ---- 柚子's look at DevJanitor ----

pub fn info_tool() -> Value {
    json!({ "type": "function", "function": { "name": INFO, "description": "看 DevJanitor（电脑管家）扫描的结果（只读）：磁盘、一键清理能清多少、开发环境。", "parameters": { "type": "object", "properties": {
        "view": { "type": "string", "enum": ["overview", "envs"], "description": "overview 磁盘和能清多少；envs 开发环境" }
    }, "required": ["view"] } } })
}

pub fn info(app: &AppHandle, args: &str, budget: i64) -> Answer {
    let j = app.state::<Janitor>();
    let st = app.state::<AppState>();
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let (label, text) = if v["view"] == "envs" {
        ("DevJanitor · 开发环境", envs_text(&j))
    } else {
        let (s, rs) = st.youzi.with_db(|c| Ok((super::settings(c), rules(c)?))).unwrap_or_default();
        ("DevJanitor · 磁盘和能清多少", situation(&j, &s, &rs, true))
    };
    Answer { module: Some(MODULE.into()), label: label.into(), state: "ok", text: fit(text, budget), records: vec![] }
}

// ---- the mock model ----

/// What the mock model calls in DevJanitor's conversation: a look, then cards by what's asked.
pub fn mock_calls(question: &str) -> Vec<crate::youzi::provider::ToolCall> {
    use crate::youzi::provider::ToolCall;
    let call = |id: &str, name: &str, args: Value| ToolCall { id: id.into(), name: name.into(), arguments: args.to_string() };
    let mut out = vec![call("j0", LOOK, json!({ "view": if question.contains("环境") { "envs" } else { "overview" } }))];
    if question.contains("看看") || question.contains("文件夹") {
        out.push(call("j1", LOOK, json!({ "view": "folder", "path": "~/Downloads" })));
    }
    if question.contains("只读") || question.contains("du") {
        out.push(call("jr", RUN, json!({ "command": "du -sh ~/Library/Caches" })));
    }
    if question.contains("清单") {
        out.push(call("jc", LOOK, json!({ "view": "cleanup" })));
    }
    if question.contains("删") || question.contains("废纸篓") {
        out.push(call("j2", TRASH, json!({ "title": "用不上的东西", "items": [{ "path": "~/.mystery-tool", "why": "（模拟）认不出来，90 天没动" }, { "path": "~/Downloads/旧版微信.dmg", "why": "（模拟）装过了的安装包" }, { "path": "~/Documents/LLM/models/embedding.gguf", "why": "（模拟）受保护的例子" }] })));
    }
    if question.contains("命令") || question.contains("conda") {
        out.push(call("j3", COMMAND, json!({ "command": "conda clean --all --yes", "what": "（模拟）清掉 Conda 没在用的包和下载的压缩包", "deletes": "pkgs 里没被任何环境用到的包", "recoverable": "以后装包时会重新下载", "frees": "约 3 GB" })));
    }
    if question.contains("sudo") || question.contains("JDK") {
        out.push(call("j4", COMMAND, json!({ "command": "sudo rm -rf /Library/Java/JavaVirtualMachines/jdk-1.8.jdk", "what": "（模拟）删掉 JDK 8", "deletes": "JDK 8 整个文件夹", "recoverable": "要用时重新装", "frees": "约 210 MB" })));
    }
    if question.contains("规则") || question.contains("一键清理") {
        out.push(call("j5", RULE, json!({ "name": "Whisper 的模型缓存", "path": "~/.cache/whisper", "recognize": "（模拟）Whisper 下载的模型", "rebuild": "用到时重新下载", "checked": false })));
    }
    out
}

/// The mock model's reply after its calls.
pub fn mock_answer(messages: &[crate::youzi::provider::Msg]) -> String {
    let cards = messages.iter().filter(|m| m.role == "tool" && m.content.starts_with("放进卡片了")).count();
    let looked = messages.iter().filter(|m| m.role == "tool" && !m.content.starts_with("放进卡片了")).count();
    if cards > 0 {
        format!("这是开发版的**模拟回答**：看了 {looked} 处，放进了 {cards} 张卡片，看过没问题再点。")
    } else {
        format!("这是开发版的**模拟回答**：看了 {looked} 处。说「删」「命令」「规则」我会提议卡片。")
    }
}

/// The commands' state map, per card (in memory).
pub type Cmds = Mutex<HashMap<i64, Arc<Cmd>>>;
