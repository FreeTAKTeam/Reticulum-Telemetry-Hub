# v3.0.0-preview.10 stabilization report

## Scope

This release cut validates RCH against the immutable stable LXMF-rs `v0.9.9`
tag and includes the RCH changes merged after preview.9. The Python 2.9.x
branch and shared-UI ownership contract are unchanged.

## Release delta

- Stable LXMF-rs 0.9.9 dependency, CI, package, and desktop-sidecar baseline.
- Independent Reticulum service identities and restored inbound field commands.
- Team-scoped REM routing and shared mesh delivery policy.
- Expiring ZeroMQ control requests with backpressure diagnostics.
- Python-compatible topic subscription persistence corrections.
- Client-type announce filtering and repaired LXMF compatibility CI paths.

## Validation evidence

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed against LXMF-rs 0.9.9 |
| strict workspace clippy | Passed with denied warnings |
| workspace and release-critical crate tests | Workspace and separate server, core, transport, and TAK suites passed; 337 server tests passed and only the environment-gated load test was ignored |
| committed `-ServerOnlyAlpha` release gate | Passed, including release build, documentation, and ZeroMQ-configured HTTP smoke |
| UI dependency audit | Passed with zero production vulnerabilities after lockfile updates for `linkify-it`, `nanoid`, and `postcss` |
| UI install, lint, type-check, tests, and production build | Passed; 35 files and 113 tests passed |
| LXMF-rs 0.9.9 daemon build | Passed from the exact tag with `zmq-pipeline-rpc`; Windows binary SHA-256 `B1D9BB96EF5996A39FBF14091239B84191E4CF66B24A1EDE7F654446D3E7BB42` |
| Windows desktop sidecar preparation and NSIS bundle | Passed with the exact LXMF-rs 0.9.9 daemon; emitted `RCH Desktop_3.0.0-preview.10_x64-setup.exe` |
| published package workflow and asset audit | Pending publication |

## External limitations

External TAK infrastructure and REM phone/deck hardware are environment-gated.
They are not implied by deterministic local or hosted package validation.

## Release identity

- Version/tag: `v3.0.0-preview.10`
- Source branch: `main`
- RCH release commit: pending
- LXMF-rs baseline: `v0.9.9`, commit
  `51fd3beebdace78d6c7f38748c6bcfe452032559`
- Server archives: Windows x64, macOS x64, macOS arm64, Linux AMD64, Linux
  Raspberry Pi 64
- Desktop packages: Windows x64 NSIS, Linux x64 AppImage
