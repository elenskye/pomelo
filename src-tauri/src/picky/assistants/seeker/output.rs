//! Word from Pomelo's built-in template (one per language, src-tauri/templates/: formatting only, made
//! from the user's seek-it-cn-template (tidied: A4, 等线) and keyreply-resume-v5, placeholder text),
//! exports (a record of each Word: name, path, language, time and the materials used — no copy of the
//! résumé), job links. Each template has one sample paragraph per part (`SLOTS_CN`,
//! `SLOTS_EN`: 姓名, 联系方式, 章节标题, 经历抬头, 要点; 第二行联系方式, 教育抬头, 说明, Summary, 空行); each
//! line of the resume repeats that paragraph's
//! properties (a numbered list's numbering too), its tab stops and each column's look; `**粗体**` in a
//! point takes the paragraph's bold look (word.rs `resume_from_template`).
//! - 中文: 经历抬头 名称 ⇥ 角色 ⇥ 时间 (时间 right-aligned; 地点 isn't printed), then 说明 (概览); 教育抬头 名称 ⇥ 时间, then 说明 「专业；说明」.
//! - English (like the user's own finished resume): 经历抬头 角色 ⇥ 时间, then 说明 「名称, 地点」 and 概览;
//!   no role: 名称 ⇥ 时间; 教育抬头 名称 ⇥ 时间, then 说明 the degree. Summary: its text in the Summary
//!   paragraph (else 说明). 空行 (a blank paragraph, copied): before each section after the first and
//!   between entries.
//! - 技能 and 证书: points only. 页首: links (网址) go on the second contact line as 「名称：网址」 when the
//!   template has one; the separator is the template's own (「  ·  」). Older data may still name a
//!   template the user picked (`Output::template`): no longer used.
//! Pages aren't counted (the user opens the Word to check it is one page). 另存 writes the docx with
//! create_new (never overwrites) when the draft is still the one it was made from, together with its
//! record (both or neither). Exports aren't undoable; linking a job is.
//! Job links (`seeker_job_resumes`) are files by path: an export, or any résumé under cv/ picked in 投石问路's
//! panel; one per job is 「本次使用」; they keep the job and the JD as they were. A link whose file is gone
//! isn't shown (the record stays). The old 简历 field of 投石问路 moved here once (`adopt_job_resumes`).
//! jobs is read first, then picky (never nested).
use super::*;
use crate::modules::common::Tx;
use crate::picky::{word, Mode};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Paragraph indexes in the template for each part of the resume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slots {
    pub name: usize,
    pub contact: usize,
    pub section: usize,
    pub entry: usize,
    pub point: usize,
    pub education: Option<usize>,
    pub note: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact2: Option<usize>,
    /// The Summary's paragraph (English).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<usize>,
    /// A blank paragraph between sections and entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blank: Option<usize>,
}

impl Slots {
    /// The paragraphs that must have text (the blank one must not).
    pub fn texts(&self) -> Vec<usize> {
        [Some(self.name), Some(self.contact), Some(self.section), Some(self.entry), Some(self.point), self.education, self.note, self.contact2, self.summary].into_iter().flatten().collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub path: String,
    pub slots: Slots,
    /// The template's paragraphs when it was set up (None: empty): a changed file asks again.
    pub paragraphs: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Export {
    pub id: String,
    pub label: String,
    pub path: String,
    #[serde(default = "cn")]
    pub lang: String,
    /// The materials it was made from (their ids): an interview on it reads their 简历题 and 档案.
    #[serde(default)]
    pub materials: Vec<String>,
    pub created_at: String,
    /// Data files before format 27 carried the draft: read for its language and materials, never kept.
    #[serde(default, skip_serializing)]
    pub draft: Option<Draft>,
}

fn cn() -> String {
    "cn".into()
}

/// The materials a draft was made from (their ids, in order, once each).
pub fn materials_of(draft: &Draft) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for id in draft.sections.iter().flat_map(|s| &s.entries).filter_map(|e| e.source.as_ref()).map(|s| &s.record_id) {
        if !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub id: i64,
    pub company: String,
    pub position: Option<String>,
    pub location: Option<String>,
    pub link: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResume {
    pub id: String,
    /// The file: an export's path, or a résumé under cv/.
    #[serde(default)]
    pub path: String,
    pub job: JobSnapshot,
    /// The draft's JD when an export was linked ("" for a file picked from cv/).
    #[serde(default)]
    pub jd: String,
    pub created_at: String,
    /// The one used for this job (one per job; the latest one linked unless changed).
    pub current: bool,
    /// Data files before format 26 named the export instead (its path is taken on import).
    #[serde(default, skip_serializing)]
    pub export_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Output {
    pub template: Option<Template>,
    #[serde(rename = "templateEn", skip_serializing_if = "Option::is_none")]
    pub template_en: Option<Template>,
    pub exports: Vec<Export>,
    pub links: Vec<JobResume>,
}

#[derive(Serialize)]
pub struct Saved {
    pub workspace: Workspace,
    pub export: Export,
}

/// Exports and links, newest first (the template is in seeker_settings).
pub fn load(c: &Connection) -> Result<Output, String> {
    let mut st = c.prepare("SELECT body FROM seeker_exports ORDER BY rowid DESC").map_err(err)?;
    let rows = st.query_map([], |r| r.get::<_, String>(0)).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let exports = rows.iter().map(|s| serde_json::from_str(s).map_err(err)).collect::<Result<_, _>>()?;
    let mut st = c.prepare("SELECT body, current FROM seeker_job_resumes ORDER BY rowid DESC").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let links = rows
        .into_iter()
        .map(|(body, current)| {
            let mut link: JobResume = serde_json::from_str(&body).map_err(err)?;
            link.current = current;
            Ok(link)
        })
        .collect::<Result<_, String>>()?;
    Ok(Output { template: None, template_en: None, exports, links })
}

/// Paths are metadata only: importing never opens or creates files.
pub fn replace(c: &Connection, output: &Output) -> Result<(), String> {
    for item in output.exports.iter().rev() {
        let mut item = item.clone();
        if let Some(d) = item.draft.take() {
            item.lang = if d.lang == "en" { "en".into() } else { "cn".into() };
            item.materials = materials_of(&d);
        }
        c.execute("INSERT INTO seeker_exports (id, body) VALUES (?1, ?2)", params![item.id, serde_json::to_string(&item).map_err(err)?]).map_err(err)?;
    }
    for link in output.links.iter().rev() {
        let mut link = link.clone();
        if link.path.is_empty() {
            link.path = link.export_id.as_ref().and_then(|id| output.exports.iter().find(|e| &e.id == id)).map(|e| e.path.clone()).ok_or("Seeker：导出和岗位的关联对不上")?;
        }
        link.export_id = None;
        c.execute("INSERT INTO seeker_job_resumes (id, job_id, job_created_at, path, body, current) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![link.id, link.job.id, link.job.created_at, link.path, serde_json::to_string(&link).map_err(err)?, link.current])
            .map_err(|_| "Seeker：岗位用的简历对不上".to_string())?;
    }
    Ok(())
}

pub fn job(state: &AppState, id: i64) -> Result<JobSnapshot, String> {
    state.with_db(|c| c.query_row("SELECT id, company, position, location, link, note, created_at FROM jobs WHERE id = ?1", [id], |r| Ok(JobSnapshot { id: r.get(0)?, company: r.get(1)?, position: r.get(2)?, location: r.get(3)?, link: r.get(4)?, note: r.get(5)?, created_at: r.get(6)? })).map_err(|_| "岗位不存在，请重新选择".into()))
}

/// Links a résumé file to a job as the one used this time (linked before: just made current again).
/// `current: false` adds it beside the one in use (the old 简历 field of a job that has one).
pub fn associate(c: &Connection, path: &str, jd: &str, job: JobSnapshot, current: bool) -> Result<(), String> {
    if current {
        c.execute("UPDATE seeker_job_resumes SET current = 0 WHERE job_id = ?1 AND job_created_at = ?2", params![job.id, job.created_at]).map_err(err)?;
    }
    let created_at: String = c.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0)).map_err(err)?;
    let link = JobResume { id: workflow::id()?, path: path.into(), job, jd: jd.into(), created_at, current, export_id: None };
    let upsert = if current { "DO UPDATE SET current = 1" } else { "DO NOTHING" };
    c.execute(&format!("INSERT INTO seeker_job_resumes (id, job_id, job_created_at, path, body, current) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(job_id, job_created_at, path) {upsert}"), params![link.id, link.job.id, link.job.created_at, link.path, serde_json::to_string(&link).map_err(err)?, current]).map_err(err)?;
    Ok(())
}

/// Same file (both resolve to one path), else the same text.
pub fn same_file(a: &str, b: &str) -> bool {
    a == b || std::fs::canonicalize(a).ok().zip(std::fs::canonicalize(b).ok()).is_some_and(|(x, y)| x == y)
}

/// A job's résumés whose file is still under cv/ (newest link first), for its panel.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Linked {
    pub path: String,
    /// Under cv/ (`en/seek-it-keyreply-resume-v5.docx`).
    pub name: String,
    pub lang: String,
    pub current: bool,
    pub jd: String,
    pub job: JobSnapshot,
    pub created_at: String,
    pub export_id: Option<String>,
}

/// The job's linked résumés and the résumés under cv/ to pick from (templates too: they can be sent).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResumes {
    pub linked: Vec<Linked>,
    pub files: Vec<files::CvFile>,
    /// No Seeker folder chosen yet.
    pub no_folder: bool,
    pub error: Option<String>,
}

pub fn job_resumes(links: &[JobResume], job: &JobSnapshot, cv: Vec<files::CvFile>) -> Vec<Linked> {
    links
        .iter()
        .filter(|l| l.job.id == job.id && l.job.created_at == job.created_at)
        .filter_map(|l| cv.iter().find(|f| same_file(&f.path, &l.path)).map(|f| Linked { path: l.path.clone(), name: f.name.clone(), lang: f.lang.clone(), current: l.current, jd: l.jd.clone(), job: l.job.clone(), created_at: l.created_at.clone(), export_id: f.export_id.clone() }))
        .collect()
}

#[tauri::command]
pub async fn picky_seeker_job_resumes(state: State<'_, AppState>, job_id: i64) -> Result<JobResumes, String> {
    let job = job(&state, job_id)?;
    let f = files::list(&state);
    let no_folder = f.error.is_none() && state.picky.with_db(|c| files::root(c, state.picky.mode))?.is_none();
    let links = state.picky.with_db(|c| Ok(load(c)?.links))?;
    Ok(JobResumes { linked: job_resumes(&links, &job, f.cv.clone()), files: f.cv, no_folder, error: f.error })
}

/// Once (v0.7 refine R2): 投石问路's old 简历 field (a path) becomes the job's résumé when it is a
/// résumé under cv/ (in use when the job has none in use yet). Not undoable, like an upgrade; the field
/// stays in pomelo.db, no longer shown. No Seeker folder yet: tried again next launch.
pub fn adopt_job_resumes(state: &AppState) -> Result<usize, String> {
    const DONE: &str = "seeker.jobResumesAdopted";
    let mode = state.picky.mode;
    let (root, done) = state.picky.with_db(|c| Ok((files::root(c, mode).ok().flatten(), crate::picky::store::pref_get::<bool>(c, DONE)?.unwrap_or(false))))?;
    let Some(root) = root.filter(|_| !done) else { return Ok(0) };
    let home = std::env::var("HOME").unwrap_or_default();
    let jobs: Vec<(JobSnapshot, String)> = state.with_db(|c| {
        let mut st = c.prepare("SELECT id, company, position, location, link, note, created_at, resume FROM jobs WHERE trim(coalesce(resume, '')) <> '' ORDER BY id").map_err(err)?;
        let rows = st.query_map([], |r| Ok((JobSnapshot { id: r.get(0)?, company: r.get(1)?, position: r.get(2)?, location: r.get(3)?, link: r.get(4)?, note: r.get(5)?, created_at: r.get(6)? }, r.get::<_, String>(7)?))).map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(err)
    })?;
    let found: Vec<(JobSnapshot, String)> = jobs
        .into_iter()
        .filter_map(|(job, p)| {
            let p = p.trim();
            let p = p.strip_prefix("~/").map_or_else(|| p.to_string(), |rest| format!("{home}/{rest}"));
            files::in_cv(&root, Path::new(&p)).ok().map(|f| (job, f))
        })
        .collect();
    state.picky.with_db(|c| {
        let exports = load(c)?.exports;
        let tx = Tx::begin(c)?;
        let mut n = 0;
        for (job, path) in found {
            let in_use: i64 = tx.query_row("SELECT count(*) FROM seeker_job_resumes WHERE job_id = ?1 AND job_created_at = ?2 AND current = 1", params![job.id, job.created_at], |r| r.get(0)).map_err(err)?;
            let path = exports.iter().find(|e| same_file(&e.path, &path)).map_or(path, |e| e.path.clone());
            associate(&tx, &path, "", job, in_use == 0)?;
            n += 1;
        }
        crate::picky::store::pref_set(&tx, DONE, &true)?;
        tx.commit()?;
        Ok(n)
    })
}

/// The real Seeker folder is out of bounds for development and test runs (mock mode).
pub fn allowed_path(path: &Path, mode: Mode) -> Result<PathBuf, String> {
    let real = std::env::var("HOME").map(|h| Path::new(&h).join("Documents/Seeker")).ok();
    let blocked = |p: &Path| mode == Mode::Mock && real.as_ref().is_some_and(|r| p.starts_with(r));
    if blocked(path) {
        return Err("开发版只能用编的 Seeker 资料".into());
    }
    let path = std::fs::canonicalize(path).map_err(|e| format!("路径打不开：{e}"))?;
    if blocked(&path) {
        return Err("开发版只能用编的 Seeker 资料".into());
    }
    Ok(path)
}

/// A web address (not an e-mail): 「https://…」, 「www.…」, 「github.com/…」.
pub fn is_link(value: &str) -> bool {
    let v = value.trim().to_lowercase();
    if v.contains('@') || v.contains(char::is_whitespace) {
        return false;
    }
    if v.contains("://") || v.starts_with("www.") {
        return true;
    }
    let host = v.split('/').next().unwrap_or("");
    host.rsplit_once('.').is_some_and(|(name, tld)| !name.is_empty() && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
}

/// The separator in the template's contact paragraph (「  ·  」 with its spaces), else 「 · 」.
pub fn separator(paragraph: Option<&str>) -> String {
    let Some(text) = paragraph else { return " · ".into() };
    let chars: Vec<char> = text.chars().collect();
    let Some(at) = chars.iter().position(|c| matches!(c, '·' | '|' | '｜' | '•')) else { return " · ".into() };
    let space = |c: &char| *c == ' ' || *c == '\u{3000}';
    let start = at - chars[..at].iter().rev().take_while(|c| space(c)).count();
    let end = at + 1 + chars[at + 1..].iter().take_while(|c| space(c)).count();
    chars[start..end].iter().collect()
}

/// The resume as (template paragraph, text) lines; a blank line has no text.
pub fn lines(draft: &Draft, t: &Template) -> Vec<(usize, String)> {
    let s = &t.slots;
    let english = draft.lang == "en";
    let sep = separator(t.paragraphs.get(s.contact).and_then(Option::as_deref));
    let mut out = vec![(s.name, draft.name.clone())];
    let shown: Vec<&HeaderField> = draft.header.iter().filter(|h| h.visible && !h.value.trim().is_empty()).collect();
    let (links, rest): (Vec<&HeaderField>, Vec<&HeaderField>) = shown.into_iter().partition(|h| s.contact2.is_some() && is_link(&h.value));
    let contact = rest.iter().map(|h| h.value.trim()).collect::<Vec<_>>().join(&sep);
    if !contact.is_empty() {
        out.push((s.contact, contact));
    }
    if let (Some(second), false) = (s.contact2, links.is_empty()) {
        out.push((second, links.iter().map(|h| format!("{}：{}", h.label.trim(), h.value.trim())).collect::<Vec<_>>().join(&sep)));
    }
    let note = s.note.unwrap_or(s.point);
    let joined = |parts: &[&str], sep: &str| parts.iter().map(|x| x.trim()).filter(|x| !x.is_empty()).collect::<Vec<_>>().join(sep);
    for (n, section) in draft.sections.iter().enumerate() {
        let headless = matches!(section.category.as_str(), "技能" | "证书" | "摘要");
        if let (Some(blank), true) = (s.blank, n > 0) {
            out.push((blank, String::new()));
        }
        out.push((s.section, section.title.clone()));
        for (i, e) in section.entries.iter().enumerate() {
            if let (Some(blank), true, false) = (s.blank, i > 0, headless) {
                out.push((blank, String::new()));
            }
            match (section.category.as_str(), s.education) {
                ("技能" | "证书", _) => {}
                ("摘要", _) => {
                    if !e.note.trim().is_empty() {
                        out.push((s.summary.unwrap_or(note), e.note.trim().into()));
                    }
                }
                ("教育", education) if english => {
                    out.push((education.unwrap_or(s.entry), format!("{}\t{}", e.name.trim(), e.time.trim())));
                    for line in [e.role.trim(), e.note.trim()] {
                        if !line.is_empty() {
                            out.push((note, line.into()));
                        }
                    }
                }
                _ if english => {
                    let place = joined(&[&e.name, &e.location], ", ");
                    if e.role.trim().is_empty() {
                        out.push((s.entry, format!("{}\t{}", e.name.trim(), e.time.trim())));
                        if !e.location.trim().is_empty() {
                            out.push((note, e.location.trim().into()));
                        }
                    } else {
                        out.push((s.entry, format!("{}\t{}", e.role.trim(), e.time.trim())));
                        if !place.is_empty() {
                            out.push((note, place));
                        }
                    }
                    if !e.note.trim().is_empty() {
                        out.push((note, e.note.trim().into()));
                    }
                }
                ("教育", Some(education)) => {
                    out.push((education, format!("{}\t{}", e.name, e.time)));
                    let line = [e.role.trim(), e.note.trim()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("；");
                    if !line.is_empty() {
                        out.push((note, line));
                    }
                }
                _ => {
                    out.push((s.entry, [&e.name, &e.role, &e.time].map(|x| x.trim()).join("\t")));
                    if !e.note.trim().is_empty() {
                        out.push((note, e.note.trim().into()));
                    }
                }
            }
            out.extend(e.points.iter().map(|p| (s.point, p.text.clone())));
        }
    }
    out
}

/// The built-in templates (formatting only; see the top of this file).
pub const TEMPLATE_CN: &[u8] = include_bytes!("../../../../templates/resume-cn.docx");
pub const TEMPLATE_EN: &[u8] = include_bytes!("../../../../templates/resume-en.docx");
/// Their sample paragraphs: 姓名, 联系方式, 第二行联系方式, 章节标题, 经历抬头, 教育抬头, 说明, 要点.
pub const SLOTS_CN: Slots = Slots { name: 0, contact: 1, contact2: Some(2), section: 3, entry: 4, education: Some(5), note: Some(6), point: 7, summary: None, blank: None };
/// 姓名, 联系方式, 章节标题, Summary, 空行, 经历抬头, 第二行, 要点, 教育抬头.
pub const SLOTS_EN: Slots = Slots { name: 0, contact: 1, section: 2, summary: Some(3), blank: Some(4), entry: 5, note: Some(6), point: 7, education: Some(8), contact2: None };

/// A language's built-in template and its bytes.
pub fn builtin(lang: &str) -> Result<(Template, &'static [u8]), String> {
    let (bytes, slots, name) = if lang == "en" { (TEMPLATE_EN, SLOTS_EN, "内置英文模板") } else { (TEMPLATE_CN, SLOTS_CN, "内置中文模板") };
    let paragraphs = word::resume_template_items(bytes)?;
    check_slots(&slots, &paragraphs)?;
    Ok((Template { path: name.into(), slots, paragraphs }, bytes))
}

/// The chosen paragraphs are in the template: text ones have text, the blank one is blank.
pub fn check_slots(s: &Slots, paragraphs: &[Option<String>]) -> Result<(), String> {
    let text = s.texts().into_iter().all(|i| paragraphs.get(i).is_some_and(Option::is_some));
    let blank = s.blank.is_none_or(|i| paragraphs.get(i).is_some_and(Option::is_none));
    if text && blank { Ok(()) } else { Err("模板刚刚改过，请重新选择".into()) }
}

/// Writes `docx` (made from `draft`) as `directory/filename` (a new file only) and its snapshot.
pub fn save_export(c: &Connection, draft: &Draft, docx: &[u8], directory: &Path, filename: &str, job: Option<JobSnapshot>) -> Result<Export, String> {
    let filename = filename.trim();
    if filename.is_empty() || filename.contains(['/', '\\', ':']) || filename.starts_with('.') || filename.chars().any(char::is_control) || !filename.to_lowercase().ends_with(".docx") {
        return Err("文件名要以 .docx 结尾，不能有 / \\ :".into());
    }
    if read(c)?.draft(&draft.lang) != Some(draft) {
        return Err("草稿刚刚改过，请重新导出".into());
    }
    let path = directory.join(filename);
    let tx = Tx::begin(c)?;
    let created_at: String = tx.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0)).map_err(err)?;
    let export = Export { id: workflow::id()?, label: filename.into(), path: path.to_string_lossy().into_owned(), lang: if draft.lang == "en" { "en".into() } else { "cn".into() }, materials: materials_of(draft), created_at, draft: None };
    tx.execute("INSERT INTO seeker_exports (id, body) VALUES (?1, ?2)", params![export.id, serde_json::to_string(&export).map_err(err)?]).map_err(err)?;
    if let Some(job) = job {
        associate(&tx, &export.path, &draft.jd, job, true)?;
    }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists { "已经有同名的文件了，请换个名字".to_string() } else { format!("文件建不了：{e}") })?;
    let written = file.write_all(docx).and_then(|_| file.sync_all());
    drop(file);
    let left = |why: String| match std::fs::remove_file(&path) {
        Ok(()) => why,
        Err(_) => format!("{why}；没写完的文件留在 {}，请手动删掉", path.display()),
    };
    written.map_err(|e| left(format!("文件没写完：{e}")))?;
    tx.commit().map_err(|e| left(format!("导出记录没存上：{e}")))?;
    Ok(export)
}

/// 另存 Word: the current draft through the template, as a new file in `directory`.
#[tauri::command]
pub async fn picky_seeker_output_save(state: State<'_, AppState>, directory: String, filename: String, job_id: Option<i64>, lang: String) -> Result<Saved, String> {
    let lang = lang_of(&lang)?;
    let w = state.picky.with_db(|c| read(c))?;
    let draft = w.draft(lang).cloned().ok_or("还没有简历草稿")?;
    let (template, bytes) = builtin(lang)?;
    let job = job_id.map(|id| job(&state, id)).transpose()?;
    if directory.trim().is_empty() {
        return Err("请选择导出到哪个文件夹".into());
    }
    let mode = state.picky.mode;
    let directory = allowed_path(Path::new(directory.trim()), mode)?;
    let (d, t) = (draft.clone(), template.clone());
    let docx = tauri::async_runtime::spawn_blocking(move || word::resume_from_template(bytes, &lines(&d, &t))).await.map_err(err)??;
    let export = state.picky.with_db(|c| save_export(c, &draft, &docx, &directory, &filename, job))?;
    Ok(Saved { workspace: state.picky.with_db(|c| read(c))?, export })
}

/// A résumé under cv/ (an export's file too) as the one this job uses (one undo step).
#[tauri::command]
pub async fn picky_seeker_job_resume(state: State<'_, AppState>, job_id: i64, path: String) -> Result<Workspace, String> {
    let job = job(&state, job_id)?;
    let mode = state.picky.mode;
    state.with_picky_undo("关联简历", |c| {
        let root = files::root(c, mode)?.ok_or("还没选 Seeker 文件夹")?;
        let file = files::in_cv(&root, Path::new(&path))?;
        let exports = load(c)?.exports;
        let path = exports.iter().find(|e| same_file(&e.path, &file)).map_or(file, |e| e.path.clone());
        let tx = Tx::begin(c)?;
        associate(&tx, &path, "", job, true)?;
        tx.commit()?;
        read(c)
    })
}

