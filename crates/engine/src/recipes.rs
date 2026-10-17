//! Finding and reading a project's `.portpilot.toml`, and the facts around it: the project's
//! name, its repository and worktree (the lease owner), and the exact folder a run starts in.
//!
//! Nothing here runs anything. The recipe is read only when the user asks: `portctl run`,
//! `portctl lease`, or the Run button in the app.

use std::path::{Path, PathBuf};

use portpilot_core::infer::folder_name;
use portpilot_core::lease::LeaseOwner;
use portpilot_core::recipe::{MAX_RECIPE_BYTES, RECIPE_FILE, Recipe, RunRecipe, parse_recipe};

use crate::git;

/// Where a recipe sits and who owns its leases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectContext {
    /// The folder that holds `.portpilot.toml`.
    pub folder: String,
    /// The recipe's `[project] name`, or the repository's or folder's name.
    pub name: String,
    pub owner: LeaseOwner,
    pub branch: Option<String>,
    pub linked_worktree: bool,
}

#[derive(Debug, Clone)]
pub struct LoadedRecipe {
    pub context: ProjectContext,
    pub recipe: Recipe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// No `.portpilot.toml` in the folder or above it, up to the repository root.
    NotFound(String),
    Invalid(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::NotFound(dir) => write!(f, "no {RECIPE_FILE} in {dir}"),
            LoadError::Invalid(e) => write!(f, "{RECIPE_FILE}: {e}"),
        }
    }
}

/// `\\?\C:\src` from `canonicalize` is `C:\src` to everyone else.
pub fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{unc}");
    }
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// The folder with the recipe: `start` or the nearest parent that has one, without leaving
/// the repository (a folder with `.git` is the last one looked at).
pub fn find_recipe_folder(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start.to_path_buf());
    let mut depth = 0;
    while let Some(d) = dir {
        if d.join(RECIPE_FILE).is_file() {
            return Some(d);
        }
        if d.join(".git").exists() || depth >= 16 {
            return None;
        }
        dir = d.parent().map(Path::to_path_buf);
        depth += 1;
    }
    None
}

/// The lease owner and display name for a folder.
pub fn project_context(folder: &str, recipe_name: Option<&str>) -> ProjectContext {
    let info = git::info(folder);
    let owner = match &info {
        Some(g) => LeaseOwner {
            common_dir: Some(g.common_dir.clone()),
            worktree: g.top_level.clone(),
        },
        None => LeaseOwner {
            common_dir: None,
            worktree: folder.to_string(),
        },
    };
    ProjectContext {
        folder: folder.to_string(),
        name: recipe_name
            .map(str::to_string)
            .or_else(|| info.as_ref().map(|g| g.repo_name.clone()))
            .unwrap_or_else(|| folder_name(folder)),
        branch: info.as_ref().and_then(|g| g.branch.clone()),
        linked_worktree: info.as_ref().is_some_and(|g| g.linked_worktree),
        owner,
    }
}

/// Reads and parses the recipe in `folder` or above it.
pub fn load(start: &Path) -> Result<LoadedRecipe, LoadError> {
    let start =
        std::fs::canonicalize(start).map_err(|_| LoadError::NotFound(display_path(start)))?;
    let folder =
        find_recipe_folder(&start).ok_or_else(|| LoadError::NotFound(display_path(&start)))?;
    let path = folder.join(RECIPE_FILE);
    let size = std::fs::metadata(&path)
        .map_err(|e| LoadError::Invalid(e.to_string()))?
        .len();
    if size > MAX_RECIPE_BYTES {
        return Err(LoadError::Invalid(format!(
            "the file is larger than {} KiB",
            MAX_RECIPE_BYTES / 1024
        )));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| LoadError::Invalid(e.to_string()))?;
    let recipe = parse_recipe(text.trim_start_matches('\u{feff}')).map_err(LoadError::Invalid)?;
    let folder = display_path(&folder);
    Ok(LoadedRecipe {
        context: project_context(&folder, recipe.name.as_deref()),
        recipe,
    })
}

/// The exact folder a run starts in: the recipe's folder or its `cwd` below it. Links that
/// lead outside the project are refused, since the user approves the folder they see.
pub fn run_folder(ctx: &ProjectContext, run: &RunRecipe) -> Result<String, String> {
    let base = std::fs::canonicalize(&ctx.folder).map_err(|e| format!("{}: {e}", ctx.folder))?;
    let target = match &run.cwd {
        Some(rel) => base.join(rel),
        None => base.clone(),
    };
    let real = std::fs::canonicalize(&target)
        .map_err(|_| format!("{} does not exist", display_path(&target)))?;
    if !real.is_dir() {
        return Err(format!("{} is not a folder", display_path(&real)));
    }
    if !real.starts_with(&base) {
        return Err(format!(
            "{} leads outside the project folder",
            display_path(&target)
        ));
    }
    Ok(display_path(&real))
}

/// SHA-256 of what the user approves when they trust a run.
pub fn trust_digest(run_name: &str, run: &RunRecipe) -> Result<String, String> {
    portpilot_win::crypto::sha256_hex(run.trust_text(run_name).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pp-recipe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_the_recipe_above_a_subfolder_but_not_above_the_repository() {
        let root = temp("find");
        let repo = root.join("shop");
        std::fs::create_dir_all(repo.join("apps").join("web")).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(root.join(RECIPE_FILE), "").unwrap();
        // The file above the repository root does not count.
        assert_eq!(find_recipe_folder(&repo.join("apps").join("web")), None);
        std::fs::write(
            repo.join(RECIPE_FILE),
            "[project]\nname = \"shop\"\n[ports.web]\npreferred = 5173\n[run.dev]\ncommand = \"pnpm dev --port {ports.web}\"\ncwd = \"apps/web\"\n",
        )
        .unwrap();
        assert_eq!(
            find_recipe_folder(&repo.join("apps").join("web")),
            Some(repo.clone())
        );
        let loaded = load(&repo.join("apps")).unwrap();
        assert_eq!(loaded.context.name, "shop");
        assert!(loaded.context.folder.ends_with("shop"));
        let folder = run_folder(&loaded.context, &loaded.recipe.runs["dev"]).unwrap();
        assert!(folder.ends_with(r"shop\apps\web"), "{folder}");
        assert!(!folder.starts_with(r"\\?\"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_and_invalid_recipes() {
        let root = temp("bad");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        assert!(matches!(load(&root), Err(LoadError::NotFound(_))));
        std::fs::write(root.join(RECIPE_FILE), "[run.dev]\n").unwrap();
        assert!(matches!(load(&root), Err(LoadError::Invalid(_))));
        std::fs::write(root.join(RECIPE_FILE), "x".repeat(70 * 1024)).unwrap();
        let err = load(&root).unwrap_err();
        assert!(err.to_string().contains("KiB"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn run_folder_must_exist() {
        let root = temp("cwd");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(
            root.join(RECIPE_FILE),
            "[run.dev]\ncommand = \"x\"\ncwd = \"missing\"\n",
        )
        .unwrap();
        let loaded = load(&root).unwrap();
        assert!(run_folder(&loaded.context, &loaded.recipe.runs["dev"]).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn digests_change_with_the_command() {
        let a = parse_recipe("[run.dev]\ncommand = \"pnpm dev\"\n").unwrap();
        let b = parse_recipe("[run.dev]\ncommand = \"pnpm dev --open\"\n").unwrap();
        let da = trust_digest("dev", &a.runs["dev"]).unwrap();
        assert_eq!(da.len(), 64);
        assert_ne!(da, trust_digest("dev", &b.runs["dev"]).unwrap());
        assert_eq!(da, trust_digest("dev", &a.runs["dev"]).unwrap());
    }
}
