# Port Pilot

Port Pilot explains every local development port on Windows: which process listens on it, which project, Git worktree, WSL distro or Docker container it belongs to, who started it, and how to stop it without hitting the wrong process. It is a desktop app with a tray icon and a command-line tool, `portctl`, built on the same engine.

[Türkçe](README.tr.md) · [How it works](docs/architecture.md) · [CLI reference](docs/cli.md) · [Safety and privacy](docs/security.md) · [FAQ](docs/faq.md)

> Status: `0.1.0-alpha.1`, the first milestone ("Explain every local dev port"). Windows 11 is the primary target; Windows 10 works on a best-effort basis.

## What it answers

"Something is on port 3000" is the symptom. Port Pilot answers the questions behind it:

- **Who owns it.** Port, bind address and exposure (local only, one network, all interfaces), the owning process with its PID *and* creation time, executable, command line, user and the chain of parents that started it.
- **Which project.** The project folder and its Git repository, branch, commit, uncommitted changes and whether it is a linked worktree. Every attribution says how it was made: *exact* (WSL working directory, Compose label) or *inferred* (a path in the command line, the executable's folder). Windows has no public API for another process's working directory, so Port Pilot never pretends to know one.
- **Who started it.** The nearest launcher in the parent chain: Claude, Codex, Gemini CLI, VS Code, Cursor, a JetBrains IDE, a terminal, or the Windows service manager.
- **Windows, WSL or Docker.** WSL's `wslrelay.exe` and Docker Desktop's `com.docker.backend.exe` hold host ports for something else. Port Pilot folds them into the Linux process (read with `ss` inside running distros) or the container (read from the Docker Engine API) they forward to, and flags forwarders with nothing behind them, the classic "port is already allocated" case.
- **Why a port is unusable.** A port inside one of Windows' reserved ranges (Hyper-V, WinNAT, WSL and Docker reserve blocks) cannot be bound even though nothing listens. `portctl who 3000` and the Doctor page say so and suggest free ports nearby.
- **Conflicts.** Two owners on one port at different addresses (for example `127.0.0.1:3000` and `[::1]:3000`), where which one answers depends on IPv4 or IPv6.

## Stopping safely

1. **Identity first.** A stop request names a listener the user saw. The engine opens the process and compares its creation time with the one shown; Windows reuses PIDs, so a mismatch stops everything. The handle stays open until the end, so the PID cannot be reused in between.
2. **Graceful first.** Windowed programs get `WM_CLOSE`. Console programs get CTRL+C, and CTRL+BREAK if CTRL+C was ignored, but only when the console is used by nothing but the target, its children, the package runner and the shell that started them. If an editor, an agent or any unrelated program shares the console, no signal is sent.
3. **Force is a second decision.** If the process is still running after the grace period, the app asks again and explains what terminating means; the CLI needs `--force` (and `--yes` without a terminal).
4. **Never** system processes, Windows services, WSL or Docker forwarders, or Port Pilot itself. WSL processes get SIGTERM inside the distro after the same start-time check; containers are stopped through the Docker API.

## Privacy

Port Pilot sends nothing anywhere: no telemetry, no crash reports, no update checks, no account. Command lines are shown with tokens, passwords and keys masked, and are never written to disk. Environment variables are never read. Details in [docs/security.md](docs/security.md).

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
portctl list --json              # versioned JSON (schemaVersion 1)
```

Exit codes are stable: 0 ok, 2 invalid arguments, 3 no listener, 4 ambiguous owner, 5 access denied or refused, 6 identity changed, 7 graceful timeout, 8 WSL or Docker unavailable, 10 internal error. See [docs/cli.md](docs/cli.md).

## Install

Port Pilot is not released yet. Pushing a `v*` tag runs the release workflow, which builds:

| File | For |
|---|---|
| `Port-Pilot_<version>_x64-setup.exe` | Installs for the current user without an admin prompt. English or Turkish installer. |
| `Port-Pilot_<version>_x64_portable.zip` | No installation. Settings stay in a `data` folder next to the exe. Includes `portctl.exe`. |
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

- Rust workspace: `portpilot-core` (model, parsers, inference; no OS calls), `portpilot-win` (IP Helper tables, ToolHelp, process handles, WMI, console control through `windows-rs`), `portpilot-engine` (discovery, Git, WSL, Docker, doctor, stop flow), `portpilot-cli` (`portctl`).
- Desktop: Tauri 2, React 19, Tailwind 4, zustand, i18next (English and Turkish).
- Tests: unit tests for every parser and rule, integration tests that start real servers and stop them through every path of the stop flow.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
