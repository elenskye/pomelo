//! 呦呦漫游 (v0.5): the personal website (its own project, its own git history) edited and published from
//! Pomelo. The content stays in the site folder (setting `site.folder`, default
//! ~/Documents/GitHub/personal-website); nothing goes into pomelo.db, backups, export, undo or search.
//! - Mode (like 小拾): `Real` only for the installed app on the real data dir (or POMELO_REAL_SITE=1): the
//!   set folder, real pushes, Vercel's result via gh. Otherwise `Sandbox`: a copy in the data dir
//!   (`site-sandbox/website`, made by 建副本 from the site folder with APFS clones) pushing to a bare repo
//!   beside it (`site-sandbox/remote.git`); a sandbox never commits or pushes towards a remote that isn't
//!   a local folder, and fakes the deployment.
//! - 发布 (publish.rs): prepare = fetch + fast-forward only (diverged / conflicting → stop) + the change
//!   list (changes.rs) with its token; confirm = message + token → check (npm run qa) → commit all → push
//!   → Vercel's result. 本地预览 = the site's dev server (server.rs).
//! - 编辑 (v0.5 阶段 2 起): wander.rs (作品), now.rs (Now、当下状态), about.rs (关于我), career.rs (职业页),
//!   registry.rs (资源库), terms.rs (话题与类别、外部链接); doc.rs / yaml.rs (files in the site's own
//!   format), images.rs (images and files copied into the site). Only in a folder fit to publish from (check_folder): a sandbox never edits a folder tied
//!   to the real GitHub. 预览 = the dev server's page in Pomelo's own window ("site-preview").

pub mod about;
pub mod career;
pub mod changes;
pub mod doc;
pub mod images;
pub mod now;
pub mod publish;
pub mod registry;
pub mod server;
pub mod terms;
pub mod tools;
pub mod wander;
pub mod yaml;

use crate::AppState;
use changes::{Changes, Diff};
use images::Asset;
use publish::{History, Job, Plan, Publisher};
use serde::Serialize;
use server::{Server, ServerView};
use wander::{Prints, Work, Works};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const PREVIEW: &str = "site-preview";

pub const REAL_ENV: &str = "POMELO_REAL_SITE";
/// Where 建副本 copies from (default: the real site folder; read only).
pub const SOURCE_ENV: &str = "POMELO_SITE_SRC";
pub const FOLDER_KEY: &str = "site.folder";
const REAL_PORT: u16 = 4328;
const SANDBOX_PORT: u16 = 4338;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Real,
    Sandbox,
}

pub fn mode(data_dir_overridden: bool) -> Mode {
    if cfg!(test) {
        return Mode::Sandbox;
    }
    if std::env::var(REAL_ENV).as_deref() == Ok("1") {
        return Mode::Real;
    }
    if cfg!(debug_assertions) || data_dir_overridden {
        Mode::Sandbox
    } else {
        Mode::Real
    }
}

pub struct Site {
    pub mode: Mode,
    home: PathBuf,
    sandbox_root: PathBuf,
    pub server: Server,
    pub publisher: Arc<Publisher>,
}

impl Site {
    pub fn new(mode: Mode, home: &Path, data_root: &Path) -> Self {
        Self { mode, home: home.into(), sandbox_root: data_root.join("site-sandbox"), server: Server::default(), publisher: Arc::default() }
    }

    pub fn real_folder(&self) -> PathBuf {
        self.home.join("Documents/GitHub/personal-website")
    }

    pub fn default_folder(&self) -> PathBuf {
        match self.mode {
            Mode::Real => self.real_folder(),
            Mode::Sandbox => self.sandbox_root.join("website"),
        }
    }

    pub fn port(&self) -> u16 {
        match self.mode {
            Mode::Real => REAL_PORT,
            Mode::Sandbox => SANDBOX_PORT,
        }
    }

    fn sandbox(&self) -> bool {
        self.mode == Mode::Sandbox
    }

    /// Pomelo is quitting: our dev server and a running check stop with it.
    pub fn on_exit(&self) {
        self.server.stop();
        self.publisher.kill();
    }
}

// ---------- the folder and its git state ----------

pub fn origin_url(folder: &Path) -> Option<String> {
    tools::git(folder, &["remote", "get-url", "origin"], Duration::from_secs(10)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// A remote that is a folder on this Mac (the sandbox's only allowed kind).
pub fn is_local_remote(url: &str) -> bool {
    !url.is_empty() && (url.starts_with('/') || url.starts_with("file://") || (!url.contains("://") && !url.contains(':')))
}

/// (owner, name) of a GitHub remote (https, ssh or scp-like).
pub fn github_repo(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://github.com/").or_else(|| url.strip_prefix("git@github.com:")).or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, name) = rest.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/')).then(|| (owner.into(), name.into()))
}

/// `site: '…'` in the site's astro.config.
pub fn site_url(folder: &Path) -> Option<String> {
    ["astro.config.mjs", "astro.config.ts", "astro.config.js"].iter().find_map(|f| {
        let text = std::fs::read_to_string(folder.join(f)).ok()?;
        let at = text.find("site:")?;
        let rest = text[at + 5..].trim_start();
        let q = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
        let url = &rest[1..rest[1..].find(q)? + 1];
        url.starts_with("http").then(|| url.trim_end_matches('/').to_string())
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// no-sandbox | missing | not-git | not-main | git-busy | no-remote | real-remote
    pub kind: &'static str,
    pub text: String,
}

fn problem(kind: &'static str, text: impl Into<String>) -> Option<Problem> {
    Some(Problem { kind, text: text.into() })
}

/// Why this folder can't be published from (None = fine).
pub fn check_folder(folder: &Path, sandbox: bool, is_default: bool) -> Option<Problem> {
    if !folder.is_dir() {
        return if sandbox && is_default { problem("no-sandbox", "开发版用网站的副本，还没有建") } else { problem("missing", format!("找不到网站文件夹：{}", folder.display())) };
    }
    let dot = folder.join(".git");
    if !dot.exists() {
        return problem("not-git", "网站文件夹不是 git 仓库");
    }
    let branch = tools::git(folder, &["symbolic-ref", "--quiet", "--short", "HEAD"], Duration::from_secs(10)).map(|s| s.trim().to_string()).unwrap_or_default();
    if branch != "main" {
        let now = if branch.is_empty() { "没有在任何分支上".to_string() } else { format!("在 {branch} 分支上") };
        return problem("not-main", format!("网站文件夹现在{now}，发布只走 main"));
    }
    if ["MERGE_HEAD", "rebase-merge", "rebase-apply", "CHERRY_PICK_HEAD", "REVERT_HEAD"].iter().any(|f| dot.join(f).exists()) {
        return problem("git-busy", "网站文件夹里有没做完的合并或变基，先在终端处理");
    }
    let Some(origin) = origin_url(folder) else { return problem("no-remote", "网站文件夹没有远端（origin）") };
    if sandbox && !is_local_remote(&origin) {
        return problem("real-remote", "开发版只推到本机的假远端，这个文件夹连着真的 GitHub");
    }
    None
}

/// 建副本: the site folder copied (APFS clones: no extra space) + a bare repo of it as the remote.
pub fn create_sandbox(src: &Path, root: &Path) -> Result<(), String> {
    let (site, remote) = (root.join("website"), root.join("remote.git"));
    if site.exists() {
        return Err("副本已经在了".into());
    }
    if !src.join(".git").exists() {
        return Err(format!("找不到要复制的网站文件夹：{}", src.display()));
    }
    std::fs::create_dir_all(root).map_err(|e| format!("建不了副本的文件夹：{e}"))?;
    let _ = std::fs::remove_dir_all(&remote);
    let copy = |flags: &str| tools::run(Path::new("/bin/cp"), root, &[flags, &src.to_string_lossy(), &site.to_string_lossy()], None, Duration::from_secs(600));
    if !copy("-cRp")?.ok {
        let _ = std::fs::remove_dir_all(&site);
        let out = copy("-Rp")?;
        if !out.ok {
            let _ = std::fs::remove_dir_all(&site);
            return Err(format!("复制失败：{}", out.tail()));
        }
    }
    let t = Duration::from_secs(120);
    let set_up = || -> Result<(), String> {
        tools::git(root, &["clone", "--quiet", "--bare", "website", "remote.git"], t)?;
        tools::git(&site, &["remote", "set-url", "origin", &remote.to_string_lossy()], t)?;
        let _ = tools::git(&site, &["config", "--unset-all", "remote.origin.pushurl"], t);
        tools::git(&site, &["fetch", "--quiet", "origin"], t)?;
        Ok(())
    };
    set_up().map_err(|e| {
        let _ = std::fs::remove_dir_all(&site);
        let _ = std::fs::remove_dir_all(&remote);
        format!("副本建好了但连不上假远端：{e}")
    })
}

/// 发布… first: fetch, fast-forward only, then the list to confirm.
pub fn prepare(folder: &Path, sandbox: bool) -> Result<Changes, String> {
    if let Some(p) = check_folder(folder, sandbox, false) {
        return Err(p.text);
    }
    publish::fetch(folder)?;
    let (ahead, behind) = publish::behind(folder)?;
    if behind > 0 && ahead > 0 {
        return Err("这里有还没推送的提交，GitHub 上也有新的提交（分叉了），先在终端处理。".into());
    }
    if behind > 0 {
        tools::git(folder, &["merge", "--ff-only", "--quiet", "origin/main"], Duration::from_secs(60)).map_err(|_| "GitHub 上有新的提交，但和这里没提交的改动碰到了同一个文件，拉不下来。先在终端处理。".to_string())?;
    }
    changes::list(folder)
}

// ---------- commands ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// real | sandbox
    pub mode: &'static str,
    pub folder: String,
    pub is_default: bool,
    pub problem: Option<Problem>,
    pub site_url: Option<String>,
    pub github_url: Option<String>,
    pub server: ServerView,
    pub job: Option<Job>,
}

fn folder_of(app: &AppHandle) -> (PathBuf, bool) {
    let site = app.state::<Site>();
    let set = app.state::<AppState>().with_db(|c| crate::settings::get(c, FOLDER_KEY)).ok().flatten().and_then(|v| v.as_str().map(PathBuf::from)).filter(|p| !p.as_os_str().is_empty());
    match set {
        Some(p) => {
            let is_default = p == site.default_folder();
            (p, is_default)
        }
        None => (site.default_folder(), true),
    }
}

fn overview(app: &AppHandle) -> Overview {
    let site = app.state::<Site>();
    let (folder, is_default) = folder_of(app);
    let problem = check_folder(&folder, site.sandbox(), is_default);
    let github_url = if problem.is_none() { origin_url(&folder).as_deref().and_then(github_repo).map(|(o, n)| format!("https://github.com/{o}/{n}")) } else { None };
    Overview {
        mode: if site.sandbox() { "sandbox" } else { "real" },
        folder: folder.display().to_string(),
        is_default,
        site_url: site_url(&folder),
        github_url,
        problem,
        server: site.server.view(site.port()),
        job: site.publisher.view(),
    }
}

/// A usable folder or the reason it isn't.
fn ready_folder(app: &AppHandle) -> Result<PathBuf, String> {
    let site = app.state::<Site>();
    let (folder, is_default) = folder_of(app);
    match check_folder(&folder, site.sandbox(), is_default) {
        Some(p) => Err(p.text),
        None => Ok(folder),
    }
}

async fn blocking<T: Send + 'static>(app: AppHandle, f: impl FnOnce(&AppHandle) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || f(&app)).await.map_err(|e| format!("出错了：{e}"))?
}

#[tauri::command]
pub async fn site_overview(app: AppHandle) -> Result<Overview, String> {
    blocking(app, |a| Ok(overview(a))).await
}

#[tauri::command]
pub async fn site_changes(app: AppHandle) -> Result<Changes, String> {
    blocking(app, |a| changes::list(&ready_folder(a)?)).await
}

#[tauri::command]
pub async fn site_diff(app: AppHandle, path: String) -> Result<Diff, String> {
    blocking(app, move |a| changes::diff(&ready_folder(a)?, &path)).await
}

#[tauri::command]
pub async fn site_prepare(app: AppHandle) -> Result<Changes, String> {
    blocking(app, |a| {
        let site = a.state::<Site>();
        if site.publisher.running() {
            return Err("正在发布，等这次发完".into());
        }
        prepare(&ready_folder(a)?, site.sandbox())
    })
    .await
}

#[tauri::command]
pub async fn site_publish(app: AppHandle, message: String, token: String) -> Result<Overview, String> {
    blocking(app, move |a| {
        let site = a.state::<Site>();
        let folder = ready_folder(a)?;
        if site.publisher.running() {
            return Err("正在发布，等这次发完".into());
        }
        let now = changes::list(&folder)?;
        if now.token != token {
            return Err("改动和确认框里的不一样了，再点一次「发布…」".into());
        }
        if now.is_empty() {
            return Err("没有要发布的改动".into());
        }
        if now.blocked() {
            return Err("有文件超过了网站规定的大小，不能发布".into());
        }
        if !now.files.is_empty() && message.trim().is_empty() {
            return Err("写好 commit 文本再发布".into());
        }
        let node_bin = tools::node_bin(&folder, &site.home)?;
        let repo = origin_url(&folder).as_deref().and_then(github_repo);
        let plan = Plan {
            check: (node_bin.join("npm"), vec!["run".into(), "qa".into()]),
            node_bin: Some(node_bin),
            sandbox: site.sandbox(),
            folder,
            message,
            token,
            repo,
            gh: tools::gh(),
            fake_deploy: Duration::from_secs(3),
        };
        let id = site.publisher.begin(&plan).ok_or("正在发布，等这次发完")?;
        publish::spawn(site.publisher.clone(), id, plan);
        Ok(overview(a))
    })
    .await
}

#[tauri::command]
pub async fn site_dismiss(app: AppHandle) -> Result<Overview, String> {
    blocking(app, |a| {
        a.state::<Site>().publisher.dismiss();
        Ok(overview(a))
    })
    .await
}

#[tauri::command]
pub async fn site_history(app: AppHandle) -> Result<History, String> {
    blocking(app, |a| {
        let site = a.state::<Site>();
        let folder = ready_folder(a)?;
        let repo = origin_url(&folder).as_deref().and_then(github_repo);
        publish::history(&folder, site.sandbox(), tools::gh().as_deref(), repo.as_ref())
    })
    .await
}

/// Starts the dev server (unless something answers already) and waits until it does.
fn ensure_server(a: &AppHandle) -> Result<ServerView, String> {
    let site = a.state::<Site>();
    let (folder, _) = folder_of(a);
    if !folder.is_dir() {
        return Err(format!("找不到网站文件夹：{}", folder.display()));
    }
    if !server::port_open(site.port()) {
        let node_bin = tools::node_bin(&folder, &site.home)?;
        site.server.start(&folder, &node_bin, site.port())?;
    }
    site.server.wait_ready(site.port(), server::START_WAIT)
}

#[tauri::command]
pub async fn site_server_start(app: AppHandle) -> Result<ServerView, String> {
    blocking(app, |a| ensure_server(a)).await
}

#[tauri::command]
pub async fn site_server_stop(app: AppHandle) -> Result<ServerView, String> {
    blocking(app, |a| {
        let site = a.state::<Site>();
        site.server.stop();
        std::thread::sleep(Duration::from_millis(400));
        Ok(site.server.view(site.port()))
    })
    .await
}

// ---------- 编辑 ----------

fn image_tmp() -> PathBuf {
    std::env::temp_dir().join(format!("pomelo-site-{}", std::process::id()))
}

#[tauri::command]
pub async fn site_works(app: AppHandle) -> Result<Works, String> {
    blocking(app, |a| wander::list(&ready_folder(a)?)).await
}

#[tauri::command]
pub async fn site_work(app: AppHandle, slug: String) -> Result<Work, String> {
    blocking(app, move |a| wander::load(&ready_folder(a)?, &slug)).await
}

#[tauri::command]
pub async fn site_work_save(app: AppHandle, work: Work) -> Result<Work, String> {
    blocking(app, move |a| {
        let folder = ready_folder(a)?;
        wander::save(&folder, &work, &wander::taxonomy(&folder))
    })
    .await
}

/// 新作品 (this month as its 年月).
#[tauri::command]
pub async fn site_work_create(app: AppHandle, slug: String, lang: String, title: String) -> Result<Work, String> {
    blocking(app, move |a| {
        let folder = ready_folder(a)?;
        let month = a.state::<AppState>().with_db(|c| crate::modules::common::current_month(c)).unwrap_or_default();
        wander::create(&folder, slug.trim(), &lang, &title, &month)
    })
    .await
}

#[tauri::command]
pub async fn site_work_add_version(app: AppHandle, slug: String, lang: String, prints: Prints) -> Result<Work, String> {
    blocking(app, move |a| wander::add_version(&ready_folder(a)?, &slug, &lang, &prints)).await
}

#[tauri::command]
pub async fn site_work_rename(app: AppHandle, slug: String, to: String, prints: Prints) -> Result<Work, String> {
    blocking(app, move |a| wander::rename(&ready_folder(a)?, &slug, to.trim(), &prints)).await
}

#[tauri::command]
pub async fn site_work_delete(app: AppHandle, slug: String, prints: Prints) -> Result<(), String> {
    blocking(app, move |a| wander::delete(&ready_folder(a)?, &slug, &prints)).await
}

/// What an added file is and its 用途 in the library, by where it was added.
fn target_of(folder: &Path, target: &str) -> Result<(&'static str, String), String> {
    Ok(match target {
        "now" => ("image", "Now".into()),
        "about" => ("image", "关于我".into()),
        "career" => ("image", "职业页".into()),
        "portraits" => ("image", "关于我 · 肖像".into()),
        "artworks" => ("image", "关于我 · 画作 / 摄影".into()),
        "cv:zh" => ("document", "职业页 · 中文简历".into()),
        "cv:en" => ("document", "职业页 · 英文简历".into()),
        "asset:image" => ("image", String::new()),
        "asset:document" => ("document", String::new()),
        t => match t.strip_prefix("wander:").filter(|s| wander::valid_slug(s)) {
            Some(slug) => ("image", format!("作品《{}》", wander::title_of(folder, slug).unwrap_or_else(|| slug.into()))),
            None => return Err("放到哪里不对".into()),
        },
    })
}

fn add_file(folder: &Path, target: &str, src: &Path, name: &str) -> Result<String, String> {
    let (kind, usage) = target_of(folder, target)?;
    images::add(folder, kind, src, name, &usage, &image_tmp())
}

/// Dropped / chosen files into the library by target (wander:<slug> | now | about | career | portraits |
/// artworks | cv:zh | cv:en | asset:image | asset:document) → their URLs on the site, in order.
#[tauri::command]
pub async fn site_files_add(app: AppHandle, target: String, paths: Vec<String>) -> Result<Vec<String>, String> {
    blocking(app, move |a| {
        let folder = ready_folder(a)?;
        paths
            .iter()
            .map(|p| {
                let src = Path::new(p);
                add_file(&folder, &target, src, &src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
            })
            .collect()
    })
    .await
}

/// Pasted / dropped bytes (the browser's File); name and target travel percent-encoded in headers.
#[tauri::command]
pub async fn site_image_bytes(app: AppHandle, request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("没有收到图片".into());
    };
    let header = |k: &str| request.headers().get(k).and_then(|v| v.to_str().ok()).map(crate::modules::attachments::percent_decode).unwrap_or_default();
    let (name, target, data) = (header("x-name"), header("x-target"), data.clone());
    blocking(app, move |a| {
        let folder = ready_folder(a)?;
        let tmp = image_tmp();
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let name = if name.is_empty() { "image.png".to_string() } else { name };
        let at = tmp.join(format!("pasted-{}", images::clean_stem(&name)));
        std::fs::write(&at, &data).map_err(|e| e.to_string())?;
        let r = add_file(&folder, &target, &at, &name);
        let _ = std::fs::remove_file(&at);
        r
    })
    .await
}

// ---------- Now、当下状态 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowList {
    pub months: Vec<now::Row>,
    pub status: now::Status,
}

#[tauri::command]
pub async fn site_now_list(app: AppHandle) -> Result<NowList, String> {
    blocking(app, |a| {
        let folder = ready_folder(a)?;
        Ok(NowList { months: now::list(&folder)?, status: now::status(&folder)? })
    })
    .await
}

#[tauri::command]
pub async fn site_now(app: AppHandle, month: String) -> Result<now::Month, String> {
    blocking(app, move |a| now::load(&ready_folder(a)?, &month)).await
}

#[tauri::command]
pub async fn site_now_save(app: AppHandle, month: now::Month) -> Result<now::Month, String> {
    blocking(app, move |a| now::save(&ready_folder(a)?, &month)).await
}

#[tauri::command]
pub async fn site_now_create(app: AppHandle, month: String, lang: String) -> Result<now::Month, String> {
    blocking(app, move |a| now::create(&ready_folder(a)?, &month, &lang)).await
}

#[tauri::command]
pub async fn site_now_delete(app: AppHandle, month: String, prints: Prints) -> Result<(), String> {
    blocking(app, move |a| now::delete(&ready_folder(a)?, &month, &prints)).await
}

#[tauri::command]
pub async fn site_status_save(app: AppHandle, status: now::Status) -> Result<now::Status, String> {
    blocking(app, move |a| now::save_status(&ready_folder(a)?, &status)).await
}

// ---------- 关于我、职业页 ----------

#[tauri::command]
pub async fn site_about(app: AppHandle) -> Result<about::About, String> {
    blocking(app, |a| about::load(&ready_folder(a)?)).await
}

#[tauri::command]
pub async fn site_about_save(app: AppHandle, about: about::About) -> Result<about::About, String> {
    blocking(app, move |a| about::save(&ready_folder(a)?, &about)).await
}

#[tauri::command]
pub async fn site_career(app: AppHandle) -> Result<career::Career, String> {
    blocking(app, |a| career::load(&ready_folder(a)?)).await
}

#[tauri::command]
pub async fn site_career_save(app: AppHandle, career: career::Career) -> Result<career::Career, String> {
    blocking(app, move |a| career::save(&ready_folder(a)?, &career)).await
}

// ---------- 资源库 ----------

#[tauri::command]
pub async fn site_registry(app: AppHandle) -> Result<Vec<registry::Entry>, String> {
    blocking(app, |a| Ok(registry::library(&ready_folder(a)?))).await
}

#[tauri::command]
pub async fn site_registry_save(app: AppHandle, entry: registry::Entry) -> Result<registry::Entry, String> {
    blocking(app, move |a| registry::save(&ready_folder(a)?, &entry)).await
}

#[tauri::command]
pub async fn site_registry_citations(app: AppHandle, kind: String, slug: String) -> Result<Vec<registry::Citation>, String> {
    blocking(app, move |a| registry::citations(&ready_folder(a)?, &kind, &slug)).await
}

#[tauri::command]
pub async fn site_registry_delete(app: AppHandle, kind: String, slug: String, print: Option<String>, with_file: bool) -> Result<(), String> {
    blocking(app, move |a| registry::delete(&ready_folder(a)?, &kind, &slug, print.as_ref(), with_file)).await
}

#[tauri::command]
pub async fn site_registry_unregistered(app: AppHandle, kind: String) -> Result<Vec<String>, String> {
    blocking(app, move |a| registry::unregistered(&ready_folder(a)?, &kind)).await
}

/// Registers the library files put there by hand; returns how many.
#[tauri::command]
pub async fn site_registry_adopt(app: AppHandle, kind: String) -> Result<usize, String> {
    blocking(app, move |a| registry::adopt(&ready_folder(a)?, &kind)).await
}

// ---------- 站点 ----------

#[tauri::command]
pub async fn site_terms(app: AppHandle) -> Result<terms::SiteData, String> {
    blocking(app, |a| terms::load(&ready_folder(a)?)).await
}

#[tauri::command]
pub async fn site_terms_save(app: AppHandle, terms: terms::TermsIn, print: Option<String>) -> Result<terms::SiteData, String> {
    blocking(app, move |a| {
        let folder = ready_folder(a)?;
        terms::save_terms(&folder, &terms, print.as_ref())?;
        terms::load(&folder)
    })
    .await
}

#[tauri::command]
pub async fn site_links_save(app: AppHandle, links: Vec<terms::Link>, print: Option<String>) -> Result<terms::SiteData, String> {
    blocking(app, move |a| {
        let folder = ready_folder(a)?;
        terms::save_links(&folder, &links, print.as_ref())?;
        terms::load(&folder)
    })
    .await
}

/// 资源库 · 图片.
#[tauri::command]
pub async fn site_assets(app: AppHandle, kind: Option<String>) -> Result<Vec<Asset>, String> {
    blocking(app, move |a| Ok(images::assets(&ready_folder(a)?, kind.as_deref().unwrap_or("image")))).await
}

/// A site image for the page (`pomelo-site://localhost/<url>`).
pub fn public_image(app: &AppHandle, url: &str) -> Option<(Vec<u8>, &'static str)> {
    let (folder, _) = folder_of(app);
    images::read_public(&folder, url)
}

/// 预览: the page on the local dev server (started if needed), in Pomelo's preview window.
#[tauri::command]
pub async fn site_preview(app: AppHandle, path: String) -> Result<(), String> {
    if !path.starts_with('/') || path.contains("://") {
        return Err("预览的地址不对".into());
    }
    let view = blocking(app.clone(), |a| ensure_server(a)).await?;
    let url: tauri::Url = format!("{}{}", view.url.trim_end_matches('/'), path).parse().map_err(|_| "预览的地址不对".to_string())?;
    match app.get_webview_window(PREVIEW) {
        Some(w) => {
            w.navigate(url).map_err(|e| e.to_string())?;
            let _ = w.show();
            let _ = w.set_focus();
        }
        None => {
            tauri::WebviewWindowBuilder::new(&app, PREVIEW, tauri::WebviewUrl::External(url)).title("预览").inner_size(1180.0, 820.0).build().map_err(|e| format!("开不了预览窗口：{e}"))?;
        }
    }
    Ok(())
}

/// 建副本 (sandbox only).
#[tauri::command]
pub async fn site_sandbox(app: AppHandle) -> Result<Overview, String> {
    blocking(app, |a| {
        let site = a.state::<Site>();
        if !site.sandbox() {
            return Err("正式版直接用网站文件夹".into());
        }
        let src = std::env::var_os(SOURCE_ENV).map(PathBuf::from).unwrap_or_else(|| site.real_folder());
        create_sandbox(&src, &site.sandbox_root)?;
        Ok(overview(a))
    })
    .await
}

/// 设置 → 呦呦漫游 → 网站文件夹 (None = back to the default).
#[tauri::command]
pub async fn site_set_folder(app: AppHandle, folder: Option<String>) -> Result<Overview, String> {
    blocking(app, move |a| {
        if a.state::<Site>().publisher.running() {
            return Err("正在发布，等这次发完再换文件夹".into());
        }
        let value = match folder.map(|f| f.trim().to_string()).filter(|f| !f.is_empty()) {
            Some(f) => {
                let p = PathBuf::from(&f);
                if !p.is_dir() {
                    return Err(format!("找不到这个文件夹：{f}"));
                }
                if !p.join("package.json").is_file() || !p.join(".git").exists() {
                    return Err("这个文件夹看起来不是网站项目（没有 package.json 或 .git）".into());
                }
                serde_json::Value::String(f)
            }
            None => serde_json::Value::Null,
        };
        let site = a.state::<Site>();
        site.server.stop();
        a.state::<AppState>().with_db(|c| crate::settings::set(c, FOLDER_KEY, &value))?;
        Ok(overview(a))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use changes::tests::{git, site_repo, write};

    #[test]
    fn remotes() {
        assert!(is_local_remote("/Users/x/site-sandbox/remote.git") && is_local_remote("file:///tmp/r.git") && is_local_remote("../r.git"));
        assert!(!is_local_remote("https://github.com/elenskye/elenskye-website.git") && !is_local_remote("git@github.com:o/r.git") && !is_local_remote(""));
        let want = Some(("elenskye".to_string(), "elenskye-website".to_string()));
        assert_eq!(github_repo("https://github.com/elenskye/elenskye-website.git"), want);
        assert_eq!(github_repo("git@github.com:elenskye/elenskye-website.git"), want);
        assert_eq!(github_repo("ssh://git@github.com/elenskye/elenskye-website"), want);
        assert_eq!(github_repo("https://gitlab.com/a/b"), None);
        assert_eq!(github_repo("/tmp/remote.git"), None);
    }

    #[test]
    fn modes_and_folders() {
        assert_eq!(mode(false), Mode::Sandbox);
        let s = Site::new(Mode::Real, Path::new("/Users/me"), Path::new("/data"));
        assert_eq!((s.default_folder(), s.port()), (PathBuf::from("/Users/me/Documents/GitHub/personal-website"), 4328));
        let s = Site::new(Mode::Sandbox, Path::new("/Users/me"), Path::new("/data"));
        assert_eq!((s.default_folder(), s.port()), (PathBuf::from("/data/site-sandbox/website"), 4338));
    }

    #[test]
    fn site_url_from_astro_config() {
        let dir = temp_dir("site-url");
        assert_eq!(site_url(&dir), None);
        write(&dir, "astro.config.mjs", "export default defineConfig({\n  site: 'https://elenskye.com/',\n  output: 'static',\n});\n");
        assert_eq!(site_url(&dir).as_deref(), Some("https://elenskye.com"));
    }

    #[test]
    fn folder_problems() {
        let dir = temp_dir("site-problems");
        assert_eq!(check_folder(&dir.join("website"), true, true).unwrap().kind, "no-sandbox");
        assert_eq!(check_folder(&dir.join("website"), false, true).unwrap().kind, "missing");
        std::fs::create_dir_all(dir.join("plain")).unwrap();
        assert_eq!(check_folder(&dir.join("plain"), true, false).unwrap().kind, "not-git");
        let (site, _) = site_repo(&dir);
        assert_eq!(check_folder(&site, true, true), None);
        git(&site, &["switch", "-q", "-c", "content/x"]);
        assert!(check_folder(&site, true, true).unwrap().text.contains("content/x"));
        git(&site, &["switch", "-q", "main"]);
        std::fs::write(site.join(".git/MERGE_HEAD"), "x").unwrap();
        assert_eq!(check_folder(&site, true, true).unwrap().kind, "git-busy");
        std::fs::remove_file(site.join(".git/MERGE_HEAD")).unwrap();
        git(&site, &["remote", "set-url", "origin", "https://github.com/elenskye/elenskye-website.git"]);
        assert_eq!(check_folder(&site, true, true).unwrap().kind, "real-remote");
        assert_eq!(check_folder(&site, false, true), None);
        git(&site, &["remote", "remove", "origin"]);
        assert_eq!(check_folder(&site, false, true).unwrap().kind, "no-remote");
    }

    #[test]
    fn sandbox_is_a_copy_with_its_own_remote() {
        let dir = temp_dir("site-sandbox");
        let (src, real_remote) = site_repo(&dir.join("src"));
        write(&src, "src/content/now/2026-10.zh.md", "---\nlanguage: zh\n---\n还没提交的\n");
        let root = dir.join("data/site-sandbox");
        create_sandbox(&src, &root).unwrap();
        let site = root.join("website");
        assert_eq!(origin_url(&site).unwrap(), root.join("remote.git").to_string_lossy());
        assert_eq!(check_folder(&site, true, true), None);
        // the uncommitted change came along; the real folder and its remote are untouched
        let c = changes::list(&site).unwrap();
        assert_eq!(c.files.iter().map(|f| f.label.as_str()).collect::<Vec<_>>(), ["Now 2026-10 中文"]);
        assert_eq!(origin_url(&src).unwrap(), real_remote.to_string_lossy());
        assert!(create_sandbox(&src, &root).unwrap_err().contains("已经在了"));
        assert!(create_sandbox(&dir.join("nope"), &dir.join("other")).unwrap_err().contains("找不到"));
    }

    #[test]
    fn prepare_fast_forwards_or_stops() {
        let dir = temp_dir("site-prepare");
        let (site, remote) = site_repo(&dir);
        let other = dir.join("other");
        git(&dir, &["clone", "-q", remote.to_str().unwrap(), "other"]);
        write(&other, "src/content/wander/big-fish.zh.md", "---\ntitle: 别处改的\n---\n");
        git(&other, &["commit", "-qam", "别处改的"]);
        git(&other, &["push", "-q", "origin", "main"]);
        // a local change in another file: pulled in, change kept
        write(&site, "src/components/Wall.astro", "<p/>\n");
        let c = prepare(&site, true).unwrap();
        assert_eq!(c.files.len(), 1);
        assert!(std::fs::read_to_string(site.join("src/content/wander/big-fish.zh.md")).unwrap().contains("别处改的"));
        // the same file changed on both sides: stop, nothing lost
        write(&other, "src/components/Wall.astro", "<b/>\n");
        git(&other, &["commit", "-qam", "又改"]);
        git(&other, &["push", "-q", "origin", "main"]);
        assert!(prepare(&site, true).unwrap_err().contains("拉不下来"));
        assert_eq!(std::fs::read_to_string(site.join("src/components/Wall.astro")).unwrap(), "<p/>\n");
        // diverged: a local commit and a new remote one
        git(&site, &["commit", "-qam", "本地的"]);
        assert!(prepare(&site, true).unwrap_err().contains("分叉"));
    }

    /// Read only on the real site folder: the change list, 最近发布 with Vercel's results through gh, the
    /// site and project pages. `cargo test real_site_read -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_site_read() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let folder = home.join("Documents/GitHub/personal-website");
        assert_eq!(check_folder(&folder, false, true), None);
        let repo = origin_url(&folder).as_deref().and_then(github_repo);
        println!("网站 {:?} · 仓库 {repo:?} · gh {:?}", site_url(&folder), tools::gh());
        let c = changes::list(&folder).unwrap();
        println!("改动 {} 个，没推的提交 {} 个", c.files.len(), c.ahead.len());
        let h = publish::history(&folder, false, tools::gh().as_deref(), repo.as_ref()).unwrap();
        println!("Vercel 项目 {:?}", h.vercel_url);
        for i in &h.items {
            println!("{} {} {:?} {:?}", &i.sha[..7], i.subject, i.state, i.url);
        }
        assert!(h.items.iter().any(|i| i.state.is_some()));
    }

    /// Editing on a copy of the real site, then the site's own `npm run qa` must still pass: every work
    /// saved back, an English version added, a big photo imported (shrunk) as cover and gallery, a new
    /// work made and renamed. `cargo test real_site_edit -- --ignored --nocapture` (the real folder is only
    /// read).
    #[test]
    #[ignore]
    fn real_site_edit() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let src = std::env::var_os(SOURCE_ENV).map(PathBuf::from).unwrap_or_else(|| home.join("Documents/GitHub/personal-website"));
        let dir = temp_dir("site-real-edit");
        create_sandbox(&src, &dir.join("sb")).unwrap();
        let site = dir.join("sb/website");
        let works = wander::list(&site).unwrap();
        assert!(works.broken.is_empty(), "{:?}", works.broken);
        for r in &works.works {
            let w = wander::load(&site, &r.slug).unwrap();
            assert_eq!(wander::save(&site, &w, &works.taxonomy).unwrap().slug, r.slug);
        }
        println!("{} 个作品读了又存，改动：{:?}", works.works.len(), changes::list(&site).unwrap().files.iter().map(|f| f.label.clone()).collect::<Vec<_>>());
        // an English version for a Chinese-only work, with a body
        let only_zh = works.works.iter().find(|r| r.langs == ["zh"]).unwrap().slug.clone();
        let w = wander::load(&site, &only_zh).unwrap();
        let mut w = wander::add_version(&site, &only_zh, "en", &w.prints).unwrap();
        w.en.as_mut().unwrap().body = "## Test\n\nA *test* body with **bold** and a [link](https://elenskye.com).\n".into();
        // a big photo: a library one blown up, added (shrunk to ≤ 1.5 MB, registered)
        let jpg = std::fs::read_dir(site.join("public/assets/images")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "jpg")).min().unwrap();
        let big = dir.join("big.jpg");
        assert!(std::process::Command::new("/usr/bin/sips").args(["-Z", "5000", "-s", "formatOptions", "100"]).arg(&jpg).arg("--out").arg(&big).output().unwrap().status.success());
        println!("大图 {:.1} MB", std::fs::metadata(&big).unwrap().len() as f64 / 1048576.0);
        let url = add_file(&site, &format!("wander:{only_zh}"), &big, "Big Photo.JPG").unwrap();
        let stored = format!("public{url}");
        assert!(std::fs::metadata(site.join(&stored)).unwrap().len() <= changes::MAX_IMAGE);
        w.cover = Some(stored.clone());
        assert!(registry::list(&site).iter().any(|e| e.file == stored && e.usage.starts_with("作品《")));
        // an image already in the library: reused where it is
        let second = format!("public{}", add_file(&site, &format!("wander:{only_zh}"), &jpg, "second.jpg").unwrap());
        assert_eq!(site.join(&second), jpg);
        w.gallery.push(wander::GalleryItem { image: stored, alt_zh: "测试图".into(), alt_en: "Test image".into(), ..Default::default() });
        w.gallery.push(wander::GalleryItem { image: second, alt_zh: "第二张".into(), ..Default::default() });
        wander::save(&site, &w, &works.taxonomy).unwrap();
        // a new work, renamed
        let made = wander::create(&site, "pomelo-test", "zh", "Pomelo 测试", "2026-10").unwrap();
        let mut made = wander::rename(&site, "pomelo-test", "pomelo-test-2", &made.prints).unwrap();
        made.category = works.taxonomy.categories[0].slug.clone();
        made.topics = vec![works.taxonomy.topics[0].slug.clone()];
        made.zh.as_mut().unwrap().body = "正文第一段。\n\n- 一\n- 二\n\n> 引用\n".into();
        wander::save(&site, &made, &works.taxonomy).unwrap();
        // the other pages: saved as they are first (nothing may change), then edited
        let before = changes::list(&site).unwrap().files.len();
        let months = now::list(&site).unwrap();
        for m in &months {
            now::save(&site, &now::load(&site, &m.month).unwrap()).unwrap();
        }
        let st = now::status(&site).unwrap();
        now::save_status(&site, &st).unwrap();
        let ab = about::save(&site, &about::load(&site).unwrap()).unwrap();
        let ca = career::save(&site, &career::load(&site).unwrap()).unwrap();
        let sd = terms::load(&site).unwrap();
        terms::save_terms(&site, &sd.terms, sd.terms_print.as_ref()).unwrap();
        terms::save_links(&site, &sd.links, sd.links_print.as_ref()).unwrap();
        for e in registry::list(&site) {
            registry::save(&site, &e).unwrap();
        }
        assert_eq!(changes::list(&site).unwrap().files.len(), before, "saving unchanged pages changed files");
        // Now: a new month in both languages, the status line
        let month = (1..=12).map(|m| format!("2099-{m:02}")).find(|m| !site.join(format!("src/content/now/{m}.zh.md")).exists()).unwrap();
        now::create(&site, &month, "zh").unwrap();
        let m = now::create(&site, &month, "en").unwrap();
        now::save(&site, &now::Month { zh: Some("十月的测试。\n".into()), en: Some("An October test.\n".into()), ..m }).unwrap();
        now::save_status(&site, &now::Status { en: format!("{} (test)", st.en), ..now::status(&site).unwrap() }).unwrap();
        // 关于我: a time line row, a new portrait (a fresh image, shrunk)
        let mut ab = ab;
        if let Some(v) = ab.zh.as_mut() {
            v.timeline.push(about::Entry { role: "测试".into(), org: "Pomelo".into(), period: "2026".into(), description: "测试行".into() });
        }
        let portrait = format!("public{}", add_file(&site, "portraits", &big, "test portrait.jpg").unwrap());
        ab.images.portraits.push(portrait.clone());
        about::save(&site, &ab).unwrap();
        // 职业页: a module in the English page
        let mut ca = ca;
        if let Some(v) = ca.en.as_mut() {
            v.sections.push(career::Section { title: "Test".into(), mode: "simple".into(), simple: vec![career::Simple { heading: "Pomelo".into(), items: "A test row".into() }], detailed: vec![] });
        }
        career::save(&site, &ca).unwrap();
        // 资源库: a new document, its description; 站点: a topic and a link
        std::fs::write(dir.join("notes.pdf"), b"%PDF-1.4 test").unwrap();
        add_file(&site, "asset:document", &dir.join("notes.pdf"), "Test Notes.pdf").unwrap();
        let doc = registry::list(&site).into_iter().find(|e| e.slug == "test-notes").unwrap();
        registry::save(&site, &registry::Entry { note: "测试文档".into(), ..doc }).unwrap();
        let mut sd = terms::load(&site).unwrap();
        sd.terms.topics.push(terms::TermIn { slug: "pomelo-test".into(), zh: "测试".into(), en: "Test".into(), used: 0 });
        assert!(terms::save_terms(&site, &sd.terms, sd.terms_print.as_ref()).unwrap());
        sd.links.push(terms::Link { key: "website".into(), label: "Pomelo".into(), href: "https://example.com".into() });
        terms::save_links(&site, &sd.links, sd.links_print.as_ref()).unwrap();
        println!("改动：{:?}", changes::list(&site).unwrap().files.iter().map(|f| (f.action, f.label.clone())).collect::<Vec<_>>());
        let node_bin = tools::node_bin(&site, &home).unwrap();
        let log = tools::Log::default();
        let ok = tools::run_logged(&node_bin.join("npm"), &site, &["run", "qa"], Some(&node_bin), &log, Duration::from_secs(900), |_| {}).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        println!("npm run qa：{}\n{}", if ok { "通过" } else { "没过" }, log.tail(if ok { 8 } else { 60 }).join("\n"));
        assert!(ok);
    }

    /// The whole thing on a copy of the real site: 建副本, a change, `npm run qa` for real, commit and push
    /// to the sandbox's remote. The real folder is only read (copied).
    /// `cargo test real_site_sandbox -- --ignored --nocapture` (POMELO_SITE_SRC to copy another folder).
    #[test]
    #[ignore]
    fn real_site_sandbox() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let src = std::env::var_os(SOURCE_ENV).map(PathBuf::from).unwrap_or_else(|| home.join("Documents/GitHub/personal-website"));
        let dir = temp_dir("site-real");
        let root = dir.join("site-sandbox");
        let t = std::time::Instant::now();
        create_sandbox(&src, &root).unwrap();
        println!("建副本 {:.1}s", t.elapsed().as_secs_f32());
        let site = root.join("website");
        let status = site.join("src/data/now-status.json");
        let before = std::fs::read_to_string(&status).unwrap();
        // a real but harmless change: the same JSON, spaced differently
        std::fs::write(&status, before.replacen("\"zh\": ", "\"zh\":  ", 1)).unwrap();
        let c = prepare(&site, true).unwrap();
        println!("改动：{:?}", c.files.iter().map(|f| (&f.action, &f.label)).collect::<Vec<_>>());
        let node_bin = tools::node_bin(&site, &home).unwrap();
        println!("Node：{}", node_bin.display());
        let plan = Plan { check: (node_bin.join("npm"), vec!["run".into(), "qa".into()]), node_bin: Some(node_bin), sandbox: true, folder: site.clone(), message: "test: 副本里的发布\n\n#第二段".into(), token: c.token, repo: None, gh: None, fake_deploy: Duration::ZERO };
        let p = Publisher::default();
        let id = p.begin(&plan).unwrap();
        let t = std::time::Instant::now();
        p.execute(id, plan);
        let j = p.view().unwrap();
        println!("发布 {:.0}s：{} {} {:?}\n检查的输出（最后 25 行）：\n{}", t.elapsed().as_secs_f32(), j.step, j.state, j.error, p.log_tail(25).join("\n"));
        assert_eq!(j.state, "done");
        let pushed = git(&root.join("remote.git"), &["log", "-1", "--format=%B", "main"]);
        assert_eq!(pushed.trim_end(), "test: 副本里的发布\n\n#第二段");
        assert!(changes::list(&site).unwrap().is_empty());
    }

    /// Our dev server on a copy of the real site: stays in our process group (even where Astro thinks an AI
    /// agent runs it), serves the home page, and is gone after 停止.
    /// `cargo test real_site_server -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_site_server() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let src = std::env::var_os(SOURCE_ENV).map(PathBuf::from).unwrap_or_else(|| home.join("Documents/GitHub/personal-website"));
        let dir = temp_dir("site-real-server");
        let root = dir.join("site-sandbox");
        create_sandbox(&src, &root).unwrap();
        let site = root.join("website");
        let node_bin = tools::node_bin(&site, &home).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let s = server::Server::default();
        s.start(&site, &node_bin, port).unwrap();
        let t = std::time::Instant::now();
        let v = s.wait_ready(port, server::START_WAIT).unwrap();
        println!("本地服务 {:.1}s：{}", t.elapsed().as_secs_f32(), v.url);
        assert!(v.ours, "the dev server left our process group");
        let page = tools::run(Path::new("/usr/bin/curl"), &site, &["-sf", &v.url], None, Duration::from_secs(60)).unwrap();
        assert!(page.ok && page.stdout.contains("class=\"lucide lucide-search\""));
        s.stop();
        let gone = (0..50).any(|_| {
            std::thread::sleep(Duration::from_millis(200));
            !server::port_open(port)
        });
        assert!(gone && s.view(port).state == "stopped", "the dev server outlived 停止");
    }
}
