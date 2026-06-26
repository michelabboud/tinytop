# Dashboard Operator Console And Retention Report

Date: 2026-06-26
Version: 0.1.25

## Summary

This slice turns the embedded Rust dashboard into a more useful operator console and follows through on the history backend work that had only been planned before.

Implemented:

- Operator status strip with Healthy, Warning, Critical, and Stale states.
- Timeline rail replacing the native range input.
- History coverage display backed by Rust `GET /api/history/coverage`.
- Rust raw-history pruning by `retentionHours`.
- Rust one-minute rollup buckets pruned by `rollupRetentionDays`.
- Expanded warning/critical thresholds for CPU, RAM, disk, load, and PSI.
- Enabled-section settings that hide/show Overview, History, Filesystem, Pressure, and Processes.
- Browser-local visible series, process table state, filesystem toggle, and last section preferences.
- Process search, sort, density controls, and process details dialog.
- Root filesystem card, pseudo/system mount toggle, and threshold-colored filesystem/pressure states.

The dashboard assets remain byte-identical between:

- `legacy/dashboard/`
- `agent/assets/dashboard/`

## Storage Split

SQLite daemon defaults:

- `defaultTheme`
- `defaultGraphMode`
- `pollIntervalMs`
- `defaultHistoryWindow`
- `retentionHours`
- `rollupRetentionDays`
- `topProcessCount`
- `redactionDefault`
- `thresholds`
- `enabledSections`

Browser-local preferences:

- active theme
- active graph mode
- selected history window
- visible history series
- process filter, sort, and density
- filesystem system-mount toggle
- last section

## Verification

Focused checks run during implementation:

```bash
bun test tests/dashboard-settings.test.ts tests/dashboard-operator-alert.test.ts tests/dashboard-timeline.test.ts tests/dashboard-process-filesystem.test.ts
bun test tests/dashboard-assets.test.ts tests/dashboard-overview.test.ts tests/webui-dialogs.test.ts
bun test tests/server.test.ts
node --check legacy/dashboard/app.js
node --check agent/assets/dashboard/app.js
cargo test --manifest-path agent/Cargo.toml -p tinytop-store sqlite_store_persists_load_and_critical_thresholds
cargo test --manifest-path agent/Cargo.toml -p tinytop-store sqlite_store_prunes_raw_history_by_cutoff
cargo test --manifest-path agent/Cargo.toml -p tinytop-store sqlite_store_tracks_one_minute_rollups_and_coverage
cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_exposes_history_coverage_api
cargo test --manifest-path agent/Cargo.toml -p tinytop-store
```

Final checks also completed:

```bash
./tinytop check
git diff --check
diff -qr agent/assets/dashboard legacy/dashboard
bun build legacy/dashboard/app.js --target=browser --outdir=/tmp/tinytop-dashboard-operator-console-check
./tinytop rust build
bun audit
cargo audit --file agent/Cargo.lock
```

Live smoke served `rust collector-dashboard-daemon v0.1.25 (embedded dashboard)` on `http://127.0.0.1:4274`, verified `/api/history/coverage`, and checked desktop/mobile rendering through Playwright MCP.

## Follow-Up

- Use `metric_rollups_1m` for longer history query ranges, not only coverage reporting.
- Add configurable history coverage and database size goals.
- Add a collector/daemon health detail drawer for API degradation.
- Add native macOS and Windows collectors when TinyTop moves beyond Linux/WSL.
