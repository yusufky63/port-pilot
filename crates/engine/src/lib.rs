//! The engine behind both the `portctl` CLI and the desktop app: discovery, Git context, the
//! WSL and Docker adapters, diagnostics and the safe stop flow.

pub mod cmd;
pub mod discover;
pub mod docker;
pub mod doctor;
pub mod git;
pub mod projects;
pub mod stop;
pub mod wsl;

pub use discover::{Discovery, Options};
pub use stop::{StopRequest, stop};

/// Every Port Pilot executable calls this first. It returns an exit code when the process was
/// started as the console helper, which attaches to another console to deliver CTRL+C.
pub fn helper_mode(args: &[String]) -> Option<i32> {
    args.iter()
        .any(|a| a == portpilot_win::control::CONSOLE_HELPER_ARG)
        .then(|| portpilot_win::control::run_console_helper(args))
}
