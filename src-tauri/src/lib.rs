//! Pomelo OS backend. The only place that touches SQLite.
//! Tauri commands here are the application-service layer shared by UI (and later AI tools).
//! Commands return `Result<T, String>`; error strings are user-facing Chinese.

mod autostart;
mod backup;
mod capture;
mod db;
mod dock;
mod health;
mod home;
mod menu;
mod modules;
mod paths;
mod youzi;
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
use tauri::{Emitter, Manager, RunEvent, State, WindowEvent};
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
    /// 柚子 (AI): its own youzi.db, model services, cloud keys (see youzi/mod.rs).
    pub(crate) youzi: youzi::Youzi,
    /// v0.4: search.db and its indexer (see search/mod.rs).
    pub(crate) search: search::Search,
}

impl AppState {
    fn init(paths: DataPaths, home: &Path, mode: youzi::Mode) -> Self {
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
        // after pomelo.db: ensure() made the folders; a 柚子 failure never blocks the rest
        let youzi = youzi::Youzi::init(&paths, home, mode);
        let search = search::Search::open(&paths);
        let (db, startup_error) = match opened {
            Ok(conn) => (Some(conn), None),
            Err(e) => (None, Some(e)),
        };
        Self { paths, db: Mutex::new(db), history: Mutex::default(), vault: Mutex::default(), startup_error, youzi, search }
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
        .plugin(capture::plugin())
        .menu(|app| menu::build(app, &menu::first_index()))
        .on_menu_event(|app, e| menu::on_event(app, &e))
        .setup(|app| {
            let base = app.path().data_dir()?; // ~/Library/Application Support
            let overridden = std::env::var_os(paths::DATA_DIR_ENV);
            let scratch = overridden.is_some();
            let mode = youzi::mode(scratch);
            let root = paths::resolve_root(&base, overridden.map(Into::into));
            let home = app.path().home_dir()?;
            app.manage(site::Site::new(site::mode(scratch), &home, &root));
            app.manage(AppState::init(DataPaths::new(root), &home, mode));
            let handle = app.handle().clone();
            app.state::<AppState>().youzi.set_app(handle.clone());
            // the API keys' keychain item: read once now (a new build's one prompt), not mid-reply
            let keys = handle.clone();
            std::thread::spawn(move || keys.state::<AppState>().youzi.keys.load());
            // once: 投石问路's old 简历 field becomes the job's résumé in Seeker
            if let Err(e) = youzi::assistants::seeker::output::adopt_job_resumes(&app.state::<AppState>()) {
                eprintln!("[seeker] 简历一栏没搬过去：{e}");
            }
            capture::register(&handle);
            tray::init(&handle)?;
            // the login item opens Pomelo in the menu bar only (hidden before the window is first drawn)
            if autostart::launched_hidden() {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            autostart::init(&handle, scratch);
            youzi::spawn_monitor(handle.clone());
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
        // closing the window keeps Pomelo running in the menu bar; ⌘Q / the menu bar quits.
        // 速记小窗: closing or losing the focus goes through its page (it writes the draft first)
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::CloseRequested { api, .. } if window.label() == capture::LABEL => {
                api.prevent_close();
                capture::request_close(window.app_handle());
            }
            WindowEvent::Focused(false) if window.label() == capture::LABEL => {
                let _ = window.emit_to(capture::LABEL, "capture-blur", ());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            reveal_data_dir,
            menu::app_menu_index,
            capture::capture_load,
            capture::capture_draft,
            capture::capture_files,
            capture::capture_pin,
            capture::capture_note,
            capture::capture_todo,
            capture::capture_undo,
            capture::capture_chat,
            capture::capture_chat_thinking,
            capture::capture_chat_effort,
            capture::capture_chat_clear,
            capture::capture_chat_keep,
            capture::capture_fit,
            capture::capture_open,
            capture::capture_hide,
            copy_text,
            youzi::youzi_info,
            youzi::youzi_status,
            youzi::youzi_ui_get,
            youzi::youzi_ui_set,
            youzi::youzi_select,
            youzi::youzi_model_stop,
            youzi::youzi_refresh,
            youzi::youzi_service,
            youzi::youzi_service_model,
            youzi::youzi_issue_action,
            youzi::youzi_issue_dismiss,
            youzi::youzi_cloud_model_save,
            youzi::youzi_cloud_model_delete,
            youzi::youzi_cloud_models_reorder,
            youzi::youzi_cloud_key_save,
            youzi::youzi_cloud_key_remove,
            youzi::chat::youzi_sessions,
            youzi::chat::youzi_messages,
            youzi::chat::youzi_send,
            youzi::chat::youzi_edit,
            youzi::chat::youzi_regenerate,
            youzi::chat::youzi_switch_version,
            youzi::chat::youzi_stop,
            youzi::context::youzi_context,
            youzi::context::youzi_summarize,
            youzi::files::youzi_import_file,
            youzi::files::youzi_attachment_library,
            youzi::files::youzi_open_file,
            youzi::index::youzi_project_files,
            youzi::index::youzi_project_file_add,
            youzi::index::youzi_project_file_replace,
            youzi::index::youzi_project_file_remove,
            youzi::index::youzi_project_file_index,
            youzi::index::youzi_index_rebuild,
            youzi::index::youzi_index_stop,
            youzi::index::youzi_retrieval_plan,
            youzi::index::youzi_retrieve,
            youzi::knowledge::youzi_kb_overview,
            youzi::knowledge::youzi_kb_library_create,
            youzi::knowledge::youzi_kb_library_rename,
            youzi::knowledge::youzi_kb_library_delete,
            youzi::knowledge::youzi_kb_libraries_reorder,
            youzi::knowledge::youzi_kb_folder_add,
            youzi::knowledge::youzi_kb_web_add,
            youzi::knowledge::youzi_kb_source_remove,
            youzi::knowledge::youzi_kb_source_cloud,
            youzi::knowledge::youzi_kb_source_rebuild,
            youzi::knowledge::youzi_kb_scan,
            youzi::knowledge::youzi_kb_open,
            youzi::knowledge::youzi_conversation_knowledge,
            youzi::knowledge::youzi_conversation_libraries,
            youzi::work::youzi_file_create_confirm,
            youzi::work::youzi_file_edit_confirm,
            youzi::work::youzi_file_edit_cancel,
            youzi::work::youzi_word_defaults,
            youzi::work::youzi_save_word,
            youzi::assistants::translate::youzi_translate,
            youzi::assistants::seeker::youzi_seeker,
            youzi::assistants::rater::youzi_rater,
            youzi::assistants::rater::youzi_rater_save,
            youzi::assistants::rater::youzi_rater_delete,
            youzi::assistants::rater::youzi_rater_seconds,
            youzi::assistants::rater::qbank::youzi_rater_suggest,
            youzi::assistants::rater::qbank::youzi_rater_add,
            youzi::assistants::rater::qbank::youzi_rater_defaults,
            youzi::assistants::rater::qbank::youzi_rater_options,
            youzi::assistants::rater::qbank::youzi_rater_options_set,
            youzi::assistants::rater::practice::youzi_rater_practice,
            youzi::assistants::rater::practice::youzi_rater_round,
            youzi::assistants::rater::practice::youzi_rater_round_start,
            youzi::assistants::rater::practice::youzi_rater_mark,
            youzi::assistants::rater::practice::youzi_rater_stop,
            youzi::assistants::rater::practice::youzi_rater_round_delete,
            youzi::assistants::rater::practice::youzi_rater_pick,
            youzi::assistants::rater::practice::youzi_rater_follow,
            youzi::assistants::rater::practice::youzi_rater_keep,
            youzi::assistants::rater::practice::youzi_rater_comment,
            youzi::assistants::rater::practice::youzi_rater_rounds_delete,
            youzi::assistants::rater::youzi_rater_order,
            youzi::assistants::rater::youzi_rater_delete_many,
            youzi::assistants::seeker::catalog::youzi_seeker_records_order,
            youzi::assistants::seeker::catalog::youzi_seeker_records_enable,
            youzi::assistants::seeker::catalog::youzi_seeker_records_delete,
            youzi::assistants::seeker::catalog::youzi_seeker_group_rename,
            youzi::assistants::seeker::catalog::youzi_seeker_groups_order,
            youzi::assistants::seeker::model::youzi_seeker_prompts,
            youzi::assistants::rater::youzi_rater_prompts,
            youzi::assistants::rater::interview::youzi_rater_interviews,
            youzi::assistants::seeker::files::youzi_seeker_files,
            youzi::assistants::seeker::files::youzi_seeker_cv_text,
            youzi::assistants::rater::interview::youzi_rater_prep,
            youzi::assistants::rater::interview::youzi_rater_prep_save,
            youzi::assistants::rater::interview::youzi_rater_prep_keep,
            youzi::assistants::rater::interview::youzi_rater_review_save,
            youzi::assistants::rater::interview::youzi_rater_summarize,
            youzi::assistants::rater::record::youzi_rater_records,
            youzi::assistants::rater::record::youzi_rater_record,
            youzi::assistants::rater::record::youzi_rater_record_start,
            youzi::assistants::rater::record::youzi_rater_record_edit,
            youzi::assistants::rater::record::youzi_rater_record_save,
            youzi::assistants::rater::record::youzi_rater_record_discard,
            youzi::assistants::rater::interview::youzi_rater_review_keep,
            youzi::assistants::rater::interview::youzi_rater_review_rule,
            youzi::assistants::rater::interview::youzi_rater_interview_delete,
            youzi::assistants::rater::interview::youzi_rater_prep_word_place,
            youzi::assistants::rater::interview::youzi_rater_prep_word,
            youzi::assistants::seeker::youzi_seeker_folder,
            youzi::assistants::seeker::youzi_seeker_new,
            youzi::assistants::seeker::youzi_seeker_save,
            youzi::assistants::seeker::youzi_seeker_delete_blocks,
            youzi::assistants::seeker::youzi_seeker_use_job,
            youzi::assistants::seeker::company::youzi_seeker_knowledge,
            youzi::assistants::seeker::company::youzi_seeker_knowledge_add,
            youzi::assistants::seeker::company::youzi_seeker_company_names,
            youzi::assistants::seeker::company::youzi_seeker_job_company,
            youzi::assistants::seeker::company::youzi_seeker_job_company_set,
            youzi::assistants::seeker::catalog::youzi_seeker_record_save,
            youzi::assistants::seeker::catalog::youzi_seeker_record_delete,
            youzi::assistants::seeker::catalog::youzi_seeker_settings,
            youzi::assistants::seeker::import::youzi_seeker_import_preview,
            youzi::assistants::seeker::import::youzi_seeker_import_confirm,
            youzi::assistants::seeker::workflow::youzi_seeker_assemble,
            youzi::assistants::seeker::workflow::youzi_seeker_sources,
            youzi::assistants::seeker::workflow::youzi_seeker_source_apply,
            youzi::assistants::seeker::workflow::youzi_seeker_reorder,
            youzi::assistants::seeker::youzi_seeker_notes_settle,
            youzi::assistants::seeker::model::youzi_seeker_suggest,
            youzi::assistants::seeker::model::youzi_seeker_review,
            youzi::assistants::seeker::output::youzi_seeker_output_save,
            youzi::assistants::seeker::output::youzi_seeker_export_name,
            youzi::assistants::seeker::output::youzi_seeker_job_resume,
            youzi::assistants::seeker::output::youzi_seeker_job_resumes,
            youzi::assistants::translate::youzi_translate_settings,
            youzi::assistants::translate::youzi_translate_input,
            youzi::assistants::translate::youzi_translate_preview,
            youzi::assistants::translate::youzi_translate_run,
            youzi::assistants::translate::youzi_translate_stop,
            youzi::assistants::pub2md::youzi_pub2md,
            youzi::assistants::pub2md::youzi_pub2md_settings,
            youzi::assistants::pub2md::youzi_pub2md_add,
            youzi::assistants::pub2md::youzi_pub2md_resume,
            youzi::assistants::pub2md::youzi_pub2md_unqueue,
            youzi::assistants::pub2md::youzi_pub2md_stop,
            youzi::assistants::pub2md::youzi_pub2md_open,
            youzi::assistants::pub2md::youzi_pub2md_keys_save,
            youzi::assistants::pub2md::youzi_pub2md_keys_remove,
            youzi::assistants::pub2md::youzi_pub2md_preview,
            youzi::assistants::pub2md::youzi_pub2md_prompts,
            youzi::assistants::pub2md::youzi_pub2md_group,
            youzi::assistants::pub2md::youzi_pub2md_delete,
            youzi::assistants::site::youzi_site_draft,
            youzi::assistants::site::youzi_site_draft_run,
            youzi::assistants::site::youzi_site_draft_stop,
            youzi::assistants::site::youzi_site_draft_clear,
            youzi::storage::youzi_storage,
            youzi::storage::youzi_storage_delete_file,
            youzi::storage::youzi_storage_delete_stray,
            youzi::storage::youzi_storage_clean_empty,
            youzi::memory::youzi_memories,
            youzi::memory::youzi_memory_save,
            youzi::memory::youzi_remember,
            youzi::memory::youzi_memory_flag,
            youzi::memory::youzi_memory_delete,
            youzi::memory::youzi_memory_enabled,
            youzi::memory::youzi_memory_enabled_set,
            youzi::memory::youzi_conversation_memory,
            youzi::memory::youzi_memory_plan,
            youzi::memory::youzi_choose_memories,
            youzi::templates::youzi_templates,
            youzi::templates::youzi_template_save,
            youzi::templates::youzi_template_pin,
            youzi::templates::youzi_template_delete,
            youzi::data::youzi_data_cloud,
            youzi::assist::youzi_assist_model,
            youzi::assist::youzi_assist_model_set,
            youzi::assist::youzi_assist_level,
            youzi::assist::youzi_assist_level_set,
            youzi::assist::youzi_polish,
            youzi::assist::youzi_site_english,
            youzi::assist::youzi_fill_work,
            youzi::assist::youzi_fill_job,
            youzi::assist::youzi_split_task,
            youzi::assist::youzi_trip_missing,
            youzi::assist::youzi_quick_fill,
            youzi::data::youzi_data_cloud_set,
            youzi::templates::youzi_template_used,
            youzi::instructions::youzi_instructions,
            youzi::instructions::youzi_session_instructions_save,
            youzi::instructions::youzi_global_instructions,
            youzi::instructions::youzi_global_instructions_save,
            youzi::instructions::youzi_presets,
            youzi::instructions::youzi_preset_save,
            youzi::instructions::youzi_preset_delete,
            youzi::library::youzi_library,
            youzi::library::youzi_rename,
            youzi::library::youzi_pin,
            youzi::library::youzi_archive,
            youzi::library::youzi_private,
            youzi::library::youzi_conversation_thinking,
            youzi::library::youzi_conversation_effort,
            youzi::chat::youzi_ask_answer,
            youzi::yearly::youzi_year_card,
            youzi::library::youzi_move,
            youzi::library::youzi_delete,
            youzi::library::youzi_project_create,
            youzi::library::youzi_project_update,
            youzi::library::youzi_project_delete,
            youzi::library::youzi_projects_reorder,
            youzi::library::youzi_drop,
            youzi::find::youzi_search,
            youzi::find::youzi_search_models,
            youzi::find::youzi_reveal,
            youzi::export::youzi_copy_conversation,
            youzi::export::youzi_export_markdown,
            youzi::export::youzi_export_package,
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
            modules::finance::fin_range,
            youzi::assistants::importer::youzi_importer,
            youzi::assistants::importer::youzi_importer_save,
            youzi::assistants::importer::youzi_importer_extract,
            youzi::assistants::importer::youzi_importer_match,
            youzi::assistants::importer::youzi_importer_tidy,
            modules::finance::import::fin_import_link_pending,
            modules::finance::import::fin_import_set_category,
            modules::finance::import::fin_import_resolve,
            modules::finance::import::fin_import_unlink,
            modules::finance::import::fin_import_preview,
            modules::finance::import::fin_import_add,
            modules::finance::import::fin_import_ignore,
            modules::finance::import::fin_import_rename,
            modules::finance::import::fin_import_delete,
            modules::finance::import::fin_imports_reorder,
            modules::finance::import::fin_imports,
            modules::finance::import::fin_import_batch,
            modules::finance::import::fin_import_save,
            modules::finance::import::fin_import_commit,
            modules::finance::import::fin_entry_sources,
            modules::finance::import::fin_refund_candidates,
            modules::finance::fin_entries,
            modules::finance::fin_entry_create,
            modules::finance::fin_entry_update,
            modules::finance::fin_entry_delete,
            modules::finance::fin_entries_update,
            modules::finance::fin_entries_delete,
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
            youzi::assist::youzi_split_note,
            youzi::files::youzi_import_image,
            youzi::files::youzi_import_web,
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
            modules::notes::notes_tag_place,
            modules::notes::notes_tag_pins,
            modules::notes::notes_tag_delete,
            search::search_all,
            search::query::search_query,
            search::query::search_similar,
            search::query::search_notes_similar,
            youzi::find::youzi_search_similar,
            youzi::skills::youzi_skills,
            youzi::skills::youzi_skill_set,
            youzi::skills::youzi_skill_key_save,
            youzi::skills::youzi_skill_key_remove,
            youzi::skills::youzi_skill_contact_set,
            youzi::schedule::youzi_schedules,
            youzi::schedule::youzi_schedule_save,
            youzi::schedule::youzi_schedule_enable,
            youzi::schedule::youzi_schedule_delete,
            youzi::schedule::youzi_schedule_run,
            youzi::schedule::youzi_seen,
            youzi::brief::youzi_brief,
            youzi::brief::youzi_brief_write,
            youzi::brief::youzi_brief_set,
            youzi::brief::youzi_brief_prompt,
            autostart::app_autostart,
            autostart::app_autostart_set,
            youzi::act::card::youzi_card_confirm,
            youzi::act::card::youzi_card_cancel,
            search::query::search_index_status,
            health::health_check,
            home::home_overview,
            dock::dock_live,
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
                capture::unregister(app);
                youzi::chat::shutdown(app);
                app.state::<AppState>().youzi.runtime.stop_ours_on_exit();
                app.state::<site::Site>().on_exit();
            }
            _ => {}
        });
}
