//! Exporting a conversation (from Picky's ConversationExport):
//! - 复制整段对话: the shown path as Markdown (no reasoning).
//! - 导出 Markdown: the shown path, numbered, with version marks, states and folded reasoning.
//! - 导出完整包: a new folder (never merged into an existing one; written beside it, then renamed in
//!   one step) holding 对话.md (shown path), 全部版本.md (every message) and 完整记录.json (the message
//!   tree, runs and context summaries). Both Markdown files end with the context summaries (对话.md:
//!   those on the shown path). Questions list their attachments and project files; replies fold
//!   their reading notes. The package also holds 附件/ with a copy of every file the conversation
//!   used (deleted ones are noted), linked from the Markdown; the JSON keeps passages and memories
//!   per run (memories stay out of the Markdown).
//! - Lookups in Pomelo (v0.3.5): replies show 「查了：…」 and fold what each lookup returned, in every
//!   Markdown form (复制 only the line); the JSON keeps them per message (with their records).

use super::chat;
use super::files::{self, MessageFile};
use super::index::Passage;
use super::reading;
use crate::modules::common::err;
use std::collections::HashMap;
use crate::AppState;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

pub const FORMAT: &str = "pomelo-picky-conversation-v1";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    id: i64,
    parent_id: Option<i64>,
    active_child_id: Option<i64>,
    role: String,
    content: String,
    reasoning: String,
    reasoning_ms: Option<i64>,
    state: String,
    run_id: Option<i64>,
    model: Option<String>,
    version_index: i64,
    version_count: i64,
    created_at: String,
    files: Vec<MessageFile>,
    notes: Vec<NoteRecord>,
    queries: Vec<super::data::Query>,
}

#[derive(Debug, Clone, Serialize)]
struct NoteRecord {
    label: String,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunRecord {
    id: i64,
    kind: String,
    model: String,
    model_name: String,
    state: String,
    error: Option<String>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    started_at: String,
    ended_at: Option<String>,
    passages: Vec<Passage>,
    memories: Vec<super::memory::UsedMemory>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryRecord {
    id: i64,
    through_message_id: i64,
    run_id: Option<i64>,
    content: String,
    created_at: String,
}

fn summaries(conn: &Connection, id: i64) -> Result<Vec<SummaryRecord>, String> {
    let mut st = conn.prepare("SELECT id, through_message_id, run_id, content, created_at FROM context_summaries WHERE session_id = ?1 ORDER BY created_at, id").map_err(err)?;
    let rows = st
        .query_map([id], |r| Ok(SummaryRecord { id: r.get(0)?, through_message_id: r.get(1)?, run_id: r.get(2)?, content: r.get(3)?, created_at: r.get(4)? }))
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

fn title(conn: &Connection, id: i64) -> Result<String, String> {
    conn.query_row("SELECT title FROM sessions WHERE id = ?1", [id], |r| r.get::<_, String>(0)).map_err(|_| "找不到这个对话，可能已经被删除了".to_string()).map(|t| t.replace('\n', " "))
}

/// Every message of the session, oldest first, with version numbers.
fn all_messages(conn: &Connection, id: i64) -> Result<Vec<Node>, String> {
    let mut st = conn
        .prepare(
            "SELECT m.id, m.parent_id, m.active_child_id, m.role, m.content, m.reasoning, m.reasoning_ms, m.state, m.run_id,
                    CASE WHEN m.role = 'assistant' THEN r.model_name END, m.created_at
             FROM messages m LEFT JOIN runs r ON r.id = m.run_id WHERE m.session_id = ?1 ORDER BY m.created_at, m.id",
        )
        .map_err(err)?;
    let mut nodes: Vec<Node> = st
        .query_map([id], |r| {
            Ok(Node {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                active_child_id: r.get(2)?,
                role: r.get(3)?,
                content: r.get(4)?,
                reasoning: r.get(5)?,
                reasoning_ms: r.get(6)?,
                state: r.get(7)?,
                run_id: r.get(8)?,
                model: r.get(9)?,
                version_index: 1,
                version_count: 1,
                created_at: r.get(10)?,
                files: vec![],
                notes: vec![],
                queries: vec![],
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let keys: Vec<(Option<i64>, String)> = nodes.iter().map(|n| (n.parent_id, n.role.clone())).collect();
    for i in 0..nodes.len() {
        let same: Vec<usize> = (0..nodes.len()).filter(|j| keys[*j] == keys[i]).collect();
        nodes[i].version_count = same.len() as i64;
        nodes[i].version_index = same.iter().position(|j| *j == i).unwrap_or(0) as i64 + 1;
    }
    for n in nodes.iter_mut() {
        if n.role == "user" {
            n.files = files::message_files(conn, n.id)?;
        } else if let Some(run) = n.run_id {
            n.notes = reading::notes_of(conn, run)?.into_iter().map(|(label, content)| NoteRecord { label, content }).collect();
            n.queries = super::data::queries_of(conn, run)?;
        }
    }
    Ok(nodes)
}

fn state_label(state: &str) -> Option<&'static str> {
    match state {
        "cancelled" => Some("已停止"),
        "failed" => Some("生成失败"),
        "interrupted" => Some("中断了"),
        "streaming" => Some("正在生成"),
        _ => None,
    }
}

fn role_label(role: &str, model: Option<&str>) -> String {
    match (role, model) {
        ("user", _) => "我".into(),
        (_, Some(m)) => format!("小拾 · {m}"),
        _ => "小拾".into(),
    }
}

/// 查了：投石问路 · 该跟进的投递（2 份）；…
fn looked_line(queries: &[super::data::Query]) -> String {
    format!("查了：{}", queries.iter().map(|q| q.label.as_str()).collect::<Vec<_>>().join("；"))
}

/// 复制整段对话.
pub fn copy_markdown(conn: &Connection, id: i64) -> Result<String, String> {
    let mut parts = vec![format!("# {}", title(conn, id)?)];
    for m in chat::path(conn, id)? {
        let mut lines = vec![format!("## {}", if m.role == "user" { "我" } else { "小拾" })];
        if let Some(s) = state_label(&m.state) {
            lines.push(format!("状态：{s}"));
        }
        lines.extend(m.files.iter().map(|f| format!("{}：{}", if f.file_id.is_some() { "资料" } else { "附件" }, f.file.name.replace('\n', " "))));
        if !m.queries.is_empty() {
            lines.push(looked_line(&m.queries));
        }
        if !m.content.is_empty() {
            lines.push(m.content.clone());
        }
        parts.push(lines.join("\n\n"));
    }
    Ok(parts.join("\n\n") + "\n")
}

/// `links`: the package's copy of each file (artifact → path under 附件/).
fn document(heading: &str, msgs: &[Node], all: bool, sums: &[SummaryRecord], links: &HashMap<i64, String>) -> String {
    let mut out = format!("# {heading}\n\n");
    for (i, m) in msgs.iter().enumerate() {
        out += &format!("## {}. {}", i + 1, role_label(&m.role, m.model.as_deref()));
        if m.version_count > 1 {
            out += &format!(" · 版本 {}/{}", m.version_index, m.version_count);
        }
        out += "\n\n";
        if all {
            out += &format!("消息：`{}`", m.id);
            if let Some(p) = m.parent_id {
                out += &format!(" · 上一条：`{p}`");
            }
            out += "\n\n";
        }
        if let Some(s) = state_label(&m.state) {
            out += &format!("状态：{s}\n\n");
        }
        for f in &m.files {
            let kind = f.label.clone().unwrap_or_else(|| "附件".into());
            let name = f.file.name.replace('\n', " ");
            out += &match (f.file.deleted, links.get(&f.file.id)) {
                (true, _) => format!("{kind}：{name}（文件已删除）\n\n"),
                (false, Some(l)) => format!("{kind}：[{name}](<{l}>)\n\n"),
                (false, None) => format!("{kind}：{name}\n\n"),
            };
        }
        if !m.queries.is_empty() {
            out += &looked_line(&m.queries);
            out += "\n\n<details>\n<summary>查到的内容</summary>\n\n";
            for q in &m.queries {
                out += &format!("### {}\n\n", q.label);
                if !q.result.is_empty() {
                    out += &format!("```text\n{}\n```\n\n", q.result.replace("```", "ʼʼʼ"));
                }
            }
            out += "</details>\n\n";
        }
        out += &m.content;
        out += "\n\n";
        if !m.reasoning.is_empty() {
            out += &format!("<details>\n<summary>思考过程</summary>\n\n{}\n\n</details>\n\n", m.reasoning);
        }
        if !m.notes.is_empty() {
            out += "<details>\n<summary>各段阅读要点</summary>\n\n";
            for n in &m.notes {
                out += &format!("### {}\n\n{}\n\n", n.label, n.content);
            }
            out += "</details>\n\n";
        }
    }
    let relevant: Vec<&SummaryRecord> = sums.iter().filter(|x| all || msgs.iter().any(|m| m.id == x.through_message_id)).collect();
    if !relevant.is_empty() {
        out += "## 上下文摘要\n\n";
        for x in relevant {
            if all {
                out += &format!("截至消息：`{}`\n\n", x.through_message_id);
            }
            out += &x.content;
            out += "\n\n";
        }
    }
    out
}

fn shown(conn: &Connection, id: i64, all: &[Node]) -> Result<Vec<Node>, String> {
    let path: Vec<i64> = chat::path(conn, id)?.iter().map(|m| m.id).collect();
    Ok(path.iter().filter_map(|pid| all.iter().find(|n| n.id == *pid).cloned()).collect())
}

/// The shown path as a Markdown file.
pub fn markdown(conn: &Connection, id: i64) -> Result<String, String> {
    let all = all_messages(conn, id)?;
    Ok(document(&title(conn, id)?, &shown(conn, id, &all)?, false, &summaries(conn, id)?, &HashMap::new()))
}

pub fn export_markdown(conn: &Connection, id: i64, path: &Path) -> Result<(), String> {
    std::fs::write(path, markdown(conn, id)?).map_err(|e| format!("没能写入文件：{e}"))
}

/// A file name that fits in a folder: no slashes, colons or leading dots; " (2)" for repeats.
fn unique_name(name: &str, taken: &mut Vec<String>) -> String {
    let clean: String = name.chars().map(|c| if c == '/' || c == ':' || c.is_control() { '-' } else { c }).collect();
    let clean = clean.trim_start_matches('.').trim().to_string();
    let clean = if clean.is_empty() { "文件".to_string() } else { clean };
    let (stem, ext) = match clean.rfind('.') {
        Some(i) if i > 0 => (clean[..i].to_string(), clean[i..].to_string()),
        _ => (clean.clone(), String::new()),
    };
    let mut n = 1;
    let mut out = clean.clone();
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&out)) {
        n += 1;
        out = format!("{stem} ({n}){ext}");
    }
    taken.push(out.clone());
    out
}

pub fn export_package(conn: &Connection, id: i64, dest: &Path, files_dir: &Path) -> Result<PathBuf, String> {
    if dest.exists() {
        return Err("这个位置已经有同名的文件或文件夹了，换个名字吧".into());
    }
    let parent = dest.parent().ok_or("选的位置不对")?;
    let t = title(conn, id)?;
    let all = all_messages(conn, id)?;
    let path = shown(conn, id, &all)?;
    let sums = summaries(conn, id)?;
    let mut st = conn
        .prepare("SELECT id, kind, model, model_name, state, error, prompt_tokens, completion_tokens, started_at, ended_at FROM runs WHERE session_id = ?1 ORDER BY started_at, id")
        .map_err(err)?;
    let runs: Vec<RunRecord> = st
        .query_map(params![id], |r| {
            Ok(RunRecord {
                id: r.get(0)?,
                kind: r.get(1)?,
                model: r.get(2)?,
                model_name: r.get(3)?,
                state: r.get(4)?,
                error: r.get(5)?,
                prompt_tokens: r.get(6)?,
                completion_tokens: r.get(7)?,
                started_at: r.get(8)?,
                ended_at: r.get(9)?,
                passages: vec![],
                memories: vec![],
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut runs = runs;
    for r in runs.iter_mut() {
        r.passages = reading::passages_of(conn, r.id)?;
        r.memories = super::memory::used_of(conn, r.id)?;
    }
    // every file the conversation used, once, with its copy's path in the package
    let mut copies: Vec<(PathBuf, String)> = vec![];
    let mut links: HashMap<i64, String> = HashMap::new();
    let mut taken: Vec<String> = vec![];
    for f in all.iter().flat_map(|n| n.files.iter()) {
        if f.file.deleted || links.contains_key(&f.file.id) {
            continue;
        }
        if let Ok(src) = files::path_of(conn, files_dir, f.file.id) {
            let name = unique_name(&f.file.name, &mut taken);
            links.insert(f.file.id, format!("附件/{name}"));
            copies.push((src, name));
        }
    }
    let record = serde_json::json!({
        "format": FORMAT,
        "title": t,
        "sessionId": id,
        "exportedAt": conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |r| r.get::<_, String>(0)).map_err(err)?,
        "activeMessageIds": path.iter().map(|m| m.id).collect::<Vec<_>>(),
        "messages": all,
        "runs": runs,
        "summaries": sums,
    });
    let mut salt = [0u8; 6];
    let _ = getrandom::fill(&mut salt);
    let stage = parent.join(format!(".pomelo-export-{}", salt.iter().map(|b| format!("{b:02x}")).collect::<String>()));
    let write = || -> std::io::Result<()> {
        std::fs::create_dir(&stage)?;
        if !copies.is_empty() {
            std::fs::create_dir(stage.join("附件"))?;
            for (src, name) in &copies {
                std::fs::copy(src, stage.join("附件").join(name))?;
            }
        }
        std::fs::write(stage.join("对话.md"), document(&t, &path, false, &sums, &links))?;
        std::fs::write(stage.join("全部版本.md"), document(&format!("{t} · 全部版本"), &all, true, &sums, &links))?;
        std::fs::write(stage.join("完整记录.json"), serde_json::to_string_pretty(&record).unwrap_or_default())?;
        std::fs::rename(&stage, dest)
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(format!("没能导出：{e}"));
    }
    Ok(dest.to_path_buf())
}

// ---------- commands ----------

#[tauri::command]
pub fn picky_copy_conversation(state: State<AppState>, id: i64) -> Result<(), String> {
    let text = state.picky.with_db(|c| copy_markdown(c, id))?;
    crate::secure::copy_plain(&text);
    Ok(())
}

#[tauri::command]
pub fn picky_export_markdown(state: State<AppState>, id: i64, path: String) -> Result<(), String> {
    state.picky.with_db(|c| export_markdown(c, id, Path::new(&path)))
}

#[tauri::command]
pub fn picky_export_package(state: State<AppState>, id: i64, path: String) -> Result<String, String> {
    let dir = state.picky.files_dir().to_path_buf();
    state.picky.with_db(|c| export_package(c, id, Path::new(&path), &dir)).map(|p| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::chat::{begin_run, begin_version, finish, Outcome};
    use crate::picky::store::tests::memory_db;

    #[test]
    fn markdown_and_package_follow_the_shown_path_and_keep_every_version() {
        let c = memory_db();
        let b = begin_run(&c, None, None, "什么是向量？", "local:a", "Qwen3 14B").unwrap();
        finish(&c, b.run_id, b.reply_id, &Outcome { content: "一组数。".into(), reasoning: "想想".into(), state: "completed", ..Default::default() }).unwrap();
        let v = begin_version(&c, b.reply_id, None, "cloud:x", "DeepSeek Flash", &[], &[]).unwrap();
        finish(&c, v.run_id, v.reply_id, &Outcome { content: "有方向的量。".into(), state: "cancelled", ..Default::default() }).unwrap();

        let run = crate::picky::context::begin_summary(&c, b.session_id, "local:a", "A").unwrap();
        crate::picky::context::finish_summary(&c, run, b.session_id, b.reply_id, "completed", Some("聊了向量"), None, (1, 1), 8192).unwrap();
        let copy = copy_markdown(&c, b.session_id).unwrap();
        assert_eq!(copy, "# 什么是向量？\n\n## 我\n\n什么是向量？\n\n## 小拾\n\n状态：已停止\n\n有方向的量。\n");

        let md = markdown(&c, b.session_id).unwrap();
        assert!(md.contains("## 2. 小拾 · DeepSeek Flash · 版本 2/2") && !md.contains("一组数。"), "{md}");
        assert!(!md.contains("上下文摘要")); // its message isn't on the shown path

        let dir = crate::db::tests::temp_dir("picky-export");
        let dest = dir.join("向量");
        export_package(&c, b.session_id, &dest, &dir.join("files")).unwrap();
        let all = std::fs::read_to_string(dest.join("全部版本.md")).unwrap();
        assert!(all.contains("一组数。") && all.contains("有方向的量。") && all.contains("<summary>思考过程</summary>"));
        assert!(all.contains("## 上下文摘要\n\n截至消息：") && all.contains("聊了向量"));
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dest.join("完整记录.json")).unwrap()).unwrap();
        assert_eq!(json["format"], FORMAT);
        assert_eq!(json["messages"].as_array().unwrap().len(), 3);
        assert_eq!(json["activeMessageIds"].as_array().unwrap().len(), 2);
        assert_eq!(json["runs"].as_array().unwrap().len(), 3);
        assert_eq!(json["summaries"][0]["content"], "聊了向量");
        // never merged into an existing folder; no staging folder left behind
        assert!(export_package(&c, b.session_id, &dest, &dir.join("files")).is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;
    use crate::picky::chat::{begin_run_in, finish, Extras, NewSession, Outcome};
    use crate::picky::files::tests::add;
    use crate::picky::store::tests::memory_db;

    #[test]
    fn packages_carry_copies_of_used_files_and_notes() {
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("picky-export-files");
        let a = add(&c, &dir, "初稿:v1.md", b"# A\nhello");
        let gone = add(&c, &dir, "旧.txt", b"old");
        let extras = Extras { attachments: vec![a, gone], ..Default::default() };
        let b = begin_run_in(&c, None, &NewSession::default(), "看看", "local:x", "X", &extras).unwrap();
        crate::picky::reading::save_note(&c, b.run_id, a, 0, "要点一").unwrap();
        finish(&c, b.run_id, b.reply_id, &Outcome { content: "好".into(), state: "completed", ..Default::default() }).unwrap();
        c.execute("UPDATE artifacts SET deleted_at = 'x' WHERE id = ?1", [gone]).unwrap();
        assert!(copy_markdown(&c, b.session_id).unwrap().contains("附件：初稿:v1.md"));
        let dest = dir.join("包");
        export_package(&c, b.session_id, &dest, &dir.join("files")).unwrap();
        let md = std::fs::read_to_string(dest.join("对话.md")).unwrap();
        assert!(md.contains("附件：[初稿:v1.md](<附件/初稿-v1.md>)"), "{md}");
        assert!(md.contains("附件：旧.txt（文件已删除）") && md.contains("<summary>各段阅读要点</summary>") && md.contains("要点一"));
        assert_eq!(std::fs::read_dir(dest.join("附件")).unwrap().count(), 1);
        let mut taken = vec![];
        assert_eq!((unique_name("x.md", &mut taken), unique_name("X.md", &mut taken), unique_name(".hidden", &mut taken)), ("x.md".into(), "X (2).md".into(), "hidden".into()));
    }
}
