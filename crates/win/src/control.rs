//! Process-control primitives. The engine composes them into the stop flow:
//! verify identity, graceful request, wait, and terminate only when force was allowed.
//!
//! Identity is checked on an open handle. While Port Pilot holds the handle the PID cannot be
//! reused, so the process that was verified is the process that gets signalled or terminated.

use std::collections::HashSet;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::System::Console::{
    AttachConsole, CTRL_BREAK_EVENT, CTRL_C_EVENT, FreeConsole, GenerateConsoleCtrlEvent,
    GetConsoleProcessList, SetConsoleCtrlHandler,
};
use windows::Win32::System::Threading::{
    DETACHED_PROCESS, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    WM_CLOSE,
};
use windows::core::{BOOL, Owned};

use crate::process::handle_start_time;

/// The argument that switches the Port Pilot executables into console-helper mode.
pub const CONSOLE_HELPER_ARG: &str = "--pp-console-ctrl";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// No process has this PID, or it has exited.
    NotRunning,
    AccessDenied,
    /// The PID belongs to a process with another creation time.
    IdentityChanged,
    Other(String),
}

/// A verified handle to the process that was shown to the user.
pub struct Target {
    handle: Owned<HANDLE>,
    pub pid: u32,
    pub can_terminate: bool,
}

fn open(pid: u32, rights: PROCESS_ACCESS_RIGHTS) -> Result<Owned<HANDLE>, OpenError> {
    match unsafe { OpenProcess(rights, false, pid) } {
        Ok(h) => Ok(unsafe { Owned::new(h) }),
        Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => Err(OpenError::AccessDenied),
        Err(e) if e.code() == ERROR_INVALID_PARAMETER.to_hresult() => Err(OpenError::NotRunning),
        Err(e) => Err(OpenError::Other(e.to_string())),
    }
}

impl Target {
    /// Opens `pid` and proves it is the instance created at `expected_start`.
    pub fn open(pid: u32, expected_start: u64) -> Result<Target, OpenError> {
        let base = PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION;
        let (handle, can_terminate) = match open(pid, base | PROCESS_TERMINATE) {
            Ok(h) => (h, true),
            Err(OpenError::AccessDenied) => (open(pid, base)?, false),
            Err(e) => return Err(e),
        };
        let start = handle_start_time(*handle).ok_or(OpenError::AccessDenied)?;
        if start != expected_start {
            return Err(OpenError::IdentityChanged);
        }
        let target = Target {
            handle,
            pid,
            can_terminate,
        };
        if target.has_exited() {
            return Err(OpenError::NotRunning);
        }
        Ok(target)
    }

    pub fn has_exited(&self) -> bool {
        let event = unsafe { WaitForSingleObject(*self.handle, 0) };
        event == WAIT_OBJECT_0
    }

    /// Waits up to `timeout`; returns whether the process exited.
    pub fn wait(&self, timeout: Duration) -> bool {
        let ms = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        let event = unsafe { WaitForSingleObject(*self.handle, ms) };
        event == WAIT_OBJECT_0
    }

    /// `TerminateProcess`: unconditional, skips the program's own shutdown code. Only after the
    /// graceful path and only when the user allowed force.
    pub fn terminate(&self) -> Result<(), String> {
        if !self.can_terminate {
            return Err("access denied".into());
        }
        unsafe { TerminateProcess(*self.handle, 1) }.map_err(|e| e.to_string())?;
        // Termination is asynchronous; give the kernel a moment to tear the process down.
        let _ = self.wait(Duration::from_secs(3));
        Ok(())
    }
}

struct WindowSearch {
    pid: u32,
    found: Vec<HWND>,
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = unsafe { &mut *(lparam.0 as *mut WindowSearch) };
    let mut owner_pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner_pid)) };
    let top_level = unsafe { GetWindow(hwnd, GW_OWNER) }.map_or(true, |h| h.is_invalid());
    if owner_pid == search.pid && top_level && unsafe { IsWindowVisible(hwnd) }.as_bool() {
        search.found.push(hwnd);
    }
    BOOL(1)
}

/// Visible top-level windows of `pid`.
pub fn top_level_windows(pid: u32) -> Vec<HWND> {
    let mut search = WindowSearch {
        pid,
        found: Vec::new(),
    };
    let _ = unsafe { EnumWindows(Some(collect_window), LPARAM(&mut search as *mut _ as isize)) };
    search.found
}

/// Asks a GUI process to close by posting WM_CLOSE to its top-level windows, the same message
/// the title-bar close button sends. Returns how many windows were asked.
pub fn close_windows(pid: u32) -> usize {
    let windows = top_level_windows(pid);
    windows
        .iter()
        .filter(|h| unsafe { PostMessageW(Some(**h), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok())
        .count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleCtrl {
    Sent,
    /// The target has no console, or it could not be attached.
    NoConsole,
    /// Other processes share the console; the signal would reach them too.
    Shared,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleSignal {
    CtrlC,
    /// CTRL+BREAK: delivered even when a process inherited "ignore CTRL+C" (programs started
    /// from MSYS shells such as Git Bash often have), and the default handler still exits the
    /// process normally.
    CtrlBreak,
}

/// Sends a console signal to the console of `pid` through a helper process, if every process
/// attached to that console is in `allowed` (the target's own tree and the shell that runs it).
///
/// Console signals can only be generated for a whole console, and only from a process attached
/// to it, so a short-lived copy of `exe` attaches to the target's console and sends it there.
pub fn console_signal(
    exe: &Path,
    pid: u32,
    allowed: &HashSet<u32>,
    signal: ConsoleSignal,
) -> ConsoleCtrl {
    let list: Vec<String> = allowed.iter().map(u32::to_string).collect();
    let child = Command::new(exe)
        .arg(CONSOLE_HELPER_ARG)
        .arg(pid.to_string())
        .arg(list.join(","))
        .arg(match signal {
            ConsoleSignal::CtrlC => "c",
            ConsoleSignal::CtrlBreak => "break",
        })
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS.0)
        .spawn();
    let Ok(mut child) = child else {
        return ConsoleCtrl::Failed;
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(0) => ConsoleCtrl::Sent,
                    Some(3) => ConsoleCtrl::NoConsole,
                    Some(4) => ConsoleCtrl::Shared,
                    _ => ConsoleCtrl::Failed,
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                return ConsoleCtrl::Failed;
            }
        }
    }
}

unsafe extern "system" fn swallow_signal(_ctrl_type: u32) -> BOOL {
    BOOL(1)
}

/// Makes the current process swallow CTRL+C and CTRL+BREAK. The test fixture uses it to play
/// a server that does not react to graceful requests.
pub fn ignore_console_signals() {
    let _ = unsafe { SetConsoleCtrlHandler(Some(swallow_signal), true) };
}

/// Helper-mode entry point: `<exe> --pp-console-ctrl <pid> <allowed,pids> [c|break]`. Exit
/// codes: 0 sent, 2 bad arguments, 3 no console, 4 shared console, 5 send failed.
pub fn run_console_helper(args: &[String]) -> i32 {
    let Some(pos) = args.iter().position(|a| a == CONSOLE_HELPER_ARG) else {
        return 2;
    };
    let Some(pid) = args.get(pos + 1).and_then(|p| p.parse::<u32>().ok()) else {
        return 2;
    };
    let allowed: HashSet<u32> = args
        .get(pos + 2)
        .map(|s| s.split(',').filter_map(|p| p.parse().ok()).collect())
        .unwrap_or_default();
    let event = match args.get(pos + 3).map(String::as_str) {
        Some("break") => CTRL_BREAK_EVENT,
        _ => CTRL_C_EVENT,
    };
    unsafe {
        let _ = FreeConsole();
        if AttachConsole(pid).is_err() {
            return 3;
        }
        let mut list = vec![0u32; 512];
        let n = GetConsoleProcessList(&mut list) as usize;
        if n == 0 || n > list.len() {
            let _ = FreeConsole();
            return 3;
        }
        let me = std::process::id();
        if list[..n].iter().any(|p| *p != me && !allowed.contains(p)) {
            let _ = FreeConsole();
            return 4;
        }
        // The helper is attached now and would receive the signal too.
        let _ = SetConsoleCtrlHandler(Some(swallow_signal), true);
        let sent = GenerateConsoleCtrlEvent(event, 0).is_ok();
        // Delivery is asynchronous; stay attached until the console has dispatched it.
        std::thread::sleep(Duration::from_millis(150));
        let _ = FreeConsole();
        if sent { 0 } else { 5 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::start_time;

    #[test]
    fn identity_mismatch_is_refused() {
        let me = std::process::id();
        let start = start_time(me).unwrap();
        assert!(Target::open(me, start).is_ok());
        assert_eq!(
            Target::open(me, start + 1).err(),
            Some(OpenError::IdentityChanged)
        );
    }

    #[test]
    fn missing_process_is_not_running() {
        // PIDs are multiples of 4; an odd one never exists.
        assert_eq!(Target::open(3, 1).err(), Some(OpenError::NotRunning));
    }

    #[test]
    fn helper_rejects_bad_arguments() {
        assert_eq!(run_console_helper(&[]), 2);
        assert_eq!(
            run_console_helper(&[CONSOLE_HELPER_ARG.into(), "x".into()]),
            2
        );
    }
}
