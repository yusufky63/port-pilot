//! Updates from the GitHub release feed. The feed (`latest.json` of the newest release) names
//! the version and carries the signature of its installer; the updater refuses an installer
//! whose signature does not match the public key built into the app (`tauri.conf.json`).
//!
//! The check is PortPilot's only network request. It sends nothing about the computer beyond
//! what any HTTPS request carries, and Settings can turn the automatic check off.
//!
//! Portable copies are never replaced in place: the installer would add a second, installed
//! copy, so they only point to the release page.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::state::AppState;
use crate::tray;

pub const RELEASES_URL: &str = "https://github.com/yusufky63/port-pilot/releases/latest";
const FIRST_CHECK: Duration = Duration::from_secs(20);
const INTERVAL: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    pub notes: Option<String>,
    /// RFC 3339, as the release feed states it.
    pub date: Option<String>,
    /// A portable copy updates by downloading the new zip.
    pub portable: bool,
}

/// Asks the feed once; remembers and announces a newer version.
async fn fetch(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    let state = app.state::<AppState>();
    let info = update.map(|u| UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone().filter(|b| !b.trim().is_empty()),
        date: u
            .raw_json
            .get("pub_date")
            .and_then(|d| d.as_str())
            .map(str::to_string),
        portable: state.portable,
    });
    *state.update.write() = info.clone();
    if let Some(i) = &info {
        let _ = app.emit("update-available", i);
    }
    tray::refresh(app);
    Ok(info)
}

/// Checks 20 seconds after start and then once a day, while the setting is on.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        loop {
            let enabled = app.state::<AppState>().settings.read().auto_update;
            if enabled {
                // A failed check (offline, GitHub unreachable) simply waits for the next one.
                let _ = tauri::async_runtime::block_on(fetch(&app));
            }
            std::thread::sleep(INTERVAL);
        }
    });
}

/// The newer version found by the last check, if any.
#[tauri::command]
pub fn update_status(app: AppHandle) -> Option<UpdateInfo> {
    app.state::<AppState>().update.read().clone()
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    fetch(&app).await
}

/// Downloads the signed installer, stops the runs the way quitting does, then lets the
/// installer replace this copy and start the new version. Portable copies open the release
/// page instead.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.portable {
        return portpilot_win::shell::open(RELEASES_URL);
    }
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("PortPilot is up to date")?;
    // `download` verifies the signature before it returns the installer.
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    let grace = Duration::from_secs(u64::from(state.settings.read().grace_seconds));
    let runs = state.runs.clone();
    tauri::async_runtime::spawn_blocking(move || runs.stop_all(grace))
        .await
        .map_err(|e| e.to_string())?;
    // On Windows the installer takes over and this process exits.
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart();
}

#[tauri::command]
pub fn open_release_page() -> Result<(), String> {
    portpilot_win::shell::open(RELEASES_URL)
}
