# Contributing

Thanks for helping. PortPilot ends processes on real machines, so the bar for changes to discovery and the stop flow is high, and the bar for everything else is friendly.

## Ground rules

- **A wrong owner is worse than no owner.** Attribution that cannot be shown to be right is marked inferred or left out. Never guess a working directory.
- **Graceful first, force on consent.** New destructive actions need a section in `docs/security.md` describing the identity check, what is refused and how the user confirms.
- **No telemetry**, analytics or accounts. The update check is the only network request; pull requests that add other network traffic will be closed.
- **New dependencies** need a license that `cargo deny check licenses` accepts and a reason in the pull request.
- **English for code, comments, UI strings and commits.** Every UI string also goes into `src/i18n/tr.ts`; the typecheck fails if a key is missing.

## Setup

Windows 10 or 11 with Rust (stable, MSVC), Node.js 22+, pnpm 9, Git and the Visual Studio C++ build tools.

```powershell
pnpm install
pnpm app:dev
```

## Before you open a pull request

All of these must pass; CI runs the same set.

```powershell
pnpm typecheck
pnpm lint
pnpm test
pnpm build
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- Parser or rule changes: add a unit test in `crates/core` with the real tool output.
- Stop-flow changes: add an integration test in `crates/engine/tests/windows.rs` using the fixture server.
- UI changes: check light and dark theme, a wide window and the minimum width (760 px). `node scripts/screenshots.mjs` captures all four; attach screenshots, but crop or blur anything from your own machine (user name, paths, processes). README pictures come from demo projects instead: `node scripts/demo-projects.mjs`, start the app the way it prints, `node scripts/screenshots.mjs --demo`, then `node scripts/demo-projects.mjs --clean`.
- User-visible changes: add a line to `CHANGELOG.md` under "Unreleased" and update `docs/` when behavior changes.

## Commits and releases

Conventional Commits (`feat(engine): …`, `fix(ui): …`, `docs: …`). Releases are tagged `vX.Y.Z` from `main`; the release workflow builds and signs the installer, the portable zip and `portctl.exe`, writes the update feed and publishes the release. See [docs/releasing.md](docs/releasing.md).

## Labels

`area/windows`, `area/wsl`, `area/docker`, `area/git`, `area/ui`, `area/cli`, `area/security`, `area/packaging`, `bug`, `enhancement`, `good-first-issue`, `help-wanted`, `needs-repro`, `access-denied`, `race-condition`.
