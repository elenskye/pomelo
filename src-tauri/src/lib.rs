//! Pomelo OS backend. The only place that touches SQLite.
//! Tauri commands here are the application-service layer shared by UI (and later AI tools).
//! Commands return `Result<T, String>`; error strings are user-facing Chinese.

mod backup;
mod db;
mod home;
mod modules;
mod paths;
mod picky;
mod search;
mod secure;
mod settings;
mod transfer;
mod tray;
mod undo;

use paths::DataPaths;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;
use tauri::{Manager, RunEvent, State, WindowEvent};
use tauri_plugin_opener::OpenerExt;

pub struct AppState {
    pub(crate) paths: DataPaths,
    db: Mutex<Option<Connection>>,
    /// Undo / redo stacks for UI edits (see undo.rs).
    history: Mutex<undo::History>,
    /// 芝麻开门: the open vault key, in memory only (see modules/vault.rs).
    vault: Mutex<modules::vault::Session>,
    /// Set when the DB could not be opened/migrated; UI shows it instead of crashing.
    startup_error: Option<String>,
    /// 小拾 (AI): its own picky.db, model services, cloud keys (see picky/mod.rs).
    pub(crate) picky: picky::Picky,
}

impl AppState {
    fn init(paths: DataPaths, home: &Path, mode: picky::Mode) -> Self {
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
        // after pomelo.db: ensure() made the folders; a 小拾 failure never blocks the rest
        let picky = picky::Picky::init(&paths, home, mode);
        let (db, startup_error) = match opened {
            Ok(conn) => (Some(conn), None),
            Err(e) => (None, Some(e)),
        };
        Self { paths, db: Mutex::new(db), history: Mutex::default(), vault: Mutex::default(), startup_error, picky }
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

/// Opens a task's link in its default app (see `common::link_target` for what counts as a link).
#[tauri::command]
fn open_link(app: tauri::AppHandle, link: String) -> Result<(), String> {
    use modules::common::{link_target, LinkTarget};
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    match link_target(&link, &home)? {
        LinkTarget::Url(url) => app.opener().open_url(url, None::<&str>),
        LinkTarget::Path(p) if !p.exists() => return Err(format!("找不到这个文件：{}", p.display())),
        LinkTarget::Path(p) => app.opener().open_path(p.display().to_string(), None::<&str>),
    }
    .map_err(|e| format!("打不开这个链接：{e}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Sweep {
    closed: usize,
    posted: usize,
}

/// Day-change chores (launch + when the date moves on): check-ins whose 时间段 ended are completed,
/// 固定支出 due by today are posted. Not undoable (it would just happen again).
#[tauri::command]
fn daily_sweep(state: State<AppState>) -> Result<Sweep, String> {
    state.with_db(|c| {
        let today = modules::todo::today(c)?;
        Ok(Sweep { closed: modules::todo::close_ended(c, &today)?, posted: modules::finance::post_due(c, &today)? })
    })
}

/// Plain copy to the pasteboard (the webview's own clipboard API is not reliable here).
#[tauri::command]
fn copy_text(text: String) {
    secure::copy_plain(&text);
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
            let overridden = std::env::var_os(paths::DATA_DIR_ENV);
            let mode = picky::mode(overridden.is_some());
            let root = paths::resolve_root(&base, overridden.map(Into::into));
            let home = app.path().home_dir()?;
            app.manage(AppState::init(DataPaths::new(root), &home, mode));
            let handle = app.handle().clone();
            app.state::<AppState>().picky.set_app(handle.clone());
            tray::init(&handle)?;
            picky::spawn_monitor(handle);
            Ok(())
        })
        // closing the window keeps Pomelo running in the menu bar; ⌘Q / the menu bar quits
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            reveal_data_dir,
            copy_text,
            picky::picky_info,
            picky::picky_status,
            picky::picky_ui_get,
            picky::picky_ui_set,
            picky::picky_select,
            picky::picky_model_stop,
            picky::picky_refresh,
            picky::picky_embedding,
            picky::picky_issue_action,
            picky::picky_issue_dismiss,
            picky::picky_cloud_model_save,
            picky::picky_cloud_model_delete,
            picky::picky_cloud_key_save,
            picky::picky_cloud_key_remove,
            picky::chat::picky_sessions,
            picky::chat::picky_messages,
            picky::chat::picky_send,
            picky::chat::picky_edit,
            picky::chat::picky_regenerate,
            picky::chat::picky_switch_version,
            picky::chat::picky_stop,
            picky::context::picky_context,
            picky::context::picky_summarize,
            picky::files::picky_import_file,
            picky::files::picky_attachment_library,
            picky::files::picky_open_file,
            picky::index::picky_project_files,
            picky::index::picky_project_file_add,
            picky::index::picky_project_file_replace,
            picky::index::picky_project_file_remove,
            picky::index::picky_project_file_index,
            picky::index::picky_index_rebuild,
            picky::index::picky_index_stop,
            picky::index::picky_retrieval_plan,
            picky::index::picky_retrieve,
            picky::storage::picky_storage,
            picky::storage::picky_storage_delete_file,
            picky::storage::picky_storage_delete_stray,
            picky::storage::picky_storage_clean_empty,
            picky::memory::picky_memories,
            picky::memory::picky_memory_save,
            picky::memory::picky_remember,
            picky::memory::picky_memory_flag,
            picky::memory::picky_memory_delete,
            picky::memory::picky_memory_enabled,
            picky::memory::picky_memory_enabled_set,
            picky::memory::picky_conversation_memory,
            picky::memory::picky_memory_plan,
            picky::memory::picky_choose_memories,
            picky::templates::picky_templates,
            picky::templates::picky_template_save,
            picky::templates::picky_template_pin,
            picky::templates::picky_template_delete,
            picky::data::picky_data_cloud,
            picky::assist::picky_polish,
            picky::assist::picky_fill_work,
            picky::assist::picky_fill_job,
            picky::assist::picky_split_task,
            picky::assist::picky_trip_missing,
            picky::assist::picky_quick_fill,
            picky::data::picky_data_cloud_set,
            picky::templates::picky_template_used,
            picky::instructions::picky_instructions,
            picky::instructions::picky_session_instructions_save,
            picky::instructions::picky_global_instructions,
            picky::instructions::picky_global_instructions_save,
            picky::instructions::picky_presets,
            picky::instructions::picky_preset_save,
            picky::instructions::picky_preset_delete,
            picky::library::picky_library,
            picky::library::picky_rename,
            picky::library::picky_pin,
            picky::library::picky_archive,
            picky::library::picky_move,
            picky::library::picky_delete,
            picky::library::picky_project_create,
            picky::library::picky_project_update,
            picky::library::picky_project_delete,
            picky::library::picky_projects_reorder,
            picky::library::picky_drop,
            picky::find::picky_search,
            picky::find::picky_search_models,
            picky::find::picky_reveal,
            picky::export::picky_copy_conversation,
            picky::export::picky_export_markdown,
            picky::export::picky_export_package,
            open_link,
            daily_sweep,
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
            modules::finance::fin_accounts,
            modules::finance::fin_account_create,
            modules::finance::fin_account_update,
            modules::finance::fin_account_delete,
            modules::finance::fin_accounts_reorder,
            modules::finance::fin_assets,
            modules::finance::fin_balances_save,
            modules::finance::fin_holdings,
            modules::finance::fin_holdings_save,
            modules::finance::fin_entries,
            modules::finance::fin_entry_create,
            modules::finance::fin_entries_create,
            modules::finance::fin_entry_update,
            modules::finance::fin_entry_delete,
            modules::finance::fin_entries_update,
            modules::finance::fin_entries_delete,
            modules::finance::fin_period_last,
            modules::finance::fin_fixed,
            modules::finance::fin_fixed_create,
            modules::finance::fin_fixed_update,
            modules::finance::fin_fixed_delete,
            modules::finance::fin_fixed_reorder,
            modules::finance::fin_budgets,
            modules::finance::fin_budgets_save,
            modules::finance::fin_totals,
            modules::jobs::job_statuses,
            modules::jobs::job_status_create,
            modules::jobs::job_status_update,
            modules::jobs::job_status_delete,
            modules::jobs::job_statuses_reorder,
            modules::jobs::jobs_list,
            modules::jobs::job_stats,
            modules::todo::todo_subtasks_create,
            modules::packing::packing_trip_items_create,
            modules::jobs::job_create,
            modules::jobs::job_update,
            modules::jobs::job_follow_up,
            modules::jobs::job_delete,
            modules::notes::notes_list,
            modules::notes::notes_overview,
            modules::notes::note_create,
            modules::notes::note_update,
            modules::notes::note_delete,
            modules::notes::notes_tag_rename,
            modules::notes::notes_tag_set,
            search::search_all,
            home::home_overview,
            modules::vault::vault_status,
            modules::vault::vault_setup,
            modules::vault::vault_unlock,
            modules::vault::vault_unlock_touch,
            modules::vault::vault_lock,
            modules::vault::vault_touch,
            modules::vault::vault_change_password,
            modules::vault::vault_list,
            modules::vault::vault_get,
            modules::vault::vault_create,
            modules::vault::vault_update,
            modules::vault::vault_delete,
            modules::vault::vault_copy,
            modules::vault::vault_generate,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // Dock icon clicked while the window is hidden
            RunEvent::Reopen { .. } => tray::show_main(app),
            RunEvent::Exit => picky::chat::shutdown(app),
            _ => {}
        });
}
