//! Inference rules: which project folder a process belongs to, who launched it, and what kind
//! of process it is. Inputs are names, paths and command lines; file system checks go through
//! the `Fs` trait so the rules are testable.

use crate::cmdline::split_windows;
use crate::model::{AttributionSource, LauncherKind, ProcessClass};

/// Files that mark a project root. `.git` is last: a manifest in a sub-package wins.
pub const MANIFESTS: &[&str] = &[
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "requirements.txt",
    "Pipfile",
    "go.mod",
    "deno.json",
    "deno.jsonc",
    "composer.json",
    "Gemfile",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "mix.exs",
    "pubspec.yaml",
    "compose.yaml",
    "compose.yml",
    "docker-compose.yml",
    "docker-compose.yaml",
    "pnpm-workspace.yaml",
    "manage.py",
    ".git",
];

/// Extensions that mark a .NET project root.
pub const MANIFEST_EXTENSIONS: &[&str] = &["sln", "slnx", "csproj", "fsproj", "vbproj"];

pub trait Fs {
    fn exists(&self, path: &str) -> bool;
    fn is_dir(&self, path: &str) -> bool;
    /// Names of files in `dir` with one of `exts` (lowercase, without the dot).
    fn files_with_ext(&self, dir: &str, exts: &[&str]) -> Vec<String>;
}

pub struct InferContext<'a> {
    pub fs: &'a dyn Fs,
    /// Lowercase folder prefixes that never contain projects (Windows, Program Files,
    /// AppData, toolchain homes).
    pub denied: Vec<String>,
    /// The user's profile folder: a project is never the profile itself.
    pub home: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inferred {
    pub root: String,
    pub source: AttributionSource,
    pub manifests: Vec<String>,
}

/// Lexically normalizes a Windows path: `/` to `\`, no `\\?\`, `.` and `..` resolved, no
/// trailing separator except on a drive root.
pub fn normalize_windows_path(path: &str) -> String {
    let p = path.trim().trim_matches('"').replace('/', "\\");
    let p = p.strip_prefix(r"\\?\").unwrap_or(&p).to_string();
    let (prefix, rest) = if let Some(unc) = p.strip_prefix(r"\\") {
        // UNC: keep \\server\share as the prefix.
        let parts: Vec<&str> = unc.splitn(3, '\\').collect();
        if parts.len() < 2 {
            return p;
        }
        (
            format!(r"\\{}\{}", parts[0], parts[1]),
            parts.get(2).copied().unwrap_or("").to_string(),
        )
    } else if p.len() >= 2 && p.as_bytes()[1] == b':' {
        (p[..2].to_ascii_uppercase(), p[2..].to_string())
    } else {
        (String::new(), p.clone())
    };
    let mut segs: Vec<&str> = Vec::new();
    for s in rest.split('\\') {
        match s {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    if prefix.is_empty() {
        return segs.join("\\");
    }
    if segs.is_empty() {
        return if prefix.ends_with(':') {
            format!("{prefix}\\")
        } else {
            prefix
        };
    }
    format!("{prefix}\\{}", segs.join("\\"))
}

pub fn is_absolute_windows_path(s: &str) -> bool {
    let b = s.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
        || s.starts_with(r"\\")
}

fn parent(path: &str) -> Option<String> {
    let idx = path.rfind('\\')?;
    if idx == 2 && path.as_bytes()[1] == b':' {
        if path.len() == 3 {
            return None;
        }
        return Some(path[..3].to_string());
    }
    if path.starts_with(r"\\") && path[2..].matches('\\').count() <= 1 {
        return None;
    }
    Some(path[..idx].to_string())
}

fn is_drive_root(path: &str) -> bool {
    path.len() == 3 && path.ends_with(":\\")
}

fn starts_with_dir(path_lower: &str, prefix_lower: &str) -> bool {
    let prefix = prefix_lower.trim_end_matches('\\');
    path_lower == prefix
        || (path_lower.starts_with(prefix)
            && path_lower.as_bytes().get(prefix.len()) == Some(&b'\\'))
}

impl InferContext<'_> {
    fn is_denied(&self, dir: &str) -> bool {
        let lower = dir.to_ascii_lowercase();
        self.denied.iter().any(|d| starts_with_dir(&lower, d))
    }

    fn is_home(&self, dir: &str) -> bool {
        self.home
            .as_deref()
            .is_some_and(|h| h.trim_end_matches('\\').eq_ignore_ascii_case(dir))
    }

    fn manifests_in(&self, dir: &str) -> Vec<String> {
        let mut found: Vec<String> = MANIFESTS
            .iter()
            .filter(|m| self.fs.exists(&format!("{dir}\\{m}")))
            .map(|m| m.to_string())
            .collect();
        found.extend(self.fs.files_with_ext(dir, MANIFEST_EXTENSIONS));
        found
    }

    /// Walks up from `start` to the first folder with a manifest.
    fn walk_up(&self, start: &str) -> Option<(String, Vec<String>)> {
        let mut dir = Some(start.to_string());
        let mut depth = 0;
        while let Some(d) = dir {
            if depth > 16 || is_drive_root(&d) || self.is_home(&d) || self.is_denied(&d) {
                return None;
            }
            let m = self.manifests_in(&d);
            if !m.is_empty() {
                return Some((d, m));
            }
            dir = parent(&d);
            depth += 1;
        }
        None
    }

    /// The project folder that contains `raw` (a file or a folder) and its manifests.
    pub fn root_for_path(&self, raw: &str) -> Option<(String, Vec<String>)> {
        let path = normalize_windows_path(raw);
        let lower = path.to_ascii_lowercase();
        // Anything under node_modules belongs to the folder that holds the outermost one.
        let start = if let Some(i) = lower.find("\\node_modules\\") {
            path[..i].to_string()
        } else if lower.ends_with("\\node_modules") {
            path[..path.len() - "\\node_modules".len()].to_string()
        } else if self.fs.is_dir(&path) {
            path
        } else {
            parent(&path)?
        };
        if self.is_denied(&start) {
            return None;
        }
        self.walk_up(&start)
    }

    /// Infers the project of a Windows process from absolute paths in its arguments, then from
    /// its executable's folder.
    pub fn project(&self, command_line: Option<&str>, exe: Option<&str>) -> Option<Inferred> {
        if let Some(cmd) = command_line {
            // argv[0] is the executable and is handled below.
            for arg in split_windows(cmd).iter().skip(1) {
                let value = arg
                    .split_once('=')
                    .map(|(_, v)| v)
                    .filter(|v| is_absolute_windows_path(v))
                    .unwrap_or(arg);
                if !is_absolute_windows_path(value) {
                    continue;
                }
                if let Some((root, manifests)) = self.root_for_path(value) {
                    return Some(Inferred {
                        root,
                        source: AttributionSource::CommandLine,
                        manifests,
                    });
                }
            }
        }
        let exe = exe?;
        let (root, manifests) = self.root_for_path(exe)?;
        Some(Inferred {
            root,
            source: AttributionSource::ExecutablePath,
            manifests,
        })
    }
}

/// The last folder name of a Windows or Linux path.
pub fn folder_name(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_string()
}

fn base_name(name: &str) -> String {
    name.to_ascii_lowercase()
        .trim_end_matches(".exe")
        .to_string()
}

fn cmd_has(cmd: Option<&str>, needles: &[&str]) -> bool {
    let Some(cmd) = cmd else { return false };
    let lower = cmd.to_ascii_lowercase().replace('/', "\\");
    needles.iter().any(|n| lower.contains(n))
}

/// What a single ancestor says about who started the chain.
pub fn launcher_kind(name: &str, command_line: Option<&str>) -> Option<LauncherKind> {
    let n = base_name(name);
    let kind = match n.as_str() {
        "claude" => LauncherKind::Claude,
        "codex" => LauncherKind::Codex,
        "gemini" => LauncherKind::Gemini,
        "node" | "bun" | "deno"
            if cmd_has(
                command_line,
                &["@anthropic-ai\\claude-code", "\\claude-code\\"],
            ) =>
        {
            LauncherKind::Claude
        }
        "node" | "bun" | "deno" if cmd_has(command_line, &["@openai\\codex"]) => {
            LauncherKind::Codex
        }
        "node" | "bun" | "deno" if cmd_has(command_line, &["@google\\gemini-cli"]) => {
            LauncherKind::Gemini
        }
        "code" | "code - insiders" | "codium" | "vscodium" => LauncherKind::VsCode,
        "cursor" => LauncherKind::Cursor,
        "windsurf" => LauncherKind::Windsurf,
        "zed" => LauncherKind::Zed,
        "idea64" | "idea" | "webstorm64" | "pycharm64" | "rider64" | "goland64" | "clion64"
        | "rustrover64" | "phpstorm64" | "datagrip64" | "rubymine64" | "fleet" => {
            LauncherKind::JetBrains
        }
        "windowsterminal" | "wezterm-gui" | "alacritty" | "hyper" | "tabby" | "conemu64"
        | "conemu" | "mintty" | "warp" => LauncherKind::Terminal,
        "explorer" => LauncherKind::Explorer,
        "services" => LauncherKind::Service,
        _ => return None,
    };
    Some(kind)
}

/// The nearest ancestor with a launcher kind; `chain` is parent first.
pub fn detect_launcher<'a>(
    chain: impl IntoIterator<Item = (u32, &'a str, Option<&'a str>)>,
) -> Option<(LauncherKind, u32, String)> {
    chain
        .into_iter()
        .find_map(|(pid, name, cmd)| launcher_kind(name, cmd).map(|k| (k, pid, name.to_string())))
}

const SYSTEM_PROCESSES: &[&str] = &[
    "system",
    "idle",
    "system idle process",
    "registry",
    "secure system",
    "smss",
    "csrss",
    "wininit",
    "winlogon",
    "services",
    "lsass",
    "lsaiso",
    "memory compression",
];

const SERVICE_PROCESSES: &[&str] = &[
    "svchost",
    "spoolsv",
    "dllhost",
    "wmiprvse",
    "msmpeng",
    "searchindexer",
];

const RELAY_PROCESSES: &[&str] = &[
    "wslrelay",
    "wslhost",
    "wslservice",
    "com.docker.backend",
    "com.docker.proxy",
    "com.docker.vpnkit",
    "vpnkit",
    "docker-proxy",
    "vmcompute",
    "vmwp",
];

const SERVICE_ACCOUNTS: &[&str] = &[
    "nt authority\\system",
    "nt authority\\local service",
    "nt authority\\network service",
];

pub fn classify_process(pid: u32, name: &str, user: Option<&str>) -> ProcessClass {
    let n = base_name(name);
    if pid == 0 || pid == 4 || SYSTEM_PROCESSES.contains(&n.as_str()) {
        return ProcessClass::System;
    }
    if RELAY_PROCESSES.contains(&n.as_str()) {
        return ProcessClass::Relay;
    }
    if SERVICE_PROCESSES.contains(&n.as_str()) {
        return ProcessClass::Service;
    }
    if let Some(u) = user
        && SERVICE_ACCOUNTS.contains(&u.to_ascii_lowercase().as_str())
    {
        return ProcessClass::Service;
    }
    ProcessClass::User
}

/// Docker's host-side forwarders, as opposed to WSL's.
pub fn is_docker_relay(name: &str) -> bool {
    let n = base_name(name);
    n.starts_with("com.docker.") || n == "vpnkit" || n == "docker-proxy"
}

pub fn is_wsl_relay(name: &str) -> bool {
    base_name(name) == "wslrelay"
}

/// Shells that may receive CTRL+C together with the target, since that is what pressing
/// CTRL+C in the same terminal does.
pub fn is_shell(name: &str) -> bool {
    matches!(
        base_name(name).as_str(),
        "cmd" | "powershell" | "pwsh" | "bash" | "sh" | "zsh" | "fish" | "nu" | "elvish" | "xonsh"
    )
}

/// Package managers and runners between a shell and the server (`pnpm dev`, `npm run dev`).
pub fn is_runner(name: &str, command_line: Option<&str>) -> bool {
    let n = base_name(name);
    match n.as_str() {
        "pnpm" | "npm" | "npx" | "yarn" | "bun" | "bunx" | "deno" | "uv" | "uvx" | "poetry"
        | "cargo" | "go" | "dotnet" | "make" | "just" | "turbo" | "nx" | "concurrently"
        | "nodemon" => true,
        "node" => cmd_has(
            command_line,
            &[
                "npm-cli.js",
                "npx-cli.js",
                "\\pnpm\\",
                "pnpm.cjs",
                "pnpm.js",
                "\\yarn\\",
                "yarn.js",
                "\\.bin\\",
                "concurrently",
                "nodemon",
                "turbo",
                "\\nx\\",
            ],
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    #[derive(Default)]
    struct FakeFs {
        files: HashSet<String>,
        dirs: HashSet<String>,
        listing: HashMap<String, Vec<String>>,
    }

    impl FakeFs {
        fn with(files: &[&str], dirs: &[&str]) -> Self {
            FakeFs {
                files: files.iter().map(|s| s.to_ascii_lowercase()).collect(),
                dirs: dirs.iter().map(|s| s.to_ascii_lowercase()).collect(),
                listing: HashMap::new(),
            }
        }
    }

    impl Fs for FakeFs {
        fn exists(&self, path: &str) -> bool {
            let p = path.to_ascii_lowercase();
            self.files.contains(&p) || self.dirs.contains(&p)
        }
        fn is_dir(&self, path: &str) -> bool {
            self.dirs.contains(&path.to_ascii_lowercase())
        }
        fn files_with_ext(&self, dir: &str, _exts: &[&str]) -> Vec<String> {
            self.listing
                .get(&dir.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default()
        }
    }

    fn ctx(fs: &FakeFs) -> InferContext<'_> {
        InferContext {
            fs,
            denied: vec![
                r"c:\program files".into(),
                r"c:\windows".into(),
                r"c:\users\dev\appdata".into(),
            ],
            home: Some(r"C:\Users\dev".into()),
        }
    }

    #[test]
    fn normalizes_paths() {
        assert_eq!(
            normalize_windows_path(r"C:\src\shop\node_modules\.bin\..\vite\bin\vite.js"),
            r"C:\src\shop\node_modules\vite\bin\vite.js"
        );
        assert_eq!(normalize_windows_path("c:/src/app/"), r"C:\src\app");
        assert_eq!(normalize_windows_path(r"\\?\D:\wt\auth"), r"D:\wt\auth");
        assert_eq!(normalize_windows_path(r"C:\"), r"C:\");
        assert_eq!(normalize_windows_path(r"C:\.."), r"C:\");
        assert_eq!(
            normalize_windows_path(r"\\wsl.localhost\Ubuntu\home\a"),
            r"\\wsl.localhost\Ubuntu\home\a"
        );
    }

    #[test]
    fn node_modules_script_maps_to_project() {
        let fs = FakeFs::with(&[r"C:\src\shop\package.json", r"C:\src\shop\.git"], &[]);
        let cmd = r#""C:\Program Files\nodejs\node.exe" C:\src\shop\node_modules\.bin\..\vite\bin\vite.js --port 5173"#;
        let got = ctx(&fs)
            .project(Some(cmd), Some(r"C:\Program Files\nodejs\node.exe"))
            .unwrap();
        assert_eq!(got.root, r"C:\src\shop");
        assert_eq!(got.source, AttributionSource::CommandLine);
        assert_eq!(
            got.manifests,
            vec!["package.json".to_string(), ".git".to_string()]
        );
    }

    #[test]
    fn monorepo_package_wins_over_repo_root() {
        let fs = FakeFs::with(
            &[r"C:\src\mono\.git", r"C:\src\mono\apps\web\package.json"],
            &[],
        );
        let cmd = r"node C:\src\mono\apps\web\node_modules\next\dist\bin\next dev";
        assert_eq!(
            ctx(&fs).project(Some(cmd), None).unwrap().root,
            r"C:\src\mono\apps\web"
        );
    }

    #[test]
    fn venv_executable_maps_to_project() {
        let fs = FakeFs::with(&[r"C:\work\api\pyproject.toml"], &[]);
        let got = ctx(&fs)
            .project(
                Some("python manage.py runserver"),
                Some(r"C:\work\api\.venv\Scripts\python.exe"),
            )
            .unwrap();
        assert_eq!(got.root, r"C:\work\api");
        assert_eq!(got.source, AttributionSource::ExecutablePath);
    }

    #[test]
    fn global_tools_and_profile_are_not_projects() {
        let fs = FakeFs::with(
            &[
                r"C:\Users\dev\AppData\Roaming\npm\package.json",
                r"C:\Users\dev\package.json",
            ],
            &[],
        );
        let c = ctx(&fs);
        assert_eq!(
            c.project(
                Some(r"node C:\Users\dev\AppData\Roaming\npm\node_modules\serve\bin\serve.js"),
                None
            ),
            None
        );
        assert_eq!(c.project(Some(r"node C:\Users\dev\server.js"), None), None);
        assert_eq!(
            c.project(None, Some(r"C:\Program Files\nodejs\node.exe")),
            None
        );
    }

    #[test]
    fn flag_values_and_directories_count() {
        let fs = FakeFs::with(&[r"D:\wt\auth\Cargo.toml"], &[r"D:\wt\auth"]);
        assert_eq!(
            ctx(&fs)
                .project(Some(r"server --root=D:\wt\auth"), None)
                .unwrap()
                .root,
            r"D:\wt\auth"
        );
    }

    #[test]
    fn dotnet_projects_by_extension() {
        let mut fs = FakeFs::with(&[], &[]);
        fs.listing
            .insert(r"c:\src\shopapi".into(), vec!["ShopApi.csproj".into()]);
        let got = ctx(&fs)
            .project(None, Some(r"C:\src\ShopApi\bin\Debug\net9.0\ShopApi.exe"))
            .unwrap();
        assert_eq!(got.root, r"C:\src\ShopApi");
        assert_eq!(got.manifests, vec!["ShopApi.csproj".to_string()]);
    }

    #[test]
    fn launchers_nearest_first() {
        let chain = vec![
            (10, "bash.exe", None),
            (
                9,
                "node.exe",
                Some(
                    r"node C:\Users\dev\AppData\Roaming\npm\node_modules\@anthropic-ai\claude-code\cli.js",
                ),
            ),
            (8, "pwsh.exe", None),
            (7, "Code.exe", None),
        ];
        let (kind, pid, _) = detect_launcher(chain).unwrap();
        assert_eq!((kind, pid), (LauncherKind::Claude, 9));
        assert_eq!(
            launcher_kind("WindowsTerminal.exe", None),
            Some(LauncherKind::Terminal)
        );
        assert_eq!(
            launcher_kind("Code - Insiders.exe", None),
            Some(LauncherKind::VsCode)
        );
        assert_eq!(launcher_kind("node.exe", Some("node server.js")), None);
        assert_eq!(detect_launcher(Vec::new()), None);
    }

    #[test]
    fn process_classes() {
        assert_eq!(classify_process(4, "System", None), ProcessClass::System);
        assert_eq!(
            classify_process(900, "svchost.exe", None),
            ProcessClass::Service
        );
        assert_eq!(
            classify_process(901, "wslrelay.exe", None),
            ProcessClass::Relay
        );
        assert_eq!(
            classify_process(902, "com.docker.backend.exe", None),
            ProcessClass::Relay
        );
        assert_eq!(
            classify_process(903, "postgres.exe", Some(r"NT AUTHORITY\NETWORK SERVICE")),
            ProcessClass::Service
        );
        assert_eq!(
            classify_process(904, "node.exe", Some(r"DESKTOP\dev")),
            ProcessClass::User
        );
        assert!(is_docker_relay("com.docker.backend.exe"));
        assert!(!is_docker_relay("wslrelay.exe"));
        assert!(is_wsl_relay("wslrelay.exe"));
    }

    #[test]
    fn shells_and_runners() {
        assert!(is_shell("pwsh.exe"));
        assert!(!is_shell("node.exe"));
        assert!(is_runner(
            "node.exe",
            Some(r"node C:\Users\dev\AppData\Roaming\npm\node_modules\pnpm\bin\pnpm.cjs dev")
        ));
        assert!(is_runner("pnpm.exe", None));
        assert!(!is_runner("node.exe", Some(r"node C:\src\shop\server.js")));
    }

    #[test]
    fn folder_names() {
        assert_eq!(folder_name(r"C:\src\shop\"), "shop");
        assert_eq!(folder_name("/home/a/api"), "api");
    }
}
