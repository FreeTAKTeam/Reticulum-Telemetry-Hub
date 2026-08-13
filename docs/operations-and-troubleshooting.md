# Operations and troubleshooting

This guide covers the Rust 3.0 preview runtime. Python 2.9.x maintenance stays
on `rch-python`; do not mix stores or packaging between release lines.

## Runtime contract

- Bind the HTTP API to loopback unless remote access is explicitly required.
- Loopback clients are trusted for Python compatibility. Remote clients require
  the first-run password or `RTH_API_KEY` (`RCH_API_KEY` remains an alias).
- Stored passwords and kill-switch PINs use Argon2id PHC records. A successful
  login transparently upgrades legacy salted SHA-256 preview records.
- Five failures in five minutes cause a five-minute per-client, per-surface
  lockout. HTTP returns `429` with `Retry-After: 300`.
- ZeroMQ command/response sockets are mandatory for LXMF delivery. RPC is
  reserved for control operations.
- TAK is a separate `r3akt-tak-service` process.

## Health checks

Check all three layers instead of treating an HTTP listener as mesh readiness:

```bash
curl http://127.0.0.1:8080/Status
curl http://127.0.0.1:8080/diagnostics/runtime
ss -ltnp
ps -ef | grep -E 'r3akt-rch-server|reticulumd|r3akt-tak-service'
```

For daemon-backed operation, diagnostics must show configured ZeroMQ endpoints,
a configured source destination, a running inbound worker, and the expected
`reticulumd` lifecycle state. Persistence, worker, Reticulum, TAK, and queue
failures are distinct states; an empty list is not proof that a failed
component is healthy.

## Error correlation

Expected client errors retain the Python-compatible `detail` envelope.
Unexpected 500 responses deliberately contain only:

```json
{"detail":"An unexpected server error occurred"}
```

Copy the `X-Request-ID` response header and locate that identifier in server
stderr or the service log. Never expose database paths, parser internals, or
credentials in a client-facing 500.

## Common failures

### `429 Too Many Requests`

Wait for the `Retry-After` interval or authenticate successfully from a client
that is not locked. Check for a stale API key in the UI connection settings,
reverse proxy, or integration. Do not restart the server to bypass lockouts.

### Authentication returns 500

This indicates the credential store could not be read or upgraded, not a bad
password. Check the SQLite path, directory permissions, available disk space,
and `/diagnostics/runtime`. Use `X-Request-ID` to find the underlying error.

### OpenSSL headers are not found on Ubuntu multiarch systems

The release runner now discovers `opensslconf.h` and `libssl.so` below
`/usr/include` and `/usr/lib`. For a direct Cargo command on an affected host:

```bash
OPENSSL_LIB_DIR=/usr/lib/x86_64-linux-gnu \
CFLAGS=-I/usr/include/x86_64-linux-gnu \
cargo test --workspace
```

Adjust the multiarch tuple for the host. Install the distribution OpenSSL
development package if neither file exists.

### Tauri reports an exhausted Linux file-watch limit

On Linux hosts with a low inotify watch limit, the release-readiness gate
removes disposable Rust debug artifacts immediately before Tauri packaging.
This preserves release artifacts while freeing watches that editors may have
attached to debug build output. Direct desktop builds can use
`cargo clean --profile dev` followed by
`cargo clean --manifest-path apps/rch-desktop/src-tauri/Cargo.toml --profile dev`
before `npm --prefix apps/rch-desktop run build`.

### Reticulum delivery remains queued or unavailable

Confirm the exact ZeroMQ URLs passed to both processes, inspect the live socket
listeners, and verify the source destination. Run:

```powershell
./scripts/local-reticulum-live-gate.ps1
```

The gate validates receipt, fanout, event polling, and batched ZeroMQ load. A
configured RPC endpoint alone does not satisfy the delivery contract.

### Desktop sidecar exits

The Tauri shell logs stdout, stderr, termination status, startup timeout, lock
poisoning, and shutdown failures. It no longer embeds a desktop API key. Fix
the reported port, binary, data-directory, or sidecar error and restart the
desktop application; loopback trust remains the local authentication boundary.

When the sibling LXMF checkout contains unrelated in-progress work, prepare a
desktop package from a previously validated `reticulumd` binary without
rebuilding that checkout:

```bash
RCH_RETICULUMD_BINARY=/absolute/path/to/reticulumd \
npm --prefix apps/rch-desktop run build
```

The path must name a trusted LXMF 0.9.9 binary built with
`zmq-pipeline-rpc`. Hosted release jobs do not use this override: they build
the pinned LXMF commit in a clean checkout.

## Migration and backup

Use `scripts/import-python-rch-production.ps1 -DryRun` before importing a
Python store. Keep the generated plan and manifest with the deployment record.
The wrapper backs up an existing Rust database when `-Force` is explicitly
used. Hash upgrades require no manual migration and preserve the stored secret
creation timestamp.

Back up the SQLite database, configuration, Reticulum identity, and attachment
directories together. Never copy a live database without a SQLite-consistent
backup or stopped service.

## Release validation

The committed gate is:

```powershell
./scripts/release-readiness.ps1
```

It covers Rust formatting, clippy, tests, denied-warning documentation, release
server/TAK builds, UI install/lint/type-check/tests/build, desktop sidecar
preparation/build, and HTTP smoke. Dependency audit, MSRV, live Reticulum, and
environment-gated TAK/REM checks are also recorded in the release report.
