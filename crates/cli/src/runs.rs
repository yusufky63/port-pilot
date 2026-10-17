//! `portctl run`, `portctl lease` and `portctl history`: managed runs, port leases and the
//! local timeline, sharing `portpilot.db` with the desktop app.

use std::collections::BTreeMap;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use portpilot_core::lease::{Lease, LeaseOwner, Want, path_key};
use portpilot_core::net::{PortRange, parse_port_range};
use portpilot_core::recipe::{DEFAULT_RANGE_SPAN, PortSpec, RECIPE_FILE};
use portpilot_core::time::unix_ms_to_filetime;
use portpilot_engine::leases::{self, LeaseUser, PortCheck};
use portpilot_engine::recipes::{self, LoadError, LoadedRecipe, display_path};
use portpilot_engine::runs::{self, OutputMode, RunSpec};
use portpilot_engine::store::{
    EventKind, EventQuery, HistoryEvent, HistoryPrefs, RunPort, Store, TrustEntry, now_ms,
};
use portpilot_engine::{doctor, paths};
use portpilot_win::control;
use portpilot_win::time::local_datetime;
use serde::Serialize;

use crate::{code, envelope, print_json, render};

fn open_store() -> Result<Store, i32> {
    Store::open(&paths::database_path()).map_err(|e| {
        eprintln!("portctl: {e}");
        code::INTERNAL
    })
}

fn start_dir(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

fn load_recipe(start: &Path) -> Result<LoadedRecipe, i32> {
    recipes::load(start).map_err(|e| {
        match &e {
            LoadError::NotFound(dir) => {
                eprintln!("No {RECIPE_FILE} in {dir} or above it (up to the repository root).");
                eprintln!("A minimal one:\n");
                eprintln!("  [ports.web]\n  preferred = 5173\n\n  [run.dev]\n  command = \"pnpm dev --port {{ports.web}}\"");
            }
            LoadError::Invalid(_) => eprintln!("portctl: {e}"),
        }
        code::RECIPE
    })
}

fn snapshot_check() -> (portpilot_core::Snapshot, Vec<PortRange>) {
    let snap = crate::discovery(crate::options(
        crate::Sources {
            no_wsl: false,
            no_docker: false,
        },
        false,
        false,
    ))
    .snapshot();
    (snap, doctor::excluded_tcp_ranges())
}

fn describe_users(users: &[LeaseUser]) -> String {
    users
        .iter()
        .map(|u| {
            let mut s = u.name.clone();
            if let Some(pid) = u.pid {
                s.push_str(&format!(" PID {pid}"));
            }
            if let Some(p) = &u.project {
                s.push_str(&format!(" ({p})"));
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn ask(question: &str) -> String {
    print!("{question}");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    let _ = std::io::stdin().lock().read_line(&mut answer);
    answer.trim().to_ascii_lowercase()
}

fn local_time(ms: i64) -> String {
    local_datetime(unix_ms_to_filetime(ms)).unwrap_or_default()
}

/// `portctl run [<name>]`.
pub fn cmd_run(
    name: Option<String>,
    path: Option<PathBuf>,
    yes: bool,
    next_free: bool,
    timeout: u64,
) -> i32 {
    let loaded = match load_recipe(&start_dir(path)) {
        Ok(l) => l,
        Err(c) => return c,
    };
    let ctx = &loaded.context;
    let names: Vec<&String> = loaded.recipe.runs.keys().collect();
    let run_name = match (name, names.as_slice()) {
        (Some(n), _) => n,
        (None, [only]) => (*only).clone(),
        (None, []) => {
            eprintln!(
                "{RECIPE_FILE} in {} has no [run.<name>] section.",
                ctx.folder
            );
            return code::RECIPE;
        }
        (None, many) => {
            eprintln!(
                "{RECIPE_FILE} has several runs; name one: {}",
                many.iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return code::INVALID;
        }
    };
    let Some(run) = loaded.recipe.runs.get(&run_name) else {
        eprintln!(
            "{RECIPE_FILE} has no [run.{run_name}]. Runs: {}",
            names
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        return code::RECIPE;
    };
    let folder = match recipes::run_folder(ctx, run) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("portctl: run.{run_name}.cwd: {e}");
            return code::RECIPE;
        }
    };
    let mut store = match open_store() {
        Ok(s) => s,
        Err(c) => return c,
    };
    let prefs = HistoryPrefs::load(&paths::data_dir());
    let interactive = std::io::stdin().is_terminal();

    let services = run.services();
    let mut ports = BTreeMap::new();
    if !services.is_empty() {
        let (snap, excluded) = snapshot_check();
        let check = PortCheck {
            snapshot: &snap,
            excluded: &excluded,
        };
        for service in services {
            let spec = &loaded.recipe.ports[&service];
            let mut lease = match leases::acquire(
                &mut store,
                &ctx.owner,
                &ctx.name,
                &service,
                spec,
                Want::Keep,
                &check,
            ) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("portctl: {e}");
                    return code::PORT_CONFLICT;
                }
            };
            let users: Vec<LeaseUser> = leases::users_of(&check, lease.port, &ctx.owner.worktree);
            if !users.is_empty() {
                eprintln!(
                    "Port {} ({service}) is in use by {}.",
                    lease.port,
                    describe_users(&users)
                );
                let move_it = next_free
                    || (interactive
                        && matches!(
                            ask("Allocate the next free port for this worktree? [y/N] ").as_str(),
                            "y" | "yes"
                        ));
                if !move_it {
                    eprintln!("Stop that listener, or rerun with --next-free.");
                    return code::PORT_CONFLICT;
                }
                lease = match leases::acquire(
                    &mut store,
                    &ctx.owner,
                    &ctx.name,
                    &service,
                    spec,
                    Want::Next,
                    &check,
                ) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("portctl: {e}");
                        return code::PORT_CONFLICT;
                    }
                };
                println!(
                    "{service} now leases port {} for this worktree.",
                    lease.port
                );
            }
            ports.insert(service, lease.port);
        }
    }
    let rendered = match run.render(&ports) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("portctl: {e}");
            return code::RECIPE;
        }
    };

    println!("Run `{run_name}` of {}", ctx.name);
    println!("  Folder   {folder}");
    println!("  Command  {}", rendered.command);
    for (k, v) in &rendered.env {
        println!("  Env      {k}={v}");
    }
    for (service, port) in &ports {
        println!("  Port     {service} {port} (leased to this worktree)");
    }

    let digest = match recipes::trust_digest(&run_name, run) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("portctl: {e}");
            return code::INTERNAL;
        }
    };
    let project_key = ctx.owner.project_key();
    let trusted = store
        .trust(&project_key, &run_name)
        .ok()
        .flatten()
        .filter(|t| t.digest == digest);
    match trusted {
        Some(t) => println!("  Trusted  since {}", local_time(t.trusted_at)),
        None if yes => {
            println!("  Running once (--yes); the command is not remembered as trusted.")
        }
        None if !interactive => {
            eprintln!(
                "PortPilot has not run this command before. Run it in a terminal to confirm, or pass --yes to run it once."
            );
            return code::INVALID;
        }
        None => {
            println!();
            println!("PortPilot has not run this command before. It runs with your rights.");
            match ask("[t] Trust and run   [o] Run once   [c] Cancel: ").as_str() {
                "t" | "trust" => {
                    if let Err(e) = store.set_trust(&TrustEntry {
                        project_key,
                        run: run_name.clone(),
                        digest,
                        folder: ctx.folder.clone(),
                        project: ctx.name.clone(),
                        trusted_at: now_ms(),
                    }) {
                        eprintln!("portctl: {e}");
                    }
                }
                "o" | "once" => {}
                _ => {
                    println!("Cancelled.");
                    return code::OK;
                }
            }
        }
    }

    let spec = RunSpec {
        name: run_name,
        project: ctx.name.clone(),
        folder,
        command: rendered.command,
        env: rendered.env,
        ports: ports
            .into_iter()
            .map(|(service, port)| RunPort { service, port })
            .collect(),
    };
    supervise(
        &store,
        &spec,
        prefs,
        Duration::from_secs(timeout.clamp(1, 120)),
    )
}

/// Runs the command in this console until it exits. The first CTRL+C sends CTRL+BREAK to the
/// run's process group; after the grace period a second CTRL+C ends the job.
fn supervise(store: &Store, spec: &RunSpec, prefs: HistoryPrefs, grace: Duration) -> i32 {
    let presses = control::catch_console_ctrl();
    let seen = presses.load(Ordering::SeqCst);
    let mut launched = match runs::launch(spec, OutputMode::Inherit) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("portctl: {e}");
            return code::INTERNAL;
        }
    };
    let id = runs::record_start(
        store,
        spec,
        launched.pid,
        launched.start,
        launched.started_at,
        prefs,
    )
    .map_err(|e| eprintln!("portctl: {e}"))
    .ok();
    println!(
        "Started PID {} in its own process group. CTRL+C stops it.\n",
        launched.pid
    );
    let mut handled = seen;
    let mut stop_sent: Option<Instant> = None;
    let mut warned = false;
    let mut ended_by: Option<&str> = None;
    let mut exit_code: Option<i32> = None;
    loop {
        if exit_code.is_none()
            && let Ok(Some(status)) = launched.child.try_wait()
        {
            exit_code = status.code();
        }
        if exit_code.is_some() && launched.job.active_processes() == 0 {
            break;
        }
        let now = presses.load(Ordering::SeqCst);
        if now != handled {
            handled = now;
            match stop_sent {
                None => {
                    eprintln!("\nportctl: sending CTRL+BREAK to the run's process group...");
                    control::ctrl_break_group(launched.pid);
                    stop_sent = Some(Instant::now());
                    ended_by = Some("stopped");
                }
                Some(t) if t.elapsed() >= Duration::from_millis(500) => {
                    eprintln!("portctl: ending the run's job.");
                    ended_by = Some("terminated");
                    let _ = launched.job.terminate(1);
                }
                Some(_) => {}
            }
        }
        if let Some(t) = stop_sent
            && !warned
            && t.elapsed() >= grace
            && launched.job.active_processes() > 0
        {
            warned = true;
            eprintln!(
                "portctl: still running after {} s. Press CTRL+C again to end it and every process it started.",
                grace.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if let Some(id) = &id
        && let Err(e) = runs::record_exit(store, id, spec, launched.pid, exit_code, ended_by, prefs)
    {
        eprintln!("portctl: {e}");
    }
    match (ended_by, exit_code) {
        (Some(_), _) => {
            println!("\nRun `{}` stopped.", spec.name);
            code::OK
        }
        (None, Some(c)) => {
            println!("\nRun `{}` exited with code {c}.", spec.name);
            c
        }
        (None, None) => code::INTERNAL,
    }
}

/// The lease owner for a folder: the recipe's project when there is one.
fn owner_for(start: &Path) -> (LeaseOwner, String, Option<LoadedRecipe>) {
    match recipes::load(start) {
        Ok(l) => (l.context.owner.clone(), l.context.name.clone(), Some(l)),
        Err(_) => {
            let dir = std::fs::canonicalize(start)
                .map(|p| display_path(&p))
                .unwrap_or_else(|_| start.display().to_string());
            let ctx = recipes::project_context(&dir, None);
            (ctx.owner, ctx.name, None)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LeaseOut {
    #[serde(flatten)]
    lease: Lease,
    users: Vec<LeaseUser>,
}

/// `portctl lease acquire <service>`: prints the port, so `$port = portctl lease acquire web`
/// works in scripts.
pub fn cmd_lease_acquire(
    service: String,
    path: Option<PathBuf>,
    preferred: Option<u16>,
    range: Option<String>,
    next: bool,
    json: bool,
) -> i32 {
    if !portpilot_core::recipe::valid_name(&service) {
        eprintln!("portctl: service names use letters, digits, - and _");
        return code::INVALID;
    }
    let (owner, project, recipe) = owner_for(&start_dir(path));
    let from_recipe = recipe
        .as_ref()
        .and_then(|r| r.recipe.ports.get(&service).cloned());
    let range = match range.as_deref().map(parse_port_range).transpose() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("portctl: {e}");
            return code::INVALID;
        }
    };
    let spec = match (from_recipe, preferred, range) {
        (_, Some(p), Some(r)) if !r.contains(p) => {
            eprintln!("portctl: --preferred {p} is outside {}-{}", r.start, r.end);
            return code::INVALID;
        }
        (_, Some(p), Some(r)) => PortSpec {
            preferred: p,
            range: r,
        },
        (_, Some(p), None) => PortSpec {
            preferred: p,
            range: PortRange {
                start: p,
                end: p.saturating_add(DEFAULT_RANGE_SPAN),
            },
        },
        (_, None, Some(r)) => PortSpec {
            preferred: r.start,
            range: r,
        },
        (Some(s), None, None) => s,
        (None, None, None) => {
            eprintln!(
                "portctl: no [ports.{service}] in {RECIPE_FILE}; pass --preferred or --range"
            );
            return code::RECIPE;
        }
    };
    let mut store = match open_store() {
        Ok(s) => s,
        Err(c) => return c,
    };
    let (snap, excluded) = snapshot_check();
    let check = PortCheck {
        snapshot: &snap,
        excluded: &excluded,
    };
    let want = if next { Want::Next } else { Want::Keep };
    let lease = match leases::acquire(&mut store, &owner, &project, &service, &spec, want, &check) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("portctl: {e}");
            return code::PORT_CONFLICT;
        }
    };
    let users = leases::users_of(&check, lease.port, &owner.worktree);
    let foreign: Vec<LeaseUser> = users.iter().filter(|u| !u.same_worktree).cloned().collect();
    let exit = if foreign.is_empty() {
        code::OK
    } else {
        code::PORT_CONFLICT
    };
    if json {
        print_json(&envelope(LeaseOut { lease, users }));
        return exit;
    }
    println!("{}", lease.port);
    if !foreign.is_empty() {
        eprintln!(
            "Port {} is in use by {}. `portctl lease acquire {service} --next` moves the lease.",
            lease.port,
            describe_users(&foreign)
        );
    }
    exit
}

/// `portctl lease release [<service>]`.
pub fn cmd_lease_release(service: Option<String>, path: Option<PathBuf>, json: bool) -> i32 {
    let (owner, _, _) = owner_for(&start_dir(path));
    let store = match open_store() {
        Ok(s) => s,
        Err(c) => return c,
    };
    let released = match store.release_leases(&owner.key(), service.as_deref()) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("portctl: {e}");
            return code::INTERNAL;
        }
    };
    if json {
        #[derive(Serialize)]
        struct Body {
            released: usize,
        }
        print_json(&envelope(Body { released }));
    } else if released == 0 {
        println!("No lease to release for {}.", owner.worktree);
    } else {
        println!(
            "Released {released} lease{} of {}.",
            if released == 1 { "" } else { "s" },
            owner.worktree
        );
    }
    if released == 0 {
        code::NO_LISTENER
    } else {
        code::OK
    }
}

/// `portctl lease list [--all]`.
pub fn cmd_lease_list(all: bool, path: Option<PathBuf>, json: bool) -> i32 {
    let Some(store) = Store::open_existing(&paths::database_path()) else {
        if json {
            #[derive(Serialize)]
            struct Body {
                leases: Vec<()>,
            }
            return print_json(&envelope(Body { leases: Vec::new() }));
        }
        println!("No leases yet.");
        return code::OK;
    };
    let (snap, excluded) = snapshot_check();
    let check = PortCheck {
        snapshot: &snap,
        excluded: &excluded,
    };
    let mut views = match leases::list(&store, &check) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("portctl: {e}");
            return code::INTERNAL;
        }
    };
    if !all {
        let (owner, _, _) = owner_for(&start_dir(path));
        let repo = owner.project_key();
        views.retain(|v| {
            path_key(v.lease.common_dir.as_deref().unwrap_or(&v.lease.worktree)) == repo
        });
    }
    if json {
        #[derive(Serialize)]
        struct Body {
            leases: Vec<leases::LeaseView>,
        }
        return print_json(&envelope(Body { leases: views }));
    }
    if views.is_empty() {
        println!(
            "No leases{}.",
            if all {
                ""
            } else {
                " for this repository; --all lists every repository"
            }
        );
        return code::OK;
    }
    let rows: Vec<Vec<String>> = views
        .iter()
        .map(|v| {
            let state = if v.stale {
                "worktree removed".to_string()
            } else if v.users.is_empty() {
                "free".to_string()
            } else if v.users.iter().all(|u| u.same_worktree) {
                format!("in use by this worktree ({})", describe_users(&v.users))
            } else {
                format!("in use by {}", describe_users(&v.users))
            };
            vec![
                v.lease.port.to_string(),
                v.lease.service.clone(),
                v.lease.project.clone(),
                v.lease.worktree.clone(),
                state,
            ]
        })
        .collect();
    print!(
        "{}",
        render::table(&["PORT", "SERVICE", "PROJECT", "WORKTREE", "STATE"], &rows)
    );
    code::OK
}

fn event_text(e: &HistoryEvent) -> String {
    let d = &e.data;
    let who = d
        .process
        .clone()
        .map(|p| match d.pid {
            Some(pid) => format!("{p} PID {pid}"),
            None => p,
        })
        .unwrap_or_default();
    let project = d
        .project
        .as_deref()
        .map(|p| format!(" · {p}"))
        .unwrap_or_default();
    match e.kind {
        EventKind::PortOpened => format!("opened by {who}{project}"),
        EventKind::PortClosed => format!("closed, was {who}{project}"),
        EventKind::RunStarted => format!(
            "run `{}` started{project}{}",
            d.run_name.as_deref().unwrap_or("?"),
            d.detail
                .as_deref()
                .map(|p| format!(" ({p})"))
                .unwrap_or_default()
        ),
        EventKind::RunExited => {
            let how = match d.detail.as_deref() {
                Some("stopped") => "stopped by PortPilot".to_string(),
                Some("terminated") => "ended by PortPilot".to_string(),
                Some("ownerExited") => "ended when its owner exited".to_string(),
                _ => match d.exit_code {
                    Some(c) => format!("exited with code {c}"),
                    None => "exited".to_string(),
                },
            };
            format!(
                "run `{}` {how}{project}",
                d.run_name.as_deref().unwrap_or("?")
            )
        }
    }
}

/// `portctl history`.
pub fn cmd_history(
    port: Option<u16>,
    kind: Option<String>,
    days: Option<u32>,
    limit: usize,
    json: bool,
) -> i32 {
    let kinds = match kind.as_deref() {
        None => Vec::new(),
        Some("runs") => vec![EventKind::RunStarted, EventKind::RunExited],
        Some("ports") => vec![EventKind::PortOpened, EventKind::PortClosed],
        Some(other) => {
            eprintln!("portctl: --kind is runs or ports, not {other}");
            return code::INVALID;
        }
    };
    let events = match Store::open_existing(&paths::database_path()) {
        Some(store) => match store.events(&EventQuery {
            kinds,
            port,
            since: days.map(|d| now_ms() - i64::from(d) * 24 * 60 * 60 * 1000),
            before: None,
            limit: limit.clamp(1, 5000),
        }) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("portctl: {e}");
                return code::INTERNAL;
            }
        },
        None => Vec::new(),
    };
    if json {
        #[derive(Serialize)]
        struct Body {
            events: Vec<HistoryEvent>,
        }
        return print_json(&envelope(Body { events }));
    }
    if events.is_empty() {
        println!(
            "No history yet. The desktop app records port changes while it runs; runs are recorded by the app and by portctl run."
        );
        return code::OK;
    }
    let rows: Vec<Vec<String>> = events
        .iter()
        .map(|e| {
            vec![
                local_time(e.at),
                e.data.port.map(|p| p.to_string()).unwrap_or_default(),
                event_text(e),
            ]
        })
        .collect();
    print!("{}", render::table(&["TIME", "PORT", "EVENT"], &rows));
    code::OK
}
