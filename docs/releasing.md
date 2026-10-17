# Releasing

1. Move the "Unreleased" entries in `CHANGELOG.md` under a new `## [X.Y.Z] - YYYY-MM-DD` heading.
2. Set the version in `package.json` and in `[workspace.package]` of `Cargo.toml`. The desktop app reads its version from `package.json`.
3. Run the full check locally: `pnpm check`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
4. Commit, tag `vX.Y.Z` and push the tag.
5. The release workflow builds the NSIS installer (`pnpm tauri build --bundles nsis`) and `portctl.exe`, packages the portable zip with `scripts/package-portable.ps1`, writes `SHA256SUMS.txt` and creates a **draft** release whose notes are the changelog section. Versions with a pre-release part (`-alpha.1`) are marked as pre-releases.
6. Download the artifacts from the draft, install and run them on a clean Windows 11 machine, then publish the draft by hand.

Builds are not code-signed yet. The README tells users how to verify a download with the checksums.

## Local packaging

```powershell
pnpm app:build
pnpm cli:build
pnpm portable
```

`release\` then holds the installer, the portable zip and `SHA256SUMS.txt`.
