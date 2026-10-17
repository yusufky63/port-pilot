//! The stop flow. Graceful first, force only when the caller allowed it, and the identity
//! check before anything is sent:
//!
//! 1. Open the process and compare its creation time with the one the user saw.
//! 2. Refuse system processes, services, WSL/Docker forwarders and PortPilot's own lineage.
//! 3. Graceful request: WM_CLOSE for windowed programs, otherwise CTRL+C on the target's
//!    console, but only when nothing outside the target's own tree and shell shares it.
//! 4. Wait for the grace period.
//! 5. Terminate only if force was allowed; children too when asked.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use portpilot_core::infer::{classify_process, is_runner, is_shell};
use portpilot_core::{
    GracefulChannel, GracefulSkip, ProcessClass, StopOutcome, StopReport, StopTarget,
};
use portpilot_win::control::{self, ConsoleCtrl, ConsoleSignal, OpenError, Target};
use portpilot_win::process::{self, ProcEntry};
use portpilot_win::wmi;

use crate::{docker, wsl};

#[derive(Debug, Clone)]
pub struct StopRequest {
    pub target: StopTarget,
    /// Terminate after the grace period if the process is still running.
    pub force: bool,
    /// With force: terminate the target's child processes too.
    pub tree: bool,
    pub grace: Duration,
}

fn report(outcome: StopOutcome, started: Instant) -> StopReport {
    StopReport {
        outcome,
        channel: None,
        graceful_skipped: None,
        detail: None,
        children_terminated: 0,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

/// Children of `pid`, recursively, each with its creation time. A child that is older than its
/// parent holds a reused PID and is skipped.
fn descendants(pid: u32, parent_start: u64, procs: &HashMap<u32, ProcEntry>) -> Vec<(u32, u64)> {
    let mut out = Vec::new();
    let mut queue = vec![(pid, parent_start)];
    while let Some((parent, pstart)) = queue.pop() {
        for p in procs
            .values()
            .filter(|p| p.ppid == parent && p.pid != parent)
        {
            if let Some(start) = process::start_time(p.pid)
                && start >= pstart
                && !out.iter().any(|(q, _)| *q == p.pid)
            {
                out.push((p.pid, start));
                queue.push((p.pid, start));
            }
        }
        if out.len() > 256 {
            break;
        }
    }
    out
}

/// Processes that may receive CTRL+C together with the target: its own tree, then upwards the
/// package runners that started it (`pnpm dev`) and the shell that runs them. An editor, an
/// agent or any other program sharing the console is not in this set, so the signal is not
/// sent at all in that case.
fn console_allowed(pid: u32, start: u64, procs: &HashMap<u32, ProcEntry>) -> HashSet<u32> {
    let mut allowed: HashSet<u32> = descendants(pid, start, procs)
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    allowed.insert(pid);
    let mut chain = Vec::new();
    let (mut child, mut child_start) = (pid, start);
    while chain.len() < 8 {
        let Some(ppid) = procs.get(&child).map(|p| p.ppid) else {
            break;
        };
        let Some(parent) = procs.get(&ppid) else {
            break;
        };
        let Some(pstart) = process::start_time(ppid) else {
            break;
        };
        if pstart > child_start {
            break;
        }
        chain.push(parent.clone());
        child = ppid;
        child_start = pstart;
    }
    let cmdlines =
        wmi::processes(&chain.iter().map(|p| p.pid).collect::<Vec<_>>()).unwrap_or_default();
    for p in chain {
        let cmd = cmdlines.get(&p.pid).and_then(|w| w.command_line.as_deref());
        if is_shell(&p.name) {
            allowed.insert(p.pid);
            break;
        }
        if !is_runner(&p.name, cmd) {
            break;
        }
        allowed.insert(p.pid);
    }
    allowed
}

fn stop_windows(
    pid: u32,
    start: u64,
    req: &StopRequest,
    helper_exe: &Path,
    started: Instant,
) -> StopReport {
    let procs = process::snapshot().unwrap_or_default();
    if process::own_lineage(&procs).contains(&pid) {
        let mut r = report(StopOutcome::Refused, started);
        r.detail = Some("PortPilot does not stop itself or the program that runs it".into());
        return r;
    }
    let target = match Target::open(pid, start) {
        Ok(t) => t,
        Err(OpenError::NotRunning) => return report(StopOutcome::AlreadyExited, started),
        Err(OpenError::IdentityChanged) => return report(StopOutcome::IdentityChanged, started),
        Err(OpenError::AccessDenied) => return report(StopOutcome::AccessDenied, started),
        Err(OpenError::Other(e)) => {
            let mut r = report(StopOutcome::Failed, started);
            r.detail = Some(e);
            return r;
        }
    };
    let name = procs.get(&pid).map(|p| p.name.clone()).unwrap_or_default();
    let user = process::detail(pid).user;
    if classify_process(pid, &name, user.as_deref()) != ProcessClass::User {
        let mut r = report(StopOutcome::Refused, started);
        r.detail = Some(format!("{name} is a system, service or forwarder process"));
        return r;
    }

    let mut channel = None;
    let mut skipped = None;
    let mut exited = false;
    if control::close_windows(pid) > 0 {
        channel = Some(GracefulChannel::CloseWindow);
        exited = target.wait(req.grace);
    } else {
        let allowed = console_allowed(pid, start, &procs);
        match control::console_signal(helper_exe, pid, &allowed, ConsoleSignal::CtrlC) {
            ConsoleCtrl::Sent => {
                channel = Some(GracefulChannel::ConsoleCtrlC);
                // Half the grace period for CTRL+C. Processes started from MSYS shells inherit
                // "ignore CTRL+C", so CTRL+BREAK follows on the same console.
                let first = req.grace / 2;
                exited = target.wait(first);
                if !exited
                    && control::console_signal(helper_exe, pid, &allowed, ConsoleSignal::CtrlBreak)
                        == ConsoleCtrl::Sent
                {
                    channel = Some(GracefulChannel::ConsoleCtrlBreak);
                    exited = target.wait(req.grace.saturating_sub(first));
                }
            }
            ConsoleCtrl::NoConsole => skipped = Some(GracefulSkip::NoConsole),
            ConsoleCtrl::Shared => skipped = Some(GracefulSkip::SharedConsole),
            ConsoleCtrl::Failed => skipped = Some(GracefulSkip::SignalFailed),
        }
    }

    if exited || target.has_exited() {
        let mut r = report(StopOutcome::Stopped, started);
        r.channel = channel;
        return r;
    }
    if !req.force {
        let mut r = report(
            if channel.is_some() {
                StopOutcome::GracefulTimeout
            } else {
                StopOutcome::GracefulUnavailable
            },
            started,
        );
        r.channel = channel;
        r.graceful_skipped = skipped;
        return r;
    }

    // Children are collected before the parent goes away and its PID could be reused.
    let children = if req.tree {
        descendants(pid, start, &procs)
    } else {
        Vec::new()
    };
    if let Err(e) = target.terminate() {
        let mut r = report(StopOutcome::AccessDenied, started);
        r.channel = channel;
        r.detail = Some(e);
        return r;
    }
    let mut terminated = 0;
    for (child, child_start) in children {
        if let Ok(t) = Target::open(child, child_start)
            && t.terminate().is_ok()
        {
            terminated += 1;
        }
    }
    let mut r = report(StopOutcome::Forced, started);
    r.channel = channel;
    r.graceful_skipped = skipped;
    r.children_terminated = terminated;
    r
}

fn stop_wsl(distro: &str, pid: u32, start: u64, req: &StopRequest, started: Instant) -> StopReport {
    let first = wsl::signal(distro, pid, start, false, req.grace);
    let outcome = match first {
        wsl::SignalResult::Gone => StopOutcome::AlreadyExited,
        wsl::SignalResult::Changed => StopOutcome::IdentityChanged,
        wsl::SignalResult::Denied => StopOutcome::AccessDenied,
        wsl::SignalResult::Exited => StopOutcome::Stopped,
        wsl::SignalResult::Unreachable => StopOutcome::RuntimeUnavailable,
        wsl::SignalResult::Alive if !req.force => StopOutcome::GracefulTimeout,
        wsl::SignalResult::Alive => {
            match wsl::signal(distro, pid, start, true, Duration::from_secs(3)) {
                wsl::SignalResult::Exited | wsl::SignalResult::Gone => StopOutcome::Forced,
                wsl::SignalResult::Changed => StopOutcome::IdentityChanged,
                wsl::SignalResult::Denied => StopOutcome::AccessDenied,
                wsl::SignalResult::Unreachable => StopOutcome::RuntimeUnavailable,
                wsl::SignalResult::Alive => StopOutcome::Failed,
            }
        }
    };
    let mut r = report(outcome, started);
    if !matches!(
        first,
        wsl::SignalResult::Gone | wsl::SignalResult::Changed | wsl::SignalResult::Unreachable
    ) {
        r.channel = Some(GracefulChannel::Sigterm);
    }
    r
}

fn stop_docker(id: &str, req: &StopRequest, started: Instant) -> StopReport {
    let result = docker::stop(id, req.grace);
    let outcome = match result {
        docker::StopResult::Stopped => StopOutcome::Stopped,
        docker::StopResult::AlreadyStopped => StopOutcome::AlreadyExited,
        docker::StopResult::Unreachable => StopOutcome::RuntimeUnavailable,
        docker::StopResult::Failed(_) => StopOutcome::Failed,
    };
    let mut r = report(outcome, started);
    r.channel = Some(GracefulChannel::DockerStop);
    if let docker::StopResult::Failed(code) = result {
        r.detail = Some(format!("Docker answered HTTP {code}"));
    }
    r
}

/// Runs the stop flow. `helper_exe` is a PortPilot executable that handles the console-helper
/// argument (the CLI and the desktop app both do).
pub fn stop(req: &StopRequest, helper_exe: &Path) -> StopReport {
    let started = Instant::now();
    match &req.target {
        StopTarget::Windows { pid, start } => stop_windows(*pid, *start, req, helper_exe, started),
        StopTarget::Wsl { distro, pid, start } => stop_wsl(distro, *pid, *start, req, started),
        StopTarget::Docker { container_id } => stop_docker(container_id, req, started),
    }
}
