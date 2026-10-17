//! The exit-code and JSON contract of `portctl`.

use std::net::TcpListener;
use std::process::Command;

const PORTCTL: &str = env!("CARGO_BIN_EXE_portctl");

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(PORTCTL)
        .args(args)
        .output()
        .expect("run portctl");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn list_json_is_versioned() {
    let (code, out) = run(&["list", "--json", "--no-wsl", "--no-docker"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schemaVersion"], 1);
    assert!(v["listeners"].is_array());
}

#[test]
fn who_explains_a_listener_of_this_test() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port().to_string();
    let (code, out) = run(&["who", &port, "--json", "--no-wsl", "--no-docker"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["verdict"], "inUse");
    assert_eq!(v["listeners"][0]["process"]["pid"], std::process::id());
    // The test process runs portctl, so portctl refuses to stop it.
    assert_eq!(v["listeners"][0]["stop"]["blocked"], "selfProcess");
}

#[test]
fn stop_without_listener_exits_3() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let (code, _) = run(&["stop", &port.to_string(), "--no-wsl", "--no-docker"]);
    assert_eq!(code, 3);
}

#[test]
fn stop_refuses_own_lineage_with_5() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port().to_string();
    let (code, _) = run(&["stop", &port, "--no-wsl", "--no-docker"]);
    assert_eq!(code, 5);
}

#[test]
fn find_returns_bindable_ports() {
    let (code, out) = run(&["find", "--range", "40000-40100", "--count", "2", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let ports = v["ports"].as_array().unwrap();
    assert_eq!(ports.len(), 2);
    for p in ports {
        let p = p.as_u64().unwrap() as u16;
        assert!(TcpListener::bind(("127.0.0.1", p)).is_ok());
    }
}

#[test]
fn invalid_arguments_exit_2() {
    assert_eq!(run(&["find", "--range", "9-1"]).0, 2);
    assert_eq!(run(&["stop"]).0, 2);
}

/// A project folder with a recipe and a data folder of its own, so the tests never touch the
/// user's leases or history.
struct Project {
    root: std::path::PathBuf,
}

impl Project {
    fn new(name: &str, recipe: &str) -> Project {
        let root = std::env::temp_dir().join(format!("pp-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app").join(".git")).unwrap();
        std::fs::write(root.join("app").join(".portpilot.toml"), recipe).unwrap();
        Project { root }
    }

    fn run(&self, args: &[&str]) -> (i32, String) {
        let out = Command::new(PORTCTL)
            .args(args)
            .current_dir(self.root.join("app"))
            .env("PORTPILOT_DATA_DIR", self.root.join("data"))
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run portctl");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Five consecutive ports nothing listens on, in the block of test `slot`. A loopback bind
/// alone is not enough: Windows lets a program bind 127.0.0.1 while another one listens on
/// 0.0.0.0 or [::] with the same port, and PortPilot rightly counts that listener as a user.
fn free_base(slot: u16) -> u16 {
    let used: std::collections::HashSet<u16> = portpilot_win::net::tcp_listeners()
        .unwrap()
        .iter()
        .map(|s| s.port)
        .collect();
    // Each test searches its own block, so parallel tests never pick the same ports.
    let start = 43000 + slot * 5000;
    (start..start + 5000)
        .step_by(61)
        .find(|b| {
            (*b..*b + 5).all(|p| !used.contains(&p) && TcpListener::bind(("127.0.0.1", p)).is_ok())
        })
        .expect("free range")
}

#[test]
fn lease_acquire_list_release() {
    let base = free_base(0);
    let p = Project::new(
        "lease",
        &format!("[ports.web]\nrange = \"{base}-{}\"\n", base + 4),
    );
    let (code, out) = p.run(&["lease", "acquire", "web"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), base.to_string());
    // Stable on a second call, moved with --next.
    assert_eq!(
        p.run(&["lease", "acquire", "web"]).1.trim(),
        base.to_string()
    );
    let (code, out) = p.run(&["lease", "acquire", "web", "--next", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schemaVersion"], 1);
    assert_eq!(v["port"], base + 1);
    assert_eq!(v["service"], "web");
    let (_, out) = p.run(&["lease", "list", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["leases"].as_array().unwrap().len(), 1);
    assert_eq!(p.run(&["lease", "release", "web"]).0, 0);
    assert_eq!(p.run(&["lease", "release", "web"]).0, 3);
}

#[test]
fn lease_on_a_busy_port_exits_9_and_unknown_service_11() {
    let base = free_base(1);
    let p = Project::new("conflict", &format!("[ports.web]\npreferred = {base}\n"));
    let (code, _) = p.run(&["lease", "acquire", "web"]);
    assert_eq!(code, 0);
    let busy = TcpListener::bind(("127.0.0.1", base)).unwrap();
    let (code, out) = p.run(&["lease", "acquire", "web"]);
    assert_eq!(code, 9, "the kept lease is in use by another program");
    assert_eq!(out.trim(), base.to_string());
    drop(busy);
    assert_eq!(p.run(&["lease", "acquire", "api"]).0, 11);
    assert_eq!(p.run(&["lease", "acquire", "api", "--range", "9-1"]).0, 2);
}

#[test]
fn run_needs_confirmation_and_passes_the_exit_code() {
    let base = free_base(2);
    let p = Project::new(
        "run",
        &format!(
            "[ports.web]\npreferred = {base}\n\n[run.dev]\ncommand = \"echo port {{ports.web}}& exit 4\"\n"
        ),
    );
    // Nobody can answer the trust prompt, so nothing runs.
    let (code, out) = p.run(&["run", "dev"]);
    assert_eq!(code, 2);
    assert!(out.contains(&format!("echo port {base}& exit 4")), "{out}");
    let (code, out) = p.run(&["run", "--yes"]);
    assert_eq!(code, 4, "{out}");
    assert!(out.contains(&format!("port {base}")), "{out}");
    let (_, out) = p.run(&["history", "--kind", "runs", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let events = v["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "runExited");
    assert_eq!(events[0]["exitCode"], 4);
    assert!(events.iter().all(|e| e["command"].is_null()));
    assert_eq!(p.run(&["run", "nope", "--yes"]).0, 11);
}

#[test]
fn run_without_recipe_exits_11() {
    let p = Project::new("none", "");
    std::fs::remove_file(p.root.join("app").join(".portpilot.toml")).unwrap();
    assert_eq!(p.run(&["run", "--yes"]).0, 11);
}
