//! Managed runs against real processes: the fixture server is started inside a Job Object,
//! attributed exactly, and stopped through its process group or, with consent, its job.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portpilot_core::lease::Want;
use portpilot_core::recipe::RECIPE_FILE;
use portpilot_core::{AttributionSource, Confidence, GracefulChannel, StopOutcome};
use portpilot_engine::leases::{self, PortCheck};
use portpilot_engine::recipes::{self, display_path};
use portpilot_engine::runs::{RunManager, RunSpec, RunState};
use portpilot_engine::store::{EventKind, EventQuery, HistoryPrefs, RunPort, Store};
use portpilot_engine::{Discovery, Options};

const FIXTURE: &str = env!("CARGO_BIN_EXE_pp-fixture");

fn local_only() -> Options {
    Options {
        wsl: false,
        docker: false,
        udp: false,
        ..Options::default()
    }
}

fn temp_project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pp-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("package.json"), "{}").unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

fn spec(dir: &Path, command: String) -> RunSpec {
    RunSpec {
        name: "dev".into(),
        project: "fixture".into(),
        folder: display_path(dir),
        command,
        env: Vec::new(),
        ports: Vec::new(),
    }
}

fn fixture_command(args: &str) -> String {
    format!("\"{FIXTURE}\" tcp 127.0.0.1 {args}")
}

/// Waits for a line that starts with `prefix` in the run's output and returns the rest.
fn wait_line(m: &RunManager, id: &str, prefix: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(chunk) = m.output(id, 0, 1000)
            && let Some(l) = chunk.lines.iter().find(|l| l.text.starts_with(prefix))
        {
            return l.text[prefix.len()..].trim().to_string();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "no {prefix} line; output: {:?}",
        m.output_text(id).unwrap_or_default()
    );
}

fn running(pid: u32) -> bool {
    portpilot_win::process::is_running(pid)
}

#[test]
fn managed_run_is_attributed_exactly_and_stops_with_ctrl_break() {
    let dir = temp_project("graceful");
    let m = RunManager::new(FIXTURE.into(), None, HistoryPrefs::default());
    let info = m.start(spec(&dir, fixture_command("0"))).unwrap();
    assert_eq!(info.state, RunState::Running);
    let port: u16 = wait_line(&m, &info.id, "LISTENING ").parse().unwrap();

    let mut d = Discovery::new(local_only());
    d.set_managed(m.managed());
    let snap = d.snapshot();
    let l = snap
        .listeners
        .iter()
        .find(|l| l.port == port)
        .expect("managed listener");
    assert_eq!(
        l.managed.as_ref().map(|r| r.run_id.as_str()),
        Some(info.id.as_str())
    );
    let p = l.project.as_ref().expect("project");
    assert_eq!(p.confidence, Confidence::Exact);
    assert_eq!(p.source, AttributionSource::Launched);
    assert!(portpilot_core::git::same_path(&p.root, &info.folder));
    assert!(p.manifests.contains(&"package.json".to_string()));
    // The fixture runs under cmd.exe, the run's first process.
    assert_eq!(l.ancestors.first().map(|a| a.pid), Some(info.pid));

    let r = m.stop(&info.id, Duration::from_secs(5), false).unwrap();
    assert_eq!(r.outcome, StopOutcome::Stopped, "{r:?}");
    assert_eq!(r.channel, Some(GracefulChannel::GroupCtrlBreak));
    let after = m.info(&info.id, false).unwrap();
    assert_eq!(after.state, RunState::Exited);
    assert_eq!(after.ended_by.as_deref(), Some("stopped"));
    assert_eq!(after.processes, 0);
    assert!(!running(info.pid));
    assert!(m.forget(&info.id));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ignoring_run_is_ended_through_its_job_only_with_consent() {
    let dir = temp_project("force");
    let m = RunManager::new(FIXTURE.into(), None, HistoryPrefs::default());
    let info = m
        .start(spec(&dir, fixture_command("0 --child --ignore-ctrl-c")))
        .unwrap();
    wait_line(&m, &info.id, "LISTENING ");
    let child: u32 = wait_line(&m, &info.id, "CHILD ")
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(running(child));

    let r = m.stop(&info.id, Duration::from_millis(800), false).unwrap();
    assert_eq!(r.outcome, StopOutcome::GracefulTimeout, "{r:?}");
    assert_eq!(m.info(&info.id, false).unwrap().state, RunState::Running);
    assert!(running(info.pid), "nothing is ended without consent");

    // The graceful request just went out, so consent ends the job at once.
    let started = Instant::now();
    let r = m.stop(&info.id, Duration::from_secs(5), true).unwrap();
    assert_eq!(r.outcome, StopOutcome::Forced, "{r:?}");
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(r.children_terminated >= 1);
    assert!(!running(info.pid));
    assert!(!running(child), "the whole job ends, grandchildren too");
    assert_eq!(
        m.info(&info.id, false).unwrap().ended_by.as_deref(),
        Some("terminated")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_batch_file_run_stops_without_waiting_for_its_prompt() {
    // pnpm, npm and yarn are .cmd shims: cmd.exe asks "Terminate batch job (Y/N)?" after
    // CTRL+BREAK, and the answer must not be needed.
    let dir = temp_project("batch");
    std::fs::write(
        dir.join("serve.cmd"),
        format!("@echo off\r\n\"{FIXTURE}\" tcp 127.0.0.1 0\r\n"),
    )
    .unwrap();
    let m = RunManager::new(FIXTURE.into(), None, HistoryPrefs::default());
    let info = m.start(spec(&dir, r".\serve.cmd".into())).unwrap();
    wait_line(&m, &info.id, "LISTENING ");
    let r = m.stop(&info.id, Duration::from_secs(5), false).unwrap();
    assert_eq!(r.outcome, StopOutcome::Stopped, "{r:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_records_the_run_without_its_command_line() {
    let dir = temp_project("history");
    let store = Arc::new(Mutex::new(Store::in_memory().unwrap()));
    let m = RunManager::new(FIXTURE.into(), Some(store.clone()), HistoryPrefs::default());
    let mut s = spec(&dir, fixture_command("0 --token=secret-value"));
    s.ports = vec![RunPort {
        service: "web".into(),
        port: 5173,
    }];
    let info = m.start(s).unwrap();
    wait_line(&m, &info.id, "LISTENING ");
    assert!(!m.list(false)[0].command.contains("secret-value"));
    m.stop(&info.id, Duration::from_secs(5), true).unwrap();

    let store = store.lock().unwrap();
    let run = store.run(&info.id).unwrap().expect("run record");
    assert!(run.ended_at.is_some());
    assert_eq!(run.command, None, "no command lines by default");
    let events = store
        .events(&EventQuery {
            limit: 10,
            ..EventQuery::default()
        })
        .unwrap();
    let kinds: Vec<EventKind> = events.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, vec![EventKind::RunExited, EventKind::RunStarted]);
    assert_eq!(events[0].data.detail.as_deref(), Some("stopped"));
    assert_eq!(events[1].data.port, Some(5173));
    assert!(events.iter().all(|e| e.data.command.is_none()));
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn recipe_ports_reach_the_command_through_a_lease() {
    let dir = temp_project("recipe");
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    let base = (42000..60000)
        .step_by(53)
        .find(|b| (*b..*b + 10).all(portpilot_engine::doctor::port_is_bindable))
        .expect("free range");
    std::fs::write(
        dir.join(RECIPE_FILE),
        format!(
            "[project]\nname = \"fixture-shop\"\n\n[ports.web]\nrange = \"{base}-{}\"\n\n[run.dev]\ncommand = '\"{FIXTURE}\" tcp 127.0.0.1 {{ports.web}}'\nenv = {{ PP_PORT = \"{{ports.web}}\" }}\n",
            base + 9
        ),
    )
    .unwrap();
    let loaded = recipes::load(&dir).unwrap();
    assert_eq!(loaded.context.name, "fixture-shop");
    let run = &loaded.recipe.runs["dev"];
    assert_eq!(run.services(), vec!["web".to_string()]);

    let mut store = Store::in_memory().unwrap();
    let snap = Discovery::new(local_only()).snapshot();
    let check = PortCheck {
        snapshot: &snap,
        excluded: &[],
    };
    let lease = leases::acquire(
        &mut store,
        &loaded.context.owner,
        &loaded.context.name,
        "web",
        &loaded.recipe.ports["web"],
        Want::Keep,
        &check,
    )
    .unwrap();
    assert_eq!(lease.port, base);
    let rendered = run
        .render(&BTreeMap::from([("web".to_string(), lease.port)]))
        .unwrap();
    assert_eq!(
        rendered.env,
        vec![("PP_PORT".to_string(), base.to_string())]
    );

    let m = RunManager::new(FIXTURE.into(), None, HistoryPrefs::default());
    let info = m
        .start(RunSpec {
            name: "dev".into(),
            project: loaded.context.name.clone(),
            folder: recipes::run_folder(&loaded.context, run).unwrap(),
            command: rendered.command,
            env: rendered.env,
            ports: vec![RunPort {
                service: "web".into(),
                port: lease.port,
            }],
        })
        .unwrap();
    let port: u16 = wait_line(&m, &info.id, "LISTENING ").parse().unwrap();
    assert_eq!(port, base, "the leased port reached the command");
    m.stop(&info.id, Duration::from_secs(5), true).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
