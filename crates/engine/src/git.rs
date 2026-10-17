//! Git context through the Git CLI. PortPilot never takes Git's index lock: every call runs
//! with `--no-optional-locks`, so a background refresh cannot get in the way of a commit.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use portpilot_core::GitInfo;
use portpilot_core::git::{
    Worktree, parse_rev_parse, parse_status_v2, parse_worktree_list, repo_name, same_path,
    short_oid,
};

use crate::cmd::{self, Spec};

const TIMEOUT: Duration = Duration::from_secs(4);
const ENV: &[(&str, &str)] = &[
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("LC_ALL", "C"),
];

fn git(dir: &str, args: &[&str]) -> Option<String> {
    let mut full: Vec<&str> = vec!["--no-optional-locks", "-C", dir];
    full.extend_from_slice(args);
    let spec = Spec {
        env: ENV,
        ..Spec::new("git", &full, TIMEOUT)
    };
    let out = cmd::run(&spec).ok()?;
    out.ok().then(|| out.stdout_text())
}

/// Git prints `C:/src/shop`; Windows tools and people expect `C:\src\shop`.
pub fn windows_path(p: &str) -> String {
    p.replace('/', "\\")
}

/// Git context of the repository that contains `dir`, or `None` outside a repository (or when
/// Git is missing or refuses the folder, for example for another owner's repository).
pub fn info(dir: &str) -> Option<GitInfo> {
    let rev = parse_rev_parse(&git(
        dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ],
    )?)?;
    let status = git(
        dir,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=normal",
        ],
    )
    .map(|s| parse_status_v2(&s));
    Some(GitInfo {
        top_level: windows_path(&rev.top_level),
        repo_name: repo_name(&rev.common_dir),
        common_dir: windows_path(&rev.common_dir),
        branch: status.as_ref().and_then(|s| s.branch.clone()),
        head: status
            .as_ref()
            .and_then(|s| s.oid.as_deref().map(short_oid)),
        dirty: status.as_ref().map(|s| s.dirty),
        linked_worktree: !same_path(&rev.git_dir, &rev.common_dir),
    })
}

/// Every worktree of the repository that contains `dir`.
pub fn worktrees(dir: &str) -> Vec<Worktree> {
    git(dir, &["worktree", "list", "--porcelain"])
        .map(|s| {
            parse_worktree_list(&s)
                .into_iter()
                .map(|mut w| {
                    w.path = windows_path(&w.path);
                    // A repository without commits lists HEAD as forty zeros.
                    w.head = w
                        .head
                        .as_deref()
                        .filter(|h| h.chars().any(|c| c != '0'))
                        .map(short_oid);
                    w
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `git version 2.47.1.windows.1` → `2.47.1.windows.1`.
pub fn version() -> Option<String> {
    let out = cmd::run(&Spec::new("git", &["--version"], TIMEOUT)).ok()?;
    out.ok().then(|| {
        out.stdout_text()
            .trim()
            .trim_start_matches("git version ")
            .to_string()
    })
}

/// Per-folder cache: status on a large repository takes a moment and a refresh runs every few
/// seconds.
pub struct GitCache {
    ttl: Duration,
    entries: Mutex<HashMap<String, (Instant, Option<GitInfo>)>>,
}

impl GitCache {
    pub fn new(ttl: Duration) -> Self {
        GitCache {
            ttl,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn info(&self, dir: &str) -> Option<GitInfo> {
        let key = dir.to_ascii_lowercase();
        if let Ok(map) = self.entries.lock()
            && let Some((at, value)) = map.get(&key)
            && at.elapsed() < self.ttl
        {
            return value.clone();
        }
        let value = info(dir);
        if let Ok(mut map) = self.entries.lock() {
            map.retain(|_, (at, _)| at.elapsed() < self.ttl * 4);
            map.insert(key, (Instant::now(), value.clone()));
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo(dir: &std::path::Path) -> bool {
        let d = dir.to_str().unwrap();
        let run = |args: &[&str]| {
            let mut full = vec!["-C", d];
            full.extend_from_slice(args);
            cmd::run(&Spec::new("git", &full, TIMEOUT))
                .map(|o| o.ok())
                .unwrap_or(false)
        };
        run(&["init", "-q", "-b", "main"])
            && run(&[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ])
    }

    #[test]
    fn repo_and_linked_worktree() {
        if version().is_none() {
            eprintln!("git not installed; skipping");
            return;
        }
        let base = std::env::temp_dir().join(format!("pp-git-{}", std::process::id()));
        let main = base.join("shop");
        let wt = base.join("shop-auth");
        std::fs::create_dir_all(&main).unwrap();
        assert!(init_repo(&main));
        let m = main.to_str().unwrap();
        let ok = cmd::run(&Spec::new(
            "git",
            &[
                "-C",
                m,
                "worktree",
                "add",
                "-q",
                "-b",
                "feat/auth",
                wt.to_str().unwrap(),
            ],
            TIMEOUT,
        ))
        .unwrap()
        .ok();
        assert!(ok);
        std::fs::write(wt.join("new.txt"), "x").unwrap();

        let main_info = info(m).unwrap();
        assert_eq!(main_info.repo_name, "shop");
        assert_eq!(main_info.branch.as_deref(), Some("main"));
        assert_eq!(main_info.dirty, Some(false));
        assert!(!main_info.linked_worktree);
        assert_eq!(main_info.head.as_ref().map(String::len), Some(7));

        let wt_info = info(wt.to_str().unwrap()).unwrap();
        assert_eq!(wt_info.repo_name, "shop");
        assert_eq!(wt_info.branch.as_deref(), Some("feat/auth"));
        assert_eq!(wt_info.dirty, Some(true));
        assert!(wt_info.linked_worktree);
        assert!(same_path(&wt_info.common_dir, &main_info.common_dir));

        let list = worktrees(m);
        assert_eq!(list.len(), 2);
        assert!(
            list.iter()
                .any(|w| w.branch.as_deref() == Some("feat/auth"))
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
