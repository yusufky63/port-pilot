//! Discovery: one call turns the socket tables, the process list, Git, WSL and Docker into a
//! `Snapshot` of explained listeners.
//!
//! A `Discovery` keeps caches between refreshes. Command lines and project folders are cached
//! per process instance (PID plus creation time), so a long-running dev server is looked up
//! once; Git state, WSL and Docker are cached for a short time.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::Path;
use std::time::{Duration, Instant};

use portpilot_core::assemble::{
    SocketGroup, group_sockets, listener_id, mark_conflicts, merge_runtimes, process_ref,
    refresh_reachability, sort_listeners,
};
use portpilot_core::docker::published;
use portpilot_core::infer::{
    Fs, InferContext, Inferred, classify_process, detect_launcher, folder_name,
};
use portpilot_core::net::endpoint;
use portpilot_core::redact::redact_command_line;
use portpilot_core::time::{filetime_to_rfc3339, format_rfc3339};
use portpilot_core::wsl::{LinuxProc, unc_path};
use portpilot_core::{
    AdapterState, AdapterStatus, Adapters, AttributionSource, Confidence, ContainerInfo, Endpoint,
    Exposure, GitInfo, Launcher, Listener, ManagedRef, Note, ProcessClass, ProcessInfo, ProcessRef,
    ProjectInfo, Protocol, Runtime, SCHEMA_VERSION, Snapshot, StopBlock, StopInfo, StopTarget,
    WslInfo,
};
use portpilot_win::process::{self, Detail, ProcEntry};
use portpilot_win::{net, wmi};

use crate::git::{GitCache, windows_path};
use crate::{docker, wsl};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub udp: bool,
    pub wsl: bool,
    pub docker: bool,
    /// Show command lines as they are instead of masking secrets.
    pub raw_command_lines: bool,
    /// How long WSL and Docker results are reused. Zero for one-shot callers like the CLI.
    pub runtime_ttl: Duration,
    pub git_ttl: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            udp: true,
            wsl: true,
            docker: true,
            raw_command_lines: false,
            runtime_ttl: Duration::ZERO,
            git_ttl: Duration::from_secs(10),
        }
    }
}

const MAX_ANCESTORS: usize = 16;

struct RealFs;

impl Fs for RealFs {
    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }
    fn is_dir(&self, path: &str) -> bool {
        Path::new(path).is_dir()
    }
    fn files_with_ext(&self, dir: &str, exts: &[&str]) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        rd.flatten()
            .take(500)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| {
                n.rsplit_once('.')
                    .is_some_and(|(_, ext)| exts.contains(&ext.to_ascii_lowercase().as_str()))
            })
            .collect()
    }
}

/// Folders that never hold projects: Windows, Program Files, AppData and toolchain homes.
fn denied_prefixes() -> Vec<String> {
    let mut out = Vec::new();
    for var in [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        if let Ok(v) = std::env::var(var) {
            out.push(v.to_ascii_lowercase());
        }
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        for sub in [
            ".cargo", ".rustup", "scoop", ".bun", ".deno", r"go\pkg", ".nvm", ".pyenv", ".vscode",
            ".cursor",
        ] {
            out.push(format!("{home}\\{sub}").to_ascii_lowercase());
        }
    }
    out
}

struct Cached<T> {
    at: Instant,
    value: T,
}

/// A run PortPilot started: its processes are attributed to its folder exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedRun {
    pub run_id: String,
    pub name: String,
    pub project: String,
    /// The working directory recorded when the run started.
    pub folder: String,
    /// `(pid, creation time)` of processes in the run: every process of the job when the
    /// caller owns it, or only the first one, matched through parent chains.
    pub members: Vec<(u32, u64)>,
}

pub struct Discovery {
    opts: Options,
    git: GitCache,
    denied: Vec<String>,
    home: Option<String>,
    current_user: Option<String>,
    elevated: bool,
    /// Raw command line per process instance.
    cmdlines: HashMap<(u32, u64), Option<String>>,
    /// Command line and creation time for processes that cannot be opened, checked by name.
    opaque: HashMap<u32, (String, Option<String>, Option<u64>)>,
    projects: HashMap<(u32, u64), Option<Inferred>>,
    wsl_cache: Option<Cached<(Vec<Listener>, AdapterState)>>,
    docker_cache: Option<Cached<Result<Vec<ContainerInfo>, AdapterState>>>,
    managed: Vec<ManagedRun>,
}

fn ensure(details: &mut HashMap<u32, Detail>, pid: u32) -> &Detail {
    details.entry(pid).or_insert_with(|| process::detail(pid))
}

/// Parent first. Stops where the parent exited or its PID was reused (a parent can never be
/// younger than its child). The flag says the direct parent is gone.
fn ancestors(
    pid: u32,
    procs: &HashMap<u32, ProcEntry>,
    details: &mut HashMap<u32, Detail>,
) -> (Vec<u32>, bool) {
    let mut out = Vec::new();
    let mut child = pid;
    let mut child_start = ensure(details, pid).start;
    while out.len() < MAX_ANCESTORS {
        let Some(entry) = procs.get(&child) else {
            break;
        };
        let ppid = entry.ppid;
        if ppid == 0 || ppid == child || out.contains(&ppid) {
            break;
        }
        if !procs.contains_key(&ppid) {
            return (out.clone(), out.is_empty());
        }
        let parent_start = ensure(details, ppid).start;
        if let (Some(c), Some(p)) = (child_start, parent_start)
            && p > c
        {
            return (out.clone(), out.is_empty());
        }
        out.push(ppid);
        child = ppid;
        child_start = parent_start;
    }
    (out, false)
}

impl Discovery {
    pub fn new(opts: Options) -> Self {
        Discovery {
            git: GitCache::new(opts.git_ttl),
            opts,
            denied: denied_prefixes(),
            home: std::env::var("USERPROFILE").ok(),
            current_user: process::current_user(),
            elevated: process::is_elevated(),
            cmdlines: HashMap::new(),
            opaque: HashMap::new(),
            projects: HashMap::new(),
            wsl_cache: None,
            docker_cache: None,
            managed: Vec::new(),
        }
    }

    /// The runs PortPilot started, for exact attribution in the next snapshots.
    pub fn set_managed(&mut self, runs: Vec<ManagedRun>) {
        self.managed = runs;
    }

    /// The run that owns process `pid` (created at `start`), directly or through its parent
    /// chain. Identity is PID plus creation time, so a reused PID never matches.
    fn managed_run(
        &self,
        pid: u32,
        start: Option<u64>,
        chain: &[u32],
        details: &HashMap<u32, Detail>,
    ) -> Option<&ManagedRun> {
        if self.managed.is_empty() {
            return None;
        }
        let me = start.map(|s| (pid, s));
        let line = me.into_iter().chain(
            chain
                .iter()
                .filter_map(|p| details.get(p).and_then(|d| d.start).map(|s| (*p, s))),
        );
        for id in line {
            if let Some(run) = self.managed.iter().find(|r| r.members.contains(&id)) {
                return Some(run);
            }
        }
        None
    }

    fn managed_project(&self, run: &ManagedRun) -> ProjectInfo {
        let fs = RealFs;
        let ctx = InferContext {
            fs: &fs,
            denied: Vec::new(),
            home: None,
        };
        ProjectInfo {
            root: run.folder.clone(),
            name: run.project.clone(),
            confidence: Confidence::Exact,
            source: AttributionSource::Launched,
            manifests: ctx.manifests_in(&run.folder),
            git: self.git.info(&run.folder),
            open_path: Some(run.folder.clone()),
        }
    }

    pub fn options(&self) -> &Options {
        &self.opts
    }

    pub fn set_options(&mut self, opts: Options) {
        if opts.wsl != self.opts.wsl || opts.udp != self.opts.udp {
            self.wsl_cache = None;
        }
        if opts.docker != self.opts.docker {
            self.docker_cache = None;
        }
        self.opts = opts;
    }

    /// Drops the WSL and Docker caches so the next snapshot asks again.
    pub fn invalidate_runtimes(&mut self) {
        self.wsl_cache = None;
        self.docker_cache = None;
    }

    fn redact(&self, cmd: Option<&String>) -> Option<String> {
        cmd.map(|c| {
            if self.opts.raw_command_lines {
                c.clone()
            } else {
                redact_command_line(c)
            }
        })
    }

    /// Fetches command lines for processes not seen before, through one WMI round trip.
    fn load_command_lines(
        &mut self,
        pids: &HashSet<u32>,
        procs: &HashMap<u32, ProcEntry>,
        details: &HashMap<u32, Detail>,
        warnings: &mut Vec<String>,
    ) {
        let mut wanted = Vec::new();
        for &pid in pids {
            if pid == 0 || pid == 4 {
                continue;
            }
            let name = procs.get(&pid).map(|p| p.name.as_str()).unwrap_or_default();
            match details.get(&pid).and_then(|d| d.start) {
                Some(start) if !self.cmdlines.contains_key(&(pid, start)) => wanted.push(pid),
                None if self.opaque.get(&pid).is_none_or(|(n, _, _)| n != name) => wanted.push(pid),
                _ => {}
            }
        }
        if wanted.is_empty() {
            return;
        }
        let found = match wmi::processes(&wanted) {
            Ok(f) => f,
            Err(e) => {
                warnings.push(e);
                return;
            }
        };
        for pid in wanted {
            let w = found.get(&pid);
            match details.get(&pid).and_then(|d| d.start) {
                Some(start) => {
                    // The process must still be the same instance after the query; otherwise
                    // the command line may belong to a newer process with the same PID.
                    if process::start_time(pid) == Some(start) {
                        self.cmdlines
                            .insert((pid, start), w.and_then(|w| w.command_line.clone()));
                    }
                }
                None => {
                    let name = procs.get(&pid).map(|p| p.name.clone()).unwrap_or_default();
                    self.opaque.insert(
                        pid,
                        (
                            name,
                            w.and_then(|w| w.command_line.clone()),
                            w.and_then(|w| w.created),
                        ),
                    );
                }
            }
        }
    }

    fn raw_cmdline(&self, pid: u32, details: &HashMap<u32, Detail>) -> Option<&String> {
        match details.get(&pid).and_then(|d| d.start) {
            Some(start) => self.cmdlines.get(&(pid, start)).and_then(Option::as_ref),
            None => self.opaque.get(&pid).and_then(|(_, c, _)| c.as_ref()),
        }
    }

    fn process_info(
        &self,
        pid: u32,
        procs: &HashMap<u32, ProcEntry>,
        details: &HashMap<u32, Detail>,
    ) -> ProcessInfo {
        let d = details.get(&pid).cloned().unwrap_or_default();
        let name = procs.get(&pid).map(|p| p.name.clone()).unwrap_or_else(|| {
            if pid == 4 {
                "System".into()
            } else {
                format!("PID {pid}")
            }
        });
        let created = d
            .start
            .or_else(|| self.opaque.get(&pid).and_then(|(_, _, c)| *c));
        let class = classify_process(pid, &name, d.user.as_deref());
        ProcessInfo {
            pid,
            start: d.start,
            started_at: created.map(filetime_to_rfc3339),
            name,
            path: d.path,
            command_line: self.redact(self.raw_cmdline(pid, details)),
            user: d.user,
            class,
            access_denied: d.access_denied,
        }
    }

    fn project_for(&mut self, pid: u32, details: &HashMap<u32, Detail>) -> Option<ProjectInfo> {
        let d = details.get(&pid)?;
        let start = d.start?;
        if !self.projects.contains_key(&(pid, start)) {
            let fs = RealFs;
            let ctx = InferContext {
                fs: &fs,
                denied: self.denied.clone(),
                home: self.home.clone(),
            };
            let cmd = self.cmdlines.get(&(pid, start)).and_then(Option::as_deref);
            let inferred = ctx.project(cmd, d.path.as_deref());
            self.projects.insert((pid, start), inferred);
        }
        let inf = self.projects.get(&(pid, start))?.clone()?;
        let git = self.git.info(&inf.root);
        Some(ProjectInfo {
            name: folder_name(&inf.root),
            open_path: Some(inf.root.clone()),
            root: inf.root,
            confidence: Confidence::Inferred,
            source: inf.source,
            manifests: inf.manifests,
            git,
        })
    }

    fn stop_info(&self, p: &ProcessInfo, lineage: &[u32]) -> StopInfo {
        let blocked = match p.class {
            ProcessClass::System => Some(StopBlock::System),
            ProcessClass::Service => Some(StopBlock::Service),
            ProcessClass::Relay => Some(StopBlock::Relay),
            ProcessClass::User => None,
        }
        .or_else(|| lineage.contains(&p.pid).then_some(StopBlock::SelfProcess))
        .or_else(|| p.access_denied.then_some(StopBlock::AccessDenied))
        .or_else(|| p.start.is_none().then_some(StopBlock::IdentityUnknown))
        .or_else(|| {
            let other_user = matches!((&p.user, &self.current_user), (Some(a), Some(b)) if !a.eq_ignore_ascii_case(b));
            (other_user && !self.elevated).then_some(StopBlock::AccessDenied)
        });
        StopInfo {
            target: match (blocked, p.start) {
                (None, Some(start)) => Some(StopTarget::Windows { pid: p.pid, start }),
                _ => None,
            },
            blocked,
        }
    }

    fn host_listeners(
        &mut self,
        groups: Vec<SocketGroup>,
        procs: &HashMap<u32, ProcEntry>,
        warnings: &mut Vec<String>,
    ) -> Vec<Listener> {
        let mut details: HashMap<u32, Detail> = HashMap::new();
        let mut chains: HashMap<u32, (Vec<u32>, bool)> = HashMap::new();
        let mut interesting: HashSet<u32> = HashSet::new();
        for g in &groups {
            if let std::collections::hash_map::Entry::Vacant(e) = chains.entry(g.pid) {
                let chain = ancestors(g.pid, procs, &mut details);
                interesting.insert(g.pid);
                interesting.extend(chain.0.iter().copied());
                e.insert(chain);
            }
        }
        self.load_command_lines(&interesting, procs, &details, warnings);
        let lineage = process::own_lineage(procs);

        let mut out = Vec::with_capacity(groups.len());
        for g in groups {
            let process = self.process_info(g.pid, procs, &details);
            let (chain, orphaned) = chains.get(&g.pid).cloned().unwrap_or_default();
            let refs: Vec<ProcessRef> = chain
                .iter()
                .map(|pid| process_ref(&self.process_info(*pid, procs, &details)))
                .collect();
            let launcher = detect_launcher(chain.iter().map(|pid| {
                (
                    *pid,
                    procs.get(pid).map(|p| p.name.as_str()).unwrap_or_default(),
                    self.raw_cmdline(*pid, &details).map(String::as_str),
                )
            }))
            .map(|(kind, pid, name)| Launcher { kind, pid, name });
            let managed = (process.class == ProcessClass::User)
                .then(|| self.managed_run(g.pid, process.start, &chain, &details))
                .flatten()
                .cloned();
            let project = match &managed {
                Some(run) => Some(self.managed_project(run)),
                None if process.class == ProcessClass::User => self.project_for(g.pid, &details),
                None => None,
            };
            let stop = self.stop_info(&process, &lineage);
            let owner = format!("{}:{}", g.pid, process.start.unwrap_or(0));
            let mut l = Listener {
                id: listener_id(Runtime::Windows, g.protocol, g.port, &owner),
                protocol: g.protocol,
                port: g.port,
                endpoints: g.endpoints,
                exposure: Exposure::Loopback,
                runtime: Runtime::Windows,
                process: Some(process),
                ancestors: refs,
                launcher,
                relay: None,
                container: None,
                wsl: None,
                project,
                url: None,
                stop,
                conflict: false,
                notes: if orphaned {
                    vec![Note::ParentExited]
                } else {
                    Vec::new()
                },
                managed: managed.map(|r| ManagedRef {
                    run_id: r.run_id,
                    name: r.name,
                }),
            };
            refresh_reachability(&mut l);
            out.push(l);
        }
        // Forget instances that are gone so the caches do not grow without bound.
        let live: HashSet<(u32, u64)> = details
            .iter()
            .filter_map(|(pid, d)| d.start.map(|s| (*pid, s)))
            .collect();
        if self.cmdlines.len() > 4 * live.len().max(256) {
            self.cmdlines.retain(|k, _| live.contains(k));
            self.projects.retain(|k, _| live.contains(k));
            self.opaque.retain(|pid, _| procs.contains_key(pid));
        }
        out
    }

    fn docker_listeners(&mut self) -> (Vec<Listener>, AdapterState) {
        if !self.opts.docker {
            return (Vec::new(), AdapterState::new(AdapterStatus::Disabled));
        }
        let fresh = self
            .docker_cache
            .as_ref()
            .is_some_and(|c| c.at.elapsed() < self.opts.runtime_ttl);
        if !fresh {
            self.docker_cache = Some(Cached {
                at: Instant::now(),
                value: docker::containers(),
            });
        }
        let containers = match &self.docker_cache.as_ref().expect("filled above").value {
            Ok(c) => c.clone(),
            Err(state) => return (Vec::new(), state.clone()),
        };
        let mut state = AdapterState::new(AdapterStatus::Available);
        let mut out = Vec::new();
        for c in containers.iter().filter(|c| c.state == "running") {
            state.items.push(c.name.clone());
            let project = c.compose_working_dir.as_ref().map(|wd| {
                let windows_dir = Path::new(wd).is_dir();
                ProjectInfo {
                    root: wd.clone(),
                    name: c.compose_project.clone().unwrap_or_else(|| folder_name(wd)),
                    confidence: Confidence::Exact,
                    source: AttributionSource::ComposeLabel,
                    manifests: Vec::new(),
                    git: if windows_dir { self.git.info(wd) } else { None },
                    open_path: windows_dir.then(|| wd.clone()),
                }
            });
            for (protocol, port) in published(c) {
                if protocol == Protocol::Udp && !self.opts.udp {
                    continue;
                }
                let mut endpoints: Vec<Endpoint> = c
                    .ports
                    .iter()
                    .filter(|p| p.protocol == protocol && p.host_port == Some(port))
                    .map(|p| {
                        let ip = p
                            .host_ip
                            .as_deref()
                            .and_then(|s| s.parse::<IpAddr>().ok())
                            .unwrap_or(IpAddr::from([0, 0, 0, 0]));
                        endpoint(ip, port)
                    })
                    .collect();
                endpoints.sort_by(|a, b| a.address.cmp(&b.address));
                endpoints.dedup();
                let short: String = c.id.chars().take(12).collect();
                let mut l = Listener {
                    id: listener_id(Runtime::Docker, protocol, port, &short),
                    protocol,
                    port,
                    endpoints,
                    exposure: Exposure::Loopback,
                    runtime: Runtime::Docker,
                    process: None,
                    ancestors: Vec::new(),
                    launcher: None,
                    relay: None,
                    container: Some(c.clone()),
                    wsl: None,
                    project: project.clone(),
                    url: None,
                    stop: StopInfo {
                        target: Some(StopTarget::Docker {
                            container_id: c.id.clone(),
                        }),
                        blocked: None,
                    },
                    conflict: false,
                    notes: Vec::new(),
                    managed: None,
                };
                refresh_reachability(&mut l);
                out.push(l);
            }
        }
        (out, state)
    }

    fn wsl_listener(
        &self,
        distro: &str,
        group: SocketGroup,
        proc_name: Option<String>,
        linux: Option<&LinuxProc>,
    ) -> Listener {
        let pid = (group.pid != 0).then_some(group.pid);
        let start = linux.and_then(|p| p.start);
        let process = pid.map(|pid| ProcessInfo {
            pid,
            start,
            started_at: None,
            name: proc_name.clone().unwrap_or_else(|| "unknown".into()),
            path: None,
            command_line: self.redact(linux.and_then(|p| p.cmdline.as_ref())),
            user: None,
            class: ProcessClass::User,
            access_denied: false,
        });
        let git = linux.and_then(|p| {
            let rev = p.rev.as_ref()?;
            let st = p.status.clone().unwrap_or_default();
            Some(GitInfo {
                top_level: rev.top_level.clone(),
                repo_name: portpilot_core::git::repo_name(&rev.common_dir),
                common_dir: rev.common_dir.clone(),
                branch: st.branch,
                head: st.oid.as_deref().map(portpilot_core::git::short_oid),
                dirty: Some(st.dirty),
                linked_worktree: !portpilot_core::git::same_path(&rev.git_dir, &rev.common_dir),
            })
        });
        let cwd = linux.and_then(|p| p.cwd.clone());
        let root = git.as_ref().map(|g| g.top_level.clone()).or_else(|| {
            cwd.clone().filter(|c| {
                let depth = c.trim_end_matches('/').matches('/').count();
                // `/`, `/root` and `/home/<user>` are homes, not projects.
                !(c == "/" || c == "/root" || (c.starts_with("/home/") && depth <= 2))
            })
        });
        let project = root.map(|root| ProjectInfo {
            name: folder_name(&root),
            open_path: Some(unc_path(distro, &root)),
            root,
            confidence: Confidence::Exact,
            source: AttributionSource::ProcCwd,
            manifests: Vec::new(),
            git,
        });
        let (target, blocked) = match (pid, start) {
            (Some(pid), Some(start)) => (
                Some(StopTarget::Wsl {
                    distro: distro.to_string(),
                    pid,
                    start,
                }),
                None,
            ),
            (None, _) => (None, Some(StopBlock::AccessDenied)),
            (Some(_), None) => (None, Some(StopBlock::IdentityUnknown)),
        };
        let owner = format!("{distro}:{}:{}", group.pid, start.unwrap_or(0));
        let mut l = Listener {
            id: listener_id(Runtime::Wsl, group.protocol, group.port, &owner),
            protocol: group.protocol,
            port: group.port,
            endpoints: group.endpoints.clone(),
            exposure: Exposure::Loopback,
            runtime: Runtime::Wsl,
            process,
            ancestors: Vec::new(),
            launcher: None,
            relay: None,
            container: None,
            wsl: Some(WslInfo {
                distro: distro.to_string(),
                cwd,
                binds: group.endpoints,
            }),
            project,
            url: None,
            stop: StopInfo { target, blocked },
            conflict: false,
            notes: Vec::new(),
            managed: None,
        };
        refresh_reachability(&mut l);
        l
    }

    fn wsl_listeners(&mut self) -> (Vec<Listener>, AdapterState) {
        if !self.opts.wsl {
            return (Vec::new(), AdapterState::new(AdapterStatus::Disabled));
        }
        if let Some(c) = &self.wsl_cache
            && c.at.elapsed() < self.opts.runtime_ttl
        {
            return c.value.clone();
        }
        let result = match wsl::running_distros() {
            Err(state) => (Vec::new(), state),
            Ok(distros) => {
                let mut state = AdapterState::new(AdapterStatus::Available);
                let mut problems = Vec::new();
                let mut out = Vec::new();
                for distro in distros {
                    match wsl::probe(&distro) {
                        Ok(probe) => {
                            if probe.no_ss {
                                problems.push(format!("{distro}: ss is not installed"));
                            }
                            let sockets: Vec<portpilot_core::assemble::HostSocket> = probe
                                .sockets
                                .iter()
                                .filter(|s| self.opts.udp || s.protocol == Protocol::Tcp)
                                .map(|s| portpilot_core::assemble::HostSocket {
                                    protocol: s.protocol,
                                    address: s.address,
                                    port: s.port,
                                    pid: s.pid.unwrap_or(0),
                                })
                                .collect();
                            for g in group_sockets(&sockets) {
                                let name = probe
                                    .sockets
                                    .iter()
                                    .find(|s| s.pid == Some(g.pid) && s.port == g.port)
                                    .and_then(|s| s.process.clone());
                                let linux = probe.procs.get(&g.pid);
                                out.push(self.wsl_listener(&distro, g, name, linux));
                            }
                        }
                        Err(e) => problems.push(format!("{distro}: {e}")),
                    }
                    state.items.push(distro);
                }
                if !problems.is_empty() {
                    state.detail = Some(problems.join("; "));
                }
                (out, state)
            }
        };
        self.wsl_cache = Some(Cached {
            at: Instant::now(),
            value: result.clone(),
        });
        result
    }

    pub fn snapshot(&mut self) -> Snapshot {
        let started = Instant::now();
        let taken_at = format_rfc3339(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
        );
        let mut warnings = Vec::new();
        let procs = process::snapshot().unwrap_or_else(|e| {
            warnings.push(e);
            HashMap::new()
        });
        let mut sockets = net::tcp_listeners().unwrap_or_else(|e| {
            warnings.push(e);
            Vec::new()
        });
        if self.opts.udp {
            sockets.extend(net::udp_endpoints().unwrap_or_else(|e| {
                warnings.push(e);
                Vec::new()
            }));
        }
        let host = self.host_listeners(group_sockets(&sockets), &procs, &mut warnings);
        let (docker, docker_state) = self.docker_listeners();
        let (wsl, wsl_state) = self.wsl_listeners();
        let mut listeners = merge_runtimes(
            host,
            docker,
            wsl,
            docker_state.status == AdapterStatus::Available,
            wsl_state.status == AdapterStatus::Available,
        );
        let conflicts = mark_conflicts(&mut listeners);
        sort_listeners(&mut listeners);
        Snapshot {
            schema_version: SCHEMA_VERSION,
            taken_at,
            duration_ms: started.elapsed().as_millis() as u64,
            listeners,
            conflicts,
            adapters: Adapters {
                wsl: wsl_state,
                docker: docker_state,
            },
            warnings,
        }
    }
}

/// The project folder around a path the user named, with its manifests. No folders are ruled
/// out here: the user chose this one.
pub fn project_root_at(path: &str) -> Option<(String, Vec<String>)> {
    let fs = RealFs;
    let ctx = InferContext {
        fs: &fs,
        denied: Vec::new(),
        home: None,
    };
    ctx.root_for_path(&windows_path(path))
}
