//! Menu bar (v0.3, from the old Picky): Pomelo keeps running when its window closes (⌘Q quits).
//! The bird icon is solid while a local model is ready, hollow otherwise. The menu shows the
//! model status and holds the everyday controls; it is rebuilt only when what it shows changes.

use crate::picky::runtime::{Dot, ServiceState, ServiceStatus, Svc};
use crate::picky::{self, Status};
use crate::AppState;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

const TRAY: &str = "pomelo";
const ICON_READY: &[u8] = include_bytes!("../icons/tray/ready@2x.png");
const ICON_IDLE: &[u8] = include_bytes!("../icons/tray/idle@2x.png");

static SHOWN: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let status = app.state::<AppState>().picky.status();
    TrayIconBuilder::with_id(TRAY)
        .icon(icon(&status)?)
        .icon_as_template(true)
        .tooltip("柚儿园")
        .menu(&build(app, &status)?)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu)
        .build(app)?;
    *SHOWN.lock().unwrap_or_else(|e| e.into_inner()) = signature(&status);
    Ok(())
}

pub fn refresh(app: &AppHandle) {
    let status = app.state::<AppState>().picky.status();
    let sig = signature(&status);
    {
        let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
        if *shown == sig {
            return;
        }
        *shown = sig;
    }
    let Some(tray) = app.tray_by_id(TRAY) else { return };
    if let Ok(menu) = build(app, &status) {
        let _ = tray.set_menu(Some(menu));
    }
    if let Ok(i) = icon(&status) {
        let _ = tray.set_icon(Some(i));
        let _ = tray.set_icon_as_template(true);
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn icon(s: &Status) -> tauri::Result<Image<'static>> {
    Image::from_bytes(if s.local.presentation.dot == Dot::Ready { ICON_READY } else { ICON_IDLE })
}

/// The first line: the selected model and its state.
fn headline(s: &Status) -> String {
    match s.selection.strip_prefix("cloud:").and_then(|id| s.cloud.iter().find(|m| m.id == id)) {
        Some(m) => format!("{} · 云端", m.name),
        None => s.local.presentation.menu_title.clone(),
    }
}

fn service(s: &Status, svc: Svc) -> &ServiceStatus {
    match svc {
        Svc::Embedding => &s.embedding,
        Svc::Rerank => &s.rerank,
    }
}

fn service_title(s: &Status, svc: Svc) -> String {
    format!("{} · {}", svc.label(), service(s, svc).text)
}

/// Everything the menu shows; a change rebuilds it.
fn signature(s: &Status) -> String {
    let local: Vec<String> = s.local.models.iter().map(|m| format!("{}|{}", m.key, m.name)).collect();
    let cloud: Vec<String> = s.cloud.iter().map(|m| format!("{}|{}", m.id, m.name)).collect();
    format!(
        "{}\n{:?}\n{:?}|{:?}|{}\n{}\n{}|{:?}|{}|{:?}\n{}\n{:?}\n{:?}",
        headline(s),
        s.local.issue.as_ref().map(|i| &i.title),
        s.local.active.as_ref().map(|m| &m.key),
        s.local.presentation.dot,
        s.local.working,
        s.selection,
        service_title(s, Svc::Embedding),
        s.embedding.state,
        service_title(s, Svc::Rerank),
        s.rerank.state,
        s.generating,
        local,
        cloud,
    )
}

fn build(app: &AppHandle, s: &Status) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str, text: &str, enabled: bool| MenuItem::with_id(app, id, text, enabled, None::<&str>);
    let sep = || PredefinedMenuItem::separator(app);
    let local_selected = s.selection == "local";
    let active = s.local.active.as_ref().map(|m| m.key.as_str());

    let mut top: Vec<Box<dyn IsMenuItem<Wry>>> = vec![Box::new(item("status", &headline(s), false)?)];
    if local_selected && s.local.presentation.dot == Dot::Error {
        if let Some(i) = &s.local.issue {
            top.push(Box::new(item("issue", &i.title.chars().take(60).collect::<String>(), false)?));
        }
    }
    top.push(Box::new(sep()?));
    top.push(Box::new(item("open", "打开柚儿园", true)?));

    // 切换聊天模型
    let mut models: Vec<Box<dyn IsMenuItem<Wry>>> = vec![Box::new(item("h-local", "本地模型", false)?)];
    for m in &s.local.models {
        let on = local_selected && active == Some(m.key.as_str());
        models.push(Box::new(CheckMenuItem::with_id(app, format!("model:local:{}", m.key), &m.name, !on && !s.local.working, on, None::<&str>)?));
    }
    if s.local.models.is_empty() {
        models.push(Box::new(item("h-none", "模型清单里没有可用的模型", false)?));
    }
    if !s.cloud.is_empty() {
        models.push(Box::new(sep()?));
        models.push(Box::new(item("h-cloud", "云端模型", false)?));
        for m in &s.cloud {
            let on = s.selection == format!("cloud:{}", m.id);
            models.push(Box::new(CheckMenuItem::with_id(app, format!("model:cloud:{}", m.id), &m.name, !on, on, None::<&str>)?));
        }
    }
    let refs: Vec<&dyn IsMenuItem<Wry>> = models.iter().map(|b| b.as_ref()).collect();
    let switch = Submenu::with_id_and_items(app, "switch", if s.generating { "切换聊天模型（正在生成回复）" } else { "切换聊天模型" }, !s.generating, &refs)?;
    top.push(Box::new(switch));
    top.push(Box::new(item("stop", "停止本地模型", active.is_some() && !s.local.working && !s.generating)?));
    top.push(Box::new(sep()?));

    // 向量服务、重排服务
    for svc in Svc::ALL {
        let st = service(s, svc);
        let stopped = st.state == ServiceState::Stopped;
        let name = st.model.as_ref().map_or("模型清单里没有", |m| m.name.as_str());
        let id = svc.arg();
        let items = [
            item(&format!("{id}-name"), &format!("{name} · {}", st.text), false)?,
            item(&format!("{id}-start"), &format!("启动{}", svc.label()), stopped && !st.working && st.model.is_some())?,
            item(&format!("{id}-stop"), &format!("停止{}", svc.label()), !stopped && !st.working)?,
        ];
        let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();
        top.push(Box::new(Submenu::with_id_and_items(app, id, service_title(s, svc), true, &refs)?));
    }
    top.push(Box::new(sep()?));
    top.push(Box::new(MenuItem::with_id(app, "quit", "退出柚儿园", true, Some("CmdOrCtrl+Q"))?));

    let refs: Vec<&dyn IsMenuItem<Wry>> = top.iter().map(|b| b.as_ref()).collect();
    Menu::with_items(app, &refs)
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    let result = match id {
        "open" => {
            show_main(app);
            Ok(())
        }
        "quit" => {
            app.exit(0);
            Ok(())
        }
        "stop" => picky::stop_model(app),
        "embedding-start" => picky::service(app, Svc::Embedding, "start"),
        "embedding-stop" => picky::service(app, Svc::Embedding, "stop"),
        "rerank-start" => picky::service(app, Svc::Rerank, "start"),
        "rerank-stop" => picky::service(app, Svc::Rerank, "stop"),
        other => match other.strip_prefix("model:") {
            Some(choice) => picky::select(app, choice),
            None => Ok(()),
        },
    };
    if let Err(e) = result {
        eprintln!("menu bar: {e}");
    }
    // a click toggles a check item by itself: always redraw from the real state
    SHOWN.lock().unwrap_or_else(|e| e.into_inner()).clear();
    refresh(app);
}
