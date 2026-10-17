//! `.portpilot.toml`: a project's opt-in recipe for managed runs and port leases.
//!
//! ```toml
//! [project]
//! name = "shop"
//!
//! [ports.web]
//! preferred = 5173
//! range = "5173-5199"
//!
//! [run.dev]
//! command = "pnpm dev --port {ports.web} --strictPort"
//! cwd = "apps/web"
//! env = { API_URL = "http://localhost:{ports.api}" }
//! ```
//!
//! PortPilot reads the file only when the user asks for it, never runs a command without the
//! user's confirmation, and passes a port to a command only where the recipe writes a
//! `{ports.<service>}` placeholder. Every other `{...}` is left as it is.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::net::{PortRange, parse_port_range};

/// The file name PortPilot looks for.
pub const RECIPE_FILE: &str = ".portpilot.toml";
/// Recipes are small; a larger file is refused before it is parsed.
pub const MAX_RECIPE_BYTES: u64 = 64 * 1024;
/// How far past `preferred` a port may be allocated when the recipe gives no range.
pub const DEFAULT_RANGE_SPAN: u16 = 99;

const MAX_COMMAND_CHARS: usize = 4096;
const MAX_ENV: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortSpec {
    pub preferred: u16,
    pub range: PortRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecipe {
    /// The command as written, with placeholders.
    pub command: String,
    /// A folder below the recipe's folder, with `\` separators; `None` for the recipe's folder.
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    pub name: Option<String>,
    pub ports: BTreeMap<String, PortSpec>,
    pub runs: BTreeMap<String, RunRecipe>,
}

/// A run with every placeholder replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedRun {
    pub command: String,
    pub env: Vec<(String, String)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRecipe {
    project: Option<RawProject>,
    #[serde(default)]
    ports: BTreeMap<String, RawPort>,
    #[serde(default)]
    run: BTreeMap<String, RawRun>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProject {
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawRange {
    Text(String),
    Pair([u32; 2]),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPort {
    preferred: Option<u32>,
    range: Option<RawRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRun {
    command: String,
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// Service and run names: a letter or digit, then letters, digits, `-` and `_`.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    key.len() <= 128
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn port(value: u32, what: &str) -> Result<u16, String> {
    u16::try_from(value)
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| format!("{what}: {value} is not a port (1-65535)"))
}

fn port_spec(service: &str, raw: RawPort) -> Result<PortSpec, String> {
    let what = format!("ports.{service}");
    let range = match raw.range {
        Some(RawRange::Text(t)) => {
            Some(parse_port_range(&t).map_err(|e| format!("{what}.range: {e}"))?)
        }
        Some(RawRange::Pair([a, b])) => {
            let (start, end) = (port(a, &what)?, port(b, &what)?);
            if end < start {
                return Err(format!("{what}.range: {start}-{end} ends before it starts"));
            }
            Some(PortRange { start, end })
        }
        None => None,
    };
    let preferred = raw
        .preferred
        .map(|p| port(p, &format!("{what}.preferred")))
        .transpose()?;
    match (preferred, range) {
        (Some(p), Some(r)) if !r.contains(p) => Err(format!(
            "{what}: preferred port {p} is outside {}-{}",
            r.start, r.end
        )),
        (Some(p), Some(r)) => Ok(PortSpec {
            preferred: p,
            range: r,
        }),
        (Some(p), None) => Ok(PortSpec {
            preferred: p,
            range: PortRange {
                start: p,
                end: p.saturating_add(DEFAULT_RANGE_SPAN),
            },
        }),
        (None, Some(r)) => Ok(PortSpec {
            preferred: r.start,
            range: r,
        }),
        (None, None) => Err(format!("{what}: give a preferred port, a range or both")),
    }
}

/// A relative folder that stays inside the recipe's folder: no drive, no root, no `..`.
fn relative_cwd(run: &str, raw: &str) -> Result<Option<String>, String> {
    let text = raw.trim().replace('/', "\\");
    let bad = || format!("run.{run}.cwd: {raw:?} must be a folder inside the project");
    if text.starts_with('\\') || text.contains(':') {
        return Err(bad());
    }
    let mut parts = Vec::new();
    for seg in text.split('\\') {
        match seg {
            "" | "." => {}
            ".." => return Err(bad()),
            s if s.chars().any(|c| c.is_control() || "<>\"|?*".contains(c)) => return Err(bad()),
            s => parts.push(s),
        }
    }
    Ok((!parts.is_empty()).then(|| parts.join("\\")))
}

/// Every `{ports.<name>}` in `text`, in order of appearance.
pub fn placeholders(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("{ports.") {
        let after = &rest[i + "{ports.".len()..];
        match after.find('}') {
            Some(end) if valid_name(&after[..end]) => {
                out.push(after[..end].to_string());
                rest = &after[end + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

fn substitute(text: &str, ports: &BTreeMap<String, u16>) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("{ports.") {
        out.push_str(&rest[..i]);
        let after = &rest[i + "{ports.".len()..];
        match after.find('}') {
            Some(end) if valid_name(&after[..end]) => {
                let name = &after[..end];
                let port = ports
                    .get(name)
                    .ok_or_else(|| format!("no port for service {name}"))?;
                out.push_str(&port.to_string());
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str("{ports.");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

impl RunRecipe {
    /// The services this run needs a port for, sorted.
    pub fn services(&self) -> Vec<String> {
        let mut set = BTreeSet::new();
        set.extend(placeholders(&self.command));
        for v in self.env.values() {
            set.extend(placeholders(v));
        }
        set.into_iter().collect()
    }

    pub fn render(&self, ports: &BTreeMap<String, u16>) -> Result<RenderedRun, String> {
        Ok(RenderedRun {
            command: substitute(&self.command, ports)?,
            env: self
                .env
                .iter()
                .map(|(k, v)| Ok((k.clone(), substitute(v, ports)?)))
                .collect::<Result<_, String>>()?,
        })
    }

    /// What the user approves when they trust a run: everything that decides what runs, and
    /// nothing that changes between runs (the ports). A different text asks again.
    pub fn trust_text(&self, run_name: &str) -> String {
        let mut text = format!(
            "portpilot-run-v1\0{run_name}\0{}\0{}\0",
            self.command,
            self.cwd.as_deref().unwrap_or("")
        );
        for (k, v) in &self.env {
            text.push_str(&format!("{k}={v}\0"));
        }
        text
    }
}

fn check_command(run: &str, command: &str) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err(format!("run.{run}.command is empty"));
    }
    if command.chars().count() > MAX_COMMAND_CHARS {
        return Err(format!(
            "run.{run}.command is longer than {MAX_COMMAND_CHARS} characters"
        ));
    }
    if command.chars().any(|c| c == '\0' || c == '\r' || c == '\n') {
        return Err(format!("run.{run}.command must be a single line"));
    }
    Ok(())
}

pub fn parse_recipe(text: &str) -> Result<Recipe, String> {
    let raw: RawRecipe = toml::from_str(text).map_err(|e| e.to_string().trim().to_string())?;
    let mut ports = BTreeMap::new();
    for (service, spec) in raw.ports {
        if !valid_name(&service) {
            return Err(format!(
                "ports.{service}: names use letters, digits, - and _"
            ));
        }
        ports.insert(service.clone(), port_spec(&service, spec)?);
    }
    let mut runs = BTreeMap::new();
    for (name, run) in raw.run {
        if !valid_name(&name) {
            return Err(format!("run.{name}: names use letters, digits, - and _"));
        }
        check_command(&name, &run.command)?;
        if run.env.len() > MAX_ENV {
            return Err(format!("run.{name}.env has more than {MAX_ENV} entries"));
        }
        for (k, v) in &run.env {
            if !valid_env_key(k) {
                return Err(format!("run.{name}.env: {k:?} is not a variable name"));
            }
            if v.contains('\0') {
                return Err(format!("run.{name}.env.{k} contains a NUL character"));
            }
        }
        let recipe = RunRecipe {
            command: run.command.trim().to_string(),
            cwd: match &run.cwd {
                Some(c) => relative_cwd(&name, c)?,
                None => None,
            },
            env: run.env,
        };
        for service in recipe.services() {
            if !ports.contains_key(&service) {
                return Err(format!(
                    "run.{name} uses {{ports.{service}}}, but there is no [ports.{service}]"
                ));
            }
        }
        runs.insert(name, recipe);
    }
    let name = raw
        .project
        .and_then(|p| p.name)
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if name.as_ref().is_some_and(|n| n.chars().count() > 128) {
        return Err("project.name is longer than 128 characters".into());
    }
    Ok(Recipe { name, ports, runs })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
[project]
name = "shop"

[ports.web]
preferred = 5173
range = "5173-5199"

[ports.api]
preferred = 8080

[ports.db]
range = [5432, 5440]

[run.dev]
command = "pnpm dev --port {ports.web} --strictPort"
cwd = "apps/web"
env = { API_URL = "http://localhost:{ports.api}", NODE_ENV = "development" }

[run.api]
command = "cargo run -- --port {ports.api}"
"#;

    #[test]
    fn parses_a_full_recipe() {
        let r = parse_recipe(FULL).unwrap();
        assert_eq!(r.name.as_deref(), Some("shop"));
        assert_eq!(
            r.ports["web"],
            PortSpec {
                preferred: 5173,
                range: PortRange {
                    start: 5173,
                    end: 5199
                }
            }
        );
        assert_eq!(
            r.ports["api"].range,
            PortRange {
                start: 8080,
                end: 8080 + DEFAULT_RANGE_SPAN
            }
        );
        assert_eq!(r.ports["db"].preferred, 5432);
        let dev = &r.runs["dev"];
        assert_eq!(dev.cwd.as_deref(), Some(r"apps\web"));
        assert_eq!(dev.services(), vec!["api".to_string(), "web".to_string()]);
        assert_eq!(r.runs["api"].services(), vec!["api".to_string()]);
    }

    #[test]
    fn renders_only_port_placeholders() {
        let r = parse_recipe(FULL).unwrap();
        let ports = BTreeMap::from([("web".to_string(), 5174), ("api".to_string(), 8081)]);
        let out = r.runs["dev"].render(&ports).unwrap();
        assert_eq!(out.command, "pnpm dev --port 5174 --strictPort");
        assert_eq!(
            out.env,
            vec![
                ("API_URL".to_string(), "http://localhost:8081".to_string()),
                ("NODE_ENV".to_string(), "development".to_string())
            ]
        );
        let run = RunRecipe {
            command: r#"node -e "console.log({a:1})" {ports. {ports.web"#.into(),
            cwd: None,
            env: BTreeMap::new(),
        };
        assert_eq!(
            run.render(&ports).unwrap().command,
            r#"node -e "console.log({a:1})" {ports. {ports.web"#
        );
        assert!(
            RunRecipe {
                command: "x {ports.nope}".into(),
                cwd: None,
                env: BTreeMap::new()
            }
            .render(&ports)
            .is_err()
        );
    }

    #[test]
    fn a_recipe_without_placeholders_gets_no_port() {
        let r = parse_recipe("[run.dev]\ncommand = \"pnpm dev\"\n").unwrap();
        assert!(r.ports.is_empty());
        assert!(r.runs["dev"].services().is_empty());
        assert_eq!(
            r.runs["dev"].render(&BTreeMap::new()).unwrap().command,
            "pnpm dev"
        );
    }

    #[test]
    fn rejects_mistakes() {
        let cases = [
            ("[run.dev]\ncommand = \"x {ports.web}\"\n", "no [ports.web]"),
            ("[ports.web]\n", "preferred port, a range"),
            ("[ports.web]\npreferred = 70000\n", "not a port"),
            ("[ports.web]\npreferred = 0\n", "not a port"),
            (
                "[ports.web]\npreferred = 80\nrange = \"3000-3999\"\n",
                "outside",
            ),
            ("[ports.web]\nrange = [10, 5]\n", "ends before"),
            ("[ports.web]\nrange = \"9-1\"\n", "invalid range"),
            ("[ports.\"a b\"]\npreferred = 80\n", "names use"),
            ("[run.dev]\ncommand = \"  \"\n", "empty"),
            ("[run.dev]\ncommand = \"a\\nb\"\n", "single line"),
            (
                "[run.dev]\ncommand = \"x\"\ncwd = \"..\\\\other\"\n",
                "inside the project",
            ),
            (
                "[run.dev]\ncommand = \"x\"\ncwd = \"C:\\\\Windows\"\n",
                "inside the project",
            ),
            (
                "[run.dev]\ncommand = \"x\"\ncwd = \"/etc\"\n",
                "inside the project",
            ),
            (
                "[run.dev]\ncommand = \"x\"\nenv = { \"1A\" = \"y\" }\n",
                "variable name",
            ),
            (
                "[run.dev]\ncommand = \"x\"\nshell = \"bash\"\n",
                "unknown field",
            ),
            ("[project]\nname = 1\n", "invalid type"),
            ("not toml", ""),
        ];
        for (text, needle) in cases {
            let err = parse_recipe(text).expect_err(text);
            assert!(err.contains(needle), "{text:?} -> {err}");
        }
    }

    #[test]
    fn cwd_is_normalized_inside_the_project() {
        let r = parse_recipe("[run.dev]\ncommand = \"x\"\ncwd = \"./apps//web/\"\n").unwrap();
        assert_eq!(r.runs["dev"].cwd.as_deref(), Some(r"apps\web"));
        let r = parse_recipe("[run.dev]\ncommand = \"x\"\ncwd = \".\"\n").unwrap();
        assert_eq!(r.runs["dev"].cwd, None);
    }

    #[test]
    fn trust_text_ignores_ports_but_not_the_command() {
        let r = parse_recipe(FULL).unwrap();
        let a = r.runs["dev"].trust_text("dev");
        let mut changed = r.runs["dev"].clone();
        changed.command.push_str(" --open");
        assert_ne!(a, changed.trust_text("dev"));
        assert_ne!(a, r.runs["dev"].trust_text("web"));
        let mut env = r.runs["dev"].clone();
        env.env.insert("EXTRA".into(), "1".into());
        assert_ne!(a, env.trust_text("dev"));
        assert_eq!(a, r.runs["dev"].clone().trust_text("dev"));
    }

    #[test]
    fn names() {
        assert!(valid_name("web"));
        assert!(valid_name("web-2_x"));
        assert!(!valid_name("-web"));
        assert!(!valid_name(""));
        assert!(!valid_name("a.b"));
        assert_eq!(
            placeholders("a {ports.web} b {ports.api}{ports.} {ports.x y}"),
            vec!["web".to_string(), "api".to_string()]
        );
    }
}
