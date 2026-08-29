# 0021 - Cadence classes and typed history without per-sample JSON

## Status

Accepted (2026-08-29 — Michel's go: "Go for the optimization plan fully") for plan `docs/plans/2026-08-29-cadence-classes-and-gpu-plan.md`.

## Context

The daemon collects one `SystemSnapshot` every `pollIntervalMs` (1.5 s) and, since ADR 0002/0004, writes the whole snapshot as JSON into `metric_samples.snapshot_json` beside the typed columns, nulling it after `snapshotJsonKeepMinutes` (60). Measured on the live box on 2026-08-29 (fresh v1 database, 26,068 rows): the table occupied 161 MB, of which 68.8 MB was live JSON and **87.4 MB (54 %) was unused space inside its pages** — a stripped row leaves its mostly-empty leaf page behind, ≈ 3.7 KB per row, permanently. The typed payload is ≈ 174 B per row. One snapshot's JSON is 28 KB: processes 65.6 % (the full argv of ten processes, average 558 characters), filesystems 29.5 % (27 mounts), all other sections 1.25 KB. Filesystem numbers move over minutes; identity never; CPU, memory, swap, load and the process list are the metrics an operator watches at 1.5 s. The dashboard reads nothing from `cpu.times` or `pressure`; `/api/snapshot` was served from the database's JSON although the exporter task already holds the latest snapshot in memory. `targetDatabaseBytes` is display-only.

## Decision

1. **Three cadence classes owned by the collector, one writer tick:** `fast` (uptime, cpu, memory, swap, load, pressure, processes, GPU when detected) every tick; `slow` (filesystems) every `retentionLadder.detailIntervalSec` (default 60), served from the collector's cache in between and stamped `filesystemsCapturedAtMs`; `static` (identity) read at start and on the slow tick.
2. **History is typed; `snapshot_json` is dropped** (schema v3). Raw windows are assembled from `metric_samples` + `host_identity` + `fs_samples` (carried forward) + process rows. `cpu.times` becomes `Option` and, with `pressure`, is absent in history — an explicit, documented omission of fields no consumer reads.
3. **Processes stay fast** in `process_samples_fast` with a `process_commands` dictionary (argv stored once), retained `processFastKeepHours` (default 24); the 60-second `process_samples` rows continue as the minute tier to the L2 horizon. `topProcessCount` is wired to the collector.
4. **Filesystems are stored on change** (size/used/available/inode used/total differ from the last stored row for the mount, or the mount is new) on the slow tick; reads carry the last row forward.
5. `/api/snapshot` answers from the in-memory latest snapshot (`503` before the first collection).
6. Rust-daemon-only (spec §13). Migrations are chained and proven on a copy of the live file; reclaiming an existing file's slack is the operator's `db vacuum`.

## Alternatives rejected

- **Per-metric interval settings / separate timers per class** — more knobs than physics; three loops racing for the collector mutex and the store; the fold and the OTel lock invariant are built around one tick.
- **A shorter JSON window** — the slack is per row; the window length does not change it.
- **Keeping `snapshot_json` nullable-but-unused** — a fake column; rule 1.
- **Storing PSI and `cpu.times` columns** — nothing reads them today; store after a chart exists (backlog).
- **A fold for processes** — a top-N list has no meaningful average; the minute tier is the fold.
- **Enforcing `targetDatabaseBytes`** — a separate decision; the hourly disk check (ADR 0020) is the safety rule.

## Consequences

- Database growth becomes ≈ 174 B per tick for scalars, ≈ 45 B per process row per tick, near zero for filesystems between real changes; expected steady state on the home box ≈ 70–80 MB (to be measured, plan §6).
- History snapshots omit `cpu.times` and `pressure` (documented in README `/api/history`); older settings documents carrying `snapshotJsonKeepMinutes` import with an "ignored" warning; `wouldDelete.snapshotJsonRows` and the coverage JSON counters disappear.
- Four schema versions (v2 dictionary/fast rows, v3 identity/drop JSON, v4 GPU) in one release; the v0→v1 path chains into them.
