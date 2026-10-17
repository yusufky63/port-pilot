# Safety, security and privacy

Port Pilot can end processes, so its safety rules are part of the engine, not only the UI.

## Privilege model

- The app and `portctl` run as a normal user. They never enable `SeDebugPrivilege` and never install a service.
- Processes are opened with `PROCESS_QUERY_LIMITED_INFORMATION`. "Access denied" for another user's or an elevated process is an expected state: the listener is shown with what can be read, and its stop action is withheld.
- There is no elevated helper in 0.1. When one is added it will be a short-lived process started through UAC for one typed operation, never a generic command runner.

## Stopping processes

| Rule | Why |
|---|---|
| Identity is `(PID, creation time)`, checked on an open handle that stays open until the end | Windows reuses PIDs; a stale PID can name an unrelated process |
| System processes, services, WSL and Docker forwarders and Port Pilot's own lineage are refused | Killing a forwarder breaks a runtime without stopping the real owner |
| The first request is always graceful | `TerminateProcess` skips the program's shutdown code and DLL cleanup |
| CTRL+C / CTRL+BREAK only when the console holds nothing but the target's tree, its package runners and its shell | A console signal reaches every process on that console |
| Terminating needs a second, explicit decision (`--force`, or the Terminate button after the result) | Force is unconditional and cannot be undone |
| WSL processes are signalled only after `/proc/<pid>/stat` start time matches | PIDs are reused inside Linux too |
| Containers are stopped through the Docker API, never by killing Docker's processes | The forwarder process is shared by every published port |

Stop requests from the UI name a listener id from the latest snapshot; the frontend never sends a PID, a command or a path to act on.

## Threat model

| Threat | Mitigation |
|---|---|
| PID reuse between scan and action | Creation-time check on the handle used for the action |
| A process exits between scan and action | Reported as "already exited"; nothing else is touched |
| Wrong process tree receives CTRL+C | Console membership is checked by a helper attached to that console; any foreign process cancels the signal |
| Malicious repository content | Port Pilot never runs project scripts. It only runs `git` with `--no-optional-locks` and `GIT_TERMINAL_PROMPT=0`, and reads manifests by name |
| Command-line secrets on screen, in bug reports or JSON | Masked by default (`--token=****`, URL passwords, known token formats); `--no-redact` and a setting show them |
| Environment secrets | Environment variables of other processes are never read |
| Local web page abusing the app | The app opens no network port; the UI talks to Rust through Tauri IPC only; the CSP blocks external connections |
| WebView compromise | No shell, file system or opener permissions; commands accept listener ids and known folders only |
| Docker API misuse | Only `GET /containers/json`, `GET /version` and, after confirmation, `POST /containers/{id}/stop` with a validated hex id |
| Booting a WSL distro by accident | Only running distros are inspected |
| A port exposed to the network by mistake | Every listener shows its exposure; `0.0.0.0` and `::` are labelled "all interfaces" |

## Privacy

- No telemetry, analytics, crash reporting or update checks. The app makes no internet requests at all.
- Nothing about processes is persisted: command lines, paths, Git state and container names live in memory and disappear with the app.
- The only file written is `settings.json` (theme, language, refresh interval, toggles).
- Command lines are masked before they reach the UI, the CLI output or JSON, unless the user turns masking off.

## Reporting a vulnerability

See [SECURITY.md](../SECURITY.md).
