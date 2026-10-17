# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
- Safe stop: identity check on an open handle, graceful request (`WM_CLOSE`, or CTRL+C then CTRL+BREAK on a console only the target's tree uses), grace period, terminate only on explicit consent, optional child processes. SIGTERM/SIGKILL inside WSL after a start-time check; container stop through the Docker API. System processes, services, forwarders and Port Pilot's own lineage are refused.
- Doctor: why a port is free, in use, shared, reserved by Windows or blocked; free ports nearby; Windows version, Git, WSL and Docker state; reserved TCP ranges.
- Desktop app: Ports dashboard with search, filters and a detail panel; Projects view with every worktree of each repository; Doctor; Settings. Light and dark theme, English and Turkish, tray icon with the dev listeners, pause and quit.
- `portctl` CLI: `list`, `who`, `tree`, `stop`, `find`, `doctor`, `project inspect`, `projects`; versioned JSON (`schemaVersion` 1) and stable exit codes.
- Secret masking in command lines (flags, environment-style assignments, URL passwords, known token formats).
- Portable mode (a `portable` file next to the exe), per-user NSIS installer.
