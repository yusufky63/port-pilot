//! Tauri shell: a discovery thread that refreshes the snapshot, the tray, and typed commands
//! for the UI. The WebView never names a process, a command or an arbitrary path: stop
//! requests carry a listener id from the current snapshot, open requests a URL or folder that
//! discovery found.

mod actions;
mod runs;
mod state;
mod tray;
mod update;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use portpilot_core::net::parse_port_range;
use portpilot_core::{Snapshot, StopReport};
use portpilot_engine::doctor::{self, Environment, PortDiagnosis};
use portpilot_engine::history::PortTracker;
use portpilot_engine::projects::{self, ProjectGroup};
use portpilot_engine::store::now_ms;
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

/// Port changes go to the history while it is turned on.
fn record_history(state: &AppState, tracker: &mut PortTracker, snapshot: &Snapshot, enabled: bool) {
    let changes = tracker.update(snapshot);
    if !enabled || changes.is_empty() {
        return;
    }
    let now = now_ms();
    let _ = state.with_store(|s| {
        for (kind, event) in &changes {
            s.record(*kind, now, event)?;
        }
        Ok(())
    });
}

fn start_discovery(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let mut discovery = Discovery::new(options_from(&state.settings.read()));
        let mut tracker = PortTracker::default();
        let mut pruned = Instant::now();
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
                // This app's runs with every process of their jobs, then the runs that
                // `portctl run` started, matched through parent chains.
                let mut managed = state.runs.managed();
                if let Some(store) = &state.store
                    && let Ok(store) = store.lock()
                {
                    if pruned.elapsed() > Duration::from_secs(3600) {
                        let _ = store.prune(now_ms());
                        pruned = Instant::now();
                    }
                    for run in portpilot_engine::runs::managed_from_store(&store, settings.history)
                    {
                        if !managed.iter().any(|m| m.run_id == run.run_id) {
                            managed.push(run);
                        }
                    }
                }
                discovery.set_managed(managed);
                let snap = Arc::new(discovery.snapshot());
                remember_paths(&state, &snap);
                record_history(&state, &mut tracker, &snap, settings.history);
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
    /// Why leases, trusted commands and history are unavailable, if they are.
    store_error: Option<String>,
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
        store_error: state.store_error.clone(),
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
    state.runs.set_prefs(settings.history_prefs());
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
    let grace = Duration::from_secs(u64::from(state.settings.read().grace_seconds));
    // A run this app started is stopped as a whole: CTRL+BREAK to its process group, and its
    // job is ended on consent.
    if let Some(m) = &listener.managed
        && state.runs.owns(&m.run_id)
    {
        let (runs, id) = (state.runs.clone(), m.run_id.clone());
        let report = tauri::async_runtime::spawn_blocking(move || runs.stop(&id, grace, force))
            .await
            .map_err(|e| e.to_string())??;
        state.invalidate.store(true, Ordering::Relaxed);
        state.wake.notify();
        return Ok(report);
    }
    let target = listener
        .stop
        .target
        .clone()
        .ok_or("PortPilot does not stop this listener")?;
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
        return Err("PortPilot only opens folders it found itself".into());
    }
    match how.as_str() {
        "folder" => actions::open_folder(&path),
        "terminal" => actions::open_terminal(&path),
        "editor" => actions::open_editor(&path),
        _ => Err(format!("unknown action {how}")),
    }
}

/// Quitting with runs still active asks in the window first: they are stopped before the app
/// exits, since closing their jobs would end them without a graceful request.
pub(crate) fn request_quit(app: &AppHandle) {
    let active = app.state::<AppState>().runs.active_count();
    if active == 0 {
        app.exit(0);
        return;
    }
    tray::show_main(app);
    let _ = app.emit("quit-requested", active);
}

/// The user confirmed quitting with active runs: CTRL+BREAK to each, the grace period, then
/// the jobs of runs that are still running are ended.
#[tauri::command]
async fn quit_app(app: AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let grace = Duration::from_secs(u64::from(state.settings.read().grace_seconds));
    let runs = state.runs.clone();
    tauri::async_runtime::spawn_blocking(move || runs.stop_all(grace))
        .await
        .map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::load())
        .setup(|app| {
            tray::create(app.handle())?;
            start_discovery(app.handle().clone());
            update::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.settings.read().close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else if state.runs.active_count() > 0 {
                    api.prevent_close();
                    request_quit(window.app_handle());
                }
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
            quit_app,
            runs::pick_project_folder,
            runs::recent_projects,
            runs::recipe_load,
            runs::run_plan,
            runs::run_reallocate,
            runs::run_start,
            runs::runs_list,
            runs::run_output,
            runs::run_output_text,
            runs::run_stop,
            runs::run_forget,
            runs::leases_list,
            runs::lease_release,
            runs::history_list,
            runs::history_clear,
            runs::trust_list,
            runs::trust_revoke,
            update::update_status,
            update::update_check,
            update::update_install,
            update::open_release_page,
        ])
        .run(tauri::generate_context!())
        .expect("error while running PortPilot");
}
