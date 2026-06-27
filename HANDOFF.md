# TinyTop Handoff

Date: 2026-06-27 14:08 Asia/Jerusalem

## Current Repo State

- Repo: `/home/michel/projects/tinytop`
- Branch: `main`
- Remote: `origin` at `git@github.com:michelabboud/tinytop.git`
- Current checkpoint version: `0.1.33`
- Version files: `VERSION`, `package.json`, `tinytop`, and `tinytop.ps1` all read `0.1.33`
- Rust crate package versions under `agent/crates/*/Cargo.toml` read `0.1.33`
- The `v0.1.33` checkpoint adds Windows PowerShell service elevation/confirmation guarding and keeps the Rust dashboard release identity current.

## Runtime State

- Dashboard URL when running: `http://127.0.0.1:4274`
- Health endpoint when running: `http://127.0.0.1:4274/health`
- Version endpoint when running: `http://127.0.0.1:4274/api/version`
- Settings endpoint when running: `http://127.0.0.1:4274/api/settings`
- History coverage endpoint when running: `http://127.0.0.1:4274/api/history/coverage`
- History points endpoint when running: `http://127.0.0.1:4274/api/history/points`
- History markers endpoint when running: `http://127.0.0.1:4274/api/history/markers`
- Health status at handoff refresh time: running
- Runtime identity at handoff refresh time: `rust collector-dashboard-daemon v0.1.33 (embedded dashboard)`
- Dashboard port `127.0.0.1:4274`: in use by `tinytop-agent serve`
- Legacy Bun collector port `127.0.0.1:4276`: free
- Active TinyTop foreground process at handoff refresh time: Rust daemon PID `355625`
- Foreground daemon was started detached with `setsid ./tinytop start`, which auto-selected Rust.
- Current foreground daemon log: `/tmp/tinytop-v0.1.33-windows-service-guard-20260627-140800.log`

## Rust Collector Confirmation

The default persistent dashboard path is the Rust collector/dashboard daemon.

Evidence:

- `ARCHITECTURE.md` states `tinytop-agent serve` serves the dashboard, returns the latest stored sample or collects a fresh one, and collects telemetry through `tinytop-collectors`.
- `tinytop-agent serve` embeds dashboard assets from `agent/assets/dashboard/` by default.
- `./tinytop systemd install` defaults to the single Rust collector/dashboard daemon.
- `./tinytop start` now auto-selects the Rust collector/dashboard daemon when a release binary or Cargo is available.
- `./tinytop status` reads `/api/version` to show the running runtime, component, version, and dashboard asset mode.
- The legacy Bun collector now lives at `legacy/bun-collector.ts` and is available only through explicit Bun development or `--bun` systemd mode.
- The legacy Bun dashboard assets now live at `legacy/dashboard/`.

## Recently Completed

### v0.1.33 - Windows Service Elevation Guard

- Added a shared PowerShell guard for mutating Windows service commands.
- `service install`, `service start`, `service stop`, `service restart`, and `service uninstall` now check elevation before running.
- Interactive non-elevated service mutations warn and ask for explicit confirmation; non-interactive non-elevated runs fail with Administrator guidance.
- Kept `service status` read-only and non-prompting.
- Updated Windows install docs and regression coverage.

### v0.1.32 - Live Connected README Screenshot

- Replaced the README screenshot with a fresh rendered capture from the running Rust collector/dashboard daemon.
- The screenshot now shows real connected dashboard values, including host identity, health, CPU, RAM, swap, load, history samples, and the green `Live` indicator.
- Bumped product and Rust crate versions to `0.1.32`.
- Rebuilt the release `tinytop-agent` binary so `/api/version` reports the current checkpoint.

### v0.1.31 - Settings Readout And Rust Agent Rebuild

- Fixed the Settings dialog effective-settings readout so compact browser/daemon defaults no longer stretch into oversized ovals.
- Changed daemon redaction and enabled-section checkboxes into compact responsive toggle controls.
- Kept the Rust embedded dashboard asset tree and legacy Bun dashboard asset tree aligned for the CSS fix.
- Bumped product and Rust crate versions to `0.1.31`.
- Added a fresh rendered dashboard screenshot to the README.
- Rebuilt the release `tinytop-agent` binary with the embedded dashboard layout fixes.

### v0.1.14 - Web UI Confirmation Dialogs

- Replaced alert-named UI hooks with `status-message`.
- Added a reusable accessible in-app confirmation dialog.
- Added a confirmed `Clear` action for the browser-local Live History session buffer.
- Added `tests/webui-dialogs.test.ts`, which scans dashboard UI files and rejects native `alert`, `confirm`, and `prompt` calls.
- Updated `README.md`, `GUIDE.md`, `ARCHITECTURE.md`, `CHANGELOG.md`, `PROGRESS.md`, and `docs/reports/2026-06-25-webui-confirmation-dialog-verification.md`.
- Committed and pushed `d829160`.
- Tagged and pushed `v0.1.14`.

### v0.1.15 - Handoff Checkpoint

- Added this root `HANDOFF.md` restart point.
- Recorded live daemon state, Rust collector confirmation, recent verification evidence, and next useful work.

### v0.1.16 - Collector Naming And Legacy Bun Placement

- Moved the legacy Bun collector daemon to `legacy/bun-collector.ts`.
- Added `bun run collector` and `bun run collector:check`, keeping writer aliases for compatibility.
- Updated the setup wizard to ask for `rust` or `bun` collector runtime.
- Kept Rust as the default one-daemon collector/dashboard path.
- Renamed newly rendered legacy Bun systemd collector service to `tinytop-collector.service`.
- Kept cleanup/status paths aware of older `tinytop-writer.service` installs.

### v0.1.17 - Embedded Rust Dashboard Assets

- Moved the static dashboard asset tree to `legacy/dashboard/` for the legacy Bun runtime.
- Added a byte-identical Rust dashboard asset tree under `agent/assets/dashboard/`.
- Embedded the dashboard HTML, CSS, browser JavaScript, and ECharts bundle into `tinytop-agent serve`.
- Kept `--public-dir` and `TINYTOP_PUBLIC_DIR` as explicit development overrides.
- Updated `./tinytop rust serve` and systemd rendering to use embedded assets by default.
- Added ADR 0006 for embedded Rust dashboard assets and legacy dashboard asset ownership.

### v0.1.18 - Documentation Sweep

- Refreshed current docs and guides for the embedded Rust collector/dashboard daemon.
- Updated current-path references from the removed root `public/` tree to `agent/assets/dashboard/` and `legacy/dashboard/`.
- Added `docs/reports/2026-06-25-documentation-sweep.md`.
- Updated the ADR index to show ADR 0001 as superseded by the Rust single-daemon runtime decision, without rewriting the historical ADR.

### v0.1.19 - History Retention Documentation

- Clarified that SQLite raw history retention is not implemented yet.
- Documented that raw rows stay in SQLite until manual archive/reset.
- Documented that `/api/history` query windows and the dashboard's 120-sample buffer are read/rendering limits, not database retention.
- Added `docs/reports/2026-06-26-history-retention-docs.md`.

### v0.1.20 - Runtime-Specific Setup Verification

- Added `bun run check:bun` for Bun dashboard/legacy collector checks.
- Added `bun run check:rust` for Rust fmt/workspace tests.
- Kept `bun run check` and `./tinytop check` as full maintainer verification.
- Updated `./tinytop setup` so Rust selections do not run Bun tests and legacy Bun selections do not run Rust tests.
- Rust release-binary systemd setup now installs the binary before running `./tinytop rust collect` as the smoke check.
- Added `docs/reports/2026-06-26-runtime-specific-verification.md`.

### v0.1.21 - Timestamp Timeline Planning And Browser Slice

- Saved `docs/superpowers/plans/2026-06-26-dashboard-timeline-settings.md` for the approved timeline, settings, retention, and rollup roadmap.
- Added History range presets for Live, 15m, 1h, 6h, and 24h.
- Replaced index-based timeline selection with timestamp-based selection.
- Changed dashboard history loading to request explicit `since_ms` and `until_ms` windows and page larger ranges through the existing `/api/history` limit.
- Fixed Rust `/api/history` backfill so explicit empty timestamp windows return `[]` instead of dropping bounds and returning default recent samples.
- Persisted the selected history range as browser-local `tinytop.historyWindow`.
- Kept `agent/assets/dashboard/` and `legacy/dashboard/` byte-identical.
- Added `tests/dashboard-timeline.test.ts`.
- Added a Rust serve-contract regression for explicit empty history bounds.

### v0.1.22 - Runtime Auto-Detect And Version Identity

- Added `/api/version` to the Rust collector/dashboard daemon and legacy Bun dashboard.
- Added `/version` to collector-compatible APIs for the Rust daemon and legacy Bun collector.
- Added a sidebar version line so users can see the serving runtime and product version in the dashboard.
- Added SQLite-backed daemon dashboard defaults with typed Rust validation.
- Added `GET /api/settings` and `PUT /api/settings` to the Rust collector/dashboard daemon.
- Added a Settings panel with `This Browser` local preferences and `This Daemon` SQLite-backed defaults.
- Added ADR 0007 for the browser-local versus daemon-wide settings split.
- Changed `./tinytop start` to auto-select Rust when available, with `TINYTOP_RUNTIME=legacy` or `TINYTOP_RUNTIME=bun` as explicit legacy overrides.
- Updated `./tinytop status` to report the running daemon runtime, component, product version, and dashboard asset mode.
- Added foreground `./tinytop stop` and `./tinytop restart` detection for Rust and legacy Bun runtimes when systemd units are not installed.
- Aligned Rust crate package versions with the product checkpoint version.
- Added `docs/reports/2026-06-26-runtime-auto-detect-version.md`.

### v0.1.23 - Settings Dialog Presentation

- Moved Settings out of the inline dashboard metrics flow into an accessible `<dialog>`.
- Changed the left-rail Settings control from an anchor to a button that opens the dialog.
- Kept `This Browser` localStorage preferences and `This Daemon` SQLite-backed defaults unchanged.
- Added Close, Cancel, Escape, backdrop close, and focus-return behavior.
- Tuned the settings grid so desktop and mobile dialog controls fit without horizontal overflow.
- Kept `agent/assets/dashboard/` and `legacy/dashboard/` byte-identical.
- Added ADR 0008 and `docs/reports/2026-06-26-settings-dialog.md`.

### v0.1.24 - Load Overview Gauge

- Added Load as the fourth overview gauge next to CPU, RAM, and swap.
- Normalized Load from 1-minute load divided by CPU core count, capped to 100.
- Added a Load sparkline using the existing normalized load history series.
- Kept the raw 1m/5m/15m load stat tile for detail context.
- Kept `agent/assets/dashboard/` and `legacy/dashboard/` byte-identical.
- Added `tests/dashboard-overview.test.ts` and `docs/reports/2026-06-26-load-gauge.md`.

### v0.1.25 - Dashboard Operator Console And Retention

- Saved `docs/superpowers/plans/2026-06-26-dashboard-operator-console.md` for the approved operator-console implementation.
- Added a top operator strip with Healthy, Warning, Critical, and Stale states.
- Replaced the native history scrubber with a canvas timeline rail, visible-window shading, selected timestamp marker, visible-series controls, and history coverage row.
- Added Rust `/api/history/coverage`.
- Added Rust raw-history pruning by `retentionHours`.
- Added Rust one-minute rollup buckets and pruning by `rollupRetentionDays`.
- Expanded daemon thresholds to CPU/RAM/disk/load/pressure warning and critical values.
- Applied enabled-section settings to Overview, History, Filesystem, Pressure, and Processes.
- Added process search/sort/density controls and a process detail dialog.
- Added filesystem root card, system-mount toggle, and threshold-colored filesystem/pressure states.
- Kept `agent/assets/dashboard/` and `legacy/dashboard/` byte-identical.
- Added `tests/dashboard-operator-alert.test.ts` and `tests/dashboard-process-filesystem.test.ts`.

### v0.1.26 - Native Dropdown Contrast

- Fixed native select dropdown option contrast for Settings and process density controls across Midnight, Matrix, Aurora, Solar, and Ember themes.
- Added dashboard regression coverage for readable native dropdown option colors.
- Updated `docs/reports/2026-06-26-select-dropdown-contrast.md`.
- Rebuilt and restarted the Rust embedded dashboard daemon; it now reports `rust collector-dashboard-daemon v0.1.26 (embedded dashboard)`.

### v0.1.27 - Dashboard Operator V2 And Platform Collector Roadmap

- Saved and executed `docs/superpowers/plans/2026-06-26-dashboard-operator-v2-platform-roadmap.md`.
- Added an operator detail drawer that explains the current Healthy/Warning/Critical/Stale state with metric value, threshold, sample age, trend, and recent-change context.
- Added additive Rust `/api/history/points` and `/api/history/markers` endpoints so the dashboard can browse 6h, 24h, 7d, and 30d ranges with rollup points and timeline markers.
- Added daemon-start, settings-change, and computed coverage-gap markers.
- Added SQLite-backed DB budget settings and coverage fields for target database bytes, budget percentage, and rollup oldest/newest timestamps.
- Polished Settings with validation, dirty-close warning, reset/defaults buttons, threshold presets, and an effective-settings readout.
- Upgraded process details with redacted copy-safe command text, optional parent PID/start time, RSS, and per-PID CPU/RAM trend.
- Started feature-gated native macOS and Windows Rust collector modules while keeping Linux/WSL as the default reference collector.
- Added ADR 0009 and ADR 0010, plus `docs/reports/2026-06-26-dashboard-operator-v2-platform-roadmap.md`.
- Cleaned the stale bottom handoff PID note.

### v0.1.28 - SVG Favicon

- Added `favicon.svg` to `legacy/dashboard/` and `agent/assets/dashboard/`.
- Replaced the blank favicon link with `/favicon.svg`.
- Added `/favicon.svg` to the Rust embedded dashboard route and asset allowlist.
- Served SVG assets as `image/svg+xml; charset=utf-8`.
- Added regression coverage for dashboard asset parity and embedded Rust serving of the favicon.
- Added `docs/reports/2026-06-26-svg-favicon.md`.

### v0.1.29 - Windows Command Center And Critical Status

- Saved and executed `docs/superpowers/plans/2026-06-26-windows-command-center-and-critical-status.md`.
- Added `tinytop.ps1` for Windows-native Rust binary install, Rust build, start, stop, restart, status, logs, and service commands.
- Added Windows service install/uninstall/start/stop/restart/status commands through PowerShell and Windows Service Control Manager.
- Made Windows builds select `--no-default-features --features windows-collector`.
- Made the Bash command center print target-specific Rust build commands and use `.exe` binary names on Windows-like shells.
- Strengthened operator strip styling so Critical, Warning, and Stale states are visually obvious at a glance.
- Cleaned the sidebar runtime identity so long WSL detection reasons collapse into a compact runtime pill plus hover detail.
- Added `docs/guides/WINDOWS.md`, ADR 0011, and `docs/reports/2026-06-26-windows-command-center-and-critical-status.md`.

### Release Binary Asset Check

- `v0.1.18` release assets were updated with `tinytop-agent-linux-x86_64` and its `.sha256` file.
- A temporary-HOME install test confirmed `./tinytop rust install-binary` downloaded and ran the release binary.

### Daemon Start

- Current daemon was started with:

  ```bash
  setsid ./tinytop start > /tmp/tinytop-v0.1.29.log 2>&1 &
  ```

- Verified health with:

  ```bash
  curl -fsS http://127.0.0.1:4274/health
  ```

## Verification Evidence From Latest Feature Checkpoint

- `./tinytop check`
  - Bun tests: `43 pass`, `0 fail`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - Rust workspace tests: passed
  - Browser bundle: built `legacy/dashboard/app.js` successfully
- `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`: clean
- `./tinytop rust build`: built `agent/target/release/tinytop-agent`
- Embedded dashboard smoke test with `./tinytop rust serve --sqlite sqlite::memory: --poll-ms 100000`
  - `/health`: `ok`
  - `/`: contained `<title>TinyTop</title>`
  - `/app.js`: contained `requestConfirmation`
  - `/vendor/echarts.min.js`: contained `echarts`
  - Process stopped after the smoke test; default ports are free
- `git diff --check`: clean

## Verification Evidence From v0.1.18 Documentation Sweep

- `./tinytop check`
  - Bun tests: `43 pass`, `0 fail`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - Rust workspace tests: passed
  - Browser bundle: built `legacy/dashboard/app.js` successfully
- `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`: clean
- `git diff --check`: clean

## Verification Evidence From v0.1.19 History Retention Documentation

- `./tinytop check`
  - Bun tests: `43 pass`, `0 fail`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - Rust workspace tests: passed
  - Browser bundle: built `legacy/dashboard/app.js` successfully
- `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`: clean
- `git diff --check`: clean

## Verification Evidence From v0.1.20 Runtime-Specific Setup Verification

- `bun test tests/wizard.test.ts`
  - Wizard tests: `9 pass`, `0 fail`
- `./tinytop check`
  - `bun run check:bun`: `46 pass`, `0 fail`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - `bun run check:rust`: Rust fmt check and workspace tests passed
  - Browser bundle: built `legacy/dashboard/app.js` successfully
- `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`: clean
- `git diff --check`: clean

## Verification Evidence From v0.1.21 Timestamp Timeline Slice

- `./tinytop check`
  - Bun tests: `50 pass`, `0 fail`, `178 expect() calls`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - Rust fmt check and workspace tests passed
  - Browser bundle built `legacy/dashboard/app.js` successfully
- `bun test tests/dashboard-timeline.test.ts tests/dashboard-assets.test.ts tests/webui-dialogs.test.ts`
  - Dashboard timeline, asset parity, and web UI policy tests: `9 pass`, `0 fail`
- `cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_respects_empty_explicit_history_bounds`
  - Explicit empty timestamp bounds regression: `1 pass`, `0 fail`
- `bun build legacy/dashboard/app.js --target=browser --outdir=/tmp/tinytop-dashboard-timeline-check`
  - Browser bundle built successfully
- `diff -qr agent/assets/dashboard legacy/dashboard`
  - No differences
- `./tinytop rust build`
  - Rebuilt `agent/target/release/tinytop-agent` so embedded dashboard bytes include the new timeline assets
- Embedded dashboard smoke test on alternate port `4284`
  - `/health`: `ok`
  - `/app.js`: contained `selectedAtMs`, `fetchHistoryPage`, `since_ms`, and `until_ms`
  - `/`: contained all five `data-history-window` buttons
  - `/api/history?limit=5&since_ms=0`: returned SQLite-backed samples
  - Alternate-port smoke daemon stopped after verification
- `bun audit`: no vulnerabilities found
- `cargo audit --file agent/Cargo.lock`: scanned 196 crate dependencies, no vulnerabilities reported
- `git diff --check`: clean

## Verification Evidence From v0.1.22 Runtime Auto-Detect And Version Identity

- Red/green focused tests:
  - `bun test tests/tinytop-script.test.ts tests/server.test.ts tests/dashboard-timeline.test.ts`: `24 pass`, `0 fail`
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_exposes_version_identity`: `1 pass`, `0 fail`
- Settings focused tests:
  - `bun test tests/dashboard-settings.test.ts tests/server.test.ts tests/dashboard-timeline.test.ts tests/dashboard-assets.test.ts`: `17 pass`, `0 fail`
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-store sqlite_store_persists_dashboard_settings`: `1 pass`, `0 fail`
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_`: `6 pass`, `0 fail`
- `./tinytop check`
  - Bun tests: `62 pass`, `0 fail`, `231 expect() calls`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - Rust fmt check and workspace tests passed
  - Browser bundle built `legacy/dashboard/app.js` successfully
- `diff -qr agent/assets/dashboard legacy/dashboard`: no differences
- `bun audit`: no vulnerabilities found
- `cargo audit --file agent/Cargo.lock`: scanned 196 crate dependencies, no vulnerabilities reported
- `git diff --check`: clean
- `./tinytop rust build`: built `agent/target/release/tinytop-agent`
- Embedded dashboard smoke through `./tinytop start` on alternate port `4285`
  - `/api/version`: returned `{"status":"ok","app":"tinytop","version":"0.1.22","runtime":"rust","component":"collector-dashboard-daemon","dashboard":"embedded"}`
  - `/`: contained `id="daemon-version"`
  - `/app.js`: contained `fetch("/api/version"` and `renderVersion`
  - `PORT=4285 ./tinytop status`: reported `rust collector-dashboard-daemon v0.1.22 (embedded dashboard)`
  - Alternate-port smoke daemon stopped after verification
- Default daemon refreshed through `./tinytop start`
  - `curl -fsS http://127.0.0.1:4274/api/version`: returned Rust `0.1.22` embedded dashboard identity
  - `curl -fsS http://127.0.0.1:4274/api/settings`: returned default daemon settings from SQLite-backed API
  - `/`: contains `id="settings"`, `This Browser`, and `This Daemon`
  - `/app.js`: contains `fetchSettings`, `saveDaemonSettings`, and `restartPollingTimer`
  - `./tinytop status`: reported `rust collector-dashboard-daemon v0.1.22 (embedded dashboard)`

## Verification Evidence From v0.1.23 Settings Dialog Presentation

- `bun test tests/dashboard-settings.test.ts tests/dashboard-assets.test.ts tests/webui-dialogs.test.ts`
  - Dashboard settings dialog, asset parity, and web UI dialog policy tests: `8 pass`, `0 fail`, `48 expect() calls`
- `./tinytop check`
  - Bun tests: `63 pass`, `0 fail`, `238 expect() calls`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - Rust fmt check and workspace tests passed
  - Browser bundle built `legacy/dashboard/app.js` successfully
- `./tinytop rust build`
  - Built `agent/target/release/tinytop-agent` with the embedded `0.1.23` dashboard assets.
- `diff -qr agent/assets/dashboard legacy/dashboard`
  - No differences.
- `git diff --check`
  - Clean.
- `bun audit`
  - No vulnerabilities found.
- `cargo audit --file agent/Cargo.lock`
  - Scanned 196 crate dependencies with no vulnerabilities reported.
- Live embedded dashboard smoke on `http://127.0.0.1:4274`
  - `./tinytop status`: reported `rust collector-dashboard-daemon v0.1.23 (embedded dashboard)`.
  - `/api/version`: returned Rust `0.1.23` embedded dashboard identity.
  - `/`: contains `id="settings-dialog"`, `id="settings-open-button"`, `This Browser`, and `This Daemon`.
  - `/`: does not contain the old inline `<section class="panel settings-panel" id="settings">` or `href="#settings"`.
  - `/app.js`: contains `openSettingsDialog`, `closeSettingsDialog`, and dialog event wiring.
  - `/api/settings`: returned SQLite-backed daemon defaults.
- Rendered browser smoke:
  - `bun ./node_modules/.bin/playwright test tinytop-settings-dialog-smoke.spec.js --reporter=line` from `/tmp/tinytop-pw`
  - Result: `2 passed`.
  - Covered desktop dialog open, mobile dialog open, no page errors, and no settings-fieldset horizontal overflow.
  - Screenshots saved outside the repo at `/tmp/tinytop-settings-dialog-desktop.png` and `/tmp/tinytop-settings-dialog-mobile.png`.

## Verification Evidence From v0.1.24 Load Overview Gauge

- Red/green focused tests:
  - Red: `bun test tests/dashboard-overview.test.ts` failed before implementation because `load-gauge` and renderer bindings did not exist.
  - Green: `bun test tests/dashboard-overview.test.ts tests/dashboard-assets.test.ts`: `4 pass`, `0 fail`, `25 expect() calls`.
- Focused regression set:
  - `bun test tests/dashboard-overview.test.ts tests/dashboard-assets.test.ts tests/dashboard-timeline.test.ts`: `9 pass`, `0 fail`, `44 expect() calls`.
- `./tinytop check`
  - Bun tests: `65 pass`, `0 fail`, `251 expect() calls`.
  - `src/server.ts --check`: `status: ok`.
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB.
  - Rust fmt check and workspace tests passed.
  - Browser bundle built `legacy/dashboard/app.js` successfully.
- `./tinytop rust build`
  - Built `agent/target/release/tinytop-agent` with the embedded `0.1.24` dashboard assets.
- `diff -qr agent/assets/dashboard legacy/dashboard`
  - No differences.
- `git diff --check`
  - Clean.
- `bun audit`
  - No vulnerabilities found.
- `cargo audit --file agent/Cargo.lock`
  - Scanned 196 crate dependencies with no vulnerabilities reported.
- Live embedded dashboard smoke on `http://127.0.0.1:4274`
  - `./tinytop status`: reported `rust collector-dashboard-daemon v0.1.24 (embedded dashboard)`.
  - `/api/version`: returned Rust `0.1.24` embedded dashboard identity.
  - `/`: contains `id="load-gauge"`, `id="load-value"`, `id="load-capacity"`, and `id="load-spark"`.
  - `/app.js`: contains `loadPercent`, `setGauge(elements.loadGauge, loadPressure)`, and `drawSparkline(elements.loadSpark, state.history.load, ...)`.
  - `/api/snapshot`: returned live load averages and CPU core count.
- Rendered browser smoke:
  - `bun ./node_modules/.bin/playwright test tinytop-load-gauge-smoke.spec.js --reporter=line` from `/tmp/tinytop-pw`
  - Result: `2 passed`.
  - Covered desktop Load gauge rendering, mobile Load gauge rendering, four overview cards, bounded gauge percentage, no page errors, and no mobile horizontal overflow.
  - Screenshots saved outside the repo at `/tmp/tinytop-load-gauge-desktop.png` and `/tmp/tinytop-load-gauge-mobile.png`.

## Verification Evidence From v0.1.25 Dashboard Operator Console And Retention

- Focused dashboard tests:
  - `bun test tests/dashboard-settings.test.ts tests/dashboard-operator-alert.test.ts tests/dashboard-timeline.test.ts tests/dashboard-process-filesystem.test.ts`: `16 pass`, `0 fail`, `112 expect() calls`.
- Focused asset/overview/dialog/server checks:
  - `bun test tests/dashboard-assets.test.ts tests/dashboard-overview.test.ts tests/webui-dialogs.test.ts`: `7 pass`, `0 fail`, `39 expect() calls`.
  - `bun test tests/server.test.ts`: `8 pass`, `0 fail`, `39 expect() calls`.
- JavaScript syntax:
  - `node --check legacy/dashboard/app.js`
  - `node --check agent/assets/dashboard/app.js`
- Rust focused checks already run during implementation:
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-store sqlite_store_persists_load_and_critical_thresholds`: passed.
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-store sqlite_store_prunes_raw_history_by_cutoff`: passed.
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-store sqlite_store_tracks_one_minute_rollups_and_coverage`: passed.
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_exposes_history_coverage_api`: passed.
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-store`: `7 pass`, `0 fail`.
- Full command-center verification:
  - `./tinytop check`: Bun tests `73 pass`, `0 fail`, `329 expect() calls`; Rust workspace tests passed; browser bundle built.
- Additional checks:
  - `git diff --check`: clean.
  - `diff -qr agent/assets/dashboard legacy/dashboard`: no differences.
  - `bun build legacy/dashboard/app.js --target=browser --outdir=/tmp/tinytop-dashboard-operator-console-check`: built `app.js`.
  - `bun audit`: no vulnerabilities found.
  - `cargo audit --file agent/Cargo.lock`: scanned 196 crate dependencies with no vulnerabilities reported.
- Release build:
  - `./tinytop rust build`: built `/home/michel/projects/tinytop/agent/target/release/tinytop-agent` with embedded v0.1.25 dashboard assets.
- Live embedded dashboard smoke on `http://127.0.0.1:4274`
  - `./tinytop status`: reported `rust collector-dashboard-daemon v0.1.25 (embedded dashboard)`.
  - `/health`: returned `ok`.
  - `/api/version`: returned Rust `0.1.25` embedded dashboard identity.
  - `/api/history/coverage`: returned coverage metadata with `retentionHours`, `rollupRetentionDays`, `rollupBucketCount`, and `databaseBytes`.
  - `/api/settings`: returned expanded CPU/RAM/disk/load/pressure warn and critical thresholds plus enabled sections.
  - `/`: contains `id="operator-status"`, `id="timeline-rail"`, `id="root-filesystem-card"`, `id="process-search"`, and `id="daemon-cpu-critical"`.
  - `/app.js`: contains `fetch("/api/history/coverage"`, `computeSnapshotStatus`, `drawTimelineRail`, `sortProcesses`, and `filterFilesystems`.
- Rendered browser smoke through Playwright MCP:
  - Desktop viewport `1440x980`: title `TinyTop`, operator strip rendered `Critical` from real root disk pressure, timeline rail `1006x96`, history coverage rendered, process search/root card present, no horizontal overflow.
  - Mobile viewport `390x844`: timeline rail `332x96`, process search/root card present, no horizontal overflow.
  - Screenshots were generated during the Playwright MCP smoke run and then left out of the repo as generated artifacts.

## Verification Evidence From v0.1.26 Native Dropdown Contrast

- Red regression test before CSS fix:
  - `bun test tests/dashboard-settings.test.ts`: failed because `.settings-group select option` styling was absent.
- Focused green checks:
  - `bun test tests/dashboard-settings.test.ts tests/dashboard-assets.test.ts`: `7 pass`, `0 fail`, `65 expect() calls`.
- Full command-center verification:
  - `./tinytop check`: Bun tests `74 pass`, `0 fail`, `339 expect() calls`; legacy server and collector checks returned JSON `ok`; Rust fmt check passed; Rust workspace tests passed; browser bundle built.
- Additional checks:
  - `git diff --check`: clean.
  - `diff -qr legacy/dashboard agent/assets/dashboard`: no differences.
  - `bun audit`: no vulnerabilities found.
  - `cargo audit --file agent/Cargo.lock`: scanned 196 crate dependencies with exit code 0.
- Release build:
  - `./tinytop rust build`: built `/home/michel/projects/tinytop/agent/target/release/tinytop-agent` with embedded v0.1.26 dashboard assets.
  - Release binary SHA-256: `4a3d5b010f1ba3d0e7684ded17eeeb218957a73e4f2314e3e908e6ab7d185556`.
- Live embedded dashboard smoke on `http://127.0.0.1:4274`:
  - `./tinytop status`: reported `rust collector-dashboard-daemon v0.1.26 (embedded dashboard)`.
  - `/health`: returned `ok`.
  - `/api/version`: returned Rust `0.1.26` embedded dashboard identity.
  - `/styles.css`: contains the Ember option selector with `background: #1c1110` and `color: #fff7ed`.
  - `/api/settings`: returned daemon defaults and enabled dashboard sections.
- Rendered browser smoke through Playwright MCP:
  - Opened Settings in Ember and verified settings select options resolve to `rgb(28, 17, 16)` background and `rgb(255, 247, 237)` text.
  - Verified process density options resolve to the same Ember option colors.
  - Screenshot saved outside the repo as `tinytop-v0.1.26-settings-ember.png`.

## Verification Evidence From v0.1.27 Dashboard Operator V2

- Full command-center verification:
  - `./tinytop check`: Bun tests `75 pass`, `0 fail`, `383 expect() calls`; legacy server and collector checks returned JSON `ok`; Rust fmt check passed; Rust workspace tests passed; browser bundle built.
- Direct verification:
  - `bun test`: `75 pass`, `0 fail`, `383 expect() calls`.
  - `cargo test --manifest-path agent/Cargo.toml --workspace`: Rust workspace tests passed.
  - `node --check legacy/dashboard/app.js` and `node --check agent/assets/dashboard/app.js`: clean.
  - `diff -qr agent/assets/dashboard legacy/dashboard`: no differences.
  - `git diff --check`: clean.
  - `cargo clippy --manifest-path agent/Cargo.toml --workspace --all-targets -- -D warnings`: clean.
- Audits:
  - `bun audit`: no vulnerabilities found.
  - `cargo audit --file agent/Cargo.lock`: scanned 196 crate dependencies with exit code 0.
- Platform collector checks:
  - `cargo check --manifest-path agent/Cargo.toml -p tinytop-collectors --target x86_64-pc-windows-gnu --no-default-features --features windows-collector`: passed.
  - `cargo check --manifest-path agent/Cargo.toml -p tinytop-collectors --target x86_64-apple-darwin --no-default-features --features macos-collector`: blocked because the local Rust toolchain is missing the `x86_64-apple-darwin` target.
- Release build:
  - `./tinytop rust build`: built `/home/michel/projects/tinytop/agent/target/release/tinytop-agent` with embedded v0.1.27 dashboard assets.
  - Release binary SHA-256: `9984f67d68368e2613cf4b773c325f920cbde5984cdfcc2daf64164c1d0ea362`.
- Live embedded dashboard smoke on `http://127.0.0.1:4274`:
  - `./tinytop status`: reported `rust collector-dashboard-daemon v0.1.27 (embedded dashboard)`.
  - `/health`: returned `ok`.
  - `/api/version`: returned Rust `0.1.27` embedded dashboard identity.
  - `/api/settings`: returned daemon defaults including `targetDatabaseBytes`.
  - `/api/history/coverage`: returned coverage metadata including `targetDatabaseBytes`, `databaseBudgetPercent`, and rollup coverage timestamps.
  - `/api/history/points?window=24h&mode=auto&limit=5`: returned chart points.
  - `/api/history/markers?since_ms=0&limit=10`: returned a `daemonStart` marker for v0.1.27.
  - `/`: contains `operator-detail-dialog`, `process-detail-dialog`, and `settings-dialog`.
  - `/app.js`: contains `openOperatorDetailDrawer`, `targetDatabaseBytes`, `fetchHistoryMarkers`, and `processTrendForPid`.
- Rendered browser smoke through Playwright MCP:
  - Page title was `TinyTop`, the operator strip and major dashboard sections were present in the accessibility tree, and the temporary viewport screenshot was removed from the repo after inspection.

## Verification Evidence From v0.1.28 SVG Favicon

- Red checks before implementation:
  - `bun test tests/dashboard-assets.test.ts`: failed because `favicon.svg` did not exist in either dashboard tree.
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_exposes_embedded_dashboard_without_public_dir`: failed because `/favicon.svg` was not served as SVG.
- Focused green checks:
  - `bun test tests/dashboard-assets.test.ts`: `3 pass`, `0 fail`, `16 expect() calls`.
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_exposes_embedded_dashboard_without_public_dir`: `1 pass`, `0 fail`.
  - `diff -qr agent/assets/dashboard legacy/dashboard`: no differences.
- Full command-center verification:
  - `./tinytop check`: Bun tests `76 pass`, `0 fail`, `387 expect() calls`; legacy server and collector checks returned JSON `ok`; Rust fmt check passed; Rust workspace tests passed; browser bundle built.
- Additional checks:
  - `cargo clippy --manifest-path agent/Cargo.toml --workspace --all-targets -- -D warnings`: clean.
  - `git diff --check`: clean.
  - `bun audit`: no vulnerabilities found.
  - `cargo audit --file agent/Cargo.lock`: scanned 196 crate dependencies with exit code 0.
- Release build:
  - `./tinytop rust build`: built `/home/michel/projects/tinytop/agent/target/release/tinytop-agent` with embedded v0.1.28 dashboard assets.
  - Release binary SHA-256: `fb7f2fa3443fa27ecb4ce02632166eef5d72e52362445b515042f8060ee5d3a5`.
- Live embedded dashboard smoke on `http://127.0.0.1:4274`:
  - `./tinytop status`: reported `rust collector-dashboard-daemon v0.1.28 (embedded dashboard)`.
  - `/api/version`: returned Rust `0.1.28` embedded dashboard identity.
  - `/favicon.svg`: returned `HTTP/1.1 200 OK` with `content-type: image/svg+xml; charset=utf-8`.
  - `/favicon.svg`: contains `<title id="title">TinyTop</title>`.

## Verification Evidence From v0.1.29 Windows Command Center And Critical Status

- Red checks before implementation:
  - `bun test tests/tinytop-powershell.test.ts tests/tinytop-script.test.ts tests/dashboard-operator-alert.test.ts`: failed because `tinytop.ps1`, target-specific `rust build --print-command`, and stronger operator status styling did not exist yet.
  - `bun test tests/dashboard-timeline.test.ts`: failed because the sidebar runtime identity still rendered the full WSL reason as prominent brand text.
- Focused green checks:
  - `bun test tests/tinytop-powershell.test.ts tests/tinytop-script.test.ts tests/dashboard-operator-alert.test.ts tests/dashboard-assets.test.ts`: `28 pass`, `0 fail`, `136 expect() calls`.
  - `bun test tests/dashboard-timeline.test.ts tests/dashboard-assets.test.ts tests/dashboard-operator-alert.test.ts`: `14 pass`, `0 fail`, `100 expect() calls`.
  - `shellcheck tinytop`: clean.
  - `diff -qr agent/assets/dashboard legacy/dashboard`: no differences.
- Full command-center verification:
  - `./tinytop check`: Bun tests `85 pass`, `0 fail`, `431 expect() calls`; legacy server and collector checks returned JSON `ok`; Rust fmt check passed; Rust workspace tests passed; browser bundle built.
- Additional checks:
  - `cargo clippy --manifest-path agent/Cargo.toml --workspace --all-targets -- -D warnings`: clean.
  - `cargo check --manifest-path agent/Cargo.toml -p tinytop-collectors --target x86_64-pc-windows-gnu --no-default-features --features windows-collector`: passed.
  - `./tinytop --plain rust build --print-command`: printed the Linux default Cargo build command.
  - `TINYTOP_RELEASE_OS=windows ./tinytop --plain rust build --print-command`: printed the Windows `windows-collector` Cargo build command.
  - `TINYTOP_RELEASE_OS=macos ./tinytop --plain rust build --print-command`: printed the macOS `macos-collector` Cargo build command.
  - `git diff --check`: clean.
  - `bun audit`: no vulnerabilities found.
  - `cargo audit --file agent/Cargo.lock`: scanned 196 crate dependencies with exit code 0.
- Release build:
  - `./tinytop rust build`: built `/home/michel/projects/tinytop/agent/target/release/tinytop-agent` with embedded v0.1.29 dashboard assets.
  - Release binary SHA-256: `a801f2f24006aebbbfcc549f1cce164b4a9214d2692f7e12cad15ba2a11d09c2`.
- Live embedded dashboard smoke on `http://127.0.0.1:4274`:
  - `./tinytop status`: reported `rust collector-dashboard-daemon v0.1.29 (embedded dashboard)`.
  - `/health`: returned `ok`.
  - `/api/version`: returned Rust `0.1.29` embedded dashboard identity.
  - `/app.js`: contains `formatRuntimeSummary` and `runtimeReason`.
  - `/styles.css`: contains `.runtime-pill`, `.runtime-reason`, and Critical operator status selectors.
  - `/api/history/coverage`: returned sample coverage, DB size, and budget metadata.
- Rendered browser smoke through Playwright MCP:
  - Page title was `TinyTop`.
  - Sidebar runtime summary rendered as `WSL`.
  - Runtime reason rendered as clamped detail with title `kernel release/version contains Microsoft WSL markers`.
  - Runtime reason clamp was `2`.

## Useful Commands

```bash
cd /home/michel/projects/tinytop
./tinytop help
./tinytop start
curl -fsS http://127.0.0.1:4274/health
curl -fsS http://127.0.0.1:4274/api/version
curl -fsS http://127.0.0.1:4274/api/settings
curl -fsS http://127.0.0.1:4274/api/history/coverage
curl -fsSI http://127.0.0.1:4274/favicon.svg
curl -fsS 'http://127.0.0.1:4274/api/history/points?window=24h&mode=auto&limit=5'
curl -fsS 'http://127.0.0.1:4274/api/history/markers?since_ms=0&limit=10'
curl -fsS http://127.0.0.1:4274/api/snapshot
curl -fsS 'http://127.0.0.1:4274/api/history?limit=5'
./tinytop check
```

## Next Useful Work

- Add wider rollup tiers if 30d browsing needs fewer points than one-minute buckets.
- Add normalized child tables for process/filesystem history if the UI starts querying those independently.
- Add live macOS and Windows CI/host verification plus release packaging.
- Add a collector/daemon health detail drawer if history or snapshot APIs degrade.

## Notes For Resuming

- TinyTop Rust daemon PID `1827235` is running at this handoff refresh. It was started with `setsid ./tinytop start`; stop it with `./tinytop stop` if you need the default dashboard port free.
- WSL user systemd was previously unavailable in this environment, so foreground Rust daemon mode is the known-working path.
- The dashboard is loopback-only by design.
- `legacy/dashboard/vendor/echarts.min.js` and `agent/assets/dashboard/vendor/echarts.min.js` are vendored third-party code and should stay excluded from local UI policy scans.
