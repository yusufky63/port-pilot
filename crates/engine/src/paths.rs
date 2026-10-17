//! Where PortPilot keeps its files. The desktop app and `portctl` use the same folder, so
//! leases, trusted commands and history are shared between them.

use std::path::PathBuf;

/// Overrides the data folder. The tests use it so they never touch the user's own data.
pub const DATA_DIR_ENV: &str = "PORTPILOT_DATA_DIR";

/// `<exe folder>\data` when a `portable` marker file sits next to the exe.
pub fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join("portable").is_file().then(|| dir.join("data"))
}

/// `%PORTPILOT_DATA_DIR%`, the portable folder, or `%APPDATA%\PortPilot`.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(dir) = portable_dir() {
        return dir;
    }
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("PortPilot")
}

/// The SQLite file with leases, trusted commands and history.
pub fn database_path() -> PathBuf {
    data_dir().join("portpilot.db")
}
