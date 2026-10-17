# Roadmap

What is planned after 0.1.0-alpha.1, roughly in order. The reasons are in [docs/research.md](docs/research.md). Suggestions are welcome as issues.

## Shipped in 0.1.0-alpha.1

- Windows TCP/UDP discovery with PID + creation-time identity, parent chain and launcher.
- Project, Git and worktree attribution with exact/inferred confidence.
- WSL and Docker Desktop attribution, forwarder folding and stale-forwarder flags.
- Safe stop: identity check, graceful first, terminate on consent.
- Doctor: port diagnosis, reserved ranges, free ports.
- Desktop app with tray, `portctl` CLI with JSON and exit codes, portable mode and NSIS installer.

## Next (0.1)

- **Managed runs**: start a project's dev command from Port Pilot inside a Job Object with a new process group, with the working directory recorded exactly, after a "trust and run" prompt showing the command.
- **Port leases** per repository, worktree and service, with a `.portpilot.toml` recipe that opts into `--port {ports.web}`.
- **History**: a local SQLite timeline of runs, exits and port changes (no command lines by default, 14-day retention).
- Code signing, then winget and Scoop packages.

## Later (0.2)

- WSL mirrored-mode detection and a connectivity probe per port.
- Docker container restart and log access; Compose service view.
- CLI to desktop over a current-user named pipe, so `portctl` can use the app's state.
- MSI for managed deployment.

## 1.0

- A read-only MCP server (`portctl serve-mcp --read-only`) with list, who, find-free and diagnose tools; destructive tools only behind an explicit user policy.
- Agent session attribution beyond the parent chain.
- Short-lived elevated helper for one typed operation at a time.
- Accessibility pass, more languages, SBOM and attested releases.

## Not planned

- Firewall management, packet capture, tunnels or sharing ports to the internet.
- A Docker Desktop replacement or a general process manager.
- Telemetry, accounts or a cloud backend.
