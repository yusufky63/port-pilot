//! Platform-independent part of Port Pilot: the snapshot model, parsers for the tools it reads
//! (Git, `ss`, `wsl.exe`, the Docker Engine API, `netsh`), and the rules that turn raw sockets
//! into explained listeners. Nothing here touches the operating system, so all of it is unit
//! tested on any machine.

pub mod assemble;
pub mod cmdline;
pub mod docker;
pub mod git;
pub mod http;
pub mod infer;
pub mod model;
pub mod net;
pub mod redact;
pub mod time;
pub mod wsl;

pub use model::*;
