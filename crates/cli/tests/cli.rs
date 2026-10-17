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
