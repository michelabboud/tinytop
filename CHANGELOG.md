# Changelog

## 0.1.27 - 2026-06-26

- Added an operator alert detail drawer explaining current state by metric, value, threshold, age, trend, and recent change.
- Added rollup-backed History ranges for 6h, 24h, 7d, and 30d through additive `/api/history/points`, while keeping `/api/history` raw-snapshot compatible.
- Added timeline markers through `/api/history/markers` for daemon starts, settings changes, and computed coverage gaps.
- Added SQLite-backed DB budget settings and coverage fields: `targetDatabaseBytes`, budget percentage, and rollup coverage timestamps.
- Polished Settings with validation, dirty-close warning, reset/defaults buttons, threshold presets, and an effective-settings readout.
- Upgraded process details with redacted copy-safe command text, parent PID/start time when available, RSS, and per-PID CPU/RAM trend.
- Started feature-gated native Rust collector modules for macOS and Windows while keeping Linux as the default reference collector.
- Added ADRs for the additive history points/markers API and feature-gated native platform collectors.
- Cleaned the stale handoff PID note.

## 0.1.26 - 2026-06-26

- Fixed native select dropdown contrast in the Settings dialog and process density control by assigning explicit readable option foreground/background colors for every dashboard theme.
- Added regression coverage for themed native dropdown option colors.
- Kept Rust embedded dashboard assets and legacy Bun dashboard assets byte-identical.

## 0.1.25 - 2026-06-26

- Added an operator status strip with Healthy, Warning, Critical, and Stale states computed from saved daemon thresholds.
- Replaced the native History range input with a canvas timeline rail, selected timestamp marker, visible-window shading, visible-series preferences, and history coverage display.
- Added Rust `/api/history/coverage`, raw-history pruning by `retentionHours`, and one-minute rollups pruned by `rollupRetentionDays`.
- Expanded settings thresholds to CPU/RAM/disk/load/pressure warning and critical values, and applied enabled-section settings to the dashboard layout.
- Added process search/sort/density controls, a process detail dialog, a root filesystem card, a system-mount toggle, and threshold-colored filesystem/pressure states.
- Kept Rust embedded dashboard assets and legacy Bun dashboard assets byte-identical.

## 0.1.24 - 2026-06-26

- Added a Load overview gauge next to CPU, RAM, and swap.
- Normalized the Load gauge from 1-minute load divided by CPU core count, matching the existing History chart load percentage.
- Added a Load sparkline to the overview row while keeping the raw 1m/5m/15m load tile for detail context.
- Kept Rust embedded dashboard assets and legacy Bun dashboard assets byte-identical.

## 0.1.23 - 2026-06-26

- Moved dashboard Settings out of the main metrics flow into an accessible modal dialog opened from the rail.
- Changed the rail Settings item from an anchor to a button so it opens the dialog instead of scrolling the dashboard.
- Kept the existing `This Browser` and `This Daemon` settings split, backed by localStorage and `/api/settings`.
- Kept Rust embedded dashboard assets and legacy Bun dashboard assets byte-identical.

## 0.1.22 - 2026-06-26

- Added `/api/version` to the Rust collector/dashboard daemon and legacy Bun dashboard, plus `/version` to collector-compatible APIs.
- Added SQLite-backed daemon dashboard defaults with `GET /api/settings` and `PUT /api/settings`.
- Added a Settings panel with separate `This Browser` local preferences and `This Daemon` daemon defaults.
- Added typed settings validation for theme, graph mode, history window, refresh interval, retention defaults, thresholds, and enabled sections.
- Added a dashboard sidebar version line so users can see whether Rust or legacy Bun is serving the page.
- Changed `./tinytop start` to auto-select the Rust collector/dashboard daemon when available, with `TINYTOP_RUNTIME=legacy` or `TINYTOP_RUNTIME=bun` as explicit legacy overrides.
- Updated `./tinytop status` to report the running daemon runtime, component, product version, and dashboard asset mode from `/api/version`.
- Added foreground `./tinytop stop`/`restart` awareness for Rust and legacy Bun processes when systemd units are not installed.
- Aligned Rust crate package versions with the product checkpoint version.

## 0.1.21 - 2026-06-26

- Saved the dashboard timeline/settings implementation plan under `docs/superpowers/plans/`.
- Added History range presets for Live, 15m, 1h, 6h, and 24h.
- Replaced index-based timeline state with timestamp-based selection.
- Changed dashboard history hydration to use explicit `since_ms` and `until_ms` windows, with client-side pagination for larger ranges.
- Persisted the selected history range in browser-local storage as `tinytop.historyWindow`.
- Added dashboard timeline regression coverage and refreshed docs for the new timeline behavior and settings roadmap.

## 0.1.20 - 2026-06-26

- Split verification scripts into runtime-specific `check:bun` and `check:rust` commands while keeping `bun run check` as the full maintainer suite.
- Updated the setup wizard to run only the selected collector's verification path: Rust choices avoid Bun tests, and legacy Bun choices avoid Rust tests.
- Made Rust release-binary systemd setup install the release binary before running the Rust smoke check.
- Added regression coverage for Rust release, Rust compile, and legacy Bun setup verification command selection.
- Updated docs and handoff notes for runtime-specific setup verification.

## 0.1.19 - 2026-06-26

- Clarified current history retention behavior across the README, user guide, install guide, API guide, operations guide, architecture docs, progress notes, and handoff.
- Documented that SQLite raw samples are retained until manual archive/reset because automatic retention is not implemented yet.
- Documented that `/api/history` windows and the dashboard's 120-sample rolling buffer are read/rendering limits, not database retention limits.
- Added a documentation report for the history-retention wording sweep.

## 0.1.18 - 2026-06-25

- Refreshed the current documentation and guides after the embedded Rust collector/dashboard asset move.
- Updated user-facing port, process, API, and operations wording to describe the Rust collector/dashboard daemon and the legacy Bun dashboard/collector fallback.
- Updated dependency and UI verification reports so current commands reference `agent/assets/dashboard/` and `legacy/dashboard/` instead of the removed root `public/` tree.
- Marked ADR 0001 as superseded in the ADR index while preserving the historical ADR file unchanged.

## 0.1.17 - 2026-06-25

- Moved the static dashboard assets from root `public/` into `legacy/dashboard/` for the legacy Bun runtime.
- Added a byte-identical Rust dashboard asset tree under `agent/assets/dashboard/`.
- Embedded the dashboard HTML, CSS, browser JavaScript, and ECharts bundle into `tinytop-agent serve`.
- Kept `--public-dir` and `TINYTOP_PUBLIC_DIR` as explicit development overrides while making embedded assets the default Rust path.
- Updated the Bun development server, command center, tests, docs, and handoff for embedded Rust dashboard ownership.
- Added regression coverage for embedded Rust serving without a dashboard directory and for legacy/Rust dashboard asset equality.
- Added ADR 0006 for embedded Rust dashboard assets and legacy dashboard asset ownership.

## 0.1.16 - 2026-06-25

- Moved the legacy Bun collector daemon from `src/collector-daemon.ts` to `legacy/bun-collector.ts`.
- Added `bun run collector` and `bun run collector:check`, keeping writer script aliases for compatibility.
- Updated the setup wizard to ask for `rust` or `bun` collector runtime; Rust means the single collector/dashboard daemon, while Bun means the legacy split collector/dashboard path.
- Renamed new legacy Bun systemd rendering/install output to `tinytop-collector.service`, while keeping cleanup and service actions aware of the older `tinytop-writer.service` name.
- Updated command-center, wizard, architecture, install, API, operations, and README wording from writer-first language to collector-first language.
- Added regression tests for the legacy collector path, setup wizard collector selection, and systemd unit rendering.

## 0.1.15 - 2026-06-25

- Added `HANDOFF.md` as the current TinyTop restart point.
- Recorded the live Rust daemon state, Rust collector confirmation, recent verification evidence, and next useful work.
- Bumped the docs-only checkpoint version so the handoff can be committed, tagged, and pulled cleanly.

## 0.1.14 - 2026-06-25

- Replaced the alert-named inline fetch-error surface with `status-message` naming.
- Added a reusable accessible in-app confirmation dialog for browser UI actions.
- Added a confirmed `Clear` action for the browser-local Live History session buffer without deleting SQLite history or changing system data.
- Added regression coverage that scans the public web UI for browser-native `alert`, `confirm`, and `prompt` calls.
- Documented the no-native-dialog web UI policy and verification evidence.

## 0.1.13 - 2026-06-25

- Added `tinytop-agent serve`, a Rust daemon that serves the dashboard, owns SQLite, collects on an interval, and exposes both public `/api/*` and legacy collector-compatible routes.
- Updated systemd defaults to install a single Rust `tinytop.service`; kept the legacy Bun split services behind `./tinytop systemd install --bun`.
- Added `./tinytop rust` commands for release-binary install, local build, collect, serve, serve-writer, test, and check.
- Updated the setup wizard to ask whether the Rust collector binary should come from a GitHub release binary or a local Cargo compile.
- Added Rust-backed DB `stats`, `check`, and `vacuum` paths so the command center can manage SQLite without Bun when a Rust binary or Cargo is available.
- Vendored the Apache ECharts browser bundle with upstream license and notice files so the Rust daemon can run without `node_modules`.
- Added Axum-based daemon tests, Rust history JSON contract tests, SQLite file-creation regression coverage, and Bash command-center tests for the Rust systemd path.
- Documented the Rust single-daemon runtime, Axum dependency decision, vendored asset provenance, and no-Bun install path.

## 0.1.12 - 2026-06-24

- Added an additive Rust workspace under `agent/` without removing or replacing the existing Bun collector.
- Added shared Rust snapshot types that serialize to the current dashboard JSON contract.
- Added a Rust Linux/WSL collector with parser, fixture, live-host, and no-shell-command tests.
- Added a SQLx-backed SQLite history store proof point for the Rust collector path.
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
