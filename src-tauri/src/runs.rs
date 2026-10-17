//! Commands for managed runs, port leases, trusted commands and history.
//!
//! A recipe is read only from a folder PortPilot found itself or the user picked in the
//! native folder dialog, and only when the user asks. A run starts only from `run_start`, which
//! checks that the recipe still says what the user reviewed (its digest) and that the ports are
//! the ones they saw.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use portpilot_core::lease::Want;
use portpilot_core::output::OutputChunk;
use portpilot_core::{Snapshot, StopReport};
use portpilot_engine::doctor;
use portpilot_engine::leases::{self, LeaseUser, LeaseView, PortCheck};
use portpilot_engine::recipes::{self, LoadError, LoadedRecipe};
use portpilot_engine::runs::{RunInfo, RunSpec};
use portpilot_engine::store::{
    EventKind, EventQuery, HistoryEvent, RecentFolder, RunPort, TrustEntry, now_ms,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

type CmdResult<T> = Result<T, String>;

fn known_folder(state: &AppState, path: &str) -> CmdResult<()> {
    if state.is_known_path(path) {
        Ok(())
    } else {
        Err("PortPilot only reads recipes in folders it found itself or you picked".into())
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
}

fn snapshot(state: &AppState) -> CmdResult<std::sync::Arc<Snapshot>> {
    state
        .snapshot
        .read()
        .clone()
        .ok_or_else(|| "no snapshot yet".into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    key: String,
    value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeRunView {
    name: String,
    /// As written in the recipe, with placeholders.
    command: String,
    cwd: Option<String>,
    env: Vec<EnvVar>,
    services: Vec<String>,
    trusted: bool,
    trusted_at: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipePortView {
    service: String,
    preferred: u16,
    range_start: u16,
    range_end: u16,
    leased: Option<u16>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeView {
    folder: String,
    project: String,
    worktree: String,
    common_dir: Option<String>,
    branch: Option<String>,
    linked_worktree: bool,
    runs: Vec<RecipeRunView>,
    ports: Vec<RecipePortView>,
}

/// A recipe, or why there is none: `recipe` and `error` both empty means no file.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeResult {
    folder: String,
    recipe: Option<RecipeView>,
    error: Option<String>,
}

fn recipe_view(state: &AppState, loaded: &LoadedRecipe) -> RecipeView {
    let ctx = &loaded.context;
    let key = ctx.owner.key();
    let project_key = ctx.owner.project_key();
    let (leases, trust) = state
        .with_store(|s| Ok((s.leases()?, s.trusted()?)))
        .unwrap_or_default();
    RecipeView {
        folder: ctx.folder.clone(),
        project: ctx.name.clone(),
        worktree: ctx.owner.worktree.clone(),
        common_dir: ctx.owner.common_dir.clone(),
        branch: ctx.branch.clone(),
        linked_worktree: ctx.linked_worktree,
        runs: loaded
            .recipe
            .runs
            .iter()
            .map(|(name, run)| {
                let digest = recipes::trust_digest(name, run).ok();
                let t = trust.iter().find(|t| {
                    t.project_key == project_key
                        && &t.run == name
                        && Some(&t.digest) == digest.as_ref()
                });
                RecipeRunView {
                    name: name.clone(),
                    command: run.command.clone(),
                    cwd: run.cwd.clone(),
                    env: run
                        .env
                        .iter()
                        .map(|(k, v)| EnvVar {
                            key: k.clone(),
                            value: v.clone(),
                        })
                        .collect(),
                    services: run.services(),
                    trusted: t.is_some(),
                    trusted_at: t.map(|t| t.trusted_at),
                }
            })
            .collect(),
        ports: loaded
            .recipe
            .ports
            .iter()
            .map(|(service, spec)| RecipePortView {
                service: service.clone(),
                preferred: spec.preferred,
                range_start: spec.range.start,
                range_end: spec.range.end,
                leased: leases
                    .iter()
                    .find(|l| l.owner_key == key && &l.service == service)
                    .map(|l| l.port),
            })
            .collect(),
    }
}

/// Opens the native folder dialog. The folder the user picks becomes one PortPilot may read a
/// recipe from.
#[tauri::command]
pub async fn pick_project_folder(app: AppHandle) -> CmdResult<Option<String>> {
    let owner = app
        .get_webview_window("main")
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
        .unwrap_or(0);
    let title = if app.state::<AppState>().ui_language.read().as_str() == "tr" {
        "Proje klasörünü seçin"
    } else {
        "Choose a project folder"
    };
    let picked = blocking(move || portpilot_win::dialog::pick_folder(owner, title)).await??;
    if let Some(p) = &picked {
        app.state::<AppState>().remember_path(p);
    }
    Ok(picked)
}

/// Folders the user ran something in or trusted a command for, newest first.
#[tauri::command]
pub fn recent_projects(state: State<AppState>) -> CmdResult<Vec<RecentFolder>> {
    let list: Vec<RecentFolder> = state
        .with_store(|s| s.recent_folders(12))?
        .into_iter()
        .filter(|f| Path::new(&f.folder).is_dir())
        .collect();
    for f in &list {
        state.remember_path(&f.folder);
    }
    Ok(list)
}

#[tauri::command]
pub async fn recipe_load(app: AppHandle, path: String) -> CmdResult<RecipeResult> {
    known_folder(&app.state::<AppState>(), &path)?;
    let p = path.clone();
    let loaded = blocking(move || recipes::load(Path::new(&p))).await?;
    let state = app.state::<AppState>();
    Ok(match loaded {
        Ok(l) => {
            state.remember_path(&l.context.folder);
            RecipeResult {
                folder: l.context.folder.clone(),
                recipe: Some(recipe_view(&state, &l)),
                error: None,
            }
        }
        Err(LoadError::NotFound(_)) => RecipeResult {
            folder: path,
            recipe: None,
            error: None,
        },
        Err(e @ LoadError::Invalid(_)) => RecipeResult {
            folder: path,
            recipe: None,
            error: Some(e.to_string()),
        },
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanPort {
    service: String,
    /// `None` when no port in the range is free.
    port: Option<u16>,
    /// The worktree already holds this lease.
    leased: bool,
    range_start: u16,
    range_end: u16,
    /// Who listens on the port now; anyone here is a conflict.
    users: Vec<LeaseUser>,
}

/// Exactly what will run, for the confirmation dialog.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPlan {
    run: String,
    project: String,
    /// The recipe folder, which identifies the run for `run_start`.
    recipe_folder: String,
    /// The exact working directory.
    folder: String,
    worktree: String,
    branch: Option<String>,
    command: String,
    env: Vec<EnvVar>,
    ports: Vec<PlanPort>,
    digest: String,
    trusted: bool,
    trusted_at: Option<i64>,
    /// Why the run cannot start, such as an exhausted port range.
    blocked: Option<String>,
}

fn plan(state: &AppState, loaded: &LoadedRecipe, run_name: &str) -> CmdResult<RunPlan> {
    let ctx = &loaded.context;
    let run = loaded
        .recipe
        .runs
        .get(run_name)
        .ok_or_else(|| format!("the recipe has no [run.{run_name}]"))?;
    let folder = recipes::run_folder(ctx, run)?;
    let digest = recipes::trust_digest(run_name, run)?;
    let snap = snapshot(state)?;
    let services = run.services();
    let excluded = if services.is_empty() {
        Vec::new()
    } else {
        doctor::excluded_tcp_ranges()
    };
    let check = PortCheck {
        snapshot: &snap,
        excluded: &excluded,
    };
    let mut ports = Vec::new();
    let mut values = BTreeMap::new();
    let mut blocked = None;
    if !services.is_empty() {
        state.with_store(|store| {
            for service in &services {
                let spec = &loaded.recipe.ports[service];
                let proposal = leases::propose(store, &ctx.owner, service, spec, &check)?;
                if proposal.is_none() {
                    blocked = Some(format!(
                        "no free port for {service} in {}-{}",
                        spec.range.start, spec.range.end
                    ));
                }
                if let Some((port, _)) = proposal {
                    values.insert(service.clone(), port);
                }
                ports.push(PlanPort {
                    service: service.clone(),
                    port: proposal.map(|p| p.0),
                    leased: proposal.is_some_and(|p| p.1),
                    range_start: spec.range.start,
                    range_end: spec.range.end,
                    users: proposal
                        .map(|(p, _)| leases::users_of(&check, p, &ctx.owner.worktree))
                        .unwrap_or_default(),
                });
            }
            Ok(())
        })?;
    }
    let trust = state
        .with_store(|s| s.trust(&ctx.owner.project_key(), run_name))
        .ok()
        .flatten()
        .filter(|t| t.digest == digest);
    let (command, env) = if blocked.is_none() {
        let r = run.render(&values)?;
        (r.command, r.env)
    } else {
        (
            run.command.clone(),
            run.env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )
    };
    Ok(RunPlan {
        run: run_name.to_string(),
        project: ctx.name.clone(),
        recipe_folder: ctx.folder.clone(),
        folder,
        worktree: ctx.owner.worktree.clone(),
        branch: ctx.branch.clone(),
        command,
        env: env
            .into_iter()
            .map(|(key, value)| EnvVar { key, value })
            .collect(),
        ports,
        digest,
        trusted: trust.is_some(),
        trusted_at: trust.map(|t| t.trusted_at),
        blocked,
    })
}

fn load_known(app: &AppHandle, path: &str) -> CmdResult<LoadedRecipe> {
    known_folder(&app.state::<AppState>(), path)?;
    recipes::load(Path::new(path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_plan(app: AppHandle, path: String, run: String) -> CmdResult<RunPlan> {
    let a = app.clone();
    blocking(move || {
        let loaded = load_known(&a, &path)?;
        plan(&a.state::<AppState>(), &loaded, &run)
    })
    .await?
}

/// "Allocate the next free port for this worktree": moves the lease past the port in use.
#[tauri::command]
pub async fn run_reallocate(
    app: AppHandle,
    path: String,
    run: String,
    service: String,
) -> CmdResult<RunPlan> {
    let a = app.clone();
    blocking(move || {
        let loaded = load_known(&a, &path)?;
        let state = a.state::<AppState>();
        let spec = loaded
            .recipe
            .ports
            .get(&service)
            .ok_or_else(|| format!("the recipe has no [ports.{service}]"))?;
        let snap = snapshot(&state)?;
        let excluded = doctor::excluded_tcp_ranges();
        let check = PortCheck {
            snapshot: &snap,
            excluded: &excluded,
        };
        state.with_store(|store| {
            leases::acquire(
                store,
                &loaded.context.owner,
                &loaded.context.name,
                &service,
                spec,
                Want::Next,
                &check,
            )
        })?;
        plan(&state, &loaded, &run)
    })
    .await?
}

/// Starts a run the user confirmed. `digest` and `ports` are what the dialog showed; if the
/// recipe changed since, nothing runs.
#[tauri::command]
pub async fn run_start(
    app: AppHandle,
    path: String,
    run: String,
    digest: String,
    ports: BTreeMap<String, u16>,
    trust: bool,
) -> CmdResult<RunInfo> {
    let a = app.clone();
    let info = blocking(move || {
        let loaded = load_known(&a, &path)?;
        let state = a.state::<AppState>();
        let ctx = &loaded.context;
        let recipe_run = loaded
            .recipe
            .runs
            .get(&run)
            .ok_or_else(|| format!("the recipe has no [run.{run}]"))?;
        if recipes::trust_digest(&run, recipe_run)? != digest {
            return Err("the recipe changed since you reviewed it; review the run again".into());
        }
        let services = recipe_run.services();
        if services.len() != ports.len() || services.iter().any(|s| !ports.contains_key(s)) {
            return Err("the ports do not match the recipe; review the run again".into());
        }
        for (service, port) in &ports {
            if !loaded.recipe.ports[service].range.contains(*port) {
                return Err(format!("port {port} is outside the range of {service}"));
            }
        }
        let folder = recipes::run_folder(ctx, recipe_run)?;
        let rendered = recipe_run.render(&ports)?;
        if !ports.is_empty() || trust {
            state.with_store(|store| {
                for (service, port) in &ports {
                    leases::claim(store, &ctx.owner, &ctx.name, service, *port)?;
                }
                if trust {
                    store.set_trust(&TrustEntry {
                        project_key: ctx.owner.project_key(),
                        run: run.clone(),
                        digest: digest.clone(),
                        folder: ctx.folder.clone(),
                        project: ctx.name.clone(),
                        trusted_at: now_ms(),
                    })?;
                }
                Ok(())
            })?;
        }
        state.remember_path(&folder);
        state.runs.start(RunSpec {
            name: run,
            project: ctx.name.clone(),
            folder,
            command: rendered.command,
            env: rendered.env,
            ports: ports
                .into_iter()
                .map(|(service, port)| RunPort { service, port })
                .collect(),
        })
    })
    .await??;
    let state = app.state::<AppState>();
    state.wake.notify();
    Ok(info)
}

#[tauri::command]
pub fn runs_list(state: State<AppState>) -> Vec<RunInfo> {
    state.runs.list(!state.settings.read().mask_secrets)
}

#[tauri::command]
pub fn run_output(state: State<AppState>, run_id: String, after: u64) -> CmdResult<OutputChunk> {
    state
        .runs
        .output(&run_id, after, 2000)
        .ok_or_else(|| "this run is gone".into())
}

#[tauri::command]
pub fn run_output_text(state: State<AppState>, run_id: String) -> CmdResult<String> {
    state
        .runs
        .output_text(&run_id)
        .ok_or_else(|| "this run is gone".into())
}

/// Graceful first; `force` ends the run's job, and only the user's second decision sends it.
#[tauri::command]
pub async fn run_stop(app: AppHandle, run_id: String, force: bool) -> CmdResult<StopReport> {
    let state = app.state::<AppState>();
    let grace = Duration::from_secs(u64::from(state.settings.read().grace_seconds));
    let runs = state.runs.clone();
    let report = blocking(move || runs.stop(&run_id, grace, force)).await??;
    state
        .invalidate
        .store(true, std::sync::atomic::Ordering::Relaxed);
    state.wake.notify();
    Ok(report)
}

#[tauri::command]
pub fn run_forget(state: State<AppState>, run_id: String) -> bool {
    state.runs.forget(&run_id)
}

#[tauri::command]
pub fn leases_list(state: State<AppState>) -> CmdResult<Vec<LeaseView>> {
    let snap = snapshot(&state)?;
    let check = PortCheck {
        snapshot: &snap,
        excluded: &[],
    };
    state.with_store(|s| leases::list(s, &check))
}

#[tauri::command]
pub fn lease_release(state: State<AppState>, owner_key: String, service: String) -> CmdResult<()> {
    state
        .with_store(|s| s.release_leases(&owner_key, Some(&service)))
        .map(drop)
}

#[tauri::command]
pub fn history_list(
    state: State<AppState>,
    kind: Option<String>,
    before: Option<(i64, i64)>,
    limit: usize,
) -> CmdResult<Vec<HistoryEvent>> {
    let kinds = match kind.as_deref() {
        Some("runs") => vec![EventKind::RunStarted, EventKind::RunExited],
        Some("ports") => vec![EventKind::PortOpened, EventKind::PortClosed],
        _ => Vec::new(),
    };
    state.with_store(|s| {
        s.events(&EventQuery {
            kinds,
            port: None,
            since: None,
            before,
            limit: limit.clamp(1, 500),
        })
    })
}

#[tauri::command]
pub fn history_clear(state: State<AppState>) -> CmdResult<()> {
    state.with_store(|s| s.clear_history())
}

#[tauri::command]
pub fn trust_list(state: State<AppState>) -> CmdResult<Vec<TrustEntry>> {
    state.with_store(|s| s.trusted())
}

#[tauri::command]
pub fn trust_revoke(state: State<AppState>, project_key: String, run: String) -> CmdResult<()> {
    state
        .with_store(|s| s.revoke_trust(&project_key, &run))
        .map(drop)
}
