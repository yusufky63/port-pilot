# PortPilot

PortPilot explains every local development port on Windows: which process listens on it, which project, Git worktree, WSL distro or Docker container it belongs to, who started it, and how to stop it without hitting the wrong process. It is a desktop app with a tray icon and a command-line tool, `portctl`, built on the same engine.

[Türkçe](README.tr.md) · [How it works](docs/architecture.md) · [CLI reference](docs/cli.md) · [Safety and privacy](docs/security.md) · [FAQ](docs/faq.md)

> Status: `0.1.0-alpha.1`, the first milestone ("Explain every local dev port"). Windows 11 is the primary target; Windows 10 works on a best-effort basis.

## What it answers

"Something is on port 3000" is the symptom. PortPilot answers the questions behind it:

- **Who owns it.** Port, bind address and exposure (local only, one network, all interfaces), the owning process with its PID *and* creation time, executable, command line, user and the chain of parents that started it.
- **Which project.** The project folder and its Git repository, branch, commit, uncommitted changes and whether it is a linked worktree. Every attribution says how it was made: *exact* (WSL working directory, Compose label) or *inferred* (a path in the command line, the executable's folder). Windows has no public API for another process's working directory, so PortPilot never pretends to know one.
- **Who started it.** The nearest launcher in the parent chain: Claude, Codex, Gemini CLI, VS Code, Cursor, a JetBrains IDE, a terminal, or the Windows service manager.
- **Windows, WSL or Docker.** WSL's `wslrelay.exe` and Docker Desktop's `com.docker.backend.exe` hold host ports for something else. PortPilot folds them into the Linux process (read with `ss` inside running distros) or the container (read from the Docker Engine API) they forward to, and flags forwarders with nothing behind them, the classic "port is already allocated" case.
- **Why a port is unusable.** A port inside one of Windows' reserved ranges (Hyper-V, WinNAT, WSL and Docker reserve blocks) cannot be bound even though nothing listens. `portctl who 3000` and the Doctor page say so and suggest free ports nearby.
- **Conflicts.** Two owners on one port at different addresses (for example `127.0.0.1:3000` and `[::1]:3000`), where which one answers depends on IPv4 or IPv6.

## Stopping safely

1. **Identity first.** A stop request names a listener the user saw. The engine opens the process and compares its creation time with the one shown; Windows reuses PIDs, so a mismatch stops everything. The handle stays open until the end, so the PID cannot be reused in between.
2. **Graceful first.** Windowed programs get `WM_CLOSE`. Console programs get CTRL+C, and CTRL+BREAK if CTRL+C was ignored, but only when the console is used by nothing but the target, its children, the package runner and the shell that started them. If an editor, an agent or any unrelated program shares the console, no signal is sent.
3. **Force is a second decision.** If the process is still running after the grace period, the app asks again and explains what terminating means; the CLI needs `--force` (and `--yes` without a terminal).
4. **Never** system processes, Windows services, WSL or Docker forwarders, or PortPilot itself. WSL processes get SIGTERM inside the distro after the same start-time check; containers are stopped through the Docker API.

## Running projects

A project can opt in with a `.portpilot.toml` recipe. PortPilot reads it only when you ask (the Run button, `portctl run`, `portctl lease`) and never runs anything from a repository on its own.

```toml
# .portpilot.toml at the repository root
[project]
name = "shop"

[ports.web]
preferred = 5173
range = "5173-5199"

[ports.api]
preferred = 8080

[run.dev]
command = "pnpm dev --port {ports.web} --strictPort"
cwd = "apps/web"
env = { API_URL = "http://localhost:{ports.api}" }
```

- **Trust prompt.** The first time, PortPilot shows the exact command, folder, environment and ports, and asks: *Trust and run*, *Run once* or *Cancel*. A trusted command runs without the prompt until the recipe changes it.
- **Managed runs.** The command runs with `cmd.exe` in its own process group inside a Windows Job Object, in the folder you saw. Its PID, creation time and folder are recorded, so its listeners are attributed *exactly*. Output is kept in memory for the Runs page. Stop sends CTRL+BREAK to the run's process group only; ending the whole job needs your consent. If PortPilot exits, the job ends the run with it.
- **Port leases.** `{ports.web}` gets a port leased to this repository, worktree and service, so two worktrees of one repository never get the same port. When the leased port is taken, PortPilot offers to allocate the next free port for the worktree. Commands without a placeholder get no port.
- **History.** A local timeline of runs, exits and port changes, kept for 14 days. No command lines unless you turn that on.

## Privacy

PortPilot sends nothing anywhere: no telemetry, no crash reports, no update checks, no account. Command lines are shown with tokens, passwords and keys masked. The local history keeps ports, process names and project folders for 14 days; it stores command lines only when you turn that on, and masks them. Environment variables of other processes are never read. Details in [docs/security.md](docs/security.md).

## The CLI

```powershell
portctl list                     # listeners with owner, project, runtime, exposure
portctl who 3000                 # explain one port, with free ports nearby
portctl tree 5173                # the full parent chain with command lines
portctl stop 3000                # graceful stop; exit code 7 if it keeps running
portctl stop 3000 --force --yes  # terminate after the grace period, no prompt
portctl find --range 3000-3999   # free ports, checked against reserved ranges
portctl doctor                   # Windows, Git, WSL, Docker, reserved ranges
portctl projects                 # listeners grouped by repository and worktree
portctl run dev                  # run [run.dev] from .portpilot.toml after the trust prompt
portctl lease acquire web        # print the port leased to this worktree's web service
portctl history --kind runs      # runs and exits of the last 14 days
portctl list --json              # versioned JSON (schemaVersion 1)
```

Exit codes are stable: 0 ok, 2 invalid arguments, 3 no listener, 4 ambiguous owner, 5 access denied or refused, 6 identity changed, 7 graceful timeout, 8 WSL or Docker unavailable, 9 port conflict, 10 internal error, 11 recipe missing or invalid. See [docs/cli.md](docs/cli.md).

## Install

PortPilot is not released yet. Pushing a `v*` tag runs the release workflow, which builds:

| File | For |
|---|---|
| `PortPilot_<version>_x64-setup.exe` | Installs for the current user without an admin prompt. English or Turkish installer. |
| `PortPilot_<version>_x64_portable.zip` | No installation. Settings stay in a `data` folder next to the exe. Includes `portctl.exe`. |
| `SHA256SUMS.txt` | Checksums for every file. |

Builds are not code-signed yet, so SmartScreen may warn. Both need the WebView2 runtime, which Windows 10 and 11 include.

### Build from source

Windows 10 or 11 with Rust (stable, MSVC), Node.js 22+, pnpm 9, Git and the Visual Studio C++ build tools.

```powershell
pnpm install
pnpm app:dev                 # desktop app with hot reload
cargo run -p portpilot-cli -- list
```

## Stack

- Rust workspace: `portpilot-core` (model, parsers, inference, recipe and lease rules; no OS calls), `portpilot-win` (IP Helper tables, ToolHelp, process handles, WMI, console control and Job Objects through `windows-rs`), `portpilot-engine` (discovery, Git, WSL, Docker, doctor, stop flow, managed runs, leases, SQLite history through `rusqlite`), `portpilot-cli` (`portctl`).
- Desktop: Tauri 2, React 19, Tailwind 4, zustand, i18next (English and Turkish).
- Tests: unit tests for every parser and rule, integration tests that start real servers and stop them through every path of the stop flow.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
