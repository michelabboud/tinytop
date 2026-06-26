# Progress

## Current Version

- Version: `0.1.25`
- Date: 2026-06-26
- Status: Local dashboard with SQLite-backed timestamp-range history browsing, CPU/RAM/swap/load overview gauges, operator status strip, timeline rail, history coverage, process/filesystem controls, a dialog-based settings surface, SQLite-backed daemon dashboard defaults, browser-local display preferences, Rust collector/dashboard single-daemon persistent runtime with embedded dashboard assets, runtime/version identity in the API and sidebar, Rust raw-history pruning and one-minute rollups, auto-detecting command-center startup, legacy Bun collector and dashboard fallback under `legacy/`, current docs/guides/reports aligned to the embedded asset layout, runtime-specific setup verification, in-app confirmation dialogs for browser-local destructive actions, Telecode-style install wizard, Bash command center, systemd user services, SQLite operations, Apache-2.0 licensing, public GitHub release assets, Bun development/fallback runtime, and a current handoff restart point.

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

### 0.1.10 - Public README And Privacy Cleanup

- [x] Added README hero image.
- [x] Added inline README install and usage guide for new users.
- [x] Removed hardcoded local home paths from public docs.
- [x] Replaced host-specific examples with generic examples.
- [x] Removed the old generated UI concept image with host-like demo strings.

### 0.1.11 - Apache License And Private Release Prep

- [x] Switched the project license to Apache License 2.0.
- [x] Added Apache-2.0 package metadata.
- [x] Added a NOTICE file.
- [x] Prepared the docs for a private GitHub release review before public conversion.

### 0.1.12 - Rust Linux Collector Preview

- [x] Kept the existing Bun collector and writer intact.
- [x] Added `agent/` as a Rust workspace.
- [x] Added shared Rust snapshot types matching the existing JSON contract.
- [x] Added a Linux/WSL Rust collector with fixture, live-host, and no-shell-command tests.
- [x] Kept Rust host collection crate-backed through `procfs` and `sysinfo`, with a reusable live `sysinfo::System`.
- [x] Added a SQLx-backed SQLite store crate for the Rust collector path.
- [x] Added `tinytop-agent collect --json` and optional `--sqlite` storage mode.
- [x] Documented the SQLx architecture decision and dependency vetting.

### 0.1.13 - Rust Single-Daemon Runtime

- [x] Added `tinytop-agent serve` as a Rust collector/dashboard daemon on `127.0.0.1:4274`.
- [x] Exposed public `/api/snapshot` and `/api/history` routes from the Rust daemon.
- [x] Exposed legacy collector-compatible `/snapshot/latest`, `/snapshot/collect`, and `/history` routes from the Rust daemon.
- [x] Added interval collection and SQLx-backed SQLite writes in the Rust daemon.
- [x] Updated `./tinytop systemd install` to default to a single Rust `tinytop.service`.
- [x] Kept the legacy Bun split services available with `./tinytop systemd install --bun`.
- [x] Added `./tinytop rust install-binary`, `build`, `serve`, `serve-writer`, `collect`, `test`, and `check`.
- [x] Added Rust-backed DB stats, integrity check, and vacuum support for the command center.
- [x] Updated the setup wizard to ask for GitHub release binary vs local Cargo compile.
- [x] Vendored Apache ECharts with upstream license and notice files for no-Bun runtime use.
- [x] Added ADR 0005 and dependency/provenance reports for Axum and vendored ECharts.

### 0.1.14 - Web UI Confirmation Dialogs

- [x] Scanned the public web UI for native browser dialog APIs.
- [x] Replaced the alert-named inline error surface with `status-message` naming.
- [x] Added a reusable accessible confirmation dialog backed by `<dialog>`.
- [x] Added a confirmed `Clear` control for the browser-local Live History session buffer.
- [x] Added regression coverage for the no-native-dialog policy.
- [x] Documented the dialog policy and rendered verification.

### 0.1.15 - Handoff Checkpoint

- [x] Added root `HANDOFF.md`.
- [x] Captured the current repo, tag, remote, runtime, and health state.
- [x] Confirmed the running daemon is the Rust collector path.
- [x] Recorded recent verification evidence and next useful work.

### 0.1.16 - Collector Naming And Legacy Bun Placement

- [x] Moved the legacy Bun collector daemon to `legacy/bun-collector.ts`.
- [x] Added `bun run collector` and `bun run collector:check` scripts while preserving writer aliases for compatibility.
- [x] Updated the setup wizard to choose `rust` or `bun` collector runtime.
- [x] Kept Rust as the default one-daemon collector/dashboard path.
- [x] Updated new legacy Bun systemd units to use `tinytop-collector.service`.
- [x] Kept command-center cleanup/status paths aware of older `tinytop-writer.service` installs.
- [x] Updated current-facing docs from writer-first language to collector-first language.

### 0.1.17 - Embedded Rust Dashboard Assets

- [x] Moved the static dashboard asset tree to `legacy/dashboard/` for the legacy Bun runtime.
- [x] Added a byte-identical Rust dashboard asset tree under `agent/assets/dashboard/`.
- [x] Embedded the dashboard HTML, CSS, browser JavaScript, and ECharts bundle into `tinytop-agent serve`.
- [x] Kept `--public-dir` and `TINYTOP_PUBLIC_DIR` as explicit development overrides.
- [x] Updated `./tinytop rust serve` and systemd rendering to use embedded assets by default.
- [x] Added regression coverage for embedded Rust serving without a dashboard directory and asset equality across legacy/Rust dashboard trees.
- [x] Added ADR 0006 for embedded Rust dashboard assets and legacy dashboard asset ownership.

### 0.1.18 - Documentation Sweep

- [x] Refreshed root docs and guides for the Rust collector/dashboard daemon and legacy Bun fallback wording.
- [x] Updated dependency and verification reports to point at `agent/assets/dashboard/` and `legacy/dashboard/`.
- [x] Marked the original Bun writer ADR as superseded in the ADR index while preserving the historical ADR file.
- [x] Added a documentation sweep report for the embedded dashboard asset transition.

### 0.1.19 - History Retention Documentation

- [x] Clarified that SQLite raw samples are retained indefinitely until manual archive/reset.
- [x] Clarified that `/api/history` query windows and the dashboard's 120-sample UI buffer are read/rendering limits, not database retention.
- [x] Updated README, guide, install, API, operations, architecture, SQLite history architecture, changelog, progress, and handoff docs.
- [x] Added a documentation report for the retention wording sweep.

### 0.1.20 - Runtime-Specific Setup Verification

- [x] Split package checks into `check:bun`, `check:rust`, and full `check`.
- [x] Updated the setup wizard so Rust selections do not run Bun tests.
- [x] Updated the setup wizard so legacy Bun selections do not run Rust tests.
- [x] Verified Rust release-binary systemd setup installs the binary before running the Rust smoke check.
- [x] Added regression coverage for Rust release, Rust compile, and legacy Bun verification command selection.

### 0.1.21 - Timestamp Timeline Planning And Browser Slice

- [x] Saved the dashboard timeline/settings implementation plan under `docs/superpowers/plans/`.
- [x] Added History range presets for Live, 15m, 1h, 6h, and 24h.
- [x] Replaced index-based timeline selection with timestamp-based selection.
- [x] Changed dashboard history hydration to use explicit `since_ms` and `until_ms` windows.
- [x] Added client-side pagination for large `/api/history` ranges.
- [x] Persisted the selected history range as a browser-local preference.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.
- [x] Added dashboard timeline regression coverage and embedded Rust smoke evidence.

### 0.1.22 - Runtime Auto-Detect And Version Identity

- [x] Added `/api/version` for the Rust collector/dashboard daemon and legacy Bun dashboard.
- [x] Added `/version` on collector-compatible APIs for the Rust daemon and legacy Bun collector.
- [x] Added a sidebar version line showing the serving collector/dashboard runtime and product version.
- [x] Added the SQLite `app_settings` table for daemon dashboard defaults.
- [x] Added `GET /api/settings` and `PUT /api/settings` to the Rust collector/dashboard daemon.
- [x] Added a Settings panel with `This Browser` local preferences and `This Daemon` SQLite-backed defaults.
- [x] Added legacy Bun fallback settings handling so the shared dashboard remains usable in legacy mode.
- [x] Changed `./tinytop start` to auto-select Rust when available and honor `TINYTOP_RUNTIME=legacy|bun` for the legacy fallback.
- [x] Updated `./tinytop status` to read `/api/version` and report the running daemon runtime, component, version, and dashboard asset mode.
- [x] Added foreground `./tinytop stop` and `./tinytop restart` handling for detected Rust and legacy Bun processes when systemd units are absent.
- [x] Aligned Rust crate package versions with the product checkpoint version.

### 0.1.23 - Settings Dialog Presentation

- [x] Moved Settings out of the inline dashboard flow into an accessible modal dialog.
- [x] Changed the rail Settings control from an anchor to a button that opens the dialog.
- [x] Kept `This Browser` and `This Daemon` settings groups intact.
- [x] Kept browser-local and SQLite-backed settings storage unchanged.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.
- [x] Added regression coverage preventing the inline settings section from returning.

### 0.1.24 - Load Overview Gauge

- [x] Added Load as the fourth overview gauge next to CPU, RAM, and swap.
- [x] Normalized Load from 1-minute load divided by CPU core count, capped to 100.
- [x] Added a Load sparkline using the existing normalized load history series.
- [x] Kept the raw 1m/5m/15m load stat tile for detailed context.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.
- [x] Added regression coverage for the Load gauge markup and renderer wiring.

### 0.1.25 - Dashboard Operator Console And Retention

- [x] Saved and executed the operator-console implementation plan under `docs/superpowers/plans/`.
- [x] Added a top operator status strip with Healthy, Warning, Critical, and Stale states from saved thresholds.
- [x] Replaced the native history scrubber with a canvas timeline rail, selected timestamp marker, visible-window shading, and history coverage row.
- [x] Added `/api/history/coverage` in the Rust daemon.
- [x] Added Rust raw-history pruning by `retentionHours`.
- [x] Added Rust one-minute rollup buckets and rollup pruning by `rollupRetentionDays`.
- [x] Expanded daemon thresholds to CPU/RAM/disk/load/pressure warn and critical values.
- [x] Made enabled dashboard sections hide/show Overview, History, Filesystem, Pressure, and Processes.
- [x] Added process search, sort, density controls, and process detail dialog.
- [x] Added filesystem root card, system-mount toggle, and threshold-colored capacity bars.
- [x] Expanded browser-local preferences for visible series, process table state, filesystem toggle, and last section.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.
- [x] Added focused dashboard, server, Rust store, and Rust daemon regression coverage.

## Known Limitations

- Legacy Bun split mode does not enforce durable retention or rollups; use the Rust daemon for automatic pruning and coverage.
- Longer-than-one-minute rollup tiers are planned but not implemented.
- Normalized filesystem/process/pressure child tables are planned but not implemented.
- The dashboard can browse timestamp presets up to 24h; rollup-backed long-range browsing is planned but not implemented.
- The app is designed for loopback/local use, not remote multi-user deployment.
- Native Windows and macOS collectors are planned but not implemented yet.

## Recommended Next Work

- [ ] Use one-minute rollups for longer history query ranges instead of only reporting coverage.
- [ ] Add configurable history coverage and database size targets in the settings UI.
- [ ] Add a collector/daemon health detail drawer when the internal collector API is unreachable.
- [ ] Add optional normalized child tables for process/filesystem history if the UI starts querying those independently.
- [ ] Add native Windows collector support.
- [ ] Add native macOS collector support.
