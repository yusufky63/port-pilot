//! Port changes for the history timeline: development listeners that appeared or went away
//! between two snapshots. Windows services, system processes and forwarders are left out; they
//! are not what anyone looks for in "what was running on 5173 yesterday".

use std::collections::HashMap;

use portpilot_core::{Listener, ProcessClass, Protocol, Snapshot};

use crate::store::{EventKind, NewEvent};

fn tracked(l: &Listener) -> bool {
    l.protocol == Protocol::Tcp
        && l.process
            .as_ref()
            .is_none_or(|p| p.class == ProcessClass::User)
}

fn event(l: &Listener) -> NewEvent {
    NewEvent {
        port: Some(l.port),
        run_id: l.managed.as_ref().map(|m| m.run_id.clone()),
        run_name: l.managed.as_ref().map(|m| m.name.clone()),
        pid: l.process.as_ref().map(|p| p.pid),
        process: l
            .container
            .as_ref()
            .map(|c| c.name.clone())
            .or_else(|| l.process.as_ref().map(|p| p.name.clone())),
        project: l.project.as_ref().map(|p| p.name.clone()),
        folder: l.project.as_ref().map(|p| p.root.clone()),
        runtime: Some(l.runtime.as_str().to_string()),
        ..NewEvent::default()
    }
}

/// Remembers the listeners of the last snapshot. The first snapshot only sets the baseline:
/// what already ran when PortPilot started is not reported as opened.
#[derive(Default)]
pub struct PortTracker {
    seen: Option<HashMap<String, NewEvent>>,
}

impl PortTracker {
    pub fn update(&mut self, snapshot: &Snapshot) -> Vec<(EventKind, NewEvent)> {
        let now: HashMap<String, NewEvent> = snapshot
            .listeners
            .iter()
            .filter(|l| tracked(l))
            .map(|l| (l.id.clone(), event(l)))
            .collect();
        let mut out = Vec::new();
        if let Some(before) = &self.seen {
            for (id, e) in &now {
                if !before.contains_key(id) {
                    out.push((EventKind::PortOpened, e.clone()));
                }
            }
            for (id, e) in before {
                if !now.contains_key(id) {
                    out.push((EventKind::PortClosed, e.clone()));
                }
            }
        }
        out.sort_by_key(|(kind, e)| (*kind == EventKind::PortOpened, e.port));
        self.seen = Some(now);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portpilot_core::assemble::listener_id;
    use portpilot_core::{
        AdapterState, AdapterStatus, Adapters, Exposure, ProcessInfo, Runtime, SCHEMA_VERSION,
        StopInfo,
    };

    fn listener(port: u16, pid: u32, class: ProcessClass) -> Listener {
        Listener {
            id: listener_id(Runtime::Windows, Protocol::Tcp, port, &format!("{pid}:1")),
            protocol: Protocol::Tcp,
            port,
            endpoints: Vec::new(),
            exposure: Exposure::Loopback,
            runtime: Runtime::Windows,
            process: Some(ProcessInfo {
                pid,
                start: Some(1),
                started_at: None,
                name: "node.exe".into(),
                path: None,
                command_line: None,
                user: None,
                class,
                access_denied: false,
            }),
            ancestors: Vec::new(),
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
            notes: Vec::new(),
            managed: None,
        }
    }

    fn snap(listeners: Vec<Listener>) -> Snapshot {
        Snapshot {
            schema_version: SCHEMA_VERSION,
            taken_at: String::new(),
            duration_ms: 0,
            listeners,
            conflicts: Vec::new(),
            adapters: Adapters {
                wsl: AdapterState::new(AdapterStatus::Disabled),
                docker: AdapterState::new(AdapterStatus::Disabled),
            },
            warnings: Vec::new(),
        }
    }

    #[test]
    fn reports_changes_after_the_baseline() {
        let mut t = PortTracker::default();
        assert!(
            t.update(&snap(vec![listener(5173, 10, ProcessClass::User)]))
                .is_empty()
        );
        let changes = t.update(&snap(vec![
            listener(3000, 11, ProcessClass::User),
            listener(135, 900, ProcessClass::Service),
        ]));
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].0, EventKind::PortClosed);
        assert_eq!(changes[0].1.port, Some(5173));
        assert_eq!(changes[1].0, EventKind::PortOpened);
        assert_eq!(changes[1].1.port, Some(3000));
        assert_eq!(changes[1].1.process.as_deref(), Some("node.exe"));
        assert!(
            t.update(&snap(vec![listener(3000, 11, ProcessClass::User)]))
                .is_empty()
        );
        // The same port with a new owner is a close and an open.
        let changes = t.update(&snap(vec![listener(3000, 12, ProcessClass::User)]));
        assert_eq!(changes.len(), 2);
    }
}
