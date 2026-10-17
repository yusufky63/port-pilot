//! Turns raw sockets into listeners: groups addresses per owner, folds WSL and Docker forwarders
//! into the Linux process or container they serve, and marks ports with more than one owner.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;

use crate::infer::{is_docker_relay, is_wsl_relay};
use crate::model::{
    Conflict, Endpoint, Listener, Note, ProcessClass, ProcessInfo, ProcessRef, Protocol, Runtime,
};
use crate::net::{endpoint, url_for, widest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostSocket {
    pub protocol: Protocol,
    pub address: IpAddr,
    pub port: u16,
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketGroup {
    pub protocol: Protocol,
    pub port: u16,
    pub pid: u32,
    pub endpoints: Vec<Endpoint>,
}

/// One group per (protocol, port, pid); a process bound to `0.0.0.0` and `::` is one listener.
pub fn group_sockets(sockets: &[HostSocket]) -> Vec<SocketGroup> {
    let mut map: BTreeMap<(u16, Protocol, u32), Vec<Endpoint>> = BTreeMap::new();
    for s in sockets {
        map.entry((s.port, s.protocol, s.pid))
            .or_default()
            .push(endpoint(s.address, s.port));
    }
    map.into_iter()
        .map(|((port, protocol, pid), mut endpoints)| {
            endpoints.sort_by(|a, b| (a.exposure, &a.address).cmp(&(b.exposure, &b.address)));
            endpoints.dedup();
            SocketGroup {
                protocol,
                port,
                pid,
                endpoints,
            }
        })
        .collect()
}

pub fn listener_id(runtime: Runtime, protocol: Protocol, port: u16, owner: &str) -> String {
    format!("{}:{}:{port}:{owner}", runtime.as_str(), protocol.as_str())
}

pub fn process_ref(p: &ProcessInfo) -> ProcessRef {
    ProcessRef {
        pid: p.pid,
        name: p.name.clone(),
        start: p.start,
        started_at: p.started_at.clone(),
        command_line: p.command_line.clone(),
    }
}

fn relay_name(l: &Listener) -> Option<&str> {
    l.process
        .as_ref()
        .filter(|p| p.class == ProcessClass::Relay)
        .map(|p| p.name.as_str())
}

/// Recomputes `exposure` and `url` after `endpoints` changed.
pub fn refresh_reachability(l: &mut Listener) {
    l.exposure = widest(&l.endpoints);
    l.url = url_for(l.protocol, l.port, &l.endpoints);
}

/// Moves the Windows forwarder rows (`com.docker.backend`, `wslrelay`) into the container or
/// Linux listener they forward to. Forwarders with nothing behind them stay, labelled with the
/// runtime they belong to and a note when that runtime was actually inspected.
pub fn merge_runtimes(
    host: Vec<Listener>,
    docker: Vec<Listener>,
    wsl: Vec<Listener>,
    docker_inspected: bool,
    wsl_inspected: bool,
) -> Vec<Listener> {
    let mut host: Vec<Option<Listener>> = host.into_iter().map(Some).collect();
    let mut out = Vec::new();

    let mut take_relays = |l: &mut Listener, accept: &dyn Fn(&str) -> bool| {
        let mut absorbed: Vec<Endpoint> = Vec::new();
        for slot in host.iter_mut() {
            let matches = slot.as_ref().is_some_and(|h| {
                h.protocol == l.protocol && h.port == l.port && relay_name(h).is_some_and(accept)
            });
            if !matches {
                continue;
            }
            let h = slot.take().expect("checked above");
            if l.relay.is_none() {
                l.relay = h.process.as_ref().map(process_ref);
            }
            absorbed.extend(h.endpoints);
        }
        if !absorbed.is_empty() {
            absorbed.sort_by(|a, b| (a.exposure, &a.address).cmp(&(b.exposure, &b.address)));
            absorbed.dedup();
            l.endpoints = absorbed;
            refresh_reachability(l);
            true
        } else {
            false
        }
    };

    for mut d in docker {
        take_relays(&mut d, &|n: &str| is_docker_relay(n) || is_wsl_relay(n));
        out.push(d);
    }
    for mut w in wsl {
        if !take_relays(&mut w, &is_wsl_relay) {
            w.notes.push(Note::NotForwarded);
        }
        out.push(w);
    }
    for mut h in host.into_iter().flatten() {
        if let Some(name) = relay_name(&h).map(str::to_string) {
            if is_docker_relay(&name) {
                h.runtime = Runtime::Docker;
                if docker_inspected {
                    h.notes.push(Note::RelayWithoutContainer);
                }
            } else if is_wsl_relay(&name) {
                h.runtime = Runtime::Wsl;
                if wsl_inspected {
                    h.notes.push(Note::RelayWithoutListener);
                }
            }
        }
        out.push(h);
    }
    out
}

/// Marks TCP ports with more than one owner. UDP sockets share ports by design (mDNS, SSDP).
pub fn mark_conflicts(listeners: &mut [Listener]) -> Vec<Conflict> {
    let mut by_port: HashMap<u16, Vec<usize>> = HashMap::new();
    for (i, l) in listeners.iter().enumerate() {
        if l.protocol == Protocol::Tcp {
            by_port.entry(l.port).or_default().push(i);
        }
    }
    let mut conflicts: Vec<Conflict> = by_port
        .into_iter()
        .filter(|(_, idx)| idx.len() > 1)
        .map(|(port, idx)| {
            for &i in &idx {
                listeners[i].conflict = true;
                if !listeners[i].notes.contains(&Note::SharedPort) {
                    listeners[i].notes.push(Note::SharedPort);
                }
            }
            Conflict {
                protocol: Protocol::Tcp,
                port,
                listener_ids: idx.iter().map(|&i| listeners[i].id.clone()).collect(),
            }
        })
        .collect();
    conflicts.sort_by_key(|c| c.port);
    conflicts
}

pub fn sort_listeners(listeners: &mut [Listener]) {
    listeners.sort_by(|a, b| {
        (a.port, a.protocol, a.runtime as u8, &a.id).cmp(&(
            b.port,
            b.protocol,
            b.runtime as u8,
            &b.id,
        ))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Exposure, StopInfo};

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn proc_info(pid: u32, name: &str, class: ProcessClass) -> ProcessInfo {
        ProcessInfo {
            pid,
            start: Some(1),
            started_at: None,
            name: name.into(),
            path: None,
            command_line: None,
            user: None,
            class,
            access_denied: false,
        }
    }

    fn listener(
        runtime: Runtime,
        port: u16,
        owner: &str,
        process: Option<ProcessInfo>,
        addrs: &[&str],
    ) -> Listener {
        let endpoints: Vec<Endpoint> = addrs.iter().map(|a| endpoint(ip(a), port)).collect();
        let mut l = Listener {
            id: listener_id(runtime, Protocol::Tcp, port, owner),
            protocol: Protocol::Tcp,
            port,
            endpoints,
            exposure: Exposure::Loopback,
            runtime,
            process,
            ancestors: vec![],
            launcher: None,
            relay: None,
            container: None,
            wsl: None,
            project: None,
            url: None,
            stop: StopInfo {
                target: None,
                blocked: None,
            },
            conflict: false,
            notes: vec![],
        };
        refresh_reachability(&mut l);
        l
    }

    #[test]
    fn groups_dual_stack_binds() {
        let s = |a: &str, port, pid| HostSocket {
            protocol: Protocol::Tcp,
            address: ip(a),
            port,
            pid,
        };
        let groups = group_sockets(&[
            s("0.0.0.0", 3000, 7),
            s("::", 3000, 7),
            s("127.0.0.1", 5173, 8),
            s("::1", 3000, 9),
        ]);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].port, 3000);
        assert_eq!(groups[0].pid, 7);
        assert_eq!(groups[0].endpoints.len(), 2);
        assert_eq!(groups[2].port, 5173);
    }

    #[test]
    fn docker_relay_folds_into_container() {
        let relay = listener(
            Runtime::Windows,
            3000,
            "50",
            Some(proc_info(50, "com.docker.backend.exe", ProcessClass::Relay)),
            &["0.0.0.0", "::"],
        );
        let container = listener(Runtime::Docker, 3000, "abc", None, &["0.0.0.0"]);
        let out = merge_runtimes(vec![relay], vec![container], vec![], true, true);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].runtime, Runtime::Docker);
        assert_eq!(out[0].relay.as_ref().unwrap().pid, 50);
        assert_eq!(out[0].endpoints.len(), 2);
        assert_eq!(out[0].exposure, Exposure::All);
    }

    #[test]
    fn wsl_relay_folds_into_linux_listener() {
        let relay = listener(
            Runtime::Windows,
            5173,
            "60",
            Some(proc_info(60, "wslrelay.exe", ProcessClass::Relay)),
            &["127.0.0.1"],
        );
        let linux = listener(
            Runtime::Wsl,
            5173,
            "Ubuntu:841",
            Some(proc_info(841, "node", ProcessClass::User)),
            &["0.0.0.0"],
        );
        let out = merge_runtimes(vec![relay], vec![], vec![linux], true, true);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].exposure, Exposure::Loopback);
        assert_eq!(out[0].url.as_deref(), Some("http://localhost:5173"));
        assert!(out[0].notes.is_empty());
    }

    #[test]
    fn orphan_relays_are_labelled() {
        let wsl_relay = listener(
            Runtime::Windows,
            8000,
            "60",
            Some(proc_info(60, "wslrelay.exe", ProcessClass::Relay)),
            &["127.0.0.1"],
        );
        let docker_relay = listener(
            Runtime::Windows,
            4000,
            "50",
            Some(proc_info(50, "com.docker.backend.exe", ProcessClass::Relay)),
            &["0.0.0.0"],
        );
        let out = merge_runtimes(
            vec![wsl_relay.clone(), docker_relay.clone()],
            vec![],
            vec![],
            true,
            true,
        );
        assert_eq!(out[0].runtime, Runtime::Wsl);
        assert_eq!(out[0].notes, vec![Note::RelayWithoutListener]);
        assert_eq!(out[1].runtime, Runtime::Docker);
        assert_eq!(out[1].notes, vec![Note::RelayWithoutContainer]);
        let blind = merge_runtimes(vec![wsl_relay], vec![], vec![], false, false);
        assert!(blind[0].notes.is_empty());
    }

    #[test]
    fn linux_listener_without_relay_gets_note() {
        let linux = listener(Runtime::Wsl, 9000, "Ubuntu:1", None, &["0.0.0.0"]);
        let out = merge_runtimes(vec![], vec![], vec![linux], true, true);
        assert_eq!(out[0].notes, vec![Note::NotForwarded]);
    }

    #[test]
    fn conflicts_need_two_owners() {
        let a = listener(
            Runtime::Windows,
            3000,
            "1",
            Some(proc_info(1, "node.exe", ProcessClass::User)),
            &["127.0.0.1"],
        );
        let b = listener(
            Runtime::Windows,
            3000,
            "2",
            Some(proc_info(2, "node.exe", ProcessClass::User)),
            &["::1"],
        );
        let c = listener(
            Runtime::Windows,
            5173,
            "3",
            Some(proc_info(3, "node.exe", ProcessClass::User)),
            &["127.0.0.1"],
        );
        let mut all = vec![a, b, c];
        let conflicts = mark_conflicts(&mut all);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].port, 3000);
        assert!(all[0].conflict && all[1].conflict && !all[2].conflict);
        assert_eq!(all[0].notes, vec![Note::SharedPort]);
    }
}
