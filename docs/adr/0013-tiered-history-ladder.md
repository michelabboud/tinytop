# ADR 0013 — A four-tier history ladder with fold-not-decimate and promote-before-prune

**Status:** Proposed (2026-08-28) — awaiting Michel's go; supersedes the "no multiple rollup tables" rejection in [ADR 0009](0009-additive-history-points-and-markers-api.md) and amends [ADR 0002](0002-initial-snapshot-json-history.md) (full `snapshot_json` per sample).

## Context

The live database measured 4.09 GB as a 72-hour rolling window (one 20 KB `snapshot_json` per 1.5 s sample; the 20 typed columns cost 139 B/row). The prune step rebuilt the rollup of the boundary minute from its surviving tail every tick, so every 1-minute rollup older than 72 h holds one or two samples — the 30-day "minute history" is decimated point readings (4,274 of 4,289 buckets measured). ADR 0009 deliberately stopped at one rollup table; the need for longer, honest history is now measured, and Michel asked for L1 raw → L2 1 min → L3 5 min → L4 1 h with configurable horizons, L3/L4 toggles, and L4 "forever".

## Decision

1. Four fixed-resolution tiers (`metric_samples`, `metric_rollups_1m`, `metric_rollups_5m`, `metric_rollups_1h`) with configurable horizons (`retentionLadder` settings block; defaults 3 d / 30 d / 90 d / 730 d; L1 ≥ 3 d and L2 ≥ 7 d always on; L3/L4 toggleable; L4 `0` = forever). Resolutions are structural, not settings.
2. **Fold, never decimate.** A coarser bucket summarises every finer row in its window: `sample_count = Σ`, `avg` weighted by count, `min` of mins, `max` of maxes. One `fold()` function serves every rung and the archive. Percentiles are out (they do not compose).
3. **A completed bucket is frozen.** Prune never rebuilds a bucket. Only an insert into a bucket rebuilds it; a late write behind a coarser watermark re-folds its ancestors.
4. **Promote before prune.** No row is deleted until every enabled coarser tier has folded past it (fold watermarks in `history_state`).
5. `snapshot_json` is retained only for a recent window (default 60 min; the `1h` preset's floor); filesystems and processes get typed detail tables at a slower cadence (default 60 s) retained for L2's horizon. A one-time v0→v1 migration takes a `VACUUM INTO` pre-image (fail closed, never overwritten, never auto-deleted), rebuilds `metric_samples` with a nullable column, and runs the product's only automatic `VACUUM`.
6. The legacy `retentionHours` / `rollupRetentionDays` keys stay as derived mirrors so the Bun runtime's settings reader keeps working unchanged.

## Alternatives rejected

- **"id + one JSON with all fields."** Measured 8.8× larger than typed columns for the same scalars (JSON stores the schema in every row); it would have kept the 3.5 GB problem while saving 25 MB.
- **Configurable resolutions.** Would require dynamic tables and bucket math driven by settings; the three coarse resolutions cover 6 h → decades of browsing at ≤ 10 k points. YAGNI.
- **Compress `snapshot_json` instead of stripping it.** 5× measured; still ~700 MB for 3 days, and the scalars inside it duplicate the columns. Compression stays an option for the detail tables later.
- **Keep decimation but document it.** Rejected outright — the rollup would remain a lie labelled as a summary.
- **Repair legacy decimated rows.** Impossible; their raw samples are gone. They stay, with `sample_count` telling the truth.

## Consequences

- Steady-state size drops from ~3.5 GB to tens of MB at defaults; a year of hourly history costs ~2 MB; L4 "forever" is affordable.
- A schema `user_version` exists for the first time (0 → 1); future changes bump it and take a pre-image the same way.
- The dashboard gains `90d`, `1y`, `all` presets and a ladder settings group; a shrink confirmation shows server-computed deletion counts.
- `hexe`-style discipline: nothing is deleted without a way back (pre-image) or a coarser summary already written.
