# ADR 0014 — Two archive tiers: a queryable SQLite sidecar and cold gzip CSV months

**Status:** Proposed (2026-08-28) — awaiting Michel's go. Depends on [ADR 0013](0013-tiered-history-ladder.md).

## Context

Michel asked for both: a queryable archive in its own database, and a cold archive of compressed files, and asked whether to compress the SQLite files as-is or exported flat files. Measured on the live 8,599 rollup rows (13 columns): gzip-6 — SQLite (vacuumed) 279 KB, **CSV 242 KB**, JSONL 314 KB; xz-6 — 189 / 185 / 221 KB. Per hourly row: CSV.gz ≈ 28 B.

## Decision

1. **Queryable archive:** expired L4 (1 h) rows are *moved* into `history-archive.sqlite` (same directory as the main DB unless `retentionLadder.archive.directory` is set), same `metric_rollups_1h` shape, `user_version 1`, plus an `archive_manifest` table. The move is a per-batch transaction over `ATTACH … AS archive`: insert, verify the count, delete from main, detach. The dashboard can read it (`source=archive`, and `auto` falls through to it for ranges older than L4).
2. **Cold archive:** months of the archive older than `coldAfterMonths` are exported as `tinytop-1h-YYYY-MM.csv.gz` (RFC 4180, header = DDL column order, gzip level 6) with a `sha256sum -c`-compatible `.sha256` sidecar, written to a `.tmp`, fsynced, **re-read and verified** (row count, first/last bucket) before rename, and recorded in `archive_manifest`. Cold export never deletes from the queryable archive in this ADR.
3. Compression codec: **gzip via `flate2`** (pure-Rust `miniz_oxide` backend; 1.1.x at planning time, vetted and pinned by the implementing lane per rule 5). Not zstd: the ~20 % gain does not pay for a C-binding dependency, and `.gz` is readable by `zcat`, DuckDB, pandas and Excel without help.

## Alternatives rejected

- **Compress the SQLite file as-is.** Compresses worse than CSV (page/b-tree overhead), and a `.sqlite.gz` is opaque until fully decompressed; not queryable, not partially readable.
- **JSONL.** Largest of the three (schema repeated per row); no tooling advantage over CSV for fixed-shape rows.
- **Parquet.** Best compression and columnar reads, but a heavy dependency (`arrow`/`parquet` crates) for a dashboard daemon; revisit if an analytics consumer appears.
- **Single archive file per row-move without a manifest.** A manifest is what lets `db archive status` and a future restore verify what was exported and when.

## Consequences

- Main DB size is bounded by the configured horizons; the archive DB grows ~1 MB/year at hourly resolution and can live on another filesystem.
- A cold month is a self-verifying artifact (`sha256sum -c`), portable to any tool.
- Two new settings (`archive.queryable`, `archive.cold`) and a directory; cold requires queryable.
