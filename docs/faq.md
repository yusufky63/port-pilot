# FAQ

## Why does a listener say "inferred" next to its project?

Windows has no public API that tells one process another process's working directory. Port Pilot names a project only when it can show its reasoning: an absolute path in the command line (`node C:\src\shop\node_modules\vite\bin\vite.js`) or the executable's own folder (`.venv\Scripts\python.exe`, `target\debug\app.exe`). It then walks up to the first folder with a manifest. Inside WSL the working directory is read from `/proc`, and Compose labels name the folder directly; those are marked "exact".

## Why is there no project for `node server.js`?

A relative path cannot be resolved without the working directory, and guessing would attach the wrong project. Start the server through your package manager (`pnpm dev`) or with an absolute path and the project is found.

## Why does "Stop" say there is no safe graceful channel?

Console programs are asked to close with CTRL+C, which Windows delivers to every process on the same console. When an editor, a coding agent or another unrelated program shares that console, sending it would stop them too, so Port Pilot does not send it. You can terminate the process instead, or stop it in the terminal where it runs.

## Why CTRL+BREAK after CTRL+C?

Programs started from MSYS shells such as Git Bash inherit a flag that makes them ignore CTRL+C. CTRL+BREAK is not affected by it and its default handler still ends the process normally, so Port Pilot sends it on the same console after half the grace period.

## Docker says "port is already allocated", but nothing listens

Look for a Docker row with the note "Docker holds this port but no running container publishes it". Docker Desktop's forwarder keeps the port; restarting Docker Desktop releases it. Also check Doctor: the port may sit in a range Windows reserved (`netsh interface ipv4 show excludedportrange protocol=tcp`), which no program can bind.

## A WSL server works inside Linux but not on `localhost` in Windows

The detail panel shows whether a Windows-side forwarder (`wslrelay.exe`) holds the port. Without one, NAT-mode WSL does not forward that socket; binding to `0.0.0.0` instead of a specific address, or switching WSL to mirrored networking, usually fixes it.

## Does Port Pilot start my WSL distros?

No. It only reads distros that are already running.

## Why are some processes "access denied"?

They belong to another user or run elevated, and Port Pilot runs as a normal user. Their ports are still listed with what Windows lets a normal user read; stopping them is not offered.

## What does Port Pilot send over the network?

Nothing. There is no telemetry, crash reporting or update check. See [security.md](security.md#privacy).

## Where are settings stored?

`%APPDATA%\PortPilot\settings.json`, or a `data` folder next to the exe in the portable build. Uninstalling with "Delete app data" removes the folder.

## Does it work on Windows 10?

Yes, on a best-effort basis. Windows 10 left Microsoft's general support on 14 October 2025, so Windows 11 is where Port Pilot is tested first.
