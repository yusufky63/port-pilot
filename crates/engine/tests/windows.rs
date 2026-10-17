//! Integration tests against real sockets and real processes: the fixture server is started,
//! discovered, and stopped through every path of the stop flow.

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use portpilot_core::{Exposure, GracefulChannel, Runtime, StopBlock, StopOutcome, StopTarget};
use portpilot_engine::{Discovery, Options, StopRequest, stop};

const FIXTURE: &str = env!("CARGO_BIN_EXE_pp-fixture");
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct Fixture {
    child: Child,
    port: u16,
    grandchild: Option<(u32, u16)>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        if let Some((pid, _)) = self.grandchild {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output();
        }
    }
}

/// `CREATE_NO_WINDOW` gives the fixture a console of its own; without it the fixture shares
/// the test runner's console (or has none, on runners without one).
fn spawn(extra: &[&str], own_console: bool) -> Fixture {
    let mut child = Command::new(FIXTURE)
        .args(["tcp", "127.0.0.1", "0"])
        .args(extra)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .creation_flags(if own_console { CREATE_NO_WINDOW } else { 0 })
        .spawn()
        .expect("start fixture");
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let port = line
        .trim()
        .trim_start_matches("LISTENING ")
        .parse()
        .unwrap();
    let mut grandchild = None;
    if extra.contains(&"--child") {
        line.clear();
        reader.read_line(&mut line).unwrap();
        let parts: Vec<&str> = line.split_whitespace().collect();
        grandchild = Some((parts[1].parse().unwrap(), parts[2].parse().unwrap()));
    }
    Fixture {
        child,
        port,
        grandchild,
    }
}

fn local_only() -> Options {
    Options {
        wsl: false,
        docker: false,
        udp: false,
        ..Options::default()
    }
}

fn target_of(port: u16) -> (u32, u64) {
    let snap = Discovery::new(local_only()).snapshot();
    let l = snap
        .listeners
        .iter()
        .find(|l| l.port == port)
        .expect("fixture listener");
    match l.stop.target.clone() {
        Some(StopTarget::Windows { pid, start }) => (pid, start),
        other => panic!("unexpected stop target {other:?}"),
    }
}

fn running(pid: u32) -> bool {
    portpilot_win::process::is_running(pid)
}

#[test]
fn discovery_explains_the_fixture() {
    let f = spawn(&[], false);
    let snap = Discovery::new(local_only()).snapshot();
    assert_eq!(snap.schema_version, 1);
    let l = snap
        .listeners
        .iter()
        .find(|l| l.port == f.port)
        .expect("listener");
    assert_eq!(l.runtime, Runtime::Windows);
    assert_eq!(l.exposure, Exposure::Loopback);
    assert_eq!(
        l.url.as_deref(),
        Some(format!("http://localhost:{}", f.port).as_str())
    );
    let p = l.process.as_ref().unwrap();
    assert_eq!(p.pid, f.child.id());
    assert!(p.start.is_some() && p.started_at.is_some());
    assert!(
        p.command_line
            .as_deref()
            .unwrap_or_default()
            .contains("pp-fixture")
    );
    assert_eq!(l.ancestors.first().map(|a| a.pid), Some(std::process::id()));
    assert!(l.stop.blocked.is_none());
}

#[test]
fn own_listener_cannot_be_stopped() {
    let own = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = own.local_addr().unwrap().port();
    let snap = Discovery::new(local_only()).snapshot();
    let l = snap
        .listeners
        .iter()
        .find(|l| l.port == port)
        .expect("own listener");
    assert_eq!(l.stop.blocked, Some(StopBlock::SelfProcess));
    assert_eq!(l.stop.target, None);
}

#[test]
fn wrong_identity_is_refused() {
    let mut f = spawn(&[], true);
    let (pid, start) = target_of(f.port);
    let req = StopRequest {
        target: StopTarget::Windows {
            pid,
            start: start + 1,
        },
        force: true,
        tree: false,
        grace: Duration::from_millis(200),
    };
    assert_eq!(
        stop(&req, FIXTURE.as_ref()).outcome,
        StopOutcome::IdentityChanged
    );
    assert!(
        f.child.try_wait().unwrap().is_none(),
        "the process must survive"
    );
}

#[test]
fn ctrl_c_stops_a_server_in_its_own_console() {
    let mut f = spawn(&[], true);
    let (pid, start) = target_of(f.port);
    let req = StopRequest {
        target: StopTarget::Windows { pid, start },
        force: false,
        tree: false,
        grace: Duration::from_secs(5),
    };
    let r = stop(&req, FIXTURE.as_ref());
    assert_eq!(r.outcome, StopOutcome::Stopped, "{r:?}");
    // CTRL+BREAK follows when the runner (Git Bash, for one) passed down "ignore CTRL+C".
    assert!(matches!(
        r.channel,
        Some(GracefulChannel::ConsoleCtrlC | GracefulChannel::ConsoleCtrlBreak)
    ));
    assert!(f.child.wait_timeout_ok());
}

#[test]
fn ignoring_server_times_out_and_force_needs_consent() {
    let mut f = spawn(&["--ignore-ctrl-c"], true);
    let (pid, start) = target_of(f.port);
    let mut req = StopRequest {
        target: StopTarget::Windows { pid, start },
        force: false,
        tree: false,
        grace: Duration::from_millis(800),
    };
    let r = stop(&req, FIXTURE.as_ref());
    assert_eq!(r.outcome, StopOutcome::GracefulTimeout, "{r:?}");
    assert!(f.child.try_wait().unwrap().is_none());
    req.force = true;
    assert_eq!(stop(&req, FIXTURE.as_ref()).outcome, StopOutcome::Forced);
    assert!(f.child.wait_timeout_ok());
}

#[test]
fn shared_console_is_never_signalled() {
    let mut f = spawn(&[], false);
    let (pid, start) = target_of(f.port);
    let req = StopRequest {
        target: StopTarget::Windows { pid, start },
        force: false,
        tree: false,
        grace: Duration::from_millis(300),
    };
    let r = stop(&req, FIXTURE.as_ref());
    assert_eq!(r.outcome, StopOutcome::GracefulUnavailable, "{r:?}");
    assert!(r.graceful_skipped.is_some());
    assert!(
        f.child.try_wait().unwrap().is_none(),
        "the test runner's console must not get CTRL+C"
    );
}

#[test]
fn force_with_tree_stops_children() {
    let f = spawn(&["--child"], false);
    let (grandchild, _) = f.grandchild.unwrap();
    assert!(running(grandchild));
    let (pid, start) = target_of(f.port);
    let req = StopRequest {
        target: StopTarget::Windows { pid, start },
        force: true,
        tree: true,
        grace: Duration::from_millis(300),
    };
    let r = stop(&req, FIXTURE.as_ref());
    assert_eq!(r.outcome, StopOutcome::Forced, "{r:?}");
    assert_eq!(r.children_terminated, 1);
    assert!(!running(pid));
    assert!(!running(grandchild));
}

#[test]
fn full_snapshot_reports_adapters() {
    let snap = Discovery::new(Options::default()).snapshot();
    // Whatever is installed, both adapters say what they found.
    let _ = (&snap.adapters.wsl.status, &snap.adapters.docker.status);
    assert!(snap.listeners.iter().all(|l| !l.id.is_empty()));
}

trait WaitExt {
    fn wait_timeout_ok(&mut self) -> bool;
}

impl WaitExt for Child {
    /// Waits up to five seconds for the child to exit.
    fn wait_timeout_ok(&mut self) -> bool {
        for _ in 0..50 {
            if let Ok(Some(_)) = self.try_wait() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }
}
