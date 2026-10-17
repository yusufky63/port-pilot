# Architecture

Port Pilot is a Windows desktop app and CLI that explains local development ports and stops their owners safely. A Rust engine does every OS call; a React UI and the `portctl` CLI render the same snapshot. The research behind the decisions is summarized in [research.md](research.md).

## Goals

1. Answer "who owns this port, and why is it running?" in one place: process, parents, project, Git worktree, WSL distro or container, launcher.
2. Say how sure each answer is. A wrong owner is worse than no owner.
3. Never act on the wrong process: identity is PID plus creation time, checked on an open handle.
4. Stay local: no account, no backend, no telemetry.

## Non-goals

Firewall management, packet capture, tunnels or sharing ports to the internet, a Docker Desktop replacement, a general process manager. Hosts-file editing and local HTTPS domains are out of scope for 0.1.

## Repository layout

```
crates/core/      portpilot-core: the snapshot model and pure logic. No OS calls.
  model.rs          Listener, ProcessInfo, ProjectInfo, StopTarget, Snapshot (schemaVersion 1)
  assemble.rs       socket grouping, WSL/Docker forwarder merging, conflict marking
  infer.rs          project root inference, launcher detection, process classes
  net.rs            exposure, URLs, netsh excluded ranges, free-port candidates
  git.rs            rev-parse, status --porcelain=v2, worktree list parsers
  docker.rs         Docker Engine API types and port mapping
  wsl.rs            wsl.exe decoding, the probe script, ss and probe parsers, signal script
  redact.rs         secret masking in command lines
  cmdline.rs        CommandLineToArgvW-compatible splitting
  http.rs           minimal HTTP/1.1 for the Docker pipe
  time.rs           FILETIME, RFC 3339 and CIM datetime conversion
crates/win/       portpilot-win: every unsafe Win32 call
  net.rs            GetExtendedTcpTable / GetExtendedUdpTable (IPv4 and IPv6)
  process.rs        ToolHelp snapshot, limited query handles, token user, own lineage
  wmi.rs            Win32_Process command lines through WMI
  control.rs        verified handles, WM_CLOSE, the console-signal helper, terminate
  os.rs, time.rs, shell.rs
crates/engine/    portpilot-engine: discovery and actions
  discover.rs       the Discovery cache and snapshot()
  git.rs            Git CLI with --no-optional-locks, per-folder cache
  docker.rs         named-pipe client, containers, stop
  wsl.rs            running distros, probe, signal
  stop.rs           the stop flow
  doctor.rs         port diagnosis, free ports, environment
  projects.rs       listeners grouped by repository and worktree
  bin/fixture.rs    test server used by the integration tests
crates/cli/       portpilot-cli: the portctl binary
src-tauri/        the desktop shell: discovery thread, tray, typed commands
src/              React UI (Ports, Projects, Doctor, Settings)
```

## From socket to explained listener

```
GetExtendedTcpTable / GetExtendedUdpTable      (IPv4 + IPv6, owner PID)
  -> group by (protocol, port, PID)            one listener per owner, all its addresses
  -> ToolHelp snapshot                          names and parent PIDs of every process
  -> OpenProcess(QUERY_LIMITED_INFORMATION)     creation time, image path, token user
  -> parent chain                               stops where a parent is younger than its child
  -> WMI Win32_Process                          command lines, once per (PID, creation time)
  -> project inference                          command-line paths, then the executable folder
  -> git rev-parse / status                     repository, branch, commit, dirty, worktree
  -> launcher                                   nearest agent, editor or terminal in the chain
  -> Docker Engine API, wsl.exe + ss            containers and Linux listeners
  -> merge forwarders                           wslrelay / com.docker.backend fold into them
  -> conflicts                                  TCP ports with more than one owner
  -> redaction                                  secrets masked before anything leaves the engine
```

### Process identity

Windows reuses PIDs, and a parent PID can point at a newer process that took the number. Port Pilot therefore treats `(PID, creation time)` as the identity everywhere:

- A command line read through WMI is kept only if the creation time is the same before and after the query.
- A parent chain stops at the first parent created after its child: that PID was reused.
- A stop request carries the creation time the user saw. The engine opens the process with `SYNCHRONIZE | QUERY_LIMITED_INFORMATION | TERMINATE`, compares the creation time on that handle and keeps the handle open for the whole flow, so the PID cannot be reused while Port Pilot acts on it.

Processes Port Pilot cannot open (other users, elevated, protected) are listed with `accessDenied` and their stop action is withheld; the app never asks for `SeDebugPrivilege`.

### Project attribution

There is no public Windows API for another process's current directory (`NtQueryInformationProcess` is documented as internal and subject to change), so attribution has two confidence levels:

| Confidence | Source | Example |
|---|---|---|
| exact | `/proc/<pid>/cwd` inside WSL | `node` in `/home/a/shop` |
| exact | `com.docker.compose.project.working_dir` | a Compose service |
| inferred | an absolute path in the arguments | `node C:\src\shop\node_modules\vite\bin\vite.js` |
| inferred | the executable's folder | `C:\src\api\.venv\Scripts\python.exe`, `target\debug\app.exe` |

Paths under `node_modules` resolve to the folder holding the outermost `node_modules`; the walk then goes up to the first folder with a manifest (`package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, `*.csproj`, `.git` and others). Windows, Program Files, ProgramData, AppData and toolchain homes are never projects, and neither is the user profile itself. Relative paths (`node server.js`) are not guessed.

### WSL

Only distros that are already running are inspected (`wsl.exe --list --running --quiet`); Port Pilot never boots one. A probe script is piped to `sh -s` over stdin, so nothing is quoted through the Windows command line. It prints `ss -Hltnup`, then for each owning PID its working directory, start time (`/proc/<pid>/stat` field 22) and command line, and Git state from inside the distro. Docker Desktop's own distros are skipped.

On the Windows side, `wslrelay.exe` holds forwarded localhost ports in NAT mode. A relay with a matching Linux listener is folded into it; a relay without one is flagged. A Linux listener without a relay is flagged as not forwarded (expected in mirrored mode).

### Docker

The Docker Engine API on `\\.\pipe\dockerDesktopLinuxEngine` or `\\.\pipe\docker_engine` (or `DOCKER_HOST` when it names a pipe) gives `GET /containers/json` with published ports and Compose labels. Host listeners owned by Docker's forwarders are folded into the container that publishes the port; a forwarder with no container behind it is flagged as a possible stale allocation. Stopping uses `POST /containers/{id}/stop`; the forwarder process is never killed.

### The stop flow

See [security.md](security.md#stopping-processes) for the full rules. In short: identity check, refuse system/service/forwarder/own lineage, graceful request, wait, terminate only with consent.

CTRL+C can only be generated for a whole console and only by a process attached to it. The engine starts a short-lived copy of the current executable with `--pp-console-ctrl <pid> <allowed pids> [c|break]` and `DETACHED_PROCESS`; the helper attaches to the target's console, reads `GetConsoleProcessList`, and sends the signal only if every attached process is in the allowed set: the target, its children, and upwards the package runners (`pnpm`, `npm`, `yarn`, `cargo`, ...) and the first shell. Programs started from MSYS shells such as Git Bash inherit "ignore CTRL+C", so CTRL+BREAK follows after half the grace period.

## Desktop app

`src-tauri` runs discovery on its own thread (default every 3 s; WSL and Docker results are reused for 10 s, Git state for 10 s) and emits each snapshot to the UI and the tray. Commands are typed: a stop request names a listener id from the current snapshot, "open folder/terminal/VS Code" accepts only folders discovery found, "open URL" only a listener's own URL. The WebView has no shell, file system or opener permission.

Settings live in `%APPDATA%\PortPilot\settings.json`, or in `data\` next to the exe when a `portable` file sits there. Nothing else is stored.

## Testing

- Unit tests for every parser, the inference rules, redaction, time conversion and assembly (`cargo test -p portpilot-core`).
- Windows tests against the real socket tables, ToolHelp, WMI and process handles (`portpilot-win`).
- Integration tests in `crates/engine/tests/windows.rs` start the fixture server and check discovery, the identity check, CTRL+C in a private console, a server that ignores signals, a shared console that must not be signalled, and terminating a process tree.
- CLI contract tests for JSON output and exit codes.
- Frontend tests for filters, formatting and the completeness of the Turkish catalog.
