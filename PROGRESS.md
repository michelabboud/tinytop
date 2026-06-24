# Progress

## Current Version

- Version: `0.1.9`
- Date: 2026-06-24
- Status: Local dashboard with SQLite-backed recent history, Telecode-style install wizard, Bash command center, systemd user services, and SQLite operations.

## Completed

### 0.1.0 - Initial Dashboard

- [x] Created standalone project folder outside `the-operator`.
- [x] Selected Bun as runtime and HTTP server.
- [x] Implemented read-only collectors for `/proc`, `df`, `ps`, `uname`, and OS release data.
- [x] Implemented WSL versus real Linux runtime detection.
- [x] Built the first dashboard UI with gauges, charts, stat tiles, filesystem bars, pressure panels, and process rows.
- [x] Claimed `127.0.0.1:4274`.
- [x] Added initial Bun tests and rendered browser QA.

### 0.1.1 - Themes And Graph Modes

- [x] Added Midnight, Matrix, Aurora, Solar, and Ember themes.
- [x] Added selectable history graph modes.
- [x] Persisted theme and graph preferences in browser-local storage.

### 0.1.2 - Timeline Scrubber

- [x] Moved Live History directly under the main gauges.
- [x] Added history scrubbing for gauge values.
- [x] Added a return-to-live control.
- [x] Kept selected sample datetime context visible.

### 0.1.3 - Graph Nav And Context

- [x] Restored Bar graph mode in Live History.
- [x] Moved graph type controls into the Live History top nav.
- [x] Relocated the timeline below the chart.
- [x] Added numeric context to graph axes, timeline values, and heatmap lanes.

### 0.1.4 - ECharts Migration

- [x] Replaced custom Live History chart rendering with Apache ECharts.
- [x] Added line, stacked area, stacked bar, heatmap, and treemap modes.
- [x] Served the ECharts browser bundle from a local dependency route.
- [x] Verified chart selection, desktop layout, and mobile layout.

### 0.1.5 - Responsive Bar Planning

- [x] Added responsive stacked bar visible-window sizing.
- [x] Documented the SQLite history architecture plan and ADR.
- [x] Kept display settings scoped to browser-local storage.

### 0.1.6 - SQLite Recent History

- [x] Implemented the Bun collector/writer process on `127.0.0.1:4276`.
- [x] Added SQLite-backed recent history storage.
- [x] Added `/api/history`.
- [x] Hydrated Live History from persisted samples on dashboard refresh.
- [x] Prevented duplicate bars when polling returns the same latest sample.
- [x] Added storage and history API tests.

### 0.1.7 - Documentation Pass

- [x] Renamed project identity to TinyTop.
- [x] Renamed package, app title, data path, browser storage keys, and fleet port claim.
- [x] Rewrote `README.md`.
- [x] Added `INSTALL.md`.
- [x] Added `GUIDE.md`.
- [x] Rewrote `ARCHITECTURE.md`.
- [x] Rewrote `CHANGELOG.md`.
- [x] Rewrote `PROGRESS.md`.
- [x] Added `docs/guides/API.md`.
- [x] Added `docs/guides/OPERATIONS.md`.
- [x] Updated `docs/sqlite-history-architecture.md`.

### 0.1.8 - Install Wizard Design

- [x] Reviewed the Telecode install wizard pattern.
- [x] Approved TinyTop's two-layer installer direction.
- [x] Documented the zero-dependency `./tinytop` Bash command center.
- [x] Documented the Bash-to-Bun handoff for `./tinytop setup` -> `bun run setup`.
- [x] Documented planned systemd user services for the writer and dashboard.
- [x] Documented planned SQLite stats, check, backup, vacuum, and reset operations.
- [x] Added ADR 0003 for the Bash bootstrap plus Bun wizard decision.

### 0.1.9 - Install Wizard Implementation

- [x] Added root `./tinytop` Bash command center.
- [x] Added Bun install guidance and `./tinytop install-bun`.
- [x] Added `./tinytop setup` handoff to `bun run setup`.
- [x] Added `src/wizard/index.ts` setup wizard with noninteractive automation flags.
- [x] Added user-space systemd service rendering and management.
- [x] Added SQLite stats, integrity check, backup, vacuum, and guarded reset commands.
- [x] Added command-center, wizard, systemd, and SQLite operation tests.

## Known Limitations

- SQLite retention is not implemented yet. The writer stores recent samples indefinitely until the database is manually archived or reset.
- Rollup tables for longer time ranges are planned but not implemented.
- Normalized filesystem/process/pressure child tables are planned but not implemented.
- The UI hydrates a 120-sample rolling window, not arbitrary long-range history browsing.
- The app is designed for loopback/local use, not remote multi-user deployment.

## Recommended Next Work

- [ ] Add raw history retention, defaulting to a configurable 24 to 72 hour window.
- [ ] Add one-minute rollups for longer history ranges.
- [ ] Add a dashboard setting for visible history duration and persisted sample count.
- [ ] Add a writer health indicator in the UI when the internal writer API is unreachable.
- [ ] Add optional normalized child tables for process/filesystem history if the UI starts querying those independently.
