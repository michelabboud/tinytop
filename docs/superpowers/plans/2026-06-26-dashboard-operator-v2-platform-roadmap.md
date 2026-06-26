# Dashboard Operator V2 And Platform Roadmap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Build the v0.1.27 operator-console phase: alert details, rollup-backed long history, DB budget settings, settings polish, richer process details, native collector scaffolding for macOS/Windows, and the stale handoff PID fix.

**Architecture:** Keep the Rust collector/dashboard daemon as the default runtime. Preserve the existing raw `/api/history` contract, add additive history point and marker APIs for long-range charting, and evolve the static dashboard in both `legacy/dashboard/` and `agent/assets/dashboard/`. Linux remains the reference collector; macOS and Windows collectors start as `cfg`-gated Rust modules using existing `sysinfo` data only.

**Tech Stack:** Rust 2024, Axum 0.8, SQLx SQLite, sysinfo/procfs, Bun tests, static HTML/CSS/vanilla JS dashboard, Apache ECharts.

## Global Constraints

- Product version for this phase is `0.1.27`.
- No new third-party dependencies unless a later task explicitly records dependency vetting under `docs/reports/`.
- `legacy/dashboard/` and `agent/assets/dashboard/` must remain byte-identical after every dashboard asset change.
- Existing `/api/history`, `/api/settings`, `/api/snapshot`, `/api/version`, `/version`, `/snapshot/latest`, and `/snapshot/collect` contracts remain backward-compatible.
- SQLite settings own daemon-wide defaults; browser `localStorage` owns per-browser display preferences.
- Linux/WSL remains the verified runtime. macOS/Windows collectors are `cfg`-gated starter modules and must not break Linux builds.
- Use TDD for each behavior change: write focused failing tests, run them red, implement, run them green.
- Use `./tinytop` for runtime stop/start/status smoke tests.

---

## File Structure

- Modify `agent/crates/tinytop-store/src/lib.rs`: settings fields, event table, history point/marker structs, rollup/raw point readers, marker reader, coverage budget output.
- Modify `agent/crates/tinytop-agent/src/writer.rs`: `/api/history/points`, `/api/history/markers`, event recording on daemon start/settings save, native collector alias.
- Modify `agent/crates/tinytop-types/src/lib.rs`: optional process parent/start fields.
- Modify `agent/crates/tinytop-collectors/src/lib.rs`: native collector aliases and platform module cfg.
- Modify `agent/crates/tinytop-collectors/src/linux.rs`: richer process metadata while keeping Linux parser fixtures stable.
- Create `agent/crates/tinytop-collectors/src/macos.rs`: macOS starter collector using `sysinfo`, compiled only on macOS.
- Create `agent/crates/tinytop-collectors/src/windows.rs`: Windows starter collector using `sysinfo`, compiled only on Windows.
- Modify `legacy/dashboard/index.html`: alert detail dialog, longer history controls, history marker/coverage budget UI, settings preset/reset/effective controls, process detail V2 controls.
- Modify `legacy/dashboard/app.js`: long-range point loading, marker rendering, alert drawer calculations, settings validation/presets/reset/dirty guard, process detail V2 rendering/redaction/copy.
- Modify `legacy/dashboard/styles.css`: drawer/dialog, markers, validation, budget, process detail V2 styles.
- Mirror dashboard assets into `agent/assets/dashboard/`.
- Modify tests under `tests/` and `agent/crates/*/tests/` for every slice.
- Update `README.md`, `GUIDE.md`, `INSTALL.md`, `ARCHITECTURE.md`, `CHANGELOG.md`, `PROGRESS.md`, `HANDOFF.md`, `docs/guides/API.md`, and `docs/guides/OPERATIONS.md`.
- Add `docs/reports/2026-06-26-dashboard-operator-v2-platform-roadmap.md`.
- Add ADR `docs/adr/0009-additive-history-points-and-markers-api.md` and ADR `docs/adr/0010-feature-gated-native-platform-collectors.md`, then index them.

---

### Task 1: Storage Settings, History Points, Markers, And Coverage Budget

**Files:**
- Modify: `agent/crates/tinytop-store/src/lib.rs`
- Modify: `agent/crates/tinytop-store/tests/sqlite_history_store.rs`

**Interfaces:**
- Produces `DashboardSettings.target_database_bytes: i64` serialized as `targetDatabaseBytes`.
- Produces `HistoryPoint`, `HistoryPointSource`, `HistoryPointsQuery`, `HistoryMarker`, `HistoryMarkerType`.
- Produces store methods:
  - `read_history_points(&self, query: HistoryPointsQuery) -> Result<Vec<HistoryPoint>, StoreError>`
  - `record_event(&self, occurred_at_ms: i64, event_type: HistoryMarkerType, label: &str, details_json: serde_json::Value) -> Result<(), StoreError>`
  - `read_history_markers(&self, query: HistoryQuery, expected_gap_ms: i64) -> Result<Vec<HistoryMarker>, StoreError>`
- Extends `HistoryCoverage` with `target_database_bytes` and `database_budget_percent`.

- [x] **Step 1: Add failing settings budget test**

Add a test that `DashboardSettings::default().target_database_bytes == 134_217_728`, that persisted settings keep a custom `target_database_bytes`, and that validation rejects values below `1_048_576` or above `10_737_418_240`.

Run:

```bash
cargo test --manifest-path agent/Cargo.toml -p tinytop-store sqlite_store_persists_database_budget_settings
```

Expected red: missing field/method or assertion failure.

- [x] **Step 2: Implement settings budget field**

Add the field with serde camelCase, default `128 * 1024 * 1024`, validation range `1 MiB..=10 GiB`, and coverage budget percentage:

```rust
pub target_database_bytes: i64,
```

Green command:

```bash
cargo test --manifest-path agent/Cargo.toml -p tinytop-store sqlite_store_persists_database_budget_settings
```

- [x] **Step 3: Add failing history points and markers tests**

Add tests that insert three raw samples across two one-minute buckets, call `read_history_points` with `Raw`, `Rollup`, and `Auto`, and assert sorted points with `source`, `sample_count`, CPU/RAM/swap/load/root values. Add marker tests that record `DaemonStart` and `SettingsChange` and infer a coverage gap when adjacent points exceed `expected_gap_ms`.

Run:

```bash
cargo test --manifest-path agent/Cargo.toml -p tinytop-store history_points
cargo test --manifest-path agent/Cargo.toml -p tinytop-store history_markers
```

Expected red: missing types/methods.

- [x] **Step 4: Implement point and marker storage**

Create `app_events` table:

```sql
CREATE TABLE IF NOT EXISTS app_events (
  event_id INTEGER PRIMARY KEY,
  occurred_at_ms INTEGER NOT NULL,
  event_type TEXT NOT NULL,
  label TEXT NOT NULL,
  details_json TEXT NOT NULL
)
```

Add `idx_app_events_occurred_at`. Read raw points from `metric_samples`, rollup points from `metric_rollups_1m`, and auto-select rollups when requested window is longer than 24h. Markers include stored events and inferred `coverageGap` rows.

Green command:

```bash
cargo test --manifest-path agent/Cargo.toml -p tinytop-store
```

---

### Task 2: Rust API Endpoints And Daemon Event Recording

**Files:**
- Modify: `agent/crates/tinytop-agent/src/writer.rs`
- Modify: `agent/crates/tinytop-agent/tests/serve_contract.rs`

**Interfaces:**
- Consumes Task 1 store methods.
- Produces:
  - `GET /api/history/points?since_ms=&until_ms=&limit=&source=auto|raw|rollup`
  - `GET /api/history/markers?since_ms=&until_ms=&expected_gap_ms=`
  - daemon start event label `Daemon started`
  - settings change event label `Settings changed`

- [x] **Step 1: Add failing serve contract tests**

Add tests asserting `/api/history/points` returns `points` and `/api/history/markers` returns `markers`, and that `/api/settings` includes `targetDatabaseBytes`.

Run:

```bash
cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract history_points
cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract history_markers
cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_persists_dashboard_settings_api
```

Expected red: routes/fields absent.

- [x] **Step 2: Implement additive endpoints**

Add route handlers that parse query params, clamp limits to `10_000`, call store methods, return `no_store(Json(...))`, and preserve `/api/history` raw sample behavior.

Green command:

```bash
cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract
```

- [x] **Step 3: Record daemon and settings events**

Record `DaemonStart` once during `serve` startup after the first collection succeeds. Record `SettingsChange` after `put_settings` succeeds, with details containing the changed keys only.

Green command:

```bash
cargo test --manifest-path agent/Cargo.toml --workspace
```

---

### Task 3: Platform Collector Scaffolding And Process Metadata

**Files:**
- Modify: `agent/crates/tinytop-types/src/lib.rs`
- Modify: `agent/crates/tinytop-collectors/Cargo.toml`
- Modify: `agent/crates/tinytop-collectors/src/lib.rs`
- Modify: `agent/crates/tinytop-collectors/src/linux.rs`
- Create: `agent/crates/tinytop-collectors/src/macos.rs`
- Create: `agent/crates/tinytop-collectors/src/windows.rs`
- Modify: `agent/crates/tinytop-collectors/tests/linux_collector.rs`
- Modify: `agent/crates/tinytop-agent/src/writer.rs`

**Interfaces:**
- Produces optional process fields `parentPid` and `startedAt`.
- Produces `tinytop_collectors::NativeCollector` alias selected by `target_os`.
- Keeps `linux::LinuxCollector` available on Linux and tests.

- [x] **Step 1: Add failing process metadata tests**

Extend Linux parser fixture tests to assert parent PID and start time are parsed or surfaced when available. Add serialization test under `tinytop-store` or `tinytop-types` that optional fields serialize as `parentPid` and `startedAt`.

Run:

```bash
cargo test --manifest-path agent/Cargo.toml -p tinytop-collectors linux_collector
cargo test --manifest-path agent/Cargo.toml -p tinytop-store history_sample_serializes_with_dashboard_field_names
```

Expected red: missing optional fields.

- [x] **Step 2: Implement Linux process metadata**

Add optional fields to `ProcessSnapshot`, enrich `sysinfo_process_text` and `parse_processes`, and keep legacy five-column process text accepted with `None` fields.

Green command:

```bash
cargo test --manifest-path agent/Cargo.toml -p tinytop-collectors
```

- [x] **Step 3: Add cfg-gated native collectors**

Move `procfs` dependency behind Linux target if needed. Add `NativeCollector` aliases:

```rust
#[cfg(target_os = "linux")]
pub type NativeCollector = linux::LinuxCollector;
#[cfg(target_os = "macos")]
pub type NativeCollector = macos::MacOsCollector;
#[cfg(target_os = "windows")]
pub type NativeCollector = windows::WindowsCollector;
```

macOS/Windows modules use `sysinfo` to collect identity, CPU, memory, swap, disks, processes, and a load-equivalent load snapshot. PSI is empty on those platforms.

Green command:

```bash
cargo test --manifest-path agent/Cargo.toml -p tinytop-collectors
cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_exposes_version_identity
```

---

### Task 4: Dashboard Long Timeline, Markers, Coverage Budget, And Alert Detail Drawer

**Files:**
- Modify: `legacy/dashboard/index.html`
- Modify: `legacy/dashboard/app.js`
- Modify: `legacy/dashboard/styles.css`
- Mirror: `agent/assets/dashboard/index.html`
- Mirror: `agent/assets/dashboard/app.js`
- Mirror: `agent/assets/dashboard/styles.css`
- Modify: `tests/dashboard-timeline.test.ts`
- Modify: `tests/dashboard-operator-alert.test.ts`

**Interfaces:**
- Consumes `/api/history/points`, `/api/history/markers`, and expanded `/api/history/coverage`.
- Produces `#operator-detail-dialog`, `#history-markers`, 7d/30d history controls, coverage budget display.

- [x] **Step 1: Add failing dashboard tests**

Assert the HTML contains 7d/30d buttons, marker container, coverage budget fields, and alert detail dialog IDs. Assert app code fetches `/api/history/points` and `/api/history/markers`, renders markers, computes alert details and trend text.

Run:

```bash
bun test tests/dashboard-timeline.test.ts tests/dashboard-operator-alert.test.ts
```

Expected red: IDs/functions/routes absent.

- [x] **Step 2: Implement long-range point loading**

Add `7d` and `30d` to `HISTORY_WINDOWS` with `source: "rollup"`. Fetch points for rollup windows, hydrate chart arrays from points, keep raw snapshots for live/short windows, and show rollup detail values when no full snapshot exists.

Green command:

```bash
bun test tests/dashboard-timeline.test.ts
```

- [x] **Step 3: Implement markers and coverage budget UI**

Fetch markers with the selected window, draw marker ticks on the timeline rail, list marker chips under coverage, and display DB budget percent against `targetDatabaseBytes`.

Green command:

```bash
bun test tests/dashboard-timeline.test.ts
```

- [x] **Step 4: Implement alert detail dialog**

Make `#operator-status` a button-like region with keyboard support. Open a dialog showing state, worst offender, current value, threshold, sample age, recent trend, and recent changes/markers.

Green command:

```bash
bun test tests/dashboard-operator-alert.test.ts
```

---

### Task 5: Settings Polish And Process Detail Drawer V2

**Files:**
- Modify: `legacy/dashboard/index.html`
- Modify: `legacy/dashboard/app.js`
- Modify: `legacy/dashboard/styles.css`
- Mirror: `agent/assets/dashboard/index.html`
- Mirror: `agent/assets/dashboard/app.js`
- Mirror: `agent/assets/dashboard/styles.css`
- Modify: `tests/dashboard-settings.test.ts`
- Modify: `tests/dashboard-process-filesystem.test.ts`

**Interfaces:**
- Produces settings preset buttons for `conservative`, `balanced`, `noisy`.
- Produces reset-to-defaults, unsaved-change confirmation, inline validation/error summary, and effective settings readout.
- Produces process detail V2 with `parentPid`, `startedAt`, CPU/RAM trend, RSS, redacted command, and copy-safe command button.

- [x] **Step 1: Add failing settings polish tests**

Assert HTML contains preset buttons, reset button, effective settings readout, validation summary, and DB budget input. Assert app code has `validateDaemonSettingsForm`, `applyThresholdPreset`, `resetDaemonSettingsForm`, `settingsFormIsDirty`, and close confirmation wiring.

Run:

```bash
bun test tests/dashboard-settings.test.ts
```

Expected red: IDs/functions absent.

- [x] **Step 2: Implement settings validation/presets/reset/dirty guard**

Validate numeric ranges and warn/critical ordering before PUT. Use confirmation dialog before closing dirty settings. Reset fills defaults. Effective settings readout summarizes poll interval, raw/rollup retention, DB budget, top process count, and threshold preset match.

Green command:

```bash
bun test tests/dashboard-settings.test.ts
```

- [x] **Step 3: Add failing process drawer V2 tests**

Assert HTML/app contain parent PID/start time fields, redacted command, copy command button, and trend rendering.

Run:

```bash
bun test tests/dashboard-process-filesystem.test.ts
```

Expected red: fields/functions absent.

- [x] **Step 4: Implement process drawer V2**

Render optional `parentPid` and `startedAt`. Compute CPU/RAM trend by matching PID across loaded snapshots. Redact commands when daemon redaction is enabled or before copying: mask `--password`, `--token`, `--secret`, `--key`, and `Authorization`-like values. Copy via `navigator.clipboard.writeText` when available and show status feedback.

Green command:

```bash
bun test tests/dashboard-process-filesystem.test.ts
```

---

### Task 6: Docs, Version, Runtime Smoke, Release

**Files:**
- Modify: `VERSION`
- Modify: `package.json`
- Modify: `tinytop`
- Modify: `agent/crates/*/Cargo.toml`
- Modify: `agent/Cargo.lock`
- Modify: root docs and guides listed in File Structure
- Add: `docs/reports/2026-06-26-dashboard-operator-v2-platform-roadmap.md`
- Add: `docs/adr/0009-additive-history-points-and-markers-api.md`
- Add: `docs/adr/0010-feature-gated-native-platform-collectors.md`

**Interfaces:**
- Produces released checkpoint `v0.1.27`.

- [x] **Step 1: Update docs and ADR**

Document new APIs, dashboard behavior, settings ownership, platform collector limits, and verification evidence. Fix stale bottom `HANDOFF.md` PID note.

- [x] **Step 2: Bump versions**

Set product, package, script, and Rust crate versions to `0.1.27`, then refresh `Cargo.lock`.

- [x] **Step 3: Run full verification**

Run:

```bash
diff -qr legacy/dashboard agent/assets/dashboard
git diff --check
./tinytop check
bun audit
cargo audit --file agent/Cargo.lock
./tinytop rust build
```

- [x] **Step 4: Live smoke with `tinytop`**

Run:

```bash
./tinytop stop
setsid ./tinytop start > /tmp/tinytop-v0.1.27.log 2>&1 &
./tinytop status
curl -fsS http://127.0.0.1:4274/api/version
curl -fsS http://127.0.0.1:4274/api/history/points
curl -fsS http://127.0.0.1:4274/api/history/markers
curl -fsS http://127.0.0.1:4274/api/history/coverage
```

- [x] **Step 5: Browser smoke**

Use Playwright to verify desktop/mobile render, no horizontal overflow, alert drawer opens, settings validation appears, long timeline controls render, and process detail V2 opens.

- [x] **Step 6: Commit, tag, push, release**

Commit with Michel author and `Co-Authored-By: Codex <noreply@openai.com>`, tag `v0.1.27`, push `main` and tag, create GitHub release, upload Linux x86_64 binary and checksum, and verify release assets.

---

## Self-Review

- Spec coverage: all seven approved suggestions map to Tasks 1-6.
- Placeholder scan: no task uses TBD/TODO language; each task has concrete files, commands, and expected red/green outcomes.
- Type consistency: frontend consumes additive Rust `points`, `markers`, and coverage budget fields; raw `/api/history` remains unchanged.
- Scope check: this is a large but coherent operator-console phase. Platform collectors are intentionally starter modules, not full parity certification.
