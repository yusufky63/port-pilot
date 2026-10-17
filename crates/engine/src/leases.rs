//! Port leases on this machine: the allocator from `portpilot-core` with the machine's view of
//! which ports are free, stored in `portpilot.db` so the app and `portctl` agree.

use std::collections::HashSet;
use std::path::Path;

use portpilot_core::lease::{Lease, LeaseOwner, Want, allocate};
use portpilot_core::net::{PortRange, excluded_range_for};
use portpilot_core::recipe::PortSpec;
use portpilot_core::{Listener, Protocol, Snapshot};
use serde::Serialize;

use crate::doctor::port_is_bindable;
use crate::store::{Store, now_ms};

/// What the machine says about ports right now.
pub struct PortCheck<'a> {
    pub snapshot: &'a Snapshot,
    pub excluded: &'a [PortRange],
}

impl PortCheck<'_> {
    /// TCP listeners on `port`.
    pub fn listeners(&self, port: u16) -> Vec<&Listener> {
        self.snapshot
            .listeners
            .iter()
            .filter(|l| l.protocol == Protocol::Tcp && l.port == port)
            .collect()
    }

    /// No listener, not reserved by Windows, and a test bind on loopback succeeds.
    pub fn usable(&self, port: u16) -> bool {
        self.listeners(port).is_empty()
            && excluded_range_for(port, self.excluded).is_none()
            && port_is_bindable(port)
    }
}

fn others(all: &[Lease], owner_key: &str, service: &str) -> HashSet<u16> {
    all.iter()
        .filter(|l| !(l.owner_key == owner_key && l.service == service))
        .map(|l| l.port)
        .collect()
}

fn current(all: &[Lease], owner_key: &str, service: &str) -> Option<u16> {
    all.iter()
        .find(|l| l.owner_key == owner_key && l.service == service)
        .map(|l| l.port)
}

/// The port this owner would get, without storing anything: its lease, or the first usable
/// port. `None` when the range is exhausted.
pub fn propose(
    store: &Store,
    owner: &LeaseOwner,
    service: &str,
    spec: &PortSpec,
    check: &PortCheck,
) -> Result<Option<(u16, bool)>, String> {
    let all = store.leases()?;
    let key = owner.key();
    let mine = current(&all, &key, service);
    let port = allocate(spec, mine, Want::Keep, &others(&all, &key, service), |p| {
        check.usable(p)
    });
    Ok(port.map(|p| (p, mine == Some(p))))
}

fn lease_for(owner: &LeaseOwner, project: &str, service: &str, port: u16) -> Lease {
    Lease {
        owner_key: owner.key(),
        service: service.to_string(),
        port,
        worktree: owner.worktree.clone(),
        common_dir: owner.common_dir.clone(),
        project: project.to_string(),
        acquired_at: now_ms(),
    }
}

/// Stores a lease: the current one (`Keep`) or the next usable port (`Next`, the answer to a
/// port conflict).
pub fn acquire(
    store: &mut Store,
    owner: &LeaseOwner,
    project: &str,
    service: &str,
    spec: &PortSpec,
    want: Want,
    check: &PortCheck,
) -> Result<Lease, String> {
    let key = owner.key();
    store.lease_tx(|all| {
        let mine = current(all, &key, service);
        let port = allocate(spec, mine, want, &others(all, &key, service), |p| {
            check.usable(p)
        })
        .ok_or_else(|| {
            format!(
                "no free port for {service} in {}-{}",
                spec.range.start, spec.range.end
            )
        })?;
        if mine == Some(port) {
            // Keep the original acquisition time.
            if let Some(l) = all
                .iter()
                .find(|l| l.owner_key == key && l.service == service)
            {
                return Ok(l.clone());
            }
        }
        Ok(lease_for(owner, project, service, port))
    })
}

/// Stores the lease for exactly `port`, the one the user saw before starting a run. Fails if
/// another worktree took it in the meantime.
pub fn claim(
    store: &mut Store,
    owner: &LeaseOwner,
    project: &str,
    service: &str,
    port: u16,
) -> Result<Lease, String> {
    let key = owner.key();
    store.lease_tx(|all| {
        if let Some(other) = all
            .iter()
            .find(|l| l.port == port && !(l.owner_key == key && l.service == service))
        {
            return Err(format!(
                "port {port} is leased to {} ({}) now",
                other.service, other.worktree
            ));
        }
        match all
            .iter()
            .find(|l| l.owner_key == key && l.service == service)
        {
            Some(l) if l.port == port => Ok(l.clone()),
            _ => Ok(lease_for(owner, project, service, port)),
        }
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaseUser {
    pub listener_id: String,
    pub pid: Option<u32>,
    pub name: String,
    pub project: Option<String>,
    /// Owned by a run of the lease's own worktree.
    pub same_worktree: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaseView {
    #[serde(flatten)]
    pub lease: Lease,
    /// Who listens on the port now.
    pub users: Vec<LeaseUser>,
    /// The worktree folder no longer exists (removed with `git worktree remove`).
    pub stale: bool,
}

/// Who listens on `port`, as lease users.
pub fn users_of(check: &PortCheck, port: u16, worktree: &str) -> Vec<LeaseUser> {
    check
        .listeners(port)
        .into_iter()
        .map(|l| LeaseUser {
            listener_id: l.id.clone(),
            pid: l.process.as_ref().map(|p| p.pid),
            name: l
                .container
                .as_ref()
                .map(|c| c.name.clone())
                .or_else(|| l.process.as_ref().map(|p| p.name.clone()))
                .unwrap_or_default(),
            project: l.project.as_ref().map(|p| p.name.clone()),
            same_worktree: l.project.as_ref().is_some_and(|p| {
                portpilot_core::git::same_path(&p.root, worktree)
                    || p.git
                        .as_ref()
                        .is_some_and(|g| portpilot_core::git::same_path(&g.top_level, worktree))
            }),
        })
        .collect()
}

/// Every lease with who uses its port now.
pub fn list(store: &Store, check: &PortCheck) -> Result<Vec<LeaseView>, String> {
    Ok(store
        .leases()?
        .into_iter()
        .map(|lease| LeaseView {
            users: users_of(check, lease.port, &lease.worktree),
            stale: !Path::new(&lease.worktree).is_dir(),
            lease,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use portpilot_core::{AdapterState, AdapterStatus, Adapters, SCHEMA_VERSION};
    use std::net::TcpListener;

    fn empty_snapshot() -> Snapshot {
        Snapshot {
            schema_version: SCHEMA_VERSION,
            taken_at: String::new(),
            duration_ms: 0,
            listeners: Vec::new(),
            conflicts: Vec::new(),
            adapters: Adapters {
                wsl: AdapterState::new(AdapterStatus::Disabled),
                docker: AdapterState::new(AdapterStatus::Disabled),
            },
            warnings: Vec::new(),
        }
    }

    fn owner(wt: &str) -> LeaseOwner {
        LeaseOwner {
            common_dir: Some(r"C:\src\shop\.git".into()),
            worktree: wt.into(),
        }
    }

    /// A free range of ten ports, found by binding.
    fn free_range() -> PortSpec {
        for base in (41000..60000).step_by(97) {
            if (base..base + 10).all(port_is_bindable) {
                return PortSpec {
                    preferred: base,
                    range: PortRange {
                        start: base,
                        end: base + 9,
                    },
                };
            }
        }
        panic!("no free test range");
    }

    #[test]
    fn worktrees_of_one_repository_get_different_ports() {
        let mut store = Store::in_memory().unwrap();
        let snap = empty_snapshot();
        let check = PortCheck {
            snapshot: &snap,
            excluded: &[],
        };
        let spec = free_range();
        let main = owner(r"C:\src\shop");
        let wt = owner(r"D:\wt\auth");
        assert_eq!(
            propose(&store, &main, "web", &spec, &check).unwrap(),
            Some((spec.preferred, false))
        );
        let a = acquire(&mut store, &main, "shop", "web", &spec, Want::Keep, &check).unwrap();
        let b = acquire(&mut store, &wt, "shop", "web", &spec, Want::Keep, &check).unwrap();
        assert_eq!(a.port, spec.preferred);
        assert_eq!(b.port, spec.preferred + 1);
        // Asking again keeps the lease.
        let again = acquire(&mut store, &main, "shop", "web", &spec, Want::Keep, &check).unwrap();
        assert_eq!(again, a);
        assert_eq!(
            propose(&store, &main, "web", &spec, &check).unwrap(),
            Some((a.port, true))
        );
        // "Allocate the next free port for this worktree".
        let next = acquire(&mut store, &main, "shop", "web", &spec, Want::Next, &check).unwrap();
        assert_eq!(next.port, spec.preferred + 2);
        assert_eq!(store.leases().unwrap().len(), 2);
    }

    #[test]
    fn a_busy_port_is_skipped_for_new_leases() {
        let mut store = Store::in_memory().unwrap();
        let snap = empty_snapshot();
        let check = PortCheck {
            snapshot: &snap,
            excluded: &[],
        };
        let spec = free_range();
        let busy = TcpListener::bind(("127.0.0.1", spec.preferred)).unwrap();
        let l = acquire(
            &mut store,
            &owner(r"C:\pp-missing-worktree-a"),
            "shop",
            "web",
            &spec,
            Want::Keep,
            &check,
        )
        .unwrap();
        assert_eq!(l.port, spec.preferred + 1);
        drop(busy);
        // Reserved by Windows counts as busy too.
        let excluded = [PortRange {
            start: spec.preferred,
            end: spec.preferred + 5,
        }];
        let check = PortCheck {
            snapshot: &snap,
            excluded: &excluded,
        };
        let l = acquire(
            &mut store,
            &owner(r"C:\pp-missing-worktree-b"),
            "shop",
            "web",
            &spec,
            Want::Keep,
            &check,
        )
        .unwrap();
        assert_eq!(l.port, spec.preferred + 6);
    }

    #[test]
    fn claim_refuses_a_port_leased_to_someone_else() {
        let mut store = Store::in_memory().unwrap();
        claim(
            &mut store,
            &owner(r"C:\pp-missing-worktree-a"),
            "shop",
            "web",
            5173,
        )
        .unwrap();
        assert!(
            claim(
                &mut store,
                &owner(r"C:\pp-missing-worktree-b"),
                "shop",
                "web",
                5173
            )
            .is_err()
        );
        // Claiming one's own port again is fine and keeps it.
        let l = claim(
            &mut store,
            &owner(r"C:\pp-missing-worktree-a"),
            "shop",
            "web",
            5173,
        )
        .unwrap();
        assert_eq!(l.port, 5173);
        let moved = claim(
            &mut store,
            &owner(r"C:\pp-missing-worktree-a"),
            "shop",
            "web",
            5174,
        )
        .unwrap();
        assert_eq!(moved.port, 5174);
        let snap = empty_snapshot();
        let views = list(
            &store,
            &PortCheck {
                snapshot: &snap,
                excluded: &[],
            },
        )
        .unwrap();
        assert_eq!(views.len(), 1);
        assert!(views[0].stale, "the worktree folder does not exist");
    }
}
