//! The macOS menu bar (v0.8): 柚儿园 · 编辑 · 页签 · 窗口. 页签 holds the main window's keys: 新页签 ⌘T,
//! 关闭页签 ⌘W (the last one goes back to 首页), 下一个 / 上一个页签 ⌃Tab ⌃⇧Tab (the page also takes
//! ⌘⇧] ⌘⇧[), then ⌘1–⌘9 for the 索引签 from the top (首页, 模块坞, the pinned modules; the page sends
//! their names, `app_menu_index`). 设置… ⌘, sits in 柚儿园; 关闭窗口 is ⇧⌘W.
//! An item goes to the focused window: the main page acts on it (event `app-menu` with the item's
//! action); the 速记小窗 only closes (⌘W, ⇧⌘W).

use tauri::menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Wry};

const PREFIX: &str = "app:";
const MAX_INDEX: usize = 9;

/// Before the page has said: the two 索引签 always there.
pub fn first_index() -> Vec<String> {
    vec!["首页".into(), "模块坞".into()]
}

pub fn build(app: &AppHandle, index: &[String]) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str, text: &str, key: &str| MenuItem::with_id(app, format!("{PREFIX}{id}"), text, true, Some(key));
    let sep = || PredefinedMenuItem::separator(app);
    let info = app.package_info();
    let about = AboutMetadata { name: Some("柚儿园".into()), version: Some(info.version.to_string()), ..Default::default() };
    let own = Submenu::with_items(
        app,
        "柚儿园",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("关于柚儿园"), Some(about))?,
            &sep()?,
            &item("settings", "设置…", "CmdOrCtrl+Comma")?,
            &sep()?,
            &PredefinedMenuItem::services(app, Some("服务"))?,
            &sep()?,
            &PredefinedMenuItem::hide(app, Some("隐藏柚儿园"))?,
            &PredefinedMenuItem::hide_others(app, Some("隐藏其他"))?,
            &PredefinedMenuItem::show_all(app, Some("全部显示"))?,
            &sep()?,
            &PredefinedMenuItem::quit(app, Some("退出柚儿园"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "编辑",
        true,
        &[
            &PredefinedMenuItem::undo(app, Some("撤销"))?,
            &PredefinedMenuItem::redo(app, Some("重做"))?,
            &sep()?,
            &PredefinedMenuItem::cut(app, Some("剪切"))?,
            &PredefinedMenuItem::copy(app, Some("拷贝"))?,
            &PredefinedMenuItem::paste(app, Some("粘贴"))?,
            &PredefinedMenuItem::select_all(app, Some("全选"))?,
        ],
    )?;
    let tabs = Submenu::with_items(
        app,
        "页签",
        true,
        &[
            &item("new-tab", "新页签", "CmdOrCtrl+T")?,
            &item("close-tab", "关闭页签", "CmdOrCtrl+W")?,
            &sep()?,
            &item("next-tab", "下一个页签", "Ctrl+Tab")?,
            &item("prev-tab", "上一个页签", "Ctrl+Shift+Tab")?,
            &sep()?,
        ],
    )?;
    for (i, name) in index.iter().take(MAX_INDEX).enumerate() {
        tabs.append(&item(&format!("go:{i}"), name, &format!("CmdOrCtrl+{}", i + 1))?)?;
    }
    let window = Submenu::with_items(
        app,
        "窗口",
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some("最小化"))?,
            &PredefinedMenuItem::maximize(app, Some("缩放"))?,
            &PredefinedMenuItem::fullscreen(app, Some("进入全屏幕"))?,
            &sep()?,
            &item("close-window", "关闭窗口", "CmdOrCtrl+Shift+W")?,
        ],
    )?;
    Menu::with_items(app, &[&own, &edit, &tabs, &window])
}

pub fn on_event(app: &AppHandle, e: &MenuEvent) {
    let Some(action) = e.id().as_ref().strip_prefix(PREFIX) else { return };
    let focused = |label: &str| app.get_webview_window(label).and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if focused(crate::capture::LABEL) {
        if matches!(action, "close-tab" | "close-window") {
            crate::capture::request_close(app);
        }
        return;
    }
    match action {
        // these only make sense on a window in front
        "close-tab" | "next-tab" | "prev-tab" if !focused("main") => {}
        "close-window" => {
            if let Some(w) = app.get_webview_window("main").filter(|_| focused("main")) {
                let _ = w.hide();
            }
        }
        _ => {
            crate::tray::show_main(app);
            let _ = app.emit_to("main", "app-menu", action);
        }
    }
}

/// The 索引签's names from the top (the page sends them when they change).
#[tauri::command]
pub fn app_menu_index(app: AppHandle, names: Vec<String>) -> Result<(), String> {
    let menu = build(&app, &names).map_err(|e| e.to_string())?;
    app.set_menu(menu).map_err(|e| e.to_string())?;
    Ok(())
}
