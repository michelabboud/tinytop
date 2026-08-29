# Tiered History Ladder — design spec (2026-08-28)

**Status:** at Michel's gate (design agreed in conversation 2026-08-28; implementation waits for his "go").
**Author:** Fable (planner/reviewer). **Executors:** Ari lanes via hexe — Fable does not write code on this.
**Plan:** `docs/plans/2026-08-28-tiered-history-ladder-plan.md`. **ADRs:** 0013 (ladder), 0014 (archive), 0015 (OTel), 0016 (config export/import).

## 1. Why — the measurements this answers

Census of the live `~/.local/share/tinytop/history.sqlite` (Fabulous `docs/reports/2026-08-28-tinytop-history-census.md`):

- 4.09 GB is a **72-hour rolling window**, not accumulation: one sample per 1.5 s, **96 % of each row is `snapshot_json`** (19,959 B) while the 20 typed columns cost 139 B.
- **Defect:** `prune_raw_history` rebuilds the rollup of the minute containing the cutoff from the surviving tail on every tick, so every 1-minute rollup older than 72 h ends up with `sample_count` 1–2 (4,274 of 4,289). "30 days of minute history" is 30 days of single point readings.
- ADR 0009 rejected "multiple rollup tables now"; this spec supersedes that rejection with a measured need.

## 2. Michel's decisions (verbatim intent, 2026-08-28)

1. Four tiers: **L1** raw (poll interval) → **L2** 1 min → **L3** 5 min → **L4** 1 h; defaults **3 d / 30 d / 90 d / 2 y**.
2. Coarser tiers are **aggregates of every finer row in the window, never picked samples** ("normalize the values between 1 and 60, not sample 1 and 61").
3. All horizons configurable; **L1 min 3 d, L2 min 7 d; L1 and L2 always on; L3 and L4 can be toggled off; L4 may be "forever".**
4. **Archive both ways:** a queryable archive in its own SQLite DB, and a cold archive of compressed files. Format decided by measurement: **CSV + gzip** (§9).
5. tinytop **checks disk space once in a while**.
6. **Emit metrics to OpenTelemetry** (push only; tinytop never reads from OTel; existing configuration stays valid and the same).
7. **Export + import configuration.**

Fable's decision on the open question (Q1): **L1 keeps typed columns at the poll interval; `snapshot_json` is kept only for a recent window (default 60 min); processes and filesystems get their own typed detail tables at a slower cadence (default 60 s) retained for L2's horizon.** Rationale: the ladder bounds time at every rung while the cost is per-row size at the bottom rung; without this the ladder governs a 3.5 GB file beautifully.

## 3. Non-goals

- Configurable *resolutions* (1 min / 5 min / 1 h are structural — table names and bucket math). Only horizons and toggles are settings.
- Percentiles at coarse tiers (do not compose; would need sketches). avg/min/max/count only.
- Reading from OpenTelemetry, Prometheus scrape endpoints, or any pull model.
- Multi-host in one DB. `hostname`/`runtime_kind` stay informational.
- UI charts for the new detail tables (API only in this spec; UI later).
- Repairing legacy decimated 1-minute rows (the raw they came from is gone). They stay, honestly labelled by `sample_count`.

## 4. The ladder

| tier | table | resolution | default keep | min | toggle | source of truth for |
|---|---|---|---|---|---|---|
| L1 | `metric_samples` | `pollIntervalMs` (1.5 s) | 3 d | 3 d | always on | `live · 15m · 1h` (raw, paged) |
| L2 | `metric_rollups_1m` | 1 min | 30 d | 7 d | always on | `6h · 24h` (via `auto`, §11 amended) |
| L3 | `metric_rollups_5m` | 5 min | 90 d | ≥ L2 keep | on/off | `7d · 30d` (via `auto`) |
| L4 | `metric_rollups_1h` | 1 h | 730 d | ≥ L3 keep (or ≥ L2 if L3 off) | on/off; `0` = forever | `90d · 1y · all` (via `auto`) |
| archive (queryable) | `history-archive.sqlite` → `metric_rollups_1h` | 1 h | forever | — | `retentionLadder.archive.queryable` | `all` beyond L4 |
| archive (cold) | `<dir>/tinytop-1h-YYYY-MM.csv.gz` + `.sha256` | 1 h | forever | — | `retentionLadder.archive.cold` | external tools |

**Invariant — promote before prune.** No row is deleted until every enabled coarser tier that depends on it has folded it and the fold watermark has passed it. Archive rows are written and verified before the L4 rows they replace are deleted.

**Invariant — a completed bucket is frozen.** A bucket is *complete* when `bucket_end_ms + grace_ms ≤ now`, `grace_ms = max(3000, 2 × pollIntervalMs)`. Prune never rebuilds any bucket. Only an insert whose `captured_at_ms` falls inside a bucket rebuilds it (late/clock-skewed samples); if that bucket lies behind a coarser tier's watermark, the containing coarser buckets are re-folded ("refold on late write").

## 5. Settings (JSON, camelCase, additive)

`DashboardSettings` gains one block. Every field has a serde default so pre-existing `app_settings` documents still parse.

```jsonc
"retentionLadder": {
  "l1": { "keepDays": 3 },                                   // min 3, max 3650
  "l2": { "keepDays": 30 },                                  // min 7, max 3650
  "l3": { "enabled": true, "keepDays": 90 },                 // keepDays ≥ l2.keepDays when enabled; max 3650
  "l4": { "enabled": true, "keepDays": 730 },                // 0 = forever; else ≥ l3.keepDays (or ≥ l2 if l3 off); max 36500
  "snapshotJsonKeepMinutes": 60,                             // min 60 (the 1h preset must always have JSON), max 1440
  "detailIntervalSec": 60,                                   // min 15, max 3600; fs_samples + process_samples cadence
  "archive": {
    "queryable": false,                                      // move expired L4 rows into history-archive.sqlite instead of deleting
    "cold": false,                                           // additionally export archive months to CSV.gz
    "coldAfterMonths": 12,                                   // min 1, max 120; a month is exported once it is this old
    "directory": ""                                          // "" = directory of the main DB; else absolute path
  },
  "diskCheck": { "intervalMinutes": 60, "minFreeBytes": 5368709120 }   // 5 GiB; min 256 MiB; interval 5–1440
}
```

**Backward compatibility (Bun `src/settings.ts` reads `retentionHours` / `rollupRetentionDays`):** both legacy keys stay in the document and in `DashboardSettings`. On read, if `retentionLadder` is absent, derive `l1.keepDays = max(3, ceil(retentionHours / 24))`, `l2.keepDays = max(7, rollupRetentionDays)`. On every save, write `retentionHours = l1.keepDays × 24` and `rollupRetentionDays = l2.keepDays`. The legacy keys are documented as *derived, read-only mirrors* in the UI.

**Validation rules (server; the dashboard mirrors them):** ranges above · `l2.keepDays ≥ 7` · `l1.keepDays ≥ 3` · monotonic: `l3.keepDays ≥ l2.keepDays` when `l3.enabled`; `l4.keepDays == 0 || l4.keepDays ≥ (l3.enabled ? l3.keepDays : l2.keepDays)` when `l4.enabled` · `archive.cold` requires `archive.queryable` · `archive.directory` must be absolute or empty · **disk-pressure rule:** while `history_state.diskPressure.active`, a save that *extends* any horizon, enables a tier, or enables an archive is refused with `StoreError::Validation("disk pressure active: free X < minFreeBytes Y; shrink first or free disk")`; shrinking is always allowed. Errors name the field and the rule, never just "invalid".

`defaultHistoryWindow` gains `"90d"`, `"1y"`, `"all"`.

## 6. Storage schema (`user_version` 0 → 1)

```sql
-- metric_samples: identical columns; snapshot_json becomes NULLABLE (rebuilt once by the v1 migration, §7)
-- metric_rollups_1m: additive columns (NULL on legacy rows)
ALTER TABLE metric_rollups_1m ADD COLUMN min_cpu_usage_percent REAL;
ALTER TABLE metric_rollups_1m ADD COLUMN min_memory_used_percent REAL;
ALTER TABLE metric_rollups_1m ADD COLUMN min_swap_used_percent REAL;
ALTER TABLE metric_rollups_1m ADD COLUMN min_load_percent REAL;
ALTER TABLE metric_rollups_1m ADD COLUMN min_root_used_percent REAL;
ALTER TABLE metric_rollups_1m ADD COLUMN max_root_used_percent REAL;

-- one shape for every coarse tier (also the archive DB's table)
CREATE TABLE IF NOT EXISTS metric_rollups_5m (
  bucket_start_ms INTEGER PRIMARY KEY,
  first_captured_at_ms INTEGER NOT NULL,
  newest_captured_at_ms INTEGER NOT NULL,
  sample_count INTEGER NOT NULL,
  avg_cpu_usage_percent REAL NOT NULL,  min_cpu_usage_percent REAL NOT NULL,  max_cpu_usage_percent REAL NOT NULL,
  avg_memory_used_percent REAL NOT NULL, min_memory_used_percent REAL NOT NULL, max_memory_used_percent REAL NOT NULL,
  avg_swap_used_percent REAL NOT NULL,  min_swap_used_percent REAL NOT NULL,  max_swap_used_percent REAL NOT NULL,
  avg_load_percent REAL NOT NULL,       min_load_percent REAL NOT NULL,       max_load_percent REAL NOT NULL,
  avg_root_used_percent REAL,           min_root_used_percent REAL,           max_root_used_percent REAL
);
CREATE INDEX IF NOT EXISTS idx_metric_rollups_5m_newest ON metric_rollups_5m (newest_captured_at_ms DESC);
CREATE TABLE IF NOT EXISTS metric_rollups_1h ( /* identical columns */ );
CREATE INDEX IF NOT EXISTS idx_metric_rollups_1h_newest ON metric_rollups_1h (newest_captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS history_state (
  state_key TEXT PRIMARY KEY, value_json TEXT NOT NULL, updated_at_ms INTEGER NOT NULL
);
-- keys: l3FoldedUntilMs (i64) · l4FoldedUntilMs (i64) · archiveMovedUntilMs (i64)
--       coldExportedUntilMonth ("YYYY-MM") · diskPressure ({active,sinceMs,freeBytes,minFreeBytes})
--       lastDiskCheckMs (i64) · schemaMigration ({from,to,preImagePath,startedMs,finishedMs})

CREATE TABLE IF NOT EXISTS fs_samples (
  captured_at_ms INTEGER NOT NULL, mount TEXT NOT NULL, filesystem TEXT NOT NULL, fs_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL, used_bytes INTEGER NOT NULL, available_bytes INTEGER NOT NULL, used_percent REAL NOT NULL,
  inode_used_percent REAL, inode_used INTEGER, inode_total INTEGER,
  PRIMARY KEY (captured_at_ms, mount)
);
CREATE INDEX IF NOT EXISTS idx_fs_samples_mount_time ON fs_samples (mount, captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS process_samples (
  captured_at_ms INTEGER NOT NULL, rank INTEGER NOT NULL, pid INTEGER NOT NULL, command TEXT NOT NULL,
  cpu_percent REAL NOT NULL, memory_percent REAL NOT NULL, rss_bytes INTEGER NOT NULL, parent_pid INTEGER, started_at TEXT,
  PRIMARY KEY (captured_at_ms, rank)
);
CREATE INDEX IF NOT EXISTS idx_process_samples_time ON process_samples (captured_at_ms DESC);
```

Archive DB `history-archive.sqlite` (same directory as the main DB unless `archive.directory` set): `user_version 1`, tables `metric_rollups_1h` (identical shape) and `archive_manifest (month TEXT PRIMARY KEY, exported_at_ms INTEGER NOT NULL, file TEXT NOT NULL, sha256 TEXT NOT NULL, row_count INTEGER NOT NULL, bytes INTEGER NOT NULL)`. Opened with `ATTACH DATABASE ? AS archive` only for the duration of a move or a read; never left attached across the pool.

## 7. Migration v0 → v1 (one-time, in `SqliteHistoryStore::connect`)

Runs after the PRAGMAs, before schema creation, when `PRAGMA user_version = 0` and `metric_samples` exists with ≥ 1 row.

`journal_mode=WAL` and `synchronous=NORMAL` are applied before the pre-image; this can write the SQLite header, but no row.

1. **Pre-image, fail closed.** `VACUUM INTO '<db>.pre-v0.sqlite'`. Refuse to proceed if that path already exists (never overwrite), or if free space on the DB's filesystem < 1.2 × current DB bytes. On refusal, `connect` returns `StoreError::Migration(reason, remedy)` and the daemon does **not** start — a silent skip would migrate later without a pre-image.
2. In one transaction: `CREATE TABLE metric_samples_v1 (… snapshot_json TEXT NULL …)`; `INSERT INTO metric_samples_v1 SELECT …, CASE WHEN captured_at_ms >= :now − :keepMs THEN snapshot_json ELSE NULL END FROM metric_samples`; `DROP TABLE metric_samples`; `ALTER TABLE metric_samples_v1 RENAME TO metric_samples`; recreate both indexes; `ALTER TABLE metric_rollups_1m ADD COLUMN …` ×6; create the new tables; `PRAGMA user_version = 1`.
3. Outside the transaction: `VACUUM` (the one automatic VACUUM in the product — it returns the ~3.5 GB). Record `history_state.schemaMigration` and an `app_events` marker `schemaMigrated` with `{from:0,to:1,preImagePath,samplesKept,jsonRowsKept,durationMs,bytesBefore,bytesAfter}`.
4. The pre-image is **never deleted automatically**. `tinytop-agent db pre-image status|remove` manages it explicitly; `remove` refuses unless `user_version ≥ 1` and the main DB passes `PRAGMA integrity_check`.

Fresh databases are created directly at v1. A DB created by the Bun runtime (NOT NULL `snapshot_json`, `user_version 0`) is migrated the first time the Rust daemon opens it.

## 8. Fold semantics (one function, every rung, the archive too)

```rust
pub struct Stat { pub avg: f64, pub min: f64, pub max: f64 }
pub struct TierBucket {
    pub bucket_start_ms: i64, pub first_captured_at_ms: i64, pub newest_captured_at_ms: i64,
    pub sample_count: i64,
    pub cpu: Stat, pub memory: Stat, pub swap: Stat, pub load: Stat,
    pub root_used: Option<Stat>,   // None when no finer row had a value
}
/// Fold finer buckets (each with its own sample_count) into one coarser bucket.
/// Returns None for an empty slice. Weighted by sample_count — never an average of averages.
pub fn fold(bucket_start_ms: i64, finer: &[TierBucket]) -> Option<TierBucket>;
/// A raw sample is a TierBucket with sample_count = 1 and avg = min = max = the value.
pub fn raw_to_bucket(sample: &RawSampleRow) -> TierBucket;
```

Rules: `sample_count = Σ count` · `avg = Σ(avg × count) / Σ count` · `min = min(min)` · `max = max(max)` · `first = min(first)` · `newest = max(newest)` · `root_used`: folded over the finer buckets where `Some`, weighted by *their* counts; `None` if none. A `root_used` fold weighted by bucket `sample_count` is exact whenever the root mount is present on every sample in the bucket (the production case); a separate per-bucket root-observation count would require a schema change and is out of scope. **Legacy 1-minute rows** (min columns NULL): read as `min = COALESCE(min, avg)`, `max_root_used = COALESCE(max_root_used, avg_root_used)`. Tier N folds from the **nearest enabled finer tier** (L4 folds from L2 when L3 is off).

## 9. Maintenance algorithm (`maintain_history`, every tick after insert; convergent and bounded: a tick promotes ≤ 50 buckets; two ticks at one `now` may promote different batches)

```
insert raw row; write detail rows if (now − last_detail_ms) ≥ detailIntervalSec;
rebuild 1m bucket of captured_at_ms from raw (adds min); if bucket < l3FoldedUntilMs → refold its 5m and 1h ancestors;
promote L3: while next 5m bucket B (start ≥ l3FoldedUntilMs) is complete: fold(B) from L2 rows → upsert; l3FoldedUntilMs = B.end   [≤ 50 buckets per tick]
promote L4: same from L3 (or L2 when L3 off), l4FoldedUntilMs                                                                 [≤ 50 per tick]
strip JSON: UPDATE metric_samples SET snapshot_json = NULL WHERE rowid IN (SELECT rowid … WHERE captured_at_ms < now − keepMinutes AND snapshot_json IS NOT NULL LIMIT 500)
prune L1: DELETE metric_samples WHERE captured_at_ms < now − l1.keepDays          (no rebuild; ever)
prune detail: DELETE fs_samples / process_samples WHERE captured_at_ms < now − l2.keepDays
prune L2: DELETE metric_rollups_1m WHERE bucket_start_ms + 60_000 ≤ min(now − l2.keepDays, dependentWatermark(L2))
prune L3: DELETE metric_rollups_5m WHERE bucket_start_ms + 300_000 ≤ min(now − l3.keepDays, dependentWatermark(L3))   [if enabled]
expire L4 (if enabled and keepDays > 0): rows with bucket_start_ms + 3_600_000 ≤ now − l4.keepDays →
    archive.queryable ? move_to_archive(rows) (ATTACH; INSERT OR IGNORE INTO archive.metric_rollups_1h; verify count; DELETE; DETACH — one transaction per batch ≤ 1,000 rows; archiveMovedUntilMs advances)
                      : DELETE
```
`dependentWatermark(T)` = the fold watermark of the nearest enabled coarser tier, or `+∞` when none is enabled. Disabling a tier stops writes to it and drops it from `dependentWatermark`; its existing rows are pruned by its own horizon only when the tier is re-enabled (disabled tables are left untouched — no silent deletion on a toggle). Every step logs counts at `debug`, and anything non-zero deleted at `info`; a step that fails logs at `error` with the SQLite message and the tick continues with the next step (a failed prune must not stop collection).

A replayed sample whose raw row has already been pruned merges through the late-write path; collectors never replay samples, so this is an API/test boundary rather than a production collector flow.

**Hourly (`diskCheck.intervalMinutes`):** free bytes of the DB's mount (from the collector's filesystem snapshot, matching the longest mount prefix of the DB path) and DB bytes. `free < minFreeBytes` → `history_state.diskPressure = {active:true,…}` + marker `diskPressure` (once per breach); recovery → `{active:false}` + marker `diskRecovered`. `diskPressure` never deletes anything; it only refuses growth (§5) and shows a banner.

**Cold export (hourly, when `archive.cold`):** for each month `M` in `archive.metric_rollups_1h` with `M ≤ now − coldAfterMonths` and `M > coldExportedUntilMonth`: write `<dir>/tinytop-1h-YYYY-MM.csv.gz` (header = the table's column names in DDL order; rows ordered by `bucket_start_ms`; RFC 4180; gzip level 6) to a `.tmp` name, fsync, compute sha256, **re-read the gzip and count rows + compare the first/last `bucket_start_ms`**, rename into place, write `.sha256` sidecar (`<hex>  <filename>` — `sha256sum -c` compatible), upsert `archive_manifest`, advance `coldExportedUntilMonth`. Cold export **never deletes** from the archive DB in this spec (a future `archive.coldPrunesQueryable` flag may; not now).

## 10. Read API (Rust daemon; additive; ADR 0009 pattern)

- `HistoryPointMode` gains `Rollup5m` (`"5m"`), `Rollup1h` (`"1h"`), `Archive` (`"archive"`); `"rollup"` keeps meaning 1m.
- **`auto` rule** *(amended 2026-08-28 — lane T4 escalated (hexe run 554) because the plan's original test row `limit 100 over 30 d → L3` was arithmetically impossible under this rule; the rule was right, the row was not, and two latent traps are closed here)*: inputs are `now_ms`, the query, and the effective ladder. `limit` = the query's page limit as the HTTP layer supplies it (clamped 1–10,000; `DEFAULT_HISTORY_LIMIT` 120 when the caller omits it; a direct store caller that passes `None` counts as 10,000). `range_ms = until_ms.unwrap_or(now_ms) − since_ms` — **never 0 because `until` is absent** (the v0.2 resolver's defect). `resolution_ms` per tier = **`pollIntervalMs` for L1** (`Tier::L1.resolution_ms()` is the sentinel 0 and must never be a divisor), 60 000 / 300 000 / 3 600 000 for L2 / L3 / L4. Choose the **finest** tier such that (a) the tier is enabled, (b) `since_ms ≥ now − keepDays × 86 400 000` (the tier still holds the start of the range; L4 `keepDays 0` = forever always satisfies (b)), and (c) `range_ms / resolution_ms ≤ limit` (integer division — the whole range fits in one page). **If tiers satisfy (a)+(b) but none satisfies (c) → the coarsest tier that satisfies (a)+(b)**; the page is then the newest `limit` buckets (existing `ORDER BY … DESC LIMIT`), and `resolutionMs` tells the caller what it got — never an error, never the finest tier's truncated sliver. **If no tier satisfies (b)** → `Archive` when `archive.queryable`, else the coarsest enabled tier. Worked table at the defaults (pollIntervalMs 1 500 · keep 3 / 30 / 90 / 730 d · limit 10 000): 1 h → L1 (2 400 pts) · 2 d → L2 (L1 holds it but 115 200 > limit) · 6 d → L2 (8 640) · 30 d → L3 (L2 holds it but 43 200 > limit; 8 640 fits) · 60 d → L4 (L3 holds it but 17 280 > limit; 1 440 fits) · 300 d → L4 by (b) · 30 d at limit 100 → L4 (nothing fits (c)) · 800 d → `archive` if queryable, else L4. Response gains `"source"` (already present) and `"resolutionMs"`.
- `GET /api/history/coverage` gains: `tiers: [{tier:"l1"|"l2"|"l3"|"l4", enabled, keepDays, resolutionMs, bucketCount, oldestMs, newestMs}]`, `snapshotJsonOldestMs`, `detailIntervalSec`, `disk: {freeBytes, minFreeBytes, pressure, lastCheckMs}`, `archive: {queryable:{enabled,path,bucketCount,oldestMs,newestMs}, cold:{enabled,directory,exportedUntilMonth,fileCount,bytes}}`, `migration: history_state.schemaMigration | null`. Existing fields keep their names.
- `GET /api/history` (raw snapshots): returns only rows with `snapshot_json IS NOT NULL`; documents that its horizon is `snapshotJsonKeepMinutes`.
- New: `GET /api/history/filesystems?sinceMs&untilMs&mount&limit` → rows of `fs_samples`; `GET /api/history/processes?sinceMs&untilMs&limit` → rows grouped by `captured_at_ms`. Both Rust-only, `limit` clamped 1–10,000.
- Config: `GET /api/settings/export` → `{"tinytopConfigVersion":1,"exportedAtMs":…,"agentVersion":"…","settings":{…DashboardSettings…}}` with `Content-Disposition: attachment; filename="tinytop-settings-YYYYMMDD-HHMM.json"`. `POST /api/settings/import` body = that document; `?dryRun=true` returns `{valid, errors[], changedKeys[], wouldDelete:{l1Rows,l2Buckets,l3Buckets,l4Buckets}}` without applying; without `dryRun` it validates (all §5 rules), applies via `put_settings`, runs `maintain_history`, records marker `settingsChange` with `{"source":"import","changed":[…]}`. Unknown top-level keys are rejected (`tinytopConfigVersion` > 1 → error naming the max supported).

## 11. Dashboard (single source `agent/assets/dashboard/`, both runtimes)

- `HISTORY_WINDOWS` gains `"90d"`, `"1y"`, `"all"` (`durationMs` = coverage's oldest across tiers/archive). **Sources (amended 2026-08-28 — T5 built this section's original `90d→"5m"`, `1y→"1h"` and the arithmetic fails: a page is ≤ 10,000 buckets, `30d` at 1 m is 43,200 so the chart silently showed 6.9 d, `90d` at 5 m is 25,920 → 34.7 d):** `live`/`15m`/`1h` stay `raw` (paged); **every preset from `6h` up requests `source=auto&limit=10000`** and renders by the returned `source`/`resolutionMs` — §10's rule (c) picks the finest tier whose whole range fits one page (at the defaults: 6h → 1 m 360 · 24h → 1 m 1,440 · 7d → 5 m 2,016 · 30d → 5 m 8,640 · 90d → 1 h 2,160 · 1y → 1 h 8,760 · all → the coarsest tier holding the oldest data, newest ≤ 10,000 buckets ≈ 416 d at 1 h; the archive holds the rest) and degrades to a coarser tier when a finer one is disabled instead of greying the button. A preset is disabled (tooltip naming the setting) only when NO enabled tier holds its start and the archive is not queryable — reason = `retentionLadder.<coarsest disabled tier>.enabled`, or `retentionLadder.<coarsest enabled tier>.keepDays` when every tier is enabled but too short; raw presets are disabled when `snapshotJsonOldestMs` is null. Fix = T5-fix1 F5.
- Settings dialog gains a **History ladder** group: L1 days (min 3), L2 days (min 7), L3 enabled + days, L4 enabled + days + "keep forever" (writes 0), snapshot JSON minutes, detail interval seconds, Archive (queryable, cold, cold-after months, directory), Disk check (interval, min free GiB). The legacy "History hours" / "Rollup days" inputs become read-only mirrors with a hint "derived from L1/L2".
- Validation mirrors §5 exactly (same messages). **Shrink confirmation:** if a save would shrink any horizon, disable a tier, or disable an archive, a confirm dialog lists what will be deleted, using `/api/settings/import?dryRun=true` on the candidate document (so the numbers come from the server, not the client).
- Coverage card shows the ladder (per-tier oldest/newest/count), disk pressure banner, archive status. Export/Import buttons in the settings dialog (download JSON; upload → dry-run diff → confirm → import).

## 12. OpenTelemetry export (push only)

`DashboardSettings.otel` block: `{ "enabled": false, "endpoint": "http://127.0.0.1:4318/v1/metrics", "protocol": "http/protobuf", "intervalSec": 60, "headersEnvVar": "TINYTOP_OTEL_HEADERS", "serviceName": "tinytop", "resourceAttributes": {} }`. Secrets never live in settings: headers (e.g. auth) are read from the environment variable named by `headersEnvVar` (`k1=v1,k2=v2`, OTLP style) at daemon start and on settings change; the settings export therefore never contains a secret.

Metrics (OTel semantic conventions where they exist; all gauges; unit in brackets): `system.cpu.utilization` [1] · `system.memory.utilization` [1] · `system.memory.usage` [By] (attr `state=used`) · `system.memory.limit` [By] · `system.paging.utilization` [1] (attr `state=used`; this is the semconv name for swap) · `system.cpu.load_average.1m` / `.5m` / `.15m` [{thread}] · `system.filesystem.utilization` [1] (attrs `mountpoint`, `type`) · `system.filesystem.usage` [By] (attrs `mountpoint`, `state=used|free`) · product-specific under the `tinytop.` prefix: `tinytop.load.percent` [1], `tinytop.pressure.some` / `.full` [1] (attr `resource=cpu|memory|io`, emitted only when the collector reports pressure). Resource: `service.name`, `service.version` = agent version, `host.name` = hostname, plus `resourceAttributes`.

Exporter: `opentelemetry` / `opentelemetry_sdk` / `opentelemetry-otlp` at the same version (0.32.x at planning time — the lane vets and pins per rule 5, report in `docs/reports/`), HTTP/protobuf, periodic reader at `intervalSec`, observed values = the latest snapshot at export time (no re-collection). Runs in its own tokio task; export failure increments `otelExportFailures` (exposed in `/api/history/coverage` under `otel: {enabled, endpoint, lastSuccessMs, failures}`) and logs at `warn` with the endpoint and error, never more than once per minute; never blocks or delays collection. Bun runtime: no OTel (documented, like retention).

## 13. Two-runtime invariant — what stays identical

Retention, folding, migration, archive, disk check, OTel and config import are **Rust-daemon-only** (as maintenance already is; ADR 0005/0009 pattern). The dashboard is single-source and identical. Bun's `history-store.ts` changes in exactly one way (found by the T1 review, 2026-08-28): its two raw `SELECT`s filter `snapshot_json IS NOT NULL`, so the raw endpoint's horizon is the JSON window on both runtimes (§10) instead of a `JSON.parse(null)` crash on migrated rows. Its `INSERT` still supplies every column including `snapshot_json`; the legacy settings keys remain present and derived. `bun run check` must stay green after every lane. **Amended 2026-08-28 (T5 lane finding, run 555):** Bun's `src/settings.ts` changes in exactly one way too — `allowedHistoryWindows` gains `7d, 30d, 90d, 1y, all`, the same ten as `DashboardSettings` (`lib.rs:181`), so a dashboard preference validates identically on both runtimes. Bun has no ladder: the dashboard hides the History ladder group and the ladder coverage card when `GET /api/settings` lacks `retentionLadder`, and never sends that key to a runtime that did not return it (T5-fix1 F4).

## 14. CLI additions (`tinytop-agent`)

`db stats` prints the ladder (per-tier counts/oldest/newest), JSON-bearing sample count, archive and disk state. `db pre-image status|remove` (§7). `db archive status|export-now` (runs one cold-export pass and prints the manifest rows written). `config export [--out FILE]` (stdout by default), `config import FILE [--dry-run]` (prints the dry-run report; exit 1 on validation errors).

## 15. Error handling & logging

`StoreError` gains `Validation(String)` (field + rule), `Migration { reason: String, remedy: String }`, `Archive { step: &'static str, source: sqlx::Error | io::Error }`. Every refusal message names the field, the rule, the observed value and the remedy. Nothing swallows errors: a failed maintenance step logs at `error` and the tick continues; a failed migration stops the daemon; a failed cold-export verification leaves the `.tmp` file for inspection and does not advance the watermark.

## 16. Testing (all in temp dirs — the live DB is never opened by a test)

Store: `fold` unit tests (weighted mean vs average-of-averages; NULL `root_used`; legacy min=NULL) · **decimation regression:** insert 40 samples/min for 3 minutes, prune past minute 1, assert minute 1's `sample_count` is still 40 · promote-before-prune: L2 rows older than the horizon survive until `l3FoldedUntilMs` passes them · bounded promotion after a simulated 2-day gap · refold on late write · migration on a fixture v0 DB with 10 JSON rows → pre-image file exists, `user_version` 1, only rows within the keep window still carry JSON, `VACUUM` shrank the file, refusal when the pre-image path pre-exists · archive move round trip (ATTACH; counts equal; main rows gone only after verification) · cold export round trip (CSV.gz re-read row-equal; sha256 sidecar verifies with `sha256sum -c`; a corrupted `.tmp` does not advance the month) · disk check with an injected free-bytes provider (pressure on/off markers, growth refused, shrink allowed) · settings validation (every rule in §5 with its exact message) · legacy alias derivation both ways.
Agent: `auto` tier selection table-driven over (since, enabled tiers, limit) · coverage JSON shape · settings export → import dry-run diff → import applies · OTel exporter against a local HTTP test server asserting one OTLP protobuf `ExportMetricsServiceRequest` with the expected metric names and resource attributes; failure path increments the counter without stalling the collection loop.
Dashboard: Bun test for `HISTORY_WINDOWS` keys and the validation mirror if `app.js` is importable; otherwise a manual checklist in the plan's verification section.

## 17. Phases and versions

Phase 1 (0.3.0): schema+migration, fold+ladder+freeze fix, settings+validation+aliases, read API + coverage, dashboard ladder + presets + confirm, CLI + docs. Phase 2 (0.4.0): queryable archive, cold export, disk check. Phase 3 (0.4.x): config export/import. Phase 4 (0.5.0): OTel export. Each lane bumps the patch; each phase closes with merge → docs → tag → `gh release` → `cargo audit` + `bun audit`.
