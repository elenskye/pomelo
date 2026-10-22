//! 小拾 · Picky (v0.3): the old Picky app rewritten into Pomelo — Pomelo's LLM service.
//! Rules (requirements.md「小拾」):
//! - Data: picky.db (store.rs), never pomelo.db. Only Rust reads/writes it.
//! - Models: local models via ~/Documents/LLM/scripts/llmctl.sh and config/models.json
//!   (runtime.rs, catalog.rs): chat one at a time on 8080, the embedding (8081) and rerank (8082)
//!   services Pomelo starts on demand and stops when idle; and three cloud services (cloud.rs,
//!   provider.rs). Pomelo never modifies the LLM folder.
//! - Mode: `Real` only for the installed app on the real data dir (or POMELO_REAL_MODELS=1);
//!   dev builds, tests and scratch data dirs are `Mock`: built-in catalog, simulated operations,
//!   canned replies, keys in memory — no script, no network, no keychain.
//! - Model control, cloud keys and 芝麻开门 are never AI tools.
//! - Selection ("local" | "cloud:<id>", preference `model.selection`) is shared by the page and the
//!   menu bar; it can't change while a reply is being written.

pub mod act;
pub mod assist;
pub mod assistants;
pub mod brief;
pub mod catalog;
pub mod chat;
pub mod cite;
pub mod cloud;
pub mod data;
pub mod context;
pub mod embed;
pub mod export;
pub mod files;
pub mod find;
pub mod index;
pub mod instructions;
pub mod knowledge;
pub mod library;
pub mod memory;
pub mod office;
pub mod pdf;
pub mod provider;
pub mod reading;
pub mod rerank;
pub mod schedule;
pub mod skills;
pub mod runtime;
pub mod storage;
pub mod store;
pub mod templates;
pub mod transfer;
pub mod word;
pub mod work;
pub mod yearly;

use crate::backup;
use crate::paths::DataPaths;
use crate::AppState;
use cloud::{CloudModel, KeyStore, Service};
use provider::Target;
use rusqlite::Connection;
use runtime::{Action, IssueKind, LocalStatus, Runtime, ServiceStatus, Svc};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

pub const REAL_ENV: &str = "POMELO_REAL_MODELS";
const SELECTION_KEY: &str = "model.selection";
/// The embedding / rerank model chosen in settings (a manifest key).
fn service_key(svc: Svc) -> String {
    format!("model.{}", svc.arg())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Real,
    Mock,
}

/// `data_dir_overridden`: POMELO_DATA_DIR is set (scratch runs of a release build).
pub fn mode(data_dir_overridden: bool) -> Mode {
    if cfg!(test) {
        return Mode::Mock;
    }
    if std::env::var(REAL_ENV).as_deref() == Ok("1") {
        return Mode::Real;
    }
    if cfg!(debug_assertions) || data_dir_overridden {
        Mode::Mock
    } else {
        Mode::Real
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Local,
    Cloud(String),
}

impl Selection {
    fn parse(s: &str) -> Self {
        match s.strip_prefix("cloud:") {
            Some(id) if !id.is_empty() => Selection::Cloud(id.into()),
            _ => Selection::Local,
        }
    }
    fn code(&self) -> String {
        match self {
            Selection::Local => "local".into(),
            Selection::Cloud(id) => format!("cloud:{id}"),
        }
    }
}

pub struct Picky {
    pub mode: Mode,
    conn: Mutex<Option<Connection>>,
    error: Option<String>,
    db_path: PathBuf,
    files_dir: PathBuf,
    pub runtime: Runtime,
    pub keys: KeyStore,
    pub runs: chat::Runs,
    pub index_jobs: index::Jobs,
    /// 知识库 scans and builds (knowledge.rs).
    pub kb: knowledge::Jobs,
    /// 助手 at work (assistants/).
    pub assistants: assistants::State,
    app: OnceLock<AppHandle>,
}

impl Picky {
    /// Opens picky.db (a failure only disables 小拾; the rest of Pomelo keeps working).
    pub fn init(paths: &DataPaths, home: &Path, mode: Mode) -> Self {
        let existed = paths.picky_db.exists();
        let opened = store::open(&paths.picky_db, &paths.backups).inspect(|c| {
            if existed {
                if let Err(e) = backup::daily_of(backup::Store::Picky, c, &paths.backups) {
                    eprintln!("picky daily backup failed: {e}");
                }
            }
        });
        let (conn, error) = match opened {
            Ok(c) => (Some(c), None),
            Err(e) => (None, Some(e)),
        };
        let runtime = Runtime::new(mode, runtime::LlmPaths::new(home));
        for svc in Svc::ALL {
            runtime.choose(svc, conn.as_ref().and_then(|c| store::pref_get::<String>(c, &service_key(svc)).ok().flatten()));
        }
        Picky {
            mode,
            conn: Mutex::new(conn),
            error,
            db_path: paths.picky_db.clone(),
            files_dir: paths.picky_files.clone(),
            runtime,
            keys: if mode == Mode::Real { KeyStore::keychain() } else { KeyStore::memory() },
            runs: chat::Runs::default(),
            index_jobs: index::Jobs::default(),
            kb: knowledge::Jobs::default(),
            assistants: assistants::State::default(),
            app: OnceLock::new(),
        }
    }

    /// Where stored copies of attachments and project files live (picky-files/).
    pub fn files_dir(&self) -> &Path {
        &self.files_dir
    }

    pub fn is_open(&self) -> bool {
        self.conn.lock().map(|c| c.is_some()).unwrap_or(false)
    }

    pub(crate) fn with_db<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T, String>) -> Result<T, String> {
        let mut guard = self.conn.lock().map_err(|_| "小拾的数据库连接异常，请重启 App".to_string())?;
        match guard.as_mut() {
            Some(c) => f(c),
            None => Err(format!("小拾的数据库没有打开：{}", self.error.as_deref().unwrap_or("未知原因"))),
        }
    }

    pub fn set_app(&self, app: AppHandle) {
        let _ = self.app.set(app);
    }

    /// Something the menu bar shows changed.
    pub fn changed(&self) {
        if let Some(app) = self.app.get() {
            crate::tray::refresh(app);
        }
    }

    pub fn selection(&self) -> Selection {
        self.with_db(|c| store::pref_get::<String>(c, SELECTION_KEY)).ok().flatten().map_or(Selection::Local, |s| Selection::parse(&s))
    }

    fn set_selection(&self, s: &Selection) -> Result<(), String> {
        self.with_db(|c| store::pref_set(c, SELECTION_KEY, &s.code()))
    }

    pub fn status(&self) -> Status {
        let cloud = self.with_db(|c| cloud::models(c)).unwrap_or_default();
        let selection = match self.selection() {
            Selection::Cloud(id) if !cloud.iter().any(|m| m.id == id) => Selection::Local, // deleted meanwhile
            s => s,
        };
        Status {
            mode: self.mode,
            error: self.error.clone(),
            local: self.runtime.local_status(),
            embedding: self.runtime.service_status(Svc::Embedding),
            rerank: self.runtime.service_status(Svc::Rerank),
            selection: selection.code(),
            keys: Service::ALL.iter().map(|s| KeyInfo { service: *s, name: s.name().into(), saved: self.keys.has(s.code()) }).collect(),
            cloud,
            generating: self.runs.busy(),
            active: self.runs.list(),
            unseen: self.with_db(|c| Ok(schedule::unseen(c))).unwrap_or(0),
        }
    }

    /// Where a new reply (or summary) goes.
    pub fn target(&self) -> Result<Route, String> {
        self.route(true)
    }

    /// For measuring only (the context check while typing): never reads the keychain, so a cloud
    /// route carries no key and must not be sent to.
    pub fn measure_route(&self) -> Result<Route, String> {
        self.route(false)
    }

    fn route(&self, with_key: bool) -> Result<Route, String> {
        self.route_with(&self.selection(), with_key)
    }

    /// Where work for `selection` goes (a 定时任务 has its own model); local = the running model.
    fn route_with(&self, selection: &Selection, with_key: bool) -> Result<Route, String> {
        match selection.clone() {
            Selection::Local => {
                let Some((m, served)) = self.runtime.ready_model() else {
                    let s = self.runtime.local_status();
                    return Err(if s.working || s.readiness == runtime::Readiness::Loading { "模型还在加载，等它就绪再发送。".into() } else { "本地模型还没有运行。".into() });
                };
                let (target, window) = if self.mode == Mode::Mock { (Target::Mock { thinking: m.thinking }, context::MOCK_WINDOW) } else { (Target::Local { model: served }, m.context) };
                Ok(Route { target, model: format!("local:{}", m.key), name: m.name, window, think: cloud::Think::local(m.thinking) })
            }
            Selection::Cloud(id) => {
                let m = self.with_db(|c| cloud::model(c, &id))?.ok_or("这个云端模型已经被删除了，请重新选择。")?;
                let key = if with_key { self.keys.read(m.service.code()).map_err(|_| "尚未保存这个云端接口的 API Key。".to_string())? } else { String::new() };
                let (target, window) = if self.mode == Mode::Mock { (Target::Mock { thinking: m.thinking != cloud::Think::Never }, context::MOCK_WINDOW) } else { (Target::Cloud { model: m.clone(), key }, m.context_window) };
                Ok(Route { target, model: format!("cloud:{}", m.id), name: m.name, window, think: m.thinking })
            }
        }
    }
}

/// Where model work goes: the backend, the run's model id, the name shown in history, and the
/// window from the catalog / cloud settings (a local server's own value wins, context.rs).
#[derive(Clone)]
pub struct Route {
    pub target: Target,
    pub model: String,
    pub name: String,
    pub window: i64,
    /// What the model can do about thinking.
    pub think: cloud::Think,
}

impl Route {
    /// The reply cap with room for reasoning when this work thinks.
    pub fn with_thinking(&self, max: i64, thinking: bool) -> i64 {
        if self.think.thinks(thinking) { max + context::think_room(self.window) } else { max }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    pub service: Service,
    pub name: String,
    pub saved: bool,
}

/// Everything the page, settings and the menu bar show about models.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub mode: Mode,
    /// picky.db could not be opened.
    pub error: Option<String>,
    pub local: LocalStatus,
    pub embedding: ServiceStatus,
    pub rerank: ServiceStatus,
    /// "local" | "cloud:<id>"
    pub selection: String,
    pub cloud: Vec<CloudModel>,
    pub keys: Vec<KeyInfo>,
    /// Anything being written.
    pub generating: bool,
    /// What is being written, one per conversation (to show and stop it after the page reloaded).
    pub active: Vec<chat::ActiveRun>,
    /// Conversations from 定时任务 not opened yet (the dot on 小拾 in the sidebar).
    pub unseen: i64,
}

/// Polls model status (local API every 3 s, every second during an operation; the embedding and
/// rerank services every 5 s), stops the services Pomelo started once idle (checked every 30 s)
/// and keeps the menu bar current. Mock mode only refreshes the menu bar and the idle stops.
/// Every 5 minutes it also takes the day's backups if missing: Pomelo may stay hidden in the menu
/// bar for days, so launch / window focus alone can no longer be relied on. Every 30 s (first 5 s
/// after launch) it runs the 定时任务 that are due (schedule.rs), and every 10 minutes it compares the
/// 知识库 folders with their index (knowledge.rs).
pub fn spawn_monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick: u64 = 0;
        loop {
            let st = app.state::<AppState>();
            if tick % 3 == 0 || st.picky.runtime.local_status().working {
                st.picky.runtime.refresh(false).await;
            }
            if tick % 5 == 0 {
                for svc in Svc::ALL {
                    st.picky.runtime.service_refresh(svc, false).await;
                }
            }
            if tick % 30 == 29 {
                for svc in st.picky.runtime.idle_services(std::time::Instant::now()) {
                    spawn_op(&app, move |app| async move { app.state::<AppState>().picky.runtime.service_control(svc, "stop").await });
                }
            }
            if tick % 30 == 5 {
                tauri::async_runtime::spawn(schedule::check(app.clone()));
            }
            if tick % knowledge::SCAN_EVERY == knowledge::SCAN_EVERY - 1 && st.picky.is_open() {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { knowledge::scan(&app).await });
            }
            crate::tray::refresh(&app);
            if tick % 300 == 299 {
                let dir = st.paths.backups.clone();
                let _ = st.with_db(|c| backup::daily(c, &dir));
                if st.picky.is_open() {
                    let _ = st.picky.with_db(|c| backup::daily_of(backup::Store::Picky, c, &dir));
                }
            }
            tick += 1;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

/// Runs a model operation in the background, then updates the menu bar.
fn spawn_op<F>(app: &AppHandle, op: impl FnOnce(AppHandle) -> F + Send + 'static)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        op(app.clone()).await;
        app.state::<AppState>().picky.changed();
    });
}

const LOCAL_BUSY: &str = "本地模型正在回答，等它答完再换。";

/// "local:<key>" switches to (starting if needed) that local model; "cloud:<id>" picks a cloud model.
/// Any time (replies being written keep their model), except changing the local model while it answers.
pub fn select(app: &AppHandle, choice: &str) -> Result<(), String> {
    let st = app.state::<AppState>();
    let p = &st.picky;
    if let Some(key) = choice.strip_prefix("local:") {
        let running = p.runtime.local_status().active.is_some_and(|m| m.key == key);
        if !running && p.runs.using("local") {
            return Err(LOCAL_BUSY.into());
        }
        p.set_selection(&Selection::Local)?;
        let _ = p.with_db(|c| Ok(assist::remember_local(c, key)));
        if !running {
            let key = key.to_string();
            spawn_op(app, move |app| async move { app.state::<AppState>().picky.runtime.activate(&key).await });
        }
    } else if let Some(id) = choice.strip_prefix("cloud:") {
        p.with_db(|c| cloud::model(c, id))?.ok_or("这个云端模型已经被删除了")?;
        p.set_selection(&Selection::Cloud(id.into()))?;
    } else {
        return Err(format!("不认识的模型：{choice}"));
    }
    p.changed();
    Ok(())
}

pub fn stop_model(app: &AppHandle) -> Result<(), String> {
    if app.state::<AppState>().picky.runs.using("local") {
        return Err("本地模型正在回答，等它答完再停止。".into());
    }
    spawn_op(app, |app| async move { app.state::<AppState>().picky.runtime.stop().await });
    Ok(())
}

/// Starts or stops the embedding / rerank service (from settings or the menu bar).
pub fn service(app: &AppHandle, svc: Svc, action: &str) -> Result<(), String> {
    if !["start", "stop"].contains(&action) {
        return Err(format!("不认识的操作：{action}"));
    }
    let action = action.to_string();
    spawn_op(app, move |app| async move { app.state::<AppState>().picky.runtime.service_control(svc, &action).await });
    Ok(())
}

// ---------- commands ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub mode: Mode,
    pub db_path: String,
    /// Attachments and project files (used from phase 5).
    pub files_dir: String,
    pub schema_version: Option<i64>,
    pub error: Option<String>,
}

#[tauri::command]
pub fn picky_info(state: State<AppState>) -> Info {
    let p = &state.picky;
    Info { mode: p.mode, db_path: p.db_path.display().to_string(), files_dir: p.files_dir.display().to_string(), schema_version: p.with_db(|c| crate::db::user_version(c)).ok(), error: p.error.clone() }
}

/// 小拾's own UI state (last open session, list collapsed…): `ui.*` keys in picky.db preferences.
#[tauri::command]
pub fn picky_ui_get(state: State<AppState>, key: String) -> Result<Option<serde_json::Value>, String> {
    ui_key(&key)?;
    state.picky.with_db(|c| store::pref_get(c, &key))
}

#[tauri::command]
pub fn picky_ui_set(state: State<AppState>, key: String, value: serde_json::Value) -> Result<(), String> {
    ui_key(&key)?;
    state.picky.with_db(|c| store::pref_set(c, &key, &value))
}

fn ui_key(key: &str) -> Result<(), String> {
    if key.starts_with("ui.") && key.len() <= 64 {
        Ok(())
    } else {
        Err(format!("不能这样存设置：{key}"))
    }
}

#[tauri::command]
pub fn picky_status(state: State<AppState>) -> Status {
    state.picky.status()
}

#[tauri::command]
pub fn picky_select(app: AppHandle, choice: String) -> Result<(), String> {
    select(&app, &choice)
}

#[tauri::command]
pub fn picky_model_stop(app: AppHandle) -> Result<(), String> {
    stop_model(&app)
}

#[tauri::command]
pub fn picky_refresh(app: AppHandle) {
    spawn_op(&app, |app| async move {
        let st = app.state::<AppState>();
        st.picky.runtime.refresh(true).await;
        for svc in Svc::ALL {
            st.picky.runtime.service_refresh(svc, true).await;
        }
    });
}

#[tauri::command]
pub fn picky_service(app: AppHandle, service: Svc, action: String) -> Result<(), String> {
    self::service(&app, service, &action)
}

/// Chooses the embedding / rerank model (a manifest key). A running service of Pomelo's switches
/// to it; another embedding model makes the search indexer rebuild the vectors.
#[tauri::command]
pub fn picky_service_model(app: AppHandle, service: Svc, key: String) -> Result<(), String> {
    let p = &app.state::<AppState>().picky;
    let s = p.runtime.service_status(service);
    if !s.models.iter().any(|m| m.key == key) {
        return Err("模型清单里没有这个模型".into());
    }
    p.with_db(|c| store::pref_set(c, &service_key(service), &key))?;
    p.runtime.choose(service, Some(key.clone()));
    if s.state != runtime::ServiceState::Stopped && s.ours && s.model.is_some_and(|m| m.key != key) {
        spawn_op(&app, move |app| async move { app.state::<AppState>().picky.runtime.service_control(service, "switch").await });
    }
    p.changed();
    Ok(())
}

#[tauri::command]
pub fn picky_issue_action(app: AppHandle, action: Action) -> Result<(), String> {
    let p = &app.state::<AppState>().picky;
    let open = |path: &Path| -> Result<(), String> {
        if p.mode == Mode::Mock {
            return Err("模拟模式下没有这个文件".into());
        }
        if !path.exists() {
            return Err(format!("找不到这个文件：{}", path.display()));
        }
        app.opener().open_path(path.display().to_string(), None::<&str>).map_err(|e| format!("打不开：{e}"))
    };
    match action {
        Action::RetryStart { key } => select(&app, &format!("local:{key}")),
        Action::RetryStop => stop_model(&app),
        Action::Refresh => {
            picky_refresh(app.clone());
            Ok(())
        }
        Action::OpenServerLog => open(&p.runtime.llm.server_log),
        Action::OpenCatalog => open(&p.runtime.llm.manifest),
        Action::OpenServiceLog { service } => match p.runtime.service_log(service) {
            Some(path) => open(&path),
            None => Err("模型清单里没有这个服务的模型".into()),
        },
    }
}

#[tauri::command]
pub fn picky_issue_dismiss(state: State<AppState>, kind: IssueKind) {
    state.picky.runtime.dismiss(kind);
    state.picky.changed();
}

#[tauri::command]
pub fn picky_cloud_model_save(state: State<AppState>, model: CloudModel) -> Result<CloudModel, String> {
    let saved = state.picky.with_db(|c| cloud::save_model(c, model))?;
    state.picky.changed();
    Ok(saved)
}

#[tauri::command]
pub fn picky_cloud_models_reorder(state: State<AppState>, ids: Vec<String>) -> Result<(), String> {
    state.picky.with_db(|c| cloud::reorder_models(c, &ids))?;
    state.picky.changed();
    Ok(())
}

#[tauri::command]
pub fn picky_cloud_model_delete(state: State<AppState>, id: String) -> Result<(), String> {
    let p = &state.picky;
    if p.runs.using(&format!("cloud:{id}")) {
        return Err("这个模型正在写回复，等它写完再删除。".into());
    }
    p.with_db(|c| cloud::delete_model(c, &id))?;
    if p.selection() == Selection::Cloud(id) {
        p.set_selection(&Selection::Local)?;
    }
    p.changed();
    Ok(())
}

#[tauri::command]
pub fn picky_cloud_key_save(state: State<AppState>, service: Service, key: String) -> Result<(), String> {
    state.picky.keys.save(service.code(), &key, &format!("Pomelo 小拾 · {}", service.name()))
}

#[tauri::command]
pub fn picky_cloud_key_remove(state: State<AppState>, service: Service) -> Result<(), String> {
    state.picky.keys.remove(service.code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_are_always_mock() {
        assert_eq!(mode(false), Mode::Mock);
        assert_eq!(mode(true), Mode::Mock);
    }

    #[test]
    fn selection_codes() {
        assert_eq!(Selection::parse("cloud:deepseek-flash"), Selection::Cloud("deepseek-flash".into()));
        assert_eq!(Selection::parse("local"), Selection::Local);
        assert_eq!(Selection::parse("cloud:"), Selection::Local);
        assert_eq!(Selection::Cloud("x".into()).code(), "cloud:x");
    }

    #[test]
    fn targets_follow_selection_and_readiness() {
        let dir = crate::db::tests::temp_dir("picky-state");
        let p = Picky::init(&DataPaths::new(dir.to_path_buf()), &dir, Mode::Mock);
        assert_eq!(p.target().err().as_deref(), Some("本地模型还没有运行。"));
        tauri::async_runtime::block_on(p.runtime.activate("ministral14b"));
        let r = p.target().unwrap();
        assert!(matches!(r.target, Target::Mock { thinking: false }));
        assert_eq!((r.model.as_str(), r.name.as_str(), r.window), ("local:ministral14b", "Ministral 3 14B", context::MOCK_WINDOW));

        p.set_selection(&Selection::Cloud("deepseek-flash".into())).unwrap();
        assert!(p.target().err().unwrap().contains("尚未保存"));
        assert_eq!(p.measure_route().unwrap().model, "cloud:deepseek-flash"); // measuring never needs the key
        p.keys.save(Service::DeepSeek.code(), "sk-test", "t").unwrap();
        let r = p.target().unwrap();
        assert_eq!((r.model.as_str(), r.name.as_str()), ("cloud:deepseek-flash", "DeepSeek Flash"));

        let s = p.status();
        assert_eq!((s.mode, s.selection.as_str(), s.cloud.len(), s.generating), (Mode::Mock, "cloud:deepseek-flash", 2, false));
        assert!(s.keys.iter().any(|k| k.service == Service::DeepSeek && k.saved));
        // a deleted cloud model falls back to local in the status
        p.with_db(|c| cloud::delete_model(c, "deepseek-flash")).unwrap();
        assert_eq!(p.status().selection, "local");
    }

    #[test]
    fn a_broken_picky_db_does_not_panic() {
        let dir = crate::db::tests::temp_dir("picky-broken");
        std::fs::write(dir.join("picky.db"), b"not a database at all, definitely").unwrap();
        let p = Picky::init(&DataPaths::new(dir.to_path_buf()), &dir, Mode::Mock);
        assert!(!p.is_open());
        assert!(p.with_db(|_| Ok(())).unwrap_err().contains("小拾的数据库没有打开"));
        assert!(p.status().error.is_some());
    }
}
