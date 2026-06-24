# Rust Agent Dependency Vetting

## Summary

The Rust collector preview added a Cargo workspace under `agent/`. As of `0.1.13`, the Rust agent also has a daemon runtime; see [2026-06-25-rust-daemon-dependency-vetting.md](2026-06-25-rust-daemon-dependency-vetting.md) for the Axum and vendored-dashboard-asset decision.

## Dependency Decisions

### SQLx

- Version pinned: `0.9.0`
- License: `MIT OR Apache-2.0`
- Minimum Rust version: `1.94.0`
- Source checked: crates.io metadata via `cargo info sqlx --verbose`; current SQLx docs through Context7 library `/websites/rs_sqlx`
- Features enabled: `runtime-tokio`, `sqlite`, `migrate`, `json`
- Default features disabled: yes
- Reason selected: SQLx supports SQLite today and PostgreSQL/MySQL later, matching the request to avoid a SQLite-only Rust storage layer.
- Usage choice: runtime `sqlx::query()` calls instead of compile-time macros, so users do not need `DATABASE_URL` during builds.

### sysinfo

- Version pinned: `0.39.5`
- License: `MIT`
- Minimum Rust version: `1.95`
- Source checked: crates.io metadata via `cargo info sysinfo`; current docs through Context7 library `/guillaume_gomez_sysinfo`
- Features enabled: `disk`, `system`
- Default features disabled: yes
- Reason selected: provides maintained cross-platform host metadata, disk refreshes, and process refreshes, which keeps the Rust collector path reusable for future Windows/macOS collectors.
- Usage choice: keep one reusable `System` in the live Linux collector and use targeted refresh kinds instead of rebuilding all collector state for every sample.

### procfs

- Version pinned: `0.18.0`
- License: `MIT OR Apache-2.0`
- Minimum Rust version: `1.70`
- Source checked: crates.io metadata via `cargo info procfs`
- Default features disabled: yes
- Reason selected: provides typed Linux access for CPU ticks, memory, load average, uptime, and pressure stall information without shelling out to host commands.
- Usage choice: use typed crate APIs for Linux-only kernel metrics; the collector has a regression test that forbids external `df`, `ps`, and `uname` command paths.

### Tokio

- Version pinned: `1.52.3`
- License: `MIT`
- Minimum Rust version: `1.71`
- Source checked: crates.io metadata via `cargo info tokio`
- Features enabled originally: `macros`, `rt-multi-thread`
- Features added by the Rust daemon: `net`, `signal`, `sync`, `time`
- Reason selected: SQLx async pool operations need an async runtime; Tokio is the most common SQLx runtime choice.

### Serde

- Version pinned: `1.0.228`
- License: `MIT OR Apache-2.0`
- Minimum Rust version: `1.56`
- Source checked: crates.io metadata via `cargo info serde`
- Features enabled: `derive`
- Reason selected: shared Rust snapshot structs must serialize to the existing dashboard JSON contract.

### serde_json

- Version pinned: `1.0.150`
- License: `MIT OR Apache-2.0`
- Source checked: crates.io search metadata
- Reason selected: the Rust agent CLI emits JSON and the SQLx store persists full snapshot JSON.

### time

- Version pinned: `0.3.51`
- License: `MIT OR Apache-2.0`
- Minimum Rust version: `1.88.0`
- Source checked: crates.io metadata via `cargo info time`
- Features enabled: `formatting`
- Reason selected: the Linux collector needs RFC 3339 UTC timestamps matching the existing snapshot contract.

## Effective Rust Requirement

The Rust workspace requires Rust `1.95.0` or newer for local builds. SQLx itself requires Rust `1.94.0`, but the selected `sysinfo` release requires Rust `1.95`.

## Alternatives Considered

- `rusqlite`: rejected for the Rust store because it is SQLite-specific and the project may add PostgreSQL/MySQL later.
- shelling out to `df`, `ps`, or `uname`: rejected for the Rust collector because the requested direction is lean crate-backed host collection, and external commands add process-spawn overhead and platform-specific parsing drift.
- `inferno` `0.12.6`: considered after implementation as a possible Rust crate addition. It is a strong flamegraph/profiling toolkit, not a host-metrics collector. It can be useful later as an optional developer profiling workflow for TinyTop itself, but it does not belong in the runtime collector or SQLx store path. It is also CDDL-1.0 licensed, so adding it as a linked library would need a deliberate license/provenance review instead of casual inclusion.
- compile-time SQLx macros: deferred because they require build-time database metadata or offline preparation. Runtime queries are simpler for a public preview and still keep SQL isolated.
- adding an HTTP framework in `0.1.12`: deferred at the time. Superseded by `0.1.13`, which adds Axum for the Rust daemon.

## Current Verification

- `cargo test --manifest-path agent/Cargo.toml --workspace`
- `cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- collect --json`
- `cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- collect --json --sqlite sqlite::memory:`
