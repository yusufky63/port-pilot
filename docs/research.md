# Research summary

The design follows a research report written before the first line of code (October 2026). This page keeps its conclusions and sources; the full report is not part of the repository.

## The problem

The port number is rarely the problem. The owner is: Docker reports "port is already allocated" while `netstat` shows nothing, WSL forwarding works for some ports and not others, Windows processes ignore graceful shutdown, and parallel AI agents run the same `pnpm dev` in several Git worktrees at once. A port tool has to *explain* ownership before it offers to *kill* anything.

Representative reports:

- Docker: [docker/compose#4950](https://github.com/docker/compose/issues/4950), "Bind for 0.0.0.0:4000 failed: port is already allocated" on Stack Overflow, Docker forum threads where the port stays allocated after containers stop.
- WSL: [microsoft/WSL#4636](https://github.com/microsoft/WSL/issues/4636), [#5942](https://github.com/microsoft/WSL/issues/5942), [#5439](https://github.com/microsoft/WSL/issues/5439), [#6953](https://github.com/microsoft/WSL/issues/6953), [#9508](https://github.com/microsoft/WSL/issues/9508), [#4406](https://github.com/microsoft/WSL/issues/4406).
- Shutdown and races: [hashicorp/nomad#10731](https://github.com/hashicorp/nomad/issues/10731) (process did not exit), [python/cpython#112190](https://github.com/python/cpython/issues/112190) (CTRL_BREAK without a shared console), [giampaolo/psutil#2519](https://github.com/giampaolo/psutil/issues/2519) (process exits before the signal).
- UX: [raycast/extensions#10283](https://github.com/raycast/extensions/issues/10283) (open a port in the browser), [microsoft/PowerToys#21623](https://github.com/microsoft/PowerToys/issues/21623) (entries nobody remembers adding).

## Prior art

| Project | What it does | What Port Pilot takes from it |
|---|---|---|
| [m4cd4r4/PortPilot](https://github.com/m4cd4r4/PortPilot) | Electron port and app manager with Claude and VS Code integration, worktree runs | Agent awareness, "reuse the running server", free port per worktree |
| [sriinnu/portpilot](https://github.com/sriinnu/portpilot) | macOS menu bar app and CLI | Tray-first access |
| [flaviocopes/port-pilot](https://github.com/flaviocopes/port-pilot) | CLI, TUI and menu bar over one scanner | One engine, several faces |
| [VolchokLV/portpilot](https://github.com/VolchokLV/portpilot) | `.test` domains, reverse proxy, mkcert | Human-friendly endpoints (later, not in 0.1) |
| [BeyondXinXin/PortPilot](https://github.com/BeyondXinXin/PortPilot) | Publishes local services to other devices | Out of scope |
| Raycast Port Manager | Listeners, kill, exposure labels; checks process start time before force kill | Start-time validation, loopback/wildcard labels |
| [System Informer](https://github.com/winsiderss/systeminformer) | Deep process and network inspection | The quality bar for Windows internals, not the scope |
| [mgesmundo/port-manager](https://github.com/mgesmundo/port-manager) | Port range allocation library | Lease model for the next milestone |

Several projects already use the name "PortPilot"; the final product name is still open.

## Decisions

| Decision | Reason | Source |
|---|---|---|
| Identity is PID + creation time | PIDs and parent PIDs are reused | [Win32_Process](https://learn.microsoft.com/windows/win32/cimwin32prov/win32-process), [GetProcessTimes](https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes) |
| Owner PIDs from the IP Helper extended tables | Native, IPv4 and IPv6, no `netstat` parsing | [MIB_TCPTABLE_OWNER_PID](https://learn.microsoft.com/windows/win32/api/tcpmib/ns-tcpmib-mib_tcptable_owner_pid) |
| Command lines through WMI | The PEB route (`NtQueryInformationProcess`) is internal | [NtQueryInformationProcess](https://learn.microsoft.com/windows/win32/api/winternl/nf-winternl-ntqueryinformationprocess) |
| Limited query rights, no debug privilege | Protected processes restrict access by design | [Process security and access rights](https://learn.microsoft.com/windows/win32/procthread/process-security-and-access-rights) |
| Graceful first, terminate on consent | `TerminateProcess` is unconditional and skips DLL detach | [TerminateProcess](https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess) |
| Console signals only on a console the target owns | `GenerateConsoleCtrlEvent` reaches the whole console | [GenerateConsoleCtrlEvent](https://learn.microsoft.com/windows/console/generateconsolectrlevent), [Console process groups](https://learn.microsoft.com/windows/console/console-process-groups) |
| Git CLI, not libgit2 | Worktree layout is Git's business | [git-worktree](https://git-scm.com/docs/git-worktree), [git-rev-parse](https://git-scm.com/docs/git-rev-parse) |
| Docker Engine API, not Docker Desktop internals | Versioned, documented port mappings and labels | [Docker Engine API](https://docs.docker.com/reference/api/engine/) |
| Exact / inferred confidence for projects | No public API for another process's working directory | as above |
| Tauri 2 + Rust | Small installer, Rust engine shared with the CLI, NSIS and MSI bundling | [Tauri 2](https://v2.tauri.app/) |
| MIT OR Apache-2.0 | The Rust ecosystem's norm; Apache adds a patent grant | |
| No telemetry | Nothing about a developer's processes needs to leave the machine | |

## Roadmap from the report

See [ROADMAP.md](../ROADMAP.md).
