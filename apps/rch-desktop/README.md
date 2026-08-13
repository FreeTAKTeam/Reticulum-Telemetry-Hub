# RCH Desktop for Rust

This is the Tauri desktop shell for the Rust RCH product line. It reuses the
shared Vue UI from `ui/` and launches the bundled ZMQ-capable `reticulumd`
before `r3akt-rch-server`. The desktop bundle also carries the `r3akt-tak-service` sidecar for
release package parity; the shell does not start it automatically yet.

## Build

```powershell
npm --prefix ui ci
npm --prefix apps/rch-desktop install
npm --prefix apps/rch-desktop run build
```

The sidecar preparation step builds LXMF `0.9.9` `reticulumd`,
`r3akt-rch-server`, and `r3akt-tak-service`, then copies them into `src-tauri/binaries/` with the
host target-triple suffix required by Tauri.

## Runtime

The desktop app starts `reticulumd` on the local ZeroMQ command endpoint, then
starts the backend on `127.0.0.1:8000`. It stops the server before the daemon and stores Rust runtime
state under the platform application data directory. The UI keeps using the
existing RCH base URL configuration and falls back to local HTTP when loaded
from the desktop shell.
