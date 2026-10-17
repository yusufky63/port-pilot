# portctl

`portctl` is the command-line face of Port Pilot. It uses the same engine as the desktop app and needs no running app.

```
portctl [--no-redact] <command>
```

`--no-redact` shows command lines without masking secrets. Every command that reads listeners also takes `--no-wsl` and `--no-docker` to skip those runtimes.

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
| 10 | Internal error |
