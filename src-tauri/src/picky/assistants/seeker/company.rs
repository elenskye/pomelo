//! 公司资料 (v0.7 阶段 ②): `<Seeker>/knowledge/` added (by the user, once) as a folder of a 小拾 知识库
//! (read in place: never copied, changed or imported). Each sub-folder is one company (`seek-lenovo`).
//! - A company's names: the folder name's parts that not every folder shares (`lenovo`, not `seek`),
//!   and from the Markdown files at the top of knowledge/ (招聘索引.md): the text of a link into the
//!   folder (`[联想](seek-lenovo/公司与招聘.md)`), or in a table row with such a link, its first cell.
//!   Shown by its first index name (else the folder name), or the name the user gave it in Seeker's
//!   设置 (`Settings::company_names`, folder → name; not undone, like the other settings): that name
//!   is shown and matched first.
//! - A job matches a company when one of the names is in the job's company or the other way round
//!   (letters, digits and CJK; a Latin name only as a whole word, ≥ 2 characters). Several: the
//!   longest name wins; a tie is no match. The user's choice for the job (`seeker_job_companies`,
//!   undoable, '' = none) wins over the match. Jobs are read from pomelo.db first, never nested.
//! - 选材 and 审查 (model.rs) search the job's company folder with the JD (`lookup`).
//! - cv/ (the past resumes, files.rs) can go into the same 知识库 the same way (`part` cv).
use super::*;
use crate::picky::index::Passage;
use crate::picky::knowledge;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// The 知识库 library Seeker's knowledge/ goes into when the user adds it (made if missing).
pub const LIBRARY: &str = "Seeker";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Company {
    pub folder: String,
    pub name: String,
    /// The name found by itself (index or folder), when the user hasn't given one.
    pub auto: String,
    #[serde(skip)]
    pub names: Vec<String>,
}

/// The user's choice of company folder for a job ('' = none).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub job_id: i64,
    pub job_created_at: String,
    pub folder: String,
}

/// knowledge/ for the Seeker page: where it is, the 知识库 it is in, its companies.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Knowledge {
    pub path: Option<String>,
    pub library: Option<String>,
    pub companies: Vec<Company>,
    /// Why it can't be read (the real Seeker folder in a development run…).
    pub error: Option<String>,
    /// cv/ (the resumes): where it is and the 知识库 it is in.
    pub cv: Option<String>,
    pub cv_library: Option<String>,
}

/// One job's company folder: the match by name, the user's choice, and the companies to choose from.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobCompany {
    pub companies: Vec<Company>,
    pub auto: Option<String>,
    pub chosen: Option<String>,
}

impl JobCompany {
    pub fn folder(&self) -> Option<&str> {
        self.chosen.as_deref().or(self.auto.as_deref()).filter(|f| !f.is_empty())
    }
}

/// Lowercase letters, digits and CJK only.
fn key(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn latin(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The companies in `dir` (knowledge/), sorted by folder.
pub fn companies(dir: &Path) -> Result<Vec<Company>, String> {
    let mut folders: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("knowledge/ 打不开：{e}"))?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    folders.sort();
    let parts = |f: &str| f.split(['-', '_', ' ', '.']).map(key).filter(|p| p.chars().count() >= 2).collect::<Vec<_>>();
    let shared: Vec<String> = if folders.len() < 2 { vec![] } else { parts(&folders[0]).into_iter().filter(|p| folders.iter().all(|f| parts(f).contains(p))).collect() };
    let mut out: Vec<Company> = folders.iter().map(|f| Company { folder: f.clone(), name: String::new(), auto: String::new(), names: parts(f).into_iter().filter(|p| !shared.contains(p)).collect() }).collect();
    let mut index: Vec<PathBuf> = std::fs::read_dir(dir).map_err(err)?.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md"))).collect();
    index.sort();
    for path in index {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for (folder, name) in index_names(&text) {
            if let Some(c) = out.iter_mut().find(|c| c.folder == folder) {
                if c.name.is_empty() {
                    c.name = name.clone();
                }
                if !c.names.contains(&name) {
                    c.names.push(name);
                }
            }
        }
    }
    for c in &mut out {
        if c.name.is_empty() {
            c.name = c.folder.clone();
        }
        c.auto = c.name.clone();
    }
    Ok(out)
}

/// The user's names (folder → name) over the found ones: shown, and matched before the others.
pub fn named(mut list: Vec<Company>, names: &std::collections::BTreeMap<String, String>) -> Vec<Company> {
    for c in &mut list {
        if let Some(n) = names.get(&c.folder).map(|n| n.trim()).filter(|n| !n.is_empty()) {
            c.name = n.to_string();
            c.names.retain(|x| x != n);
            c.names.insert(0, n.to_string());
        }
    }
    list
}

/// The names the user gives (folder → name; blank: the found one again): one name per company.
pub fn clean_names(names: &std::collections::BTreeMap<String, String>, folders: &[String]) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut out = std::collections::BTreeMap::new();
    for (folder, name) in names {
        let name = name.trim();
        if name.is_empty() || !folders.contains(folder) {
            continue;
        }
        if name.chars().count() > 30 {
            return Err(format!("「{name}」太长了，公司名称最多 30 个字"));
        }
        if out.values().any(|n: &String| key(n) == key(name)) {
            return Err(format!("两家公司都叫「{name}」，请换一个名称"));
        }
        out.insert(folder.clone(), name.to_string());
    }
    Ok(out)
}

/// (folder, company name) pairs in an index file: a link's text, or in a table row its first cell.
pub fn index_names(text: &str) -> Vec<(String, String)> {
    let clean = |s: &str| unlink(s).replace(['*', '`'], "").trim().to_string();
    let mut out = vec![];
    for line in text.lines() {
        let row = line.trim_start().starts_with('|');
        let first = row.then(|| line.trim().trim_start_matches('|').split('|').next().map(clean).unwrap_or_default());
        let mut rest = line;
        while let Some(open) = rest.find('[') {
            let Some(close) = rest[open..].find("](").map(|i| open + i) else { break };
            let Some(end) = rest[close..].find(')').map(|i| close + i) else { break };
            let (label, target) = (&rest[open + 1..close], &rest[close + 2..end]);
            rest = &rest[end + 1..];
            if target.contains("://") || target.starts_with('#') {
                continue;
            }
            let Some(folder) = target.trim_start_matches("./").split('/').next().filter(|f| !f.is_empty() && target.contains('/')) else { continue };
            let name = first.clone().unwrap_or_else(|| clean(label));
            if !name.is_empty() && name.chars().count() <= 30 && !out.contains(&(folder.to_string(), name.clone())) {
                out.push((folder.to_string(), name));
            }
        }
    }
    out
}

/// `[text](target)` → text.
fn unlink(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|i| open + i) else { break };
        let Some(end) = rest[close..].find(')').map(|i| close + i) else { break };
        out.push_str(&rest[..open]);
        out.push_str(&rest[open + 1..close]);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The folder whose name best matches a job's company (None: none, or a tie).
pub fn matched(company: &str, companies: &[Company]) -> Option<String> {
    let job = key(company);
    let words: Vec<String> = company.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    let latin_key: String = company.chars().filter(char::is_ascii_alphanumeric).flat_map(|c| c.to_lowercase()).collect();
    let mut best: Option<(usize, &str)> = None;
    let mut tie = false;
    for c in companies {
        for name in &c.names {
            let n = key(name);
            let len = n.chars().count();
            if len < 2 {
                continue;
            }
            let hit = if latin(&n) {
                let seq: Vec<String> = name.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
                latin_key == n || (!seq.is_empty() && words.windows(seq.len()).any(|w| w == seq.as_slice()))
            } else {
                job.contains(&n) || (job.chars().count() >= 2 && n.contains(&job))
            };
            if !hit {
                continue;
            }
            match best {
                Some((l, f)) if l > len || (l == len && f == c.folder) => {}
                Some((l, _)) if l == len => tie = true,
                _ => {
                    best = Some((len, &c.folder));
                    tie = false;
                }
            }
        }
    }
    best.filter(|_| !tie).map(|(_, f)| f.to_string())
}

pub fn load(c: &Connection) -> Result<Vec<Choice>, String> {
    let mut st = c.prepare("SELECT job_id, job_created_at, folder FROM seeker_job_companies ORDER BY rowid").map_err(err)?;
    let rows = st.query_map([], |r| Ok(Choice { job_id: r.get(0)?, job_created_at: r.get(1)?, folder: r.get(2)? })).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

pub fn replace(c: &Connection, choices: &[Choice]) -> Result<(), String> {
    for ch in choices {
        c.execute("INSERT INTO seeker_job_companies (job_id, job_created_at, folder) VALUES (?1, ?2, ?3)", params![ch.job_id, ch.job_created_at, ch.folder]).map_err(|_| "Seeker：岗位的公司资料重复了".to_string())?;
    }
    Ok(())
}

/// None: back to the match by name.
pub fn choose(c: &Connection, job: &output::JobSnapshot, folder: Option<&str>) -> Result<(), String> {
    match folder {
        None => c.execute("DELETE FROM seeker_job_companies WHERE job_id = ?1 AND job_created_at = ?2", params![job.id, job.created_at]),
        Some(f) => c.execute("INSERT INTO seeker_job_companies (job_id, job_created_at, folder) VALUES (?1, ?2, ?3) ON CONFLICT DO UPDATE SET folder = excluded.folder", params![job.id, job.created_at, f]),
    }
    .map_err(err)?;
    Ok(())
}

/// The remembered Seeker folder ("": none).
fn seeker_folder(c: &Connection) -> Result<String, String> {
    Ok(c.query_row("SELECT folder FROM seeker_settings WHERE id = 1", [], |r| r.get(0)).optional().map_err(err)?.unwrap_or_default())
}

/// knowledge/ of the Seeker folder, if there is one (the real one is out of bounds in mock mode).
pub fn dir(folder: &str, mode: crate::picky::Mode) -> Result<Option<PathBuf>, String> {
    sub(folder, "knowledge", mode)
}

/// A sub-folder (knowledge/, cv/) of the Seeker folder, if there is one.
pub fn sub(folder: &str, part: &str, mode: crate::picky::Mode) -> Result<Option<PathBuf>, String> {
    if folder.trim().is_empty() {
        return Ok(None);
    }
    let dir = Path::new(folder.trim()).join(part);
    if !dir.is_dir() {
        return Ok(None);
    }
    output::allowed_path(&dir, mode).map(Some)
}

/// The 知识库 source that is this folder, with its library's name.
fn source(c: &Connection, dir: &Path) -> Result<Option<(knowledge::Source, String)>, String> {
    let libraries = knowledge::libraries(c)?;
    Ok(knowledge::sources(c, None)?.into_iter().find(|s| s.path.as_deref().is_some_and(|p| Path::new(p) == dir)).map(|s| {
        let name = libraries.iter().find(|l| l.id == s.library_id).map(|l| l.name.clone()).unwrap_or_default();
        (s, name)
    }))
}

fn knowledge_view(state: &AppState) -> Knowledge {
    let folder = match state.picky.with_db(|c| seeker_folder(c)) {
        Ok(f) => f,
        Err(e) => return Knowledge { error: Some(e), ..Default::default() },
    };
    let cv = sub(&folder, "cv", state.picky.mode).ok().flatten();
    let cv_library = cv.as_ref().and_then(|d| state.picky.with_db(|c| source(c, d)).ok().flatten()).map(|(_, name)| name);
    let cv = cv.map(|d| d.to_string_lossy().into_owned());
    let dir = match dir(&folder, state.picky.mode) {
        Ok(Some(d)) => d,
        Ok(None) => return Knowledge { cv, cv_library, ..Default::default() },
        Err(e) => return Knowledge { error: Some(e), ..Default::default() },
    };
    let names = state.picky.with_db(|c| settings_of(c)).map(|s| s.company_names).unwrap_or_default();
    let (companies, error) = match companies(&dir) {
        Ok(list) => (named(list, &names), None),
        Err(e) => (vec![], Some(e)),
    };
    let library = state.picky.with_db(|c| source(c, &dir)).ok().flatten().map(|(_, name)| name);
    Knowledge { path: Some(dir.to_string_lossy().into_owned()), library, companies, error, cv, cv_library }
}

#[tauri::command]
pub async fn picky_seeker_knowledge(state: State<'_, AppState>) -> Result<Knowledge, String> {
    Ok(knowledge_view(&state))
}

/// Names knowledge/'s companies (folder → name; blank: the found name). A setting: not undone.
#[tauri::command]
pub async fn picky_seeker_company_names(state: State<'_, AppState>, names: std::collections::BTreeMap<String, String>) -> Result<Knowledge, String> {
    let view = knowledge_view(&state);
    let folders: Vec<String> = view.companies.iter().map(|c| c.folder.clone()).collect();
    let names = clean_names(&names, &folders)?;
    state.picky.with_db(|c| {
        let settings = catalog::Settings { company_names: names, ..settings_of(c)? };
        set_settings(c, "options", &serde_json::to_string(&settings).map_err(err)?)
    })?;
    Ok(knowledge_view(&state))
}

/// Adds knowledge/ (or `part` cv: cv/) to the 知识库 「Seeker」 (made if missing) and starts building
/// it; one undo step.
#[tauri::command]
pub async fn picky_seeker_knowledge_add(app: AppHandle, part: Option<String>) -> Result<Knowledge, String> {
    let state = app.state::<AppState>();
    let part = if part.as_deref() == Some("cv") { "cv" } else { "knowledge" };
    let folder = state.picky.with_db(|c| seeker_folder(c))?;
    let dir = sub(&folder, part, state.picky.mode)?.ok_or(format!("Seeker 文件夹里没有 {part}/"))?;
    let data = state.paths.root.clone();
    let id = state.with_picky_undo(&format!("把 {part}/ 加进知识库"), |c| {
        if source(c, &dir)?.is_some() {
            return Err(format!("{part}/ 已经在知识库里了"));
        }
        let tx = crate::modules::common::Tx::begin(c)?;
        let library = match knowledge::libraries(&tx)?.into_iter().find(|l| l.name == LIBRARY) {
            Some(l) => l.id,
            None => knowledge::create_library(&tx, LIBRARY)?,
        };
        let id = knowledge::add_folder(&tx, library, &dir, &data)?;
        tx.commit()?;
        Ok(id)
    })?;
    knowledge::scan_and_build(&app, id);
    Ok(knowledge_view(&state))
}

/// A job's company folder (`job`: as 投石问路 has it now).
pub fn job_company(state: &AppState, job: &output::JobSnapshot) -> Result<JobCompany, String> {
    let view = knowledge_view(state);
    let chosen = state.picky.with_db(|c| Ok(load(c)?.into_iter().find(|ch| ch.job_id == job.id && ch.job_created_at == job.created_at).map(|ch| ch.folder)))?;
    let auto = matched(&job.company, &view.companies);
    Ok(JobCompany { companies: view.companies, auto, chosen })
}

#[tauri::command]
pub async fn picky_seeker_job_company(state: State<'_, AppState>, job_id: i64) -> Result<JobCompany, String> {
    job_company(&state, &output::job(&state, job_id)?)
}

/// `folder`: None = by name again; "" = none; else one of knowledge/'s companies.
#[tauri::command]
pub async fn picky_seeker_job_company_set(state: State<'_, AppState>, job_id: i64, folder: Option<String>) -> Result<JobCompany, String> {
    let job = output::job(&state, job_id)?;
    if let Some(f) = folder.as_deref().filter(|f| !f.is_empty()) {
        if !knowledge_view(&state).companies.iter().any(|c| c.folder == f) {
            return Err("knowledge/ 里没有这家公司了".into());
        }
    }
    state.with_picky_undo("选公司资料", |c| choose(c, &job, folder.as_deref()))?;
    job_company(&state, &job)
}

/// What 选材 / 审查 read about the job's company.
#[derive(Debug, Default)]
pub struct Lookup {
    pub company: Option<String>,
    pub passages: Vec<Passage>,
    /// Why nothing was searched (shown with the suggestion); None when there is no job.
    pub note: Option<String>,
}

/// The passages of the job's company folder most like `query` (the JD). `cloud`: the model is a cloud
/// one (a folder not allowed there isn't searched).
pub async fn lookup(app: &AppHandle, job: Option<&JobRef>, query: &str, cloud: bool) -> Lookup {
    let Some(job) = job else { return Lookup::default() };
    let state = app.state::<AppState>();
    let snapshot = match output::job(&state, job.id) {
        Ok(j) if j.created_at == job.created_at => j,
        _ => return Lookup { note: Some("关联的岗位已经不在了".into()), ..Default::default() },
    };
    let found = match job_company(&state, &snapshot) {
        Ok(f) => f,
        Err(e) => return Lookup { note: Some(e), ..Default::default() },
    };
    let Some(folder) = found.folder().map(str::to_string) else { return Lookup { note: Some("这个岗位没有对上 knowledge/ 里的公司".into()), ..Default::default() } };
    let company = found.companies.iter().find(|c| c.folder == folder).map(|c| c.name.clone()).unwrap_or_else(|| folder.clone());
    let none = |why: &str| Lookup { company: Some(company.clone()), passages: vec![], note: Some(why.into()) };
    let Ok(Some(kdir)) = state.picky.with_db(|c| seeker_folder(c)).and_then(|f| dir(&f, state.picky.mode)) else { return none("Seeker 文件夹里没有 knowledge/") };
    let Ok(Some((src, library))) = state.picky.with_db(|c| source(c, &kdir)) else { return none("knowledge/ 还没加进知识库") };
    if cloud && !src.cloud {
        return none("knowledge/ 设成了不发给云端");
    }
    let scope = knowledge::Scope { libraries: vec![library.clone()], sources: vec![(src, library)], skipped: vec![] };
    match knowledge::retrieve_under(app, query, &scope, &format!("{folder}/")).await {
        Ok(passages) if passages.is_empty() => none("公司资料里没有相关的段落"),
        Ok(passages) => Lookup { company: Some(company), passages, note: None },
        Err(e) => none(&format!("公司资料没查成：{e}")),
    }
}
