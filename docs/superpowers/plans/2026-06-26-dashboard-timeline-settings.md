# Dashboard Timeline And Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fragile sample-index dashboard timeline with a timestamp-based history navigator, then add clearly separated browser-local and SQLite-backed daemon settings.

**Architecture:** The first vertical slice stays API-compatible by using the existing `/api/history` timestamp query parameters and client-side pagination, so the embedded Rust dashboard and legacy Bun dashboard can both gain a working timeline without a store migration. The next slices add a typed `app_settings` table and `/api/settings` endpoints in the Rust collector/dashboard daemon, then expose those settings in a dashboard UI that distinguishes "this browser" preferences from daemon defaults.

**Tech Stack:** Rust collector/dashboard daemon with Axum and SQLx/SQLite, legacy Bun dashboard fallback, static dashboard assets in `agent/assets/dashboard/` mirrored byte-for-byte into `legacy/dashboard/`, Bun tests, Rust tests.

## Global Constraints

- Default runtime remains the single Rust collector/dashboard daemon.
- `agent/assets/dashboard/` and `legacy/dashboard/` must stay byte-identical for shared dashboard files.
- No Svelte or new frontend framework in this phase; improve the existing HTML/CSS/vanilla JavaScript dashboard first.
- No new third-party dependencies for the timeline slice.
- Browser-local settings stay in `localStorage` when they are per-device preferences.
- Daemon-wide defaults and collection behavior settings belong in SQLite in the Rust collector/dashboard daemon.
- Use TDD for behavior changes: write the failing test, watch it fail, then implement.
- Update `README.md`, `ARCHITECTURE.md`, `PROGRESS.md`, `CHANGELOG.md`, and `HANDOFF.md` when user-visible behavior changes.

---

## File Structure

- Modify `agent/assets/dashboard/index.html`: timeline presets, timestamp range labels, live/inspect controls, accessible copy.
- Modify `agent/assets/dashboard/app.js`: timestamp-based selection, history window state, paged `/api/history` fetches, browser-local range preference, keyboard/chart selection behavior.
- Modify `agent/assets/dashboard/styles.css`: responsive timeline preset and status layout.
- Mirror changed dashboard files into `legacy/dashboard/`.
- Create or modify `tests/dashboard-timeline.test.ts`: static behavior tests for timestamp timeline controls and API-compatible history pagination.
- Modify `tests/dashboard-assets.test.ts`: keep parity coverage intact.
- Later modify `agent/crates/tinytop-store/src/lib.rs`: `app_settings` schema and typed accessors.
- Later modify `agent/crates/tinytop-agent/src/writer.rs`: `GET /api/settings` and `PUT /api/settings`.
- Later create Rust store/API tests for settings persistence and validation.
- Modify docs and `VERSION` at each completed release checkpoint.

---

### Task 1: Timestamp Timeline Browser Slice

**Files:**
- Modify: `agent/assets/dashboard/index.html`
- Modify: `agent/assets/dashboard/app.js`
- Modify: `agent/assets/dashboard/styles.css`
- Modify: `legacy/dashboard/index.html`
- Modify: `legacy/dashboard/app.js`
- Modify: `legacy/dashboard/styles.css`
- Create: `tests/dashboard-timeline.test.ts`

**Interfaces:**
- Consumes existing `/api/history?limit=<n>&since_ms=<epoch>&until_ms=<epoch>` response shape: `{ samples: [{ capturedAtMs, snapshot }] }`.
- Produces dashboard state fields: `state.historyWindowKey`, `state.selectedAtMs`, `state.historyFetchToken`, and `state.snapshots` sorted oldest-first by `capturedAt`.
- Produces functions used by UI events: `setHistoryWindow(key)`, `fetchHistoryWindow()`, `selectHistoryTimestamp(timestampMs)`, and `returnToLiveHistory()`.

- [ ] **Step 1: Write the failing static dashboard test**

Create `tests/dashboard-timeline.test.ts`:

```ts
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const html = readFileSync("legacy/dashboard/index.html", "utf8");
const app = readFileSync("legacy/dashboard/app.js", "utf8");

describe("dashboard timestamp timeline", () => {
  test("offers explicit history range presets", () => {
    expect(html).toContain('data-history-window="live"');
    expect(html).toContain('data-history-window="15m"');
    expect(html).toContain('data-history-window="1h"');
    expect(html).toContain('data-history-window="6h"');
    expect(html).toContain('data-history-window="24h"');
  });

  test("tracks selection by timestamp instead of sample index", () => {
    expect(app).toContain("selectedAtMs");
    expect(app).toContain("selectHistoryTimestamp");
    expect(app).not.toContain("selectedSampleIndex");
  });

  test("fetches history with timestamp windows and paginates large ranges", () => {
    expect(app).toContain("since_ms");
    expect(app).toContain("until_ms");
    expect(app).toContain("fetchHistoryPage");
    expect(app).toContain("MAX_HISTORY_PAGE_SIZE");
    expect(app).not.toContain("window_seconds=${HISTORY_WINDOW_SECONDS}");
  });

  test("persists only this browser's selected history window locally", () => {
    expect(app).toContain("tinytop.historyWindow");
    expect(app).toContain("readStoredValue(STORAGE_KEYS.historyWindow");
    expect(app).toContain("storeValue(STORAGE_KEYS.historyWindow");
  });
});
```

- [ ] **Step 2: Run the failing test**

Run: `bun test tests/dashboard-timeline.test.ts`

Expected: fails because the current dashboard still uses `selectedSampleIndex`, fixed 120-sample history, and no preset controls.

- [ ] **Step 3: Implement timestamp timeline controls**

Update `agent/assets/dashboard/index.html` so the history panel has a compact preset control before the range input:

```html
<nav class="history-window-nav" aria-label="History range">
  <button type="button" data-history-window="live" aria-pressed="true">Live</button>
  <button type="button" data-history-window="15m" aria-pressed="false">15m</button>
  <button type="button" data-history-window="1h" aria-pressed="false">1h</button>
  <button type="button" data-history-window="6h" aria-pressed="false">6h</button>
  <button type="button" data-history-window="24h" aria-pressed="false">24h</button>
</nav>
```

Keep the range input, but make its semantics timestamp-based:

```html
<input
  id="history-scrubber"
  type="range"
  min="0"
  max="0"
  value="0"
  step="1"
  aria-label="Browse history timeline by timestamp"
  disabled
/>
```

- [ ] **Step 4: Implement API-compatible history pagination**

Update `agent/assets/dashboard/app.js`:

```js
const MAX_HISTORY_PAGE_SIZE = 10_000;
const MAX_HISTORY_RENDER_SAMPLES = 1_200;
const HISTORY_WINDOWS = {
  live: { label: "Live", durationMs: 5 * 60 * 1000, pageSize: 240 },
  "15m": { label: "15m", durationMs: 15 * 60 * 1000, pageSize: 900 },
  "1h": { label: "1h", durationMs: 60 * 60 * 1000, pageSize: 2_400 },
  "6h": { label: "6h", durationMs: 6 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE },
  "24h": { label: "24h", durationMs: 24 * 60 * 60 * 1000, pageSize: MAX_HISTORY_PAGE_SIZE },
};
```

Replace `selectedSampleIndex` with `selectedAtMs`. Add `fetchHistoryPage({ sinceMs, untilMs, limit })`, `fetchHistoryWindow()`, and `downsampleHistorySamples(samples, maxSamples)`. `fetchHistoryWindow()` must request explicit `since_ms` and `until_ms`; if the server returns a full page and the oldest sample is still newer than `since_ms`, request the next page ending at `oldest.capturedAtMs - 1`.

- [ ] **Step 5: Preserve live polling behavior**

When `state.selectedAtMs === null`, newly polled snapshots remain live and move the selected sample to the newest item. When `state.selectedAtMs !== null`, newly polled snapshots update the graph but do not move the inspected timestamp. `returnToLiveHistory()` clears `selectedAtMs`, updates labels, and redraws the chart.

- [ ] **Step 6: Mirror dashboard assets into legacy**

Copy the edited `agent/assets/dashboard/index.html`, `app.js`, and `styles.css` into `legacy/dashboard/` so `tests/dashboard-assets.test.ts` continues to prove byte identity.

- [ ] **Step 7: Run focused verification**

Run:

```bash
bun test tests/dashboard-timeline.test.ts tests/dashboard-assets.test.ts tests/webui-dialogs.test.ts
bun build legacy/dashboard/app.js --target=browser --outdir=/tmp/tinytop-dashboard-timeline-check
```

Expected: all tests pass and the browser bundle builds.

---

### Task 2: Timeline Runtime Smoke

**Files:**
- Modify: `docs/reports/2026-06-26-dashboard-timeline-settings.md`

**Interfaces:**
- Consumes `./tinytop rust serve --sqlite sqlite::memory: --poll-ms 100000`.
- Produces a verification report with endpoint and asset evidence.

- [ ] **Step 1: Start the Rust daemon on a free port**

If the default `127.0.0.1:4274` is busy, stop only the TinyTop daemon that this session owns or use an alternate free port.

Run:

```bash
PORT=4284 ./tinytop rust serve --sqlite sqlite::memory: --poll-ms 100000
```

- [ ] **Step 2: Verify embedded dashboard assets and history API**

Run:

```bash
curl -fsS http://127.0.0.1:4284/health
curl -fsS http://127.0.0.1:4284/app.js | rg "selectedAtMs|fetchHistoryPage|data-history-window"
curl -fsS "http://127.0.0.1:4284/api/history?limit=5&since_ms=0"
```

- [ ] **Step 3: Record evidence**

Create `docs/reports/2026-06-26-dashboard-timeline-settings.md` with the focused test commands, smoke-test commands, and any known limits. Stop the smoke daemon before closeout unless the user explicitly asks to keep it running.

---

### Task 3: SQLite Daemon Settings Foundation

**Files:**
- Modify: `agent/crates/tinytop-store/src/lib.rs`
- Modify: `agent/crates/tinytop-store/tests/sqlite_history_store.rs`
- Modify: `agent/crates/tinytop-agent/src/writer.rs`
- Modify: `agent/crates/tinytop-agent/tests/serve_contract.rs`
- Create: `docs/adr/0007-daemon-and-browser-dashboard-settings.md`

**Interfaces:**
- Produces SQLite table: `app_settings (setting_key TEXT PRIMARY KEY, value_json TEXT NOT NULL, updated_at_ms INTEGER NOT NULL)`.
- Produces Rust methods: `get_settings()`, `put_settings(settings)`, and typed validation for daemon defaults.
- Produces HTTP endpoints: `GET /api/settings` and `PUT /api/settings`.

- [ ] **Step 1: Write Rust store tests for settings persistence**
- [ ] **Step 2: Implement `app_settings` schema and typed accessors**
- [ ] **Step 3: Write Rust API contract tests**
- [ ] **Step 4: Implement settings routes with validation and no-store responses**
- [ ] **Step 5: Update ADR 0007 with the browser-vs-daemon split**

Daemon settings to persist in SQLite:

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

Browser-local settings to keep in localStorage:

- active theme override for this browser
- active graph mode override for this browser
- selected timeline range for this browser
- panel expanded/collapsed state
- table sort/filter/search
- density/layout preference
- dismissed UI hints
- last visible section or scroll position

---

### Task 4: Settings UI

**Files:**
- Modify: `agent/assets/dashboard/index.html`
- Modify: `agent/assets/dashboard/app.js`
- Modify: `agent/assets/dashboard/styles.css`
- Mirror: `legacy/dashboard/index.html`
- Mirror: `legacy/dashboard/app.js`
- Mirror: `legacy/dashboard/styles.css`
- Create or modify: `tests/dashboard-settings.test.ts`

**Interfaces:**
- Consumes `GET /api/settings`.
- Produces an in-dashboard settings panel with two groups: `This Browser` and `This Daemon`.

- [ ] **Step 1: Write static UI tests for the settings panel labels and controls**
- [ ] **Step 2: Add a settings button and panel**
- [ ] **Step 3: Wire browser-local settings without API calls**
- [ ] **Step 4: Wire daemon settings to `GET /api/settings` and `PUT /api/settings`**
- [ ] **Step 5: Run focused dashboard and API tests**

---

### Task 5: Retention And Rollups

**Files:**
- Modify: `agent/crates/tinytop-store/src/lib.rs`
- Modify: `agent/crates/tinytop-agent/src/writer.rs`
- Modify: `agent/crates/tinytop-agent/src/main.rs`
- Modify tests under `agent/crates/tinytop-store/tests/` and `agent/crates/tinytop-agent/tests/`

**Interfaces:**
- Consumes SQLite daemon settings from Task 3.
- Produces automatic retention enforcement and optional rollup storage for long dashboard windows.

- [ ] **Step 1: Write retention tests that prove old raw samples are pruned only after the configured retention window**
- [ ] **Step 2: Add retention enforcement after inserts**
- [ ] **Step 3: Add rollup query/storage design if 24h windows remain too dense for raw samples**
- [ ] **Step 4: Document operational impact and defaults**

---

## Parallelization Plan

- Task 1 is a single dashboard asset write set and should be implemented by one worker to avoid conflicts.
- Task 2 can run after Task 1 tests pass.
- Task 3 can run in parallel with Task 4 design exploration, but code changes should wait until the settings API contract is committed.
- Documentation updates can run in parallel after each task's behavior is known.
- Final review and release are sequential gates.

## Verification Plan

Focused timeline slice:

```bash
bun test tests/dashboard-timeline.test.ts tests/dashboard-assets.test.ts tests/webui-dialogs.test.ts
bun build legacy/dashboard/app.js --target=browser --outdir=/tmp/tinytop-dashboard-timeline-check
```

Full release checkpoint:

```bash
./tinytop check
bun audit
cargo audit --file agent/Cargo.lock
git diff --check
```

## Release Closeout

At a completed release checkpoint:

- Update `VERSION` and `package.json`.
- Update `README.md`, `ARCHITECTURE.md`, `PROGRESS.md`, `CHANGELOG.md`, and `HANDOFF.md`.
- Commit with Michel's GitHub noreply author and `Co-Authored-By: Codex <noreply@openai.com>`.
- Tag `v$(cat VERSION)`.
- Push `main` and the tag.
- Create a GitHub release with verification notes and any release assets required for the current version.
