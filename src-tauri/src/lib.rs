//! Pomelo OS backend. The only place that touches SQLite.
//! Tauri commands here are the application-service layer shared by UI (and later AI tools).
//! Commands return `Result<T, String>`; error strings are user-facing Chinese.

mod backup;
mod db;
mod modules;
mod paths;
mod settings;
mod transfer;

use paths::DataPaths;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;
use tauri::{Manager, State};
use tauri_plugin_opener::OpenerExt;

pub struct AppState {
    pub(crate) paths: DataPaths,
    db: Mutex<Option<Connection>>,
    /// Set when the DB could not be opened/migrated; UI shows it instead of crashing.
    startup_error: Option<String>,
}

impl AppState {
    fn init(paths: DataPaths) -> Self {
        let opened = paths.ensure().and_then(|_| {
            let mut conn = db::open(&paths.db)?;
            db::migrate(&mut conn, db::MIGRATIONS, &paths.backups)?;
            Ok(conn)
        });
        match opened {
            Ok(conn) => Self { paths, db: Mutex::new(Some(conn)), startup_error: None },
            Err(e) => Self { paths, db: Mutex::new(None), startup_error: Some(e) },
        }
    }

    pub(crate) fn with_db<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T, String>) -> Result<T, String> {
        let mut guard = self.db.lock().map_err(|_| "数据库连接异常，请重启 App".to_string())?;
        match guard.as_mut() {
            Some(conn) => f(conn),
            None => Err(format!("数据库没有打开：{}", self.startup_error.as_deref().unwrap_or("未知原因"))),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    app_version: String,
    is_dev: bool,
    data_dir: String,
    db_path: String,
    schema_version: Option<i64>,
    startup_error: Option<String>,
}

#[tauri::command]
fn app_info(app: tauri::AppHandle, state: State<AppState>) -> AppInfo {
    AppInfo {
        app_version: app.package_info().version.to_string(),
        is_dev: paths::is_dev(),
        data_dir: state.paths.root.display().to_string(),
        db_path: state.paths.db.display().to_string(),
        schema_version: state.with_db(|c| db::user_version(c)).ok(),
        startup_error: state.startup_error.clone(),
    }
}

#[tauri::command]
fn reveal_data_dir(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    app.opener()
        .open_path(state.paths.root.display().to_string(), None::<&str>)
        .map_err(|e| format!("无法打开数据目录：{e}"))
}

#[tauri::command]
fn settings_get(state: State<AppState>, key: String) -> Result<Option<Value>, String> {
    state.with_db(|c| settings::get(c, &key))
}

#[tauri::command]
fn settings_set(state: State<AppState>, key: String, value: Value) -> Result<(), String> {
    state.with_db(|c| settings::set(c, &key, &value))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let base = app.path().data_dir()?; // ~/Library/Application Support
            let root = paths::resolve_root(&base, std::env::var_os(paths::DATA_DIR_ENV).map(Into::into));
            app.manage(AppState::init(DataPaths::new(root)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            reveal_data_dir,
            settings_get,
            settings_set,
            transfer::import_preview,
            transfer::import_apply,
            modules::books::books_list,
            modules::books::books_create,
            modules::books::books_update,
            modules::books::books_delete,
            modules::films::films_list,
            modules::films::films_create,
            modules::films::films_update,
            modules::films::films_delete,
            modules::kinds::kinds_list,
            modules::kinds::kinds_create,
            modules::kinds::kinds_rename,
            modules::kinds::kinds_delete,
            modules::kinds::kinds_reorder,
            modules::drawing::drawing_courses,
            modules::drawing::drawing_course_create,
            modules::drawing::drawing_course_update,
            modules::drawing::drawing_course_delete,
            modules::drawing::drawing_lessons,
            modules::drawing::drawing_lesson_create,
            modules::drawing::drawing_lesson_update,
            modules::drawing::drawing_lesson_delete,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
