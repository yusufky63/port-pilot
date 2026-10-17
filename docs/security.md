# Safety, security and privacy

PortPilot can end processes, so its safety rules are part of the engine, not only the UI.

## Privilege model

- The app and `portctl` run as a normal user. They never enable `SeDebugPrivilege` and never install a service.
- Processes are opened with `PROCESS_QUERY_LIMITED_INFORMATION`. "Access denied" for another user's or an elevated process is an expected state: the listener is shown with what can be read, and its stop action is withheld.
- There is no elevated helper in 0.1. When one is added it will be a short-lived process started through UAC for one typed operation, never a generic command runner.

## Stopping processes

| Rule | Why |
|---|---|
| Identity is `(PID, creation time)`, checked on an open handle that stays open until the end | Windows reuses PIDs; a stale PID can name an unrelated process |
| System processes, services, WSL and Docker forwarders and PortPilot's own lineage are refused | Killing a forwarder breaks a runtime without stopping the real owner |
| The first request is always graceful | `TerminateProcess` skips the program's shutdown code and DLL cleanup |
| CTRL+C / CTRL+BREAK only when the console holds nothing but the target's tree, its package runners and its shell | A console signal reaches every process on that console |
| Terminating needs a second, explicit decision (`--force`, or the Terminate button after the result) | Force is unconditional and cannot be undone |
| WSL processes are signalled only after `/proc/<pid>/stat` start time matches | PIDs are reused inside Linux too |
| Containers are stopped through the Docker API, never by killing Docker's processes | The forwarder process is shared by every published port |

Stop requests from the UI name a listener id from the latest snapshot; the frontend never sends a PID, a command or a path to act on.

## Running project commands

| Rule | Why |
|---|---|
| A recipe is read only when the user asks (Run button, `portctl run`, `portctl lease`), and in the app only from a folder discovery found or the user picked in the native dialog | Opening a repository must never execute or even parse anything from it on its own |
| Before a command runs the first time, the exact command, folder, environment and ports are shown with *Trust and run*, *Run once* and *Cancel* | The user decides what runs with their rights |
| Trust is the SHA-256 of the run's command, `cwd` and environment as written; any change asks again. Trusted commands are listed in Settings and can be revoked | A repository update cannot swap in a different command under an old approval |
| The app's `run_start` re-reads the recipe and refuses to run if its digest or the ports differ from what the dialog showed | What the user saw is what runs |
| `cmd.exe` is started by its full path; `cwd` must stay inside the recipe's folder, after links are resolved | A planted `cmd.exe` or a link out of the project cannot redirect the run |
| Ports reach a command only through `{ports.<service>}` placeholders | PortPilot never rewrites a command that did not opt in |
| Runs are created suspended, put into a Job Object, then resumed; the job ends the run when its owner exits | No process of a run escapes the job or outlives PortPilot |
| Stopping a run sends CTRL+BREAK to its own process group; ending the job needs a second, explicit decision | The signal reaches only the run, and force stays the user's call |

## Threat model

| Threat | Mitigation |
|---|---|
| PID reuse between scan and action | Creation-time check on the handle used for the action |
| A process exits between scan and action | Reported as "already exited"; nothing else is touched |
| Wrong process tree receives CTRL+C | Console membership is checked by a helper attached to that console; any foreign process cancels the signal |
| Malicious repository content | PortPilot runs a project command only from `.portpilot.toml`, only on request and only after the trust prompt (see above). Otherwise it only runs `git` with `--no-optional-locks` and `GIT_TERMINAL_PROMPT=0`, and reads manifests by name. Recipes over 64 KiB or with unknown keys are refused |
| A trusted recipe changed later | The trust digest no longer matches, so the prompt shows the new command again |
| A run outliving PortPilot or holding ports after a crash | Every run is in a Job Object with kill-on-close; runs whose owner exited are closed in the history |
| Command-line secrets on screen, in bug reports or JSON | Masked by default (`--token=****`, URL passwords, known token formats); `--no-redact` and a setting show them |
| Environment secrets | Environment variables of other processes are never read |
| A tampered update or feed | The installer must carry a minisign signature that matches the public key compiled into the app; the updater refuses anything else. The private key lives only in the repository's Actions secrets and the maintainer's vault. Updates come over HTTPS from GitHub. |
| Local web page abusing the app | The app opens no network port; the UI talks to Rust through Tauri IPC only; the CSP blocks external connections |
| WebView compromise | No shell, file system or opener permissions; commands accept listener ids, run ids and known folders only. A run starts only from a recipe file on disk, in a known folder, with the digest of that file's command |
| Docker API misuse | Only `GET /containers/json`, `GET /version` and, after confirmation, `POST /containers/{id}/stop` with a validated hex id |
| Booting a WSL distro by accident | Only running distros are inspected |
| A port exposed to the network by mistake | Every listener shows its exposure; `0.0.0.0` and `::` are labelled "all interfaces" |

## Privacy

- No telemetry, analytics or crash reporting. The only internet request is the update check: the desktop app fetches `https://github.com/yusufky63/port-pilot/releases/latest/download/latest.json` 20 seconds after it starts and then once a day, plus whenever the user presses *Check now*. Nothing about the machine is sent. Settings > Updates turns the automatic check off. `portctl` makes no internet requests.
- Two files are written, both in the data folder: `settings.json` (theme, language, refresh interval, toggles) and `portpilot.db`.
- `portpilot.db` holds port leases (worktree folders, service names, ports), trusted run commands (as SHA-256 digests with the recipe folder and project name; not the command text), the runs PortPilot started, and the history: runs started and exited, and TCP ports that opened or closed, with process names, PIDs and project folders. History and finished runs are deleted after 14 days; Settings can turn history off and clear it.
- Command lines are not stored by default. With "Include the command line of each run" on, a run's command is stored masked. Command lines of other processes are never stored.
- The output of a managed run stays in memory and is gone when the run is removed or the app exits.
- Command lines are masked before they reach the UI, the CLI output or JSON, unless the user turns masking off. The run dialog and `portctl run` show a recipe's command exactly as it will run, since that is what the user approves.

## Reporting a vulnerability

See [SECURITY.md](../SECURITY.md).
