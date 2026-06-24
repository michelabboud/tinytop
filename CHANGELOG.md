# Changelog

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
