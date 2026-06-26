# Dashboard Operator V2 And Platform Collector Roadmap Report

Date: 2026-06-26
Version: 0.1.27

## Summary

TinyTop v0.1.27 implements the approved dashboard operator V2 slice and starts the native platform collector roadmap.

## Delivered

- Added an operator detail drawer for the top Healthy/Warning/Critical/Stale strip with metric value, threshold, sample age, trend, and recent-change context.
- Added additive Rust `/api/history/points` and `/api/history/markers` endpoints.
- Added rollup-backed dashboard ranges for 6h, 24h, 7d, and 30d.
- Added daemon-start, settings-change, and computed coverage-gap timeline markers.
- Added SQLite-backed DB budget settings and coverage fields for target database bytes, budget percentage, and rollup oldest/newest timestamps.
- Polished Settings with validation, unsaved-change warning, reset/defaults actions, threshold presets, and an effective-settings readout.
- Upgraded process details with redacted copy-safe command text, optional parent PID/start time, RSS, and per-PID CPU/RAM trend.
- Added feature-gated macOS and Windows collector starter modules while keeping Linux/WSL as the default reference collector.
- Kept `agent/assets/dashboard/` and `legacy/dashboard/` byte-identical.
- Cleaned the stale `HANDOFF.md` PID note and refreshed the handoff with the live v0.1.27 daemon.

## Verification

- `bun test`: `75 pass`, `0 fail`, `383 expect() calls`.
- `cargo test --manifest-path agent/Cargo.toml --workspace`: passed.
- `./tinytop check`: passed; Bun tests, legacy server check, legacy collector check, Rust fmt, Rust workspace tests, and browser bundle build all completed.
- `node --check legacy/dashboard/app.js` and `node --check agent/assets/dashboard/app.js`: clean.
- `diff -qr agent/assets/dashboard legacy/dashboard`: no differences.
- `git diff --check`: clean.
- `cargo clippy --manifest-path agent/Cargo.toml --workspace --all-targets -- -D warnings`: clean.
- `bun audit`: no vulnerabilities found.
- `cargo audit --file agent/Cargo.lock`: no vulnerabilities reported.
- `cargo check --manifest-path agent/Cargo.toml -p tinytop-collectors --target x86_64-pc-windows-gnu --no-default-features --features windows-collector`: passed.
- `cargo check --manifest-path agent/Cargo.toml -p tinytop-collectors --target x86_64-apple-darwin --no-default-features --features macos-collector`: blocked by missing local Rust target `x86_64-apple-darwin`.
- `./tinytop rust build`: built `agent/target/release/tinytop-agent`.
- Release binary SHA-256: `9984f67d68368e2613cf4b773c325f920cbde5984cdfcc2daf64164c1d0ea362`.

## Live Smoke

- Restarted with `setsid ./tinytop start > /tmp/tinytop-v0.1.27.log 2>&1 &`.
- `./tinytop status`: reports `rust collector-dashboard-daemon v0.1.27 (embedded dashboard)`.
- Live PID at report time: `1783062`.
- `/health`: `ok`.
- `/api/version`: Rust `0.1.27`, component `collector-dashboard-daemon`, dashboard `embedded`.
- `/api/settings`: includes `targetDatabaseBytes`.
- `/api/history/coverage`: includes DB budget fields and rollup coverage timestamps.
- `/api/history/points?window=24h&mode=auto&limit=5`: returned chart points.
- `/api/history/markers?since_ms=0&limit=10`: returned a v0.1.27 `daemonStart` marker.
- `/`: contains `operator-detail-dialog`, `process-detail-dialog`, and `settings-dialog`.
- `/app.js`: contains `openOperatorDetailDrawer`, `targetDatabaseBytes`, `fetchHistoryMarkers`, and `processTrendForPid`.

## Notes

- macOS and Windows collectors are starter modules behind feature gates. Linux/WSL remains the verified reference runtime.
- The local macOS cross-check needs `rustup target add x86_64-apple-darwin` before it can compile on this machine.
