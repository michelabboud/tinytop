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
- Current schema version: v1, with nullable recent-window `snapshot_json`, one-minute/five-minute/hourly rollup tables, typed filesystem/process detail tables, migration/disk/fold state, and daemon timeline events
- Current retention behavior: Rust daemon still prunes raw rows by `retentionHours` and one-minute rollups by `rollupRetentionDays`; the v1 migration strips legacy JSON older than 60 minutes, while population and maintenance of the new ladder tables land in the subsequent ladder tasks

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

Fresh databases are created directly at schema v1. The DDL below is also the
post-migration shape; the six minimum/root-maximum columns appear at the end of
`metric_rollups_1m` because SQLite appends them when upgrading a populated v0
database.

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
  snapshot_json TEXT
);

CREATE INDEX IF NOT EXISTS idx_metric_samples_captured_at
  ON metric_samples (captured_at_ms DESC);

CREATE INDEX IF NOT EXISTS idx_metric_samples_runtime_captured_at
  ON metric_samples (runtime_kind, captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS app_settings (
  setting_key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS metric_rollups_1m (
  bucket_start_ms INTEGER PRIMARY KEY,
  first_captured_at_ms INTEGER NOT NULL,
  newest_captured_at_ms INTEGER NOT NULL,
  sample_count INTEGER NOT NULL,
  avg_cpu_usage_percent REAL NOT NULL,
  max_cpu_usage_percent REAL NOT NULL,
  avg_memory_used_percent REAL NOT NULL,
  max_memory_used_percent REAL NOT NULL,
  avg_swap_used_percent REAL NOT NULL,
  max_swap_used_percent REAL NOT NULL,
  avg_load_percent REAL NOT NULL,
  max_load_percent REAL NOT NULL,
  avg_root_used_percent REAL,
  min_cpu_usage_percent REAL,
  min_memory_used_percent REAL,
  min_swap_used_percent REAL,
  min_load_percent REAL,
  min_root_used_percent REAL,
  max_root_used_percent REAL
);

CREATE INDEX IF NOT EXISTS idx_metric_rollups_1m_newest
  ON metric_rollups_1m (newest_captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS metric_rollups_5m (
  bucket_start_ms INTEGER PRIMARY KEY,
  first_captured_at_ms INTEGER NOT NULL,
  newest_captured_at_ms INTEGER NOT NULL,
  sample_count INTEGER NOT NULL,
  avg_cpu_usage_percent REAL NOT NULL,
  min_cpu_usage_percent REAL NOT NULL,
  max_cpu_usage_percent REAL NOT NULL,
  avg_memory_used_percent REAL NOT NULL,
  min_memory_used_percent REAL NOT NULL,
  max_memory_used_percent REAL NOT NULL,
  avg_swap_used_percent REAL NOT NULL,
  min_swap_used_percent REAL NOT NULL,
  max_swap_used_percent REAL NOT NULL,
  avg_load_percent REAL NOT NULL,
  min_load_percent REAL NOT NULL,
  max_load_percent REAL NOT NULL,
  avg_root_used_percent REAL,
  min_root_used_percent REAL,
  max_root_used_percent REAL
);

CREATE INDEX IF NOT EXISTS idx_metric_rollups_5m_newest
  ON metric_rollups_5m (newest_captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS metric_rollups_1h (
  bucket_start_ms INTEGER PRIMARY KEY,
  first_captured_at_ms INTEGER NOT NULL,
  newest_captured_at_ms INTEGER NOT NULL,
  sample_count INTEGER NOT NULL,
  avg_cpu_usage_percent REAL NOT NULL,
  min_cpu_usage_percent REAL NOT NULL,
  max_cpu_usage_percent REAL NOT NULL,
  avg_memory_used_percent REAL NOT NULL,
  min_memory_used_percent REAL NOT NULL,
  max_memory_used_percent REAL NOT NULL,
  avg_swap_used_percent REAL NOT NULL,
  min_swap_used_percent REAL NOT NULL,
  max_swap_used_percent REAL NOT NULL,
  avg_load_percent REAL NOT NULL,
  min_load_percent REAL NOT NULL,
  max_load_percent REAL NOT NULL,
  avg_root_used_percent REAL,
  min_root_used_percent REAL,
  max_root_used_percent REAL
);

CREATE INDEX IF NOT EXISTS idx_metric_rollups_1h_newest
  ON metric_rollups_1h (newest_captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS history_state (
  state_key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS fs_samples (
  captured_at_ms INTEGER NOT NULL,
  mount TEXT NOT NULL,
  filesystem TEXT NOT NULL,
  fs_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  used_bytes INTEGER NOT NULL,
  available_bytes INTEGER NOT NULL,
  used_percent REAL NOT NULL,
  inode_used_percent REAL,
  inode_used INTEGER,
  inode_total INTEGER,
  PRIMARY KEY (captured_at_ms, mount)
);

CREATE INDEX IF NOT EXISTS idx_fs_samples_mount_time
  ON fs_samples (mount, captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS process_samples (
  captured_at_ms INTEGER NOT NULL,
  rank INTEGER NOT NULL,
  pid INTEGER NOT NULL,
  command TEXT NOT NULL,
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  rss_bytes INTEGER NOT NULL,
  parent_pid INTEGER,
  started_at TEXT,
  PRIMARY KEY (captured_at_ms, rank)
);

CREATE INDEX IF NOT EXISTS idx_process_samples_time
  ON process_samples (captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS app_events (
  event_id INTEGER PRIMARY KEY,
  occurred_at_ms INTEGER NOT NULL,
  marker_type TEXT NOT NULL,
  label TEXT NOT NULL,
  details_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_app_events_occurred_type
  ON app_events (occurred_at_ms DESC, marker_type);

PRAGMA user_version = 1;
```

## Schema Versions And Migration

`SqliteHistoryStore::connect` applies the SQLite pragmas, reads
`PRAGMA user_version`, and ensures schema v1 before it runs the older runtime
name canonicalization. A new or empty database is built directly at v1. A v1
database only receives idempotent `CREATE ... IF NOT EXISTS` checks.

A populated v0 database is migrated fail-closed:

1. The store resolves the database directory to its longest matching mounted
   filesystem and requires free bytes greater than or equal to
   `database_bytes + database_bytes / 5`.
2. It refuses if `<database>.pre-v0.sqlite` already exists. Otherwise
   `VACUUM INTO` creates that complete pre-image before any row is changed.
3. One SQLite transaction rebuilds `metric_samples` with nullable
   `snapshot_json`, retains JSON only for rows within the last 60 minutes,
   preserves every typed row and `sample_id`, adds the v1 rollup columns and
   tables, and sets `user_version = 1`. A failed transaction leaves v0 in
   place.
4. The store runs the product's one automatic post-migration `VACUUM`, writes
   `history_state.schemaMigration`, and records a `schemaMigrated` event with
   the pre-image path, row counts, duration, and before/after file sizes.

The pre-image is never overwritten, replaced, or deleted automatically. Until
the explicit `db pre-image` operator commands land in the later CLI task, an
operator must move a pre-existing pre-image aside manually before retrying a
refused migration.

## Why Store Snapshot JSON

The UI does not only need graph values. Timeline browsing needs the full selected sample so gauges, filesystem cards, pressure panels, and process rows can render the selected point in time. Storing recent `snapshot_json` lets refresh hydration restore the same UI state without re-collecting fake or partial data. Schema v1 makes the column nullable so older raw rows can retain their compact typed metrics without retaining the dominant JSON payload indefinitely.

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

Long range chart points:

```text
/api/history/points?source=rollup&since_ms=<range-start>&until_ms=<range-end>
```

The Rust daemon maps one-minute rollup rows into chart-ready points for 6h, 24h, 7d, and 30d dashboard windows. Rollup points carry aggregate metric values and sample counts; full raw snapshot detail still comes from `/api/history`.

Timeline markers:

```text
/api/history/markers?since_ms=<range-start>&until_ms=<range-end>&expected_gap_ms=<gap>
```

The Rust daemon returns persisted `daemonStart`, `settingsChange`, and one-time `schemaMigrated` events from `app_events`, plus computed `coverageGap` markers inferred from raw sample spacing.

## Rollups And Coverage

The Rust daemon rebuilds the affected one-minute rollup bucket after each sample insert. Rollups are additive to the raw history table; `/api/history` still returns raw samples today.

`GET /api/history/coverage` reports:

- raw sample count
- oldest raw sample timestamp
- newest raw sample timestamp
- configured raw retention hours
- configured rollup retention days
- one-minute rollup bucket count
- SQLite database size in bytes
- configured target DB size in bytes
- database budget percentage
- oldest/newest rollup timestamps

## Retention

Current behavior:

- Every successful scheduled or manual Rust collection writes one raw row into `metric_samples`.
- On the one-time populated-v0 migration, JSON is retained for the most recent 60 minutes and set to `NULL` on older rows; all typed columns and rows are preserved. Fresh and post-migration inserts still include JSON until the subsequent ladder-maintenance task adds ongoing window stripping.
- The Rust daemon deletes raw rows older than the configured `retentionHours` cutoff.
- The Rust daemon deletes rollup buckets older than the configured `rollupRetentionDays` cutoff.
- `metric_rollups_5m`, `metric_rollups_1h`, `fs_samples`, and `process_samples` exist in v1 but are not populated or used for reads by this schema-only task.
- `/api/history` and `/history` select bounded windows for callers; reads do not delete rows, but Rust daemon maintenance prunes according to settings.
- The dashboard hydrates the browser-selected timestamp window. Live, 15m, and 1h use `/api/history`; 6h, 24h, 7d, and 30d use `/api/history/points` backed by one-minute rollups. Raw windows may be paged and downsampled only for browser rendering; that is a rendering limit, not a storage limit.
- `Clear` in the dashboard clears only the current browser tab's loaded samples and leaves SQLite untouched.
- Legacy Bun split mode keeps the earlier manual archive/reset behavior.

Current defaults:

- Raw samples: configurable, default 72 hours.
- One-minute rollups: 30 days.
- Migrated v0 JSON window: 60 minutes.
- Target database budget: 128 MiB.

## Future Tables

Potential future normalized tables:

- `pressure_samples`

Schema v1 now reserves `fs_samples` and `process_samples` for typed detail rows; their cadence and retention maintenance are implemented by later ladder tasks. Pressure detail remains inside recent snapshot JSON in this phase. See [docs/adr/0002-initial-snapshot-json-history.md](adr/0002-initial-snapshot-json-history.md).

## Operational Notes

SQLite may create sidecar files beside the database:

- `history.sqlite-wal`
- `history.sqlite-shm`

Back up all three when the Rust daemon or legacy Bun collector is running, or stop the SQLite owner before copying only `history.sqlite`.

See [docs/guides/OPERATIONS.md](guides/OPERATIONS.md) for inspection, backup, and reset commands.
