//! 小拾 · Picky (v0.3): the old Picky app rewritten into Pomelo — Pomelo's LLM service.
//! Rules (requirements.md「小拾」):
//! - Data: picky.db (store.rs), never pomelo.db. Only Rust reads/writes it.
//! - Models: local chat models via ~/Documents/LLM scripts (runtime.rs, one at a time on 8080),
//!   the embedding service on 8081, and three cloud services (cloud.rs, provider.rs). The model
//!   files, manifest and scripts are never modified.
//! - Mode: `Real` only for the installed app on the real data dir (or POMELO_REAL_MODELS=1);
//!   dev builds, tests and scratch data dirs are `Mock`: built-in catalog, simulated operations,
//!   canned replies, keys in memory — no script, no network, no keychain.
//! - Model control, cloud keys and 芝麻开门 are never AI tools.
//! - Selection ("local" | "cloud:<id>", preference `model.selection`) is shared by the page and the
//!   menu bar; it can't change while a reply is being written.

pub mod assist;
pub mod catalog;
pub mod chat;
pub mod cloud;
pub mod data;
pub mod context;
pub mod embed;
pub mod export;
pub mod files;
pub mod find;
pub mod index;
pub mod instructions;
pub mod library;
pub mod memory;
pub mod pdf;
pub mod provider;
pub mod reading;
pub mod runtime;
pub mod storage;
pub mod store;
pub mod templates;
pub mod transfer;

use crate::backup;
use crate::paths::DataPaths;
use crate::AppState;
use cloud::{CloudModel, KeyStore, Service};
use provider::Target;
use rusqlite::Connection;
use runtime::{Action, EmbStatus, IssueKind, LocalStatus, Runtime};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

pub const REAL_ENV: &str = "POMELO_REAL_MODELS";
const SELECTION_KEY: &str = "model.selection";

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
        Picky {
            mode,
            conn: Mutex::new(conn),
            error,
            db_path: paths.picky_db.clone(),
            files_dir: paths.picky_files.clone(),
            runtime: Runtime::new(mode, runtime::LlmPaths::new(home)),
            keys: if mode == Mode::Real { KeyStore::keychain() } else { KeyStore::memory() },
            runs: chat::Runs::default(),
            index_jobs: index::Jobs::default(),
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
            embedding: self.runtime.emb_status(),
            selection: selection.code(),
            keys: Service::ALL.iter().map(|s| KeyInfo { service: *s, name: s.name().into(), saved: self.keys.has(*s) }).collect(),
            cloud,
            generating: self.runs.busy(),
            active_run: self.runs.active_run(),
            active_session: self.runs.active_session(),
            active_kind: self.runs.active_kind(),
            active_reading: self.runs.active_reading(),
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
        match self.selection() {
            Selection::Local => {
                let Some((m, served)) = self.runtime.ready_model() else {
                    let s = self.runtime.local_status();
                    return Err(if s.working || s.readiness == runtime::Readiness::Loading { "模型还在加载，等它就绪再发送。".into() } else { "本地模型还没有运行。".into() });
                };
                let (target, window) = if self.mode == Mode::Mock { (Target::Mock { thinking: m.thinking }, context::MOCK_WINDOW) } else { (Target::Local { model: served }, m.context) };
                Ok(Route { target, model: format!("local:{}", m.key), name: m.name, window })
            }
            Selection::Cloud(id) => {
                let m = self.with_db(|c| cloud::model(c, &id))?.ok_or("这个云端模型已经被删除了，请重新选择。")?;
                let key = if with_key { self.keys.read(m.service)? } else { String::new() };
                let (target, window) = if self.mode == Mode::Mock { (Target::Mock { thinking: false }, context::MOCK_WINDOW) } else { (Target::Cloud { model: m.clone(), key }, m.context_window) };
                Ok(Route { target, model: format!("cloud:{}", m.id), name: m.name, window })
            }
        }
    }
}

/// Where model work goes: the backend, the run's model id, the name shown in history, and the
/// window from the catalog / cloud settings (a local server's own value wins, context.rs).
pub struct Route {
    pub target: Target,
    pub model: String,
    pub name: String,
    pub window: i64,
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
    pub embedding: EmbStatus,
    /// "local" | "cloud:<id>"
    pub selection: String,
    pub cloud: Vec<CloudModel>,
    pub keys: Vec<KeyInfo>,
    pub generating: bool,
    /// The reply being written (to stop it after the page was reloaded), and its conversation.
    pub active_run: Option<i64>,
    pub active_session: Option<i64>,
    /// reply | summary
    pub active_kind: Option<&'static str>,
    /// The reply reading material in segments: where it is.
    pub active_reading: Option<chat::Reading>,
}

/// Polls model status (local API every 3 s, every second during an operation; embedding every
/// 5 s) and keeps the menu bar current. Mock mode only refreshes the menu bar.
/// Every 5 minutes it also takes the day's backups if missing: Pomelo may stay hidden in the menu
/// bar for days, so launch / window focus alone can no longer be relied on.
pub fn spawn_monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick: u64 = 0;
        loop {
            let st = app.state::<AppState>();
            if tick % 3 == 0 || st.picky.runtime.local_status().working {
                st.picky.runtime.refresh(false).await;
            }
            if tick % 5 == 0 {
                st.picky.runtime.emb_refresh(false).await;
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

const BUSY: &str = "正在生成回复，等它写完再切换。";

/// "local:<key>" switches to (starting if needed) that local model; "cloud:<id>" picks a cloud model.
pub fn select(app: &AppHandle, choice: &str) -> Result<(), String> {
    let st = app.state::<AppState>();
    let p = &st.picky;
    if p.runs.busy() {
        return Err(BUSY.into());
    }
    if let Some(key) = choice.strip_prefix("local:") {
        p.set_selection(&Selection::Local)?;
        let _ = p.with_db(|c| Ok(assist::remember_local(c, key)));
        let running = p.runtime.local_status().active.is_some_and(|m| m.key == key);
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
    if app.state::<AppState>().picky.runs.busy() {
        return Err("正在生成回复，等它写完再停止模型。".into());
    }
    spawn_op(app, |app| async move { app.state::<AppState>().picky.runtime.stop().await });
    Ok(())
}

pub fn embedding(app: &AppHandle, action: &str) -> Result<(), String> {
    if !["start", "stop"].contains(&action) {
        return Err(format!("不认识的操作：{action}"));
    }
    let action = action.to_string();
    spawn_op(app, move |app| async move { app.state::<AppState>().picky.runtime.emb_control(&action).await });
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
        st.picky.runtime.emb_refresh(true).await;
    });
}

#[tauri::command]
pub fn picky_embedding(app: AppHandle, action: String) -> Result<(), String> {
    embedding(&app, &action)
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
        Action::OpenEmbeddingLog => open(&p.runtime.llm.embedding_log),
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
pub fn picky_cloud_model_delete(state: State<AppState>, id: String) -> Result<(), String> {
    let p = &state.picky;
    if p.runs.busy() && p.selection() == Selection::Cloud(id.clone()) {
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
    state.picky.keys.save(service, &key)
}

#[tauri::command]
pub fn picky_cloud_key_remove(state: State<AppState>, service: Service) -> Result<(), String> {
    state.picky.keys.remove(service)
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
        p.keys.save(Service::DeepSeek, "sk-test").unwrap();
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
