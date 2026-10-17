//! Tauri shell: a discovery thread that refreshes the snapshot, the tray, and typed commands
//! for the UI. The WebView never names a process, a command or an arbitrary path: stop
//! requests carry a listener id from the current snapshot, open requests a URL or folder that
//! discovery found.

mod actions;
mod state;
mod tray;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use portpilot_core::net::parse_port_range;
use portpilot_core::{Snapshot, StopReport};
use portpilot_engine::doctor::{self, Environment, PortDiagnosis};
use portpilot_engine::projects::{self, ProjectGroup};
use portpilot_engine::{Discovery, Options, StopRequest};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

use crate::state::{AppState, Settings};

type CmdResult<T> = Result<T, String>;

fn options_from(s: &Settings) -> Options {
    Options {
        udp: true,
        wsl: s.include_wsl,
        docker: s.include_docker,
        raw_command_lines: !s.mask_secrets,
        runtime_ttl: Duration::from_secs(10),
        git_ttl: Duration::from_secs(10),
    }
}

fn remember_paths(state: &AppState, snapshot: &Snapshot) {
    let mut known = state.known_paths.write();
    for l in &snapshot.listeners {
        if let Some(p) = &l.project {
            if let Some(open) = &p.open_path {
                known.insert(AppState::path_key(open));
            }
            if let Some(g) = &p.git
                && l.runtime == portpilot_core::Runtime::Windows
            {
                known.insert(AppState::path_key(&g.top_level));
            }
        }
    }
}

fn start_discovery(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let mut discovery = Discovery::new(options_from(&state.settings.read()));
        loop {
            let settings = state.settings.read().clone();
            let opts = options_from(&settings);
            if &opts != discovery.options() {
                discovery.set_options(opts);
            }
            if state.invalidate.swap(false, Ordering::Relaxed) {
                discovery.invalidate_runtimes();
            }
            if !state.paused.load(Ordering::Relaxed) {
                let snap = Arc::new(discovery.snapshot());
                remember_paths(&state, &snap);
                *state.snapshot.write() = Some(snap.clone());
                let _ = app.emit("snapshot", &*snap);
                tray::refresh(&app);
            }
            let mut woken = state.wake.flag.lock();
            if !*woken {
                state.wake.cond.wait_for(
                    &mut woken,
                    Duration::from_secs(u64::from(settings.refresh_seconds)),
                );
            }
            *woken = false;
        }
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: String,
    portable: bool,
    data_dir: String,
    elevated: bool,
    user: Option<String>,
    has_vscode: bool,
    settings_error: Option<String>,
    paused: bool,
}

#[tauri::command]
fn app_info(app: AppHandle, state: State<AppState>) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        portable: state.portable,
        data_dir: state.data_dir.display().to_string(),
        elevated: portpilot_win::process::is_elevated(),
        user: portpilot_win::process::current_user(),
        has_vscode: actions::has_vscode(),
        settings_error: state.settings_error.read().clone(),
        paused: state.paused.load(Ordering::Relaxed),
    }
}

#[tauri::command]
fn snapshot_get(state: State<AppState>) -> Option<Snapshot> {
    state.snapshot.read().as_deref().cloned()
}

#[tauri::command]
fn refresh_now(state: State<AppState>) {
    state.invalidate.store(true, Ordering::Relaxed);
    state.wake.notify();
}

#[tauri::command]
fn set_paused(app: AppHandle, state: State<AppState>, paused: bool) {
    state.paused.store(paused, Ordering::Relaxed);
    state.wake.notify();
    let _ = app.emit("paused", paused);
    tray::refresh(&app);
}

#[tauri::command]
fn settings_get(state: State<AppState>) -> Settings {
    state.settings.read().clone()
}

#[tauri::command]
fn settings_set(state: State<AppState>, settings: Settings) -> CmdResult<Settings> {
    let settings = settings.sanitized();
    settings.save(&state.data_dir)?;
    *state.settings.write() = settings.clone();
    *state.settings_error.write() = None;
    state.wake.notify();
    Ok(settings)
}

#[tauri::command]
fn set_ui_language(app: AppHandle, state: State<AppState>, language: String) {
    *state.ui_language.write() = if language == "tr" {
        "tr".into()
    } else {
        "en".into()
    };
    tray::refresh(&app);
}

#[tauri::command]
async fn stop_listener(
    app: AppHandle,
    listener_id: String,
    force: bool,
    tree: bool,
) -> CmdResult<StopReport> {
    let state = app.state::<AppState>();
    let snapshot = state.snapshot.read().clone().ok_or("no snapshot yet")?;
    let listener = snapshot
        .listeners
        .iter()
        .find(|l| l.id == listener_id)
        .ok_or("this listener is gone; refresh and try again")?;
    let target = listener
        .stop
        .target
        .clone()
        .ok_or("Port Pilot does not stop this listener")?;
    let grace = Duration::from_secs(u64::from(state.settings.read().grace_seconds));
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let req = StopRequest {
        target,
        force,
        tree,
        grace,
    };
    let report = tauri::async_runtime::spawn_blocking(move || portpilot_engine::stop(&req, &exe))
        .await
        .map_err(|e| e.to_string())?;
    state.invalidate.store(true, Ordering::Relaxed);
    state.wake.notify();
    Ok(report)
}

#[tauri::command]
async fn diagnose_port(app: AppHandle, port: u16) -> CmdResult<PortDiagnosis> {
    let snapshot = app
        .state::<AppState>()
        .snapshot
        .read()
        .clone()
        .ok_or("no snapshot yet")?;
    tauri::async_runtime::spawn_blocking(move || {
        doctor::diagnose_port(port, &snapshot, &doctor::excluded_tcp_ranges())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn find_free(app: AppHandle, range: String, count: usize) -> CmdResult<Vec<u16>> {
    let range = parse_port_range(&range)?;
    let snapshot = app
        .state::<AppState>()
        .snapshot
        .read()
        .clone()
        .ok_or("no snapshot yet")?;
    tauri::async_runtime::spawn_blocking(move || {
        doctor::find_free(
            range,
            count.clamp(1, 50),
            &snapshot,
            &doctor::excluded_tcp_ranges(),
        )
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn environment() -> CmdResult<Environment> {
    tauri::async_runtime::spawn_blocking(doctor::environment)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn projects_overview(app: AppHandle) -> CmdResult<Vec<ProjectGroup>> {
    let snapshot = app
        .state::<AppState>()
        .snapshot
        .read()
        .clone()
        .ok_or("no snapshot yet")?;
    let groups = tauri::async_runtime::spawn_blocking(move || projects::overview(&snapshot))
        .await
        .map_err(|e| e.to_string())?;
    let state = app.state::<AppState>();
    {
        let mut known = state.known_paths.write();
        for g in &groups {
            if g.runtime == portpilot_core::Runtime::Windows {
                for w in &g.worktrees {
                    known.insert(AppState::path_key(&w.path));
                }
            }
        }
    }
    *state.projects.write() = groups.clone();
    Ok(groups)
}

#[tauri::command]
fn open_url(state: State<AppState>, listener_id: String) -> CmdResult<()> {
    let snapshot = state.snapshot.read().clone().ok_or("no snapshot yet")?;
    let url = snapshot
        .listeners
        .iter()
        .find(|l| l.id == listener_id)
        .and_then(|l| l.url.clone())
        .ok_or("this listener has no URL")?;
    actions::open_url(&url)
}

#[tauri::command]
fn open_path(state: State<AppState>, path: String, how: String) -> CmdResult<()> {
    if !state.is_known_path(&path) {
        return Err("Port Pilot only opens folders it found itself".into());
    }
    match how.as_str() {
        "folder" => actions::open_folder(&path),
        "terminal" => actions::open_terminal(&path),
        "editor" => actions::open_editor(&path),
        _ => Err(format!("unknown action {how}")),
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(AppState::load())
        .setup(|app| {
            tray::create(app.handle())?;
            start_discovery(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.state::<AppState>().settings.read().close_to_tray
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            snapshot_get,
            refresh_now,
            set_paused,
            settings_get,
            settings_set,
            set_ui_language,
            stop_listener,
            diagnose_port,
            find_free,
            environment,
            projects_overview,
            open_url,
            open_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Port Pilot");
}
