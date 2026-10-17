//! Settings, the data folder, and the state shared between the discovery thread, the tray and
//! the commands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use parking_lot::{Condvar, Mutex, RwLock};
use portpilot_core::Snapshot;
use portpilot_engine::projects::ProjectGroup;
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
    /// Closing the window keeps Port Pilot in the notification area.
    pub close_to_tray: bool,
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

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // Write to a temporary file first so a crash never leaves half a settings file.
        let tmp = dir.join("settings.json.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dir.join("settings.json")).map_err(|e| e.to_string())
    }
}

/// `<exe folder>\data` when a `portable` marker file sits next to the exe.
pub fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join("portable").is_file().then(|| dir.join("data"))
}

pub fn data_dir() -> PathBuf {
    if let Some(dir) = portable_dir() {
        return dir;
    }
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("PortPilot")
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
    /// Folders the UI may open: project roots and worktrees Port Pilot itself found.
    pub known_paths: RwLock<HashSet<String>>,
    pub paused: AtomicBool,
    pub wake: Wake,
    /// Drop the WSL and Docker caches on the next refresh.
    pub invalidate: AtomicBool,
    /// UI language resolved by the frontend, for the tray menu.
    pub ui_language: RwLock<String>,
}

impl AppState {
    pub fn load() -> Self {
        let dir = data_dir();
        let (settings, error) = match Settings::load(&dir) {
            Ok(s) => (s, None),
            Err(e) => (Settings::default(), Some(e)),
        };
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
        }
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
    }

    #[test]
    fn path_keys() {
        assert_eq!(AppState::path_key(r"C:\Src\Shop\"), r"c:\src\shop");
        assert_eq!(AppState::path_key("C:/src/shop"), r"c:\src\shop");
    }
}
