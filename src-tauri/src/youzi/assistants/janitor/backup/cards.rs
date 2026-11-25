//! The Agent's backup cards (1.1.0 Phase 2; requirements「DevJanitor › 备份」), beside its trash / command /
//! rule ones in `janitor_cards` (agent.rs `Card`), each for the drive it was proposed on (`drive`: its UUID):
//! - `move` (`MOVE`): renames or moves on the drive (paths relative to its root): never copies, never deletes;
//!   a task whose drive folder moves follows it; folders a move empties go (a lone .DS_Store counts as empty).
//! - `drive_trash` (`TRASH`): to the drive's own Trash (sandbox: the fake drive's `.Trashes/501`); never a
//!   task's drive folder or one holding it.
//! - `task` (`TASK`): a new task or a changed one, checked like the page's (`save_task`), with what the two
//!   folders compare to now.
//! Rust checks a proposal before it's a card (paths there, VAULT and 旧版 refused) and again when I click.
//! None of it is undone (requirements: 跟着改名、任务的设置、去重都不进撤销).

use super::super::agent::{self, Card, TrashItem};
use super::super::{rules, Janitor, Mode};
use super::{compare_task, copy, diff, drive_of, prune_empty, save_task, Settings, TaskInput};
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use tauri::State;

pub const MOVE: &str = "pomelo_janitor_backup_move";
pub const TRASH: &str = "pomelo_janitor_backup_trash";
pub const TASK: &str = "pomelo_janitor_backup_task";
const ITEMS_MAX: usize = 100;

pub fn is_tool(name: &str) -> bool {
    [MOVE, TRASH, TASK].contains(&name)
}

fn card_kind_moves(kind: &str) -> bool {
    kind == "move" || kind == "drive_trash"
}

pub fn is_kind(kind: &str) -> bool {
    ["move", "drive_trash", "task"].contains(&kind)
}

/// A task card's task, and what its folders compare to when proposed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskPlan {
    /// The task it changes (None: a new one).
    pub id: Option<i64>,
    pub source: String,
    pub target: String,
    pub why: String,
    /// Compared now (None: the drive wasn't in).
    pub same: Option<u64>,
    pub new: u64,
    pub changed: u64,
    pub moved: u64,
    pub archived: u64,
    /// The task as it was (a change).
    pub was: Option<(String, String)>,
}

/// A drive path from the model (relative, or the drive's real path in front) → relative, checked.
fn drive_rel(drive: Option<&super::Volume>, p: &str) -> Result<String, String> {
    let mut p = p.trim().trim_matches(['"', '\'']).to_string();
    if let Some(v) = drive {
        let root = v.path.to_string_lossy().into_owned();
        for pre in [root, format!("/Volumes/{}", v.name)] {
            if let Some(r) = p.strip_prefix(&pre).filter(|r| r.is_empty() || r.starts_with('/')) {
                p = r.to_string();
                break;
            }
        }
    }
    let rel = p.trim_matches('/').to_string();
    if rel.is_empty() {
        return Err("不能是整块硬盘".into());
    }
    for part in rel.split('/') {
        if part.is_empty() || part == ".." || part == "." || part.starts_with('.') {
            return Err(format!("「{rel}」不是硬盘上的路径（相对硬盘根目录写，不带 . 和 ..）"));
        }
        if rules::sealed_name(part) {
            return Err("碰到了不能动的地方，不提议".into());
        }
    }
    if rel.split('/').next() == Some(copy::OLD) {
        return Err(format!("「{}」里是被覆盖的旧版本，由用户自己清，不提议", copy::OLD));
    }
    Ok(rel)
}

/// Bytes under a drive path (a file: its size).
fn size_at(real: &Path) -> u64 {
    match std::fs::symlink_metadata(real) {
        Ok(m) if m.is_dir() => diff::list(real, &diff::Skip::default(), &AtomicBool::new(false)).map(|l| l.files.values().map(|(_, x)| x.size).sum()).unwrap_or(0),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

/// The task whose drive folder is `rel` or inside it (moving it carries the task along; trashing it can't be).
fn task_at(s: &Settings, rel: &str) -> Option<i64> {
    let rel = diff::nfc(rel);
    s.tasks.iter().find(|t| rules::within(&diff::nfc(&t.target), &rel)).map(|t| t.id)
}

/// The task whose drive folder holds `rel`.
fn task_over(s: &Settings, rel: &str) -> Option<i64> {
    let rel = diff::nfc(rel);
    s.tasks.iter().find(|t| rules::within(&rel, &diff::nfc(&t.target)) && rel != diff::nfc(&t.target)).map(|t| t.id)
}

fn text(v: &Value, k: &str, max: usize) -> String {
    v[k].as_str().unwrap_or("").trim().chars().take(max).collect()
}

/// A backup proposal checked into a card.
pub fn build(j: &Janitor, s: &Settings, name: &str, args: &str) -> Result<Card, String> {
    if rules::mentions_sealed(args) {
        return Err("碰到了不能动的地方，不提议".into());
    }
    let v: Value = serde_json::from_str(args).map_err(|_| "参数不是 JSON".to_string())?;
    let drive = drive_of(j, s);
    let uuid = s.drive.as_ref().map(|d| d.uuid.clone());
    if s.drive.is_none() {
        return Err("还没选移动硬盘（用户在「备份」页选）".into());
    }
    match name {
        TASK => {
            let id = v["id"].as_i64();
            let was = match id {
                Some(i) => Some(s.tasks.iter().find(|t| t.id == i).ok_or(format!("没有任务 #{i}"))?.clone()),
                None => None,
            };
            let source = agent::virt_of(&j.home, v["source"].as_str().unwrap_or(""))?;
            let input = TaskInput { id, source, target: text(&v, "target", 300), excludes: was.as_ref().map(|t| t.excludes.clone()).unwrap_or_default() };
            let after = save_task(j, s.clone(), input)?;
            let t = match id {
                Some(i) => after.tasks.iter().find(|t| t.id == i),
                None => after.tasks.last(),
            }
            .cloned()
            .ok_or("任务没存成")?;
            if was.as_ref().is_some_and(|w| w.source == t.source && w.target == t.target) {
                return Err(format!("任务 #{} 已经是这样了", t.id));
            }
            let mut plan = TaskPlan { id, source: t.source.clone(), target: t.target.clone(), why: text(&v, "why", 300), was: was.map(|w| (w.source, w.target)), ..Default::default() };
            if let Some(dv) = &drive {
                let (d, _) = compare_task(j, dv, &t, &AtomicBool::new(false))?;
                plan.same = Some(d.same);
                (plan.new, plan.changed, plan.moved, plan.archived) = (d.new.len() as u64, d.changed.len() as u64, d.moved.len() as u64, (d.archived.len() + d.recent.len()) as u64);
            }
            let title = match id {
                Some(i) => format!("改任务 #{i}"),
                None => "新任务".into(),
            };
            Ok(Card { kind: "task".into(), title, task: Some(plan), drive: uuid, ..Default::default() })
        }
        MOVE | TRASH => {
            let dv = drive.ok_or("移动硬盘没插上：插上了再提议")?;
            let mut items: Vec<TrashItem> = Vec::new();
            for it in v["items"].as_array().cloned().unwrap_or_default().iter().take(ITEMS_MAX) {
                let path = drive_rel(Some(&dv), it[if name == MOVE { "from" } else { "path" }].as_str().unwrap_or(""))?;
                let real = dv.path.join(&path);
                let Ok(m) = std::fs::symlink_metadata(&real) else { return Err(format!("硬盘上没有 {path}（先用 pomelo_janitor_backup 的 drive 看清楚路径）")) };
                if items.iter().any(|x| x.path == path) {
                    continue;
                }
                let mut item = TrashItem { size: size_at(&real), kind: if m.is_dir() { "文件夹".into() } else { "文件".into() }, why: text(it, "why", 200), ..Default::default() };
                if name == MOVE {
                    let to = drive_rel(Some(&dv), it["to"].as_str().unwrap_or(""))?;
                    if diff::nfc(&to) == diff::nfc(&path) || rules::within(&diff::nfc(&to), &diff::nfc(&path)) {
                        return Err(format!("{path} → {to}：不能挪进它自己里面"));
                    }
                    if std::fs::symlink_metadata(dv.path.join(&to)).is_ok() {
                        return Err(format!("硬盘上已经有 {to}：换个名字，或者只挪里面的文件"));
                    }
                    item.note = task_at(s, &path).map(|i| format!("任务 #{i} 的硬盘文件夹会跟着改"));
                    item.to = Some(to);
                } else {
                    if let Some(i) = task_at(s, &path) {
                        return Err(format!("{path} 是任务 #{i} 的硬盘文件夹（或包着它），不能移到废纸篓"));
                    }
                    item.note = task_over(s, &path).map(|i| format!("在任务 #{i} 的硬盘文件夹里：电脑上还有的话，下次备份会再复制"));
                }
                item.path = path;
                items.push(item);
            }
            if items.is_empty() {
                return Err("items 是空的".into());
            }
            let (kind, title) = if name == MOVE { ("move", "在硬盘上跟着改") } else { ("drive_trash", "硬盘里重复的") };
            Ok(Card { kind: kind.into(), title: Some(text(&v, "title", 40)).filter(|t| !t.is_empty()).unwrap_or_else(|| title.into()), items, drive: uuid, ..Default::default() })
        }
        _ => Err("没有这个工具".into()),
    }
}

/// A free name in `dir` for `name` (`name 2`, `name 3`…).
fn free_in(dir: &Path, name: &str) -> std::path::PathBuf {
    let mut to = dir.join(name);
    let mut n = 2;
    while std::fs::symlink_metadata(&to).is_ok() {
        to = dir.join(format!("{name} {n}"));
        n += 1;
    }
    to
}

/// To the drive's own Trash.
fn to_drive_trash(j: &Janitor, root: &Path, real: &Path) -> Result<(), String> {
    match j.mode {
        Mode::Real => crate::youzi::assistants::pub2md::move_to_trash(real),
        Mode::Sandbox => {
            let bin = root.join(".Trashes").join("501");
            std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
            let name = real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            std::fs::rename(real, free_in(&bin, &name)).map_err(|e| e.to_string())
        }
    }
}

/// Does a card: (state, result, the settings when a task changed).
pub fn run(j: &Janitor, s: &Settings, c: &Card, picked: &[usize]) -> Result<(&'static str, Value, Option<Settings>), String> {
    if !is_kind(&c.kind) || c.state != "pending" {
        return Err("这张卡片已经处理过了".into());
    }
    if c.drive.as_ref() != s.drive.as_ref().map(|d| &d.uuid) {
        return Err("这张卡片是给另一块移动硬盘的".into());
    }
    let dv = drive_of(j, s).ok_or("移动硬盘没插上")?;
    if c.kind == "task" {
        let p = c.task.as_ref().ok_or("卡片里没有任务")?;
        let excludes = p.id.and_then(|i| s.tasks.iter().find(|t| t.id == i)).map(|t| t.excludes.clone()).unwrap_or_default();
        return Ok(match save_task(j, s.clone(), TaskInput { id: p.id, source: p.source.clone(), target: p.target.clone(), excludes }) {
            Ok(after) => ("done", json!({}), Some(after)),
            Err(e) => ("failed", json!({ "error": e }), None),
        });
    }
    if picked.is_empty() {
        return Err("至少勾上一项".into());
    }
    let mut after = s.clone();
    let mut moved_task = false;
    let mut results = Vec::new();
    for (k, it) in c.items.iter().enumerate() {
        let r: Result<(), String> = if !picked.contains(&k) {
            Err("没勾".into())
        } else {
            (|| {
                let path = drive_rel(Some(&dv), &it.path)?;
                let real = dv.path.join(&path);
                if std::fs::symlink_metadata(&real).is_err() {
                    return Err("硬盘上已经没有了".into());
                }
                if c.kind == "move" {
                    let to = drive_rel(Some(&dv), it.to.as_deref().unwrap_or(""))?;
                    let dest = dv.path.join(&to);
                    if std::fs::symlink_metadata(&dest).is_ok() {
                        return Err("新位置上已经有东西".into());
                    }
                    std::fs::create_dir_all(dest.parent().ok_or("没有上一层")?).map_err(|e| e.to_string())?;
                    std::fs::rename(&real, &dest).map_err(|e| e.to_string())?;
                    prune_empty(&dv.path, real.parent());
                    let (from, to) = (diff::nfc(&path), diff::nfc(&to));
                    for t in after.tasks.iter_mut() {
                        let tt = diff::nfc(&t.target);
                        if rules::within(&tt, &from) {
                            t.target = format!("{to}{}", &tt[from.len()..]);
                            moved_task = true;
                        }
                    }
                    Ok(())
                } else {
                    if let Some(i) = task_at(&after, &path) {
                        return Err(format!("是任务 #{i} 的硬盘文件夹"));
                    }
                    to_drive_trash(j, &dv.path, &real)
                }
            })()
        };
        results.push(json!({ "done": r.is_ok(), "error": r.err().filter(|e| e != "没勾") }));
    }
    let done = results.iter().filter(|r| r["done"] == true).count();
    Ok((if done > 0 { "done" } else { "failed" }, json!({ "items": results }), moved_task.then_some(after)))
}

/// 在硬盘上改 / 移到硬盘的废纸篓 / 建任务: the picked items of a backup card (a task card: all of it).
#[tauri::command]
pub async fn janitor_card_backup(app: tauri::AppHandle, state: State<'_, AppState>, janitor: State<'_, Janitor>, id: i64, picked: Vec<usize>) -> Result<Card, String> {
    if janitor.backup.running() {
        return Err("正在备份，等它做完或先停下".into());
    }
    super::ensure_fake(&janitor)?;
    let card = state.youzi.with_db(|c| agent::card(c, id))?;
    let card_kind = card.kind.clone();
    let s = super::load(&state);
    let j = janitor.inner().clone();
    let (st, result, after) = tauri::async_runtime::spawn_blocking(move || run(&j, &s, &card, &picked)).await.map_err(|e| e.to_string())??;
    if let Some(after) = &after {
        super::store_settings(&state, after)?;
    }
    state.youzi.with_db(|c| agent::set_state(c, id, st, Some(&result)))?;
    super::compare_in_background(&janitor, super::load(&state));
    // the drive changed: PikPak follows
    if st == "done" && card_kind_moves(&card_kind) {
        super::pikpak::kick(&app);
    }
    state.youzi.with_db(|c| agent::card(c, id))
}
