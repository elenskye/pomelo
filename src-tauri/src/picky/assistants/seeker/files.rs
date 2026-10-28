//! cv/ and material/ of the Seeker folder (v0.7 阶段 ⑦): read in place, never changed or moved.
//! - cv/: the resumes (.docx, .pdf; .doc / .pages listed but not read) by language folder (cn/, en/;
//!   else Chinese when the name has CJK). Versions of one resume are one `series`: the name without
//!   its version (`-v3`, ` v3`, `_V3`) or date (`-2026-10-28`, `20261028`) at the end. Tagged: 模板 (a
//!   template Seeker uses), Pomelo 导出 (an export's path), and the 投石问路 jobs whose company the
//!   series names (company.rs `matched` with the series' words as names: `keyreply` ↔ 「KeyReply」).
//! - Its text (`text`): a .docx's paragraphs, a .pdf's pages — only inside <Seeker>/cv/.
//! - material/ (证书原件): file names by sub-folder only; Pomelo never opens them for a model.
//! - 小拾 reads the list and a resume's text through 「Seeker 资料」 (skills/seeker.rs); cv/ can go
//!   into the 知识库 「Seeker」 like knowledge/ (company.rs).
//! - The real Seeker folder is out of bounds in mock mode (output.rs `allowed_path`).
use super::*;
use std::path::{Path, PathBuf};

const RESUME: [&str; 4] = ["docx", "pdf", "doc", "pages"];

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CvFile {
    pub path: String,
    /// Under cv/ (`en/seek-it-keyreply-resume-v5.docx`).
    pub name: String,
    pub lang: String,
    pub series: String,
    pub version: Option<u32>,
    /// Seconds since 1970.
    pub modified: i64,
    pub size: u64,
    pub template: bool,
    pub export: bool,
    /// 投石问路 jobs the series names (「KeyReply · 软件开发实习」).
    pub jobs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialFile {
    pub path: String,
    pub name: String,
    /// The sub-folder (awards, certificates…; "" at the top).
    pub group: String,
    pub size: u64,
    pub modified: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Files {
    pub cv: Vec<CvFile>,
    pub material: Vec<MaterialFile>,
    /// transfer/ is still there (the import is offered).
    pub transfer: bool,
    pub error: Option<String>,
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        let hidden = p.file_name().and_then(|n| n.to_str()).is_none_or(|n| n.starts_with('.') || n.starts_with("~$"));
        if hidden {
            continue;
        }
        if p.is_dir() {
            if depth > 0 {
                walk(&p, depth - 1, out);
            }
        } else {
            out.push(p);
        }
    }
}

fn stamp(p: &Path) -> (u64, i64) {
    let m = std::fs::metadata(p).ok();
    let size = m.as_ref().map_or(0, |m| m.len());
    let modified = m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
    (size, modified)
}

/// A file name's resume and version: `seek-it-keyreply-resume-v5` → (`seek-it-keyreply-resume`, 5);
/// a date at the end goes too.
pub fn series(stem: &str) -> (String, Option<u32>) {
    let mut s = stem.trim().to_string();
    let mut version = None;
    for _ in 0..2 {
        // …-v3 / … v3 / …_V3
        let lower = s.to_lowercase();
        if let Some(i) = lower.rfind('v').filter(|i| *i > 0 && s.len() > i + 1 && s[i + 1..].chars().all(|c| c.is_ascii_digit()) && s[..*i].ends_with(['-', '_', ' '])) {
            version = s[i + 1..].parse().ok();
            s = s[..i].trim_end_matches(['-', '_', ' ']).to_string();
            continue;
        }
        // …-2026-10-28 / …20261028
        let digits: String = s.chars().rev().take_while(|c| c.is_ascii_digit() || *c == '-').collect::<Vec<_>>().into_iter().rev().collect();
        if digits.chars().filter(char::is_ascii_digit).count() == 8 && s.len() > digits.len() {
            s = s[..s.len() - digits.len()].trim_end_matches(['-', '_', ' ']).to_string();
            continue;
        }
        break;
    }
    (s, version)
}

fn cjk(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x4E00..=0x9FFF))
}

/// What a series names, for matching a job's company: its words but the common ones.
fn words(series: &str) -> Vec<String> {
    const COMMON: [&str; 9] = ["seek", "it", "resume", "cv", "简历", "en", "cn", "final", "new"];
    series.split(|c: char| !c.is_alphanumeric()).map(str::to_lowercase).filter(|w| w.chars().count() >= 2 && !COMMON.contains(&w.as_str())).collect()
}

/// cv/ and material/ of `root` (the Seeker folder). `templates`, `exports`: paths Seeker knows;
/// `jobs`: (label, company).
pub fn scan(root: &Path, templates: &[String], exports: &[String], jobs: &[(String, String)]) -> Files {
    let same = |a: &Path, b: &str| std::fs::canonicalize(a).ok().zip(std::fs::canonicalize(b).ok()).is_some_and(|(x, y)| x == y);
    let mut files = Files { transfer: root.join("transfer").is_dir(), ..Default::default() };
    let cv = root.join("cv");
    let mut found = vec![];
    walk(&cv, 3, &mut found);
    for p in found {
        let ext = p.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
        if !RESUME.contains(&ext.as_str()) {
            continue;
        }
        let name = p.strip_prefix(&cv).unwrap_or(&p).to_string_lossy().into_owned();
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let lang = match name.split('/').next() {
            Some("cn") | Some("zh") => "cn",
            Some("en") => "en",
            _ if cjk(stem) => "cn",
            _ => "en",
        };
        let (series, version) = series(stem);
        let named: Vec<company::Company> = vec![company::Company { folder: series.clone(), name: series.clone(), names: words(&series) }];
        let jobs = jobs.iter().filter(|(_, company)| !named[0].names.is_empty() && company::matched(company, &named).is_some()).map(|(label, _)| label.clone()).collect();
        let (size, modified) = stamp(&p);
        let path = p.to_string_lossy().into_owned();
        files.cv.push(CvFile { template: templates.iter().any(|t| same(&p, t)), export: exports.iter().any(|e| same(&p, e)), path, name, lang: lang.into(), series, version, modified, size, jobs });
    }
    // Chinese first; by series, its newest version first
    let order = |x: &CvFile| (x.lang != "cn", x.series.to_lowercase(), std::cmp::Reverse(x.version), std::cmp::Reverse(x.modified));
    files.cv.sort_by_key(order);
    let material = root.join("material");
    let mut found = vec![];
    walk(&material, 3, &mut found);
    for p in found {
        let rel = p.strip_prefix(&material).unwrap_or(&p);
        let group = if rel.components().count() > 1 { rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default() } else { String::new() };
        let (size, modified) = stamp(&p);
        files.material.push(MaterialFile { path: p.to_string_lossy().into_owned(), name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), group, size, modified });
    }
    files
}

/// The text of one resume under `root`/cv/ (paragraphs or pages, blank lines between).
pub fn text(root: &Path, path: &Path) -> Result<String, String> {
    let cv = std::fs::canonicalize(root.join("cv")).map_err(|_| "Seeker 文件夹里没有 cv/".to_string())?;
    let p = std::fs::canonicalize(path).map_err(|_| "找不到这份简历".to_string())?;
    if !p.starts_with(&cv) {
        return Err("只能读 cv/ 里的简历".into());
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("读不了：{e}"))?;
    match p.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
        Some("docx") => Ok(crate::picky::word::read_docx(&bytes)?.items.iter().map(|i| i.text.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n")),
        Some("pdf") => Ok(crate::picky::pdf::pages(&bytes)?.join("\n\n")),
        _ => Err("只能读 Word（.docx）和 PDF".into()),
    }
}

/// The Seeker folder Pomelo may read now (none chosen: None; the real one in mock mode: an error).
pub fn root(c: &Connection, mode: crate::picky::Mode) -> Result<Option<PathBuf>, String> {
    let folder: String = c.query_row("SELECT folder FROM seeker_settings WHERE id = 1", [], |r| r.get(0)).optional().map_err(err)?.unwrap_or_default();
    if folder.trim().is_empty() {
        return Ok(None);
    }
    output::allowed_path(Path::new(folder.trim()), mode).map(Some)
}

/// The list for the pages and 小拾: templates and exports from picky.db, jobs from pomelo.db (read
/// apart, never nested).
pub fn list(state: &AppState) -> Files {
    let read = state.picky.with_db(|c| {
        let w = read(c)?;
        let templates: Vec<String> = [w.output.template.as_ref(), w.output.template_en.as_ref()].into_iter().flatten().map(|t| t.path.clone()).collect();
        Ok((root(c, state.picky.mode)?, templates, w.output.exports.iter().map(|e| e.path.clone()).collect::<Vec<_>>()))
    });
    let (root, templates, exports) = match read {
        Ok((Some(r), t, e)) => (r, t, e),
        Ok((None, ..)) => return Files::default(),
        Err(e) => return Files { error: Some(e), ..Default::default() },
    };
    let jobs: Vec<(String, String)> = state
        .with_db(|c| {
            let mut st = c.prepare("SELECT company, position FROM jobs ORDER BY id").map_err(err)?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
            Ok(rows.into_iter().map(|(c, p)| (p.map_or(c.clone(), |p| format!("{c} · {p}")), c)).collect())
        })
        .unwrap_or_default();
    scan(&root, &templates, &exports, &jobs)
}

#[tauri::command]
pub async fn picky_seeker_files(state: State<'_, AppState>) -> Result<Files, String> {
    Ok(list(&state))
}

#[tauri::command]
pub async fn picky_seeker_cv_text(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let root = state.picky.with_db(|c| root(c, state.picky.mode))?.ok_or("还没选 Seeker 文件夹")?;
    text(&root, Path::new(&path))
}
