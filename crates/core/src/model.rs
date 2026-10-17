//! The snapshot model shared by the CLI (`--json`), the desktop app and its frontend.
//!
//! Field names are camelCase on the wire. `schemaVersion` changes only when a field is removed
//! or changes meaning; new optional fields keep the version.

use serde::{Deserialize, Serialize};

/// Version of the JSON shape produced by `portctl ... --json` and the desktop bridge.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Tcp => "tcp",
            Protocol::Udp => "udp",
        }
    }
}

/// Who can reach an endpoint. Ordered from narrowest to widest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Exposure {
    /// 127.0.0.0/8 or ::1: only this machine.
    Loopback,
    /// One specific non-loopback address: that network.
    Lan,
    /// 0.0.0.0 or ::: every adapter, including Wi-Fi and VPNs.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Runtime {
    Windows,
    Wsl,
    Docker,
}

impl Runtime {
    pub fn as_str(self) -> &'static str {
        match self {
            Runtime::Windows => "windows",
            Runtime::Wsl => "wsl",
            Runtime::Docker => "docker",
        }
    }
}

/// How sure PortPilot is about a project folder. There is no public Windows API for another
/// process's working directory, so most Windows attributions are inferred and say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    /// Reported by the runtime itself (the Linux kernel, a Compose label) or recorded when Port
    /// Pilot started the process.
    Exact,
    /// Derived from a path in the command line or the executable's location.
    Inferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AttributionSource {
    /// `/proc/<pid>/cwd` inside WSL.
    ProcCwd,
    /// `com.docker.compose.project.working_dir`.
    ComposeLabel,
    /// An absolute path among the process arguments.
    CommandLine,
    /// The folder of the executable, for example a virtualenv or `target\debug`.
    ExecutablePath,
    /// The working directory PortPilot started a managed run in.
    Launched,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessClass {
    /// A program the user started.
    User,
    /// A Windows service host or a process running as a service account.
    Service,
    /// The kernel and core session processes.
    System,
    /// A forwarder that holds a host port for WSL or Docker.
    Relay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    /// Identity token next to the PID: the creation time as a FILETIME on Windows, the start
    /// time in clock ticks inside WSL. PIDs are reused; this pair is not.
    pub start: Option<u64>,
    /// Creation time as RFC 3339 UTC, when known.
    pub started_at: Option<String>,
    pub name: String,
    pub path: Option<String>,
    /// Redacted unless the caller asked for raw command lines.
    pub command_line: Option<String>,
    pub user: Option<String>,
    pub class: ProcessClass,
    /// The process could not be opened for query (another user, elevated, protected).
    pub access_denied: bool,
}

/// A process in an ownership chain, without the details only the owner needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRef {
    pub pid: u32,
    pub name: String,
    pub start: Option<u64>,
    pub started_at: Option<String>,
    pub command_line: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LauncherKind {
    Claude,
    Codex,
    Gemini,
    VsCode,
    Cursor,
    Windsurf,
    JetBrains,
    Zed,
    Terminal,
    Explorer,
    Service,
}

/// The nearest ancestor that tells who started a listener: an agent, an editor or a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Launcher {
    pub kind: LauncherKind,
    pub pid: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    pub top_level: String,
    pub repo_name: String,
    /// The shared `.git` directory; equal for every worktree of one repository.
    pub common_dir: String,
    /// `None` on a detached HEAD.
    pub branch: Option<String>,
    /// Abbreviated commit; `None` before the first commit.
    pub head: Option<String>,
    /// `None` when `git status` did not finish in time.
    pub dirty: Option<bool>,
    /// A linked worktree (`git worktree add`), not the main checkout.
    pub linked_worktree: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    /// Windows path, or a Linux path for WSL listeners.
    pub root: String,
    pub name: String,
    pub confidence: Confidence,
    pub source: AttributionSource,
    /// Manifest files found in `root` (`package.json`, `Cargo.toml`, ...).
    pub manifests: Vec<String>,
    pub git: Option<GitInfo>,
    /// The folder as Windows can open it (`\\wsl.localhost\...` for WSL paths).
    pub open_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerPort {
    pub protocol: Protocol,
    pub host_ip: Option<String>,
    pub host_port: Option<u16>,
    pub container_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerInfo {
    /// Full container id.
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
    pub compose_working_dir: Option<String>,
    pub ports: Vec<ContainerPort>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WslInfo {
    pub distro: String,
    /// Linux working directory from `/proc/<pid>/cwd`.
    pub cwd: Option<String>,
    /// Where the socket is bound inside the distro.
    pub binds: Vec<Endpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub address: String,
    pub port: u16,
    pub exposure: Exposure,
}

/// What a stop request has to name so the engine can prove it acts on the same instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StopTarget {
    #[serde(rename_all = "camelCase")]
    Windows { pid: u32, start: u64 },
    #[serde(rename_all = "camelCase")]
    Wsl {
        distro: String,
        pid: u32,
        start: u64,
    },
    #[serde(rename_all = "camelCase")]
    Docker { container_id: String },
}

/// Why a listener cannot be stopped from PortPilot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopBlock {
    /// Kernel or session process.
    System,
    /// A Windows service; use the Services app.
    Service,
    /// A WSL or Docker forwarder; stop the Linux process or the container instead.
    Relay,
    /// The process belongs to another user or is elevated.
    AccessDenied,
    /// The creation time could not be read, so the instance cannot be verified.
    IdentityUnknown,
    /// PortPilot itself, or the process that runs it.
    SelfProcess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopInfo {
    pub target: Option<StopTarget>,
    pub blocked: Option<StopBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Note {
    /// A WSL forwarder holds the port but no Linux listener matches it.
    RelayWithoutListener,
    /// Docker holds the port but no running container publishes it.
    RelayWithoutContainer,
    /// A Linux listener with no Windows-side forwarder in the table.
    NotForwarded,
    /// The parent process exited or its PID now belongs to a newer process.
    ParentExited,
    /// Another owner listens on the same port at a different address.
    SharedPort,
}

/// The managed run a listener belongs to: PortPilot started its process tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRef {
    pub run_id: String,
    /// The run's name in the recipe (`[run.dev]` is `dev`).
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listener {
    /// Stable while the same owner holds the same port.
    pub id: String,
    pub protocol: Protocol,
    pub port: u16,
    /// Where the port is reachable on Windows.
    pub endpoints: Vec<Endpoint>,
    /// Widest exposure among `endpoints`.
    pub exposure: Exposure,
    pub runtime: Runtime,
    /// The owner: a Windows process, or the Linux process for WSL listeners. Empty for Docker,
    /// where the container is the owner.
    pub process: Option<ProcessInfo>,
    /// Parent first, then its parent, up to the session root or the first process that exited.
    pub ancestors: Vec<ProcessRef>,
    pub launcher: Option<Launcher>,
    /// The Windows process that forwards traffic for WSL or Docker.
    pub relay: Option<ProcessRef>,
    pub container: Option<ContainerInfo>,
    pub wsl: Option<WslInfo>,
    pub project: Option<ProjectInfo>,
    /// A URL to open for TCP listeners.
    pub url: Option<String>,
    pub stop: StopInfo,
    pub conflict: bool,
    pub notes: Vec<Note>,
    /// Set when the owner belongs to a run PortPilot started.
    #[serde(default)]
    pub managed: Option<ManagedRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub protocol: Protocol,
    pub port: u16,
    pub listener_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdapterStatus {
    Available,
    /// Turned off in settings or by a flag.
    Disabled,
    NotInstalled,
    NotRunning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterState {
    pub status: AdapterStatus,
    pub detail: Option<String>,
    /// Running distros for WSL, running containers for Docker.
    pub items: Vec<String>,
}

impl AdapterState {
    pub fn new(status: AdapterStatus) -> Self {
        AdapterState {
            status,
            detail: None,
            items: Vec::new(),
        }
    }

    pub fn with_detail(status: AdapterStatus, detail: impl Into<String>) -> Self {
        AdapterState {
            status,
            detail: Some(detail.into()),
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Adapters {
    pub wsl: AdapterState,
    pub docker: AdapterState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub schema_version: u32,
    pub taken_at: String,
    pub duration_ms: u64,
    pub listeners: Vec<Listener>,
    pub conflicts: Vec<Conflict>,
    pub adapters: Adapters,
    /// Non-fatal problems, such as a table that could not be read.
    pub warnings: Vec<String>,
}

/// Result of a stop request, from the engine to the CLI and the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopOutcome {
    /// The process exited after a graceful request.
    Stopped,
    /// The process was terminated after the grace period.
    Forced,
    /// The process exited before PortPilot acted.
    AlreadyExited,
    /// The PID now belongs to another process; nothing was done.
    IdentityChanged,
    /// The process ignored the graceful request and force was not allowed.
    GracefulTimeout,
    /// No safe graceful channel exists and force was not allowed.
    GracefulUnavailable,
    AccessDenied,
    /// The target class may not be stopped (system, service, relay, PortPilot itself).
    Refused,
    /// WSL or Docker could not be reached.
    RuntimeUnavailable,
    Failed,
}

/// How the graceful request was delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GracefulChannel {
    /// WM_CLOSE to the process's top-level windows.
    CloseWindow,
    /// CTRL+C on a console that only the target's own process tree uses.
    ConsoleCtrlC,
    /// CTRL+BREAK on that console, after CTRL+C was ignored.
    ConsoleCtrlBreak,
    /// CTRL+BREAK to a managed run's own process group.
    GroupCtrlBreak,
    /// SIGTERM inside WSL.
    Sigterm,
    /// `POST /containers/{id}/stop`.
    DockerStop,
}

/// Why no graceful request was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GracefulSkip {
    /// No top-level window and no console to signal.
    NoConsole,
    /// Other programs share the console; CTRL+C would reach them too.
    SharedConsole,
    /// The console helper could not deliver the signal.
    SignalFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopReport {
    pub outcome: StopOutcome,
    pub channel: Option<GracefulChannel>,
    /// Why the graceful channel was skipped, when it was.
    pub graceful_skipped: Option<GracefulSkip>,
    pub detail: Option<String>,
    /// Child processes that were terminated together with the target.
    pub children_terminated: u32,
    pub elapsed_ms: u64,
}
