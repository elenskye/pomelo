//! Importing the Seeker folder's `transfer/` (Markdown, read only; Pomelo is where the user keeps the
//! records afterwards): the user previews, picks per item (a source imported before: file version or
//! keep the current one; one the files don't have any more: remove it), confirms. One undo step.
//! A file's text before its first `## ` is a note about the file and isn't imported.
//! - `cn-modules/<n> - 基本信息.md` → 基本信息 (`- 姓名：…` lines); `… 网申.md` → 网申资料 (a `## ` per
//!   answer); `教育 / 技能 / 项目 / 工作` → materials: a `## 名称` per entry, then field lines
//!   `- 角色：` (or 职位 / 专业 / 学历) `- 时间：` `- 地点：` `- 概览：` (or 说明) `- 技术栈：`, then the points
//!   (`- **小标题**：…`). A file without `## ` (技能) is one material titled by its heading.
//! - `SEEKER.md` (共用准则), `rules/cn-modules.md` (素材润色), `rules/cn-resume.md` (整体审查) → a rule per
//!   `## ` section (a first line `用于：…` sets the task); a section named 「有效个人约定」 → a decision
//!   per bullet (`- **名称**：内容`).
//! - `FACTS.md` → a fact per bullet (`- **事项**：口径`), its `## ` group as the category.
//! - `projects/*.md` → 经历档案 (one each; lines `- 素材：名称` before its first `## ` link it to that
//!   Chinese and / or English material).
//! - English (阶段 ③, same shapes): `en-modules/` Summary (a `## ` per version, its text), Contact
//!   (基本信息), Skills, Education, Projects, Experience, Certifications (like 技能: points only);
//!   `rules/en-modules.md` (素材润色), `rules/en-resume.md` (整体审查). SEEKER.md, FACTS.md, projects/
//!   are both languages'.
//! - `rules/qbank.md` (题库维护) and `rules/interview.md` (面试): Rater's rules, a rule per `## ` section
//!   like the others (rater.rs `RULES`). Archives aren't imported.
//! A record's source is `<path>#<title>:<n>` (n: the how-many-th with that title in the file), so a
//! later import matches it though items move; point ids come from the bullet's 小标题.
use super::{catalog::Record, *};
use crate::modules::common::Tx;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub struct ImportItem {
    pub incoming: Record,
    pub current: Option<Record>,
}

#[derive(Serialize)]
pub struct Preview {
    pub items: Vec<ImportItem>,
    /// Imported before, not in the files any more.
    pub gone: Vec<Record>,
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

/// What a file under transfer/ becomes: (what kind of file, the category, the language; "" = both),
/// or why it's left out.
pub fn classify(path: &str) -> Result<(&'static str, &'static str, &'static str), &'static str> {
    let lower = path.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if !(name.ends_with(".md") || name.ends_with(".txt")) {
        return Err("只导入 Markdown 和文本");
    }
    if name.contains("归档") || name.contains("archive") {
        return Err("归档的不导入");
    }
    if lower.starts_with("en-modules/") {
        let kind = if name.contains("summary") {
            ("material", "摘要")
        } else if name.contains("contact") {
            ("profile", "基本信息")
        } else if name.contains("skill") {
            ("material", "技能")
        } else if name.contains("educat") {
            ("material", "教育")
        } else if name.contains("project") {
            ("material", "项目")
        } else if name.contains("experience") || name.contains("work") || name.contains("intern") {
            ("material", "工作")
        } else if name.contains("certif") {
            ("material", "证书")
        } else {
            return Err("不认识的素材文件");
        };
        return Ok((kind.0, kind.1, "en"));
    }
    match lower.as_str() {
        "rules/en-modules.md" => return Ok(("rule", "素材润色", "en")),
        "rules/en-resume.md" => return Ok(("rule", "整体审查", "en")),
        "rules/qbank.md" => return Ok(("rule", crate::picky::assistants::rater::RULES[0], "cn")),
        "rules/interview.md" => return Ok(("rule", crate::picky::assistants::rater::RULES[1], "cn")),
        "seeker.md" => return Ok(("rule", "共用准则", "")),
        "facts.md" => return Ok(("facts", "", "")),
        _ if lower.starts_with("projects/") => return Ok(("dossier", "经历档案", "")),
        _ => {}
    }
    Ok(if lower.starts_with("cn-modules/") {
        if name.contains("网申") {
            ("application", "网申", "")
        } else if name.contains("基本信息") {
            ("profile", "基本信息", "cn")
        } else if name.contains("教育") {
            ("material", "教育", "cn")
        } else if name.contains("技能") {
            ("material", "技能", "cn")
        } else if name.contains("项目") {
            ("material", "项目", "cn")
        } else if name.contains("工作") || name.contains("实习") {
            ("material", "工作", "cn")
        } else {
            return Err("不认识的素材文件");
        }
    } else if lower == "rules/cn-modules.md" {
        ("rule", "素材润色", "cn")
    } else if lower == "rules/cn-resume.md" {
        ("rule", "整体审查", "cn")
    } else {
        return Err("不是要导入的资料");
    })
}

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
    let bare = title.split_once('-').filter(|(n, rest)| (1..=2).contains(&n.trim().len()) && n.trim().chars().all(|c| c.is_ascii_digit()) && !rest.trim().is_empty()).map_or(title.as_str(), |(_, t)| t).trim().to_string();
    (bare, lines)
}

fn bullet(line: &str) -> Option<&str> {
    let t = line.trim_start();
    t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("• ")).map(str::trim)
}


/// `**名称**：内容` → (名称, 内容); else None.
fn labelled(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix("**")?;
    let (label, after) = rest.split_once("**")?;
    let after = after.trim_start();
    let body = after.strip_prefix('：').or_else(|| after.strip_prefix(':')).unwrap_or(after);
    let label = label.trim().trim_end_matches(['：', ':']).trim();
    (!label.is_empty()).then(|| (label.to_string(), body.trim().to_string()))
}

/// Points with ids from their 小标题 (`**智能管线**：…` → `智能管线`), else from their place.
fn points(lines: &[String]) -> Vec<Point> {
    let mut out: Vec<Point> = vec![];
    for text in lines {
        let id = match labelled(text).map(|(l, _)| l) {
            Some(l) if !out.iter().any(|p| p.id == l) && l != catalog::STACK => l,
            _ => format!("{}", out.len() + 1),
        };
        out.push(Point { id, text: text.clone() });
    }
    out
}

/// `## ` sections: (heading, lines). What comes before the first one is the file's note.
fn sections<'a>(lines: &[&'a str]) -> Vec<(String, Vec<&'a str>)> {
    let mut out: Vec<(String, Vec<&str>)> = vec![];
    for line in lines {
        match line.trim().strip_prefix("## ") {
            Some(h) => out.push((h.trim().to_string(), vec![])),
            None => {
                if let Some(last) = out.last_mut() {
                    last.1.push(line);
                }
            }
        }
    }
    out
}

fn block(lines: &[&str]) -> String {
    lines.join("\n").trim().to_string()
}

/// A material: field lines (`- 角色：…`) up to the first point, then the points.
fn material(category: &str, title: &str, lines: &[&str]) -> Record {
    let mut r = Record { kind: "material".into(), category: category.into(), title: title.into(), ..Default::default() };
    let (mut bullets, mut text) = (vec![], vec![]);
    for line in lines.iter().filter(|l| !l.trim().is_empty()) {
        let Some(b) = bullet(line) else {
            text.push(line.trim().to_string());
            continue;
        };
        let field = bullets.is_empty().then(|| b.split_once('：').or_else(|| b.split_once(':'))).flatten();
        match field.map(|(k, v)| (k.trim(), v.trim().to_string())) {
            Some(("角色" | "职位" | "专业" | "学历", v)) => r.role = v,
            Some(("时间", v)) => r.time = v,
            Some(("地点", v)) => r.location = v,
            Some(("概览" | "说明", v)) => text.insert(0, v),
            Some(("技术栈", v)) => r.stack = v,
            _ => bullets.push(b.to_string()),
        }
    }
    r.text = text.into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n");
    r.points = points(&bullets);
    r
}

/// The records a file holds (sources not set yet), each with the materials it names (经历档案).
pub fn parse(path: &str, kind: &str, category: &str, text: &str) -> Vec<(Record, Vec<String>)> {
    let (title, lines) = split_heading(path, text);
    let plain = |r: Record| (r, vec![]);
    match kind {
        "material" => {
            let parts = sections(&lines);
            if parts.is_empty() {
                let r = material(category, &title, &lines);
                return if r.points.is_empty() && r.text.is_empty() && r.stack.is_empty() { vec![] } else { vec![plain(r)] };
            }
            parts.iter().map(|(h, body)| plain(material(category, h, body))).collect()
        }
        "application" => sections(&lines)
            .into_iter()
            .map(|(h, body)| {
                let text = body.iter().map(|l| bullet(l).unwrap_or(l.trim())).collect::<Vec<_>>().join("\n").trim().to_string();
                plain(Record { kind: kind.into(), category: category.into(), title: h, text, ..Default::default() })
            })
            .collect(),
        "profile" => {
            let mut r = Record { kind: kind.into(), category: category.into(), title, ..Default::default() };
            for line in lines {
                let t = bullet(line).unwrap_or(line.trim()).replace("**", "");
                if let Some((label, value)) = t.split_once('：').or_else(|| t.split_once(':')) {
                    r.fields.push(HeaderField { id: format!("f{}", r.fields.len() + 1), label: label.trim().into(), value: value.trim().into(), visible: true });
                }
            }
            if r.fields.is_empty() { vec![] } else { vec![plain(r)] }
        }
        "rule" => {
            let mut out = vec![];
            for (h, body) in sections(&lines) {
                if h.contains("个人约定") {
                    for b in body.iter().filter_map(|l| bullet(l)) {
                        let (name, text) = labelled(b).unwrap_or_else(|| (b.chars().take(12).collect(), b.to_string()));
                        out.push(plain(Record { kind: "decision".into(), category: category.into(), title: name, text, ..Default::default() }));
                    }
                    continue;
                }
                let first = body.iter().position(|l| !l.trim().is_empty());
                let used = first.and_then(|i| body[i].trim().strip_prefix("用于：").map(|u| (i, u.trim().to_string())));
                let (category, body) = match used {
                    Some((i, u)) if !u.is_empty() => (u, &body[i + 1..]),
                    _ => (category.to_string(), &body[..]),
                };
                let text = block(body);
                if !text.is_empty() {
                    out.push(plain(Record { kind: "rule".into(), category, title: h, text, ..Default::default() }));
                }
            }
            out
        }
        "facts" => {
            let mut out = vec![];
            for (group, body) in sections(&lines) {
                for b in body.iter().filter_map(|l| bullet(l)) {
                    let (name, text) = labelled(b).unwrap_or_else(|| (b.chars().take(12).collect(), b.to_string()));
                    out.push(plain(Record { kind: "fact".into(), category: group.clone(), title: name, text, ..Default::default() }));
                }
            }
            out
        }
        "dossier" => {
            let mut links = vec![];
            let mut body = vec![];
            for line in &lines {
                match bullet(line).and_then(|b| b.strip_prefix("素材：")).filter(|_| !body.iter().any(|l: &&str| l.trim().starts_with("## "))) {
                    Some(name) => links.push(name.trim().to_string()),
                    None => body.push(*line),
                }
            }
            let text = block(&body);
            if text.is_empty() { vec![] } else { vec![(Record { kind: "fact".into(), category: category.into(), title, text, ..Default::default() }, links)] }
        }
        _ => vec![],
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
    let mut out = Preview { items: vec![], gone: vec![], skipped: vec![] };
    let mut all: Vec<(Record, Vec<String>, Option<Record>)> = vec![];
    for path in files {
        let relative = path.strip_prefix(root).map_err(err)?.to_string_lossy().into_owned();
        let (kind, category, lang) = match classify(&relative) {
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
        let mut seen = HashMap::<String, usize>::new();
        for (mut incoming, links) in parse(&relative, kind, category, &text) {
            let n = seen.entry(incoming.title.clone()).or_default();
            *n += 1;
            let source = format!("{relative}#{}:{n}", incoming.title);
            let current = current.iter().find(|r| r.source.as_deref() == Some(source.as_str())).cloned();
            incoming.lang = lang.into();
            match &current {
                // the file's text, keeping what the user set here (category, language, on / off, links)
                Some(old) => (incoming.id, incoming.revision, incoming.category, incoming.lang, incoming.enabled, incoming.material, incoming.material_en) = (old.id.clone(), old.revision, old.category.clone(), old.lang.clone(), old.enabled, old.material.clone(), old.material_en.clone()),
                None => incoming.id = workflow::id()?,
            }
            incoming.source = Some(source);
            all.push((incoming, links, current));
        }
    }
    // a 经历档案 names its materials: ones in these files, else ones already here (by language)
    let materials: Vec<(String, String, String)> = all.iter().map(|(r, _, _)| r).chain(current.iter()).filter(|r| r.kind == "material").map(|r| (r.title.clone(), r.id.clone(), r.lang.clone())).collect();
    for (incoming, links, current) in all.iter_mut() {
        for name in links.iter() {
            match materials.iter().find(|(t, _, _)| t == name) {
                Some((_, id, lang)) if lang == "en" => incoming.material_en = Some(id.clone()),
                Some((_, id, _)) => incoming.material = Some(id.clone()),
                None => {}
            }
        }
        if current.as_ref() != Some(incoming) {
            out.items.push(ImportItem { incoming: incoming.clone(), current: current.clone() });
        }
    }
    let sources: std::collections::HashSet<&str> = all.iter().filter_map(|(r, _, _)| r.source.as_deref()).collect();
    out.gone = current.iter().filter(|r| r.source.as_deref().is_some_and(|s| !sources.contains(s))).cloned().collect();
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

/// Saves the chosen items and removes the chosen gone ones (their last version stays in the history).
pub fn confirm(c: &Connection, records: &[Record], remove: &[String]) -> Result<(), String> {
    let tx = Tx::begin(c)?;
    for record in records {
        catalog::put(&tx, record)?;
    }
    for id in remove {
        catalog::delete(&tx, id)?;
    }
    tx.commit()
}

#[tauri::command]
pub async fn picky_seeker_import_confirm(state: State<'_, AppState>, records: Vec<Record>, remove: Vec<String>) -> Result<Workspace, String> {
    state.with_picky_undo("导入 Seeker 资料", |c| {
        confirm(c, &records, &remove)?;
        read(c)
    })
}
