# SQLite History Architecture Plan

## Goal

Persist WSL/Linux dashboard history so the UI can reload with recent samples already available, while keeping data collection and SQLite ownership in one dedicated Bun process.

## Recommendation

Use two local Bun processes:

1. `collector-writer`: owns collection, SQLite writes, SQLite reads, retention, migrations, and query optimization.
2. `dashboard`: serves the browser UI and calls the writer process for current and historical data. It never opens the SQLite database directly.

Browser-only settings such as theme, graph mode, and timeline position should stay in `localStorage`.

## Process Boundary

The writer process should expose a local loopback API, for example on `127.0.0.1:4276`:

- `GET /health`
- `GET /snapshot/latest`
- `GET /history?since_ms=&until_ms=&limit=&resolution=raw`
- `GET /processes/latest`
- `GET /filesystems/latest`

The dashboard process keeps its public port on `127.0.0.1:4274` and proxies read requests to the writer. This keeps SQLite connection lifecycle, WAL checkpointing, migrations, and read/write concurrency in one process.

## Table Strategy

Use one main raw metrics table, not daily tables.

SQLite B-tree indexes handle timestamp-range queries well, and a single table keeps the dashboard queries simple. Daily tables make retention and backups look attractive, but they complicate every query crossing midnight and add schema-management overhead. If history grows large, add rollup tables and retention instead of partitioning first.

## Proposed Schema

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS metric_samples (
  sample_id INTEGER PRIMARY KEY,
  captured_at_ms INTEGER NOT NULL UNIQUE,
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
  root_used_percent REAL
);

CREATE INDEX IF NOT EXISTS idx_metric_samples_captured_at
  ON metric_samples (captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS pressure_samples (
  sample_id INTEGER PRIMARY KEY REFERENCES metric_samples(sample_id) ON DELETE CASCADE,
  cpu_some_avg10 REAL,
  memory_some_avg10 REAL,
  io_some_avg10 REAL
);

CREATE TABLE IF NOT EXISTS filesystem_samples (
  sample_id INTEGER NOT NULL REFERENCES metric_samples(sample_id) ON DELETE CASCADE,
  mount TEXT NOT NULL,
  filesystem TEXT NOT NULL,
  fs_type TEXT NOT NULL,
  used_percent REAL NOT NULL,
  inode_used_percent REAL,
  used_bytes INTEGER NOT NULL,
  size_bytes INTEGER NOT NULL,
  PRIMARY KEY (sample_id, mount)
);

CREATE TABLE IF NOT EXISTS process_samples (
  sample_id INTEGER NOT NULL REFERENCES metric_samples(sample_id) ON DELETE CASCADE,
  rank INTEGER NOT NULL,
  pid INTEGER NOT NULL,
  command TEXT NOT NULL,
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  rss_bytes INTEGER NOT NULL,
  PRIMARY KEY (sample_id, rank)
);

CREATE INDEX IF NOT EXISTS idx_process_samples_sample_cpu
  ON process_samples (sample_id, cpu_percent DESC);

CREATE TABLE IF NOT EXISTS metric_rollups_1m (
  bucket_start_ms INTEGER PRIMARY KEY,
  sample_count INTEGER NOT NULL,
  cpu_avg REAL NOT NULL,
  cpu_max REAL NOT NULL,
  memory_avg REAL NOT NULL,
  memory_max REAL NOT NULL,
  swap_avg REAL NOT NULL,
  swap_max REAL NOT NULL,
  load_avg REAL NOT NULL,
  load_max REAL NOT NULL
);
```

## Write Path

- Collect every polling interval in the writer process.
- Use prepared statements.
- Write each sample in one transaction:
  1. Insert `metric_samples`.
  2. Insert pressure row.
  3. Insert filesystem rows.
  4. Insert top process rows.
- Keep the dashboard read API on the same process so there is no multi-process SQLite write contention.
- Periodically call `PRAGMA optimize`.

## Read Path

Startup dashboard fill:

```sql
SELECT *
FROM metric_samples
WHERE captured_at_ms >= ?
ORDER BY captured_at_ms ASC
LIMIT ?;
```

Live chart window:

- For raw recent history, query the last `X` seconds or the last `N` samples from `metric_samples`.
- For longer time ranges, query `metric_rollups_1m`.
- The UI should still calculate its visible graph window from viewport width, but its source buffer should come from the writer.

Latest details:

- `metric_samples` gives the main cards and graph.
- `filesystem_samples` and `process_samples` are fetched for the currently selected `sample_id` or latest sample.

## Retention

Suggested defaults:

- Raw samples: 24 to 72 hours.
- One-minute rollups: 30 days.
- Longer rollups can be added only when needed.

Retention should delete by `captured_at_ms` from `metric_samples`; child rows are removed through cascading foreign keys.

## Migration Path

1. Add `src/history/schema.ts` with migrations and pragmas.
2. Add `src/history/writer.ts` for inserts and range reads.
3. Add `src/collector-daemon.ts` as the writer process entry point.
4. Change `src/server.ts` to read `/api/snapshot` and `/api/history` from the writer API.
5. Update the frontend bootstrap to request the last configured window, for example `GET /api/history?window_seconds=300`.

## Open Questions

- Exact raw retention duration: 24h, 72h, or user configurable.
- Whether process history should store only top 10 rows or a wider configurable top N.
- Whether the writer API should use a second loopback port or a Unix socket on Linux/WSL.
