//! The notification-area icon: counts, the dev listeners at a glance, pause and quit. Nothing
//! here stops a process; stopping always goes through the window and its confirmation.

use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use portpilot_core::{Listener, ProcessClass, Protocol, Snapshot};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::state::AppState;

const TRAY_ID: &str = "main";
const MAX_ITEMS: usize = 10;

struct Texts {
    open: &'static str,
    pause: &'static str,
    quit: &'static str,
    paused: &'static str,
}

fn texts(lang: &str) -> Texts {
    if lang == "tr" {
        Texts {
            open: "PortPilot'u aç",
            pause: "Keşfi duraklat",
            quit: "Çık",
            paused: "duraklatıldı",
        }
    } else {
        Texts {
            open: "Open PortPilot",
            pause: "Pause discovery",
            quit: "Quit",
            paused: "paused",
        }
    }
}

fn summary(lang: &str, listeners: usize, conflicts: usize) -> String {
    if lang == "tr" {
        format!("{listeners} dinleyici, {conflicts} çakışma")
    } else {
        let l = if listeners == 1 {
            "listener"
        } else {
            "listeners"
        };
        let c = if conflicts == 1 {
            "conflict"
        } else {
            "conflicts"
        };
        format!("{listeners} {l}, {conflicts} {c}")
    }
}

/// Listeners worth a tray entry: TCP, not owned by Windows itself.
fn dev_listeners(s: &Snapshot) -> Vec<&Listener> {
    s.listeners
        .iter()
        .filter(|l| l.protocol == Protocol::Tcp)
        .filter(|l| {
            !l.process
                .as_ref()
                .is_some_and(|p| matches!(p.class, ProcessClass::System | ProcessClass::Service))
        })
        .collect()
}

fn item_label(l: &Listener) -> String {
    let owner = l
        .container
        .as_ref()
        .map(|c| c.name.clone())
        .or_else(|| l.process.as_ref().map(|p| p.name.clone()))
        .unwrap_or_default();
    match &l.project {
        Some(p) => format!("{}    {owner} · {}", l.port, p.name),
        None => format!("{}    {owner}", l.port),
    }
}

/// Last menu content, so the menu is only rebuilt when it would look different.
static LAST: Mutex<String> = Mutex::new(String::new());

fn build_menu(app: &AppHandle, snapshot: Option<&Snapshot>) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let lang = state.ui_language.read().clone();
    let t = texts(&lang);
    let paused = state.paused.load(Ordering::Relaxed);
    let menu = Menu::new(app)?;
    if let Some(s) = snapshot {
        let dev = dev_listeners(s);
        menu.append(&MenuItem::with_id(
            app,
            "summary",
            summary(&lang, dev.len(), s.conflicts.len()),
            false,
            None::<&str>,
        )?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        for l in dev.iter().take(MAX_ITEMS) {
            menu.append(&MenuItem::with_id(
                app,
                format!("listener:{}", l.id),
                item_label(l),
                true,
                None::<&str>,
            )?)?;
        }
        if !dev.is_empty() {
            menu.append(&PredefinedMenuItem::separator(app)?)?;
        }
    }
    menu.append(&MenuItem::with_id(app, "open", t.open, true, None::<&str>)?)?;
    menu.append(&CheckMenuItem::with_id(
        app,
        "pause",
        t.pause,
        true,
        paused,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", t.quit, true, None::<&str>)?)?;
    Ok(menu)
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, None)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("PortPilot")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref().to_string();
            match id.as_str() {
                "open" => show_main(app),
                "quit" => crate::request_quit(app),
                "pause" => {
                    let state = app.state::<AppState>();
                    let now = !state.paused.load(Ordering::Relaxed);
                    state.paused.store(now, Ordering::Relaxed);
                    state.wake.notify();
                    let _ = app.emit("paused", now);
                    refresh(app);
                }
                other => {
                    if let Some(listener) = other.strip_prefix("listener:") {
                        show_main(app);
                        let _ = app.emit("focus-listener", listener.to_string());
                    }
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Updates the tooltip and, when the content changed, the menu.
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let state = app.state::<AppState>();
    let snapshot = state.snapshot.read().clone();
    let lang = state.ui_language.read().clone();
    let paused = state.paused.load(Ordering::Relaxed);
    let mut tooltip = "PortPilot".to_string();
    if let Some(s) = &snapshot {
        tooltip = format!(
            "PortPilot: {}",
            summary(&lang, dev_listeners(s).len(), s.conflicts.len())
        );
    }
    if paused {
        tooltip = format!("{tooltip} ({})", texts(&lang).paused);
    }
    let _ = tray.set_tooltip(Some(&tooltip));

    let signature = format!(
        "{lang}|{paused}|{}",
        snapshot
            .as_ref()
            .map(|s| {
                let dev = dev_listeners(s);
                let items: Vec<String> = dev
                    .iter()
                    .take(MAX_ITEMS)
                    .map(|l| format!("{}={}", l.id, item_label(l)))
                    .collect();
                format!("{}|{}|{}", dev.len(), s.conflicts.len(), items.join(","))
            })
            .unwrap_or_default()
    );
    let mut last = LAST.lock();
    if *last == signature {
        return;
    }
    if let Ok(menu) = build_menu(app, snapshot.as_deref()) {
        let _ = tray.set_menu(Some(menu));
        *last = signature;
    }
}
