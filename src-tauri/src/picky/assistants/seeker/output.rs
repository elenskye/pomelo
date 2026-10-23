//! One-page Word from the user's template, LibreOffice's page count, immutable exports, job links.
//! The user picks a paragraph of the template for each part (姓名, 联系方式, 章节标题, 经历抬头, 要点; and
//! optionally 教育抬头, 说明); each line of the resume repeats that paragraph's properties, its tab
//! stops (经历抬头: 时间 ⇥ 名称 ⇥ 角色 ⇥ 地点; 教育抬头: 名称 ⇥ 时间) and each column's look; `**粗体**`
//! in a point takes the paragraph's bold look (word.rs `resume_from_template`). The template file is
//! only read. 预览 renders a PDF in a private temp folder; 另存 writes that same docx with create_new
//! (never overwrites) only when it is one page and the draft hasn't changed, together with its
//! snapshot (both or neither). Exports aren't undoable; linking a job and 另开草稿 are.
//! Job links keep the job and the JD as they were; jobs is read first, then picky (never nested).
use super::*;
use crate::modules::common::Tx;
use crate::picky::{pdf, word, Mode};
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
    pub pages: usize,
    pub renderer: String,
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
    pub exports: Vec<Export>,
    pub links: Vec<JobExport>,
}

#[derive(Serialize, Deserialize)]
struct Prepared {
    draft: Draft,
    template: Template,
    folder: String,
    pages: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    ticket: String,
    pages: usize,
    pdf_path: String,
    default_folder: String,
    renderer: String,
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
    Ok(Output { template: None, exports, links })
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

fn job(state: &AppState, id: i64) -> Result<JobSnapshot, String> {
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

fn staging(ticket: &str) -> Result<PathBuf, String> {
    if ticket.len() != 32 || !ticket.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("导出预览编号无效".into());
    }
    Ok(std::env::temp_dir().join(format!("pomelo-seeker-{ticket}")))
}

/// The resume as (template paragraph, text) lines.
pub fn lines(draft: &Draft, s: &Slots) -> Vec<(usize, String)> {
    let mut out = vec![(s.name, draft.name.clone())];
    let contact = draft.header.iter().filter(|h| h.visible && !h.value.trim().is_empty()).map(|h| h.value.trim()).collect::<Vec<_>>().join(" · ");
    if !contact.is_empty() {
        out.push((s.contact, contact));
    }
    let note = s.note.unwrap_or(s.point);
    for section in &draft.sections {
        out.push((s.section, section.title.clone()));
        for e in &section.entries {
            match (section.category.as_str(), s.education) {
                ("技能", _) => {}
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

fn soffice() -> Option<&'static str> {
    ["/Applications/LibreOffice.app/Contents/MacOS/soffice", "/opt/homebrew/bin/soffice", "/usr/local/bin/soffice"].into_iter().find(|p| Path::new(p).is_file())
}

/// docx → PDF with LibreOffice (own profile in `dir`, 90 seconds at most) → the number of pages.
fn render(dir: &Path) -> Result<usize, String> {
    let executable = soffice().ok_or("算不了页数：这台 Mac 没有装 LibreOffice")?;
    let profile = reqwest::Url::from_directory_path(dir.join("profile")).map_err(|_| "临时文件夹不能用来渲染")?;
    let mut child = std::process::Command::new(executable)
        .arg(format!("-env:UserInstallation={profile}"))
        .args(["--headless", "--convert-to", "pdf:writer_pdf_Export", "--outdir"])
        .arg(dir)
        .arg(dir.join("resume.docx"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("LibreOffice 打不开：{e}"))?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Err("LibreOffice 渲染失败".into()),
            Ok(None) if started.elapsed().as_secs() < 90 => std::thread::sleep(std::time::Duration::from_millis(200)),
            outcome => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match outcome {
                    Err(e) => format!("LibreOffice 出错：{e}"),
                    _ => "LibreOffice 超过 90 秒没算完".into(),
                });
            }
        }
    }
    let pages = pdf::pages(&std::fs::read(dir.join("resume.pdf")).map_err(|_| "LibreOffice 没有生成 PDF".to_string())?)?.len();
    if pages == 0 {
        return Err("渲染出来没有页面".into());
    }
    Ok(pages)
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
pub async fn picky_seeker_template(state: State<'_, AppState>, template: Option<Template>) -> Result<Workspace, String> {
    if let Some(t) = &template {
        let paragraphs = word::resume_template_items(&std::fs::read(allowed_path(Path::new(&t.path), state.picky.mode)?).map_err(err)?)?;
        let s = &t.slots;
        let chosen = [Some(s.name), Some(s.contact), Some(s.section), Some(s.entry), Some(s.point), s.education, s.note];
        if paragraphs != t.paragraphs || chosen.into_iter().flatten().any(|i| paragraphs.get(i).and_then(Option::as_ref).is_none()) {
            return Err("模板刚刚改过，请重新选择".into());
        }
    }
    let body = template.as_ref().map(serde_json::to_string).transpose().map_err(err)?;
    state.picky.with_db(|c| {
        c.execute("INSERT INTO seeker_settings (id, template) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET template = excluded.template", [body]).map_err(err)?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_seeker_output_preview(state: State<'_, AppState>) -> Result<Preview, String> {
    let w = state.picky.with_db(|c| read(c))?;
    let draft = w.draft.ok_or("还没有简历草稿")?;
    let template = w.output.template.ok_or("请先在「模板与规则」里选好 Word 模板")?;
    let mode = state.picky.mode;
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = std::fs::read(allowed_path(Path::new(&template.path), mode)?).map_err(|e| format!("模板打不开：{e}"))?;
        if word::resume_template_items(&bytes)? != template.paragraphs {
            return Err("模板刚刚改过，请在「模板与规则」里重新选择".into());
        }
        let docx = word::resume_from_template(&bytes, &lines(&draft, &template.slots))?;
        let ticket = workflow::id()?;
        let dir = staging(&ticket)?;
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&dir).map_err(err)?;
        let result = (|| -> Result<Preview, String> {
            std::fs::write(dir.join("resume.docx"), docx).map_err(err)?;
            let pages = render(&dir)?;
            let folder = if w.folder.is_empty() { String::new() } else { Path::new(&w.folder).join("cv/cn").to_string_lossy().into_owned() };
            std::fs::write(dir.join("prepared.json"), serde_json::to_vec(&Prepared { draft, template, folder: folder.clone(), pages }).map_err(err)?).map_err(err)?;
            Ok(Preview { ticket, pages, pdf_path: dir.join("resume.pdf").to_string_lossy().into_owned(), default_folder: folder, renderer: "LibreOffice".into() })
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        result
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn picky_seeker_output_discard(ticket: String) -> Result<(), String> {
    match std::fs::remove_dir_all(staging(&ticket)?) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(err(e)),
        _ => Ok(()),
    }
}

/// Writes the previewed docx as `directory/filename` (a new file only) and its snapshot.
pub fn save_export(c: &Connection, ticket: &str, dir: &Path, directory: &Path, filename: &str, job: Option<JobSnapshot>) -> Result<Export, String> {
    let prepared: Prepared = serde_json::from_slice(&std::fs::read(dir.join("prepared.json")).map_err(|_| "预览已失效，请重新分页")?).map_err(err)?;
    if prepared.pages != 1 {
        return Err(format!("现在是 {} 页，请精简后重新分页", prepared.pages));
    }
    let filename = filename.trim();
    if filename.is_empty() || filename.contains(['/', '\\', ':']) || filename.starts_with('.') || filename.chars().any(char::is_control) || !filename.to_lowercase().ends_with(".docx") {
        return Err("文件名要以 .docx 结尾，不能有 / \\ :".into());
    }
    if read(c)?.draft.as_ref() != Some(&prepared.draft) {
        return Err("草稿改过了，请重新分页".into());
    }
    let path = directory.join(filename);
    let bytes = std::fs::read(dir.join("resume.docx")).map_err(err)?;
    let tx = Tx::begin(c)?;
    let created_at: String = tx.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0)).map_err(err)?;
    let export = Export { id: ticket.into(), label: filename.into(), draft: prepared.draft, path: path.to_string_lossy().into_owned(), pages: prepared.pages, renderer: "LibreOffice".into(), template: prepared.template, created_at };
    tx.execute("INSERT INTO seeker_exports (id, body) VALUES (?1, ?2)", params![export.id, serde_json::to_string(&export).map_err(err)?]).map_err(err)?;
    if let Some(job) = job {
        associate(&tx, &export, job)?;
    }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists { "已经有同名的文件了，请换个名字".to_string() } else { format!("文件建不了：{e}") })?;
    let written = file.write_all(&bytes).and_then(|_| file.sync_all());
    drop(file);
    let left = |why: String| match std::fs::remove_file(&path) {
        Ok(()) => why,
        Err(_) => format!("{why}；没写完的文件留在 {}，请手动删掉", path.display()),
    };
    written.map_err(|e| left(format!("文件没写完：{e}")))?;
    tx.commit().map_err(|e| left(format!("导出记录没存上：{e}")))?;
    Ok(export)
}

#[tauri::command]
pub async fn picky_seeker_output_save(state: State<'_, AppState>, ticket: String, directory: String, filename: String, job_id: Option<i64>) -> Result<Saved, String> {
    let dir = staging(&ticket)?;
    // a retry after it went through: the same export, no second file
    if let Some(export) = state.picky.with_db(|c| Ok(load(c)?.exports.into_iter().find(|e| e.id == ticket)))? {
        return Ok(Saved { workspace: state.picky.with_db(|c| read(c))?, export });
    }
    let job = job_id.map(|id| job(&state, id)).transpose()?;
    if directory.trim().is_empty() {
        return Err("请选择导出到哪个文件夹".into());
    }
    let directory = allowed_path(Path::new(directory.trim()), state.picky.mode)?;
    let export = state.picky.with_db(|c| save_export(c, &ticket, &dir, &directory, &filename, job))?;
    let _ = std::fs::remove_dir_all(&dir);
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
