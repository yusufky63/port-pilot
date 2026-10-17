# FAQ

## Why does a listener say "inferred" next to its project?

Windows has no public API that tells one process another process's working directory. PortPilot names a project only when it can show its reasoning: an absolute path in the command line (`node C:\src\shop\node_modules\vite\bin\vite.js`) or the executable's own folder (`.venv\Scripts\python.exe`, `target\debug\app.exe`). It then walks up to the first folder with a manifest. Inside WSL the working directory is read from `/proc`, and Compose labels name the folder directly; those are marked "exact".

## Why is there no project for `node server.js`?

A relative path cannot be resolved without the working directory, and guessing would attach the wrong project. Start the server through your package manager (`pnpm dev`) or with an absolute path and the project is found.

## Why does "Stop" say there is no safe graceful channel?

Console programs are asked to close with CTRL+C, which Windows delivers to every process on the same console. When an editor, a coding agent or another unrelated program shares that console, sending it would stop them too, so PortPilot does not send it. You can terminate the process instead, or stop it in the terminal where it runs.

## Why CTRL+BREAK after CTRL+C?

Programs started from MSYS shells such as Git Bash inherit a flag that makes them ignore CTRL+C. CTRL+BREAK is not affected by it and its default handler still ends the process normally, so PortPilot sends it on the same console after half the grace period.

## Docker says "port is already allocated", but nothing listens

Look for a Docker row with the note "Docker holds this port but no running container publishes it". Docker Desktop's forwarder keeps the port; restarting Docker Desktop releases it. Also check Doctor: the port may sit in a range Windows reserved (`netsh interface ipv4 show excludedportrange protocol=tcp`), which no program can bind.

## A WSL server works inside Linux but not on `localhost` in Windows

The detail panel shows whether a Windows-side forwarder (`wslrelay.exe`) holds the port. Without one, NAT-mode WSL does not forward that socket; binding to `0.0.0.0` instead of a specific address, or switching WSL to mirrored networking, usually fixes it.

## Does PortPilot start my WSL distros?

No. It only reads distros that are already running.

## Why are some processes "access denied"?

They belong to another user or run elevated, and PortPilot runs as a normal user. Their ports are still listed with what Windows lets a normal user read; stopping them is not offered.

## Why does PortPilot ask before running a project's command?

A `.portpilot.toml` comes with the repository, and anyone who can change the repository can change the command in it. PortPilot therefore shows the exact command, folder, environment and ports, and runs it only after you choose *Trust and run* or *Run once*. A trusted command is remembered by its SHA-256; if the recipe changes the command, its folder or its environment, PortPilot asks again. Trusted commands are listed in Settings and can be revoked.

## Will PortPilot add `--port` to my dev server?

Only where the recipe writes `{ports.<service>}`. Commands without a placeholder run exactly as written and get no port from PortPilot.

## Two worktrees of my repository both want port 5173

Give the service a range in `.portpilot.toml` (`[ports.web] range = "5173-5199"`) and use `{ports.web}` in the command. Each worktree gets its own lease from that range, and keeps it until you release it (Runs page, or `portctl lease release web`). When a leased port is taken by something else, the run dialog and `portctl run` offer to allocate the next free port for that worktree.

## What happens to my runs when PortPilot quits?

Quitting with active runs asks first, then sends each run CTRL+BREAK, waits the grace period and ends the runs that are still going. If PortPilot crashes, Windows ends the runs with it: they live in a Job Object that closes with the app. `portctl run` works the same way in its own terminal: CTRL+C stops the run gracefully, a second CTRL+C after the grace period ends it.

## What is in the history, and how do I turn it off?

Runs (start, exit code, how they ended) and TCP ports that opened or closed while PortPilot ran, with the process name, PID and project folder. Entries older than 14 days are deleted. Command lines are stored only when you turn on "Include the command line of each run", and then masked. Settings has a switch to stop recording and a button to clear the history.

## What does PortPilot send over the network?

Nothing. There is no telemetry, crash reporting or update check. See [security.md](security.md#privacy).

## Where are settings stored?

`%APPDATA%\PortPilot\settings.json` and `portpilot.db` (leases, trusted commands, runs and history), or a `data` folder next to the exe in the portable build. `portctl` uses the same folder. Uninstalling with "Delete app data" removes the folder.

## Does it work on Windows 10?

Yes, on a best-effort basis. Windows 10 left Microsoft's general support on 14 October 2025, so Windows 11 is where PortPilot is tested first.
