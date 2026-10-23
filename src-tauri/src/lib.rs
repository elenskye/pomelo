//! Pomelo OS backend. The only place that touches SQLite.
//! Tauri commands here are the application-service layer shared by UI (and later AI tools).
//! Commands return `Result<T, String>`; error strings are user-facing Chinese.

mod autostart;
mod backup;
mod db;
mod home;
mod modules;
mod paths;
mod picky;
mod search;
mod secure;
mod settings;
mod site;
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
    /// v0.4: search.db and its indexer (see search/mod.rs).
    pub(crate) search: search::Search,
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
        let search = search::Search::open(&paths);
        let (db, startup_error) = match opened {
            Ok(conn) => (Some(conn), None),
            Err(e) => (None, Some(e)),
        };
        Self { paths, db: Mutex::new(db), history: Mutex::default(), vault: Mutex::default(), startup_error, picky, search }
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
        .plugin(autostart::plugin())
        .setup(|app| {
            let base = app.path().data_dir()?; // ~/Library/Application Support
            let overridden = std::env::var_os(paths::DATA_DIR_ENV);
            let scratch = overridden.is_some();
            let mode = picky::mode(scratch);
            let root = paths::resolve_root(&base, overridden.map(Into::into));
            let home = app.path().home_dir()?;
            app.manage(site::Site::new(site::mode(scratch), &home, &root));
            app.manage(AppState::init(DataPaths::new(root), &home, mode));
            let handle = app.handle().clone();
            app.state::<AppState>().picky.set_app(handle.clone());
            tray::init(&handle)?;
            // the login item opens Pomelo in the menu bar only (hidden before the window is first drawn)
            if autostart::launched_hidden() {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            autostart::init(&handle, scratch);
            picky::spawn_monitor(handle.clone());
            search::indexer::spawn(handle);
            Ok(())
        })
        // 附件 images for the page: pomelo-file://localhost/<id> (stored images only, by id)
        .register_asynchronous_uri_scheme_protocol("pomelo-file", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let state = app.state::<AppState>();
                let dir = modules::attachments::folder_of(&state.paths.root);
                let found = request.uri().path().trim_start_matches('/').parse::<i64>().map_err(|e| e.to_string()).and_then(|id| state.with_db(|c| modules::attachments::image_bytes(c, &dir, id)));
                let resp = match found {
                    Ok((bytes, mime)) => tauri::http::Response::builder().header("Content-Type", mime).header("Cache-Control", "max-age=31536000, immutable").body(bytes),
                    Err(_) => tauri::http::Response::builder().status(404).body(Vec::new()),
                };
                responder.respond(resp.unwrap_or_else(|_| tauri::http::Response::new(Vec::new())));
            });
        })
        // 呦呦漫游's images for the page: pomelo-site://localhost/<url on the site> (images under the site's public/)
        .register_asynchronous_uri_scheme_protocol("pomelo-site", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let url = modules::attachments::percent_decode(request.uri().path());
                let resp = match site::public_image(&app, &url) {
                    Some((bytes, mime)) => tauri::http::Response::builder().header("Content-Type", mime).header("Cache-Control", "no-cache").body(bytes),
                    None => tauri::http::Response::builder().status(404).body(Vec::new()),
                };
                responder.respond(resp.unwrap_or_else(|_| tauri::http::Response::new(Vec::new())));
            });
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
            picky::picky_service,
            picky::picky_service_model,
            picky::picky_issue_action,
            picky::picky_issue_dismiss,
            picky::picky_cloud_model_save,
            picky::picky_cloud_model_delete,
            picky::picky_cloud_models_reorder,
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
            picky::knowledge::picky_kb_overview,
            picky::knowledge::picky_kb_library_create,
            picky::knowledge::picky_kb_library_rename,
            picky::knowledge::picky_kb_library_delete,
            picky::knowledge::picky_kb_libraries_reorder,
            picky::knowledge::picky_kb_folder_add,
            picky::knowledge::picky_kb_web_add,
            picky::knowledge::picky_kb_source_remove,
            picky::knowledge::picky_kb_source_cloud,
            picky::knowledge::picky_kb_source_rebuild,
            picky::knowledge::picky_kb_scan,
            picky::knowledge::picky_kb_open,
            picky::knowledge::picky_conversation_knowledge,
            picky::knowledge::picky_conversation_libraries,
            picky::work::picky_file_create_confirm,
            picky::work::picky_file_edit_confirm,
            picky::work::picky_file_edit_cancel,
            picky::work::picky_word_defaults,
            picky::work::picky_save_word,
            picky::assistants::translate::picky_translate,
            picky::assistants::seeker::picky_seeker,
            picky::assistants::seeker::picky_seeker_folder,
            picky::assistants::seeker::picky_seeker_new,
            picky::assistants::seeker::picky_seeker_save,
            picky::assistants::seeker::picky_seeker_delete_blocks,
            picky::assistants::seeker::catalog::picky_seeker_record_save,
            picky::assistants::seeker::catalog::picky_seeker_record_delete,
            picky::assistants::seeker::catalog::picky_seeker_settings,
            picky::assistants::seeker::import::picky_seeker_import_preview,
            picky::assistants::seeker::import::picky_seeker_import_confirm,
            picky::assistants::seeker::workflow::picky_seeker_assemble,
            picky::assistants::seeker::workflow::picky_seeker_sources,
            picky::assistants::seeker::workflow::picky_seeker_source_apply,
            picky::assistants::seeker::workflow::picky_seeker_confirm,
            picky::assistants::seeker::workflow::picky_seeker_restore,
            picky::assistants::seeker::workflow::picky_seeker_reorder,
            picky::assistants::seeker::workflow::picky_seeker_apply,
            picky::assistants::seeker::model::picky_seeker_suggest,
            picky::assistants::seeker::output::picky_seeker_template_read,
            picky::assistants::seeker::output::picky_seeker_template,
            picky::assistants::seeker::output::picky_seeker_output_preview,
            picky::assistants::seeker::output::picky_seeker_output_save,
            picky::assistants::seeker::output::picky_seeker_output_discard,
            picky::assistants::seeker::output::picky_seeker_job_link,
            picky::assistants::seeker::output::picky_seeker_export_restore,
            picky::assistants::translate::picky_translate_settings,
            picky::assistants::translate::picky_translate_input,
            picky::assistants::translate::picky_translate_preview,
            picky::assistants::translate::picky_translate_run,
            picky::assistants::translate::picky_translate_stop,
            picky::assistants::pub2md::picky_pub2md,
            picky::assistants::pub2md::picky_pub2md_settings,
            picky::assistants::pub2md::picky_pub2md_add,
            picky::assistants::pub2md::picky_pub2md_resume,
            picky::assistants::pub2md::picky_pub2md_unqueue,
            picky::assistants::pub2md::picky_pub2md_stop,
            picky::assistants::pub2md::picky_pub2md_open,
            picky::assistants::pub2md::picky_pub2md_keys_save,
            picky::assistants::pub2md::picky_pub2md_keys_remove,
            picky::assistants::pub2md::picky_pub2md_preview,
            picky::assistants::pub2md::picky_pub2md_prompts,
            picky::assistants::pub2md::picky_pub2md_group,
            picky::assistants::pub2md::picky_pub2md_delete,
            picky::assistants::site::picky_site_draft,
            picky::assistants::site::picky_site_draft_run,
            picky::assistants::site::picky_site_draft_stop,
            picky::assistants::site::picky_site_draft_clear,
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
            picky::assist::picky_site_english,
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
            picky::library::picky_private,
            picky::library::picky_conversation_thinking,
            picky::yearly::picky_year_card,
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
            modules::attachments::attachments_add_paths,
            modules::attachments::attachments_add_bytes,
            modules::attachments::attachments_of,
            modules::attachments::attachment_open,
            modules::attachments::attachments_usage,
            modules::attachments::attachments_clean,
            modules::days::days_list,
            modules::days::day_create,
            modules::days::day_update,
            modules::days::day_delete,
            modules::todo::todo_tasks_from_note,
            picky::assist::picky_split_note,
            picky::files::picky_import_image,
            picky::files::picky_import_web,
            modules::custom::custom_list,
            modules::custom::custom_create,
            modules::custom::custom_update,
            modules::custom::custom_delete,
            modules::lists::list_fields,
            modules::lists::list_field_create,
            modules::lists::list_field_update,
            modules::lists::list_field_delete,
            modules::lists::list_fields_reorder,
            modules::lists::list_items,
            modules::lists::list_item_create,
            modules::lists::list_item_update,
            modules::lists::list_item_delete,
            modules::lists::list_items_reorder,
            modules::progress::progress_courses,
            modules::progress::progress_course_create,
            modules::progress::progress_course_update,
            modules::progress::progress_course_delete,
            modules::progress::progress_courses_reorder,
            modules::progress::progress_lessons,
            modules::progress::progress_lesson_create,
            modules::progress::progress_lesson_update,
            modules::progress::progress_lesson_delete,
            modules::progress::progress_lessons_reorder,
            modules::notes::note_create,
            modules::notes::note_update,
            modules::notes::note_delete,
            modules::notes::notes_tag_rename,
            modules::notes::notes_tag_set,
            search::search_all,
            search::query::search_query,
            search::query::search_similar,
            search::query::search_notes_similar,
            picky::find::picky_search_similar,
            picky::skills::picky_skills,
            picky::skills::picky_skill_set,
            picky::skills::picky_skill_key_save,
            picky::skills::picky_skill_key_remove,
            picky::skills::picky_skill_contact_set,
            picky::schedule::picky_schedules,
            picky::schedule::picky_schedule_save,
            picky::schedule::picky_schedule_enable,
            picky::schedule::picky_schedule_delete,
            picky::schedule::picky_schedule_run,
            picky::schedule::picky_seen,
            picky::brief::picky_brief,
            picky::brief::picky_brief_write,
            picky::brief::picky_brief_set,
            autostart::app_autostart,
            autostart::app_autostart_set,
            picky::act::card::picky_card_confirm,
            picky::act::card::picky_card_cancel,
            search::query::search_index_status,
            home::home_overview,
            site::site_overview,
            site::site_changes,
            site::site_diff,
            site::site_prepare,
            site::site_publish,
            site::site_dismiss,
            site::site_history,
            site::site_server_start,
            site::site_server_stop,
            site::site_sandbox,
            site::site_set_folder,
            site::site_works,
            site::site_work,
            site::site_work_save,
            site::site_work_create,
            site::site_work_add_version,
            site::site_work_rename,
            site::site_work_delete,
            site::site_image_bytes,
            site::site_files_add,
            site::site_now_list,
            site::site_now,
            site::site_now_save,
            site::site_now_create,
            site::site_now_delete,
            site::site_status_save,
            site::site_about,
            site::site_about_save,
            site::site_career,
            site::site_career_save,
            site::site_registry,
            site::site_registry_save,
            site::site_registry_citations,
            site::site_registry_delete,
            site::site_registry_unregistered,
            site::site_registry_adopt,
            site::site_terms,
            site::site_terms_save,
            site::site_links_save,
            site::site_assets,
            site::site_preview,
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
            modules::vault::vault_reorder,
            modules::vault::vault_copy,
            modules::vault::vault_generate,
            modules::vault::vault_archive,
            modules::vault::vault_categories,
            modules::vault::vault_category_create,
            modules::vault::vault_category_rename,
            modules::vault::vault_category_delete,
            modules::vault::vault_category_reorder,
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
            RunEvent::Exit => {
                picky::chat::shutdown(app);
                app.state::<AppState>().picky.runtime.stop_ours_on_exit();
                app.state::<site::Site>().on_exit();
            }
            _ => {}
        });
}
