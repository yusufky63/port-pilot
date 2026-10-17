//! Test fixture: a tiny server for the integration tests.
//!
//! `pp-fixture tcp <addr> <port> [--child] [--ignore-ctrl-c]` binds a TCP listener, prints
//! `LISTENING <port>`, optionally starts a child fixture (`CHILD <pid> <port>`), then waits to
//! be stopped. `--ignore-ctrl-c` makes it swallow CTRL+C and CTRL+BREAK.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = portpilot_engine::helper_mode(&args) {
        std::process::exit(code);
    }
    if args.iter().any(|a| a == "--ignore-ctrl-c") {
        portpilot_win::control::ignore_console_signals();
    }
    let addr = args.get(2).cloned().unwrap_or_else(|| "127.0.0.1".into());
    let port: u16 = args.get(3).and_then(|p| p.parse().ok()).unwrap_or(0);
    let listener = TcpListener::bind((addr.as_str(), port)).expect("bind");
    let mut out = std::io::stdout();
    writeln!(
        out,
        "LISTENING {}",
        listener.local_addr().expect("addr").port()
    )
    .ok();
    out.flush().ok();

    // Kept alive for as long as this process runs.
    let mut _child = None;
    if args.iter().any(|a| a == "--child") {
        let mut child = Command::new(std::env::current_exe().expect("exe"))
            .args(["tcp", "127.0.0.1", "0"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn child");
        let mut line = String::new();
        BufReader::new(child.stdout.take().expect("stdout"))
            .read_line(&mut line)
            .ok();
        let child_port = line.trim().trim_start_matches("LISTENING ").to_string();
        writeln!(out, "CHILD {} {child_port}", child.id()).ok();
        out.flush().ok();
        _child = Some(child);
    }
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
