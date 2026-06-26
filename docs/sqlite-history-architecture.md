# SQLite History Architecture

This document describes the implemented SQLite history architecture for TinyTop. The design goal is refresh-safe recent history with a single local process owning database lifecycle and writes.

## Summary

- Default SQLite owner: `tinytop-agent serve`
- Legacy Bun SQLite owner: `legacy/bun-collector.ts`
- Rust store module: `agent/crates/tinytop-store`
- Legacy Bun store module: `src/history-store.ts`
- Public dashboard API: Rust daemon on `127.0.0.1:4274`
- Default database path: `~/.local/share/tinytop/history.sqlite`
- Override path: `TINYTOP_HISTORY_DB=/path/to/history.sqlite`
- Current history shape: one `metric_samples` table with indexed metric columns and full snapshot JSON
- Current retention: no automatic pruning; rows remain until manual archive/reset

## Process Boundary

The default runtime uses one Rust process:

1. `tinytop-agent serve` on `127.0.0.1:4274`
   - Serves embedded static frontend assets from `agent/assets/dashboard/`.
   - Serves vendored Apache ECharts from embedded dashboard assets.
   - Serves `/api/snapshot` and `/api/history`.
   - Exposes legacy collector-compatible routes on the same port.
   - Collects local telemetry.
   - Owns the SQLite connection.
   - Applies SQLite pragmas and schema setup.
   - Writes samples.

The legacy Bun development runtime uses two local processes:

1. `dashboard` on `127.0.0.1:4274`
   - Serves static frontend assets.
   - Serves `/vendor/echarts.min.js` from `legacy/dashboard/vendor/`.
   - Proxies `/api/snapshot` and `/api/history` to the collector process.
   - Never opens SQLite.

2. `legacy-bun-collector` on `127.0.0.1:4276`
   - Collects local telemetry.
   - Owns the SQLite connection.
   - Applies SQLite pragmas and schema setup.
   - Writes samples.
   - Answers current and historical reads.

Both runtimes avoid accidental multi-process writes and keep WAL behavior, migrations, pragmas, and future retention policy in one process.

## Legacy Collector API

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/health` | Plain-text collector health check |
| `GET` | `/snapshot/latest` | Return latest stored sample, collecting one if none exists |
| `GET` | `/snapshot/collect` | Collect and store a new sample immediately |
| `GET` | `/history` | Return timestamp-window history samples |

`/history` query parameters:

| Parameter | Meaning |
| --- | --- |
| `limit` | Maximum number of samples, default `120`, maximum enforced by store normalization |
| `window_seconds` | Relative window when `since_ms` is absent, default `300` |
| `since_ms` | Inclusive Unix epoch millisecond lower bound |
| `until_ms` | Inclusive Unix epoch millisecond upper bound |

The Rust daemon and legacy Bun collector return history oldest first so charts and timeline controls can render naturally.

## SQLite Pragmas

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;
```

`WAL` gives the SQLite owner better read/write behavior. `NORMAL` sync is the pragmatic local-dashboard setting. `busy_timeout` prevents avoidable transient lock failures. `foreign_keys` is enabled now so future child tables can rely on cascading behavior.

## Current Schema

```sql
CREATE TABLE IF NOT EXISTS metric_samples (
  sample_id INTEGER PRIMARY KEY,
  captured_at_ms INTEGER NOT NULL UNIQUE,
  snapshot_timestamp TEXT NOT NULL,
  hostname TEXT NOT NULL,
  runtime_kind TEXT NOT NULL,
  cpu_usage_percent REAL NOT NULL,
  cpu_cores INTEGER NOT NULL,
  memory_used_percent REAL NOT NULL,
  memory_used_bytes INTEGER NOT NULL,
  memory_total_bytes INTEGER NOT NULL,
  swap_used_percent REAL NOT NULL,
  swap_used_bytes INTEGER NOT NULL,
  swap_total_bytes INTEGER NOT NULL,
  load_one REAL NOT NULL,
  load_five REAL NOT NULL,
  load_fifteen REAL NOT NULL,
  load_percent REAL NOT NULL,
  runnable_threads INTEGER NOT NULL,
  total_threads INTEGER NOT NULL,
  root_used_percent REAL,
  snapshot_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_metric_samples_captured_at
  ON metric_samples (captured_at_ms DESC);

CREATE INDEX IF NOT EXISTS idx_metric_samples_runtime_captured_at
  ON metric_samples (runtime_kind, captured_at_ms DESC);
```

## Why Store Snapshot JSON

The UI does not only need graph values. Timeline browsing needs the full selected sample so gauges, filesystem cards, pressure panels, and process rows can render the selected point in time. Storing `snapshot_json` lets refresh hydration restore the same UI state without re-collecting fake or partial data.

Typed columns are still stored for graph values and future rollups, so history is not trapped inside JSON.

## Write Path

1. The Rust daemon collection loop runs every `HISTORY_POLL_MS` milliseconds. In legacy Bun mode, the collector timer calls `/snapshot/collect`.
2. The collector reads local Linux/WSL sources.
3. `tinytop-store` or `openHistoryStore().insertSnapshot()` writes the sample in SQLite.
4. The insert uses `captured_at_ms` as a unique timestamp key.
5. If a sample with the same timestamp exists, the row is updated.

## Read Path

Latest sample:

```sql
SELECT captured_at_ms, snapshot_json
FROM metric_samples
ORDER BY captured_at_ms DESC
LIMIT 1;
```

History window:

```sql
SELECT captured_at_ms, snapshot_json
FROM metric_samples
WHERE captured_at_ms >= ?
  AND captured_at_ms <= ?
ORDER BY captured_at_ms DESC
LIMIT ?;
```

The SQLite owner reverses the selected rows before returning them so the browser receives oldest-to-newest samples.

## Frontend Hydration

On startup, the dashboard `app.js` requests the browser-selected timestamp range:

```text
/api/history?limit=<range-page-size>&since_ms=<range-start>&until_ms=<range-end>
```

The browser then:

1. Sorts samples oldest first.
2. Deduplicates by captured timestamp.
3. Renders the latest sample into the dashboard.
4. Pages backward through larger ranges when the server returns a full result page.
5. Downsamples only if the browser has more points than it should render.
6. Starts polling `/api/snapshot`.

This is why browser refresh now refills History instead of starting from one sample.

## Retention

Retention is not implemented yet. The current database grows until manually archived or reset.

Current behavior:

- Every successful scheduled or manual collection writes one raw row into `metric_samples`.
- `/api/history` and `/history` select bounded windows for callers, but they never delete older rows.
- The dashboard hydrates the browser-selected timestamp window, currently Live, 15m, 1h, 6h, or 24h. Large windows are paged through `/api/history`, deduplicated by captured timestamp, and downsampled only for browser rendering; that is a rendering limit, not a storage limit.
- `Clear` in the dashboard clears only the current browser tab's loaded samples and leaves SQLite untouched.

Recommended future retention:

- Raw samples: configurable, default 24 to 72 hours.
- One-minute rollups: 30 days.
- Longer rollups: only if the UI adds longer-range browsing.

When retention is implemented, it should delete by `captured_at_ms` from `metric_samples`. Future child tables should use cascading foreign keys.

## Future Tables

Potential future normalized tables:

- `pressure_samples`
- `filesystem_samples`
- `process_samples`
- `metric_rollups_1m`

These were deliberately not implemented in the first persistence slice because the current UI hydrates full snapshots. See [docs/adr/0002-initial-snapshot-json-history.md](adr/0002-initial-snapshot-json-history.md).

## Operational Notes

SQLite may create sidecar files beside the database:

- `history.sqlite-wal`
- `history.sqlite-shm`

Back up all three when the Rust daemon or legacy Bun collector is running, or stop the SQLite owner before copying only `history.sqlite`.

See [docs/guides/OPERATIONS.md](guides/OPERATIONS.md) for inspection, backup, and reset commands.
