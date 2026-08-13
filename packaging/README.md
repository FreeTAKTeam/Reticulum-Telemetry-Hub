# RCH Rust Packaging

The Rust packaging line now has two release package shapes:

- Server package: deployable `r3akt-rch-server` binary, `r3akt-tak-service`
  binary, checksum-recorded LXMF `0.9.9` `reticulumd` binary with ZeroMQ
  support, mandatory ZeroMQ southbound configuration, shared UI bundle, service
  helper files, config templates, and checksums. Current release CI builds
  Windows x64, macOS x64, macOS arm64, Linux AMD64, and Linux Raspberry Pi 64
  server archives.
- Desktop packages: Tauri bundles from `apps/rch-desktop` with `reticulumd`,
  the Rust server, and TAK service sidecars. Current CI builds Windows x64 NSIS and Linux x64
  AppImage artifacts.

Python 2.9.x packaging remains on `rch-python` and keeps using the existing
Electron wrapper.

The server-only alpha gate remains `scripts/release-readiness.ps1
-ServerOnlyAlpha`. Full release packaging is handled by
`.github/workflows/rust-release.yml`, which mirrors the Python release workflow
shape: manual workflow artifacts on `workflow_dispatch` and file attachment
when a GitHub release is published. Server package names include the resolved
release version, for example
`rch-rust-full-windows-x64-v3.0.0-preview.10.zip`; the same version, Git ref,
and commit SHA are written into `release-manifest.json` inside the archive.
Manual workflow runs default to `v3.0.0-preview.10` and can override that label
with the `release_version` input. While `main` remains the default branch,
GitHub does not expose `workflow_dispatch` for workflows that only exist on
`rust-next`, so the release workflow also runs on relevant `rust-next` pushes
to validate packaging before the default-branch cutover.

Latest staging evidence before the macOS and Raspberry Pi 64 matrix expansion:
`Build Rust Release Packages` run `26696071364`
passed on commit `8dc69773af38ced251138c007c6f0bdc9543ea02`. It uploaded
`rch-rust-full-windows-x64-rust-next`, `rch-rust-full-linux-x64-rust-next`,
`rch-desktop-windows-x64-nsis`, and `rch-desktop-linux-x64-appimage`; downloaded
artifacts matched their SHA-256 sidecars.

Draft notes for the latest Rust preview are in
`docs/release-notes-v3.0.0-preview.10.md`.

Local desktop builds normally compile `reticulumd` from the sibling
`LXMF-rs` checkout. Set `RCH_RETICULUMD_BINARY` to an absolute, validated
LXMF 0.9.9 `reticulumd` path when that checkout is intentionally dirty; hosted
packages always build the pinned clean LXMF commit.

Pull request quality control is handled by
`.github/workflows/rust-pr-quality.yml`. It runs Rust 1.88 formatting, clippy,
locked workspace tests, release builds for the server and TAK service, and
`cargo audit`; the workspace separately verifies its declared Rust 1.85
minimum version.

## Python Store Migration

Rust v3 packages use an offline, local-only production migration wrapper before
the Rust server starts. Run a dry run first to inventory the Python store,
Reticulum identity/config inputs, runtime file directories, and target paths:

```powershell
scripts\import-python-rch-production.ps1 -SourceRoot . -LegacyStore RTH_Store -TargetDir target\production-rch-3 -DryRun
```

The dry run writes `migration-plan.json` without copying data or running the
Rust database converter. The apply run copies `config.ini`, `identity`,
Reticulum transport identity/config, `telemetry.db`, root `telemetry.db`, files,
images, and LXMF runtime data, then converts `rth_api.sqlite` into the Rust
`rch_state.sqlite3` snapshot database:

```powershell
scripts\import-python-rch-production.ps1 -SourceRoot . -LegacyStore RTH_Store -TargetDir target\production-rch-3
```

If `rch_state.sqlite3` already exists, the script stops unless `-Force` is
provided. `-Force` backs up the existing database to
`rch_state.sqlite3.before-v3-migration.<timestamp>` before replacing it. The
apply run writes `rust-migration-report.json`, `migration-plan.json`,
`migration-manifest.json`, and a compatibility `MANIFEST.txt` in the target
directory.

`scripts\migrate-python-rch.ps1` remains available as a lightweight local
developer wrapper. The Rust converter is available directly as:

```powershell
cargo run -p r3akt-rch-core --bin migrate_python_rch -- --legacy-db RCH_Store\rth_api.sqlite --target-db RTH_Store\rch_state.sqlite3 --legacy-config RCH_Store\config.ini
```

The runtime can also prompt for this import on first start:

```powershell
r3akt-rch-server start --data-dir RTH_Store --prompt-python-import
```

With `--prompt-python-import`, Rust only prompts when `rch_state.sqlite3` is
new or empty. It checks the runtime `config.ini`, `[migration]` keys such as
`python_store` or `python_db`, and nearby `RCH_Store`/`RTH_Store` directories.
If no legacy store is found, it asks for the Python store directory or
`rth_api.sqlite` path.

## Release Lines

- Python maintenance: `v2.9.x` from `rch-python`.
- Rust alpha previews: `v3.0.0-alpha.N` or `v3.0.0-preview.N` from
  `rust-next`, with server package artifacts and optional desktop artifacts.
- Rust stable: `v3.0.0` after all Rust server and desktop gates pass.
