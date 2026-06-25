# Changelog

## 0.1.14 - 2026-06-25

- Replaced the alert-named inline fetch-error surface with `status-message` naming.
- Added a reusable accessible in-app confirmation dialog for browser UI actions.
- Added a confirmed `Clear` action for the browser-local Live History session buffer without deleting SQLite history or changing system data.
- Added regression coverage that scans the public web UI for browser-native `alert`, `confirm`, and `prompt` calls.
- Documented the no-native-dialog web UI policy and verification evidence.

## 0.1.13 - 2026-06-25

- Added `tinytop-agent serve`, a Rust daemon that serves the dashboard, owns SQLite, collects on an interval, and exposes both public `/api/*` and writer-compatible routes.
- Updated systemd defaults to install a single Rust `tinytop.service`; kept the legacy Bun split services behind `./tinytop systemd install --bun`.
- Added `./tinytop rust` commands for release-binary install, local build, collect, serve, serve-writer, test, and check.
- Updated the setup wizard to ask whether the Rust agent should come from a GitHub release binary or a local Cargo compile.
- Added Rust-backed DB `stats`, `check`, and `vacuum` paths so the command center can manage SQLite without Bun when a Rust agent or Cargo is available.
- Vendored the Apache ECharts browser bundle under `public/vendor/` with upstream license and notice files so the Rust daemon can run without `node_modules`.
- Added Axum-based daemon tests, Rust history JSON contract tests, SQLite file-creation regression coverage, and Bash command-center tests for the Rust systemd path.
- Documented the Rust single-daemon runtime, Axum dependency decision, vendored asset provenance, and no-Bun install path.

## 0.1.12 - 2026-06-24

- Added an additive Rust workspace under `agent/` without removing or replacing the existing Bun collector.
- Added shared Rust snapshot types that serialize to the current dashboard JSON contract.
- Added a Rust Linux/WSL collector with parser, fixture, live-host, and no-shell-command tests.
- Added a SQLx-backed SQLite history store proof point for the Rust agent path.
- Added `tinytop-agent collect --json` and optional `--sqlite` collect-and-store mode.
- Documented the Rust collector preview, SQLx decision, dependency vetting, crate-backed host collection, and Rust `1.95.0` requirement.

## 0.1.11 - 2026-06-24

- Changed the project license from MIT to Apache License 2.0.
- Added package license metadata and a NOTICE file for Apache-2.0 attribution.
- Prepared the repository for a private GitHub release before public conversion.

## 0.1.10 - 2026-06-24

- Added a README hero image and inline new-user install guide.
- Removed public-doc references to local home paths, host names, and personalized implementation notes.
- Removed the old generated UI concept image that contained host-like demo strings.

## 0.1.9 - 2026-06-24

- Implemented the root `./tinytop` Bash command center with help, Bun install guidance, doctor/status, dependency install, verification, foreground start, split start, logs, monitor, and restart/stop wrappers.
- Added `bun run setup` as a real Bun setup wizard launched by `./tinytop setup`, with noninteractive automation flags and systemd mode.
- Added user-space systemd rendering and management for `tinytop-writer.service` and `tinytop-dashboard.service`.
- Added SQLite operations for stats, integrity check, backup, vacuum, and guarded reset.
- Added tests for the Bash command center, setup wizard, systemd unit rendering, and SQLite operations.

## 0.1.8 - 2026-06-24

- Recorded the approved Telecode-style install wizard design for TinyTop.
- Chose a two-layer installer: a zero-dependency `./tinytop` Bash command center that can bootstrap Bun, then a richer `bun run setup` wizard once Bun exists.
- Added ADR 0003 for the Bash bootstrap plus Bun wizard architecture.
- Documented the planned command surface for setup, start, restart, stop, status, logs, monitor, stats, SQLite maintenance, backups, and systemd user services.

## 0.1.7 - 2026-06-24

- Renamed the project to TinyTop, including package name, app title, default SQLite data directory, browser storage keys, documentation, and local port claim.
- Rewrote the root `README.md`, `INSTALL.md`, `GUIDE.md`, `ARCHITECTURE.md`, `PROGRESS.md`, and `CHANGELOG.md` documentation set.
- Added operations and API guides under `docs/guides/`.
- Documented ports, environment variables, SQLite location, runtime modes, verification commands, troubleshooting, and current persistence limitations.

## 0.1.6 - 2026-06-24

- Implemented SQLite-backed recent history through a dedicated Bun collector/writer process on `127.0.0.1:4276`.
- Added `/api/history` hydration so refreshing the dashboard refills the Live History chart instead of starting from scratch.
- Made frontend history insertion timestamp-aware so repeated latest samples update in place rather than duplicating bars.
- Added tests for persistent history storage and the dashboard history API.

## 0.1.5 - 2026-06-24

- Made stacked bar history use a viewport-derived visible sample count so bars keep a minimum width and the live window rolls left.
- Added a SQLite history architecture plan and ADR for a dedicated collector/writer process and dashboard read path.
- Kept dashboard display settings as browser-local preferences.

## 0.1.4 - 2026-06-24

- Replaced the hand-rolled Live History canvas chart with Apache ECharts served from the local dependency tree.
- Added ECharts-backed stacked area, stacked bar, heatmap, and treemap graph modes.
- Added a local `/vendor/echarts.min.js` route and coverage for serving that bundle.
- Kept visible-window sample counts, chart sample selection, and compact selected-sample metric chips.

## 0.1.3 - 2026-06-24

- Restored the Live History bar graph mode.
- Moved graph-type controls into the Live History top nav.
- Moved the timeline into its own row under the chart with selected datetime context.
- Added selected-sample metric values and percent-axis labels so bar, line, and area modes have numeric context.
- Added latest-value labels to heatmap lanes so the view has numeric context.
- Kept area mode as a filled-under-line chart for the independent CPU, RAM, swap, and load series.

## 0.1.2 - 2026-06-24

- Moved Live History directly below the CPU, RAM, and swap gauges.
- Removed the duplicate bar history mode.
- Added a timeline scrubber that lets the main gauges inspect older local samples.
- Added a Live control that returns the gauges to the newest sample.

## 0.1.1 - 2026-06-24

- Added five selectable dashboard themes: Midnight, Matrix, Aurora, Solar, and Ember.
- Added four live history graph modes: line, area, bars, and heatmap.
- Persisted theme and graph preferences in browser-local storage.
- Updated chart rendering so theme changes recolor canvas graphs immediately.

## 0.1.0 - 2026-06-24

- Added the initial standalone Bun dashboard project.
- Claimed local port `127.0.0.1:4274`.
- Added read-only live collectors for `/proc`, `df`, `ps`, `uname`, and OS release data.
- Added automatic WSL versus real Linux runtime detection.
- Added dark operations dashboard UI with gauges, stat tiles, charts, filesystem bars, pressure meters, and process rows.
- Added Bun unit tests and rendered Playwright QA coverage.
