//! Diagnostics: why a port is busy or unusable, which ports are free, and what the machine's
//! runtimes look like.

use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};
use std::time::Duration;

use portpilot_core::net::{PortRange, excluded_range_for, free_candidates, parse_excluded_ranges};
use portpilot_core::{AdapterState, Listener, Protocol, Snapshot};
use portpilot_win::{os, process};
use serde::Serialize;

use crate::cmd::{self, Spec};
use crate::{docker, git, wsl};

/// Port ranges Windows has reserved for TCP (Hyper-V, WinNAT, Docker and WSL take blocks of
/// them). A port inside one cannot be bound even when no process uses it.
pub fn excluded_tcp_ranges() -> Vec<PortRange> {
    let mut out = Vec::new();
    for family in ["ipv4", "ipv6"] {
        let args = [
            "interface",
            family,
            "show",
            "excludedportrange",
            "protocol=tcp",
        ];
        if let Ok(o) = cmd::run(&Spec::new("netsh.exe", &args, Duration::from_secs(5)))
            && o.ok()
        {
            out.extend(parse_excluded_ranges(&o.stdout_text()));
        }
    }
    out.sort_by_key(|r| (r.start, r.end));
    out.dedup();
    out
}

fn bindable(addr: SocketAddr) -> Result<(), std::io::ErrorKind> {
    TcpListener::bind(addr).map(drop).map_err(|e| e.kind())
}

/// Binds the loopback addresses only: a loopback listener never triggers a firewall prompt.
/// An unavailable IPv6 stack does not make a port busy.
pub fn port_is_bindable(port: u16) -> bool {
    if bindable(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).is_err() {
        return false;
    }
    !matches!(
        bindable(SocketAddr::from((Ipv6Addr::LOCALHOST, port))),
        Err(std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied)
    )
}

fn used_tcp_ports(snapshot: &Snapshot) -> HashSet<u16> {
    snapshot
        .listeners
        .iter()
        .filter(|l| l.protocol == Protocol::Tcp)
        .map(|l| l.port)
        .collect()
}

/// Free TCP ports in `range`, verified by binding each one.
pub fn find_free(
    range: PortRange,
    count: usize,
    snapshot: &Snapshot,
    excluded: &[PortRange],
) -> Vec<u16> {
    let used = used_tcp_ports(snapshot);
    let mut out = Vec::new();
    for port in free_candidates(range, &used, excluded, count.saturating_mul(8).max(16)) {
        if port_is_bindable(port) {
            out.push(port);
            if out.len() >= count {
                break;
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// Nothing listens and the port can be bound.
    Free,
    /// One owner listens.
    InUse,
    /// More than one owner listens, on different addresses.
    Shared,
    /// Windows has reserved the port; binding fails with "access denied".
    Reserved,
    /// No listener is visible but binding fails: a process of another user, a socket in
    /// another network namespace, or a stale forwarder.
    Blocked,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortDiagnosis {
    pub port: u16,
    pub verdict: Verdict,
    pub listeners: Vec<Listener>,
    pub excluded_range: Option<PortRange>,
    pub suggestions: Vec<u16>,
}

pub fn diagnose_port(port: u16, snapshot: &Snapshot, excluded: &[PortRange]) -> PortDiagnosis {
    let listeners: Vec<Listener> = snapshot
        .listeners
        .iter()
        .filter(|l| l.protocol == Protocol::Tcp && l.port == port)
        .cloned()
        .collect();
    let excluded_range = excluded_range_for(port, excluded);
    let verdict = match listeners.len() {
        0 if excluded_range.is_some() => Verdict::Reserved,
        0 if port_is_bindable(port) => Verdict::Free,
        0 => Verdict::Blocked,
        1 => Verdict::InUse,
        _ => Verdict::Shared,
    };
    let suggestions = if verdict == Verdict::Free {
        Vec::new()
    } else {
        let end = port.saturating_add(200).max(port);
        find_free(
            PortRange {
                start: port.saturating_add(1).max(1),
                end,
            },
            3,
            snapshot,
            excluded,
        )
    };
    PortDiagnosis {
        port,
        verdict,
        listeners,
        excluded_range,
        suggestions,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub os: Option<String>,
    /// Windows 10 is out of general support since 2025-10-14; best effort there.
    pub windows10: bool,
    pub elevated: bool,
    pub user: Option<String>,
    pub git: Option<String>,
    pub wsl: AdapterState,
    pub wsl_version: Option<String>,
    pub docker: AdapterState,
    pub docker_version: Option<String>,
    pub excluded_tcp: Vec<PortRange>,
}

pub fn environment() -> Environment {
    let version = os::version();
    let wsl_state = match wsl::running_distros() {
        Ok(d) => AdapterState {
            status: portpilot_core::AdapterStatus::Available,
            detail: None,
            items: d,
        },
        Err(s) => s,
    };
    let docker_state = match docker::containers() {
        Ok(c) => AdapterState {
            status: portpilot_core::AdapterStatus::Available,
            detail: None,
            items: c.into_iter().map(|c| c.name).collect(),
        },
        Err(s) => s,
    };
    Environment {
        os: version.as_ref().map(|v| v.label()),
        windows10: version.as_ref().is_some_and(|v| v.is_windows_10()),
        elevated: process::is_elevated(),
        user: process::current_user(),
        git: git::version(),
        wsl_version: wsl::version(),
        wsl: wsl_state,
        docker_version: docker::version().map(|v| format!("{} (API {})", v.version, v.api_version)),
        docker: docker_state,
        excluded_tcp: excluded_tcp_ranges(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_port_is_not_bindable() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(!port_is_bindable(port));
        drop(l);
        assert!(port_is_bindable(port));
    }
}
