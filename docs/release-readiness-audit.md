# Rust Release Readiness Audit

This audit maps the Rust transition goal to concrete artifacts and evidence.
It is intentionally stricter than a green CI badge: the initial Rust alpha is
not release-ready until every required local, CI, server-package, ZeroMQ, REM,
and Reticulum gate is either passed or recorded as an explicit alpha risk.

Audit date: 2026-06-22; preview.10 supplement: 2026-08-13

The table retains historical evidence. Current preview.10 findings, validation,
limitations, and artifact identity are tracked in
[`stabilization-v3.0.0-preview.10.md`](stabilization-v3.0.0-preview.10.md).

## Objective

Bring the Rust RCH server package to an initial alpha release state. This is a
server-package-only milestone, not the full stable 3.0 release.

## Success Criteria

| Requirement | Evidence artifact | Current evidence | Status |
| --- | --- | --- | --- |
| Preserve Python 2.9.x maintenance line separately from Rust work | `docs/rust-transition.md`, `rch-python` branch | `docs/rust-transition.md` names `rch-python` as the Python 2.9.6 maintenance branch and `rust-next` as the Rust edition | Passed |
| Rust server owns the northbound/UI-facing RCH contract | `crates/r3akt-rch-server/src/lib.rs`, OpenAPI route tests | `openapi_covers_python_northbound_route_inventory`, UI endpoint inventory tests, and HTTP/WebSocket route tests are in the server test suite | Passed locally and in CI gate |
| Python parity and Rust-required improvements are classified separately | `docs/release-contract-matrix.json`, `scripts/python-rust-parity.ps1`, `docs/release-parity-report.md` | Matrix labels `must-match-python`, `rust-additive-required`, and `intentional-difference`; generated report records baseline commits and separates Python-visible regressions from Rust-only release requirements such as REM compatibility and install-over-Python migration. A live two-server parity probe passed on 2026-06-22 against Rust `http://127.0.0.1:18180` and Python `http://127.0.0.1:18181`: Rust OpenAPI had no failed route probes for the matrix scope, and Python OpenAPI had no failed route probes for must-match route contracts. Rust-only control, streaming, and `/openapi.yaml` routes are now classified as Rust-additive HTTP routes instead of Python-required routes. | Passed locally |
| Rust core preserves Python-shaped domain behavior | `crates/r3akt-rch-core/src/lib.rs` tests | Core tests cover topics, clients, missions, teams, members, assets, skills, assignments, checklists, EAM, delivery policy, authorization, SQLite migration, and Python-shaped responses | Passed locally |
| TAK is a separate service, not embedded in `r3akt-rch-server` | `crates/r3akt-tak-connector/src/bin/r3akt-tak-service.rs`, packaging files | `r3akt-tak-service` bridges RCH telemetry/chat to TAK and TAK CoT to RCH marker routes through northbound HTTP; `r3akt-rch-server` does not own TAK socket lifecycle | Passed locally |
| Voice is an additional LXMF chat capability, not a voice-only destination class | `crates/r3akt-rch-server/src/lib.rs` tests | Voice-capable peer routing tests keep voice-capable identities in chat/fanout routing | Passed locally |
| Rust MSRV gate is valid for Rust 1.85 | `.github/workflows/rust-pr-quality.yml`, local `cargo +1.85.0` checks | PR quality control has a dedicated Rust 1.85 locked workspace check plus denied-warning rustdoc; Rust 1.88 remains the normal release toolchain. Historical `Rust workspace` run `26696071362` passed the committed gate on commit `8dc69773af38ced251138c007c6f0bdc9543ea02`. | Dedicated workflow defined; preview.9 run pending |
| PR Rust quality control is explicit and branch-protectable | `.github/workflows/rust-pr-quality.yml` | Pull requests into `rust-next` or `main` get separate checks for `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace -- --test-threads=1`, release builds for `r3akt-rch-server` and `r3akt-tak-service`, and `cargo audit --deny warnings`. The workflow is branch-protectable; current push evidence is provided by the stricter committed alpha gate in `Rust workspace` run `26696071362`. | Workflow defined; push gate passed |
| Release CI gate runs the committed alpha verifier | `.github/workflows/rust.yml`, `scripts/release-readiness.ps1` | CI invokes `scripts/release-readiness.ps1 -ServerOnlyAlpha`, which runs Rust format, clippy, workspace tests, the server release build, and the ZeroMQ-configured release HTTP smoke. `Rust workspace` run `26696071362` passed on commit `8dc69773af38ced251138c007c6f0bdc9543ea02`. | Passed in CI |
| Server release binary builds and smokes with ZeroMQ configured | `scripts/release-readiness.ps1` | Alpha runner builds `r3akt-rch-server`, starts it with `--lxmf-zmq-command`, `--lxmf-zmq-response`, and `--reticulumd-source`, and validates `/Status`, `/openapi.json`, `/Help`, `/api/v1/app/info`, and `/diagnostics/runtime` against a temporary SQLite DB. This passed locally through `.\scripts\release-readiness.ps1 -ServerOnlyAlpha` on 2026-05-28 against LXMF-rs `origin/main` `cbccf0f`. The Rust workspace baseline now targets LXMF-rs `v0.9.5` commit `7cafc5b4be21ff4f777d0f2300cfb79e5d0da23c`; on 2026-07-18, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the focused release-critical crate tests passed against that baseline. Earlier release-build and local workflow-YAML checks passed before publishing `v3.0.0-preview.2` against the prior `v0.5.0` baseline. | Passed locally |
| Full Rust release packaging mirrors Python release artifact flow | `.github/workflows/rust-release.yml`, `scripts/build-rust-release-package.ps1`, `packaging/`, `apps/rch-desktop/` | Rust release packaging supports manual workflow artifacts and published-release asset attachment. `Build Rust Release Packages` run `26696071364` passed on commit `8dc69773af38ced251138c007c6f0bdc9543ea02`, producing Linux and Windows server archives plus Linux AppImage and Windows NSIS desktop artifacts. The workflow now also defines macOS x64, macOS arm64, and Linux Raspberry Pi 64 server archive jobs for the next release/manual run. | Passed in CI for prior matrix; expanded matrix needs next run |
| Release packages carry traceable version metadata | `.github/workflows/rust-release.yml`, `scripts/build-rust-release-package.ps1`, `release-manifest.json` inside the server archive | Server archive names include the resolved release version from the GitHub release tag, pushed tag, manual workflow input, or branch ref. The green `rust-next` packaging run produced `rch-rust-full-windows-x64-rust-next.zip` and `rch-rust-full-linux-x64-rust-next.tar.gz`; both downloaded manifests record `release_version=rust-next`, `git_ref=rust-next`, `git_sha=8dc69773af38ced251138c007c6f0bdc9543ea02`, and inclusion of server, TAK service, and UI payloads. The expanded matrix keeps the same naming and manifest path for Windows x64, macOS x64, macOS arm64, Linux AMD64, and Linux Raspberry Pi 64 packages. | Passed in CI for prior matrix; expanded matrix needs next run |
| ZeroMQ is the mandatory server-package southbound command transport | `crates/r3akt-transport-rns/src/lib.rs`, `crates/r3akt-rch-server/src/lib.rs`, live REM validation | RCH runs outbound REM fanout through the LXMF-rs ZeroMQ SDK envelope protocol when `--lxmf-zmq-command`, `--lxmf-zmq-response`, and `--reticulumd-source` are configured. Optimized REM command channels use the ZeroMQ path without reintroducing RPC compatibility. The committed alpha verifier passed in CI on 2026-05-30, and the later live REM bridge fix maps `auto` to daemon params `method=direct` plus `try_propagation_on_fail=true` so current `reticulumd` receives the intended direct-with-propagation-fallback instruction. | Passed in CI and live validation |
| Live REM reduced-signature fanout works against connected phones | `docs/rem-southbound-interface.md`, `docs/release-live-stress-report.md` | Earlier 2026-05-28 runs proved the reduced field `9` command shape across checklist, EAM, log, marker, and telemetry payloads but did not prove phone receipt. The later live retest documented in `docs/release-live-stress-report.md` created online checklist `RCH TCP aligned REM checklist 162257`; `/Events` showed `recipient_count=2`, `sent=2`, and `command_type=checklist.create.online`; reticulumd traces showed both Noemi and Pixel messages reaching `sent: link resource`; Pixel 7 and Pixel 8a screenshots showed the checklist at the top of REM Checklists. | Passed live for two-phone checklist fanout |
| Live Reticulum direct receipt, fanout, ZeroMQ event polling, and local ZeroMQ load work outside local unit mocks | `crates/r3akt-rch-server/src/lib.rs`, `scripts/local-reticulum-live-gate.ps1`, live env-gated tests | `scripts/local-reticulum-live-gate.ps1 -IncludeZmqEventPoll -IncludeZmqLoad -DiscoverySettleSeconds 10 -ReceiptPollAttempts 180` passed locally on 2026-06-22. The refreshed gate starts three temporary `reticulumd.exe` nodes, verifies direct receipt and two-recipient fanout through the outbound worker path, polls `sdk_poll_events_v2` over the LXMF-rs ZeroMQ RPC loop, then runs a 500-message chunked `sdk_send_batch_v2` load check with 500 accepted and 500 received across two receiver daemons. A heavier local stress run also passed on 2026-06-22 with `-ZmqLoadOnly -NodeCount 5 -LoadReceiverCount 4 -LoadMessages 1000 -LoadSenderClients 4 -DiscoverySettleSeconds 10 -LoadPollAttempts 480 -LoadPollDelayMs 250`, delivering 1000 accepted and 1000 received messages evenly across four receiver daemons. A live-device RC stress pass on 2026-06-22 found two USB Columba phones (`Pixel_7`, `SM_G950W`) plus announced `silkedeck` and `corvodeck` peers; `raphydeck` was not present in the current 5,000-row announce cache. The first propagated canary exposed a daemon/RPC hang from an unbounded bridge connect; after bounding bridge connects, one canary and a 25-message bounded run to the phone/deck targets posted 25/25 successfully, with RCH diagnostics reporting 26 accepted propagated dispatches, zero dispatch failures, zero stuck futures, and zero send timeouts. The same script still supports `-ExternalConfigPath` for controlled RMAP testnet validation, but that external profile has not been rerun after this refresh. | Passed locally and live-device dispatch; external RMAP refresh still useful |
| Live TAK target profile works with the standalone service | `crates/r3akt-tak-connector/src/lib.rs`, `r3akt-tak-service` | Local TCP/UDP/TLS loopback and service bridge tests pass; `.\scripts\release-readiness.ps1 -LiveTak` requires both outbound `R3AKT_TAK_LIVE_COT_URL` and inbound `R3AKT_TAK_LIVE_INBOUND_COT_URL`; after the TAK server was restarted on 2026-05-11, `tcp://137.184.101.250:8087` passed live keepalive, reconnect, and bidirectional inbound relay validation | Passed against target profile |

## Required Release Gates

These gates must pass before declaring the Rust edition release-ready:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -- --test-threads=1
cargo test -p r3akt-rch-server
cargo test -p r3akt-rch-core
cargo test -p r3akt-transport-rns
cargo test -p r3akt-tak-connector
cargo +1.85.0 check --workspace --all-targets --locked
cargo audit --deny warnings
$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --workspace --no-deps
npm --prefix ui ci
npm --prefix ui run lint
npm --prefix ui run typecheck
npm --prefix ui run test
npm --prefix ui run build
cargo build --release -p r3akt-rch-server
.\scripts\release-readiness.ps1 -ServerOnlyAlpha
.\scripts\local-reticulum-live-gate.ps1 -IncludeZmqEventPoll -IncludeZmqLoad -DiscoverySettleSeconds 10 -ReceiptPollAttempts 180
.\scripts\local-reticulum-live-gate.ps1 -ZmqLoadOnly -NodeCount 5 -LoadReceiverCount 4 -LoadMessages 1000 -LoadSenderClients 4 -DiscoverySettleSeconds 10 -LoadPollAttempts 480 -LoadPollDelayMs 250
.\scripts\local-reticulum-live-gate.ps1 -ExternalConfigPath <reticulumd-rmap-testnet.toml> -TimeoutSeconds 180 -DiscoverySettleSeconds 45 -ReceiptPollAttempts 240 -ReceiptPollDelayMs 1000
.\scripts\python-rust-parity.ps1 -RustBaseUrl <rust-url> -PythonBaseUrl <python-url>
```

For a release candidate, also run the external live gates when target
infrastructure is available:

```powershell
.\scripts\release-readiness.ps1 -LiveTak -LiveReticulum
```

`-LiveTak` requires reachable TAK infrastructure for both directions through
`R3AKT_TAK_LIVE_COT_URL` and `R3AKT_TAK_LIVE_INBOUND_COT_URL`. For clear TCP
targets without `R3AKT_TAK_LIVE_INBOUND_EXPECT_UID`, the inbound gate performs
an active bidirectional relay probe instead of depending on unsolicited CoT.

`-LiveReticulum` requires reachable Reticulum/LXMF peers through
`R3AKT_RETICULUMD_RPC_ENDPOINT`, `R3AKT_RETICULUMD_SOURCE`,
`R3AKT_RETICULUMD_RECEIPT_DESTINATION`, and
`R3AKT_RETICULUMD_FANOUT_DESTINATIONS`.

## Current Decision

The Rust edition is not ready for the stable 3.0 release yet.

The initial alpha release scope is the Rust server/package line, with desktop
bundles treated as preview artifacts. The server alpha gates are now green on
commit `8dc69773af38ced251138c007c6f0bdc9543ea02`: the committed Rust 1.85
`ServerOnlyAlpha` verifier passed in CI, full release packaging passed in CI,
downloaded server and desktop artifacts passed checksum verification, and live
two-phone REM checklist fanout is documented as delivered and visible in both
REM phone UIs.

No current local server-alpha blocker is recorded in this audit. Remaining work
before stable `v3.0.0`:

- Run the external RMAP Reticulum profile again after the latest ZeroMQ event
  polling refresh, if public testnet evidence is required for the release notes.
- Publish a tagged preview or alpha release so the packaging workflow embeds a
  semantic release tag such as `v3.0.0-preview.4` instead of the staging branch
  label `rust-next`.
- Continue broad parity hardening for less common Python edge cases listed in
  `README.md` and `docs/release-contract-matrix.json`.
