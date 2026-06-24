# SQLite History Architecture

## Goal

Persist WSL/Linux dashboard history so the UI can reload with recent samples already available, while keeping data collection and SQLite ownership in one dedicated Bun process.

## Current Architecture

Use two local Bun processes:

1. `collector-writer`: owns collection, SQLite writes, SQLite reads, migrations, and query optimization.
2. `dashboard`: serves the browser UI and calls the writer process for current and historical data. It never opens the SQLite database directly.

Browser-only settings such as theme, graph mode, and timeline position should stay in `localStorage`.

## Process Boundary

The writer process exposes a local loopback API on `127.0.0.1:4276` by default:

- `GET /health`
- `GET /snapshot/latest`
- `GET /snapshot/collect`
- `GET /history?since_ms=&until_ms=&limit=&window_seconds=`

The dashboard process keeps its public port on `127.0.0.1:4274` and proxies read requests to the writer. This keeps SQLite connection lifecycle, WAL checkpointing, migrations, and read/write concurrency in one process.

## Table Strategy

Use one main raw metrics table, not daily tables.

SQLite B-tree indexes handle timestamp-range queries well, and a single table keeps the dashboard queries simple. Daily tables make retention and backups look attractive, but they complicate every query crossing midnight and add schema-management overhead. If history grows large, add rollup tables and retention instead of partitioning first.

## Current Schema

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
  root_used_percent REAL,
  snapshot_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_metric_samples_captured_at
  ON metric_samples (captured_at_ms DESC);

CREATE INDEX IF NOT EXISTS idx_metric_samples_runtime_captured_at
  ON metric_samples (runtime_kind, captured_at_ms DESC);
```

The full `SystemSnapshot` is stored as `snapshot_json` so the browser can hydrate gauges, selected-sample details, filesystem cards, pressure panels, and process rows from the same persisted sample. Typed metric columns keep graph and future rollup queries index-friendly.

## Write Path

- Collect every polling interval in the writer process.
- Use prepared statements.
- Write each sample in one transaction with an upsert on `captured_at_ms`.
- Keep the dashboard read API on the same process so there is no multi-process SQLite write contention.
- Periodically call `PRAGMA optimize`.

## Read Path

Startup dashboard fill:

```sql
SELECT *
FROM metric_samples
WHERE captured_at_ms >= ?
ORDER BY captured_at_ms DESC
LIMIT ?;
```

The writer reverses this newest-window query before returning JSON so the browser receives samples oldest first.

Live chart window:

- For raw recent history, query the last `X` seconds or the last `N` samples from `metric_samples`.
- For longer time ranges, query `metric_rollups_1m`.
- The UI should still calculate its visible graph window from viewport width, but its source buffer should come from the writer.

Latest details:

- `metric_samples.snapshot_json` gives the main cards and graph for the currently selected sample or latest sample.

## Retention

Suggested defaults:

- Raw samples: 24 to 72 hours.
- One-minute rollups: 30 days.
- Longer rollups can be added only when needed.

Retention should delete by `captured_at_ms` from `metric_samples`. If future child tables are added, they should use cascading foreign keys.

## Implemented Files

- `src/history-store.ts` contains SQLite setup, indexes, prepared statements, inserts, and range reads.
- `src/collector-daemon.ts` contains the writer API and scheduled collection loop.
- `src/server.ts` proxies `/api/snapshot` and `/api/history` to the writer process.
- `public/app.js` hydrates history from `/api/history` before live polling.

## Open Questions

- Exact raw retention duration: 24h, 72h, or user configurable.
- Whether future process history queries need normalized child tables or wider configurable top N.
- Whether the writer API should use a second loopback port or a Unix socket on Linux/WSL.
