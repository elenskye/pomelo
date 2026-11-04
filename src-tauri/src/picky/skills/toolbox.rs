//! Word 工具箱 (`pomelo_word`) and 文本工具箱 (`pomelo_text`) (v0.4.5 improve-10): 改文件 as plugin
//! skills that 柚子 uses when it decides to. A toolbox changes only files the question has an original
//! of — its attachments added from disk (`artifacts.source_path`), the file an older question picked to
//! change — and the 知识库's files (in place); never any other path.
//! - view=files: what may be changed (also in the system message: `note`). read: the numbered
//!   paragraphs (Word with its formatting marks and tables as rows, word.rs; 「不能改」 marked), ending
//!   with a reminder that nothing is proposed yet (models said 「放进卡片了」 after only reading). edit: changes into the reply's card (work.rs `file_edits`, one file per
//!   reply, checked the same way), written only when the user ticks them and confirms. create (Word):
//!   a new document's name and Markdown, saved only when the user confirms (and picks the folder).
//! - Excel 工具箱 (`pomelo_excel`, v0.8; the file's own logic in excel.rs): the same files, .xlsx only. read: the
//!   rows with addressed cells; edit: cells, added and deleted rows into the card, each checked first; create: a
//!   new workbook. Reading a big workbook happens outside picky.db's lock (`excel_plan`, then `excel_commit`).
//! - Nothing here writes a file. 定时任务 get the toolboxes too (v0.8): their cards wait in the conversation they
//!   wrote, for the user to tick (chat.rs).

use crate::picky::data::Answer;
use crate::picky::excel::{self, Book, NewSheet, Planned};
use crate::picky::knowledge::{self, Scope};
use crate::picky::reading::{Kind, Source};
use crate::picky::{context, files, work};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::Path;

pub const WORD_TOOL: &str = "pomelo_word";
pub const TEXT_TOOL: &str = "pomelo_text";
pub const EXCEL_TOOL: &str = "pomelo_excel";
/// A workbook bigger than this isn't read.
const MAX_EXCEL: u64 = 50 * 1024 * 1024;

pub fn is_tool(name: &str) -> bool {
    name == WORD_TOOL || name == TEXT_TOOL || name == EXCEL_TOOL
}

/// What the offered toolboxes may change, for the system message (no view=files round needed).
pub fn note(tools: &[Value], word: &[Editable], text: &[Editable]) -> String {
    let has = |n: &str| tools.iter().any(|t| t["function"]["name"].as_str() == Some(n));
    let list = |files: &[Editable]| files.iter().map(|f| format!("{}（{}）", f.name, f.from)).collect::<Vec<_>>().join("、");
    let mut out = vec![];
    if has(WORD_TOOL) {
        out.push(if word.is_empty() { "- 这个问题没有能改的 Word 文件（Word 工具箱只能 create 新建）。".to_string() } else { format!("- 能改的 Word（file 写这里的名字）：{}", list(word)) });
    }
    if has(TEXT_TOOL) && !text.is_empty() {
        out.push(format!("- 能改的 Markdown / 纯文本（file 写这里的名字）：{}", list(text)));
    }
    out.join("\n")
}

/// What Excel 工具箱 may change, for the system message.
pub fn excel_note(tools: &[Value], excel_files: &[Editable]) -> String {
    if !tools.iter().any(|t| t["function"]["name"].as_str() == Some(EXCEL_TOOL)) {
        return String::new();
    }
    if excel_files.is_empty() {
        return "- 这个问题没有能改的 Excel 文件（Excel 工具箱只能 create 新建）。".into();
    }
    format!("- 能改的 Excel（file 写这里的名字）：{}", excel_files.iter().map(|f| format!("{}（{}）", f.name, f.from)).collect::<Vec<_>>().join("、"))
}

/// A file a toolbox may change: how it's called, where it is, where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Editable {
    pub name: String,
    pub path: String,
    pub from: String,
}

fn ext(p: &str) -> String {
    Path::new(p).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn fits(word: bool, p: &str) -> bool {
    let e = ext(p);
    if word { e == "docx" } else { matches!(e.as_str(), "md" | "markdown" | "txt" | "text") }
}

/// The files the question may change with a toolbox (`word`: Word, else Markdown / plain text).
pub fn editable(picky: &Connection, search: Option<&Connection>, sources: &[Source], legacy: Option<&work::Target>, kb: Option<&Scope>, word: bool) -> Vec<Editable> {
    files_that(picky, search, sources, legacy, kb, &|p| fits(word, p))
}

/// The workbooks the question may change with Excel 工具箱.
pub fn editable_excel(picky: &Connection, search: Option<&Connection>, sources: &[Source], kb: Option<&Scope>) -> Vec<Editable> {
    files_that(picky, search, sources, None, kb, &|p| ext(p) == "xlsx")
}

fn files_that(picky: &Connection, search: Option<&Connection>, sources: &[Source], legacy: Option<&work::Target>, kb: Option<&Scope>, fits: &dyn Fn(&str) -> bool) -> Vec<Editable> {
    let mut out: Vec<Editable> = vec![];
    let mut add = |name: String, path: String, from: String| {
        if fits(&path) && Path::new(&path).is_file() && !out.iter().any(|e| e.path == path) {
            out.push(Editable { name, path, from });
        }
    };
    let mut seen: Vec<i64> = vec![];
    for s in sources.iter().filter(|s| s.kind == Kind::Attachment && s.artifact > 0) {
        if seen.contains(&s.artifact) {
            continue;
        }
        seen.push(s.artifact);
        if let Ok(Some(p)) = files::source_of(picky, s.artifact) {
            let name = Path::new(&p).file_name().map_or_else(|| p.clone(), |n| n.to_string_lossy().to_string());
            add(name, p, "附件".into());
        }
    }
    if let Some(t) = legacy {
        add(t.name.clone(), t.path.clone(), "要改的文件".into());
    }
    if let (Some(search), Some(scope)) = (search, kb) {
        for l in knowledge::listed(search, scope).unwrap_or_default().into_iter().filter(|l| l.source.kind == "folder" && l.error.is_none()) {
            if let Ok(full) = knowledge::file_path(&l.source, &l.path) {
                add(l.shown.clone(), full.display().to_string(), format!("知识库 · {}", l.library));
            }
        }
    }
    out
}

fn failed(what: &str, label: &str, why: &str) -> Answer {
    Answer { module: None, label: format!("{what} · {label}"), state: "failed", text: format!("没成：{why}"), records: vec![] }
}

/// The file `want` names: its name, its path, or a part of its name (one).
fn pick<'a>(files: &'a [Editable], want: &str) -> Result<&'a Editable, String> {
    let want = want.trim();
    if files.is_empty() {
        return Err("这个问题没有能改的文件（附件要从访达拖进来或点选；知识库里的也行）".into());
    }
    if want.is_empty() {
        return if files.len() == 1 { Ok(&files[0]) } else { Err(format!("file 写文件名（有：{}）", files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join("、"))) };
    }
    if let Some(f) = files.iter().find(|f| f.name == want || f.path == want) {
        return Ok(f);
    }
    let base = |f: &Editable| f.name.rsplit('/').next().unwrap_or(&f.name).to_string();
    let found: Vec<&Editable> = files.iter().filter(|f| base(f) == want || f.name.contains(want)).collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("没有「{want}」（能改的：{}）", files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join("、"))),
        many => Err(format!("有 {} 个文件对得上，file 写完整的名字：{}", many.len(), many.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join("、"))),
    }
}

/// A paragraph as the model sees it.
fn shown(doc: &work::Doc, i: usize) -> String {
    let (text, fixed) = &doc.items[i];
    match fixed {
        Some(why) => format!("【{}】（{why}，不能改）{}", i + 1, text),
        None if work::is_table(doc, i) => format!("【{}】（表格：一行一行，格子用 | 隔开；只能改格子里的字）\n{}", i + 1, text),
        None => format!("【{}】\n{}", i + 1, text),
    }
}

/// A Word paragraph without line breaks gets none (a blank line still starts a new paragraph).
fn normalized(doc: &work::Doc, i: usize, before: &str, after: &str) -> String {
    if doc.kind == "word" && !before.contains('\n') && !work::is_table(doc, i) {
        after.split("\n\n").map(|p| p.trim_matches('\n').replace('\n', " ")).collect::<Vec<_>>().join("\n\n")
    } else {
        after.trim_end().to_string()
    }
}

/// Runs a toolbox call; true when the reply's card changed (to send it to the page).
pub fn run(conn: &Connection, run_id: i64, files: &[Editable], word: bool, arguments: &str, budget: i64) -> (Answer, bool) {
    let what = if word { "Word 工具箱" } else { "文本工具箱" };
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let est = context::estimate;
    let budget = budget.min(crate::picky::data::MAX_RESULT * 2);
    let view = args["view"].as_str().unwrap_or("files");
    if view == "files" {
        if files.is_empty() {
            return (Answer { module: None, label: format!("{what} · 没有能改的文件"), state: "ok", text: "这个问题没有能改的文件：附件要从访达拖进来或点选（粘贴的截图、附件库里选的没有原文件），知识库里的文件也能改。".into(), records: vec![] }, false);
        }
        let lines: Vec<String> = files.iter().map(|f| format!("- {}（{}）", f.name, f.from)).collect();
        return (Answer { module: None, label: format!("{what} · 能改的文件（{} 个）", files.len()), state: "ok", text: format!("能改的文件：\n{}", lines.join("\n")), records: vec![] }, false);
    }
    if view == "create" {
        if !word {
            return (failed(what, "没新建", "文本工具箱不能新建文件"), false);
        }
        let name = work::clean_name(args["name"].as_str().unwrap_or("").trim().trim_end_matches(".docx"));
        let content = args["content"].as_str().unwrap_or("").trim().to_string();
        if content.is_empty() {
            return (failed(what, "没新建", "content 写这份文档的全文（Markdown）"), false);
        }
        return match work::propose_new_word(conn, run_id, &name, &content) {
            Ok(folder) => (
                Answer { module: None, label: format!("{what} · 提议新建《{name}.docx》"), state: "ok", text: format!("已放进确认卡片：新建《{name}.docx》，存到 {folder}。还没存：用户点「保存」才存（可以换文件夹），回答里不要说已经存好了。"), records: vec![] },
                true,
            ),
            Err(e) => (failed(what, "没新建", &e), false),
        };
    }
    let f = match pick(files, args["file"].as_str().unwrap_or("")) {
        Ok(f) => f,
        Err(e) => return (failed(what, "没找到文件", &e), false),
    };
    // a kind it handles, a regular file, not too big
    let doc = match work::target(&f.path).and_then(|_| work::read(Path::new(&f.path))) {
        Ok(d) => d,
        Err(e) => return (failed(what, &format!("《{}》", f.name), &e), false),
    };
    let total = doc.items.len();
    match view {
        "edit" => {
            let changes: Vec<(usize, String)> = args["changes"]
                .as_array()
                .map(|xs| {
                    xs.iter()
                        .filter_map(|c| {
                            let n = c["paragraph"].as_u64().or_else(|| c["paragraph"].as_str().and_then(|s| s.trim().parse().ok()))?;
                            Some((n as usize, c["text"].as_str().unwrap_or("").to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if changes.is_empty() {
                return (failed(what, "没提议", "changes 写每段的段号和改好的整段文字"), false);
            }
            let target = work::Target { path: f.path.clone(), name: f.name.clone() };
            let (mut kept, mut refused, mut same) = (vec![], vec![], 0);
            for (n, after) in changes {
                let Some(i) = n.checked_sub(1).filter(|i| *i < total) else {
                    refused.push(format!("第 {n} 段：没有这一段（共 {total} 段）"));
                    continue;
                };
                let (before, _) = &doc.items[i];
                let after = normalized(&doc, i, before, &after);
                if after == before.trim_end() {
                    same += 1;
                    continue;
                }
                match work::check(&doc, i, &after).and_then(|_| work::propose_change(conn, run_id, &target, &doc, i, before, &after)) {
                    Ok(()) => kept.push(if after.trim().is_empty() {
                        format!("{n}（整段删掉，不留空段）")
                    } else {
                        match after.split("\n\n").filter(|p| !p.trim().is_empty()).count() {
                            0 | 1 => n.to_string(),
                            k => format!("{n}（后面加 {} 段）", k - 1),
                        }
                    }),
                    Err(e) => refused.push(format!("第 {n} 段：{e}")),
                }
            }
            let mut lines = vec![];
            if !kept.is_empty() {
                lines.push(format!("已放进确认卡片：《{}》第 {} 段。还没写进文件：用户勾选后点「写入文件」才写，回答里不要说已经改好了。", f.name, kept.join("、")));
            }
            if same > 0 {
                lines.push(format!("{same} 段和原文一样，没放进去。"));
            }
            if !refused.is_empty() {
                lines.push(format!("没放进去的：\n{}", refused.join("\n")));
            }
            let state = if kept.is_empty() { "failed" } else { "ok" };
            (Answer { module: None, label: format!("{what} · 提议改《{}》{} 段", f.name, kept.len()), state, text: lines.join("\n"), records: vec![] }, !kept.is_empty())
        }
        _ => {
            let num = |k: &str| args[k].as_u64().or_else(|| args[k].as_str().and_then(|s| s.trim().parse().ok())).map(|n| n as usize);
            let from = num("from").unwrap_or(1).max(1);
            if from > total {
                return (failed(what, &format!("《{}》", f.name), &format!("只有 {total} 段")), false);
            }
            let to = num("to").unwrap_or(total).clamp(from, total);
            let mut lines = vec![];
            let mut used = 0;
            let mut last = from - 1;
            for i in from - 1..to {
                let line = shown(&doc, i);
                if used + est(&line) > budget && i > from - 1 {
                    lines.push(format!("……第 {}–{to} 段放不下了，下次从 from={} 接着读", i + 1, i + 1));
                    break;
                }
                used += est(&line);
                lines.push(line);
                last = i + 1;
            }
            let head = format!("《{}》共 {total} 段，这是第 {from}–{last} 段：", f.name);
            let tail = "（这是原文，还没有改动，卡片里也还没有东西：要改就调用 view=edit，把改好的整段文字写进 changes；edit 返回「已放进确认卡片」才算提议了。）";
            (Answer { module: None, label: format!("{what} · 读《{}》第 {from}–{last} 段（共 {total} 段）", f.name), state: "ok", text: format!("{head}\n\n{}\n\n{tail}", lines.join("\n\n")), records: vec![] }, false)
        }
    }
}

// ---------- Excel 工具箱 ----------

/// What a `pomelo_excel` call came to, before touching picky.db.
#[derive(Debug, Clone)]
pub enum ExcelStep {
    Done(Answer),
    /// `notes`: what the model should know about them (a 合计 that leaves out added rows).
    Changes { target: work::Target, sha256: String, sheets: i64, planned: Vec<Planned>, refused: Vec<String>, notes: Vec<String> },
    New { name: String, sheets: Vec<NewSheet> },
}

fn excel_failed(label: &str, why: &str) -> ExcelStep {
    ExcelStep::Done(failed("Excel 工具箱", label, why))
}

/// A cell's value as the model wrote it (a number or true / false without quotes).
fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
        other => other.to_string(),
    }
}

fn rows_of(v: &Value) -> Vec<Vec<String>> {
    v.as_array().map(|rows| rows.iter().map(|r| r.as_array().map(|cells| cells.iter().map(value_text).collect()).unwrap_or_else(|| vec![value_text(r)])).collect()).unwrap_or_default()
}

/// Reads and checks: no database here.
pub fn excel_plan(files: &[Editable], arguments: &str, budget: i64) -> ExcelStep {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let budget = budget.min(crate::picky::data::MAX_RESULT * 2);
    let view = args["view"].as_str().unwrap_or("files");
    if view == "files" {
        if files.is_empty() {
            return ExcelStep::Done(Answer { module: None, label: "Excel 工具箱 · 没有能改的文件".into(), state: "ok", text: "这个问题没有能改的 Excel：附件要从访达拖进来或点选，知识库里的 .xlsx 也能改；只认 .xlsx。".into(), records: vec![] });
        }
        let lines: Vec<String> = files.iter().map(|f| format!("- {}（{}）", f.name, f.from)).collect();
        return ExcelStep::Done(Answer { module: None, label: format!("Excel 工具箱 · 能改的文件（{} 个）", files.len()), state: "ok", text: format!("能改的文件：\n{}", lines.join("\n")), records: vec![] });
    }
    if view == "create" {
        let name = work::clean_name(args["name"].as_str().unwrap_or("").trim().trim_end_matches(".xlsx"));
        let sheets: Vec<NewSheet> = args["sheets"].as_array().map(|xs| xs.iter().map(|s| NewSheet { name: s["name"].as_str().unwrap_or("").to_string(), rows: rows_of(&s["rows"]) }).collect()).unwrap_or_default();
        if sheets.iter().all(|s| s.rows.is_empty()) {
            return excel_failed("没新建", "sheets 写每张表的名字和行（rows：一行一个数组，第一行是表头）");
        }
        return ExcelStep::New { name, sheets: excel::clean_sheets(sheets) };
    }
    let f = match pick(files, args["file"].as_str().unwrap_or("")) {
        Ok(f) => f,
        Err(e) => return excel_failed("没找到文件", &e),
    };
    let label = format!("《{}》", f.name);
    if std::fs::metadata(&f.path).is_ok_and(|m| m.len() > MAX_EXCEL) {
        return excel_failed(&label, "文件太大了");
    }
    let bytes = match std::fs::read(&f.path) {
        Ok(b) => b,
        Err(_) => return excel_failed(&label, "读不了这个文件，可能已经移走或删除了"),
    };
    let book = match Book::open(&bytes) {
        Ok(b) => b,
        Err(e) => return excel_failed(&label, &e),
    };
    let text = |v: &Value| v.as_str().unwrap_or("").to_string();
    match view {
        "edit" => {
            let (mut planned, mut refused, mut notes) = (vec![], vec![], vec![]);
            // an item without its sheet: the one written outside them
            let sheet_of = |v: &Value| if v["sheet"].as_str().is_some_and(|x| !x.trim().is_empty()) { text(&v["sheet"]) } else { text(&args["sheet"]) };
            let mut keep = |what: String, p: Result<Planned, String>| match p {
                Ok(p) => planned.push(p),
                Err(e) => refused.push(format!("{what}：{e}")),
            };
            for c in args["changes"].as_array().into_iter().flatten() {
                let cell = text(&c["cell"]);
                keep(cell.clone(), book.plan_cell(&sheet_of(c), &cell, &value_text(&c["value"])));
            }
            let num = |v: &Value| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).map(|n| n as u32);
            let changed: Vec<String> = args["changes"].as_array().into_iter().flatten().map(|c| text(&c["cell"]).trim().to_uppercase()).collect();
            for a in args["add_rows"].as_array().into_iter().flatten() {
                match num(&a["after"]) {
                    Some(after) => {
                        let p = book.plan_add(&sheet_of(a), after, &rows_of(&a["rows"]));
                        if p.is_ok() {
                            let left: Vec<String> = book.ranges_ending_at(&sheet_of(a), after).into_iter().filter(|f| !changed.iter().any(|c| f.starts_with(&format!("{c}（")))).collect();
                            if !left.is_empty() {
                                notes.push(format!("{}的范围停在第 {after} 行：写入时会扩到新加的行，新行算进去，不用另外改这些公式（回答里可以告诉用户）", left.join("、")));
                            }
                        }
                        keep(format!("第 {after} 行后"), p)
                    }
                    None => keep("加行".into(), Err("after 写加在第几行后面（0 是最前面）".into())),
                }
            }
            for d in args["delete_rows"].as_array().into_iter().flatten() {
                let rows: Vec<u32> = d["rows"].as_array().map(|r| r.iter().filter_map(num).collect()).or_else(|| num(&d["row"]).map(|r| vec![r])).unwrap_or_default();
                if rows.is_empty() {
                    keep("删行".into(), Err("rows 写要删的行号".into()));
                }
                for row in rows {
                    keep(format!("第 {row} 行"), book.plan_delete(&sheet_of(d), row));
                }
            }
            if planned.is_empty() && refused.is_empty() {
                return excel_failed("没提议", "changes 写要改的格子，add_rows 写要加的行，delete_rows 写要删的行");
            }
            ExcelStep::Changes { target: work::Target { path: f.path.clone(), name: f.name.clone() }, sha256: crate::picky::pdf::sha256_hex(&bytes), sheets: book.sheet_count() as i64, planned, refused, notes }
        }
        _ => {
            let num = |k: &str| args[k].as_u64().or_else(|| args[k].as_str().and_then(|s| s.trim().parse().ok())).map(|n| n as u32);
            let sheet = text(&args["sheet"]);
            let first = if sheet.is_empty() { book.first_sheet().to_string() } else { sheet };
            let from = num("from").unwrap_or(1).max(1);
            let to = num("to").unwrap_or(u32::MAX).max(from);
            let keyword = text(&args["keyword"]);
            let (name, rows, stopped) = match book.read_rows(&first, from, to, &keyword, budget - 200) {
                Ok(x) => x,
                Err(e) => return excel_failed(&label, &e),
            };
            let merged = book.merged_text(&name).unwrap_or_default();
            let mut lines = vec![format!("《{}》的表：\n{}", f.name, book.outline()), format!("表「{name}」（每行：行号：列=值，空格子不写）：")];
            if !merged.is_empty() {
                lines.push(format!("合并的格子：{merged}（只改左上角那格）"));
            }
            lines.push(if rows.is_empty() { "（这个范围没有内容）".to_string() } else { rows });
            if let Some(next) = stopped {
                lines.push(format!("……后面的放不下了，下次从 from={next} 接着读"));
            }
            lines.push("（这是原文，还没有改动，卡片里也还没有东西：要改就调用 view=edit；edit 返回「已放进确认卡片」才算提议了。）".into());
            ExcelStep::Done(Answer { module: None, label: format!("Excel 工具箱 · 读《{}》表「{name}」", f.name), state: "ok", text: lines.join("\n"), records: vec![] })
        }
    }
}

/// Puts what `excel_plan` found into the reply's card; true when the card changed.
pub fn excel_commit(conn: &Connection, run_id: i64, step: ExcelStep) -> (Answer, bool) {
    let what = "Excel 工具箱";
    match step {
        ExcelStep::Done(a) => (a, false),
        ExcelStep::New { name, sheets } => match work::propose_new_excel(conn, run_id, &name, &sheets) {
            Ok(folder) => (Answer { module: None, label: format!("{what} · 提议新建《{name}.xlsx》"), state: "ok", text: format!("已放进确认卡片：新建《{name}.xlsx》，存到 {folder}。还没存：用户点「保存」才存（可以换文件夹），回答里不要说已经存好了。"), records: vec![] }, true),
            Err(e) => (failed(what, "没新建", &e), false),
        },
        ExcelStep::Changes { target, sha256, sheets, planned, mut refused, notes } => {
            let mut kept = vec![];
            for p in planned {
                match work::propose_excel(conn, run_id, &target, &sha256, sheets, &p) {
                    Ok(()) => kept.push(p.label),
                    Err(e) => refused.push(format!("{}：{e}", p.label)),
                }
            }
            let mut lines = vec![];
            if !kept.is_empty() {
                lines.push(format!("已放进确认卡片：《{}》{}。还没写进文件：用户勾选后点「写入文件」才写，回答里不要说已经改好了。公式的结果要等 Excel 打开文件时才重算。", target.name, kept.join("、")));
            }
            if !refused.is_empty() {
                lines.push(format!("没放进去的：\n{}", refused.join("\n")));
            }
            if !kept.is_empty() && !notes.is_empty() {
                lines.push(format!("注意：{}", notes.join("\n")));
            }
            let state = if kept.is_empty() { "failed" } else { "ok" };
            (Answer { module: None, label: format!("{what} · 提议改《{}》{} 处", target.name, kept.len()), state, text: lines.join("\n"), records: vec![] }, !kept.is_empty())
        }
    }
}

/// `excel_plan` then `excel_commit`, for a caller already holding the database (tests).
#[cfg(test)]
pub fn run_excel(conn: &Connection, run_id: i64, files: &[Editable], arguments: &str, budget: i64) -> (Answer, bool) {
    excel_commit(conn, run_id, excel_plan(files, arguments, budget))
}

/// The Excel 工具箱 definition.
pub fn excel_tool() -> Value {
    let value = json!({ "description": "新的值：数字、文字、2026-10-01 这样的日期、15% 这样的百分比，= 开头是公式（=B3*2）；空字符串是清空；' 开头是照原样的文字（'007）" });
    let rows = json!({ "type": "array", "description": "一行一个数组，从 A 列写起", "items": { "type": "array", "items": {} } });
    json!({ "type": "function", "function": {
        "name": EXCEL_TOOL,
        "description": "改用户的 Excel（.xlsx）文件，或者新建 Excel。只能改这个问题的附件（它们的原文件）和知识库里的 .xlsx。view=files：看能改哪些。view=read：读一张表（sheet，不写是第一张；from、to 是行号；keyword 只看含这些字的行），每行写成「行号：列=值 | 列=值」，公式写成 =B3*2（结果 6），日期、百分比是看得懂的样子，最上面列出每张表的大小和不能改的地方。view=edit：提议改动，放进确认卡片，用户勾选后才写进文件：changes 改格子（空格子也能填）；add_rows 在某一行后面加行（after：0 是最前面；样式照着上面那行；公式不会自动填，要写出来）；delete_rows 删行。行号都按现在读到的写，不用替前面的改动重新算；加行删行时下面的行和用到它们的公式由程序挪好。一条回答只改一个文件，改之前先 read。只在回答里写改法不会进卡片；回答里写格子的位置（B3），不要写成出处编号。公式的结果要等 Excel 打开文件时才重算。view=create：新建一个 Excel（name 文件名；sheets：每张表的 name 和 rows，第一行是表头），放进确认卡片，用户点「保存」才存。",
        "parameters": { "type": "object", "properties": {
            "view": { "type": "string", "enum": ["files", "read", "edit", "create"] },
            "file": { "type": "string", "description": "read / edit：文件名（files 列出的；只有一个文件时可以不写）" },
            "sheet": { "type": "string", "description": "read：表名（不写是第一张）" },
            "from": { "type": "integer", "description": "read：从第几行" },
            "to": { "type": "integer", "description": "read：到第几行" },
            "keyword": { "type": "string", "description": "read：只看含这些字的行" },
            "changes": { "type": "array", "description": "edit：要改的格子", "items": { "type": "object", "properties": { "sheet": { "type": "string", "description": "表名（只有一张表时可以不写）" }, "cell": { "type": "string", "description": "格子，如 B3" }, "value": value }, "required": ["cell", "value"] } },
            "add_rows": { "type": "array", "description": "edit：要加的行", "items": { "type": "object", "properties": { "sheet": { "type": "string", "description": "表名" }, "after": { "type": "integer", "description": "加在第几行后面（0 是最前面）" }, "rows": rows }, "required": ["after", "rows"] } },
            "delete_rows": { "type": "array", "description": "edit：要删的行", "items": { "type": "object", "properties": { "sheet": { "type": "string", "description": "表名" }, "rows": { "type": "array", "description": "行号", "items": { "type": "integer" } } }, "required": ["rows"] } },
            "name": { "type": "string", "description": "create：文件名（不用写 .xlsx）" },
            "sheets": { "type": "array", "description": "create：每张表", "items": { "type": "object", "properties": { "name": { "type": "string", "description": "表名" }, "rows": rows }, "required": ["rows"] } }
        }, "required": ["view"] }
    } })
}

/// The tool definition (`word`: Word 工具箱, else 文本工具箱).
pub fn tool(word: bool) -> Value {
    let (name, description) = if word {
        (
            WORD_TOOL,
            "改用户的 Word（.docx）文件，或者新建 Word。只能改这个问题的附件（它们的原文件）和知识库里的 Word 文件。view=files：看能改哪些；view=read：按段号读（from、to），每段前面是【段号】，带着格式标记：段首 # 是标题（## 是二级标题），段首 - 或 1. 是列表，**加粗**、*斜体*、<u>下划线</u>，\\* 是星号本身；表格是一段，一行一行写，格子用 | 隔开，只能改格子里的字（行数、每行的格数不变，整张表照原样写回、只改要改的格子）；标着「不能改」的（域、文本框、空段、有的表格）不能改。view=edit：提议改动（changes：每段的段号和改好的整段文字，格式照上面的标记写，不改的格式原样保留；要删掉一段就写空文字；要在一段后面加新段落，就在这段的文字里空一行再写新的），会放进确认卡片，用户勾选后才写进文件；一条回答只改一个文件，改之前先 read 要改的部分。只在回答里写改法不会进卡片。段号不是出处编号：回答里写「第 n 段」，不要写成 [n]。view=create：新建一个 Word（name 文件名，content 用 Markdown 写的全文），放进确认卡片，用户点「保存」才存。",
        )
    } else {
        (
            TEXT_TOOL,
            "改用户的 Markdown 和纯文本文件。只能改这个问题的附件（它们的原文件）和知识库里的这类文件。view=files：看能改哪些；view=read：按段号读（from、to），段落是空行隔开的块，每段前面是【段号】。view=edit：提议改动（changes：每段的段号和改好的整段文字，保持原来的格式标记；要删掉一段就写空文字），放进确认卡片，用户勾选后才写进文件；一条回答只改一个文件，改之前先 read 要改的部分。只在回答里写改法不会进卡片。段号不是出处编号：回答里写「第 n 段」，不要写成 [n]。",
        )
    };
    let mut props = json!({
        "view": { "type": "string", "enum": if word { json!(["files", "read", "edit", "create"]) } else { json!(["files", "read", "edit"]) } },
        "file": { "type": "string", "description": "read / edit：文件名（files 列出的；只有一个文件时可以不写）" },
        "from": { "type": "integer", "description": "read：从第几段（1 开始）" },
        "to": { "type": "integer", "description": "read：到第几段" },
        "changes": { "type": "array", "description": "edit：要改的段落", "items": { "type": "object", "properties": { "paragraph": { "type": "integer", "description": "段号" }, "text": { "type": "string", "description": "改好的整段文字" } }, "required": ["paragraph", "text"] } }
    });
    if word {
        props["name"] = json!({ "type": "string", "description": "create：文件名（不用写 .docx）" });
        props["content"] = json!({ "type": "string", "description": "create：全文（Markdown：标题、列表、表格、加粗都行）" });
    }
    json!({ "type": "function", "function": { "name": name, "description": description, "parameters": { "type": "object", "properties": props, "required": ["view"] } } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::files::tests::add;
    use crate::picky::store::tests::memory_db;

    fn att(c: &Connection, a: i64) -> Source {
        let _ = c;
        Source { kind: Kind::Attachment, artifact: a, ordinal: 0, label: "x".into(), content: String::new(), kb: None, earlier: false }
    }

    /// Excel 工具箱: reads with addresses, proposes into the card, writes the ticked ones; a new workbook; a file
    /// changed since isn't written.
    #[test]
    fn excel_reads_proposes_and_writes() {
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("toolbox-excel");
        let sample = crate::picky::excel::tests::sample_xlsx();
        let path = dir.join("预算.xlsx");
        std::fs::write(&path, &sample).unwrap();
        let a = add(&c, &dir, "预算.xlsx", &sample);
        files::remember_source(&c, a, &path).unwrap();
        let other = add(&c, &dir, "别的.md", b"# x");
        let list = editable_excel(&c, None, &[att(&c, a), att(&c, other)], None);
        assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["预算.xlsx"]);
        let rid = crate::picky::chat::begin_run(&c, None, None, "改预算", "local:x", "X").unwrap().run_id;
        let (r, changed) = run_excel(&c, rid, &list, r#"{"view":"read","sheet":"预算 表","from":3,"to":4}"#, 4000);
        assert!(!changed && r.text.contains("3：A=房租 | B=3000 | C=2026-10-01") && r.text.contains("合并的格子：A1:B1") && r.text.contains("「引用」"), "{r:?}");
        // cells and rows into the card; what can't be done is said; the file untouched
        let (e, changed) = run_excel(&c, rid, &list, r#"{"view":"edit","changes":[{"sheet":"预算 表","cell":"B3","value":3200},{"sheet":"预算 表","cell":"B1","value":"x"}],"add_rows":[{"sheet":"预算 表","after":5,"rows":[["水费",60]]}],"delete_rows":[{"sheet":"预算 表","rows":[4]}]}"#, 4000);
        assert!(changed && e.text.contains("已放进确认卡片") && e.text.contains("表「预算 表」B3") && e.text.contains("改左上角的 A1"), "{e:?}");
        // the 合计 stops right above the added row: said, unless this edit changes it too
        assert!(e.text.contains("注意：B6（=SUM(B3:B5)）的范围停在第 5 行：写入时会扩到新加的行"), "{e:?}");
        let rid0 = crate::picky::chat::begin_run(&c, None, None, "加一行", "local:x", "X").unwrap().run_id;
        let (e0, _) = run_excel(&c, rid0, &list, r#"{"view":"edit","changes":[{"sheet":"预算 表","cell":"b6","value":"=SUM(B3:B6)"}],"add_rows":[{"sheet":"预算 表","after":5,"rows":[["水费",60]]}]}"#, 4000);
        assert!(!e0.text.contains("注意"), "{e0:?}");
        let (e1, _) = run_excel(&c, rid0, &list, r#"{"view":"edit","sheet":"预算 表","add_rows":[{"after":4,"rows":[["水费",60]]}]}"#, 4000);
        assert_eq!(e1.state, "ok", "{e1:?}"); // the sheet outside the items
        assert!(!e1.text.contains("注意"), "{e1:?}"); // inside the range: Excel widens it
        assert_eq!(std::fs::read(&path).unwrap(), sample);
        let card = work::edit_of(&c, rid).unwrap().unwrap();
        assert_eq!((card.kind.as_str(), card.items.len()), ("excel", 3));
        let labels: Vec<&str> = card.items.iter().map(|i| i.label.as_deref().unwrap()).collect();
        assert_eq!(labels, ["表「预算 表」B3", "表「预算 表」删第 4 行", "表「预算 表」第 5 行后加 1 行"]);
        assert_eq!((card.items[0].before.as_str(), card.items[0].after.as_str()), ("3000", "3200"));
        // the same cell again replaces it
        run_excel(&c, rid, &list, r#"{"view":"edit","changes":[{"sheet":"预算 表","cell":"B3","value":"3300"}]}"#, 4000);
        let card = work::edit_of(&c, rid).unwrap().unwrap();
        assert_eq!((card.items.len(), card.items[0].after.as_str()), (3, "3300"));
        // only the ticked ones are written
        let keep: Vec<i64> = card.items.iter().filter(|i| !i.label.as_deref().unwrap().contains("删")).map(|i| i.position).collect();
        let written = work::write(&c, rid, &keep).unwrap();
        assert_eq!(written.state, "written");
        let b = Book::open(&std::fs::read(&path).unwrap()).unwrap();
        let (_, text, _) = b.read_rows("预算 表", 1, 99, "", 10_000).unwrap();
        assert!(text.contains("B=3300") && text.contains("A=水费") && text.contains("A=水电"), "{text}");
        // changed since: not written
        let rid2 = crate::picky::chat::begin_run(&c, None, None, "再改", "local:x", "X").unwrap().run_id;
        run_excel(&c, rid2, &list, r#"{"view":"edit","changes":[{"sheet":"预算 表","cell":"B3","value":"1"}]}"#, 4000);
        std::fs::write(&path, &sample).unwrap();
        let pos = work::edit_of(&c, rid2).unwrap().unwrap().items[0].position;
        assert!(work::write(&c, rid2, &[pos]).unwrap_err().contains("被改过了"));
        // a new workbook: saved where the user says, never over a file
        let rid3 = crate::picky::chat::begin_run(&c, None, None, "做个表", "local:x", "X").unwrap().run_id;
        let (n, changed) = run_excel(&c, rid3, &[], r#"{"view":"create","name":"开销","sheets":[{"name":"十月","rows":[["项目","金额"],["房租",3000]]}]}"#, 4000);
        assert!(changed && n.label == "Excel 工具箱 · 提议新建《开销.xlsx》", "{n:?}");
        let card = work::edit_of(&c, rid3).unwrap().unwrap();
        assert_eq!((card.kind.as_str(), card.name.as_str()), ("new-excel", "开销.xlsx"));
        assert!(card.items[0].after.contains("| 房租 | 3000 |"));
        let saved = work::create_word(&c, rid3, &dir.display().to_string(), "开销").unwrap();
        assert!(saved.state == "written" && saved.path.ends_with("开销.xlsx"), "{saved:?}");
        let (_, rows, _) = Book::open(&std::fs::read(&saved.path).unwrap()).unwrap().read_rows("十月", 1, 9, "", 1000).unwrap();
        assert!(rows.contains("2：A=房租 | B=3000"), "{rows}");
        // no files: says how to give one
        let (none, _) = run_excel(&c, rid3, &[], r#"{"view":"read"}"#, 4000);
        assert!(none.state == "failed" && none.text.contains("从访达拖进来"), "{none:?}");
    }

    /// 改文件 by tools: only files with an original; read, propose (one file per reply), new Word.
    #[test]
    fn toolboxes_read_and_propose_into_the_card() {
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("toolbox");
        let doc_path = dir.join("求职信.docx");
        std::fs::write(&doc_path, crate::picky::word::tests::sample_docx()).unwrap();
        let md_path = dir.join("周报.md");
        std::fs::write(&md_path, "# 周报\n\n这周做完了知识库。\n\n下周做 Work。\n").unwrap();
        // attachments: one added from disk (has an original), one pasted (none)
        let a = add(&c, &dir, "求职信.docx", &crate::picky::word::tests::sample_docx());
        files::remember_source(&c, a, &doc_path).unwrap();
        let pasted = add(&c, &dir, "截图.md", b"# x\ny");
        let m = add(&c, &dir, "周报.md", b"# x");
        files::remember_source(&c, m, &md_path).unwrap();
        let sources = vec![att(&c, a), att(&c, pasted), att(&c, m)];
        let word = editable(&c, None, &sources, None, None, true);
        assert_eq!(word.iter().map(|e| (e.name.as_str(), e.from.as_str())).collect::<Vec<_>>(), [("求职信.docx", "附件")]);
        let text = editable(&c, None, &sources, None, None, false);
        assert_eq!(text.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["周报.md"]);
        let rid = crate::picky::chat::begin_run(&c, None, None, "改一下", "local:x", "X").unwrap().run_id;
        // files and read (Word with its marks)
        let (a1, _) = run(&c, rid, &word, true, r#"{"view":"files"}"#, 4000);
        assert!(a1.text.contains("- 求职信.docx（附件）"), "{a1:?}");
        let (r, changed) = run(&c, rid, &word, true, r#"{"view":"read","from":1,"to":2}"#, 4000);
        assert!(!changed && r.text.contains("【1】\n# 求职信") && r.text.contains("【2】\n我对贵公司的 **产品经理** 岗位很感兴趣。") && r.label.contains("第 1–2 段（共 6 段）"), "{r:?}");
        // edit: into the card (not the file); a fixed paragraph and a missing one refused
        let (e, changed) = run(&c, rid, &word, true, r#"{"view":"edit","changes":[{"paragraph":2,"text":"我对贵公司的 **高级产品经理** 岗位很感兴趣。"},{"paragraph":3,"text":"x"},{"paragraph":9,"text":"y"}]}"#, 4000);
        assert!(changed && e.text.contains("已放进确认卡片：《求职信.docx》第 2 段") && e.text.contains("第 3 段") && e.text.contains("第 9 段：没有这一段"), "{e:?}");
        let card = work::edit_of(&c, rid).unwrap().unwrap();
        assert_eq!((card.kind.as_str(), card.items.len(), card.items[0].paragraph), ("word", 1, 2));
        assert_eq!(std::fs::read(&doc_path).unwrap(), crate::picky::word::tests::sample_docx()); // nothing written
        // the same paragraph again replaces it; another file in the same reply is refused
        run(&c, rid, &word, true, r#"{"view":"edit","changes":[{"paragraph":2,"text":"我对贵公司的 *产品经理* 岗位很感兴趣。"}]}"#, 4000);
        let card = work::edit_of(&c, rid).unwrap().unwrap();
        assert_eq!(card.items.len(), 1);
        assert!(card.items[0].after.contains("*产品经理*"));
        let (x, _) = run(&c, rid, &text, false, r#"{"view":"edit","file":"周报.md","changes":[{"paragraph":2,"text":"改了"}]}"#, 4000);
        assert!(x.state == "failed" && x.text.contains("只改一个文件"), "{x:?}");
        // confirming writes it
        let written = work::write(&c, rid, &[card.items[0].position]).unwrap();
        assert_eq!(written.state, "written");
        assert_eq!(crate::picky::word::read_docx(&std::fs::read(&doc_path).unwrap()).unwrap().items[1].text, "我对贵公司的 *产品经理* 岗位很感兴趣。");
        // a new Word: a card with its name and folder, saved on confirm
        let rid2 = crate::picky::chat::begin_run(&c, None, None, "写成 Word", "local:x", "X").unwrap().run_id;
        let (n, changed) = run(&c, rid2, &[], true, r##"{"view":"create","name":"调研/报告","content":"# 调研\n\n结论。"}"##, 4000);
        assert!(changed && n.label == "Word 工具箱 · 提议新建《调研 报告.docx》", "{n:?}");
        let card = work::edit_of(&c, rid2).unwrap().unwrap();
        assert_eq!((card.kind.as_str(), card.name.as_str()), ("new-word", "调研 报告.docx"));
        let saved = work::create_word(&c, rid2, &dir.display().to_string(), "调研 报告").unwrap();
        assert!(saved.state == "written" && saved.path.ends_with("调研 报告.docx") && Path::new(&saved.path).is_file(), "{saved:?}");
        // a 知识库 file goes by 「文件夹/路径」: written in place all the same
        std::fs::create_dir_all(dir.join("资料")).unwrap();
        let kb_path = dir.join("资料").join("笔记.md");
        std::fs::write(&kb_path, "# 笔记\n\n旧的一段。\n").unwrap();
        let kb = [Editable { name: "资料/笔记.md".into(), path: kb_path.display().to_string(), from: "知识库 · 测试".into() }];
        let rid3 = crate::picky::chat::begin_run(&c, None, None, "改知识库", "local:x", "X").unwrap().run_id;
        let (k, _) = run(&c, rid3, &kb, false, r#"{"view":"edit","file":"笔记.md","changes":[{"paragraph":2,"text":"新的一段。"}]}"#, 4000);
        assert_eq!(k.state, "ok", "{k:?}");
        let pos = work::edit_of(&c, rid3).unwrap().unwrap().items[0].position;
        assert_eq!(work::write(&c, rid3, &[pos]).unwrap().state, "written");
        assert_eq!(std::fs::read_to_string(&kb_path).unwrap(), "# 笔记\n\n新的一段。\n");
                // no files: says how to give one
        let (none, _) = run(&c, rid2, &[], false, r#"{"view":"read"}"#, 4000);
        assert!(none.state == "failed" && none.text.contains("从访达拖进来"));
    }
}
