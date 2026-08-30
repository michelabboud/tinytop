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
- Current schema version: v4, with typed metric rows, interned host identity, on-change filesystem rows and mount-presence events, one-minute/five-minute/hourly rollup tables, typed process detail tables with millisecond start times and nullable GPU percent, a per-tick process table and command dictionary, interned GPU adapters and per-tick GPU samples, migration/disk/fold state, and daemon timeline events
- Current retention behavior: Rust daemon maintenance reads the validated `retentionLadder` block for L1–L4 horizons/toggles, filesystem check cadence, and per-tick process history; legacy `retentionHours` and `rollupRetentionDays` remain derived compatibility mirrors

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
   - Serves `/vendor/echarts.min.js` from the shared `agent/assets/dashboard/` tree.
   - Proxies `/api/snapshot` and `/api/history` to the collector process.
   - Never opens SQLite.

2. `legacy-bun-collector` on `127.0.0.1:4276`
   - Collects local telemetry.
   - Owns the SQLite connection.
   - Applies SQLite pragmas and schema setup.
   - Writes samples.
   - Answers current and historical reads.

Both runtimes avoid accidental multi-process writes. From schema v3, they must
also use different database paths: the Bun store reads and writes the legacy
JSON payload column that the Rust v3 schema removes. The Rust daemon does not
refuse a Bun writer on its behalf, so operators must keep the database paths
separate.

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

**WAL facts (measured 2026-08-29):** source WAL frames are included in the `VACUUM INTO` pre-image; the pre-image uses rollback-journal mode and has no WAL sidecar. Inspection replays a killed writer's WAL. The CLI can exit with the WAL un-checkpointed, and SQLite recovers it on the next open.

## Current Schema

Fresh databases are created directly at schema v4. The DDL below is also the
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
  runnable_threads INTEGER,
  total_threads INTEGER,
  root_used_percent REAL,
  identity_id INTEGER REFERENCES host_identity(identity_id),
  uptime_seconds INTEGER,
  memory_available_bytes INTEGER,
  swap_free_bytes INTEGER,
  last_pid INTEGER,
  filesystems_captured_at_ms INTEGER,
  CHECK (identity_id IS NULL OR (
    uptime_seconds IS NOT NULL
    AND memory_available_bytes IS NOT NULL
    AND swap_free_bytes IS NOT NULL
  ))
);

CREATE INDEX IF NOT EXISTS idx_metric_samples_captured_at
  ON metric_samples (captured_at_ms DESC);

CREATE INDEX IF NOT EXISTS idx_metric_samples_runtime_captured_at
  ON metric_samples (runtime_kind, captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS host_identity (
  identity_id INTEGER PRIMARY KEY,
  first_seen_ms INTEGER NOT NULL,
  hostname TEXT NOT NULL,
  platform TEXT NOT NULL,
  arch TEXT NOT NULL,
  distro TEXT NOT NULL,
  kernel TEXT NOT NULL,
  runtime_kind TEXT NOT NULL,
  runtime_confidence TEXT NOT NULL,
  runtime_reason TEXT NOT NULL,
  UNIQUE (
    hostname, platform, arch, distro, kernel,
    runtime_kind, runtime_confidence, runtime_reason
  )
);

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

CREATE TABLE IF NOT EXISTS fs_mount_events (
  captured_at_ms INTEGER NOT NULL,
  mount TEXT NOT NULL,
  present INTEGER NOT NULL CHECK (present IN (0, 1)),
  PRIMARY KEY (mount, captured_at_ms)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS process_samples (
  captured_at_ms INTEGER NOT NULL,
  rank INTEGER NOT NULL,
  pid INTEGER NOT NULL,
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  rss_bytes INTEGER NOT NULL,
  parent_pid INTEGER,
  started_at_ms INTEGER,
  command_id INTEGER REFERENCES process_commands(command_id),
  gpu_percent REAL,
  PRIMARY KEY (captured_at_ms, rank)
);

CREATE INDEX IF NOT EXISTS idx_process_samples_time
  ON process_samples (captured_at_ms DESC);

CREATE INDEX IF NOT EXISTS idx_process_samples_command
  ON process_samples (command_id);

CREATE TABLE IF NOT EXISTS process_commands (
  command_id INTEGER PRIMARY KEY,
  command TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS process_samples_fast (
  captured_at_ms INTEGER NOT NULL,
  rank INTEGER NOT NULL,
  pid INTEGER NOT NULL,
  command_id INTEGER NOT NULL REFERENCES process_commands(command_id),
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  rss_bytes INTEGER NOT NULL,
  parent_pid INTEGER,
  started_at_ms INTEGER,
  gpu_percent REAL,
  PRIMARY KEY (captured_at_ms, rank)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_process_samples_fast_command
  ON process_samples_fast (command_id);

CREATE TABLE IF NOT EXISTS gpu_adapters (
  adapter_id INTEGER PRIMARY KEY,
  stable_id TEXT NOT NULL UNIQUE,
  vendor TEXT NOT NULL,
  name TEXT NOT NULL,
  driver TEXT NOT NULL,
  first_seen_ms INTEGER NOT NULL,
  last_seen_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS gpu_samples (
  captured_at_ms INTEGER NOT NULL,
  adapter_id INTEGER NOT NULL REFERENCES gpu_adapters(adapter_id),
  busy_percent REAL,
  memory_used_bytes INTEGER,
  memory_total_bytes INTEGER,
  temperature_c REAL,
  PRIMARY KEY (captured_at_ms, adapter_id)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS app_events (
  event_id INTEGER PRIMARY KEY,
  occurred_at_ms INTEGER NOT NULL,
  marker_type TEXT NOT NULL,
  label TEXT NOT NULL,
  details_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_app_events_occurred_type
  ON app_events (occurred_at_ms DESC, marker_type);

PRAGMA user_version = 4;
```

The value written into `fs_samples.captured_at_ms` is the enumeration key
`fs_key_ms`: the snapshot's `filesystemsCapturedAtMs`, or the sample's
`captured_at_ms` when the snapshot has no filesystem stamp.

`process_samples_fast` is the per-tick process history table. Its `command_id`
references the dictionary, so repeated command text is stored once. Schema v4
stores process start time in nullable Unix-millisecond `started_at_ms` columns
and GPU usage in nullable `gpu_percent` columns in both process tiers. Both
process tables index `command_id` because orphan-command pruning probes each
table by that key.

## Schema Versions And Migration

`SqliteHistoryStore::connect` applies the SQLite pragmas, reads
`PRAGMA user_version`, and ensures schema v4 before it runs the older runtime
name canonicalization. A new or empty database is built directly at v4. A v4
database only receives idempotent `CREATE ... IF NOT EXISTS` checks.

A v1 database migrates to v2 in one transaction. Before any write, the store
checks that the linked SQLite version is at least 3.35.0, which is required by
`ALTER TABLE ... DROP COLUMN`; an older library is refused with
`schema migration requires SQLite ≥ 3.35.0 (linked: <version>)`. If
`ALTER TABLE ... DROP COLUMN` fails after that check because an index, trigger,
or view still references the old column (none exists in the shipped schema),
the store reports SQLite's own error text; the transaction rolls back and the
file is untouched. There is never a silent copy-table fallback. The transaction
creates the dictionary and fast table, adds and backfills
`process_samples.command_id`, verifies that no row remains unmapped, creates
both command indexes, drops the old command column, writes a
`schemaMigrated` app-event marker, and sets `PRAGMA user_version = 2`. Any
in-transaction failure rolls back. There is deliberately no pre-image and no
post-migration `VACUUM`: SQLite's transaction provides the atomicity, and the
design is recorded in
[ADR 0023](adr/0023-schema-v2-migration-one-transaction-no-pre-image.md).

A populated v0 database is migrated fail-closed:

1. The store resolves the database directory to its longest matching mounted
   filesystem and requires free bytes greater than or equal to
   `database_bytes + database_bytes / 5`.
2. It refuses if `<database>.pre-v0.sqlite` already exists. Otherwise
   `VACUUM INTO` creates that complete pre-image before any row is changed.
3. One SQLite transaction rebuilds `metric_samples` with a nullable legacy
   payload column, retains only its recent payload rows, preserves every typed
   row and `sample_id`, adds the v1 rollup columns and tables, sets
   `user_version = 1`, and writes
   `history_state.schemaMigration` with the pre-image path, row counts,
   `bytesBefore`, and `startedAtMs`. Its `vacuumedAtMs`, `bytesAfter`, and
   `durationMs` fields remain `null` in this transaction. A failed transaction
   leaves v0 in place without an audit record that claims otherwise.
4. The store runs the product's one automatic post-migration `VACUUM`, then
   atomically completes those three audit fields and records the one
   `schemaMigrated` event. On every v1 connection, an audit whose
   `vacuumedAtMs` is still `null` causes the VACUUM and audit completion to run
   again. This makes a crash after the schema commit recoverable and keeps the
   completion idempotent.

Schema v3 rebuilds `metric_samples` in one transaction: it creates the v3
table, copies the typed v2 columns while preserving `sample_id`, decodes each
row's own legacy payload to intern identity and backfill its per-row uptime,
required memory/swap scalars, optional last PID, and filesystem enumeration
stamp, then replaces the old table and recreates its indexes. A row whose
legacy payload was already absent remains non-assembleable; no value is copied
from a neighbouring row. The transaction also creates `host_identity` and
`fs_mount_events`, backfills mount presence events from existing detail rows,
writes exactly one `schemaMigrated` marker, and sets `user_version = 3`.

Before strict snapshot decoding, the v3 migration normalises only negative
`inodeUsed` and `inodeTotal` fields to absent values. The legacy Bun writer could
produce those impossible counts through an unclamped `inodeTotal - inodeFree`
subtraction on filesystems where `f_ffree > f_files`; the backfill stores neither
inode field, so this loses and invents no typed history. The audit records
`legacyInodeRowsNormalised`, and the `history migration info` line reports the
same affected-row count. The decode guard remains fail-closed for every other
unknown field shape or value, leaving the database untouched for operator
recovery.

The v2→v3 table creation and backfill run in one transaction. A second writer is
serialised by SQLite's write lock, and any failure or crash rolls the transaction
back. The explicit guard is the post-backfill identity-count check: the migration
refuses to commit unless the number of assembleable rows equals the number of
decoded JSON rows.

Schema v4 is also one transaction. It rebuilds both `process_samples_fast` and
`process_samples` into their canonical fresh-v4 column order, converting
`started_at TEXT` with
`CAST(strftime('%s', started_at) AS INTEGER) * 1000`. Unparseable non-NULL text
becomes NULL and is counted as `startedAtUnparsed` in both the migration marker
and the information log. Before replacing either source table, the migration
requires its rebuilt row count to match exactly; any mismatch or other failure
rolls the transaction back. It recreates all three process indexes, adds
`gpu_percent` to the minute tier, creates `gpu_adapters` and `gpu_samples`, and
writes one `schemaMigrated` marker before setting `user_version = 4`.

The v0 path still performs its existing pre-image/VACUUM migration first and
then chains into v1→v2→v3→v4; v1 and v2 follow the same remaining chain, and v3
runs only the v3→v4 transaction. Unsupported schema versions are refused with
`unsupported SQLite schema version <version> at <path> (supported version is
4)`. The v0 pre-image is never overwritten, replaced, or deleted automatically.
`tinytop-agent db pre-image status` inspects its canonical path and main-database
state; `tinytop-agent db pre-image remove --yes` removes only that exact file and
refuses unless the main database has completed schema v1 and passes integrity checking.

### Filesystems on change

The filesystem key is `fs_key_ms`: the snapshot's
`filesystemsCapturedAtMs`, or the sample capture time when a collector does not
provide a stamp. A row is written only for a new enumeration and only when a
mount is new or its filesystem/type, size, used, available, or inode values
changed. Enumeration stamps are monotonic: a stamp older than the last processed
stamp keeps the metric row, stores no filesystem rows or events, and warns at
most once per minute of `captured_at_ms`; a stamp equal to the last one is the
steady state and writes or warns nothing. `fs_mount_events` records each
appearance and disappearance. Reads use
the selected sample's stored stamp, include a mount only when its latest event
at or before that stamp says present, and take its latest values at or before
that stamp. Retention keeps each mount's newest row and newest event as a
carry-forward floor; a mount that has been absent past the horizon can be fully
pruned.

### GPU samples

`gpu_adapters` interns each platform-stable adapter ID, so `gpu_samples` stores
an integer foreign key rather than repeating the PCI-style string every tick.
The writer primes this adapter cache at connect. A new adapter records its
identity plus `first_seen_ms` and `last_seen_ms`; an existing adapter's
`last_seen_ms` is updated at most once per minute. Each detected adapter writes
one sample per fast tick, including nullable busy, memory, and temperature
values. Hosts with no detected adapter write no GPU rows.

GPU samples share the L1 horizon and are pruned by the dedicated GPU step after
L1 raw pruning. A settings-import dry run reports matching rows as
`wouldDelete.gpuSampleRows`, using the candidate L1 cutoff, so a shorter horizon
does not hide this deletion.

### Identity interning

`host_identity` interns the eight stable identity strings. The writer primes
its current identity from the newest sample and inserts a new row only when one
of those strings changes. Uptime is deliberately per sample, not identity data,
so it round-trips exactly across reboots.

### Assembling a history snapshot

`/api/history` is built from typed tables. A row is assembleable iff
`identity_id IS NOT NULL`; the database constraint requires its non-optional
uptime, memory-available, and swap-free scalars as well. The assembler: (1)
reads the metric row and identity; (2) restores its typed CPU, memory, swap, and
load fields; (3) resolves filesystems by stamp, change rows, and presence
events; (4) selects fast process rows or the eligible minute capture; (5) reads
all GPU rows for the history window in one batch and attaches each capture's
adapters; and (6) returns the original snapshot envelope oldest first.
`cpu.times` and all `pressure.*.some`/`pressure.*.full` lines are absent because
v4 does not store
them; `load.runnable`, `load.totalThreads`, and `load.lastPid` are absent where
the collector has no source.

## Write Path

1. The Rust daemon collection loop runs every `HISTORY_POLL_MS` milliseconds. In legacy Bun mode, the collector timer calls `/snapshot/collect`.
2. The collector reads local Linux/WSL sources.
3. `tinytop-store` writes the metric sample, filesystem changes, and GPU adapter/sample rows in one transaction, then writes the per-tick process rows in a separate transaction. Each process command is interned with `INSERT OR IGNORE` followed by a dictionary lookup; the resulting `command_id` is used by both process tables.
4. The fast process transaction replaces rows at the same `captured_at_ms`, preserving idempotent collection while keeping metric readers independent.
5. Once per detail interval, the existing minute-tier `process_samples` capture is written with the same dictionary IDs.

## Read Path

History windows are assembled from typed tables. Process history uses the table
selected by its retention window:

```sql
SELECT p.captured_at_ms, p.rank, p.pid, c.command,
       p.cpu_percent, p.memory_percent, p.rss_bytes,
       p.parent_pid, p.started_at_ms, p.gpu_percent
FROM process_samples_fast AS p
JOIN process_commands AS c ON c.command_id = p.command_id
ORDER BY p.captured_at_ms DESC, p.rank;
```

History window:

```sql
-- The same query shape is used with process_samples for minute history.
-- Table choice is made from the requested since_ms and processFastKeepHours.
```

For `/api/history/processes`, the source is `fast` iff `since_ms` is present
and `since_ms >= now_ms - processFastKeepHours × 3,600,000`; an open-ended or
older window uses the minute table. The `until_ms` value needs no additional
check because the handler's contract does not allow it beyond now. The SQLite
owner reverses selected captures before returning them so callers receive
oldest-to-newest data. Rust history endpoints accept camelCase range parameters
(`sinceMs`, `untilMs`) and retain the existing snake_case aliases.

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

Chart-ready ladder points:

```text
/api/history/points?source=auto&sinceMs=<range-start>&untilMs=<range-end>&limit=<page-size>
```

The Rust daemon accepts `auto`, `raw`, `rollup` (the unchanged one-minute name), `5m`, `1h`, and `archive`. The response keeps `points` and adds top-level `source`, `resolutionMs`, and `available`. A queryable archive returns hourly points with `{source:"archive", resolutionMs:3600000, available:true}`. An explicit archive request while `retentionLadder.archive.queryable` is false remains an empty `{available:false, points:[]}` page.

`auto` uses the HTTP-clamped limit (1–10,000, default 120), `untilMs` or the current time, and the configured L1 poll interval rather than `Tier::L1`'s zero sentinel. In finest-to-coarsest order it selects the first enabled tier that retains the range start and satisfies `rangeMs / resolutionMs <= limit` using integer division. When retaining tiers exist but none fits, it selects the coarsest retaining tier so the caller gets the newest bounded page rather than a fine-tier sliver. When no tier retains the start, it selects archive if queryable archive is enabled, otherwise the coarsest enabled tier. L4 `keepDays: 0` always retains the start.

For `/api/history/points`, the effective page limit is the supplied `limit` clamped to 1–10,000; a direct store caller that passes `None` gets 10,000, and the resolver and reader use that same value, while raw `/api/history` keeps its 120-row default.

Typed detail reads are Rust-only and bounded:

```text
/api/history/filesystems?sinceMs=<start>&untilMs=<end>&mount=/data&limit=<rows>
/api/history/gpus?sinceMs=<start>&untilMs=<end>&adapter=pci-0000:02:00.0&limit=<rows>
/api/history/processes?sinceMs=<start>&untilMs=<end>&limit=<captures>
```

Filesystem results are ordered oldest-first and may be filtered by exact mount.
GPU results are oldest-first, accept an optional exact stable-ID filter, and
limit rows. Process results are grouped by `capturedAtMs`; the limit applies to
capture timestamps, so a page never cuts a ranked process group in half. All
three limits clamp to 1–10,000.

Timeline markers:

```text
/api/history/markers?since_ms=<range-start>&until_ms=<range-end>&expected_gap_ms=<gap>
```

The Rust daemon returns persisted `daemonStart`, `settingsChange`, and one-time `schemaMigrated` events from `app_events`, plus computed `coverageGap` markers inferred from raw sample spacing.

## Rollups And Coverage

The Rust daemon maintains a fixed-resolution ladder: L1 raw samples, L2 one-minute rollups, L3 five-minute rollups, and L4 hourly rollups. Rollups are additive to the raw history table; `/api/history` returns recent snapshots assembled from the typed tables (no `cpu.times`, no pressure lines; `load.runnable`/`totalThreads`/`lastPid` are absent when the collector has no source).

Every rung uses the same `fold` rule. `sample_count` is the sum of the finer rows' counts, averages are weighted by those counts, minima are the minimum of minima, and maxima are the maximum of maxima. Root-filesystem utilization ignores finer buckets that have no root value and becomes `NULL` only when none of them reported one. This preserves all represented measurements instead of selecting one sample or averaging already-aggregated averages. Legacy L2 rows whose v1 minimum/root-maximum columns are `NULL` read their corresponding average as the missing bound, so they remain promotable without claiming reconstructed detail.

Each insert compares the affected L2 minute's existing `sample_count` with the number of raw rows still present after the raw upsert. If the existing count is greater, the raw rows are provably partial: that minute is folded from the frozen bucket plus the new sample and is never rebuilt from the partial tail. Open minutes, and duplicate-timestamp upserts whose raw and rollup counts are equal, continue to rebuild from raw so replacements remain exact. If a late or clock-skewed insert lands behind an L3 or L4 fold watermark, the insert then re-folds the already-promoted five-minute and hourly ancestors. Persisted `l3Enabled`/`l4Enabled` state prevents this repair path from writing a disabled tier after it has retained an old watermark.

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
- a `tiers` entry for each of L1 through L4 with enabled state, retention days, resolution, count, and oldest/newest bucket timestamps
- configured detail sampling interval
- last persisted disk state as `freeBytes`, `minFreeBytes`, `pressure`, `pressureSinceMs`, and `lastCheckMs`
- queryable archive configuration plus real bucket count/range, and cold archive configuration/state
- the persisted schema-migration document, or `null` on a fresh v4 database

## Retention

Rust maintenance runs after each insert in this order:

- Every successful scheduled or manual Rust collection writes one raw row into `metric_samples`.
- The insert refreshes its L2 minute and repairs already-promoted ancestors for a late write.
- Maintenance promotes at most 50 complete L3 buckets, then at most 50 complete L4 buckets. A bucket is complete only when its end plus `max(3 seconds, 2 × poll interval)` has passed. L4 folds from L2 when L3 is disabled.
- Each successful promotion advances `history_state.l3FoldedUntilMs` or `l4FoldedUntilMs` to the promoted bucket's end. Pruning uses the watermarks visible at the start of the tick, so a newly promoted range becomes deletion authority on the next tick.
- L1 rows are deleted after their horizon without rebuilding any L2 bucket.
- GPU sample rows are deleted after the same L1 horizon; interned adapter identity rows remain for reconnects and future samples.
- Filesystems are checked on the configured cadence (60 seconds by default), recorded only on change, and pruned with their presence-event floor; minute-tier process detail is retained through the L2 horizon.
- Per-tick process rows are retained for `retentionLadder.processFastKeepHours` (1–72 hours, default 24); older process windows fall back to the minute tier. Fast rows are deleted in LIMIT-bounded batches using a row-value `IN` query because `process_samples_fast` is `WITHOUT ROWID`.
- Orphan dictionary commands are pruned in a bounded batch only after process rows were deleted in that maintenance pass. The indexed `command_id` probes cover both process tables.
- An L2 bucket can be deleted only when its end is older than the L2 horizon and no later than the nearest enabled coarser watermark. L3 uses the same rule against L4. L4 expires by its own horizon; `0` means forever.
- A disabled L3 or L4 table is neither written nor pruned. It is removed from the dependency chain, while its existing rows remain untouched until the tier is re-enabled.
- `/api/history` selects bounded Rust-daemon windows and returns only assembleable typed rows; reads do not delete rows, but Rust daemon maintenance prunes according to settings. The legacy Bun split path remains a separate, legacy database format.
- The dashboard hydrates the browser-selected timestamp window. Live, 15m, and 1h use paged `/api/history`; every preset from 6h through All makes one `/api/history/points?source=auto&limit=10000` request. The response reports `source` (`raw`, `rollup`, `5m`, `1h`, or `archive`) and `resolutionMs`; non-raw points retain their `sampleCount`. At the default ladder the server returns 6h → 1 minute (360 points), 24h → 1 minute (1,440), 7d → 5 minutes (2,016), 30d → 5 minutes (8,640), 90d → 1 hour (2,160), and 1y → 1 hour (8,760). All starts at the oldest retained data and uses the coarsest tier that holds it; the newest 10,000 hourly buckets cover about 416 days, while the queryable archive holds the rest. A long preset is disabled only when no enabled tier holds its start and the archive is not queryable; a missing tier record does not count as coverage. If the active preset becomes unavailable, the browser refetches the nearest finer preset without persisting the fallback. On Bun, missing coverage disables 6h and longer with a Rust-daemon tooltip while raw presets remain available. Raw windows and browser rendering may still be paged/downsampled; those are transport/rendering limits, not storage limits.
- `Clear` in the dashboard clears only the current browser tab's loaded samples and leaves SQLite untouched.
- Legacy Bun split mode keeps the earlier manual archive/reset behavior.

`DashboardSettings.retentionLadder` is the Rust maintenance authority:

- L1 defaults to 3 days (range 3–3,650) and L2 to 30 days (range 7–3,650); both are always enabled.
- L3 defaults to enabled for 90 days and must be at least L2 when enabled. L4 defaults to enabled for 730 days; `0` means forever, otherwise it must be at least L3, or L2 when L3 is disabled.
- Filesystems default to a 60-second check cadence (range 15–3,600 seconds), and per-tick process rows default to 24 hours (range 1–72).
- Cold archive configuration requires queryable archive configuration, cold-after is 1–120 months, and an archive directory is empty or absolute.
- Disk-check configuration defaults to every 60 minutes and 5 GiB minimum free space; the interval is 5–1,440 minutes and the threshold cannot be below 256 MiB.

Every explicit settings save validates the complete block and writes `retentionHours = l1.keepDays × 24` plus `rollupRetentionDays = l2.keepDays` for Bun compatibility. These legacy fields are derived mirrors: a typed save that edits only `retentionHours` or `rollupRetentionDays` is overwritten from the authoritative ladder. The save transaction also updates `history_state.l3Enabled` and `l4Enabled`, so a late insert immediately after disabling a tier cannot refold into it before the next maintenance tick.

`DashboardSettings::from_document` is the only decoder for settings documents that may lack `retentionLadder`. It derives a stored pre-ladder document in memory from the legacy fields (`ceil(retentionHours / 24)`, floored at 3 days; rollup days floored at 7) without rewriting it, and merges a legacy-only update onto the persisted ladder. The Task 10 import endpoint must use this decoder.

If `history_state.diskPressure.active` is present, saves that extend a horizon, enable L3/L4, or enable an archive are refused with `disk pressure active: free X < minFreeBytes Y; shrink first or free disk`; shrinking remains allowed. The ladder validator owns this rule for both pure validation and the persisted settings path.

## Settings transfer

The Rust daemon exports settings in an envelope with `tinytopConfigVersion`, `exportedAtMs`, `agentVersion`, and `settings`. `MAX_CONFIG_VERSION` is currently 1. The envelope is independent of the agent release and secret-free by construction: `DashboardSettings` contains no credential, and settings for external integrations name environment variables instead of holding their values.

The `otel` block is part of the settings document, while `headersEnvVar` is only the name of an environment variable containing request headers. Header values are never exported. Because the block is additive, an imported document that omits `otel` keeps the daemon's persisted OTel settings, and `tinytopConfigVersion` remains 1; this preserves settings-transfer compatibility with 0.4.1 documents.

Import is split into a read-only plan and an authoritative apply. Planning rejects unknown envelope keys and unsupported versions, decodes through `DashboardSettings::from_document`, normalizes the legacy L1/L2 mirrors, validates the candidate against the persisted ladder and disk-pressure state, and reports ignored unknown keys inside `settings` as warnings. It returns changed keys and `wouldDelete` without writing. Applying repeats that plan and then calls `put_settings`, whose `BEGIN IMMEDIATE` validation is authoritative if pressure changes between preview and write.

`wouldDelete` uses the same predicates as maintenance: L1 and `gpuSampleRows`
count `captured_at_ms < cutoff` at the candidate L1 horizon; rollups count
`bucket_start_ms + resolution <= cutoff`; and `processFastRows` counts per-tick
process rows older than the candidate `processFastKeepHours` horizon. The
candidate ladder is converted through the maintenance configuration builder,
so disabled tiers count zero and forever L4 counts zero. Queryable-archive L4
counts are the rows that leave `main` and move to the archive. Counts mean
“older than the candidate horizon”: they can include rows already past the
current horizon but not yet pruned because L2/L3 deletion is watermark-gated
and each tick is bounded. The counts describe rows matching those predicates at
preview time; maintenance may take several ticks to reach that number, and rows
that cross the candidate cutoff between the preview and a maintenance tick are
pruned as well—the ladder working as configured, not a preview error.

HTTP import runs maintenance after the settings write and then records a `settingsChange` marker labelled `Settings imported` with `{"source":"import","changed":[…]}`. CLI import records the same marker but never runs maintenance: a running daemon re-reads settings on every collection tick and at startup, while running pruning from a second process beside the daemon would violate the maintenance ownership boundary.

Both CLI verbs inspect an existing writable database without creating or migrating it. They refuse a missing database and `user_version = 0`; import also refuses unknown top-level keys, a config version newer than 1, invalid settings, and retention growth under active disk pressure. The dry-run prints the same plan shape used by the dashboard.

`config export --out` publishes a synced temporary file with a same-directory hard link when the filesystem supports atomic no-clobber links. Where hard links are unsupported, it re-checks that the target is absent and then renames the temporary file; that fallback has a window of a few microseconds in which a file created by another process could be replaced.

## Disk check

The Rust daemon checks immediately when its disk task starts, then sleeps for the current `retentionLadder.diskCheck.intervalMinutes`. Settings are read again at the start of every iteration, so a changed interval applies after the current sleep, on the next tick. The check measures the main database's parent directory using the longest matching mount prefix from ADR 0017. Mount enumeration and its per-mount `statvfs` work run on a Tokio blocking thread so a slow network mount cannot stall the HTTP runtime. The report also includes SQLite's current database bytes.

The state machine uses the exact boundary `freeBytes < minFreeBytes`, with no hysteresis:

- An inactive state that crosses below the minimum becomes active, records `sinceMs`, and emits one `diskPressure` timeline marker.
- A continuing breach refreshes `freeBytes` and `minFreeBytes`, preserves `sinceMs`, and emits no additional marker.
- An active state at or above the minimum becomes inactive, clears `sinceMs`, and emits one `diskRecovered` marker.
- A continuing healthy state refreshes the measured bytes without emitting a marker.

Without hysteresis, a flapping filesystem emits at most one transition marker per configured check interval during a continuous daemon run. Because every daemon start performs an immediate check, a restart may emit a marker sooner than one interval after the previous run's last marker.

Each successful check writes `diskPressure`, `lastDiskCheckMs`, and any transition marker in one SQLite transaction. A stopped process therefore cannot commit only part of a transition. If free space is undeterminable, ADR 0020 requires no write at all: the last pressure state and check time remain visible, no marker is emitted, and the daemon logs the path-specific error. A daemon restart runs another check immediately.

Disk pressure never deletes history. While active, it only refuses the growth operations described in [Retention](#retention)—extending a horizon or enabling a tier/archive—while allowing shrink operations. The dashboard shows the pressure banner, and `/api/history/coverage` plus `db stats --json` expose `freeBytes`, `minFreeBytes`, `pressure`, `pressureSinceMs`, and `lastCheckMs`.

## Archive

The Rust daemon's queryable archive is `history-archive.sqlite`. With an empty
`retentionLadder.archive.directory` it lives in the main database's directory;
otherwise the validated absolute directory is used. The archive has
`user_version = 1`, an hourly `metric_rollups_1h` table identical to the main
L4 table (including the newest-time index), and `archive_manifest` for the later
cold-export phase. The tables, index, and `PRAGMA user_version = 1` are created
inside one SQLite transaction, so interrupted initialization leaves no partial
schema for the foreign-file guard to refuse on retry.

When queryable archiving is enabled and L4 has a finite horizon, maintenance
moves at most ten batches of 1,000 expired rows per tick. Each batch uses one
main-pool connection: schema setup uses a standalone archive connection, then
the main connection attaches the archive only for the move. Immediately after
ATTACH it sets `PRAGMA archive.synchronous = FULL` and reads the value back;
moving refuses unless SQLite reports `2` (`FULL`), while `main` remains at
`NORMAL`. Phase A uses a deferred `BEGIN`, selects the oldest expired range,
copies it with `INSERT OR REPLACE`, and commits the archive-only write. Outside
a transaction, verification counts selected main keys that exist in the
committed archive; extra archive keys inside the numeric interval do not count.
Phase B uses `BEGIN IMMEDIATE` and deletes a main row only when all 18 non-key
columns equal its archive copy (`IS` for the three nullable root-used columns).
When every selected row was deleted, the same transaction writes
`history_state.archiveMovedUntilMs`; a partial batch leaves the advisory
watermark unchanged. DETACH brackets every attached operation, and a failed
DETACH discards the connection rather than returning it attached to the pool.
With queryable archiving disabled, finite-horizon L4 expiry remains a direct
main-table deletion and does not create an archive file.

ADRs 0018 and 0019 record the two-commit order and its durability boundary: with
`main` in WAL, SQLite uses no super-journal and commits attached files in ATTACH
order, so one cross-file transaction would make main's DELETE durable before
the archive's INSERT; `archive.synchronous = FULL` additionally fsyncs phase A
before phase B can commit. The crash matrix is therefore: before phase A
commits, nothing changes; between the commits, the batch is in both files and
the archive copy is fsynced, so the next call converges through `OR REPLACE`;
after phase B commits, the move and its watermark are done. A batch absent from
both files is unreachable after either a process kill or a power cut. Failures
through `commit copy` remove nothing from main. A `verify`, `begin delete`,
`delete`, `watermark`, or `commit delete` failure leaves the committed archive
copy in place while phase B is absent or rolled back, so nothing is deleted
from main and retry refreshes the copy. A content mismatch is not an error: the
changed main row stays, successfully matched rows may be deleted, and the
partial batch does not advance the watermark; the next copy refreshes it. A
`detach` failure after a successful phase B leaves both data commits and the
watermark complete, with the connection discarded. If an earlier operation and
DETACH both fail, the earlier actionable error is returned and the detach
failure is logged with its step.

Archive point reads and coverage never attach and never create. They first
check for the file, then use a dedicated `read_only(true)`,
`create_if_missing(false)` SQLite connection. A missing file is an empty
archive; an existing archive reports its real count and minimum/maximum hourly
bucket starts. Archive points reuse the L4 bucket-to-history-point mapping and
are returned oldest first with one-hour resolution.

### Cold export

When `retentionLadder.archive.cold` and `queryable` are enabled, the daemon
checks once an hour for complete, fully moved UTC calendar months in the queryable archive.
A month is exportable only when it is at least `coldAfterMonths` old, every hour
in it has expired from main (`end_of_month_ms + l4_keep_ms + one day <= now`),
it is later than `coldExportedUntilMonth`, and main holds no rows for that
month. The pass checks candidates in order and stops at the first month that
still has main rows, so the monotone watermark never seals a month while its
bounded archive move is still catching up. Disabled L4 and `l4.keepDays: 0`
(forever) therefore have no exportable months. Eligible months are exported
oldest first, at most 12 per pass; `db archive status` lists recorded manifest
months without that per-pass bound. A row that arrives late for an
already exported month remains queryable in `history-archive.sqlite`; the
sealed CSV is not silently rewritten on a later pass.
A month with no archived rows is not exportable and is skipped.

Each month produces `tinytop-1h-YYYY-MM.csv.gz` and
`tinytop-1h-YYYY-MM.csv.gz.sha256` in the archive directory. The gzip level is
6. The CSV header is the archive DDL column order, data rows are ordered by
`bucket_start_ms`, records use RFC 4180 CRLF endings, numbers use their Rust
decimal representation, and SQL `NULL` is an empty field. The export is first
written to `.csv.gz.tmp`, fsynced, streamed through SHA-256, and then decoded
again. That verification must reproduce the selected row count and the first
and last bucket timestamps before the file is atomically renamed. The daemon
then fsyncs the directory on Unix and, in order, writes the checksum sidecar as
`<hex>  <filename>`, records the file in `archive_manifest`, and advances the
main-database watermark. The manifest, rather than an unrecorded stale file, is
the committed cold-export record.

Every failure leaves the queryable archive untouched and is safe to retry:

- `cold months` or `cold read` leaves no new file or state.
- `cold write`, `cold fsync`, `cold hash`, or `cold verify` may leave a `.tmp`;
  it is safe to delete, and neither manifest nor watermark advances.
- `cold rename` leaves either the `.tmp` or the previous target in place, with
  no new manifest or watermark.
- `cold directory fsync` leaves the verified target in place with no sidecar,
  manifest row, or watermark; retrying re-exports and replaces it.
- `cold sidecar` may leave the verified target without a matching sidecar.
- `cold manifest` may leave the target and sidecar without a manifest row.
- `cold watermark` may leave a manifest row with the older main watermark.

The last four cases are convergent: retrying re-exports and replaces the
month before continuing. Cold export never deletes rows from
`history-archive.sqlite`. To verify a file independently, run this in the
archive directory:

```bash
sha256sum -c tinytop-1h-YYYY-MM.csv.gz.sha256
```

## Future Tables

Potential future normalized tables:

- `pressure_samples`

Pressure detail remains outside the typed-history model until a consumer
requires it. See [ADR 0024](adr/0024-schema-v3-typed-history-identity-interning-filesystems-on-change.md).

## Operational Notes

Stop every TinyTop writer before starting a populated-v0 migration, including
the Rust daemon and the legacy Bun collector. The migration deliberately has no
cross-process lock spanning `VACUUM INTO` and the following schema transaction;
allowing another writer in that interval could make the pre-image and migrated
database describe different points in time. Normal single-owner runtime rules
prevent this after startup, but migration remains an operator-controlled
maintenance boundary.

SQLite may create sidecar files beside the database:

- `history.sqlite-wal`
- `history.sqlite-shm`

Back up all three when the Rust daemon or legacy Bun collector is running, or stop the SQLite owner before copying only `history.sqlite`.

See [docs/guides/OPERATIONS.md](guides/OPERATIONS.md) for inspection, backup, and reset commands.
