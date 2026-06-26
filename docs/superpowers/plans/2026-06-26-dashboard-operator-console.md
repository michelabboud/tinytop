# Dashboard Operator Console Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Turn the embedded TinyTop dashboard into a more useful local operator console with a real timeline rail, visible saved thresholds, operator state, better process/filesystem controls, and enforced SQLite history retention.

**Architecture:** Keep the current Rust collector/dashboard daemon and embedded vanilla dashboard assets. Add backend retention, one-minute rollups, and history coverage to `tinytop-store`/`tinytop-agent`; add UI behavior in the shared dashboard assets and mirror byte-identically between `agent/assets/dashboard/` and `legacy/dashboard/`.

**Tech Stack:** Rust 2024, Axum, SQLx SQLite, Bun tests, vanilla HTML/CSS/JavaScript, Apache ECharts, Playwright smoke verification.

## Global Constraints

- Default runtime remains the single Rust collector/dashboard daemon.
- Native macOS and Windows collectors are not part of this phase; TinyTop remains Linux/WSL for real telemetry.
- No Svelte or new frontend framework in this phase.
- No new third-party dependencies.
- `agent/assets/dashboard/` and `legacy/dashboard/` must stay byte-identical for shared dashboard files.
- Browser-local preferences stay in `localStorage`.
- Daemon-wide defaults and collection/history behavior stay in SQLite through the Rust daemon.
- Existing `/api/snapshot`, `/api/history`, `/api/settings`, and `/api/version` contracts remain backward-compatible.
- Use TDD: write focused failing tests, run them red, implement, run green.
- Update `README.md`, `GUIDE.md`, `ARCHITECTURE.md`, `PROGRESS.md`, `CHANGELOG.md`, `HANDOFF.md`, and a report under `docs/reports/`.

---

## File Structure

- Modify `agent/crates/tinytop-store/src/lib.rs`: settings threshold expansion, raw pruning, one-minute rollup table, history coverage type and query.
- Modify `agent/crates/tinytop-store/tests/sqlite_history_store.rs`: retention, rollup, coverage, settings compatibility tests.
- Modify `agent/crates/tinytop-agent/src/writer.rs`: apply retention after collection and expose `/api/history/coverage`.
- Modify `agent/crates/tinytop-agent/tests/serve_contract.rs`: coverage API and settings shape contract.
- Modify `legacy/dashboard/index.html`, `styles.css`, `app.js`: operator operator status strip, timeline rail, settings controls, process/filesystem controls.
- Mirror the same dashboard files into `agent/assets/dashboard/`.
- Add or extend Bun static tests under `tests/`: dashboard timeline V2, settings application, operator alert, process table, filesystem filtering, history coverage.
- Update docs, `VERSION`, package manifests, lockfiles, release notes, and handoff.

---

### Task 1: History Retention, Rollups, And Coverage API

**Files:**
- Modify: `agent/crates/tinytop-store/src/lib.rs`
- Modify: `agent/crates/tinytop-store/tests/sqlite_history_store.rs`
- Modify: `agent/crates/tinytop-agent/src/writer.rs`
- Modify: `agent/crates/tinytop-agent/tests/serve_contract.rs`

**Interfaces:**
- Produces `HistoryCoverage` serialized as camelCase JSON with `sampleCount`, `oldestCapturedAtMs`, `newestCapturedAtMs`, `retentionHours`, `rollupRetentionDays`, `rollupBucketCount`, and `databaseBytes`.
- Produces store methods `history_coverage(settings)`, `prune_raw_history(cutoff_ms)`, `prune_rollups(cutoff_ms)`, and internal one-minute rollup upsert on `insert_snapshot`.
- Produces HTTP route `GET /api/history/coverage`.
- Collection loop loads SQLite settings and applies raw and rollup retention after storing each sample.

- [x] **Step 1: Write failing Rust store tests**

Add tests proving:

```rust
#[tokio::test]
async fn sqlite_store_prunes_raw_history_by_cutoff() {
    let store = SqliteHistoryStore::connect("sqlite::memory:").await.expect("store");
    store.insert_snapshot(1_000, &snapshot("2026-06-24T12:00:01Z", 10.0)).await.expect("old insert");
    store.insert_snapshot(2_000, &snapshot("2026-06-24T12:00:02Z", 20.0)).await.expect("new insert");

    let deleted = store.prune_raw_history(1_500).await.expect("prune");
    assert_eq!(deleted, 1);

    let history = store.read_history(HistoryQuery { since_ms: Some(0), until_ms: None, limit: Some(10) }).await.expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].captured_at_ms, 2_000);
}
```

```rust
#[tokio::test]
async fn sqlite_store_tracks_one_minute_rollups_and_coverage() {
    let store = SqliteHistoryStore::connect("sqlite::memory:").await.expect("store");
    store.insert_snapshot(60_100, &snapshot("2026-06-24T12:01:00Z", 10.0)).await.expect("insert one");
    store.insert_snapshot(60_900, &snapshot("2026-06-24T12:01:01Z", 30.0)).await.expect("insert two");

    let coverage = store.history_coverage(&DashboardSettings::default()).await.expect("coverage");
    assert_eq!(coverage.sample_count, 2);
    assert_eq!(coverage.rollup_bucket_count, 1);
    assert_eq!(coverage.oldest_captured_at_ms, Some(60_100));
    assert_eq!(coverage.newest_captured_at_ms, Some(60_900));
    assert!(coverage.database_bytes >= 0);
}
```

- [x] **Step 2: Run red tests**

Run:

```bash
cargo test --manifest-path agent/Cargo.toml -p tinytop-store sqlite_store_prunes_raw_history_by_cutoff
cargo test --manifest-path agent/Cargo.toml -p tinytop-store sqlite_store_tracks_one_minute_rollups_and_coverage
```

Expected: fail because the methods and rollup table do not exist yet.

- [x] **Step 3: Implement store schema and methods**

Add `metric_rollups_1m` with bucket start, sample count, averages, maxima, and first/newest sample times. Upsert the matching bucket inside `insert_snapshot`. Add pruning methods and `history_coverage`.

- [x] **Step 4: Add failing agent API test**

Add a `serve_exposes_history_coverage_api` test that starts the daemon, calls `/api/history/coverage`, and asserts `sampleCount`, `retentionHours`, `rollupRetentionDays`, `rollupBucketCount`, and `databaseBytes`.

- [x] **Step 5: Implement API and retention application**

Add `.route("/api/history/coverage", get(history_coverage))`. In `collect_and_store`, after insert, read settings and prune raw samples older than `retentionHours`; prune rollups older than `rollupRetentionDays`.

- [x] **Step 6: Run focused green tests**

Run:

```bash
cargo test --manifest-path agent/Cargo.toml -p tinytop-store
cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract
```

Expected: all focused Rust tests pass.

---

### Task 2: Visible Settings And Section Application

**Files:**
- Modify: `legacy/dashboard/index.html`
- Modify: `legacy/dashboard/styles.css`
- Modify: `legacy/dashboard/app.js`
- Mirror: `agent/assets/dashboard/index.html`
- Mirror: `agent/assets/dashboard/styles.css`
- Mirror: `agent/assets/dashboard/app.js`
- Modify: `tests/dashboard-settings.test.ts`

**Interfaces:**
- Extends daemon thresholds with load and critical thresholds while accepting old settings through JS normalization.
- Applies `enabledSections` by hiding Overview, History, Filesystem, Pressure, and Processes sections plus matching nav links.
- Adds daemon settings controls for enabled sections and load/critical thresholds.
- Keeps theme, graph, history range, visible series, process search/sort/density, filesystem filter, and last section in browser storage.

- [x] **Step 1: Write failing dashboard settings tests**

Assert that HTML contains section toggles and load/critical threshold controls, and JS contains `applyEnabledSections`, `metricStatus`, `normalizeThresholds`, `tinytop.visibleSeries`, `tinytop.processFilter`, `tinytop.processDensity`, `tinytop.filesystemShowSystem`, and `tinytop.lastSection`.

- [x] **Step 2: Run red tests**

Run:

```bash
bun test tests/dashboard-settings.test.ts
```

Expected: fail because the dashboard does not yet expose/apply those controls.

- [x] **Step 3: Implement settings normalization and UI application**

Add robust JS defaults for:

- `cpuWarn`, `cpuCritical`
- `memoryWarn`, `memoryCritical`
- `diskWarn`, `diskCritical`
- `loadWarn`, `loadCritical`
- `pressureWarn`, `pressureCritical`

Use existing warning values to derive missing critical values when old settings are loaded.

- [x] **Step 4: Add section visibility controls**

Add checkboxes in the daemon settings dialog and apply them immediately after settings load/save.

- [x] **Step 5: Run focused green tests**

Run:

```bash
bun test tests/dashboard-settings.test.ts tests/dashboard-assets.test.ts
```

Expected: all focused tests pass and mirrored assets remain byte-identical.

---

### Task 3: Operator Alert Strip And Threshold States

**Files:**
- Modify: `legacy/dashboard/index.html`
- Modify: `legacy/dashboard/styles.css`
- Modify: `legacy/dashboard/app.js`
- Mirror: `agent/assets/dashboard/index.html`
- Mirror: `agent/assets/dashboard/styles.css`
- Mirror: `agent/assets/dashboard/app.js`
- Create or modify: `tests/dashboard-operator-status.test.ts`

**Interfaces:**
- Produces operator status elements `#operator-status`, `#operator-state`, `#operator-summary`, `#operator-age`, and `#operator-offender`.
- Produces `computeSnapshotStatus(snapshot, settings, nowMs)` and `renderOperatorStatus(snapshot, stateKind)`.
- Adds `data-status="healthy|warning|critical|stale"` to operator status strip and metric panels.
- Uses SQLite-backed thresholds from `/api/settings`.

- [x] **Step 1: Write failing static tests**

Assert operator status markup exists and app code contains `computeSnapshotStatus`, `renderOperatorStatus`, `data-status`, `operatorAge`, and threshold names.

- [x] **Step 2: Run red tests**

Run:

```bash
bun test tests/dashboard-operator-status.test.ts
```

Expected: fail because operator status strip and functions do not exist.

- [x] **Step 3: Implement operator status strip and threshold coloring**

Classify:

- `stale` when latest sample age exceeds three poll intervals or fetch fails repeatedly.
- `critical` when any metric reaches critical threshold or snapshot fetch fails.
- `warning` when any metric reaches warning threshold.
- `healthy` otherwise.

Show the worst offender by label and value.

- [x] **Step 4: Add chart threshold bands**

Add ECharts mark lines/areas for warning and critical levels in Line and Area modes. Keep Heatmap and Treemap available but treat them as alternate views, not the primary timeline.

- [x] **Step 5: Run focused green tests**

Run:

```bash
bun test tests/dashboard-operator-status.test.ts tests/dashboard-overview.test.ts tests/dashboard-timeline.test.ts tests/dashboard-assets.test.ts
```

Expected: all focused tests pass.

---

### Task 4: Timeline V2 Rail And History Coverage UI

**Files:**
- Modify: `legacy/dashboard/index.html`
- Modify: `legacy/dashboard/styles.css`
- Modify: `legacy/dashboard/app.js`
- Mirror: `agent/assets/dashboard/index.html`
- Mirror: `agent/assets/dashboard/styles.css`
- Mirror: `agent/assets/dashboard/app.js`
- Modify: `tests/dashboard-timeline.test.ts`

**Interfaces:**
- Replaces the native visible range input with a canvas time rail `#timeline-rail`.
- Produces `drawTimelineRail`, `timelineTimestampFromPointer`, `handleTimelinePointer`, `renderHistoryCoverage`, and `fetchHistoryCoverage`.
- Keeps keyboard selection on `#history-chart`.
- Adds coverage fields `#history-coverage`, `#history-oldest`, `#history-newest`, `#history-db-size`.
- Adds visible series toggles persisted as `tinytop.visibleSeries`.

- [x] **Step 1: Write failing timeline V2 tests**

Assert markup contains the timeline rail and coverage elements, no visible native `id="history-scrubber"` range input remains, and app contains the new rail/coverage functions.

- [x] **Step 2: Run red tests**

Run:

```bash
bun test tests/dashboard-timeline.test.ts
```

Expected: fail because timeline rail and coverage UI do not exist yet.

- [x] **Step 3: Implement rail rendering and pointer selection**

Draw a compact timeline overview on canvas with:

- mini utilization trace
- current visible window shading
- selected timestamp marker
- warning/critical background bands

Pointer/touch drag selects the closest timestamp; the Now button returns to live.

- [x] **Step 4: Implement coverage fetch/render**

Fetch `/api/history/coverage` after settings/history loads and after snapshots are collected. Render oldest/newest sample, retained hours, rollup bucket count, and DB size.

- [x] **Step 5: Run focused green tests**

Run:

```bash
bun test tests/dashboard-timeline.test.ts tests/dashboard-assets.test.ts
bun build legacy/dashboard/app.js --target=browser --outdir=/tmp/tinytop-dashboard-operator-check
```

Expected: tests pass and browser bundle builds.

---

### Task 5: Process And Filesystem Operator Controls

**Files:**
- Modify: `legacy/dashboard/index.html`
- Modify: `legacy/dashboard/styles.css`
- Modify: `legacy/dashboard/app.js`
- Mirror: `agent/assets/dashboard/index.html`
- Mirror: `agent/assets/dashboard/styles.css`
- Mirror: `agent/assets/dashboard/app.js`
- Create: `tests/dashboard-process-filesystem.test.ts`

**Interfaces:**
- Produces process controls `#process-search`, `#process-density`, sortable headers with `data-process-sort`, and `#process-detail-dialog`.
- Produces filesystem controls `#filesystem-show-system`, `#root-filesystem-card`, and `filterFilesystems`.
- Browser-local keys: `tinytop.processFilter`, `tinytop.processSort`, `tinytop.processDensity`, `tinytop.filesystemShowSystem`.

- [x] **Step 1: Write failing tests**

Assert markup and app bindings exist for process search/sort/density/details and filesystem show-system/root card/filtering.

- [x] **Step 2: Run red tests**

Run:

```bash
bun test tests/dashboard-process-filesystem.test.ts
```

Expected: fail because controls do not exist yet.

- [x] **Step 3: Implement process controls**

Sort by PID, command, CPU, RAM, or RSS. Filter by PID/command. Apply compact or comfortable density to the table. Open process details from a row button.

- [x] **Step 4: Implement filesystem controls**

Always show root filesystem in a prominent card. Hide pseudo/system mounts by default, with a browser-local toggle to include them. Use SQLite disk thresholds for warning/critical status.

- [x] **Step 5: Run focused green tests**

Run:

```bash
bun test tests/dashboard-process-filesystem.test.ts tests/dashboard-assets.test.ts
```

Expected: all focused tests pass.

---

### Task 6: Runtime Truth, Docs, Version, And Release

**Files:**
- Modify: `README.md`
- Modify: `GUIDE.md`
- Modify: `ARCHITECTURE.md`
- Modify: `PROGRESS.md`
- Modify: `CHANGELOG.md`
- Modify: `HANDOFF.md`
- Modify: `docs/guides/API.md`
- Create: `docs/reports/2026-06-26-dashboard-operator-console.md`
- Modify: `VERSION`
- Modify: `package.json`
- Modify: `tinytop`
- Modify: `agent/crates/*/Cargo.toml`
- Modify: `agent/Cargo.lock`

**Interfaces:**
- Documents Linux/WSL-only native telemetry for this release.
- Documents dashboard Timeline V2, thresholds, operator status strip, process/filesystem controls, settings split, history retention, rollups, and coverage API.
- Bumps version to `0.1.25`.
- Publishes tag `v0.1.25` and GitHub release with Linux x86_64 asset.

- [x] **Step 1: Run full verification**

Run:

```bash
./tinytop check
./tinytop rust build
diff -qr agent/assets/dashboard legacy/dashboard
git diff --check
bun audit
cargo audit --file agent/Cargo.lock
```

- [x] **Step 2: Run live Rust smoke**

Serve the new Rust daemon on a free port or the owned default port, then verify:

```bash
curl -fsS http://127.0.0.1:<port>/health
curl -fsS http://127.0.0.1:<port>/api/version
curl -fsS http://127.0.0.1:<port>/api/settings
curl -fsS http://127.0.0.1:<port>/api/history/coverage
curl -fsS http://127.0.0.1:<port>/ | rg "operator-status|timeline-rail|process-search|root-filesystem-card"
```

- [x] **Step 3: Run Playwright visual smoke**

Verify desktop and mobile dashboard screenshots show the operator status strip, four gauges, timeline rail, settings dialog, process controls, and no horizontal overflow.

- [x] **Step 4: Update docs and version**

Update every file listed above. Record verification evidence in the report and handoff.

- [x] **Step 5: Commit, tag, push, release**

Commit with Michel's author and Codex trailer, tag `v0.1.25`, push `main` and tag, create GitHub release, upload `tinytop-agent-linux-x86_64` and `.sha256`, and verify the remote release.
