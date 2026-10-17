//! The Windows-only part of Port Pilot. Every unsafe call lives here behind small safe
//! functions: IP Helper socket tables, ToolHelp and process handles for identity, WMI for
//! command lines, and the process-control primitives the engine composes into a safe stop.
//!
//! The app runs as a normal user. Processes of other users and elevated processes report
//! `access_denied` instead of failing the scan.

pub mod control;
pub mod net;
pub mod os;
pub mod process;
pub mod shell;
pub mod time;
pub mod wide;
pub mod wmi;
