# History Retention Documentation Sweep

Date: 2026-06-26

## Purpose

Clarify how long TinyTop keeps history today.

## Current Behavior

- SQLite raw samples are stored in `metric_samples`.
- Automatic retention is not implemented yet.
- Raw samples remain in SQLite until the user archives or resets the database.
- `/api/history` and `/history` query parameters limit returned rows only.
- The dashboard hydrates a recent 120-sample window and keeps a browser-local 120-sample rolling buffer; this is not a database retention policy.
- The dashboard `Clear` action clears only the current browser tab's loaded samples.

## Documents Updated

- `README.md`
- `GUIDE.md`
- `INSTALL.md`
- `ARCHITECTURE.md`
- `PROGRESS.md`
- `CHANGELOG.md`
- `HANDOFF.md`
- `docs/guides/API.md`
- `docs/guides/OPERATIONS.md`
- `docs/sqlite-history-architecture.md`

## Verification

- `./tinytop check`
  - Bun tests: `43 pass`, `0 fail`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - Rust workspace tests: passed
  - Browser bundle: built `legacy/dashboard/app.js`
- `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`: clean
- `git diff --check`: clean

## Follow-Up

Implement raw-history retention as a daemon-owned SQLite maintenance path, then update this report with the final policy. The standing recommendation remains a configurable raw-history window, defaulting to 24 to 72 hours, plus one-minute rollups for longer ranges.
