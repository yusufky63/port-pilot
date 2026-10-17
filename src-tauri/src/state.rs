//! Settings, the data folder, and the state shared between the discovery thread, the tray and
//! the commands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex as StdMutex};

use parking_lot::{Condvar, Mutex, RwLock};
use portpilot_core::Snapshot;
use portpilot_engine::paths::{data_dir, portable_dir};
use portpilot_engine::projects::ProjectGroup;
use portpilot_engine::runs::RunManager;
use portpilot_engine::store::{HistoryPrefs, Store};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// "system", "light" or "dark".
    pub theme: String,
    /// "system", "en" or "tr".
    pub language: String,
    pub refresh_seconds: u32,
    pub include_wsl: bool,
    pub include_docker: bool,
    /// Mask tokens and passwords in command lines.
    pub mask_secrets: bool,
    /// Seconds a process gets to exit after a graceful request.
    pub grace_seconds: u32,
    /// Closing the window keeps PortPilot in the notification area.
    pub close_to_tray: bool,
    /// Record runs, exits and port changes in the local history (14 days).
    pub history: bool,
    /// Also record the (masked) command line of each run.
    pub history_command_lines: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "system".into(),
            language: "system".into(),
            refresh_seconds: 3,
            include_wsl: true,
            include_docker: true,
            mask_secrets: true,
            grace_seconds: 5,
            close_to_tray: true,
            history: true,
            history_command_lines: false,
        }
    }
}

impl Settings {
    /// Keeps values inside the ranges the UI offers, whatever the file says.
    pub fn sanitized(mut self) -> Self {
        self.refresh_seconds = self.refresh_seconds.clamp(2, 60);
        self.grace_seconds = self.grace_seconds.clamp(1, 60);
        if !matches!(self.theme.as_str(), "system" | "light" | "dark") {
            self.theme = "system".into();
        }
        if !matches!(self.language.as_str(), "system" | "en" | "tr") {
            self.language = "system".into();
        }
        self
    }

    pub fn load(dir: &Path) -> Result<Settings, String> {
        let path = dir.join("settings.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Settings>(&text)
                .map(Settings::sanitized)
                .map_err(|e| format!("{} is not valid: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn history_prefs(&self) -> HistoryPrefs {
        HistoryPrefs {
            enabled: self.history,
            command_lines: self.history && self.history_command_lines,
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // Write to a temporary file first so a crash never leaves half a settings file.
        let tmp = dir.join("settings.json.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dir.join("settings.json")).map_err(|e| e.to_string())
    }
}

/// Wakes the discovery thread early: after a stop, a settings change or a manual refresh.
#[derive(Default)]
pub struct Wake {
    pub flag: Mutex<bool>,
    pub cond: Condvar,
}

impl Wake {
    pub fn notify(&self) {
        *self.flag.lock() = true;
        self.cond.notify_all();
    }
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub portable: bool,
    pub settings: RwLock<Settings>,
    /// Set when settings.json exists but cannot be read; defaults are used and saving is
    /// refused so the file is not overwritten unseen.
    pub settings_error: RwLock<Option<String>>,
    pub snapshot: RwLock<Option<Arc<Snapshot>>>,
    pub projects: RwLock<Vec<ProjectGroup>>,
    /// Folders the UI may open: project roots and worktrees PortPilot itself found.
    pub known_paths: RwLock<HashSet<String>>,
    pub paused: AtomicBool,
    pub wake: Wake,
    /// Drop the WSL and Docker caches on the next refresh.
    pub invalidate: AtomicBool,
    /// UI language resolved by the frontend, for the tray menu.
    pub ui_language: RwLock<String>,
    /// `portpilot.db`: leases, trusted commands, runs and history. `None` when it cannot be
    /// opened; the rest of the app works without it.
    pub store: Option<Arc<StdMutex<Store>>>,
    pub store_error: Option<String>,
    /// Runs this app started. Dropping it on exit closes their jobs.
    pub runs: RunManager,
}

impl AppState {
    pub fn load() -> Self {
        let dir = data_dir();
        let (settings, error) = match Settings::load(&dir) {
            Ok(s) => (s, None),
            Err(e) => (Settings::default(), Some(e)),
        };
        let (store, store_error) = match Store::open(&dir.join("portpilot.db")) {
            Ok(s) => (Some(Arc::new(StdMutex::new(s))), None),
            Err(e) => (None, Some(e)),
        };
        let helper = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("port-pilot.exe"));
        let runs = RunManager::new(helper, store.clone(), settings.history_prefs());
        AppState {
            portable: portable_dir().is_some(),
            data_dir: dir,
            settings: RwLock::new(settings),
            settings_error: RwLock::new(error),
            snapshot: RwLock::new(None),
            projects: RwLock::new(Vec::new()),
            known_paths: RwLock::new(HashSet::new()),
            paused: AtomicBool::new(false),
            wake: Wake::default(),
            invalidate: AtomicBool::new(false),
            ui_language: RwLock::new("en".into()),
            store,
            store_error,
            runs,
        }
    }

    /// Runs `f` with the database, or says why it is not available.
    pub fn with_store<T>(
        &self,
        f: impl FnOnce(&mut Store) -> Result<T, String>,
    ) -> Result<T, String> {
        let store = self.store.as_ref().ok_or_else(|| {
            self.store_error
                .clone()
                .unwrap_or_else(|| "the history database is not available".into())
        })?;
        let mut guard = store.lock().map_err(|e| e.to_string())?;
        f(&mut guard)
    }

    pub fn remember_path(&self, p: &str) {
        self.known_paths.write().insert(Self::path_key(p));
    }

    pub fn path_key(p: &str) -> String {
        p.trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_ascii_lowercase()
    }

    pub fn is_known_path(&self, p: &str) -> bool {
        self.known_paths.read().contains(&Self::path_key(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_and_sanitize() {
        let dir = std::env::temp_dir().join(format!("pp-settings-{}", std::process::id()));
        let s = Settings {
            refresh_seconds: 1,
            theme: "neon".into(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.refresh_seconds, 2);
        assert_eq!(s.theme, "system");
        s.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir).unwrap(), s);
        std::fs::write(dir.join("settings.json"), "{ nope").unwrap();
        assert!(Settings::load(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(s.theme, "dark");
        assert_eq!(s.grace_seconds, 5);
        assert!(s.history);
        assert!(!s.history_command_lines);
        assert_eq!(
            s.history_prefs(),
            HistoryPrefs {
                enabled: true,
                command_lines: false
            }
        );
    }

    #[test]
    fn path_keys() {
        assert_eq!(AppState::path_key(r"C:\Src\Shop\"), r"c:\src\shop");
        assert_eq!(AppState::path_key("C:/src/shop"), r"c:\src\shop");
    }
}
