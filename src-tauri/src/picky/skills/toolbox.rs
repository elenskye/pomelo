//! Word 工具箱 (`pomelo_word`) and 文本工具箱 (`pomelo_text`) (v0.4.5 improve-10): 改文件 as plugin
//! skills that 小拾 uses when it decides to. A toolbox changes only files the question has an original
//! of — its attachments added from disk (`artifacts.source_path`), the file an older question picked to
//! change — and the 知识库's files (in place); never any other path.
//! - view=files: what may be changed (also in the system message: `note`). read: the numbered
//!   paragraphs (Word with its formatting marks and tables as rows, word.rs; 「不能改」 marked), ending
//!   with a reminder that nothing is proposed yet (models said 「放进卡片了」 after only reading). edit: changes into the reply's card (work.rs `file_edits`, one file per
//!   reply, checked the same way), written only when the user ticks them and confirms. create (Word):
//!   a new document's name and Markdown, saved only when the user confirms (and picks the folder).
//! - Nothing here writes a file; 定时任务 never get a toolbox (chat.rs).

use crate::picky::data::Answer;
use crate::picky::knowledge::{self, Scope};
use crate::picky::reading::{Kind, Source};
use crate::picky::{context, files, work};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::Path;

pub const WORD_TOOL: &str = "pomelo_word";
pub const TEXT_TOOL: &str = "pomelo_text";

pub fn is_tool(name: &str) -> bool {
    name == WORD_TOOL || name == TEXT_TOOL
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
    let mut out: Vec<Editable> = vec![];
    let mut add = |name: String, path: String, from: String| {
        if fits(word, &path) && Path::new(&path).is_file() && !out.iter().any(|e| e.path == path) {
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
