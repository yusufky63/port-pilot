//! Port leases: a port reserved for one service of one worktree of one repository, so two
//! checkouts of the same project do not fight over `5173`.
//!
//! The owner of a lease is the repository's shared Git directory plus the worktree folder plus
//! the service name. Projects outside Git use their folder for both. This module decides which
//! port to hand out; the engine checks the machine and stores the result.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::recipe::PortSpec;

/// Who a lease belongs to, apart from the service name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaseOwner {
    /// The shared `.git` directory; `None` outside Git.
    pub common_dir: Option<String>,
    /// The worktree (or project) folder.
    pub worktree: String,
}

/// `C:/Src/Shop/` and `c:\src\shop` name the same folder on Windows.
pub fn path_key(path: &str) -> String {
    path.trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

impl LeaseOwner {
    /// The stored identity of the owner: case and separators do not matter.
    pub fn key(&self) -> String {
        let common = self.common_dir.as_deref().unwrap_or(&self.worktree);
        format!("{}|{}", path_key(common), path_key(&self.worktree))
    }

    /// The repository (or project folder) the worktree belongs to, for trust records.
    pub fn project_key(&self) -> String {
        path_key(self.common_dir.as_deref().unwrap_or(&self.worktree))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lease {
    pub owner_key: String,
    pub service: String,
    pub port: u16,
    pub worktree: String,
    pub common_dir: Option<String>,
    pub project: String,
    /// Unix milliseconds.
    pub acquired_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    /// The owner's current port if it still fits the spec, otherwise the first usable one.
    Keep,
    /// The next usable port after the current one: the answer to "this port is taken".
    Next,
}

/// Ports in the order they are tried: `preferred` first, then upwards to the end of the range,
/// then from the start of the range. With `after`, the order starts behind that port and wraps.
pub fn candidates(spec: &PortSpec, after: Option<u16>) -> Vec<u16> {
    let (start, end) = (spec.range.start, spec.range.end);
    let first = match after {
        Some(a) if spec.range.contains(a) => {
            if a == end {
                start
            } else {
                a + 1
            }
        }
        _ => spec.preferred,
    };
    let mut out: Vec<u16> = (first..=end).collect();
    out.extend(start..first);
    if let Some(a) = after {
        out.retain(|p| *p != a);
    }
    out
}

/// Picks a port for one owner and service.
///
/// `current` is the owner's lease for this service, `taken` the ports leased to anyone else,
/// and `usable` says whether a port is free on this machine right now (no listener, not
/// reserved by Windows, bindable). A kept lease is returned without asking `usable`: the
/// owner's own server may be the one listening on it.
pub fn allocate(
    spec: &PortSpec,
    current: Option<u16>,
    want: Want,
    taken: &HashSet<u16>,
    mut usable: impl FnMut(u16) -> bool,
) -> Option<u16> {
    if want == Want::Keep
        && let Some(c) = current
        && spec.range.contains(c)
        && !taken.contains(&c)
    {
        return Some(c);
    }
    let after = if want == Want::Next { current } else { None };
    candidates(spec, after)
        .into_iter()
        .find(|p| !taken.contains(p) && usable(*p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::PortRange;

    fn spec(preferred: u16, start: u16, end: u16) -> PortSpec {
        PortSpec {
            preferred,
            range: PortRange { start, end },
        }
    }

    #[test]
    fn owner_keys_ignore_case_and_separators() {
        let a = LeaseOwner {
            common_dir: Some(r"C:\Src\Shop\.git".into()),
            worktree: r"D:\wt\auth\".into(),
        };
        let b = LeaseOwner {
            common_dir: Some("c:/src/shop/.git".into()),
            worktree: "d:/WT/auth".into(),
        };
        assert_eq!(a.key(), b.key());
        assert_eq!(a.key(), r"c:\src\shop\.git|d:\wt\auth");
        assert_eq!(a.project_key(), r"c:\src\shop\.git");
        let plain = LeaseOwner {
            common_dir: None,
            worktree: r"C:\src\notes".into(),
        };
        assert_eq!(plain.key(), r"c:\src\notes|c:\src\notes");
        assert_eq!(plain.project_key(), r"c:\src\notes");
    }

    #[test]
    fn candidate_order_starts_at_preferred_and_wraps() {
        assert_eq!(
            candidates(&spec(5175, 5173, 5177), None),
            vec![5175, 5176, 5177, 5173, 5174]
        );
        assert_eq!(
            candidates(&spec(5175, 5173, 5177), Some(5176)),
            vec![5177, 5173, 5174, 5175]
        );
        assert_eq!(
            candidates(&spec(5173, 5173, 5175), Some(5175)),
            vec![5173, 5174]
        );
        // A current port outside the range restarts at preferred.
        assert_eq!(
            candidates(&spec(5173, 5173, 5174), Some(9000)),
            vec![5173, 5174]
        );
        assert_eq!(candidates(&spec(65535, 65535, 65535), None), vec![65535]);
    }

    #[test]
    fn first_lease_gets_preferred_when_free() {
        let s = spec(5173, 5173, 5199);
        assert_eq!(
            allocate(&s, None, Want::Keep, &HashSet::new(), |_| true),
            Some(5173)
        );
    }

    #[test]
    fn skips_ports_leased_to_other_worktrees_and_busy_ports() {
        let s = spec(5173, 5173, 5199);
        let taken = HashSet::from([5173, 5174]);
        assert_eq!(
            allocate(&s, None, Want::Keep, &taken, |p| p != 5175),
            Some(5176)
        );
    }

    #[test]
    fn keeps_an_existing_lease_even_while_it_is_in_use() {
        let s = spec(5173, 5173, 5199);
        assert_eq!(
            allocate(&s, Some(5180), Want::Keep, &HashSet::new(), |_| false),
            Some(5180)
        );
    }

    #[test]
    fn moves_a_lease_that_no_longer_fits_the_range() {
        let s = spec(3000, 3000, 3009);
        assert_eq!(
            allocate(&s, Some(5180), Want::Keep, &HashSet::new(), |_| true),
            Some(3000)
        );
    }

    #[test]
    fn next_moves_past_the_current_port() {
        let s = spec(5173, 5173, 5199);
        let taken = HashSet::from([5174]);
        assert_eq!(
            allocate(&s, Some(5173), Want::Next, &taken, |_| true),
            Some(5175)
        );
        // Wraps to the start of the range and never returns the current port.
        let s = spec(5173, 5173, 5175);
        assert_eq!(
            allocate(&s, Some(5175), Want::Next, &HashSet::new(), |_| true),
            Some(5173)
        );
        assert_eq!(
            allocate(&s, Some(5173), Want::Next, &HashSet::new(), |p| p == 5173),
            None
        );
    }

    #[test]
    fn exhausted_range_gives_none() {
        let s = spec(5173, 5173, 5175);
        let taken = HashSet::from([5173, 5174, 5175]);
        assert_eq!(allocate(&s, None, Want::Keep, &taken, |_| true), None);
        assert_eq!(
            allocate(&s, None, Want::Keep, &HashSet::new(), |_| false),
            None
        );
    }

    #[test]
    fn usable_is_not_asked_about_taken_ports() {
        let s = spec(5173, 5173, 5176);
        let taken = HashSet::from([5173]);
        let mut asked = Vec::new();
        let got = allocate(&s, None, Want::Keep, &taken, |p| {
            asked.push(p);
            p == 5175
        });
        assert_eq!(got, Some(5175));
        assert_eq!(asked, vec![5174, 5175]);
    }
}
