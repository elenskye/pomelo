//! Word from the user's template (one per language), immutable exports, job links.
//! The user picks a paragraph of the template for each part (姓名, 联系方式, 章节标题, 经历抬头, 要点; and
//! optionally 第二行联系方式, 教育抬头, 说明, Summary, 空行); each line of the resume repeats that paragraph's
//! properties (a numbered list's numbering too), its tab stops and each column's look; `**粗体**` in a
//! point takes the paragraph's bold look (word.rs `resume_from_template`).
//! - 中文: 经历抬头 时间 ⇥ 名称 ⇥ 角色 ⇥ 地点, then 说明 (概览); 教育抬头 名称 ⇥ 时间, then 说明 「专业；说明」.
//! - English (like the user's own finished resume): 经历抬头 角色 ⇥ 时间, then 说明 「名称, 地点」 and 概览;
//!   no role: 名称 ⇥ 时间; 教育抬头 名称 ⇥ 时间, then 说明 the degree. Summary: its text in the Summary
//!   paragraph (else 说明). 空行 (a blank paragraph, copied): before each section after the first and
//!   between entries.
//! - 技能 and 证书: points only. 页首: links (网址) go on the second contact line as 「名称：网址」 when the
//!   template has one; the separator is the template's own (「  ·  」). The template file is only read.
//! Pages aren't counted (the user opens the Word to check it is one page). 另存 writes the docx with
//! create_new (never overwrites) when the draft is still the one it was made from, together with its
//! snapshot (both or neither). Exports aren't undoable; linking a job and 另开草稿 are.
//! Job links keep the job and the JD as they were; jobs is read first, then picky (never nested).
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
    pub draft: Draft,
    pub path: String,
    pub template: Template,
    pub created_at: String,
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
pub struct JobExport {
    pub id: String,
    pub export_id: String,
    pub job: JobSnapshot,
    pub jd: String,
    pub created_at: String,
    /// The version used for this job (one per job; the latest one linked unless changed).
    pub current: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Output {
    pub template: Option<Template>,
    #[serde(rename = "templateEn", skip_serializing_if = "Option::is_none")]
    pub template_en: Option<Template>,
    pub exports: Vec<Export>,
    pub links: Vec<JobExport>,
}

impl Output {
    pub fn template(&self, lang: &str) -> Option<&Template> {
        if lang == "en" { self.template_en.as_ref() } else { self.template.as_ref() }
    }
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
    let mut st = c.prepare("SELECT body, current FROM seeker_job_exports ORDER BY rowid DESC").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    let links = rows
        .into_iter()
        .map(|(body, current)| {
            let mut link: JobExport = serde_json::from_str(&body).map_err(err)?;
            link.current = current;
            Ok(link)
        })
        .collect::<Result<_, String>>()?;
    Ok(Output { template: None, template_en: None, exports, links })
}

/// Paths are metadata only: importing never opens or creates files.
pub fn replace(c: &Connection, output: &Output) -> Result<(), String> {
    for item in output.exports.iter().rev() {
        validate(&item.draft)?;
        c.execute("INSERT INTO seeker_exports (id, body) VALUES (?1, ?2)", params![item.id, serde_json::to_string(item).map_err(err)?]).map_err(err)?;
    }
    for link in output.links.iter().rev() {
        c.execute("INSERT INTO seeker_job_exports (id, export_id, job_id, job_created_at, body, current) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![link.id, link.export_id, link.job.id, link.job.created_at, serde_json::to_string(link).map_err(err)?, link.current])
            .map_err(|_| "Seeker：导出和岗位的关联对不上".to_string())?;
    }
    Ok(())
}

pub fn job(state: &AppState, id: i64) -> Result<JobSnapshot, String> {
    state.with_db(|c| c.query_row("SELECT id, company, position, location, link, note, created_at FROM jobs WHERE id = ?1", [id], |r| Ok(JobSnapshot { id: r.get(0)?, company: r.get(1)?, position: r.get(2)?, location: r.get(3)?, link: r.get(4)?, note: r.get(5)?, created_at: r.get(6)? })).map_err(|_| "岗位不存在，请重新选择".into()))
}

/// Links an export to a job as the one used this time (linked before: just made current again).
pub fn associate(c: &Connection, export: &Export, job: JobSnapshot) -> Result<(), String> {
    c.execute("UPDATE seeker_job_exports SET current = 0 WHERE job_id = ?1 AND job_created_at = ?2", params![job.id, job.created_at]).map_err(err)?;
    let created_at: String = c.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0)).map_err(err)?;
    let link = JobExport { id: workflow::id()?, export_id: export.id.clone(), job, jd: export.draft.jd.clone(), created_at, current: true };
    c.execute("INSERT INTO seeker_job_exports (id, export_id, job_id, job_created_at, body, current) VALUES (?1, ?2, ?3, ?4, ?5, 1) ON CONFLICT(job_id, job_created_at, export_id) DO UPDATE SET current = 1", params![link.id, link.export_id, link.job.id, link.job.created_at, serde_json::to_string(&link).map_err(err)?]).map_err(err)?;
    Ok(())
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
                    out.push((s.entry, [&e.time, &e.name, &e.role, &e.location].map(|x| x.trim()).join("\t")));
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

#[tauri::command]
pub async fn picky_seeker_template_read(state: State<'_, AppState>, path: String) -> Result<Vec<Option<String>>, String> {
    let path = allowed_path(Path::new(&path), state.picky.mode)?;
    if path.extension().and_then(|s| s.to_str()).map(str::to_lowercase).as_deref() != Some("docx") {
        return Err("请选择 docx 模板".into());
    }
    word::resume_template_items(&std::fs::read(path).map_err(err)?)
}

#[tauri::command]
/// The chosen paragraphs are in the template: text ones have text, the blank one is blank.
pub fn check_slots(s: &Slots, paragraphs: &[Option<String>]) -> Result<(), String> {
    let text = s.texts().into_iter().all(|i| paragraphs.get(i).is_some_and(Option::is_some));
    let blank = s.blank.is_none_or(|i| paragraphs.get(i).is_some_and(Option::is_none));
    if text && blank { Ok(()) } else { Err("模板刚刚改过，请重新选择".into()) }
}

#[tauri::command]
pub async fn picky_seeker_template(state: State<'_, AppState>, template: Option<Template>, lang: String) -> Result<Workspace, String> {
    let lang = lang_of(&lang)?;
    if let Some(t) = &template {
        let paragraphs = word::resume_template_items(&std::fs::read(allowed_path(Path::new(&t.path), state.picky.mode)?).map_err(err)?)?;
        if paragraphs != t.paragraphs {
            return Err("模板刚刚改过，请重新选择".into());
        }
        check_slots(&t.slots, &paragraphs)?;
    }
    let body = template.as_ref().map(serde_json::to_string).transpose().map_err(err)?;
    let column = if lang == "en" { "template_en" } else { "template" };
    state.picky.with_db(|c| {
        c.execute(&format!("INSERT INTO seeker_settings (id, {column}) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET {column} = excluded.{column}"), [body]).map_err(err)?;
        read(c)
    })
}

/// Writes `docx` (made from `draft`) as `directory/filename` (a new file only) and its snapshot.
pub fn save_export(c: &Connection, draft: &Draft, template: &Template, docx: &[u8], directory: &Path, filename: &str, job: Option<JobSnapshot>) -> Result<Export, String> {
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
    let export = Export { id: workflow::id()?, label: filename.into(), draft: draft.clone(), path: path.to_string_lossy().into_owned(), template: template.clone(), created_at };
    tx.execute("INSERT INTO seeker_exports (id, body) VALUES (?1, ?2)", params![export.id, serde_json::to_string(&export).map_err(err)?]).map_err(err)?;
    if let Some(job) = job {
        associate(&tx, &export, job)?;
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
    let template = w.output.template(lang).cloned().ok_or(if lang == "en" { "请先在「设置」里选好英文简历模板" } else { "请先在「设置」里选好中文简历模板" })?;
    let job = job_id.map(|id| job(&state, id)).transpose()?;
    if directory.trim().is_empty() {
        return Err("请选择导出到哪个文件夹".into());
    }
    let mode = state.picky.mode;
    let directory = allowed_path(Path::new(directory.trim()), mode)?;
    let (d, t) = (draft.clone(), template.clone());
    let docx = tauri::async_runtime::spawn_blocking(move || {
        let bytes = std::fs::read(allowed_path(Path::new(&t.path), mode)?).map_err(|e| format!("模板打不开：{e}"))?;
        if word::resume_template_items(&bytes)? != t.paragraphs {
            return Err("模板刚刚改过，请在「设置」里重新选择".to_string());
        }
        word::resume_from_template(&bytes, &lines(&d, &t))
    })
    .await
    .map_err(err)??;
    let export = state.picky.with_db(|c| save_export(c, &draft, &template, &docx, &directory, &filename, job))?;
    Ok(Saved { workspace: state.picky.with_db(|c| read(c))?, export })
}

#[tauri::command]
pub async fn picky_seeker_job_link(state: State<'_, AppState>, export_id: String, job_id: i64) -> Result<Workspace, String> {
    let job = job(&state, job_id)?;
    state.with_picky_undo("关联 Seeker 导出", |c| {
        let export = load(c)?.exports.into_iter().find(|e| e.id == export_id).ok_or("这次导出已经不在了")?;
        let tx = Tx::begin(c)?;
        associate(&tx, &export, job)?;
        tx.commit()?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_seeker_export_restore(state: State<'_, AppState>, export_id: String) -> Result<Workspace, String> {
    state.with_picky_undo("从导出另开草稿", |c| {
        let export = load(c)?.exports.into_iter().find(|e| e.id == export_id).ok_or("这次导出已经不在了")?;
        let tx = Tx::begin(c)?;
        save(&tx, &export.draft)?;
        workflow::baseline(&tx, &export.draft)?;
        tx.commit()?;
        read(c)
    })
}
