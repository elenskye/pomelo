//! Pomelo OS backend. The only place that touches SQLite.
//! Tauri commands here are the application-service layer shared by UI (and later AI tools).
//! Commands return `Result<T, String>`; error strings are user-facing Chinese.

mod backup;
mod db;
mod modules;
mod paths;
mod settings;
mod transfer;
mod undo;

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
    /// Undo / redo stacks for UI edits (see undo.rs).
    history: Mutex<undo::History>,
    /// Set when the DB could not be opened/migrated; UI shows it instead of crashing.
    startup_error: Option<String>,
}

impl AppState {
    fn init(paths: DataPaths) -> Self {
        let existed = paths.db.exists();
        let opened = paths.ensure().and_then(|_| {
            let mut conn = db::open(&paths.db)?;
            db::migrate(&mut conn, db::MIGRATIONS, &paths.backups)?;
            if existed {
                // best effort: a failed daily snapshot must not block startup (UI retries on focus)
                if let Err(e) = backup::daily(&conn, &paths.backups) {
                    eprintln!("daily backup failed: {e}");
                }
            }
            Ok(conn)
        });
        match opened {
            Ok(conn) => Self { paths, db: Mutex::new(Some(conn)), history: Mutex::default(), startup_error: None },
            Err(e) => Self { paths, db: Mutex::new(None), history: Mutex::default(), startup_error: Some(e) },
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
            transfer::export_to,
            undo::undo_last,
            undo::redo_last,
            backup::backups_list,
            backup::backup_now,
            backup::backup_daily,
            backup::backup_restore,
            backup::reveal_backups_dir,
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
            modules::drawing::drawing_courses_reorder,
            modules::drawing::drawing_lessons_reorder,
            modules::drawing::drawing_course_create,
            modules::drawing::drawing_course_update,
            modules::drawing::drawing_course_delete,
            modules::drawing::drawing_lessons,
            modules::drawing::drawing_lesson_create,
            modules::drawing::drawing_lesson_update,
            modules::drawing::drawing_lesson_delete,
            modules::packing::packing_templates,
            modules::packing::packing_templates_reorder,
            modules::packing::packing_items_reorder,
            modules::packing::packing_template_create,
            modules::packing::packing_template_update,
            modules::packing::packing_template_delete,
            modules::packing::packing_trips,
            modules::packing::packing_trip_create,
            modules::packing::packing_trip_update,
            modules::packing::packing_trip_delete,
            modules::packing::packing_template_items,
            modules::packing::packing_template_item_create,
            modules::packing::packing_template_item_update,
            modules::packing::packing_template_item_delete,
            modules::packing::packing_trip_items,
            modules::packing::packing_trip_item_create,
            modules::packing::packing_trip_item_update,
            modules::packing::packing_trip_item_delete,
            modules::todo::todo_groups,
            modules::todo::todo_group_create,
            modules::todo::todo_group_rename,
            modules::todo::todo_group_delete,
            modules::todo::todo_goals,
            modules::todo::todo_goal_create,
            modules::todo::todo_goal_update,
            modules::todo::todo_goal_delete,
            modules::todo::todo_tasks,
            modules::todo::todo_task_create,
            modules::todo::todo_task_update,
            modules::todo::todo_task_set_done,
            modules::todo::todo_task_delete,
            modules::todo::todo_agenda,
            modules::todo::todo_done_between,
            modules::todo::todo_task_skip,
            modules::todo::todo_logs,
            modules::todo::todo_log,
            modules::todo::todo_log_delete,
            modules::todo::todo_month,
            modules::todo::todo_groups_reorder,
            modules::todo::todo_goals_reorder,
            modules::todo::todo_tasks_reorder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
