//! 开机自动启动 (v0.4): a LaunchAgent (~/Library/LaunchAgents/Pomelo OS.plist, written by
//! tauri-plugin-autostart; no System Events) opening Pomelo with `--hidden`: menu bar only, no
//! window. The switch is pomelo.db setting `app.autostart` (default on). Only the installed app on the
//! real data dir registers (at launch, following the switch); dev builds and scratch data dirs
//! (POMELO_DATA_DIR) only keep the setting, so testing never touches the real login items.

use crate::{settings, AppState};
use serde::Serialize;
use std::sync::OnceLock;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

pub const HIDDEN_ARG: &str = "--hidden";
const KEY: &str = "app.autostart";
static REGISTERS: OnceLock<bool> = OnceLock::new();

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_autostart::Builder::new().arg(HIDDEN_ARG).build()
}

/// Opened by the LaunchAgent: stay in the menu bar (lib.rs hides the window in setup).
pub fn launched_hidden() -> bool {
    std::env::args().any(|a| a == HIDDEN_ARG)
}

/// At launch: whether this copy may register, then make the login item follow the switch.
pub fn init(app: &AppHandle, data_dir_overridden: bool) {
    let _ = REGISTERS.set(!cfg!(debug_assertions) && !data_dir_overridden);
    let on = wanted(app);
    if let Err(e) = apply(app, on) {
        eprintln!("autostart: {e}");
    }
}

fn registers() -> bool {
    REGISTERS.get().copied().unwrap_or(false)
}

fn wanted(app: &AppHandle) -> bool {
    use tauri::Manager;
    app.state::<AppState>().with_db(|c| settings::get(c, KEY)).ok().flatten().and_then(|v| v.as_bool()).unwrap_or(true)
}

fn apply(app: &AppHandle, on: bool) -> Result<(), String> {
    if !registers() {
        return Ok(());
    }
    let m = app.autolaunch();
    match (on, m.is_enabled().map_err(|e| e.to_string())?) {
        (true, false) => m.enable().map_err(|e| e.to_string()),
        (false, true) => m.disable().map_err(|e| e.to_string()),
        _ => Ok(()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Autostart {
    pub on: bool,
    /// This copy registers it (the installed app); otherwise only the setting is kept.
    pub registers: bool,
}

#[tauri::command]
pub fn app_autostart(app: AppHandle) -> Autostart {
    Autostart { on: wanted(&app), registers: registers() }
}

#[tauri::command]
pub fn app_autostart_set(app: AppHandle, state: State<AppState>, on: bool) -> Result<Autostart, String> {
    apply(&app, on).map_err(|e| format!("没能更改开机自动启动：{e}"))?;
    state.with_db(|c| settings::set(c, KEY, &serde_json::Value::Bool(on)))?;
    Ok(Autostart { on, registers: registers() })
}
