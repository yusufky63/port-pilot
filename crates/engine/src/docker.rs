//! Docker Desktop through the Docker Engine API on its named pipe. Port Pilot reads container
//! port mappings and Compose labels, and stops a container only when the user confirms it.
//! It never kills Docker's host-side proxy process.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use portpilot_core::docker::{ApiVersion, is_valid_container_id, parse_containers, parse_version};
use portpilot_core::http::{Response, parse_response, request};
use portpilot_core::{AdapterState, AdapterStatus, ContainerInfo};

const TIMEOUT: Duration = Duration::from_secs(4);

/// Pipes to try, in order: `DOCKER_HOST` when it names a pipe, then Docker Desktop's defaults.
fn pipe_candidates() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(host) = std::env::var("DOCKER_HOST")
        && let Some(path) = host.strip_prefix("npipe://")
    {
        out.push(path.replace('/', "\\").trim_start_matches('\\').to_string());
    }
    out.push(r"\\.\pipe\dockerDesktopLinuxEngine".into());
    out.push(r"\\.\pipe\docker_engine".into());
    out.into_iter()
        .map(|p| {
            if p.starts_with(r"\\") {
                p
            } else {
                format!(r"\\{p}")
            }
        })
        .collect()
}

fn tcp_docker_host() -> bool {
    std::env::var("DOCKER_HOST").is_ok_and(|h| h.starts_with("tcp://") || h.starts_with("ssh://"))
}

fn find_pipe() -> Option<String> {
    pipe_candidates()
        .into_iter()
        .find(|p| Path::new(p).exists())
}

fn exchange(pipe: &str, method: &str, path: &str) -> Result<Response, String> {
    let pipe = pipe.to_string();
    let req = request(method, path);
    let (tx, rx) = mpsc::channel();
    // A pipe read cannot time out on its own, so the call runs on a thread the caller can stop
    // waiting for.
    std::thread::spawn(move || {
        let result = (|| -> Result<Response, String> {
            let mut f = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&pipe)
                .map_err(|e| format!("cannot open the Docker pipe: {e}"))?;
            f.write_all(&req).map_err(|e| e.to_string())?;
            let mut raw = Vec::new();
            f.read_to_end(&mut raw).map_err(|e| e.to_string())?;
            parse_response(&raw)
        })();
        let _ = tx.send(result);
    });
    rx.recv_timeout(TIMEOUT)
        .map_err(|_| "Docker did not answer in time".to_string())?
}

fn not_reachable() -> AdapterState {
    if tcp_docker_host() {
        return AdapterState::with_detail(
            AdapterStatus::Error,
            "DOCKER_HOST points to a TCP or SSH engine, which is not supported yet",
        );
    }
    let installed = ["ProgramFiles", "LOCALAPPDATA"].iter().any(|var| {
        std::env::var(var).is_ok_and(|base| {
            Path::new(&base)
                .join(r"Docker\Docker\Docker Desktop.exe")
                .exists()
                || Path::new(&base)
                    .join(r"Programs\DockerDesktop\Docker Desktop.exe")
                    .exists()
        })
    });
    AdapterState::new(if installed {
        AdapterStatus::NotRunning
    } else {
        AdapterStatus::NotInstalled
    })
}

/// Running containers, or the adapter state that explains why there are none.
pub fn containers() -> Result<Vec<ContainerInfo>, AdapterState> {
    let pipe = find_pipe().ok_or_else(not_reachable)?;
    let resp = exchange(&pipe, "GET", "/containers/json")
        .map_err(|e| AdapterState::with_detail(AdapterStatus::Error, e))?;
    if resp.status != 200 {
        return Err(AdapterState::with_detail(
            AdapterStatus::Error,
            format!("Docker answered HTTP {}", resp.status),
        ));
    }
    parse_containers(&resp.body).map_err(|e| AdapterState::with_detail(AdapterStatus::Error, e))
}

pub fn version() -> Option<ApiVersion> {
    let pipe = find_pipe()?;
    let resp = exchange(&pipe, "GET", "/version").ok()?;
    (resp.status == 200)
        .then(|| parse_version(&resp.body).ok())
        .flatten()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopResult {
    Stopped,
    AlreadyStopped,
    Unreachable,
    Failed(u16),
}

/// `POST /containers/{id}/stop?t=<grace>`: Docker sends SIGTERM and kills the container after
/// the grace period, which is Docker's own stop semantics.
pub fn stop(id: &str, grace: Duration) -> StopResult {
    if !is_valid_container_id(id) {
        return StopResult::Failed(400);
    }
    let Some(pipe) = find_pipe() else {
        return StopResult::Unreachable;
    };
    let path = format!("/containers/{id}/stop?t={}", grace.as_secs().max(1));
    let pipe_owned = pipe.clone();
    // Stopping waits for the container, so this call gets the grace period on top of the
    // usual timeout.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(exchange_long(&pipe_owned, &path));
    });
    match rx.recv_timeout(grace + TIMEOUT * 2) {
        Ok(Ok(r)) if r.status == 204 => StopResult::Stopped,
        Ok(Ok(r)) if r.status == 304 || r.status == 404 => StopResult::AlreadyStopped,
        Ok(Ok(r)) => StopResult::Failed(r.status),
        Ok(Err(_)) | Err(_) => StopResult::Unreachable,
    }
}

fn exchange_long(pipe: &str, path: &str) -> Result<Response, String> {
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe)
        .map_err(|e| e.to_string())?;
    f.write_all(&request("POST", path))
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    f.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    parse_response(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_names_are_absolute() {
        for p in pipe_candidates() {
            assert!(p.starts_with(r"\\.\pipe\"), "{p}");
        }
    }

    #[test]
    fn invalid_ids_never_reach_docker() {
        assert_eq!(
            stop("../../version", Duration::from_secs(1)),
            StopResult::Failed(400)
        );
    }
}
