# RCH Rust Transition

`rust-next` is the staged Rust edition of Reticulum Community Hub. The Python
implementation at release `2.9.6` is preserved on `rch-python` for long-term
maintenance.

## Branches

- `rch-python`: Python RCH maintenance branch created from `main` at
  `a520574ecbc243841a1a215b616b6792452de2a1`.
- `rust-next`: Rust R3AKT/RCH workspace imported as the repository root.
- `ui-shared`: canonical Vue web UI source shared by Python maintenance and
  Rust product work.
- `main`: remains the GitHub default branch until Rust release gates pass and
  the default branch is switched deliberately.

## Rust Public Surface

- `r3akt-rch-server`: UI-facing and northbound backend replacement.
- `r3akt-rch-core`: RCH domain model, persistence, mission/checklist/topic
  behavior, and Python-compatible response shaping.
- `r3akt-transport-rns`: LXMF-rs and `reticulumd` adapter boundary.
- `r3akt-tak-connector`: separately deployable TAK connector service boundary.
  `r3akt-tak-service` uses RCH northbound HTTP routes for RCH-to-TAK and
  TAK-to-RCH CoT exchange; TAK socket lifecycle is not part of
  `r3akt-rch-server`.

## Release Gates

Before switching the GitHub default branch to Rust, the branch must pass:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p r3akt-rch-server
cargo test -p r3akt-rch-core
cargo test -p r3akt-transport-rns
cargo test -p r3akt-tak-connector
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo audit --deny warnings
npm --prefix ui run typecheck
```

The live smoke test should start `r3akt-rch-server` with a temporary SQLite
database and validate `/Status`, `/openapi.json`, `/Help`, `/api/v1/app/info`,
and at least one topic/chat/checklist/mission flow.

GitHub PR quality control is handled by
`.github/workflows/rust-pr-quality.yml`. The workflow uses Rust 1.88 for the
normal quality gates and Rust 1.85 for a locked minimum-version workspace
check. It exposes separate required checks for formatting, clippy with
`-D warnings`, MSRV and denied-warning rustdoc, locked workspace tests, UI
lint/type-check/tests/build, release server/TAK-service builds, and
`cargo audit`.

The Python parity plus Rust capability gate is tracked in
`docs/release-contract-matrix.json` and generated with
`scripts/python-rust-parity.ps1`. The matrix separates Python-compatible
contracts from Rust-only required improvements such as REM compatibility,
LXMF-rs/`reticulumd` runtime behavior, install-over-Python migration, Tauri
sidecars, and the standalone TAK service. Rust release readiness requires no
unwaived Python-visible regressions and green evidence for Rust additive
capabilities.

## Packaging

- Initial alpha server gates are built from `r3akt-rch-server` and still use
  `scripts/release-readiness.ps1 -ServerOnlyAlpha`.
- Full GitHub release packaging is handled by
  `.github/workflows/rust-release.yml`. It builds Windows x64, macOS x64,
  macOS arm64, Linux AMD64, and Linux Raspberry Pi 64 server archives
  containing `r3akt-rch-server`, `r3akt-tak-service`, the shared UI bundle,
  service helpers, templates, and checksums. Server archive filenames include
  the resolved release version from the GitHub release tag, pushed tag, or
  manual workflow input, and `release-manifest.json` records that version with
  the Git ref and commit SHA used for the build.
- Desktop packages are built from `apps/rch-desktop` with Tauri. The app loads
  the shared Vue UI and starts `r3akt-rch-server` as a managed sidecar on
  `127.0.0.1:8000`; CI currently emits Windows x64 NSIS and Linux x64 AppImage
  artifacts.
- Python `2.9.x` keeps its existing Electron package on `rch-python`; Electron
  is not part of `rust-next`.

## Current Validation Snapshot

The dated evidence below is historical. The current preview.10 evidence and
exact artifact metadata are maintained in
[`stabilization-v3.0.0-preview.10.md`](stabilization-v3.0.0-preview.10.md).

Validated during the root Rust import and refreshed on 2026-05-11:

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test -p r3akt-rch-core`: passed.
- `cargo test -p r3akt-transport-rns`: passed.
- `cargo test -p r3akt-tak-connector`: passed.
- Local TAK bidirectional TCP loopback validation: passed for
  `r3akt-tak-connector tak_tcp_loopback_validates_bidirectional_cot_workflow`,
  covering outbound keepalive send and inbound parsed CoT receive in one local
  workflow.
- TAK Protocol v1 payload validation: passed for
  `r3akt-tak-connector tak_proto_tcp_sender_pushes_stream_framed_protobuf_payload`,
  proving `TAK_PROTO>0` emits stream-framed protobuf payloads instead of XML
  while `COT_URL` remains the transport selector.
- Standalone TAK service bridge validation: passed for
  `r3akt-tak-service service_bridges_rch_telemetry_and_chat_to_tak_cot_socket`
  and `service_bridges_inbound_tak_cot_to_rch_marker_route`, proving the
  separate process boundary uses RCH northbound HTTP for both CoT directions in
  local loopback coverage.
- `cargo test -p r3akt-rch-server`: passed, including the server library,
  gateway binary, `release_major_functionality`, and SAR HTTP seeder suites;
  the server library now includes 213 tests.
- `cargo test --workspace`: passed across all Rust crates and examples.
- `cargo build --release -p r3akt-rch-server`: passed for the deployable
  server binary.
- `.\scripts\release-readiness.ps1 -PlanOnly`: passed, proving the committed
  release gate runner expands the local and optional live checks without running
  external infrastructure.
- `.\scripts\release-readiness.ps1 -ServerOnlyAlpha -SkipClippy
  -SkipWorkspaceTests`: passed, covering Rust format, the
  `r3akt-rch-server` release binary, and the release HTTP smoke with mandatory
  ZeroMQ SDK endpoints configured.
- `.github/workflows/rust.yml`: now invokes the committed release gate runner
  with `-ServerOnlyAlpha`; Tauri desktop bundling is outside the initial alpha
  release workflow.
- The `Rust workspace` CI run `26696071362` passed on commit
  `8dc69773af38ced251138c007c6f0bdc9543ea02`, proving the committed
  `ServerOnlyAlpha` verifier on Rust 1.85 for the current staging branch.
- The `Build Rust Release Packages` CI run `26696071364` passed on the same
  commit, producing Windows/Linux server archives plus Windows NSIS and Linux
  AppImage desktop artifacts. Downloaded artifacts verified against their
  sidecar SHA-256 files, and both server archive manifests record
  `release_version=rust-next`, `git_ref=rust-next`, and the same commit SHA.
- `npm ci`, `npm audit --audit-level=moderate`, `npm run lint`,
  `npm run test`, and `npm run build` in `ui/`: passed, including zero audited
  UI vulnerabilities after the lockfile refresh and 23 Vitest files / 70 tests.
- `npm run build` in `apps/rch-desktop/`: passed, including shared UI build,
  Tauri sidecar preparation, optimized desktop build, and Windows x64 NSIS
  installer generation for `RCH Desktop_3.0.0-preview.7_x64-setup.exe`.
- Release-binary smoke test with a temporary SQLite database: passed for
  `/Status`, `/openapi.json`, `/Help`, `/api/v1/app/info`, topic creation/list,
  chat creation/list, checklist template creation, mission creation/list, and
  offline checklist creation/list.
- Repeatable local three-node `reticulumd.exe` receipt/fanout/ZeroMQ event
  validation: `scripts/local-reticulum-live-gate.ps1 -IncludeZmqEventPoll
  -DiscoverySettleSeconds 10 -ReceiptPollAttempts 180` passed on 2026-05-30.
  The refreshed gate runs both `r3akt-rch-server` live Reticulum receipt/fanout
  tests through the outbound worker path, then validates `sdk_poll_events_v2`
  over the LXMF-rs ZeroMQ RPC loop.
- Local daemon-involved ZeroMQ load validation is available through
  `scripts/local-reticulum-live-gate.ps1 -ZmqLoadOnly -LoadMessages <count>`.
  It sends chunked `sdk_send_batch_v2` requests from one local daemon to local
  receiver daemons and verifies receiver-side `sdk_poll_events_v2` inbound
  events. The default 500-message run passed on 2026-06-22 with 500 accepted
  and 500 received, balanced across two local receiver daemons; larger
  `-LoadMessages` values remain useful explicit saturation tests for the
  daemon/network delivery layer. A 1,000-message saturation run across four
  receiver daemons also passed on 2026-06-22 with 1,000 accepted and 1,000
  received.
- The daemon-side scalability fix is a persistent bounded delivery scheduler in
  `reticulumd`: accepted messages enter one runtime-backed queue, direct links
  are reused instead of reset per message, per-peer delivery defaults to ordered
  single in-flight work, and burst receipt events are drained without one wakeup
  per receipt. Inspect `delivery_pipeline` in `daemon_status_ex`,
  `sdk_snapshot_v2`, or `sdk_status_v2` while running the load gate.
- Controlled external RMAP Reticulum validation: the same script passed on
  2026-05-11 with `-ExternalConfigPath` pointing at the local RMAP testnet
  config, using three temporary controlled identities connected through public
  TCP hubs and delivering direct receipt plus two-recipient fanout.
- Local two-daemon `reticulumd.exe` receipt validation: passed for
  `r3akt-rch-server live_reticulumd_direct_send_receipt_is_delivered_when_configured`,
  with one local daemon sending through the Rust server to a second daemon's
  announced LXMF delivery destination and settling the direct receipt.
- Local three-daemon `reticulumd.exe` fanout validation: passed for
  `r3akt-rch-server live_reticulumd_topic_fanout_receipts_are_delivered_when_configured`,
  with one source daemon sending a topic fanout through the Rust server to two
  announced LXMF delivery destinations and settling both direct receipts.
- Live smoke test with a temporary SQLite database: passed for `/openapi.json`,
  `/Help`, `/api/v1/app/info`, authenticated `/Status`, topic creation, chat
  message creation, checklist template creation, mission creation, and offline
  checklist creation.

Release blockers cleared in the latest parity pass:

- `r3akt-rch-server` tests no longer hang in mission fanout coverage; the fake
  Reticulum RPC helper now reads one HTTP request frame instead of waiting for
  connection close.
- Voice-capable LXMF destinations remain chat destinations. Voice is treated as
  an additional feature on top of LXMF chat, not as a separate voice-only
  routing class.
- The transport crate now has ZeroMQ SDK adapter coverage for R3AKT frame
  sends, normal RCH LXMF field payload sends, batched fanout payloads, and
  inbound SDK event conversion. Server runtime southbound traffic uses the
  ZeroMQ SDK pipeline only.
- ZeroMQ is the permanent LXMF data plane for send, ordered batch acceptance,
  delivery status, and event traffic. RPC is an optional administration
  channel and must not be called from HTTP delivery or fanout hot paths.
- Production dependencies use released LXMF `0.9.9`. The matching
  ZMQ-capable `reticulumd` is bundled and described by
  `config/lxmf-runtime-baseline.json`; sibling `main` is checked separately by
  the scheduled compatibility workflow.
- Multi-recipient dispatch now uses `sdk_send_batch_v2` over ZeroMQ so a topic
  fanout enters `reticulumd` as one bounded batch request instead of one RPC
  call per recipient.
- `assignment_asset_link_fanout_uses_python_generic_markdown_shape` proves the
  generic LXMF fanout path for assignment asset links renders Python-style
  markdown with resolved mission, checklist task, and asset names.

## Known Parity Boundaries

The Rust edition preserves the RCH northbound contract as the compatibility
target. Remaining external validation should be tracked explicitly, especially
broader real-network Reticulum validation outside the local multi-daemon
harness. After the target TAK server was restarted on 2026-05-11, the configured
`tcp://137.184.101.250:8087` profile passed keepalive, reconnect, and
bidirectional inbound relay validation through the standalone TAK service.

Use `.\scripts\release-readiness.ps1 -ServerOnlyAlpha` for the local
server-only alpha release gate set. Use
`.\scripts\local-reticulum-live-gate.ps1 -IncludeZmqEventPoll` for the
repeatable local Reticulum direct receipt, fanout, and ZeroMQ event-poll gate.
Add `-LiveTak` and `-LiveReticulum` only when the required TAK and Reticulum
environment variables point at reachable
infrastructure. `-LiveTak` is a
send-and-receive gate and requires both `R3AKT_TAK_LIVE_COT_URL` and
`R3AKT_TAK_LIVE_INBOUND_COT_URL`. For clear TCP TAK targets without
`R3AKT_TAK_LIVE_INBOUND_EXPECT_UID`, the inbound gate identifies a receiver,
publishes a probe CoT through the outbound URL, and requires a relayed inbound
CoT response.

See `docs/release-readiness-audit.md` for the prompt-to-artifact release
checklist and the current release decision.
