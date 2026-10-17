//! The engine behind both the `portctl` CLI and the desktop app: discovery, Git context, the
//! WSL and Docker adapters, diagnostics, the safe stop flow, managed runs, port leases and the
//! local history database.

pub mod cmd;
pub mod discover;
pub mod docker;
pub mod doctor;
pub mod git;
pub mod history;
pub mod leases;
pub mod paths;
pub mod projects;
pub mod recipes;
pub mod runs;
pub mod stop;
pub mod store;
pub mod wsl;

pub use discover::{Discovery, ManagedRun, Options};
pub use stop::{StopRequest, stop};

/// Every PortPilot executable calls this first. It returns an exit code when the process was
/// started as the console helper, which attaches to another console to deliver CTRL+C.
pub fn helper_mode(args: &[String]) -> Option<i32> {
    args.iter()
        .any(|a| a == portpilot_win::control::CONSOLE_HELPER_ARG)
        .then(|| portpilot_win::control::run_console_helper(args))
}
