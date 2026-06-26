# TinyTop Handoff

Date: 2026-06-26 16:56 Asia/Jerusalem

## Current Repo State

- Repo: `/home/michel/projects/tinytop`
- Branch: `main`
- Remote: `origin` at `git@github.com:michelabboud/tinytop.git`
- Current checkpoint version: `0.1.24`
- Version files: `VERSION`, `package.json`, and `tinytop` all read `0.1.24`
- Rust crate package versions under `agent/crates/*/Cargo.toml` read `0.1.24`
- The `v0.1.24` checkpoint adds a Load overview gauge while keeping the Rust embedded dashboard and legacy Bun dashboard assets byte-identical.

## Runtime State

- Dashboard URL when running: `http://127.0.0.1:4274`
- Health endpoint when running: `http://127.0.0.1:4274/health`
- Version endpoint when running: `http://127.0.0.1:4274/api/version`
- Settings endpoint when running: `http://127.0.0.1:4274/api/settings`
- Health status at handoff refresh time: running
- Runtime identity at handoff refresh time: `rust collector-dashboard-daemon v0.1.24 (embedded dashboard)`
- Dashboard port `127.0.0.1:4274`: in use by `tinytop-agent serve`
- Legacy Bun collector port `127.0.0.1:4276`: free
- Active TinyTop foreground process at handoff refresh time: Rust daemon PID `1283644`
- Foreground daemon was started detached with `setsid ./tinytop start`, which auto-selected Rust.

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

### Release Binary Asset Check

- `v0.1.18` release assets were updated with `tinytop-agent-linux-x86_64` and its `.sha256` file.
- A temporary-HOME install test confirmed `./tinytop rust install-binary` downloaded and ran the release binary.

### Daemon Start

- Started the Rust daemon with:

  ```bash
  ./tinytop rust serve
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

## Useful Commands

```bash
cd /home/michel/projects/tinytop
./tinytop help
./tinytop start
curl -fsS http://127.0.0.1:4274/health
curl -fsS http://127.0.0.1:4274/api/version
curl -fsS http://127.0.0.1:4274/api/settings
curl -fsS http://127.0.0.1:4274/api/snapshot
curl -fsS 'http://127.0.0.1:4274/api/history?limit=5'
./tinytop check
```

## Next Useful Work

- Add SQLite raw-history retention with a configurable 24 to 72 hour default.
- Add one-minute rollups for longer history ranges.
- Apply saved settings to daemon-side collection and retention enforcement where appropriate.
- Add a collector/daemon health indicator in the UI if history or snapshot APIs degrade.
- Add native Windows and macOS collectors when the project moves beyond Linux/WSL.

## Notes For Resuming

- TinyTop Rust daemon PID `1283644` is running at this handoff refresh. It was started with `setsid ./tinytop start`; stop it with `./tinytop stop` or `kill 1283644` if you need the default dashboard port free.
- WSL user systemd was previously unavailable in this environment, so foreground Rust daemon mode is the known-working path.
- The dashboard is loopback-only by design.
- `legacy/dashboard/vendor/echarts.min.js` and `agent/assets/dashboard/vendor/echarts.min.js` are vendored third-party code and should stay excluded from local UI policy scans.
