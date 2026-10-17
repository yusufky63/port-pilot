# PortPilot: notes for coding agents

Windows desktop app + CLI that explains local dev ports (owner process, parents, project, Git worktree, WSL, Docker), stops owners safely, and runs a project's dev commands from `.portpilot.toml` with port leases and a local history. Design: `docs/architecture.md`; safety rules: `docs/security.md`; plan: `ROADMAP.md`.

## Layout

- `crates/core` pure logic and parsers (no OS calls), `crates/win` all unsafe Win32, `crates/engine` discovery and the stop flow, `crates/cli` `portctl`, `src-tauri` desktop shell, `src` React UI.
- Same conventions as the author's other Tauri apps (ReClaim, Unlost): exact dependency versions, house-design tokens in `src/styles/globals.css` (one brand colour, blue), i18n in `src/i18n/en.ts` + `tr.ts`.

## Rules that must not regress

- Process identity is `(pid, creation time)`. Never act on a PID alone; never send a PID from the UI (stop requests carry a listener id).
- Graceful first. Console signals only when `GetConsoleProcessList` shows nothing outside the target's tree, its runners and its shell. Terminate only with explicit consent.
- Project folders are "exact" only when a runtime reports them (WSL `/proc`, Compose label) or PortPilot started the run there. Everything else is "inferred"; never guess from relative paths.
- Nothing from a repository runs without the trust prompt; trust is the SHA-256 of the run's command, cwd and env. Recipes are read only on explicit user action, and ports enter commands only through `{ports.<service>}`.
- Managed runs: suspended spawn into a Job Object with kill-on-close, `CREATE_NEW_PROCESS_GROUP`, graceful stop is CTRL+BREAK to the run's group, `TerminateJobObject` only on consent.
- No network traffic except the signed-update check (Settings can turn it off), no persisted command lines (unless the user enables them for run history, masked), secrets masked by default. History keeps 14 days.
- Git always runs with `--no-optional-locks`.

## Check matrix (all must pass)

```
pnpm typecheck && pnpm lint && pnpm test && pnpm build
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Integration tests start `pp-fixture` servers and stop them; Git Bash passes "ignore CTRL+C" to children, which is why the stop flow follows CTRL+C with CTRL+BREAK. Tests that touch leases, trust or history set `PORTPILOT_DATA_DIR` or use `Store::in_memory()`, never the user's data folder.

UI checks: `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9333"; pnpm app:dev`, then `node scripts/screenshots.mjs` (light/dark, 1440 and 760 px). Screenshots show the real machine; keep them out of the repository. README pictures in `docs/screenshots` come from `scripts/demo-projects.mjs` plus `screenshots.mjs --demo` (run both from PowerShell; Git Bash mangles the Windows paths).
