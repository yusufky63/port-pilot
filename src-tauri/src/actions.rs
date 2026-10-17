//! Opening things for the user: a listener's URL, a project folder, a terminal or VS Code.
//! Callers pass only values PortPilot found itself; nothing here runs a command line taken
//! from the UI.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DETACHED_PROCESS: u32 = 0x0000_0008;

/// `\\wsl.localhost\Ubuntu\home\a\shop` → (`Ubuntu`, `/home/a/shop`).
pub fn split_wsl_unc(path: &str) -> Option<(String, String)> {
    let lower = path.to_ascii_lowercase();
    let rest = if lower.starts_with(r"\\wsl.localhost\") {
        &path[r"\\wsl.localhost\".len()..]
    } else if lower.starts_with(r"\\wsl$\") {
        &path[r"\\wsl$\".len()..]
    } else {
        return None;
    };
    let (distro, linux) = rest.split_once('\\').unwrap_or((rest, ""));
    if distro.is_empty() {
        return None;
    }
    Some((distro.to_string(), format!("/{}", linux.replace('\\', "/"))))
}

fn windows_terminal() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join(r"Microsoft\WindowsApps\wt.exe");
    p.exists().then_some(p)
}

pub fn open_url(url: &str) -> Result<(), String> {
    if !url.starts_with("http://") || url.chars().any(char::is_whitespace) {
        return Err("not a local URL".into());
    }
    portpilot_win::shell::open(url)
}

pub fn open_folder(path: &str) -> Result<(), String> {
    if !Path::new(path).is_dir() {
        return Err(format!("{path} does not exist"));
    }
    portpilot_win::shell::open(path)
}

/// A GUI program started on its own: no console, and none of PortPilot's standard handles, so
/// its logs never mix into PortPilot's output.
fn detached(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS);
    cmd
}

fn valid_distro(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

pub fn open_terminal(path: &str) -> Result<(), String> {
    if let Some((distro, linux)) = split_wsl_unc(path) {
        if !valid_distro(&distro) || linux.contains('"') {
            return Err(format!("{path} cannot be opened in a terminal"));
        }
        if let Some(wt) = windows_terminal()
            && detached(&wt)
                .args(["wsl.exe", "-d", &distro, "--cd", &linux])
                .spawn()
                .is_ok()
        {
            return Ok(());
        }
        return portpilot_win::shell::start_in(
            "wsl.exe",
            &format!("-d {distro} --cd \"{linux}\""),
            None,
        );
    }
    if !Path::new(path).is_dir() {
        return Err(format!("{path} does not exist"));
    }
    if let Some(wt) = windows_terminal()
        && detached(&wt).arg("-d").arg(path).spawn().is_ok()
    {
        return Ok(());
    }
    // The folder is the working directory, so nothing is quoted into a command line.
    portpilot_win::shell::start_in("powershell.exe", "-NoExit -NoLogo", Some(path))
}

fn vscode() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("LOCALAPPDATA")
            .map(|b| PathBuf::from(b).join(r"Programs\Microsoft VS Code\Code.exe")),
        std::env::var_os("ProgramFiles")
            .map(|b| PathBuf::from(b).join(r"Microsoft VS Code\Code.exe")),
        std::env::var_os("LOCALAPPDATA").map(|b| {
            PathBuf::from(b).join(r"Programs\Microsoft VS Code Insiders\Code - Insiders.exe")
        }),
    ];
    candidates.into_iter().flatten().find(|p| p.exists())
}

pub fn has_vscode() -> bool {
    vscode().is_some()
}

pub fn open_editor(path: &str) -> Result<(), String> {
    let code = vscode().ok_or("VS Code was not found")?;
    detached(&code)
        .arg(path)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_unc_paths() {
        assert_eq!(
            split_wsl_unc(r"\\wsl.localhost\Ubuntu\home\a\shop"),
            Some(("Ubuntu".into(), "/home/a/shop".into()))
        );
        assert_eq!(
            split_wsl_unc(r"\\wsl$\Debian"),
            Some(("Debian".into(), "/".into()))
        );
        assert_eq!(split_wsl_unc(r"C:\src\shop"), None);
    }

    #[test]
    fn only_local_urls() {
        assert!(open_url("file:///C:/Windows").is_err());
        assert!(open_url("http://localhost:3000 & calc").is_err());
    }
}
