//! Parsers for the Git commands PortPilot runs. Git's own CLI is the source of truth for
//! worktree layout; nothing here reads `.git` directly.
//!
//! Commands, all with `-C <dir>`:
//! - `git rev-parse --path-format=absolute --show-toplevel --git-dir --git-common-dir`
//! - `git status --porcelain=v2 --branch --untracked-files=normal`
//! - `git worktree list --porcelain`

use crate::infer::folder_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevParse {
    pub top_level: String,
    pub git_dir: String,
    pub common_dir: String,
}

/// Git prints forward slashes on Windows; paths are returned as Git printed them, minus the
/// trailing newline. Callers convert separators for display.
pub fn parse_rev_parse(out: &str) -> Option<RevParse> {
    let mut lines = out.lines().map(str::trim).filter(|l| !l.is_empty());
    Some(RevParse {
        top_level: lines.next()?.to_string(),
        git_dir: lines.next()?.to_string(),
        common_dir: lines.next()?.to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// Full commit id; `None` before the first commit.
    pub oid: Option<String>,
    /// `None` on a detached HEAD.
    pub branch: Option<String>,
    pub dirty: bool,
}

pub fn parse_status_v2(out: &str) -> Status {
    let mut st = Status::default();
    for line in out.lines() {
        if let Some(oid) = line.strip_prefix("# branch.oid ") {
            st.oid = (oid != "(initial)").then(|| oid.trim().to_string());
        } else if let Some(head) = line.strip_prefix("# branch.head ") {
            st.branch = (head != "(detached)").then(|| head.trim().to_string());
        } else if !line.starts_with('#') && !line.trim().is_empty() {
            st.dirty = true;
        }
    }
    st
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Worktree {
    pub path: String,
    pub head: Option<String>,
    /// Short branch name without `refs/heads/`.
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
}

pub fn parse_worktree_list(out: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    let mut cur: Option<Worktree> = None;
    for line in out.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            if let Some(w) = cur.take() {
                list.push(w);
            }
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => {
                if let Some(w) = cur.take() {
                    list.push(w);
                }
                cur = Some(Worktree {
                    path: value.to_string(),
                    ..Worktree::default()
                });
            }
            _ => {
                let Some(w) = cur.as_mut() else { continue };
                match key {
                    "HEAD" => w.head = Some(value.to_string()),
                    "branch" => {
                        w.branch = Some(
                            value
                                .strip_prefix("refs/heads/")
                                .unwrap_or(value)
                                .to_string(),
                        )
                    }
                    "detached" => w.detached = true,
                    "bare" => w.bare = true,
                    "locked" => w.locked = true,
                    "prunable" => w.prunable = true,
                    _ => {}
                }
            }
        }
    }
    if let Some(w) = cur {
        list.push(w);
    }
    list
}

/// The repository's name from its common Git directory: the folder that holds `.git`, or a
/// bare repository's folder without the `.git` suffix.
pub fn repo_name(common_dir: &str) -> String {
    let trimmed = common_dir.trim_end_matches(['/', '\\']);
    let last = folder_name(trimmed);
    if last.eq_ignore_ascii_case(".git") {
        let parent = &trimmed[..trimmed.len() - last.len()];
        return folder_name(parent.trim_end_matches(['/', '\\']));
    }
    last.strip_suffix(".git").unwrap_or(&last).to_string()
}

/// Paths in Git output compare equal regardless of separators and case on Windows.
pub fn same_path(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

pub fn short_oid(oid: &str) -> String {
    oid.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rev_parse_lines() {
        let out = "C:/src/shop\nC:/src/shop/.git/worktrees/auth\nC:/src/shop/.git\n";
        assert_eq!(
            parse_rev_parse(out),
            Some(RevParse {
                top_level: "C:/src/shop".into(),
                git_dir: "C:/src/shop/.git/worktrees/auth".into(),
                common_dir: "C:/src/shop/.git".into(),
            })
        );
        assert_eq!(parse_rev_parse("only\n"), None);
    }

    #[test]
    fn status_v2() {
        let clean = "# branch.oid 5f32a0d1e2c3\n# branch.head feature/auth\n# branch.upstream origin/feature/auth\n# branch.ab +0 -0\n";
        assert_eq!(
            parse_status_v2(clean),
            Status {
                oid: Some("5f32a0d1e2c3".into()),
                branch: Some("feature/auth".into()),
                dirty: false
            }
        );
        let dirty = "# branch.oid (initial)\n# branch.head (detached)\n1 .M N... 100644 100644 100644 a b src/x.ts\n? new.txt\n";
        assert_eq!(
            parse_status_v2(dirty),
            Status {
                oid: None,
                branch: None,
                dirty: true
            }
        );
    }

    #[test]
    fn worktree_porcelain() {
        let out = "worktree C:/src/shop\nHEAD 5f32a0d\nbranch refs/heads/main\n\nworktree D:/wt/auth\nHEAD a82f31c\nbranch refs/heads/feat/auth\nlocked claude agent\n\nworktree D:/wt/old\nHEAD 1111111\ndetached\nprunable gitdir file points to non-existent location\n";
        let list = parse_worktree_list(out);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert_eq!(list[1].path, "D:/wt/auth");
        assert_eq!(list[1].branch.as_deref(), Some("feat/auth"));
        assert!(list[1].locked);
        assert!(list[2].detached && list[2].prunable);
        assert_eq!(list[2].branch, None);
    }

    #[test]
    fn repo_names() {
        assert_eq!(repo_name("C:/src/shop/.git"), "shop");
        assert_eq!(repo_name(r"C:\src\shop\.git\"), "shop");
        assert_eq!(repo_name("/srv/git/shop.git"), "shop");
        assert_eq!(repo_name("/home/a/api/.git"), "api");
    }

    #[test]
    fn path_comparison() {
        assert!(same_path("C:/src/shop/", r"c:\src\shop"));
        assert!(!same_path("C:/src/shop", "C:/src/shop2"));
        assert_eq!(short_oid("5f32a0d1e2c3"), "5f32a0d");
    }
}
