# Architecture

PortPilot is a Windows desktop app and CLI that explains local development ports, stops their owners safely, and runs a project's dev commands when the project opts in. A Rust engine does every OS call; a React UI and the `portctl` CLI render the same snapshot. The research behind the decisions is summarized in [research.md](research.md).

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
  recipe.rs         .portpilot.toml parser, {ports.<service>} rendering, trust text
  lease.rs          lease owner keys and the port allocator
  output.rs         in-memory run output: ANSI stripping, line splitting, caps
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
  job.rs            Job Objects, suspended spawn into a job, kill-on-close
  dialog.rs         the native folder picker
  crypto.rs         SHA-256 through CNG for trust digests
  os.rs, time.rs, shell.rs
crates/engine/    portpilot-engine: discovery and actions
  discover.rs       the Discovery cache and snapshot()
  git.rs            Git CLI with --no-optional-locks, per-folder cache
  docker.rs         named-pipe client, containers, stop
  wsl.rs            running distros, probe, signal
  stop.rs           the stop flow
  doctor.rs         port diagnosis, free ports, environment
  projects.rs       listeners grouped by repository and worktree
  paths.rs          the data folder shared by the app and portctl
  store.rs          portpilot.db: leases, trusted commands, runs, history (rusqlite)
  recipes.rs        finding and reading .portpilot.toml, the exact run folder
  leases.rs         leases checked against listeners, reserved ranges and a bind
  runs.rs           launching runs, the app's RunManager, run history records
  history.rs        port changes between snapshots
  bin/fixture.rs    test server used by the integration tests
crates/cli/       portpilot-cli: the portctl binary
src-tauri/        the desktop shell: discovery thread, tray, typed commands, run commands
src/              React UI (Ports, Projects, Runs, History, Doctor, Settings)
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

Windows reuses PIDs, and a parent PID can point at a newer process that took the number. PortPilot therefore treats `(PID, creation time)` as the identity everywhere:

- A command line read through WMI is kept only if the creation time is the same before and after the query.
- A parent chain stops at the first parent created after its child: that PID was reused.
- A stop request carries the creation time the user saw. The engine opens the process with `SYNCHRONIZE | QUERY_LIMITED_INFORMATION | TERMINATE`, compares the creation time on that handle and keeps the handle open for the whole flow, so the PID cannot be reused while PortPilot acts on it.

Processes PortPilot cannot open (other users, elevated, protected) are listed with `accessDenied` and their stop action is withheld; the app never asks for `SeDebugPrivilege`.

### Project attribution

There is no public Windows API for another process's current directory (`NtQueryInformationProcess` is documented as internal and subject to change), so attribution has two confidence levels:

| Confidence | Source | Example |
|---|---|---|
| exact | `/proc/<pid>/cwd` inside WSL | `node` in `/home/a/shop` |
| exact | `com.docker.compose.project.working_dir` | a Compose service |
| exact | the folder PortPilot started a managed run in | `pnpm dev` started from the Runs page |
| inferred | an absolute path in the arguments | `node C:\src\shop\node_modules\vite\bin\vite.js` |
| inferred | the executable's folder | `C:\src\api\.venv\Scripts\python.exe`, `target\debug\app.exe` |

Paths under `node_modules` resolve to the folder holding the outermost `node_modules`; the walk then goes up to the first folder with a manifest (`package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, `*.csproj`, `.git` and others). Windows, Program Files, ProgramData, AppData and toolchain homes are never projects, and neither is the user profile itself. Relative paths (`node server.js`) are not guessed.

### WSL

Only distros that are already running are inspected (`wsl.exe --list --running --quiet`); PortPilot never boots one. A probe script is piped to `sh -s` over stdin, so nothing is quoted through the Windows command line. It prints `ss -Hltnup`, then for each owning PID its working directory, start time (`/proc/<pid>/stat` field 22) and command line, and Git state from inside the distro. Docker Desktop's own distros are skipped.

On the Windows side, `wslrelay.exe` holds forwarded localhost ports in NAT mode. A relay with a matching Linux listener is folded into it; a relay without one is flagged. A Linux listener without a relay is flagged as not forwarded (expected in mirrored mode).

### Docker

The Docker Engine API on `\\.\pipe\dockerDesktopLinuxEngine` or `\\.\pipe\docker_engine` (or `DOCKER_HOST` when it names a pipe) gives `GET /containers/json` with published ports and Compose labels. Host listeners owned by Docker's forwarders are folded into the container that publishes the port; a forwarder with no container behind it is flagged as a possible stale allocation. Stopping uses `POST /containers/{id}/stop`; the forwarder process is never killed.

### The stop flow

See [security.md](security.md#stopping-processes) for the full rules. In short: identity check, refuse system/service/forwarder/own lineage, graceful request, wait, terminate only with consent.

CTRL+C can only be generated for a whole console and only by a process attached to it. The engine starts a short-lived copy of the current executable with `--pp-console-ctrl <pid> <allowed pids> [c|break]` and `DETACHED_PROCESS`; the helper attaches to the target's console, reads `GetConsoleProcessList`, and sends the signal only if every attached process is in the allowed set: the target, its children, and upwards the package runners (`pnpm`, `npm`, `yarn`, `cargo`, ...) and the first shell. Programs started from MSYS shells such as Git Bash inherit "ignore CTRL+C", so CTRL+BREAK follows after half the grace period.

## Managed runs

A project opts in with `.portpilot.toml`:

```toml
[project]
name = "shop"              # optional; the repository or folder name otherwise

[ports.web]
preferred = 5173           # tried first
range = "5173-5199"        # or [5173, 5199]; default preferred..preferred+99

[run.dev]
command = "pnpm dev --port {ports.web} --strictPort"
cwd = "apps/web"           # optional, inside the recipe's folder
env = { API_URL = "http://localhost:{ports.api}" }
```

The file is read only on an explicit action (the Run button, `portctl run`, `portctl lease`), from the chosen folder or its parents up to the repository root. It is limited to 64 KiB and unknown keys are errors. Only `{ports.<service>}` is replaced; every other `{...}` stays as written, and a command without placeholders gets no port.

Starting a run:

```
read recipe -> pick run -> exact folder (canonical, must stay inside the project)
  -> ports: the worktree's leases, or the first free port of each range
  -> show command, folder, environment, ports        (nothing has run yet)
  -> Trust and run | Run once | Cancel               (trusted: SHA-256 of command, cwd, env)
  -> run_start re-reads the recipe, checks the digest and the ports, claims the leases
  -> CreateJobObject (KILL_ON_JOB_CLOSE)
  -> cmd.exe /d /s /c "<command>" with CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP
     (+ CREATE_NO_WINDOW and piped output in the app; the caller's console for portctl run)
  -> AssignProcessToJobObject, then resume: nothing runs outside the job
  -> record PID, creation time, folder, ports in portpilot.db
```

`cmd.exe` is started by its full path from `%SystemRoot%`, so a `cmd.exe` in the project cannot take its place. The app captures stdout and stderr into a capped in-memory log (5,000 lines, 1 MiB) and drops it when the run is removed or the app exits.

Attribution: discovery receives every `(PID, creation time)` in the app's jobs. A listener whose process or any ancestor matches is the run's, with `confidence: exact`, `source: launched` and `managed: { runId, name }`. `portctl` reads active runs from `portpilot.db` (runs whose owner process is alive) and matches them through the parent chain, so runs started by the app and by `portctl run` are attributed in both.

Stopping: CTRL+BREAK goes to the run's process group (its first process's PID) through the console helper, which still checks that every process on the console belongs to the job. Only the run's processes are in that group. If the job is still not empty after the grace period, the user can end it with `TerminateJobObject`, which ends every process the run started. CTRL+BREAK also gets past cmd.exe's "Terminate batch job (Y/N)?" for `.cmd` shims such as `pnpm.cmd`, because the run's stdin is empty. A run is over when its job has no process left.

Owning: a job is created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so a run never outlives the app or `portctl run` that owns it, including after a crash. Quitting the app with active runs asks first, then stops each run gracefully and ends the ones still running after the grace period. Runs whose owner exited are marked ended in `portpilot.db` by the next PortPilot process that looks.

## Port leases

A lease reserves one port for one service of one worktree. Its owner is the repository's shared Git directory plus the worktree folder (both lowercased, `\` separators) plus the service name; projects outside Git use their folder for both. Leases are rows in `portpilot.db` with a unique port, so the app and `portctl` never hand the same port to two owners; allocations run in an immediate SQLite transaction.

Allocation (`portpilot-core::lease::allocate`): an existing lease is kept while it fits the recipe's range. Otherwise ports are tried from `preferred` to the end of the range, then from its start; a port is skipped when another owner leases it, a listener uses it (Windows, WSL or Docker), Windows reserved it, or a test bind on `127.0.0.1` and `[::1]` fails. "Allocate the next free port for this worktree" starts after the current port and wraps. A lease stays until it is released; leases of worktrees that no longer exist are shown as removed.

## History and the local database

`portpilot.db` (SQLite through `rusqlite` with the bundled SQLite, WAL mode) sits next to `settings.json`:

| Table | Holds | Kept |
|---|---|---|
| `leases` | owner key, service, port, worktree and Git folder, project name | until released |
| `trust` | project key, run name, SHA-256 of the run's command, cwd and environment, recipe folder, project name | until revoked |
| `runs` | run id, name, project, folder, PID, creation time, owner process, ports, exit code; masked command only when enabled | 14 days after the run ended |
| `events` | run started and exited, TCP port opened and closed: time, port, PID, process name, project, folder, runtime, exit code; masked command only when enabled | 14 days |

The app's discovery thread compares each snapshot with the previous one and records TCP listeners of user processes, WSL and Docker that appeared or went away; the first snapshot after start is only a baseline. Runs are recorded by whoever owns them (the app or `portctl run`). Old rows are deleted when the database opens and once an hour. History can be turned off and cleared in Settings; with history off, run records are deleted when the run ends.

## Desktop app

`src-tauri` runs discovery on its own thread (default every 3 s; WSL and Docker results are reused for 10 s, Git state for 10 s) and emits each snapshot to the UI and the tray. Commands are typed: a stop request names a listener id from the current snapshot, "open folder/terminal/VS Code" accepts only folders discovery found, "open URL" only a listener's own URL. The WebView has no shell, file system or opener permission.

The only network request is the update check (`src-tauri/src/update.rs`): Tauri's updater fetches `latest.json` of the newest GitHub release 20 seconds after start and then daily, and installs a package only if its minisign signature matches the public key in `tauri.conf.json`. Portable copies only open the release page.

Settings live in `%APPDATA%\PortPilot\settings.json`, or in `data\` next to the exe when a `portable` file sits there; `portpilot.db` sits next to it. `portctl` uses the same folder, and `PORTPILOT_DATA_DIR` overrides it (the tests use that).

A recipe is read only from a folder discovery found or one the user chose in the native folder dialog, which Rust opens; the WebView never names a folder PortPilot did not see. Run commands take a folder, a run name, the digest the dialog showed and the ports it showed; stop commands take a run id.

## Testing

- Unit tests for every parser, the inference rules, redaction, time conversion and assembly (`cargo test -p portpilot-core`).
- Windows tests against the real socket tables, ToolHelp, WMI and process handles (`portpilot-win`).
- Integration tests in `crates/engine/tests/windows.rs` start the fixture server and check discovery, the identity check, CTRL+C in a private console, a server that ignores signals, a shared console that must not be signalled, and terminating a process tree.
- Integration tests in `crates/engine/tests/runs.rs` start the fixture as a managed run inside a job and check exact attribution, CTRL+BREAK to the run's process group, a run that ignores it and is ended through its job only with consent (grandchildren included), a `.cmd` run, history records without command lines, and a recipe port reaching the command through a lease.
- Unit tests for the recipe parser, the lease allocator, run output, the database (leases, trust, runs, events, retention) and Job Objects.
- CLI contract tests for JSON output and exit codes, including `lease`, `run` and `history` with a data folder of their own.
- Frontend tests for filters, formatting and the completeness of the Turkish catalog.
