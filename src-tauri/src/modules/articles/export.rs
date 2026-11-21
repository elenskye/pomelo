//! 见字如面's export (v1.0 Phase 4). Rules:
//! - What: one article, the selected ones, a tag (the tag's list: sub-tags too, not archived), or all (archived
//!   too, never the trash). A locked one goes only while 芝麻开门 is open (its text then); else it's listed as left.
//! - Where: one article goes straight into the chosen folder; more go into folders by tag — an article with several
//!   tags under its first tag's deepest level (#读书 and #读书/小说 → 读书/小说; unrelated tags: the one the text has
//!   first), never twice; untagged ones in the folder itself. Names are titles (无标题 when empty; / : \ → -); a name
//!   taken there (on disk, by a tag folder, or by another of this export) gets 2, 3… Nothing on disk is overwritten.
//! - Attachments: next to each file a folder of the same name with the article's files (the originals); Markdown's
//!   links point into it (relative paths), so the whole folder can be moved and still opens.
//! - Markdown: the text as is (Bear's marks, fold marks, [[links]], tags); only the links to its files change, and a
//!   note set in (`![[念念不忘/…]]`) becomes a quote of it (its text, then its time; links.rs `quote_embeds`), in every format.
//! - PDF / Word: drawn by the print page (print.rs) one article at a time; Rust writes what it's given to the
//!   planned path. Style, contents, header and page numbers, tags and date are the page's to draw.

use super::import::{relink, Left};
use super::{get, list, Filter};
use crate::modules::attachments::{self, FileInfo};
use crate::modules::tags;
use crate::modules::vault::Key;
use crate::AppState;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::State;

const NAME_MAX: usize = 80;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    /// One or the selected ones, in this order.
    #[serde(default)]
    pub ids: Vec<i64>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    /// md | pdf | docx
    pub format: String,
    /// pomelo (柚儿园) | print (印刷)
    pub style: String,
    /// 目录
    pub toc: bool,
    /// 页眉和页码
    pub header: bool,
    /// 标签和日期
    pub meta: bool,
    /// The folder chosen.
    pub dest: String,
}

impl Options {
    pub fn ext(&self) -> Result<&'static str, String> {
        match self.format.as_str() {
            "md" => Ok("md"),
            "pdf" => Ok("pdf"),
            "docx" => Ok("docx"),
            f => Err(format!("不认识的格式：{f}")),
        }
    }
}

/// One article on its way out.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Planned {
    pub id: i64,
    pub title: String,
    pub content: String,
    /// In the text's order.
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub files: Vec<FileInfo>,
    /// The file to write.
    pub path: PathBuf,
    /// The folder of its attachments (when it has any).
    pub assets: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub written: usize,
    /// The folder they went into.
    pub folder: String,
    /// The file, when there was one.
    pub file: Option<String>,
    pub left: Vec<Left>,
}

/// The articles a target names, readable (a locked one with `key`); the locked ones without it are left out.
pub fn pick(conn: &Connection, target: &Target, key: Option<&Key>) -> Result<(Vec<super::Article>, Vec<Left>), String> {
    let ids: Vec<i64> = if !target.ids.is_empty() {
        target.ids.clone()
    } else if let Some(t) = &target.tag {
        list(conn, &Filter { tag: Some(t.clone()), ..Default::default() })?.into_iter().map(|s| s.id).collect()
    } else if target.all {
        let mut v: Vec<i64> = list(conn, &Filter { view: Some("all".into()), ..Default::default() })?.into_iter().map(|s| s.id).collect();
        v.extend(list(conn, &Filter { view: Some("archived".into()), ..Default::default() })?.into_iter().map(|s| s.id));
        v
    } else {
        vec![]
    };
    let mut out = Vec::new();
    let mut left = Vec::new();
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        let a = get(conn, id, key)?;
        if a.trashed_at.is_some() && target.ids.is_empty() {
            continue;
        }
        if a.locked && !a.open {
            left.push(Left { title: a.title.clone(), what: "加密的：芝麻开门锁着，没导出".into() });
            continue;
        }
        let content = crate::modules::links::quote_embeds(conn, &a.content)?;
        out.push(super::Article { content, ..a });
    }
    if out.is_empty() && left.is_empty() {
        return Err("没有要导出的笔记".into());
    }
    Ok((out, left))
}

/// A title as a file name.
pub fn safe_name(s: &str) -> String {
    let s: String = s.chars().map(|c| if matches!(c, '/' | ':' | '\\') { '-' } else if c.is_control() { ' ' } else { c }).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s: String = s.trim_start_matches('.').trim().chars().take(NAME_MAX).collect();
    let s = s.trim().to_string();
    if s.is_empty() {
        "无标题".into()
    } else {
        s
    }
}

/// The tag folder of an article with these tags (in the text's order): the first tag's deepest level.
pub fn folder_tag(tags: &[String]) -> Option<String> {
    let first = tags.first()?;
    let mut best = first;
    for t in tags {
        if tags::under(t, first) && t.matches('/').count() > best.matches('/').count() {
            best = t;
        }
    }
    Some(best.clone())
}

/// Where each article goes (several: by tag) and its attachments' folder; nothing written yet.
pub fn plan(articles: Vec<super::Article>, dest: &Path, ext: &str) -> Vec<Planned> {
    let several = articles.len() > 1;
    let rel = |a: &super::Article| -> Vec<String> {
        if !several {
            return vec![];
        }
        folder_tag(&tags::tags_in(&a.content, true).into_iter().chain(a.tags.iter().cloned()).collect::<Vec<_>>()).map(|t| t.split('/').map(safe_name).collect()).unwrap_or_default()
    };
    // names taken per folder (lower case: the disk ignores case), tag folders first
    let mut taken: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let folders: Vec<Vec<String>> = articles.iter().map(rel).collect();
    for f in &folders {
        let mut at = dest.to_path_buf();
        for seg in f {
            taken.entry(at.clone()).or_default().insert(seg.to_lowercase());
            at.push(seg);
        }
    }
    let mut out = Vec::new();
    for (a, f) in articles.into_iter().zip(folders) {
        let dir = f.iter().fold(dest.to_path_buf(), |p, s| p.join(s));
        let files: Vec<FileInfo> = a.attachments.clone();
        let has_assets = !files.is_empty();
        let base = safe_name(&a.title);
        let names = taken.entry(dir.clone()).or_default();
        let mut n = 1;
        let stem = loop {
            let cand = if n == 1 { base.clone() } else { format!("{base} {n}") };
            let file = format!("{cand}.{ext}");
            let free = !names.contains(&file.to_lowercase()) && !dir.join(&file).exists() && (!has_assets || (!names.contains(&cand.to_lowercase()) && !dir.join(&cand).exists()));
            if free {
                names.insert(file.to_lowercase());
                if has_assets {
                    names.insert(cand.to_lowercase());
                }
                break cand;
            }
            n += 1;
        };
        let tags = if a.locked { a.tags.clone() } else { tags::tags_in(&a.content, true) };
        out.push(Planned {
            id: a.id,
            title: a.title,
            content: a.content,
            tags,
            created_at: a.created_at,
            updated_at: a.updated_at,
            path: dir.join(format!("{stem}.{ext}")),
            assets: has_assets.then(|| dir.join(&stem)),
            files,
        });
    }
    out
}

fn mkdirs(p: &Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("没能建立文件夹「{}」：{e}", p.display()))
}

/// Writes a file that mustn't be there yet.
pub fn write_new(path: &Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    if let Some(p) = path.parent() {
        mkdirs(p)?;
    }
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("没能写「{}」：{e}", path.display()))?;
    f.write_all(data).map_err(|e| format!("没能写「{}」：{e}", path.display()))
}

/// Copies an article's files into its folder (names kept; a repeat gets 2); returns the files missing here.
pub fn copy_files(conn: &Connection, attachments_dir: &Path, p: &Planned) -> Result<Vec<Left>, String> {
    let Some(dir) = &p.assets else { return Ok(vec![]) };
    mkdirs(dir)?;
    let mut left = Vec::new();
    let mut used = HashSet::new();
    for f in &p.files {
        let src = if f.missing { Err("这个附件不在这台 Mac 上".to_string()) } else { attachments::path_of(conn, attachments_dir, f.id) };
        match src {
            Ok(src) => {
                let (stem, ext) = match f.name.rsplit_once('.') {
                    Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
                    _ => (f.name.clone(), String::new()),
                };
                let mut name = f.name.clone();
                for n in 2.. {
                    if used.insert(name.to_lowercase()) && !dir.join(&name).exists() {
                        break;
                    }
                    name = format!("{stem} {n}{ext}");
                }
                std::fs::copy(&src, dir.join(&name)).map_err(|e| format!("没能拷贝附件「{}」：{e}", f.name))?;
            }
            Err(e) => left.push(Left { title: p.title.clone(), what: format!("附件「{}」：{e}", f.name) }),
        }
    }
    Ok(left)
}

/// The text with its file links pointing into its folder (`名字/文件`).
pub fn markdown_of(p: &Planned) -> String {
    let Some(dir) = &p.assets else { return p.content.clone() };
    let folder = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let names: HashSet<&str> = p.files.iter().map(|f| f.name.as_str()).collect();
    relink(&p.content, |t| names.contains(t).then(|| format!("{folder}/{t}")))
}

/// Writes the Markdown files (and every format's attachment folders).
pub fn write_markdown(conn: &Connection, attachments_dir: &Path, planned: &[Planned], report: &mut ExportReport) -> Result<(), String> {
    for p in planned {
        report.left.extend(copy_files(conn, attachments_dir, p)?);
        write_new(&p.path, markdown_of(p).as_bytes())?;
        report.written += 1;
    }
    Ok(())
}

/// ~/Downloads/见字如面 (the dialog's first choice).
pub fn default_dest(home: &Path) -> PathBuf {
    home.join("Downloads").join("见字如面")
}

pub fn check_dest(dest: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(dest.trim());
    if !p.is_absolute() {
        return Err("先选一个存到哪的文件夹".into());
    }
    if p.exists() && !p.is_dir() {
        return Err("存到的地方不是文件夹".into());
    }
    mkdirs(&p)?;
    Ok(p)
}

pub fn report_folder(planned: &[Planned], dest: &Path, report: &mut ExportReport) {
    report.folder = dest.display().to_string();
    if planned.len() == 1 {
        report.file = Some(planned[0].path.display().to_string());
    }
}

// ---------- commands ----------

/// Exports (Markdown here; PDF and Word through the print page). Not an edit: no ⌘Z.
#[tauri::command]
pub async fn articles_export(app: tauri::AppHandle, target: Target, options: Options) -> Result<ExportReport, String> {
    export(&app, target, options).await
}

pub async fn export(app: &tauri::AppHandle, target: Target, options: Options) -> Result<ExportReport, String> {
    use tauri::Manager;
    let state: State<'_, AppState> = app.state();
    let ext = options.ext()?;
    let dest = check_dest(&options.dest)?;
    let key = state.vault_key().ok();
    let (articles, left) = state.with_db(|c| pick(c, &target, key.as_ref()))?;
    let planned = plan(articles, &dest, ext);
    let mut report = ExportReport { left, ..Default::default() };
    report_folder(&planned, &dest, &mut report);
    let dir = attachments::folder_of(&state.paths.root);
    if options.format == "md" {
        state.with_db(|c| write_markdown(c, &dir, &planned, &mut report))?;
        return Ok(report);
    }
    state.with_db(|c| {
        for p in &planned {
            report.left.extend(copy_files(c, &dir, p)?);
        }
        Ok(())
    })?;
    let (written, more) = crate::print::run(app, planned, options).await?;
    report.written = written;
    report.left.extend(more);
    Ok(report)
}

/// A scratch data dir only (POMELO_DATA_DIR; testing.md「真实窗口」): POMELO_TRY_EXPORT = {"target": …, "options": …}
/// (or a list of them) exports once the window is up; the reports go to <data>/export-try.json.
pub fn try_on_start(app: &tauri::AppHandle, root: &Path) {
    let Ok(spec) = std::env::var("POMELO_TRY_EXPORT") else { return };
    let (app, out) = (app.clone(), root.join("export-try.json"));
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let specs: Vec<serde_json::Value> = match serde_json::from_str(&spec) {
            Ok(serde_json::Value::Array(v)) => v,
            Ok(v) => vec![v],
            Err(e) => vec![serde_json::json!({ "error": e.to_string() })],
        };
        let mut all = Vec::new();
        for v in specs {
            let r = async {
                let target: Target = serde_json::from_value(v["target"].clone()).map_err(|e| e.to_string())?;
                let options: Options = serde_json::from_value(v["options"].clone()).map_err(|e| e.to_string())?;
                export(&app, target, options).await
            }
            .await;
            all.push(r.map(|r| serde_json::to_value(r).unwrap_or_default()).unwrap_or_else(|e| serde_json::json!({ "error": e })));
        }
        let _ = std::fs::write(out, serde_json::to_string_pretty(&all).unwrap_or_default());
    });
}

/// ~/Downloads/见字如面
#[tauri::command]
pub fn articles_export_dir(app: tauri::AppHandle) -> Result<String, String> {
    use tauri::Manager;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    Ok(default_dest(&home).display().to_string())
}

/// Shows an exported file (or folder) in Finder.
#[tauri::command]
pub fn articles_export_reveal(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err("找不到导出的文件了".into());
    }
    app.opener().reveal_item_in_dir(p).map_err(|e| format!("没能在访达里显示：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::modules::articles::{create, tests::add, update, Patch};

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("pomelo-export-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn folders_names_and_files() {
        assert_eq!(folder_tag(&["读书".into(), "读书/小说".into(), "旅行".into()]), Some("读书/小说".into()));
        assert_eq!(folder_tag(&["旅行".into(), "读书/小说".into()]), Some("旅行".into()));
        assert_eq!(folder_tag(&[]), None);
        assert_eq!(safe_name(" a/b:c\n "), "a-b-c");
        assert_eq!(safe_name(".."), "无标题");
        let c = memory_db();
        let att = tmp();
        let pic = attachments::store(&c, &att, "图 1.png", b"\x89PNG x").unwrap();
        let a = create(&c, Patch { content: Some(format!("# 读书\n#读书 #读书/小说\n![]({})", super::super::encode_name(&pic.name))), attachments: Some(vec![pic.id]), ..Default::default() }).unwrap().id;
        let b = add(&c, "# 读书\n#读书/小说 同名的一篇");
        let u = add(&c, "# 读书\n没有标签");
        let z = add(&c, "# 归档的\n#旅行");
        update(&c, z, Patch { archived: Some(true), ..Default::default() }, None).unwrap();
        let w = add(&c, "# 扔掉的");
        crate::modules::articles::trash(&c, w).unwrap();
        // all: archived too, never the trash
        let (all, left) = pick(&c, &Target { all: true, ..Default::default() }, None).unwrap();
        assert_eq!((all.len(), left.len()), (4, 0));
        let dest = tmp();
        std::fs::create_dir_all(dest.join("旅行")).unwrap();
        std::fs::write(dest.join("旅行/归档的.md"), "已经有了").unwrap();
        let planned = plan(all, &dest, "md");
        let path = |id: i64| planned.iter().find(|p| p.id == id).unwrap().path.strip_prefix(&dest).unwrap().display().to_string();
        // the newer first (the list's order): the second one of a name gets 2, its files folder too
        assert_eq!(path(b), "读书/小说/读书.md");
        assert_eq!(path(a), "读书/小说/读书 2.md");
        // an untagged one with no files may share a tag folder's name
        assert_eq!(path(u), "读书.md");
        assert_eq!(path(z), "旅行/归档的 2.md");
        let mut report = ExportReport::default();
        write_markdown(&c, &att, &planned, &mut report).unwrap();
        assert_eq!(report.written, 4);
        let md = std::fs::read_to_string(dest.join("读书/小说/读书 2.md")).unwrap();
        assert!(md.ends_with("![](读书%202/图%201.png)"), "{md}");
        assert_eq!(std::fs::read(dest.join("读书/小说/读书 2/图 1.png")).unwrap(), b"\x89PNG x");
        assert_eq!(std::fs::read_to_string(dest.join("旅行/归档的.md")).unwrap(), "已经有了");
        // one goes straight in; a tag: its list
        let (one, _) = pick(&c, &Target { ids: vec![a], ..Default::default() }, None).unwrap();
        // (its files' folder would be the tag folder 读书 there)
        assert_eq!(plan(one, &dest, "pdf")[0].path, dest.join("读书 2.pdf"));
        let (t, _) = pick(&c, &Target { tag: Some("读书".into()), ..Default::default() }, None).unwrap();
        assert_eq!(t.len(), 2);
        // locked: only with the key
        let k = crate::modules::vault::test_key();
        crate::modules::articles::set_locked(&c, u, true, &k).unwrap();
        let (_, left) = pick(&c, &Target { ids: vec![u, b], ..Default::default() }, None).unwrap();
        assert_eq!(left.len(), 1);
        let (got, _) = pick(&c, &Target { ids: vec![u], ..Default::default() }, Some(&k)).unwrap();
        assert!(got[0].content.contains("没有标签"));
        assert!(pick(&c, &Target::default(), None).is_err());
    }
}
