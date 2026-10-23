//! Importing the Seeker folder's `transfer/` (Bear-style Markdown, read only): the user previews,
//! picks per item (a source imported before: file version or keep the current one), confirms.
//! Files and what they become (by path; anything else is listed as skipped):
//! - `cn-modules/<n> - 基本信息.md` → 基本信息 (`- 姓名：…` lines), `… 网申.md` → 网申资料, `教育 / 技能 / 项目 / 工作` → materials of that category.
//!   A material file is blocks split by `---`: a bold line, then lines up to the bullets, then
//!   `- ` / `* ` bullets (`**小标题**：…` kept). 项目: bold = role, next line `名称：概览`; others:
//!   bold = name, next line = role. A `YYYY.MM - YYYY.MM　地点` line is time and place; a `技术栈：`
//!   line becomes a point. A file of bullets only (技能) is one record titled by its heading.
//! - `rules/cn-modules.md` → rule 素材润色, `rules/cn-resume.md` → rule 整体审查, `SEEKER.md` → rule
//!   共用准则, `FACTS.md` → fact 已确认事实, `projects/*.md` → fact 经历档案: one record per file.
//! - English (`en-modules/`, `rules/en-*`), the question bank and archives are left for later.
//! A record's source is `<path>#<title>:<n>` (n: the how-many-th block with that title), so a later
//! import matches it though blocks move; point ids come from the bullet's 小标题 when it has one.
use super::{catalog::Record, *};
use crate::modules::common::Tx;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub struct ImportItem {
    pub incoming: Record,
    pub current: Option<Record>,
}

#[derive(Serialize)]
pub struct Preview {
    pub items: Vec<ImportItem>,
    pub skipped: Vec<String>,
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("读不到 transfer/：{e}"))? {
        let entry = entry.map_err(err)?;
        let kind = entry.file_type().map_err(err)?;
        if kind.is_symlink() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if kind.is_dir() {
            collect(&entry.path(), files)?;
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
    Ok(())
}

/// What a file under transfer/ becomes: (kind, category), or why it's left out.
pub fn classify(path: &str) -> Result<(&'static str, &'static str), &'static str> {
    let lower = path.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if !(name.ends_with(".md") || name.ends_with(".txt")) {
        return Err("只导入 Markdown 和文本");
    }
    if lower.starts_with("en-modules/") || lower.starts_with("rules/en-") {
        return Err("英文资料以后再导入");
    }
    if name.contains("qbank") || name.contains("题库") {
        return Err("面试题库以后再导入");
    }
    if name.contains("归档") || name.contains("archive") {
        return Err("归档的不导入");
    }
    Ok(if lower.starts_with("cn-modules/") {
        if name.contains("网申") {
            ("application", "网申")
        } else if name.contains("基本信息") {
            ("profile", "基本信息")
        } else if name.contains("教育") {
            ("material", "教育")
        } else if name.contains("技能") {
            ("material", "技能")
        } else if name.contains("项目") {
            ("material", "项目")
        } else if name.contains("工作") || name.contains("实习") {
            ("material", "工作")
        } else {
            return Err("不认识的素材文件");
        }
    } else if lower == "rules/cn-modules.md" {
        ("rule", "素材润色")
    } else if lower == "rules/cn-resume.md" {
        ("rule", "整体审查")
    } else if lower == "seeker.md" {
        ("rule", "共用准则")
    } else if lower == "facts.md" {
        ("fact", "已确认事实")
    } else if lower.starts_with("projects/") {
        ("fact", "经历档案")
    } else {
        return Err("不是要导入的资料");
    })
}

/// The file's `# 标题` without its number (「4 - 项目经历」 → 项目经历), and the lines after it
/// without Bear tags (`#职业/中文简历`).
fn split_heading<'a>(path: &str, text: &'a str) -> (String, Vec<&'a str>) {
    let stem = Path::new(path).file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let mut title = None;
    let mut lines = vec![];
    for line in text.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix("# ") {
            if title.is_none() && lines.iter().all(|l: &&str| l.trim().is_empty()) {
                title = Some(h.trim().to_string());
                continue;
            }
        }
        if t.starts_with('#') && !t.starts_with("# ") && !t.starts_with("##") && !t.contains(' ') {
            continue; // a Bear tag
        }
        lines.push(line);
    }
    let title = title.unwrap_or(stem);
    let bare = title.split_once(" - ").filter(|(n, _)| n.trim().chars().all(|c| c.is_ascii_digit())).map_or(title.as_str(), |(_, t)| t).trim().to_string();
    (bare, lines)
}

fn bullet(line: &str) -> Option<&str> {
    let t = line.trim_start();
    t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("• ")).map(str::trim)
}

fn bold_line(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix("**").and_then(|x| x.strip_suffix("**")).filter(|x| !x.contains("**")).map(str::trim)
}

/// `2026.06 - 2026.07　新加坡` → (time, place); None if the line doesn't start with a date.
fn time_line(line: &str) -> Option<(String, String)> {
    let words: Vec<&str> = line.split(char::is_whitespace).filter(|w| !w.is_empty()).collect();
    if !words.first()?.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let n = words.iter().take_while(|w| w.starts_with(|c: char| c.is_ascii_digit()) || matches!(**w, "-" | "–" | "—" | "~" | "至" | "至今")).count();
    Some((words[..n].join(" "), words[n..].join(" ")))
}

/// Points with ids from their 小标题 (`**智能管线**：…` → `智能管线`), else from their place.
fn points(lines: &[String]) -> Vec<Point> {
    let mut out: Vec<Point> = vec![];
    for text in lines {
        let label = text.strip_prefix("**").and_then(|t| t.split_once("**")).map(|(l, _)| l.trim().to_string()).filter(|l| !l.is_empty()).or_else(|| text.starts_with("技术栈").then(|| "技术栈".to_string()));
        let id = match label {
            Some(l) if !out.iter().any(|p| p.id == l) => l,
            _ => format!("{}", out.len() + 1),
        };
        out.push(Point { id, text: text.clone() });
    }
    out
}

fn block_record(category: &str, title_of_file: &str, block: &[&str]) -> Option<Record> {
    let mut lines = block.iter().map(|l| l.trim_end()).filter(|l| !l.trim().is_empty()).peekable();
    let bold = lines.peek().and_then(|l| bold_line(l)).map(str::to_string);
    if bold.is_some() {
        lines.next();
    }
    let mut r = Record { kind: "material".into(), category: category.into(), ..Default::default() };
    let (mut extra, mut bullets, mut tail) = (vec![], vec![], vec![]);
    for line in lines {
        if let Some(b) = bullet(line) {
            bullets.push(b.to_string());
        } else if !bullets.is_empty() {
            tail.push(line.trim().to_string());
        } else if let Some((time, place)) = time_line(line).filter(|_| r.time.is_empty()) {
            (r.time, r.location) = (time, place);
        } else if line.trim().starts_with("技术栈") {
            bullets.push(line.trim().to_string());
        } else {
            extra.push(line.trim().to_string());
        }
    }
    let mut extra = extra.into_iter();
    match (bold, category) {
        (Some(role), "项目") => {
            r.role = role;
            let first = extra.next().unwrap_or_default();
            match first.split_once('：').or_else(|| first.split_once(':')) {
                Some((name, note)) => (r.title, r.text) = (name.trim().into(), note.trim().into()),
                None => r.title = first,
            }
        }
        (Some(name), _) => {
            r.title = name;
            r.role = extra.next().unwrap_or_default();
        }
        (None, _) => r.title = title_of_file.into(),
    }
    let rest: Vec<String> = extra.chain(tail).collect();
    if !rest.is_empty() {
        r.text = [r.text.clone()].into_iter().chain(rest).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
    }
    if r.title.is_empty() {
        r.title = title_of_file.into();
    }
    r.points = points(&bullets);
    (!r.points.is_empty() || !r.text.is_empty() || !r.time.is_empty()).then_some(r)
}

/// The records a file holds (sources not set yet).
pub fn parse(path: &str, kind: &str, category: &str, text: &str) -> Vec<Record> {
    let (title, lines) = split_heading(path, text);
    match kind {
        "material" | "application" => {
            let mut blocks: Vec<Vec<&str>> = vec![vec![]];
            for line in lines {
                let t = line.trim();
                let starts = bold_line(t).is_some() && blocks.last().is_some_and(|b| b.iter().any(|l| bullet(l).is_some()));
                if t.chars().all(|c| c == '-') && t.len() >= 3 || starts {
                    blocks.push(vec![]);
                    if t.starts_with('-') {
                        continue;
                    }
                }
                blocks.last_mut().unwrap().push(line);
            }
            let mut out: Vec<Record> = blocks.iter().filter_map(|b| block_record(category, &title, b)).collect();
            if kind == "application" {
                for r in &mut out {
                    // an answer is its text: the bullets read as lines
                    r.kind = kind.into();
                    let body = r.points.drain(..).map(|p| p.text).collect::<Vec<_>>();
                    r.text = [std::mem::take(&mut r.text)].into_iter().chain(body).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
                    (r.time, r.role, r.location) = Default::default();
                }
            }
            out
        }
        "profile" => {
            let mut r = Record { kind: kind.into(), category: category.into(), title, ..Default::default() };
            for line in lines {
                let t = bullet(line).unwrap_or(line.trim()).replace("**", "");
                if let Some((label, value)) = t.split_once('：').or_else(|| t.split_once(':')) {
                    r.fields.push(HeaderField { id: format!("f{}", r.fields.len() + 1), label: label.trim().into(), value: value.trim().into(), visible: true });
                }
            }
            if r.fields.is_empty() { vec![] } else { vec![r] }
        }
        _ => {
            let body = lines.join("\n").trim().to_string();
            if body.is_empty() { vec![] } else { vec![Record { kind: kind.into(), category: category.into(), title, text: body, ..Default::default() }] }
        }
    }
}

/// Every file under `root` (transfer/): the records, matched with the ones imported before.
pub fn preview(root: &Path, current: &[Record]) -> Result<Preview, String> {
    if std::fs::symlink_metadata(root).map_err(|_| "Seeker 文件夹里没有 transfer/".to_string())?.file_type().is_symlink() {
        return Err("transfer/ 不能是替身".into());
    }
    let mut files = vec![];
    collect(root, &mut files)?;
    files.sort();
    let mut out = Preview { items: vec![], skipped: vec![] };
    for path in files {
        let relative = path.strip_prefix(root).map_err(err)?.to_string_lossy().into_owned();
        let (kind, category) = match classify(&relative) {
            Ok(k) => k,
            Err(why) => {
                out.skipped.push(format!("{relative}：{why}"));
                continue;
            }
        };
        if std::fs::metadata(&path).map_err(err)?.len() > 2 * 1024 * 1024 {
            out.skipped.push(format!("{relative}：超过 2 MB"));
            continue;
        }
        let Ok(text) = String::from_utf8(std::fs::read(&path).map_err(err)?) else {
            out.skipped.push(format!("{relative}：不是 UTF-8 文本"));
            continue;
        };
        let mut seen = std::collections::HashMap::<String, usize>::new();
        for mut incoming in parse(&relative, kind, category, &text) {
            let n = seen.entry(incoming.title.clone()).or_default();
            *n += 1;
            let source = format!("{relative}#{}:{n}", incoming.title);
            let current = current.iter().find(|r| r.source.as_deref() == Some(source.as_str())).cloned();
            match &current {
                // the file's text, keeping what the user set here (kind, category, on / off)
                Some(old) => {
                    (incoming.id, incoming.revision, incoming.kind, incoming.category, incoming.enabled) = (old.id.clone(), old.revision, old.kind.clone(), old.category.clone(), old.enabled);
                }
                None => incoming.id = workflow::id()?,
            }
            incoming.source = Some(source);
            if current.as_ref().is_some_and(|old| *old == incoming) {
                continue; // unchanged
            }
            out.items.push(ImportItem { incoming, current });
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn picky_seeker_import_preview(state: State<'_, AppState>) -> Result<Preview, String> {
    let view = state.picky.with_db(|c| read(c))?;
    if view.folder.is_empty() {
        return Err("请先选择 Seeker 文件夹".into());
    }
    let folder = output::allowed_path(Path::new(&view.folder), state.picky.mode)?;
    let records = view.catalog.records;
    tauri::async_runtime::spawn_blocking(move || preview(&folder.join("transfer"), &records)).await.map_err(err)?
}

#[tauri::command]
pub async fn picky_seeker_import_confirm(state: State<'_, AppState>, records: Vec<Record>) -> Result<Workspace, String> {
    state.with_picky_undo("导入 Seeker 资料", |c| {
        let tx = Tx::begin(c)?;
        for record in &records {
            catalog::put(&tx, record)?;
        }
        tx.commit()?;
        read(c)
    })
}
