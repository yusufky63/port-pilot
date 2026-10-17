//! Managed runs: a project's dev command started by PortPilot.
//!
//! - The command runs with `cmd.exe /d /s /c` in the exact folder the user approved, with
//!   `CREATE_NEW_PROCESS_GROUP`, inside a Job Object that ends the run if its owner (the app
//!   or `portctl run`) exits. Nothing is started that the user did not confirm.
//! - The PID, creation time and folder are recorded when the process starts, so listeners of
//!   the run are attributed to that folder exactly.
//! - The app captures stdout and stderr in memory; nothing of it is written to disk.
//! - Stopping sends CTRL+BREAK to the run's process group, which only the run's own processes
//!   belong to. Ending the whole job (`TerminateJobObject`) needs the user's consent.

use std::collections::HashSet;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portpilot_core::output::{OutputChunk, OutputLog, Stream};
use portpilot_core::redact::redact_command_line;
use portpilot_core::time::{filetime_to_rfc3339, format_rfc3339};
use portpilot_core::{GracefulChannel, GracefulSkip, StopOutcome, StopReport};
use portpilot_win::control::{self, ConsoleCtrl, ConsoleSignal};
use portpilot_win::job::{Job, spawn_in_job};
use portpilot_win::process;
use serde::Serialize;

use crate::discover::ManagedRun;
use crate::store::{EventKind, HistoryPrefs, NewEvent, RunPort, RunRecord, Store, now_ms};

const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Everything about a run that the user approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    /// The run's name in the recipe.
    pub name: String,
    pub project: String,
    /// The exact working directory.
    pub folder: String,
    /// The command with its ports filled in.
    pub command: String,
    pub env: Vec<(String, String)>,
    pub ports: Vec<RunPort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Pipes into memory and a console without a window (the desktop app).
    Capture,
    /// The caller's console (`portctl run`).
    Inherit,
}

pub struct Launched {
    pub child: Child,
    pub job: Job,
    pub pid: u32,
    /// Creation time of the first process; with `pid`, the run's identity.
    pub start: u64,
    pub started_at: i64,
}

/// A run's id: its first process's PID and creation time, which never repeat together.
pub fn run_id(pid: u32, start: u64) -> String {
    format!("{pid:x}-{start:x}")
}

fn cmd_exe() -> PathBuf {
    // The full path: a `cmd.exe` in the project folder must never be the one that runs.
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join(r"System32\cmd.exe")
}

/// Starts the run's command inside a new job.
pub fn launch(spec: &RunSpec, mode: OutputMode) -> Result<Launched, String> {
    if !Path::new(&spec.folder).is_dir() {
        return Err(format!("{} does not exist", spec.folder));
    }
    let mut cmd = Command::new(cmd_exe());
    // `/s` strips exactly the outer quotes, so the command reaches cmd.exe as written.
    cmd.raw_arg(format!("/d /s /c \"{}\"", spec.command))
        .current_dir(&spec.folder)
        .envs(spec.env.iter().map(|(k, v)| (k, v)));
    let mut flags = CREATE_NEW_PROCESS_GROUP;
    match mode {
        OutputMode::Capture => {
            cmd.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            flags |= CREATE_NO_WINDOW;
        }
        OutputMode::Inherit => {
            cmd.stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
        }
    }
    let job = Job::new(true)?;
    let child = spawn_in_job(&mut cmd, flags, &job).map_err(|e| format!("could not start: {e}"))?;
    let pid = child.id();
    let start =
        process::child_start_time(&child).ok_or("could not read the run's creation time")?;
    Ok(Launched {
        child,
        job,
        pid,
        start,
        started_at: now_ms(),
    })
}

fn ports_text(ports: &[RunPort]) -> Option<String> {
    (!ports.is_empty()).then(|| {
        ports
            .iter()
            .map(|p| format!("{} {}", p.service, p.port))
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// The current process as a run owner.
pub fn own_identity() -> (u32, u64) {
    let me = std::process::id();
    (me, process::start_time(me).unwrap_or(0))
}

/// Records a started run: always in the run registry (other PortPilot processes attribute
/// its listeners from there), and in the timeline when history is on.
pub fn record_start(
    store: &Store,
    spec: &RunSpec,
    pid: u32,
    start: u64,
    started_at: i64,
    prefs: HistoryPrefs,
) -> Result<String, String> {
    let id = run_id(pid, start);
    let (owner_pid, owner_start) = own_identity();
    let command =
        (prefs.enabled && prefs.command_lines).then(|| redact_command_line(&spec.command));
    store.insert_run(&RunRecord {
        id: id.clone(),
        name: spec.name.clone(),
        project: spec.project.clone(),
        folder: spec.folder.clone(),
        pid,
        start,
        started_at,
        ended_at: None,
        exit_code: None,
        owner_pid,
        owner_start,
        ports: spec.ports.clone(),
        command: command.clone(),
    })?;
    if prefs.enabled {
        store.record(
            EventKind::RunStarted,
            started_at,
            &NewEvent {
                port: spec.ports.first().map(|p| p.port),
                run_id: Some(id.clone()),
                run_name: Some(spec.name.clone()),
                pid: Some(pid),
                project: Some(spec.project.clone()),
                folder: Some(spec.folder.clone()),
                runtime: Some("windows".into()),
                detail: ports_text(&spec.ports),
                command,
                ..NewEvent::default()
            },
        )?;
    }
    Ok(id)
}

/// Records the end of a run once. `ended_by` is `stopped` or `terminated` when PortPilot
/// ended it.
pub fn record_exit(
    store: &Store,
    id: &str,
    spec: &RunSpec,
    pid: u32,
    exit_code: Option<i32>,
    ended_by: Option<&str>,
    prefs: HistoryPrefs,
) -> Result<(), String> {
    let now = now_ms();
    if !store.finish_run(id, now, exit_code.map(i64::from))? {
        return Ok(());
    }
    if !prefs.enabled {
        return store.delete_run(id);
    }
    store.record(
        EventKind::RunExited,
        now,
        &NewEvent {
            port: spec.ports.first().map(|p| p.port),
            run_id: Some(id.to_string()),
            run_name: Some(spec.name.clone()),
            pid: Some(pid),
            project: Some(spec.project.clone()),
            folder: Some(spec.folder.clone()),
            runtime: Some("windows".into()),
            exit_code: exit_code.map(i64::from),
            detail: ended_by.map(str::to_string),
            ..NewEvent::default()
        },
    )
}

/// Runs other PortPilot processes started and still own, from the registry. Only the first
/// process is known, so listeners are matched through their parent chain. Runs whose owner
/// exited are closed first, with a history entry when `history` is on.
pub fn managed_from_store(store: &Store, history: bool) -> Vec<ManagedRun> {
    let alive = |pid: u32, start: u64| process::start_time(pid) == Some(start);
    let _ = store.sweep_lost_runs(alive, history);
    store
        .unfinished_runs()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| alive(r.pid, r.start))
        .map(|r| ManagedRun {
            run_id: r.id,
            name: r.name,
            project: r.project,
            folder: r.folder,
            members: vec![(r.pid, r.start)],
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    Running,
    /// A graceful request was sent and PortPilot is waiting.
    Stopping,
    /// No process of the run is left.
    Exited,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub id: String,
    pub name: String,
    pub project: String,
    pub folder: String,
    pub command: String,
    pub ports: Vec<RunPort>,
    pub pid: u32,
    pub start: u64,
    pub started_at: String,
    pub state: RunState,
    /// Exit code of the first process.
    pub exit_code: Option<i32>,
    pub ended_at: Option<String>,
    /// `stopped` or `terminated` when PortPilot ended the run.
    pub ended_by: Option<String>,
    /// Processes in the run's job.
    pub processes: u32,
    pub lines: usize,
}

struct Status {
    state: RunState,
    exit_code: Option<i32>,
    ended_at: Option<i64>,
    ended_by: Option<&'static str>,
    graceful_at: Option<Instant>,
}

struct Run {
    id: String,
    spec: RunSpec,
    pid: u32,
    start: u64,
    job: Job,
    status: Mutex<Status>,
    output: Mutex<OutputLog>,
}

impl Run {
    fn exited(&self) -> bool {
        self.status
            .lock()
            .map(|s| s.state == RunState::Exited)
            .unwrap_or(true)
    }

    fn info(&self, raw: bool) -> RunInfo {
        let s = self.status.lock().expect("run status");
        RunInfo {
            id: self.id.clone(),
            name: self.spec.name.clone(),
            project: self.spec.project.clone(),
            folder: self.spec.folder.clone(),
            command: if raw {
                self.spec.command.clone()
            } else {
                redact_command_line(&self.spec.command)
            },
            ports: self.spec.ports.clone(),
            pid: self.pid,
            start: self.start,
            started_at: filetime_to_rfc3339(self.start),
            state: s.state,
            exit_code: s.exit_code,
            ended_at: s.ended_at.map(format_rfc3339),
            ended_by: s.ended_by.map(str::to_string),
            processes: if s.state == RunState::Exited {
                0
            } else {
                self.job.active_processes()
            },
            lines: self.output.lock().map(|o| o.len()).unwrap_or(0),
        }
    }

    fn set_state(&self, state: RunState, ended_by: Option<&'static str>) {
        if let Ok(mut s) = self.status.lock()
            && s.state != RunState::Exited
        {
            s.state = state;
            s.ended_by = ended_by;
        }
    }
}

struct Inner {
    runs: Mutex<Vec<Arc<Run>>>,
    helper: PathBuf,
    store: Option<Arc<Mutex<Store>>>,
    prefs: Arc<Mutex<HistoryPrefs>>,
}

/// The desktop app's runs. Dropping the manager closes every job, which ends the runs.
#[derive(Clone)]
pub struct RunManager {
    inner: Arc<Inner>,
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

fn pump(run: Arc<Run>, stream: Stream, mut pipe: impl Read) {
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if let Ok(mut out) = run.output.lock() {
                    out.push(stream, &buf[..n]);
                }
            }
        }
    }
    if let Ok(mut out) = run.output.lock() {
        out.finish(stream);
    }
}

impl RunManager {
    /// `helper` is a PortPilot executable that handles the console-helper argument; `store`
    /// records runs and history when it is there.
    pub fn new(helper: PathBuf, store: Option<Arc<Mutex<Store>>>, prefs: HistoryPrefs) -> Self {
        RunManager {
            inner: Arc::new(Inner {
                runs: Mutex::new(Vec::new()),
                helper,
                store,
                prefs: Arc::new(Mutex::new(prefs)),
            }),
        }
    }

    pub fn set_prefs(&self, prefs: HistoryPrefs) {
        if let Ok(mut p) = self.inner.prefs.lock() {
            *p = prefs;
        }
    }

    fn prefs(&self) -> HistoryPrefs {
        self.inner.prefs.lock().map(|p| *p).unwrap_or_default()
    }

    fn find(&self, id: &str) -> Option<Arc<Run>> {
        self.inner
            .runs
            .lock()
            .ok()?
            .iter()
            .find(|r| r.id == id)
            .cloned()
    }

    pub fn start(&self, spec: RunSpec) -> Result<RunInfo, String> {
        let mut launched = launch(&spec, OutputMode::Capture)?;
        let id = run_id(launched.pid, launched.start);
        let stdout = launched.child.stdout.take();
        let stderr = launched.child.stderr.take();
        let run = Arc::new(Run {
            id: id.clone(),
            spec,
            pid: launched.pid,
            start: launched.start,
            job: launched.job,
            status: Mutex::new(Status {
                state: RunState::Running,
                exit_code: None,
                ended_at: None,
                ended_by: None,
                graceful_at: None,
            }),
            output: Mutex::new(OutputLog::default()),
        });
        if let Some(store) = &self.inner.store
            && let Ok(store) = store.lock()
        {
            // The run already started; a history problem must not stop it.
            let _ = record_start(
                &store,
                &run.spec,
                run.pid,
                run.start,
                launched.started_at,
                self.prefs(),
            );
        }
        if let Some(out) = stdout {
            let r = run.clone();
            std::thread::spawn(move || pump(r, Stream::Stdout, out));
        }
        if let Some(err) = stderr {
            let r = run.clone();
            std::thread::spawn(move || pump(r, Stream::Stderr, err));
        }
        let watcher = run.clone();
        let store = self.inner.store.clone();
        let prefs = self.inner.prefs.clone();
        let mut child = launched.child;
        std::thread::spawn(move || {
            let code = child.wait().ok().and_then(|s| s.code());
            // The run lasts as long as any process in its job, not only the first one.
            while watcher.job.active_processes() > 0 {
                std::thread::sleep(Duration::from_millis(200));
            }
            let ended_by = {
                let mut s = watcher.status.lock().expect("run status");
                s.state = RunState::Exited;
                s.exit_code = code;
                s.ended_at = Some(now_ms());
                s.ended_by
            };
            let prefs = prefs.lock().map(|p| *p).unwrap_or_default();
            if let Some(store) = &store
                && let Ok(store) = store.lock()
            {
                let _ = record_exit(
                    &store,
                    &watcher.id,
                    &watcher.spec,
                    watcher.pid,
                    code,
                    ended_by,
                    prefs,
                );
            }
        });
        self.inner
            .runs
            .lock()
            .map_err(|e| e.to_string())?
            .insert(0, run.clone());
        Ok(run.info(false))
    }

    /// Newest first. `raw` shows commands without masking secrets.
    pub fn list(&self, raw: bool) -> Vec<RunInfo> {
        self.inner
            .runs
            .lock()
            .map(|runs| runs.iter().map(|r| r.info(raw)).collect())
            .unwrap_or_default()
    }

    pub fn info(&self, id: &str, raw: bool) -> Option<RunInfo> {
        self.find(id).map(|r| r.info(raw))
    }

    pub fn output(&self, id: &str, after: u64, limit: usize) -> Option<OutputChunk> {
        let run = self.find(id)?;
        let out = run.output.lock().ok()?;
        Some(out.since(after, limit.clamp(1, 5000)))
    }

    pub fn output_text(&self, id: &str) -> Option<String> {
        let run = self.find(id)?;
        let out = run.output.lock().ok()?;
        Some(out.text())
    }

    pub fn active_count(&self) -> usize {
        self.inner
            .runs
            .lock()
            .map(|runs| runs.iter().filter(|r| !r.exited()).count())
            .unwrap_or(0)
    }

    /// Removes a finished run and its output from the list.
    pub fn forget(&self, id: &str) -> bool {
        let Ok(mut runs) = self.inner.runs.lock() else {
            return false;
        };
        let before = runs.len();
        runs.retain(|r| !(r.id == id && r.exited()));
        runs.len() != before
    }

    /// Every process of every running run, with creation times, for discovery.
    pub fn managed(&self) -> Vec<ManagedRun> {
        let runs: Vec<Arc<Run>> = self
            .inner
            .runs
            .lock()
            .map(|r| r.iter().filter(|r| !r.exited()).cloned().collect())
            .unwrap_or_default();
        runs.iter()
            .map(|r| {
                let mut members: Vec<(u32, u64)> = r
                    .job
                    .process_ids()
                    .into_iter()
                    .filter_map(|pid| process::start_time(pid).map(|s| (pid, s)))
                    .collect();
                if !members.contains(&(r.pid, r.start)) {
                    members.push((r.pid, r.start));
                }
                ManagedRun {
                    run_id: r.id.clone(),
                    name: r.spec.name.clone(),
                    project: r.spec.project.clone(),
                    folder: r.spec.folder.clone(),
                    members,
                }
            })
            .collect()
    }

    fn wait_recorded(run: &Run) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !run.exited() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// CTRL+BREAK to the run's process group; returns the channel or why it was skipped.
    fn send_break(&self, run: &Run) -> (Option<GracefulChannel>, Option<GracefulSkip>) {
        let members = run.job.process_ids();
        // The helper attaches to the run's console through a process that is still on it.
        let Some(attach) = members
            .iter()
            .copied()
            .find(|p| *p == run.pid)
            .or_else(|| members.iter().copied().find(|p| process::is_running(*p)))
        else {
            return (None, Some(GracefulSkip::NoConsole));
        };
        let allowed: HashSet<u32> = members.into_iter().collect();
        match control::console_signal_to_group(
            &self.inner.helper,
            attach,
            &allowed,
            ConsoleSignal::CtrlBreak,
            run.pid,
        ) {
            ConsoleCtrl::Sent => (Some(GracefulChannel::GroupCtrlBreak), None),
            ConsoleCtrl::NoConsole => (None, Some(GracefulSkip::NoConsole)),
            ConsoleCtrl::Shared => (None, Some(GracefulSkip::SharedConsole)),
            ConsoleCtrl::Failed => (None, Some(GracefulSkip::SignalFailed)),
        }
    }

    /// Graceful first: CTRL+BREAK to the run's process group and up to `grace` to exit. With
    /// `force`, the job is ended afterwards; when a graceful request already went out in the
    /// last two minutes, `force` ends it right away.
    pub fn stop(&self, id: &str, grace: Duration, force: bool) -> Result<StopReport, String> {
        let run = self.find(id).ok_or("this run is gone")?;
        let started = Instant::now();
        if run.exited() {
            return Ok(report(StopOutcome::AlreadyExited, started));
        }
        let recent = run
            .status
            .lock()
            .ok()
            .and_then(|s| s.graceful_at)
            .is_some_and(|t| t.elapsed() < Duration::from_secs(120));
        let (mut channel, mut skipped) = (None, None);
        if !(force && recent) {
            if let Ok(mut s) = run.status.lock() {
                s.graceful_at = Some(Instant::now());
            }
            run.set_state(RunState::Stopping, Some("stopped"));
            (channel, skipped) = self.send_break(&run);
            if channel.is_some() && run.job.wait_empty(grace) {
                Self::wait_recorded(&run);
                let mut r = report(StopOutcome::Stopped, started);
                r.channel = channel;
                return Ok(r);
            }
        }
        if run.job.active_processes() == 0 {
            Self::wait_recorded(&run);
            let mut r = report(StopOutcome::Stopped, started);
            r.channel = channel;
            return Ok(r);
        }
        if !force {
            run.set_state(RunState::Running, None);
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
            return Ok(r);
        }
        let members = run.job.process_ids().len() as u32;
        run.set_state(RunState::Stopping, Some("terminated"));
        run.job.terminate(1)?;
        run.job.wait_empty(Duration::from_secs(5));
        Self::wait_recorded(&run);
        let mut r = report(StopOutcome::Forced, started);
        r.channel = channel;
        r.graceful_skipped = skipped;
        r.children_terminated = members.saturating_sub(1);
        Ok(r)
    }

    /// Stops every run before the app quits: CTRL+BREAK to all of them, up to `grace` to
    /// exit, then the jobs of the ones still running are ended. The caller asked the user.
    /// Returns how many had to be ended.
    pub fn stop_all(&self, grace: Duration) -> usize {
        let runs: Vec<Arc<Run>> = self
            .inner
            .runs
            .lock()
            .map(|r| r.iter().filter(|r| !r.exited()).cloned().collect())
            .unwrap_or_default();
        for run in &runs {
            run.set_state(RunState::Stopping, Some("stopped"));
            let _ = self.send_break(run);
        }
        let deadline = Instant::now() + grace;
        let mut ended = 0;
        for run in &runs {
            let left = deadline.saturating_duration_since(Instant::now());
            if !run.job.wait_empty(left) {
                run.set_state(RunState::Stopping, Some("terminated"));
                if run.job.terminate(1).is_ok() {
                    ended += 1;
                }
                run.job.wait_empty(Duration::from_secs(3));
            }
            Self::wait_recorded(run);
        }
        ended
    }

    /// Whether this manager started the run.
    pub fn owns(&self, id: &str) -> bool {
        self.find(id).is_some()
    }
}
