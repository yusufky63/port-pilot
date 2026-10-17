# portctl

`portctl` is the command-line face of PortPilot. It uses the same engine as the desktop app and needs no running app.

```
portctl [--no-redact] <command>
```

`--no-redact` shows command lines without masking secrets. Every command that reads listeners also takes `--no-wsl` and `--no-docker` to skip those runtimes.

`portctl` shares its data folder with the desktop app (`%APPDATA%\PortPilot`, the portable `data` folder, or `PORTPILOT_DATA_DIR`): leases, trusted run commands and history are the same in both. Listeners of runs started by the app or by `portctl run` are attributed to their folder exactly (`exact, started by PortPilot`).

## Commands

### list

```powershell
portctl list [--json] [--all] [--udp]
```

TCP listeners with port, addresses, runtime, PID, owner, project and branch. `--all` adds Windows services and system processes; `--udp` adds UDP sockets. The footer shows how many listeners were hidden and the state of WSL and Docker.

### who

```powershell
portctl who <port> [--json]
```

Explains one TCP port: verdict (`free`, `inUse`, `shared`, `reserved`, `blocked`), every owner with its ownership chain, the Windows reserved range that contains it, and up to three free ports nearby. Exit code 3 when nothing listens.

```
Port 5173: in use

TCP 127.0.0.1:5173  [loopback only]
└─ node.exe PID 18420 [verified]
   ├─ Started     2026-10-17 14:22:03 (36 min ago)
   ├─ Command     node C:\src\acme-web\node_modules\vite\bin\vite.js --host
   ├─ Parent      node.exe (PID 17304)
   ├─ Started by  VS Code (Code.exe PID 1000)
   ├─ Runtime     Windows
   ├─ Project     C:\src\acme-web [inferred from the command line]
   ├─ Git         acme-web / feature/auth @ 5f32a0d
   ├─ Worktree    D:\wt\auth
   ├─ Exposure    loopback only
   ├─ URL         http://localhost:5173
   └─ Stop        portctl stop 5173 --pid 18420
```

### tree

```powershell
portctl tree <port> [--json]
```

The owner and every ancestor with its command line, until the chain reaches a process that exited.

### stop

```powershell
portctl stop <port> [--pid <pid>] [--force] [--tree] [--yes] [--timeout <seconds>] [--json]
```

1. Finds the TCP owner of the port. With several owners, `--pid` picks one (exit code 4 otherwise).
2. Refuses system processes, services, WSL and Docker forwarders and its own lineage (exit code 5).
3. Sends a graceful request: `WM_CLOSE`, or CTRL+C then CTRL+BREAK when the console is the target's alone. WSL processes get SIGTERM; containers are stopped through Docker.
4. Waits `--timeout` seconds (default 5).
5. With `--force`, terminates the process if it is still running; `--tree` also terminates its children. `--force` asks for confirmation in a terminal and requires `--yes` otherwise.

Without `--force`, a process that keeps running gives exit code 7 and the reason the graceful request was skipped, if it was.

### find

```powershell
portctl find [--range 3000-3999] [--count 1] [--json]
```

Free TCP ports: not used by any listener (Windows, WSL or Docker), outside Windows' reserved ranges, and bindable on `127.0.0.1` and `[::1]`.

### doctor

```powershell
portctl doctor [windows|wsl|docker|port <n>] [--json]
```

Windows version, user and elevation, Git version, WSL and Docker state, and the reserved TCP port ranges. `doctor port <n>` is the same as `who <n>`.

### project inspect / projects

```powershell
portctl project inspect [<path>] [--json]
portctl projects [--json]
```

`project inspect` shows the project around a folder (the current one by default): manifests, Git state, worktrees and the listeners attributed to it. `projects` lists every repository with listeners, with all of its worktrees.

### run

```powershell
portctl run [<name>] [--path <dir>] [--yes] [--next-free] [--timeout <seconds>]
```

Runs `[run.<name>]` from `.portpilot.toml` in the folder (the current one by default) or its parents up to the repository root. The name can be left out when the recipe has one run.

1. Each `{ports.<service>}` gets the port leased to this worktree, or the first free port of the service's range. If a leased port is in use, a terminal asks whether to allocate the next free port for this worktree; `--next-free` does that without asking, otherwise the exit code is 9.
2. The exact folder, command, environment and ports are printed.
3. A command PortPilot has not run before asks `[t] Trust and run  [o] Run once  [c] Cancel`. A trusted command (same command, `cwd` and `env` as when it was trusted) runs without asking. Without a terminal, `--yes` runs it once; otherwise the exit code is 2.
4. The command runs with `cmd.exe` in this console, in its own process group, inside a Job Object: closing the terminal ends it.
5. CTRL+C sends CTRL+BREAK to the run's process group only. If it is still running after `--timeout` seconds (default 5), a second CTRL+C ends the job with every process the run started.

The exit code is the command's own when it ends by itself, and 0 when you stopped it.

```toml
[project]
name = "shop"

[ports.web]
preferred = 5173
range = "5173-5199"

[run.dev]
command = "pnpm dev --port {ports.web} --strictPort"
cwd = "apps/web"
env = { API_URL = "http://localhost:{ports.api}" }
```

### lease

```powershell
portctl lease acquire <service> [--path <dir>] [--preferred <port>] [--range <a-b>] [--next] [--json]
portctl lease release [<service>] [--path <dir>] [--json]
portctl lease list [--all] [--path <dir>] [--json]
```

A lease reserves a port for one service of one worktree of one repository. `acquire` prints the port (`$port = portctl lease acquire web`): the worktree's existing lease, or the first free port of `[ports.<service>]` in the recipe; `--preferred` and `--range` give or override the spec. `--next` moves the lease to the next free port. When the kept port is in use by another program, the port is still printed and the exit code is 9. `release` without a service releases every lease of the worktree (exit code 3 when there was none). `list` shows the leases of the current repository, or of all with `--all`, with who uses each port now.

### history

```powershell
portctl history [--port <n>] [--kind runs|ports] [--days <n>] [--limit 50] [--json]
```

The local timeline, newest first: runs that started and exited (with their exit code or "stopped by PortPilot"), and TCP ports that opened or closed while the desktop app ran. Entries are kept for 14 days. Command lines appear only when history of command lines is turned on in the app.

## JSON

Every `--json` output carries `"schemaVersion": 1`. Field names are camelCase and follow `crates/core/src/model.rs`. The version changes only when a field is removed or changes meaning; new optional fields keep it.

`start` is the process creation time as a Windows FILETIME (100 ns since 1601) for Windows processes and the start time in clock ticks for WSL processes. It is an identity token, not a timestamp to display; `startedAt` is the RFC 3339 form.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 2 | Invalid arguments, or `--force` without `--yes` and no terminal |
| 3 | No matching listener |
| 4 | Ambiguous owner; pick one with `--pid` |
| 5 | Access denied, or the process may not be stopped |
| 6 | Identity changed: the PID belongs to another process now |
| 7 | Graceful timeout: still running, or no safe graceful channel |
| 8 | WSL or Docker unavailable |
| 9 | Port conflict: a leased port is in use, or no port of the range is free |
| 10 | Internal error |
| 11 | `.portpilot.toml` missing or invalid, or no such run |
