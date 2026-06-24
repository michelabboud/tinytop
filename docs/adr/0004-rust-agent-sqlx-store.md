# 0004 - Additive Rust Agent With SQLx Store

## Status

Accepted

## Context

TinyTop started as a Bun dashboard with a Bun collector/writer process. The next platform goal is native collectors for Linux/WSL, Windows, and macOS with shared code where possible and platform-specific code where necessary. The existing collector already works for Linux/WSL and must not be removed or broken while the Rust path is developed.

Storage is SQLite today, but the project may use another database later. The Rust path should avoid baking direct SQLite-only APIs into collector code.

## Decision

Add a Rust workspace under `agent/` as an additive preview path:

- `tinytop-types` owns Rust structs for the existing `SystemSnapshot` JSON contract.
- `tinytop-collectors` owns platform collectors; the first implemented backend is Linux/WSL.
- `tinytop-store` owns Rust-side history persistence and uses SQLx with SQLite today.
- `tinytop-agent` is a small CLI for collecting JSON and optionally writing a sample through SQLx.

Keep the existing Bun collector and writer intact. Do not wire Rust into the default `./tinytop start` runtime until the Rust writer HTTP API reaches compatibility with the current Bun writer API.

Use SQLx instead of a SQLite-specific Rust library. Pin SQLx to the latest stable release available during implementation and isolate SQL in `tinytop-store` so future PostgreSQL/MySQL work has one boundary to change.

Use crate-backed host collection for the Rust Linux backend. The collector uses `procfs` for Linux kernel metrics and `sysinfo` for disk/process/system metadata. It does not execute external host commands for metrics collection.

## Alternatives Rejected

### Replace The Bun Collector Immediately

This would risk breaking the working dashboard while the Rust path is still gaining parity. The safer path is additive: prove the Rust collector and SQLx store first, then add a runtime switch later.

### Use rusqlite For The Rust Store

`rusqlite` is mature for SQLite, but the user explicitly wants room for future database backends. SQLx supports SQLite, PostgreSQL, and MySQL through one async toolkit, making it a better fit for the Rust agent boundary.

### Put Platform Code Directly In The Agent Binary

That would make the binary grow into a mixed CLI, storage, and OS collection module. Separate crates keep shared types, platform collection, storage, and command handling independently testable.

## Consequences

- The repo now has an optional Rust toolchain requirement for the preview path: Rust `1.95.0` or newer, driven by `sysinfo` `0.39.5`.
- The default Bun runtime remains unchanged.
- Future work can add Windows/macOS collectors without changing the dashboard JSON contract.
- Future work can add a Rust writer HTTP API that mirrors `/health`, `/snapshot/latest`, `/snapshot/collect`, and `/history`.
