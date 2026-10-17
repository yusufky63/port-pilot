//! The project view: listeners grouped by repository, with every worktree of the repository
//! listed, including the ones that run nothing.

use std::collections::BTreeMap;

use portpilot_core::git::same_path;
use portpilot_core::{Runtime, Snapshot};
use serde::Serialize;

use crate::git;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeView {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub detached: bool,
    pub locked: bool,
    pub prunable: bool,
    /// The main checkout rather than a linked worktree.
    pub main: bool,
    pub dirty: Option<bool>,
    pub listener_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroup {
    pub key: String,
    pub name: String,
    pub runtime: Runtime,
    /// Shared `.git` folder; `None` for projects outside Git.
    pub common_dir: Option<String>,
    pub root: String,
    pub open_path: Option<String>,
    pub worktrees: Vec<WorktreeView>,
    /// Listeners attributed to the project folder itself (projects outside Git).
    pub listener_ids: Vec<String>,
}

pub fn overview(snapshot: &Snapshot) -> Vec<ProjectGroup> {
    let mut groups: BTreeMap<String, ProjectGroup> = BTreeMap::new();
    for l in &snapshot.listeners {
        let Some(p) = &l.project else { continue };
        match &p.git {
            Some(g) => {
                let key = format!(
                    "{}|{}",
                    l.runtime.as_str(),
                    g.common_dir.to_ascii_lowercase()
                );
                let group = groups.entry(key.clone()).or_insert_with(|| {
                    // Worktree lists come from Git on Windows; for WSL repositories the
                    // worktrees seen in listeners are what is known.
                    let worktrees = if l.runtime == Runtime::Wsl {
                        Vec::new()
                    } else {
                        git::worktrees(&g.top_level)
                            .into_iter()
                            .filter(|w| !w.bare)
                            .map(|w| WorktreeView {
                                main: same_path(&format!("{}\\.git", w.path), &g.common_dir),
                                path: w.path,
                                branch: w.branch,
                                head: w.head,
                                detached: w.detached,
                                locked: w.locked,
                                prunable: w.prunable,
                                dirty: None,
                                listener_ids: Vec::new(),
                            })
                            .collect()
                    };
                    ProjectGroup {
                        key,
                        name: g.repo_name.clone(),
                        runtime: l.runtime,
                        common_dir: Some(g.common_dir.clone()),
                        root: g.top_level.clone(),
                        open_path: p.open_path.clone(),
                        worktrees,
                        listener_ids: Vec::new(),
                    }
                });
                let wt = match group
                    .worktrees
                    .iter_mut()
                    .find(|w| same_path(&w.path, &g.top_level))
                {
                    Some(w) => w,
                    None => {
                        group.worktrees.push(WorktreeView {
                            path: g.top_level.clone(),
                            branch: g.branch.clone(),
                            head: g.head.clone(),
                            detached: g.branch.is_none(),
                            locked: false,
                            prunable: false,
                            main: !g.linked_worktree,
                            dirty: g.dirty,
                            listener_ids: Vec::new(),
                        });
                        group.worktrees.last_mut().expect("just pushed")
                    }
                };
                wt.dirty = g.dirty;
                if !wt.listener_ids.contains(&l.id) {
                    wt.listener_ids.push(l.id.clone());
                }
            }
            None => {
                let key = format!("{}|{}", l.runtime.as_str(), p.root.to_ascii_lowercase());
                let group = groups.entry(key.clone()).or_insert_with(|| ProjectGroup {
                    key,
                    name: p.name.clone(),
                    runtime: l.runtime,
                    common_dir: None,
                    root: p.root.clone(),
                    open_path: p.open_path.clone(),
                    worktrees: Vec::new(),
                    listener_ids: Vec::new(),
                });
                group.listener_ids.push(l.id.clone());
            }
        }
    }
    let mut list: Vec<ProjectGroup> = groups.into_values().collect();
    for g in &mut list {
        // The main checkout first, then worktrees that run something, then the rest.
        g.worktrees.sort_by(|a, b| {
            (
                !a.main,
                a.listener_ids.is_empty(),
                a.path.to_ascii_lowercase(),
            )
                .cmp(&(
                    !b.main,
                    b.listener_ids.is_empty(),
                    b.path.to_ascii_lowercase(),
                ))
        });
    }
    list.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    list
}
