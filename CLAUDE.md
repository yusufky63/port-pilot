# Port Pilot: notes for coding agents

Windows desktop app + CLI that explains local dev ports (owner process, parents, project, Git worktree, WSL, Docker) and stops owners safely. Design: `docs/architecture.md`; safety rules: `docs/security.md`; plan: `ROADMAP.md`.

## Layout

- `crates/core` pure logic and parsers (no OS calls), `crates/win` all unsafe Win32, `crates/engine` discovery and the stop flow, `crates/cli` `portctl`, `src-tauri` desktop shell, `src` React UI.
- Same conventions as the author's other Tauri apps (ReClaim, Unlost): exact dependency versions, house-design tokens in `src/styles/globals.css` (one brand colour, blue), i18n in `src/i18n/en.ts` + `tr.ts`.

## Rules that must not regress

- Process identity is `(pid, creation time)`. Never act on a PID alone; never send a PID from the UI (stop requests carry a listener id).
- Graceful first. Console signals only when `GetConsoleProcessList` shows nothing outside the target's tree, its runners and its shell. Terminate only with explicit consent.
- Project folders are "exact" only when a runtime reports them (WSL `/proc`, Compose label). Everything else is "inferred"; never guess from relative paths.
- No network traffic, no persisted command lines, secrets masked by default.
- Git always runs with `--no-optional-locks`.

## Check matrix (all must pass)

```
pnpm typecheck && pnpm lint && pnpm test && pnpm build
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Integration tests start `pp-fixture` servers and stop them; Git Bash passes "ignore CTRL+C" to children, which is why the stop flow follows CTRL+C with CTRL+BREAK.

UI checks: `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9333"; pnpm app:dev`, then `node scripts/screenshots.mjs` (light/dark, 1440 and 760 px). Screenshots show the real machine; keep them out of the repository.
