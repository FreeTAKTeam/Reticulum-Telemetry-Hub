# RCH Rust v3.0.0-preview.10 release notes

This prerelease moves the Rust 3.0 product line to the stable LXMF-rs 0.9.9
baseline and includes the RCH improvements merged since preview.9. Python
2.9.x remains the maintenance line on `rch-python`.

## Highlights

- Pins all release, CI, server, transport, and desktop package paths to the
  immutable LXMF-rs `v0.9.9` release commit.
- Restores inbound LXMF field commands and registers RCH as an independent
  Reticulum service with distinct service identities.
- Adds team-scoped REM directory routing and shared mesh delivery policy.
- Prevents stale ZeroMQ control requests from accumulating after callers time
  out, with operator-visible backpressure and expiry diagnostics.
- Corrects topic subscription restoration and persistence while preserving
  Python-compatible historical topicless rows.
- Adds client-type filtering to the announces UI and repairs the hosted
  compatibility workflow against the sibling LXMF checkout layout.
- Refreshes the locked UI dependency graph so the production dependency audit
  is clean before packaging.

## LXMF-rs compatibility

RCH preview.10 builds against stable LXMF-rs `v0.9.9`, commit
`51fd3beebdace78d6c7f38748c6bcfe452032559`. The packaged `reticulumd` is
built from that same immutable commit with `zmq-pipeline-rpc` enabled.

LXMF-rs 0.9.9 targets Python Reticulum 1.4.2 software parity and ships its
release interoperability, performance, package, checksum, SBOM, provenance,
and OCI evidence separately. Hardware, public-network, and third-party-client
validation remain separate evidence tracks.

## Validation and artifacts

See the [preview.10 stabilization report](stabilization-v3.0.0-preview.10.md)
for the release-cut evidence. Server archives embed the RCH tag and commit,
the exact LXMF-rs commit, and SHA-256 metadata.

## Known boundaries

- This is preview software, not stable `v3.0.0`.
- External TAK and REM phone/deck checks require available infrastructure and
  are disclosed separately when unavailable.
- Operators should verify `/Status`, `/diagnostics/runtime`, processes,
  listeners, and live ZeroMQ attachment for each deployment.
