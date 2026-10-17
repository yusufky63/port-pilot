# Releasing

Releases are automatic: pushing a `vX.Y.Z` tag builds, signs and publishes everything.

1. Move the "Unreleased" entries in `CHANGELOG.md` under a new `## [X.Y.Z] - YYYY-MM-DD` heading. That section becomes the release notes and the notes in the update banner.
2. Set the version in `package.json` and in `[workspace.package]` of `Cargo.toml`. The desktop app reads its version from `package.json`; the updater compares versions with semver, so `0.1.0-alpha.2` is newer than `0.1.0-alpha.1`.
3. Run the full check locally: `pnpm check`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
4. Commit, tag `vX.Y.Z` and push the tag.
5. The release workflow runs the checks, builds the NSIS installer with the updater signing key (`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in the repository's Actions secrets), builds `portctl.exe`, and runs `scripts/package-portable.ps1`. That writes the portable zip, `latest.json` (version, notes, the installer's URL and signature) and `SHA256SUMS.txt`. The release is published right away and marked *latest*, which is where installed copies look: `releases/latest/download/latest.json`.

The updater key pair belongs to the app: the public key is in `src-tauri/tauri.conf.json`, the private key and its password are in the Actions secrets and in the maintainer's vault (`PORTPILOT_UPDATER_PRIVATE_KEY`, `PORTPILOT_UPDATER_KEY_PASSWORD`). Losing the private key means installed copies can no longer update themselves; a new key needs a new public key in the app and a manual download once.

The executables are not code-signed with a certificate yet. The release notes tell users how to verify a download with the checksums.

## Local packaging

```powershell
pnpm app:build   # with TAURI_SIGNING_PRIVATE_KEY set, so the installer gets a .sig
pnpm cli:build
pnpm portable
```

`release\` then holds the installer, the portable zip, `latest.json` and `SHA256SUMS.txt`; `release-notes.md` holds the notes.
