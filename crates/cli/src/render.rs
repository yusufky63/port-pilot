//! Plain-text output. Every line names its source and confidence the same way the desktop app
//! does, so a pasted terminal output explains as much as a screenshot.

use std::time::{SystemTime, UNIX_EPOCH};

use portpilot_core::time::unix_ms_to_filetime;
use portpilot_core::{
    AdapterState, AdapterStatus, AttributionSource, Confidence, Exposure, GracefulChannel,
    GracefulSkip, LauncherKind, Listener, Note, ProcessClass, Runtime, StopBlock, StopOutcome,
    StopReport,
};
use portpilot_engine::doctor::{Environment, PortDiagnosis, Verdict};
use portpilot_engine::projects::ProjectGroup;
use portpilot_win::time::local_datetime;

pub fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(c.chars().count());
            }
        }
    }
    let line = |cells: Vec<&str>| {
        let mut s = String::new();
        for (i, c) in cells.iter().enumerate() {
            if i + 1 == cells.len() {
                s.push_str(c);
            } else {
                s.push_str(&format!("{c:<width$}  ", width = widths[i]));
            }
        }
        s.trim_end().to_string()
    };
    let mut out = line(headers.to_vec());
    out.push('\n');
    for r in rows {
        out.push_str(&line(r.iter().map(String::as_str).collect()));
        out.push('\n');
    }
    out
}

fn now_filetime() -> u64 {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    unix_ms_to_filetime(ms)
}

pub fn age(start: u64) -> String {
    let secs = now_filetime().saturating_sub(start) / 10_000_000;
    match secs {
        0..=59 => format!("{secs} s ago"),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h {} min ago", secs / 3600, (secs % 3600) / 60),
        _ => format!("{} days ago", secs / 86_400),
    }
}

pub fn exposure_label(e: Exposure) -> &'static str {
    match e {
        Exposure::Loopback => "loopback only",
        Exposure::Lan => "one network address",
        Exposure::All => "all interfaces",
    }
}

pub fn runtime_label(l: &Listener) -> String {
    match l.runtime {
        Runtime::Windows => "Windows".into(),
        Runtime::Wsl => match &l.wsl {
            Some(w) => format!("WSL ({})", w.distro),
            None => "WSL".into(),
        },
        Runtime::Docker => "Docker".into(),
    }
}

pub fn launcher_label(kind: LauncherKind) -> &'static str {
    match kind {
        LauncherKind::Claude => "Claude",
        LauncherKind::Codex => "Codex",
        LauncherKind::Gemini => "Gemini CLI",
        LauncherKind::VsCode => "VS Code",
        LauncherKind::Cursor => "Cursor",
        LauncherKind::Windsurf => "Windsurf",
        LauncherKind::JetBrains => "a JetBrains IDE",
        LauncherKind::Zed => "Zed",
        LauncherKind::Terminal => "a terminal",
        LauncherKind::Explorer => "Explorer",
        LauncherKind::Service => "the service manager",
    }
}

pub fn block_reason(b: StopBlock) -> &'static str {
    match b {
        StopBlock::System => "it is a Windows system process",
        StopBlock::Service => "it is a Windows service; use the Services app",
        StopBlock::Relay => {
            "it forwards a port for WSL or Docker; stop the Linux process or the container instead"
        }
        StopBlock::AccessDenied => "it belongs to another user or runs elevated",
        StopBlock::IdentityUnknown => {
            "its start time cannot be read, so PortPilot cannot prove it is the same process"
        }
        StopBlock::SelfProcess => "it is PortPilot or the program running it",
    }
}

pub fn note_text(n: Note) -> &'static str {
    match n {
        Note::RelayWithoutListener => {
            "a WSL forwarder holds this port but no Linux process listens on it"
        }
        Note::RelayWithoutContainer => {
            "Docker holds this port but no running container publishes it (Docker Desktop's own services, or a stale allocation)"
        }
        Note::NotForwarded => {
            "no Windows-side forwarder seen: unreachable from Windows in NAT mode, expected in mirrored mode"
        }
        Note::ParentExited => "the process that started it has exited",
        Note::SharedPort => "another owner listens on the same port at a different address",
    }
}

fn owner_label(l: &Listener) -> String {
    if let Some(c) = &l.container {
        return format!("{} ({})", c.name, c.image);
    }
    match &l.process {
        Some(p) => p.name.clone(),
        None => "unknown".into(),
    }
}

fn project_label(l: &Listener) -> String {
    let Some(p) = &l.project else {
        return "-".into();
    };
    match p.git.as_ref().and_then(|g| g.branch.clone()) {
        Some(branch) => format!("{} ({branch})", p.name),
        None => p.name.clone(),
    }
}

fn addresses(l: &Listener) -> String {
    l.endpoints
        .iter()
        .map(|e| e.address.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn list(listeners: &[&Listener]) -> String {
    let rows: Vec<Vec<String>> = listeners
        .iter()
        .map(|l| {
            let mut flags = Vec::new();
            if l.conflict {
                flags.push("conflict");
            }
            if l.exposure == Exposure::All {
                flags.push("all interfaces");
            } else if l.exposure == Exposure::Lan {
                flags.push("network");
            }
            vec![
                l.port.to_string(),
                l.protocol.as_str().to_string(),
                addresses(l),
                runtime_label(l),
                l.process
                    .as_ref()
                    .map(|p| p.pid.to_string())
                    .unwrap_or_else(|| "-".into()),
                owner_label(l),
                project_label(l),
                flags.join(", "),
            ]
        })
        .collect();
    table(
        &[
            "PORT", "PROTO", "ADDRESS", "RUNTIME", "PID", "OWNER", "PROJECT", "NOTES",
        ],
        &rows,
    )
}

pub fn adapter_line(name: &str, a: &AdapterState) -> String {
    let status = match a.status {
        AdapterStatus::Available if a.items.is_empty() => "available, nothing running".to_string(),
        AdapterStatus::Available => format!("{} ({})", a.items.len(), a.items.join(", ")),
        AdapterStatus::Disabled => "skipped".into(),
        AdapterStatus::NotInstalled => "not installed".into(),
        AdapterStatus::NotRunning => "not running".into(),
        AdapterStatus::Error => "error".into(),
    };
    match &a.detail {
        Some(d) => format!("{name}: {status} ({d})"),
        None => format!("{name}: {status}"),
    }
}

fn confidence_label(c: Confidence, s: AttributionSource) -> &'static str {
    match (c, s) {
        (Confidence::Exact, AttributionSource::ProcCwd) => "exact, working directory",
        (Confidence::Exact, AttributionSource::ComposeLabel) => "exact, Compose label",
        (Confidence::Exact, AttributionSource::Launched) => "exact, started by PortPilot",
        (_, AttributionSource::CommandLine) => "inferred from the command line",
        (_, AttributionSource::ExecutablePath) => "inferred from the executable's folder",
        _ => "inferred",
    }
}

/// The ownership chain of one listener, as an indented tree.
pub fn listener_detail(l: &Listener) -> String {
    let mut lines: Vec<(String, String)> = Vec::new();
    let mut push = |k: &str, v: String| lines.push((k.to_string(), v));
    if let Some(p) = &l.process {
        if let Some(start) = p.start.filter(|_| l.runtime == Runtime::Windows) {
            let when = local_datetime(start).unwrap_or_default();
            push("Started", format!("{when} ({})", age(start)));
        }
        if let Some(cmd) = &p.command_line {
            push("Command", cmd.clone());
        } else if p.access_denied {
            push("Command", "not readable (another user or elevated)".into());
        }
        if let Some(path) = &p.path {
            push("Executable", path.clone());
        }
        if let Some(user) = &p.user {
            push("User", user.clone());
        }
    }
    if let Some(c) = &l.container {
        push(
            "Container",
            format!("{} ({})", c.name, c.id.chars().take(12).collect::<String>()),
        );
        push("Image", c.image.clone());
        if let (Some(p), Some(s)) = (&c.compose_project, &c.compose_service) {
            push("Compose", format!("{p} / {s}"));
        }
        push("Status", c.status.clone());
    }
    if let Some(parent) = l.ancestors.first() {
        push("Parent", format!("{} (PID {})", parent.name, parent.pid));
    }
    if let Some(launcher) = &l.launcher {
        push(
            "Started by",
            format!(
                "{} ({} PID {})",
                launcher_label(launcher.kind),
                launcher.name,
                launcher.pid
            ),
        );
    }
    if let Some(m) = &l.managed {
        push("Run", format!("{} (managed by PortPilot)", m.name));
    }
    push("Runtime", runtime_label(l));
    if let Some(relay) = &l.relay {
        push(
            "Forwarded by",
            format!("{} (PID {})", relay.name, relay.pid),
        );
    }
    if let Some(w) = &l.wsl {
        if let Some(cwd) = &w.cwd {
            push("Linux cwd", cwd.clone());
        }
        let binds: Vec<String> = w.binds.iter().map(|e| e.address.clone()).collect();
        push("Linux bind", binds.join(", "));
    }
    match &l.project {
        Some(p) => {
            push(
                "Project",
                format!("{} [{}]", p.root, confidence_label(p.confidence, p.source)),
            );
            if let Some(g) = &p.git {
                let mut s = g.repo_name.clone();
                if let Some(b) = &g.branch {
                    s.push_str(&format!(" / {b}"));
                } else {
                    s.push_str(" / detached HEAD");
                }
                if let Some(h) = &g.head {
                    s.push_str(&format!(" @ {h}"));
                }
                if g.dirty == Some(true) {
                    s.push_str(", uncommitted changes");
                }
                push("Git", s);
                if g.linked_worktree {
                    push("Worktree", g.top_level.clone());
                }
            }
        }
        None if l.runtime == Runtime::Windows
            && l.process
                .as_ref()
                .is_some_and(|p| p.class == ProcessClass::User) =>
        {
            push(
                "Project",
                "unknown (no project path in the command line)".into(),
            );
        }
        None => {}
    }
    push("Exposure", exposure_label(l.exposure).into());
    if let Some(url) = &l.url {
        push("URL", url.clone());
    }
    match l.stop.blocked {
        Some(b) => push("Stop", format!("not offered: {}", block_reason(b))),
        None => push(
            "Stop",
            format!(
                "portctl stop {}{}",
                l.port,
                l.process
                    .as_ref()
                    .map(|p| format!(" --pid {}", p.pid))
                    .unwrap_or_default()
            ),
        ),
    }
    for n in &l.notes {
        push("Note", note_text(*n).into());
    }

    let width = lines.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let mut out = String::new();
    let addrs: Vec<String> = l
        .endpoints
        .iter()
        .map(|e| format!("{}:{}", e.address, e.port))
        .collect();
    out.push_str(&format!(
        "{} {}  [{}]\n",
        l.protocol.as_str().to_uppercase(),
        addrs.join(", "),
        exposure_label(l.exposure)
    ));
    let head = match (&l.process, &l.container) {
        (Some(p), _) => {
            let verified = if p.start.is_some() {
                "verified"
            } else {
                "unverified"
            };
            format!("{} PID {} [{verified}]", p.name, p.pid)
        }
        (None, Some(c)) => format!("container {}", c.name),
        _ => "owner unknown".into(),
    };
    out.push_str(&format!("└─ {head}\n"));
    let n = lines.len();
    for (i, (k, v)) in lines.into_iter().enumerate() {
        let branch = if i + 1 == n { "└─" } else { "├─" };
        out.push_str(&format!("   {branch} {k:<width$}  {v}\n"));
    }
    out
}

pub fn who(d: &PortDiagnosis) -> String {
    let mut out = format!("Port {}: ", d.port);
    out.push_str(match d.verdict {
        Verdict::Free => "free",
        Verdict::InUse => "in use",
        Verdict::Shared => "in use by more than one owner",
        Verdict::Reserved => "reserved by Windows",
        Verdict::Blocked => "not usable, but no visible listener holds it",
    });
    out.push_str("\n\n");
    for l in &d.listeners {
        out.push_str(&listener_detail(l));
        out.push('\n');
    }
    if let Some(r) = d.excluded_range {
        out.push_str(&format!(
            "Windows reserves ports {}-{} (Hyper-V, WSL or Docker took this block). See `netsh interface ipv4 show excludedportrange protocol=tcp`.\n",
            r.start, r.end
        ));
    }
    if d.verdict == Verdict::Blocked {
        out.push_str("Binding to it fails. A process of another user, an elevated process or a stale forwarder may hold it.\n");
    }
    if !d.suggestions.is_empty() {
        let s: Vec<String> = d.suggestions.iter().map(u16::to_string).collect();
        out.push_str(&format!("Free nearby: {}\n", s.join(", ")));
    }
    out
}

/// Full chain: owner, then every ancestor with its command line.
pub fn tree(l: &Listener) -> String {
    let mut out = String::new();
    let addrs: Vec<String> = l
        .endpoints
        .iter()
        .map(|e| format!("{}:{}", e.address, e.port))
        .collect();
    out.push_str(&format!(
        "{} {}\n",
        l.protocol.as_str().to_uppercase(),
        addrs.join(", ")
    ));
    let mut chain: Vec<(u32, String, Option<String>)> = Vec::new();
    if let Some(p) = &l.process {
        chain.push((p.pid, p.name.clone(), p.command_line.clone()));
    }
    for a in &l.ancestors {
        chain.push((a.pid, a.name.clone(), a.command_line.clone()));
    }
    if let Some(c) = &l.container {
        out.push_str(&format!("└─ container {} ({})\n", c.name, c.image));
    }
    for (depth, (pid, name, cmd)) in chain.iter().enumerate() {
        let indent = "   ".repeat(depth);
        out.push_str(&format!("{indent}└─ {name} PID {pid}\n"));
        if let Some(cmd) = cmd {
            out.push_str(&format!("{indent}   {cmd}\n"));
        }
    }
    if let Some(r) = &l.relay {
        out.push_str(&format!(
            "forwarded on Windows by {} PID {}\n",
            r.name, r.pid
        ));
    }
    if l.notes.contains(&Note::ParentExited) {
        out.push_str("The process that started it has exited.\n");
    }
    out
}

pub fn stop_report(r: &StopReport) -> String {
    let channel = r.channel.map(|c| match c {
        GracefulChannel::CloseWindow => "closed its window",
        GracefulChannel::ConsoleCtrlC => "sent CTRL+C",
        GracefulChannel::ConsoleCtrlBreak => "sent CTRL+C, then CTRL+BREAK",
        GracefulChannel::GroupCtrlBreak => "sent CTRL+BREAK to the run's process group",
        GracefulChannel::Sigterm => "sent SIGTERM",
        GracefulChannel::DockerStop => "asked Docker to stop the container",
    });
    let mut s = match r.outcome {
        StopOutcome::Stopped => format!("Stopped ({}).", channel.unwrap_or("graceful")),
        StopOutcome::Forced => "Terminated after the grace period.".to_string(),
        StopOutcome::AlreadyExited => "It had already exited.".to_string(),
        StopOutcome::IdentityChanged => {
            "Not stopped: the PID now belongs to a different process.".to_string()
        }
        StopOutcome::GracefulTimeout => format!(
            "Still running after the grace period ({}). Run again with --force to terminate it.",
            channel.unwrap_or("graceful request")
        ),
        StopOutcome::GracefulUnavailable => {
            "No safe graceful channel. Run again with --force to terminate it.".to_string()
        }
        StopOutcome::AccessDenied => "Not stopped: access denied.".to_string(),
        StopOutcome::Refused => {
            "Not stopped: PortPilot does not stop this kind of process.".to_string()
        }
        StopOutcome::RuntimeUnavailable => {
            "Not stopped: WSL or Docker could not be reached.".to_string()
        }
        StopOutcome::Failed => "Stopping failed.".to_string(),
    };
    if let Some(why) = r.graceful_skipped {
        let why = match why {
            GracefulSkip::NoConsole => "the process has no window and no console",
            GracefulSkip::SharedConsole => {
                "its console is shared with other programs, which CTRL+C would also reach"
            }
            GracefulSkip::SignalFailed => "the console signal could not be sent",
        };
        s.push_str(&format!("\nGraceful request skipped: {why}."));
    }
    if r.children_terminated > 0 {
        s.push_str(&format!(
            "\nAlso terminated {} child process(es).",
            r.children_terminated
        ));
    }
    if let Some(d) = &r.detail {
        s.push_str(&format!("\n{d}"));
    }
    s
}

pub fn environment(e: &Environment) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Windows: {}\n",
        e.os.clone().unwrap_or_else(|| "unknown".into())
    ));
    if e.windows10 {
        out.push_str("  Windows 10 left general support on 2025-10-14; PortPilot works there on a best-effort basis.\n");
    }
    out.push_str(&format!(
        "User: {}{}\n",
        e.user.clone().unwrap_or_default(),
        if e.elevated { " (elevated)" } else { "" }
    ));
    out.push_str(&format!(
        "Git: {}\n",
        e.git.clone().unwrap_or_else(|| "not found".into())
    ));
    out.push_str(&adapter_line("WSL", &e.wsl));
    out.push('\n');
    if let Some(v) = &e.wsl_version {
        out.push_str(&format!("  {v}\n"));
    }
    out.push_str(&adapter_line("Docker", &e.docker));
    out.push('\n');
    if let Some(v) = &e.docker_version {
        out.push_str(&format!("  Engine {v}\n"));
    }
    if e.excluded_tcp.is_empty() {
        out.push_str("Reserved TCP port ranges: none\n");
    } else {
        let ranges: Vec<String> = e
            .excluded_tcp
            .iter()
            .map(|r| {
                if r.start == r.end {
                    r.start.to_string()
                } else {
                    format!("{}-{}", r.start, r.end)
                }
            })
            .collect();
        out.push_str(&format!(
            "Reserved TCP port ranges: {}\n",
            ranges.join(", ")
        ));
    }
    out
}

pub fn projects(groups: &[ProjectGroup]) -> String {
    let mut out = String::new();
    for g in groups {
        let rt = match g.runtime {
            Runtime::Windows => String::new(),
            Runtime::Wsl => " [WSL]".into(),
            Runtime::Docker => " [Docker]".into(),
        };
        out.push_str(&format!("{}{rt}  {}\n", g.name, g.root));
        for w in &g.worktrees {
            let branch = w.branch.clone().unwrap_or_else(|| "detached".into());
            let mut tags = Vec::new();
            if w.main {
                tags.push("main checkout".to_string());
            }
            if w.dirty == Some(true) {
                tags.push("uncommitted changes".into());
            }
            if w.locked {
                tags.push("locked".into());
            }
            let running = if w.listener_ids.is_empty() {
                "no listeners".to_string()
            } else {
                format!("{} listener(s)", w.listener_ids.len())
            };
            out.push_str(&format!(
                "  {branch:<28} {}  {running}{}\n",
                w.path,
                if tags.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", tags.join(", "))
                }
            ));
        }
        if !g.listener_ids.is_empty() {
            out.push_str(&format!("  {} listener(s)\n", g.listener_ids.len()));
        }
    }
    if groups.is_empty() {
        out.push_str("No listener could be tied to a project folder.\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_table() {
        let t = table(&["A", "BB"], &[vec!["xxx".into(), "y".into()]]);
        assert_eq!(t, "A    BB\nxxx  y\n");
    }

    #[test]
    fn ages() {
        assert!(age(now_filetime() - 90 * 10_000_000).starts_with("1 min"));
        assert!(age(now_filetime() - 2 * 3600 * 10_000_000).starts_with("2 h"));
    }
}
