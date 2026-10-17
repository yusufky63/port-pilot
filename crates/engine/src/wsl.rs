//! WSL through `wsl.exe`. Only distros that are already running are inspected; starting a
//! distro just to look at it would change the machine.

use std::path::PathBuf;
use std::time::Duration;

use portpilot_core::wsl::{
    PROBE_SCRIPT, Probe, decode_output, parse_distro_names, parse_probe, signal_script,
};
use portpilot_core::{AdapterState, AdapterStatus};

use crate::cmd::{self, Output, Spec};

const LIST_TIMEOUT: Duration = Duration::from_secs(6);
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Asks newer `wsl.exe` builds for UTF-8 instead of UTF-16.
const ENV: &[(&str, &str)] = &[("WSL_UTF8", "1")];

fn wsl_exe() -> Option<PathBuf> {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let path = PathBuf::from(root).join(r"System32\wsl.exe");
    path.exists().then_some(path)
}

fn run(args: &[&str], stdin: Option<&[u8]>, timeout: Duration) -> Result<Output, String> {
    let exe = wsl_exe().ok_or("wsl.exe not found")?;
    let exe = exe.to_string_lossy().into_owned();
    cmd::run(&Spec {
        stdin,
        env: ENV,
        ..Spec::new(&exe, args, timeout)
    })
}

fn first_line(bytes: &[u8]) -> Option<String> {
    decode_output(bytes)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

/// Running distros, without Docker Desktop's internal ones.
pub fn running_distros() -> Result<Vec<String>, AdapterState> {
    if wsl_exe().is_none() {
        return Err(AdapterState::new(AdapterStatus::NotInstalled));
    }
    let out = run(&["--list", "--running", "--quiet"], None, LIST_TIMEOUT)
        .map_err(|e| AdapterState::with_detail(AdapterStatus::Error, e))?;
    let names = parse_distro_names(&decode_output(&out.stdout));
    if out.ok() || !names.is_empty() {
        return Ok(names);
    }
    // wsl.exe exits non-zero with a (localized) message when nothing runs or WSL is missing.
    let detail = first_line(&out.stdout).or_else(|| first_line(&out.stderr));
    Err(AdapterState {
        status: AdapterStatus::NotRunning,
        detail,
        items: Vec::new(),
    })
}

/// Listening sockets and their processes inside one running distro.
pub fn probe(distro: &str) -> Result<Probe, String> {
    let out = run(
        &["-d", distro, "--exec", "sh", "-s"],
        Some(PROBE_SCRIPT.as_bytes()),
        PROBE_TIMEOUT,
    )?;
    if !out.ok() && out.stdout.is_empty() {
        return Err(first_line(&out.stderr)
            .unwrap_or_else(|| format!("wsl.exe exited with {:?}", out.code)));
    }
    Ok(parse_probe(&String::from_utf8_lossy(&out.stdout)))
}

/// What the signal script reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalResult {
    Gone,
    Changed,
    Denied,
    Exited,
    Alive,
    Unreachable,
}

/// Sends SIGTERM or SIGKILL after checking the start time, then waits up to `wait`.
pub fn signal(distro: &str, pid: u32, start: u64, kill: bool, wait: Duration) -> SignalResult {
    let tenths = (wait.as_millis() / 100).clamp(1, 600) as u32;
    let script = signal_script(pid, start, if kill { "KILL" } else { "TERM" }, tenths);
    let Ok(out) = run(
        &["-d", distro, "--exec", "sh", "-s"],
        Some(script.as_bytes()),
        wait + PROBE_TIMEOUT,
    ) else {
        return SignalResult::Unreachable;
    };
    match String::from_utf8_lossy(&out.stdout).trim() {
        "gone" => SignalResult::Gone,
        "changed" => SignalResult::Changed,
        "denied" => SignalResult::Denied,
        "exited" => SignalResult::Exited,
        "alive" => SignalResult::Alive,
        _ => SignalResult::Unreachable,
    }
}

/// `wsl.exe --version` first line, for diagnostics.
pub fn version() -> Option<String> {
    let out = run(&["--version"], None, LIST_TIMEOUT).ok()?;
    if !out.ok() {
        return None;
    }
    first_line(&out.stdout)
}
