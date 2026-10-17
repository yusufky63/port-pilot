//! `portctl`: the command-line face of PortPilot.
//!
//! Exit codes: 0 success, 2 invalid arguments, 3 no matching listener, 4 ambiguous ownership,
//! 5 access denied or refused, 6 identity changed (PID reuse), 7 graceful timeout,
//! 8 WSL or Docker unavailable, 9 port conflict, 10 internal error, 11 recipe missing or
//! invalid. `portctl run` ends with the run's own exit code when the run exits by itself.

mod render;
mod runs;

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use portpilot_core::net::{PortRange, parse_port_range};
use portpilot_core::{Listener, ProcessClass, Protocol, SCHEMA_VERSION, StopOutcome};
use portpilot_engine::doctor::{self, PortDiagnosis};
use portpilot_engine::store::Store;
use portpilot_engine::{Discovery, Options, StopRequest, git, paths, projects};
use serde::Serialize;

mod code {
    pub const OK: i32 = 0;
    pub const INVALID: i32 = 2;
    pub const NO_LISTENER: i32 = 3;
    pub const AMBIGUOUS: i32 = 4;
    pub const ACCESS_DENIED: i32 = 5;
    pub const IDENTITY_CHANGED: i32 = 6;
    pub const GRACEFUL_TIMEOUT: i32 = 7;
    pub const RUNTIME_UNAVAILABLE: i32 = 8;
    pub const PORT_CONFLICT: i32 = 9;
    pub const INTERNAL: i32 = 10;
    pub const RECIPE: i32 = 11;
}

#[derive(Parser)]
#[command(
    name = "portctl",
    version,
    about = "Explain and control local development ports on Windows",
    after_help = "Exit codes: 0 ok, 2 invalid arguments, 3 no listener, 4 ambiguous owner, 5 access denied, 6 identity changed, 7 graceful timeout, 8 runtime unavailable, 9 port conflict, 10 internal error, 11 recipe missing or invalid."
)]
struct Cli {
    /// Show command lines without masking secrets.
    #[arg(long, global = true)]
    no_redact: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Clone, Copy)]
struct Sources {
    /// Skip WSL distros.
    #[arg(long)]
    no_wsl: bool,
    /// Skip Docker Desktop.
    #[arg(long)]
    no_docker: bool,
}

#[derive(Subcommand)]
enum Command {
    /// List listening ports and who owns them.
    List {
        #[arg(long)]
        json: bool,
        /// Include Windows services and system processes.
        #[arg(long)]
        all: bool,
        /// Include UDP sockets.
        #[arg(long)]
        udp: bool,
        #[command(flatten)]
        sources: Sources,
    },
    /// Explain who owns a port and what can be done about it.
    Who {
        port: u16,
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        sources: Sources,
    },
    /// Show the full process chain behind a port.
    Tree {
        port: u16,
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        sources: Sources,
    },
    /// Stop the owner of a port: a graceful request first, terminate only with --force.
    Stop {
        port: u16,
        /// Pick one owner when several listen on the port.
        #[arg(long)]
        pid: Option<u32>,
        /// Terminate the process if it is still running after the grace period.
        #[arg(long)]
        force: bool,
        /// With --force, terminate its child processes too.
        #[arg(long)]
        tree: bool,
        /// Do not ask before --force (required when no one can answer).
        #[arg(long, short)]
        yes: bool,
        /// Grace period in seconds.
        #[arg(long, default_value_t = 5)]
        timeout: u64,
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        sources: Sources,
    },
    /// Find free TCP ports, checked against listeners, Windows' reserved ranges and a bind.
    Find {
        #[arg(long, default_value = "3000-3999")]
        range: String,
        #[arg(long, default_value_t = 1)]
        count: usize,
        #[arg(long)]
        json: bool,
    },
    /// Check the machine (Windows, WSL, Docker, reserved ports) or one port.
    Doctor {
        #[command(subcommand)]
        what: Option<DoctorWhat>,
        #[arg(long, global = true)]
        json: bool,
    },
    /// Inspect a project folder: Git state, worktrees and its listeners.
    Project {
        #[command(subcommand)]
        what: ProjectWhat,
    },
    /// Listeners grouped by repository and worktree.
    Projects {
        #[arg(long)]
        json: bool,
    },
    /// Start a run from .portpilot.toml in this console, after showing the exact command.
    Run {
        /// The run's name (`[run.<name>]`); optional when the recipe has one run.
        name: Option<String>,
        /// The project folder (the current one by default).
        #[arg(long)]
        path: Option<PathBuf>,
        /// Run once without asking when the command is not trusted yet.
        #[arg(long, short)]
        yes: bool,
        /// When a leased port is in use, move the lease to the next free port.
        #[arg(long)]
        next_free: bool,
        /// Grace period in seconds after the first CTRL+C.
        #[arg(long, default_value_t = 5)]
        timeout: u64,
    },
    /// Port leases per repository, worktree and service.
    Lease {
        #[command(subcommand)]
        what: LeaseWhat,
    },
    /// Runs, exits and port changes of the last 14 days.
    History {
        /// Only events of this port.
        #[arg(long)]
        port: Option<u16>,
        /// `runs` or `ports`.
        #[arg(long)]
        kind: Option<String>,
        /// Only the last N days.
        #[arg(long)]
        days: Option<u32>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum LeaseWhat {
    /// Lease a port for a service of this worktree and print it.
    Acquire {
        service: String,
        #[arg(long)]
        path: Option<PathBuf>,
        /// Overrides `[ports.<service>] preferred`.
        #[arg(long)]
        preferred: Option<u16>,
        /// Overrides `[ports.<service>] range`, such as 5173-5199.
        #[arg(long)]
        range: Option<String>,
        /// Move the lease to the next free port.
        #[arg(long)]
        next: bool,
        #[arg(long)]
        json: bool,
    },
    /// Release one service's lease, or every lease of this worktree.
    Release {
        service: Option<String>,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Leases of this repository, or of every repository with --all.
    List {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum DoctorWhat {
    /// Why a port is busy or unusable, and free ports nearby.
    Port { port: u16 },
    /// Windows version, user, Git and reserved port ranges.
    Windows,
    /// WSL distros and their state.
    Wsl,
    /// Docker Desktop's engine and containers.
    Docker,
}

#[derive(Subcommand)]
enum ProjectWhat {
    /// Inspect a folder (the current one by default).
    Inspect {
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

/// Discovery that also knows the runs PortPilot started (from the app or `portctl run`), so
/// their listeners are attributed to the run's folder exactly.
fn discovery(opts: Options) -> Discovery {
    let mut d = Discovery::new(opts);
    if let Some(store) = Store::open_existing(&paths::database_path()) {
        let history = portpilot_engine::store::HistoryPrefs::load(&paths::data_dir()).enabled;
        d.set_managed(portpilot_engine::runs::managed_from_store(&store, history));
    }
    d
}

fn options(sources: Sources, udp: bool, raw: bool) -> Options {
    Options {
        udp,
        wsl: !sources.no_wsl,
        docker: !sources.no_docker,
        raw_command_lines: raw,
        ..Options::default()
    }
}

fn print_json<T: Serialize>(value: &T) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(s) => {
            println!("{s}");
            code::OK
        }
        Err(e) => {
            eprintln!("portctl: {e}");
            code::INTERNAL
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Envelope<T: Serialize> {
    schema_version: u32,
    #[serde(flatten)]
    body: T,
}

fn envelope<T: Serialize>(body: T) -> Envelope<T> {
    Envelope {
        schema_version: SCHEMA_VERSION,
        body,
    }
}

fn is_visible(l: &Listener, all: bool, udp: bool) -> bool {
    if l.protocol == Protocol::Udp && !udp {
        return false;
    }
    all || !l
        .process
        .as_ref()
        .is_some_and(|p| matches!(p.class, ProcessClass::System | ProcessClass::Service))
}

fn cmd_list(json: bool, all: bool, udp: bool, sources: Sources, raw: bool) -> i32 {
    let mut snap = discovery(options(sources, udp, raw)).snapshot();
    let total = snap.listeners.len();
    snap.listeners.retain(|l| is_visible(l, all, udp));
    if json {
        return print_json(&snap);
    }
    let refs: Vec<&Listener> = snap.listeners.iter().collect();
    print!("{}", render::list(&refs));
    let hidden = total - snap.listeners.len();
    println!();
    println!(
        "{} listeners, {} conflicts. {}. {}.",
        snap.listeners.len(),
        snap.conflicts.len(),
        render::adapter_line("WSL", &snap.adapters.wsl),
        render::adapter_line("Docker", &snap.adapters.docker)
    );
    if hidden > 0 {
        println!("{hidden} system and service listeners hidden; --all shows them.");
    }
    for w in &snap.warnings {
        eprintln!("warning: {w}");
    }
    code::OK
}

fn diagnose(port: u16, sources: Sources, raw: bool) -> PortDiagnosis {
    let snap = discovery(options(sources, false, raw)).snapshot();
    doctor::diagnose_port(port, &snap, &doctor::excluded_tcp_ranges())
}

fn cmd_who(port: u16, json: bool, sources: Sources, raw: bool) -> i32 {
    let d = diagnose(port, sources, raw);
    if json {
        return print_json(&envelope(&d));
    }
    print!("{}", render::who(&d));
    if d.listeners.is_empty() {
        code::NO_LISTENER
    } else {
        code::OK
    }
}

fn cmd_tree(port: u16, json: bool, sources: Sources, raw: bool) -> i32 {
    let snap = discovery(options(sources, false, raw)).snapshot();
    let owners: Vec<&Listener> = snap
        .listeners
        .iter()
        .filter(|l| l.port == port && l.protocol == Protocol::Tcp)
        .collect();
    if owners.is_empty() {
        eprintln!("No listener on TCP port {port}.");
        return code::NO_LISTENER;
    }
    if json {
        #[derive(Serialize)]
        struct Body<'a> {
            listeners: Vec<&'a Listener>,
        }
        return print_json(&envelope(Body { listeners: owners }));
    }
    for l in owners {
        println!("{}", render::tree(l));
    }
    code::OK
}

fn confirm(question: &str) -> bool {
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn describe(l: &Listener) -> String {
    if let Some(c) = &l.container {
        return format!("container {} ({})", c.name, c.image);
    }
    match &l.process {
        Some(p) => format!(
            "{} PID {} on {} port {}",
            p.name,
            p.pid,
            render::runtime_label(l),
            l.port
        ),
        None => format!("the owner of port {}", l.port),
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_stop(
    port: u16,
    pid: Option<u32>,
    force: bool,
    tree: bool,
    yes: bool,
    timeout: u64,
    json: bool,
    sources: Sources,
    raw: bool,
) -> i32 {
    let snap = discovery(options(sources, false, raw)).snapshot();
    let mut owners: Vec<&Listener> = snap
        .listeners
        .iter()
        .filter(|l| l.port == port && l.protocol == Protocol::Tcp)
        .collect();
    if let Some(pid) = pid {
        owners.retain(|l| l.process.as_ref().is_some_and(|p| p.pid == pid));
    }
    let l = match owners.as_slice() {
        [] => {
            eprintln!(
                "No listener on TCP port {port}{}.",
                pid.map(|p| format!(" with PID {p}")).unwrap_or_default()
            );
            return code::NO_LISTENER;
        }
        [one] => *one,
        many => {
            eprintln!("Several owners listen on port {port}; pick one with --pid:");
            for l in many {
                eprintln!("  {}", describe(l));
            }
            return code::AMBIGUOUS;
        }
    };
    let Some(target) = l.stop.target.clone() else {
        let why = l
            .stop
            .blocked
            .map(render::block_reason)
            .unwrap_or("its identity is unknown");
        eprintln!("PortPilot will not stop {}: {why}.", describe(l));
        return code::ACCESS_DENIED;
    };
    if force && !yes {
        if !std::io::stdin().is_terminal() {
            eprintln!("--force needs --yes when no one can confirm.");
            return code::INVALID;
        }
        if !confirm(&format!(
            "Terminate {} if it ignores the graceful request?",
            describe(l)
        )) {
            eprintln!("Cancelled.");
            return code::OK;
        }
    }
    if !json {
        println!("Stopping {}...", describe(l));
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("portctl.exe"));
    let report = portpilot_engine::stop(
        &StopRequest {
            target,
            force,
            tree,
            grace: Duration::from_secs(timeout.clamp(1, 120)),
        },
        &exe,
    );
    let exit = match report.outcome {
        StopOutcome::Stopped | StopOutcome::Forced | StopOutcome::AlreadyExited => code::OK,
        StopOutcome::IdentityChanged => code::IDENTITY_CHANGED,
        StopOutcome::GracefulTimeout | StopOutcome::GracefulUnavailable => code::GRACEFUL_TIMEOUT,
        StopOutcome::AccessDenied | StopOutcome::Refused => code::ACCESS_DENIED,
        StopOutcome::RuntimeUnavailable => code::RUNTIME_UNAVAILABLE,
        StopOutcome::Failed => code::INTERNAL,
    };
    if json {
        print_json(&envelope(&report));
        return exit;
    }
    println!("{}", render::stop_report(&report));
    if exit == code::OK {
        let still = portpilot_win::net::tcp_listeners()
            .map(|s| s.iter().any(|s| s.port == port))
            .unwrap_or(false);
        if still {
            println!("Port {port} is still in use; `portctl who {port}` shows by whom.");
        } else {
            println!("Port {port} is free.");
        }
    }
    exit
}

fn cmd_find(range: &str, count: usize, json: bool) -> i32 {
    let range: PortRange = match parse_port_range(range) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("portctl: {e}");
            return code::INVALID;
        }
    };
    let snap = discovery(Options {
        udp: false,
        wsl: true,
        docker: true,
        ..Options::default()
    })
    .snapshot();
    let ports = doctor::find_free(
        range,
        count.clamp(1, 100),
        &snap,
        &doctor::excluded_tcp_ranges(),
    );
    if json {
        #[derive(Serialize)]
        struct Body {
            ports: Vec<u16>,
        }
        return print_json(&envelope(Body { ports }));
    }
    if ports.is_empty() {
        eprintln!("No free port in {}-{}.", range.start, range.end);
        return code::NO_LISTENER;
    }
    for p in ports {
        println!("{p}");
    }
    code::OK
}

fn cmd_doctor(what: Option<DoctorWhat>, json: bool, raw: bool) -> i32 {
    if let Some(DoctorWhat::Port { port }) = what {
        return cmd_who(
            port,
            json,
            Sources {
                no_wsl: false,
                no_docker: false,
            },
            raw,
        );
    }
    let env = doctor::environment();
    if json {
        return print_json(&envelope(&env));
    }
    match what {
        Some(DoctorWhat::Wsl) => {
            println!("{}", render::adapter_line("WSL", &env.wsl));
            if let Some(v) = &env.wsl_version {
                println!("  {v}");
            }
        }
        Some(DoctorWhat::Docker) => {
            println!("{}", render::adapter_line("Docker", &env.docker));
            if let Some(v) = &env.docker_version {
                println!("  Engine {v}");
            }
        }
        _ => print!("{}", render::environment(&env)),
    }
    code::OK
}

fn cmd_project_inspect(path: Option<PathBuf>, json: bool, raw: bool) -> i32 {
    let path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let text = path
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();
    let Some((root, manifests)) = portpilot_engine::discover::project_root_at(&text) else {
        eprintln!("No project manifest or Git repository around {text}.");
        return code::NO_LISTENER;
    };
    let info = git::info(&root);
    let worktrees = info
        .as_ref()
        .map(|g| git::worktrees(&g.top_level))
        .unwrap_or_default();
    let snap = discovery(options(
        Sources {
            no_wsl: true,
            no_docker: false,
        },
        false,
        raw,
    ))
    .snapshot();
    let same = |p: &str| portpilot_core::git::same_path(p, &root);
    let listeners: Vec<&Listener> = snap
        .listeners
        .iter()
        .filter(|l| {
            l.project.as_ref().is_some_and(|p| {
                same(&p.root)
                    || info.as_ref().zip(p.git.as_ref()).is_some_and(|(a, b)| {
                        portpilot_core::git::same_path(&a.top_level, &b.top_level)
                    })
            })
        })
        .collect();
    if json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body<'a> {
            root: &'a str,
            manifests: &'a [String],
            git: Option<&'a portpilot_core::GitInfo>,
            worktrees: Vec<serde_json::Value>,
            listeners: Vec<&'a Listener>,
        }
        let wts = worktrees
            .iter()
            .map(|w| serde_json::json!({"path": w.path, "branch": w.branch, "head": w.head, "detached": w.detached, "locked": w.locked, "prunable": w.prunable}))
            .collect();
        return print_json(&envelope(Body {
            root: &root,
            manifests: &manifests,
            git: info.as_ref(),
            worktrees: wts,
            listeners,
        }));
    }
    println!("Project: {root}");
    println!("Manifests: {}", manifests.join(", "));
    match &info {
        Some(g) => {
            println!(
                "Git: {} / {} @ {}{}",
                g.repo_name,
                g.branch.clone().unwrap_or_else(|| "detached HEAD".into()),
                g.head.clone().unwrap_or_else(|| "no commits".into()),
                if g.dirty == Some(true) {
                    ", uncommitted changes"
                } else {
                    ""
                }
            );
            if worktrees.len() > 1 {
                println!("Worktrees:");
                for w in &worktrees {
                    println!(
                        "  {:<28} {}",
                        w.branch.clone().unwrap_or_else(|| "detached".into()),
                        w.path
                    );
                }
            }
        }
        None => println!("Git: not a repository"),
    }
    if listeners.is_empty() {
        println!("Listeners: none found for this project");
    } else {
        println!();
        print!("{}", render::list(&listeners));
    }
    code::OK
}

fn cmd_projects(json: bool, raw: bool) -> i32 {
    let snap = discovery(options(
        Sources {
            no_wsl: false,
            no_docker: false,
        },
        false,
        raw,
    ))
    .snapshot();
    let groups = projects::overview(&snap);
    if json {
        #[derive(Serialize)]
        struct Body {
            projects: Vec<projects::ProjectGroup>,
        }
        return print_json(&envelope(Body { projects: groups }));
    }
    print!("{}", render::projects(&groups));
    code::OK
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = portpilot_engine::helper_mode(&args) {
        std::process::exit(code);
    }
    let cli = Cli::parse();
    let raw = cli.no_redact;
    let exit = match cli.command {
        Command::List {
            json,
            all,
            udp,
            sources,
        } => cmd_list(json, all, udp, sources, raw),
        Command::Who {
            port,
            json,
            sources,
        } => cmd_who(port, json, sources, raw),
        Command::Tree {
            port,
            json,
            sources,
        } => cmd_tree(port, json, sources, raw),
        Command::Stop {
            port,
            pid,
            force,
            tree,
            yes,
            timeout,
            json,
            sources,
        } => cmd_stop(port, pid, force, tree, yes, timeout, json, sources, raw),
        Command::Find { range, count, json } => cmd_find(&range, count, json),
        Command::Doctor { what, json } => cmd_doctor(what, json, raw),
        Command::Project {
            what: ProjectWhat::Inspect { path, json },
        } => cmd_project_inspect(path, json, raw),
        Command::Projects { json } => cmd_projects(json, raw),
        Command::Run {
            name,
            path,
            yes,
            next_free,
            timeout,
        } => runs::cmd_run(name, path, yes, next_free, timeout),
        Command::Lease { what } => match what {
            LeaseWhat::Acquire {
                service,
                path,
                preferred,
                range,
                next,
                json,
            } => runs::cmd_lease_acquire(service, path, preferred, range, next, json),
            LeaseWhat::Release {
                service,
                path,
                json,
            } => runs::cmd_lease_release(service, path, json),
            LeaseWhat::List { all, path, json } => runs::cmd_lease_list(all, path, json),
        },
        Command::History {
            port,
            kind,
            days,
            limit,
            json,
        } => runs::cmd_history(port, kind, days, limit, json),
    };
    std::process::exit(exit);
}
