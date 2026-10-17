# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Managed runs: start a project's dev command from a `.portpilot.toml` recipe with `cmd.exe`, in its own process group, inside a Job Object that ends the run if its owner exits. The PID, creation time and exact folder are recorded, so the run's listeners are attributed exactly (`source: launched`). The app keeps stdout and stderr in memory for the new Runs page.
- Trust prompt before a recipe command runs: the exact command, folder, environment and ports, with *Trust and run*, *Run once* and *Cancel*. Trusted commands are remembered by SHA-256 and listed in Settings, where they can be revoked. Nothing from a repository runs on its own.
- Stopping a run sends CTRL+BREAK to its process group only; ending its job needs a second, explicit decision. Quitting the app with active runs asks first and stops them.
- Port leases per repository, worktree and service. `{ports.<service>}` placeholders in a recipe get a leased port; a conflict offers the next free port for the worktree. Commands without a placeholder get no port.
- History: a local SQLite timeline (`portpilot.db`) of runs, exits and TCP port changes, kept for 14 days, with a History page, a switch to turn it off and a button to clear it. Command lines only when turned on, masked.
- `portctl run`, `portctl lease acquire|release|list` and `portctl history`; exit codes 9 (port conflict) and 11 (recipe missing or invalid).
- `portctl` attributes listeners of runs started by the app or by another `portctl run` to their folder exactly.
- `PORTPILOT_DATA_DIR` overrides the data folder (used by the tests).

### Changed

- The product is named PortPilot everywhere (window, tray, installer `PortPilot_<version>_x64-setup.exe`, portable zip `PortPilot_<version>_x64_portable.zip`).
- The data folder now also holds `portpilot.db`; Settings calls it "Data folder".

### Fixed

- Windows Terminal and VS Code no longer inherit PortPilot's output handles, so their logs stay out of PortPilot's console. Without Windows Terminal, PowerShell and WSL terminals start through the shell in a console window of their own, with the folder as the working directory.

## [0.1.0-alpha.1] - 2026-10-17

First milestone: "Explain every local dev port".

### Added

- Discovery of listening TCP sockets and bound UDP sockets, IPv4 and IPv6, from the Windows IP Helper tables, grouped per owner with every bound address and its exposure (local, one network, all interfaces).
- Process identity as PID plus creation time; executable, user and command line (through WMI), and the parent chain up to the first parent that exited or whose PID was reused.
- Launcher detection: Claude, Codex, Gemini CLI, VS Code, Cursor, Windsurf, Zed, JetBrains IDEs, terminals, Explorer and the service manager.
- Project attribution from command-line paths and executable folders (inferred), WSL working directories and Compose labels (exact), with Git repository, branch, commit, uncommitted changes and linked-worktree detection.
- WSL: listeners in running distros through `ss`, with Linux PID, start time, command line, working directory and Git state; `wslrelay.exe` rows folded into them; relays without a Linux listener and listeners without a relay flagged.
- Docker Desktop: published ports and Compose project and service from the Docker Engine API; Docker's forwarder rows folded into the container; forwarders with no container flagged.
- Conflicts: TCP ports with more than one owner.
- Safe stop: identity check on an open handle, graceful request (`WM_CLOSE`, or CTRL+C then CTRL+BREAK on a console only the target's tree uses), grace period, terminate only on explicit consent, optional child processes. SIGTERM/SIGKILL inside WSL after a start-time check; container stop through the Docker API. System processes, services, forwarders and PortPilot's own lineage are refused.
- Doctor: why a port is free, in use, shared, reserved by Windows or blocked; free ports nearby; Windows version, Git, WSL and Docker state; reserved TCP ranges.
- Desktop app: Ports dashboard with search, filters and a detail panel; Projects view with every worktree of each repository; Doctor; Settings. Light and dark theme, English and Turkish, tray icon with the dev listeners, pause and quit.
- `portctl` CLI: `list`, `who`, `tree`, `stop`, `find`, `doctor`, `project inspect`, `projects`; versioned JSON (`schemaVersion` 1) and stable exit codes.
- Secret masking in command lines (flags, environment-style assignments, URL passwords, known token formats).
- Portable mode (a `portable` file next to the exe), per-user NSIS installer.
