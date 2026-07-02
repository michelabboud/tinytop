# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

TinyTop is a standalone local dashboard for live WSL/Linux workstation status. It ships as a single Rust daemon (`tinytop-agent`) that serves a browser dashboard, collects host telemetry, owns SQLite history, and exposes dashboard/history APIs over loopback (`127.0.0.1:4274`). A parallel Bun/TypeScript runtime exists for development and as a fallback collector. Read `ARCHITECTURE.md` for the full topology, data flow, and API contract before making structural changes.

## The two-runtime model (most important thing to understand)

There are **two implementations of the same product** that must stay behaviorally identical:

- **Rust** (`agent/`) — the default, persistent runtime. A Cargo workspace of four crates. This is what `systemd install` and `./tinytop start` use when a binary/Cargo is available.
- **Bun/TypeScript** (`src/`, `legacy/`) — development server + legacy collector, and the fallback when Rust is unavailable (`TINYTOP_RUNTIME=legacy`).

Both produce the same `SystemSnapshot` JSON and serve the same dashboard API. When you change a metric, a snapshot field, or an API route, you almost always must change it **in both runtimes** and update their tests. The Rust snapshot structs (`agent/crates/tinytop-types`) are serialized to match the Bun JSON contract exactly.

### Byte-identical dashboard assets — a hard invariant

`legacy/dashboard/` and `agent/assets/dashboard/` (which Rust embeds) **must stay byte-identical**, including `favicon.svg`. `tests/dashboard-assets.test.ts` enforces this. When editing the dashboard UI (`app.js`, `styles.css`, `index.html`), edit **both trees identically** or the asset-parity test fails.

## Commands

The root `./tinytop` (Bash) is the supported Linux/WSL operator entrypoint; `tinytop.ps1` is the Windows command center. Both work before Bun is installed (for help/bootstrap).

### Build / run / test (development)

```bash
bun test                      # all Bun tests
bun test tests/parsers.test.ts          # a single Bun test file
bun test -t "parses pressure"           # a single test by name

bun run dev                   # Bun dashboard (spawns legacy/bun-collector.ts)
bun run collector             # legacy Bun collector alone

# Rust workspace
cargo test --manifest-path agent/Cargo.toml --workspace
cargo test --manifest-path agent/Cargo.toml -p tinytop-store    # single crate
cargo run  --manifest-path agent/Cargo.toml -p tinytop-agent -- collect --json
cargo run  --manifest-path agent/Cargo.toml -p tinytop-agent -- serve
```

### The verification gates (run before claiming done)

```bash
bun run check:bun     # bun test + Bun server/collector --check self-tests
bun run check:rust    # cargo fmt --check + cargo test --workspace
bun run check         # both of the above — the full gate
```

`check:bun` and `check:rust` are deliberately separate because a Rust-only or Bun-only change should run the matching suite. The `--check` flag on the Bun server/collector is a built-in self-test mode, not a test runner.

### Operator commands (also the integration surface)

```bash
./tinytop help | doctor | status | logs
./tinytop start | stop | restart        # auto-selects Rust, falls back to Bun
./tinytop rust build | install-binary | collect | serve
./tinytop systemd install --rust | start | stop
./tinytop db stats | backup | check
```

`tests/tinytop-script.test.ts` and `tests/tinytop-powershell.test.ts` test these CLI scripts directly — changes to `tinytop` / `tinytop.ps1` need those tests updated.

## Crate / module map

| Path | Role |
| --- | --- |
| `agent/crates/tinytop-types` | Rust snapshot structs; serialize to the dashboard JSON contract |
| `agent/crates/tinytop-collectors` | Platform collectors; Linux/WSL default, feature-gated `macos-collector` / `windows-collector` |
| `agent/crates/tinytop-store` | SQLx SQLite store (samples, 1-min rollups, timeline events, daemon defaults). **All SQL is isolated here** so a future Postgres/MySQL backend doesn't leak into collector code |
| `agent/crates/tinytop-agent` | CLI + daemon: collection timer, history, dashboard serving, legacy-compatible routes |
| `src/parsers.ts` | Pure `/proc`/pressure/load/filesystem parsing (Bun) |
| `src/collector.ts` | Bun live host reads → `SystemSnapshot` |
| `src/history-store.ts` | Bun SQLite store |
| `src/server.ts` | Bun dashboard HTTP server |
| `legacy/bun-collector.ts` | Legacy Bun collector daemon |

The Rust Linux collector uses `procfs` (CPU ticks, memory, load, uptime, PSI), `sysinfo` (disks, processes, identity), and `rustix`'s `statvfs(2)` for per-filesystem inode counts (ADR 0012) — it does **not** shell out to `df`/`ps`/`uname`. It reuses one `sysinfo::System` across samples for CPU/process deltas. Native macOS/Windows collectors are feature-gated and only a first slice; Linux is the reference implementation.

## Conventions specific to this repo

- **Dependencies are pinned to exact versions** in `agent/Cargo.toml` (`=x.y.z`). Keep that style; vetting goes in `docs/reports/`.
- **Architecture decisions are recorded as ADRs** in `docs/adr/` (already 11). Check them before changing architecture; supersede, never edit, an existing ADR. The Rust-single-daemon and embedded-assets decisions (ADR 0005, 0006) explain the runtime model above.
- **History API is additive**: raw snapshots via `/api/history`; rollup-backed long-range points via `/api/history/points`; timeline/gap markers via `/api/history/markers` (ADR 0009). Don't repurpose `/api/history` for rollups.
- The Rust daemon also keeps legacy collector routes (`/snapshot/latest`, `/snapshot/collect`, `/history`, `/version`) on the same port for API continuity.
- Default DB: `~/.local/share/tinytop/history.sqlite`. Network exposure is loopback-only by default.

## Docs to keep current (per the global workflow rules)

`CHANGELOG.md`, `PROGRESS.md`, `README.md`/`ARCHITECTURE.md`, and `VERSION` (single source of truth, currently `0.1.34`) are all live and expected to be updated per task. New ADRs for architectural decisions.
