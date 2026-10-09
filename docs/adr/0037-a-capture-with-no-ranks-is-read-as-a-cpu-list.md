# 0037 - A capture with no ranks at all is read as a CPU list; the v6 migration takes the write lock first

## Status

**Accepted (2026-10-09)** — the rulings on the deep review of schema v6 (`docs/reviews/2026-10-09-deep-review-0.14.0-c22b57a-schema-v6.md`, findings 1, 2, 3 and 5), part of plan `docs/plans/2026-10-09-memory-ranked-processes-plan.md`. Extends ADR 0036 and **supersedes one sentence of it**, in its Consequences: "every v6 sample that has processes has a memory list, so a capture in which no row has a `memory_rank` predates v6 and has no memory ranking to show." Such a capture was written by a pre-v6 *writer*, before **or after** the file's migration. Everything else in ADR 0036 stands; ADR 0036 itself is not edited.

## Context

ADR 0036 gave both process tables `cpu_rank` and `memory_rank` and backfilled `cpu_rank = rank` on every row that existed at the migration. It assumed that after the migration only a v6 writer would write.

The deep review showed that assumption can fail (finding 1). The schema version is checked once, when a daemon opens the file. If a 0.14 binary migrates the file while a pre-0.14 daemon is still running — `collect --sqlite …`, or `serve` on any port — the old daemon keeps running its ten-column `INSERT` against the v6 table. SQLite accepts it and leaves `swap_bytes`, `cpu_rank` and `memory_rank` NULL. The reviewer reproduced it six times out of six: 13 to 16 rows per attempt that belonged to neither list, and no error anywhere. Such rows would be read back with no rank, so a consumer that shows "the CPU list" and "the memory list" would show them in neither, and nothing would tell anyone that it had happened.

The same review found three weaknesses in the migration itself:

- **Finding 2.** The transaction started with a plain `BEGIN`, which reads first and asks for the write lock only at the first `ALTER`. A second process holding the lock made that `ALTER` fail with "database is locked", and the operator was told to look for a trigger or a view. Reproduced with two migrators, six of six. A second migrator that had read version 5 just before the first committed would also have met a v6 table in its shape check and been told to move the file aside.
- **Finding 3.** The backfill is true only for rows written by a binary older than 0.13.0 (ADR 0036, Consequences). The migration had no way to notice a file a 0.13.0 binary had written to: its verification re-checked what its own `UPDATE` had just done.
- **Finding 5.** The reviewer measured 1,302 ms where ADR 0036 documents 864–955 ms.

## Decision

1. **A capture in which every row has both `cpu_rank` and `memory_rank` NULL is read as a CPU list.** At read time each row of such a capture is returned with `cpu_rank` equal to its stored `rank`. Its `memory_rank` and `swap_bytes` stay absent. The reasoning is ADR 0036's own: the only writers that produce a capture with no rank at all are the ones that know a single list, the top N by CPU in CPU order, so the row's position *is* its CPU position.

2. **A capture in which at least one row has a rank is returned exactly as stored**, including any row of it that has neither. A v6 writer produced it, and what an unranked row beside ranked ones means is not known. Mixed captures are not repaired.

3. **Nothing is written back.** The rows on disk keep their NULLs. The database stays a record of what each writer wrote; the rule is an interpretation applied to what is read.

4. **The rule lives in one function**, `read_unranked_capture_as_cpu_list` in `agent/crates/tinytop-store/src/lib.rs`, and every read that returns process rows goes through it: both process SELECTs of `read_history` (per-tick and the per-minute fallback), `read_history_processes`, and therefore the sample `insert_snapshot` returns, which is read back through `read_history`'s path. It is evaluated per `captured_at_ms`, per table, over the rows that read returns.

5. **The v5→v6 transaction starts with `BEGIN IMMEDIATE`**, issued on an acquired connection the way the settings writes already do. The write lock is taken before the first read, so a second process waits, up to the connection's 5 s busy timeout, instead of failing later.

6. **`user_version` is read again inside the transaction, and only that value is acted on.** 5: migrate. 6: another process migrated the file while this one waited — nothing is written, the transaction is ended, and the file is treated exactly as one opened at v6 (no second backfill, no second marker, no error). Anything else: refused, with the version found.

7. **"Database is locked" has its own error**: it names the lock, not a trigger or a view, and its remedy is to stop the other tinytop process or wait and start again.

8. **A capture larger than `MAX_TOP_PROCESS_COUNT` (50) refuses the migration**, inside the same transaction and before the first write. Before 0.13.0 a capture held at most the configured process count, which could never be set above 50. A larger capture was written with `rank` as an ordinal, and `cpu_rank = rank` would mislabel its rows. The error names the table, the number of such captures and the size of the largest.

## Alternatives rejected

- **A trigger that aborts an `INSERT` with both ranks NULL.** It would stop the old daemon's rows at the door. Rejected: it makes a rule about *writers* part of the schema, so the fresh DDL, every later migration and every later migration's shape check have to carry and recognise it; and it turns a recoverable labelling gap into lost samples, because the old daemon's process rows for every tick would be refused instead of stored.
- **A repair `UPDATE` at startup** (`SET cpu_rank = rank` on captures with no rank). Rejected: it rewrites history on a guess at every start, it cannot repair what a daemon that is still running writes after the start, and once it has run nothing in the file shows that the situation ever occurred. The read-time rule gives every consumer the same answer and leaves the evidence where it was written.
- **Doing nothing in code and documenting "stop the service first".** The deploy step does say that now. Rejected as the only measure: it protects the one deployment that follows the document, and the failure it leaves behind is silent — no error, no marker, rows that simply appear in neither list.
- **Applying the rule per row** (any row with neither rank gets `cpu_rank = rank`). Rejected: in a capture a v6 writer produced, `rank` is an ordinal, and the rows after the first N are in the memory list only. An unranked row there has no known CPU position.
- **Keying the rule on `cpu_rank` alone** ("no row has a `cpu_rank`"). Rejected: a capture whose rows all carry a `memory_rank` and no `cpu_rank` was written by something that knows the two lists, and would be relabelled.

## Consequences

- **The sentence of ADR 0036 quoted under Status is superseded.** A capture in which no row has a `memory_rank` was written by a pre-v6 writer, before *or after* the file's migration, or was stored from a snapshot that carried no ranks. For a client the two cases now look the same: every row has `cpuRank` equal to `rank`, and no `memoryRank`. On disk they differ: rows from before the migration hold the backfilled `cpu_rank`; rows an old daemon wrote afterwards hold NULL.
- **The same sentence in `docs/sqlite-history-architecture.md` is corrected** (that file is not an ADR).
- **A snapshot that carries no ranks at all is still stored with both ranks NULL** (ADR 0036: the store records what it is given), and is now read back as a CPU list. `insert_snapshot` therefore no longer returns such a snapshot's processes unchanged: they come back with `cpuRank` set to their position. The legacy Bun collector, the one producer in this repository that reports no ranks, ranks its list by CPU (CHANGELOG 0.13.0).
- **A client can still receive a row with neither rank, in one case only: a mixed capture** — at least one row of the capture has a rank and that row has none. The Rust collectors never emit one. It is returned as stored, and a consumer should place such a row in neither list.
- **The rule looks at the rows a read returns.** The reads join `process_commands`, so a row whose `command_id` is NULL is not returned and does not count. The per-minute table allows a NULL `command_id`, but no writer has stored one since schema v2 and the v2 migration refused to leave any (ADR 0023), so no capture is known whose verdict this changes.
- **An old daemon left running is no longer a silent data problem, and still not a supported state.** Its captures hold N rows, are read as a CPU list, and have no memory list and no swap for as long as it runs. The deploy step stops the service before the new binary starts.
- **The capture-size check catches a 0.13.0-written file only when a capture exceeds 50 rows.** A 0.13.0 capture at N = 12 holds at most 24 rows and cannot be told apart from an older binary's capture at N = 24. A v5 file that a 0.13.0 binary wrote to at N ≤ 25 is still migrated, with its memory-only rows labelled as CPU-ranked. ADR 0036's rule is unchanged: a 0.13.0 binary must not be run against a database that will later be migrated. The live database had exactly 8 or 12 rows in every capture when the review read it.
- **Two migrators are safe.** Whichever takes the lock second waits for the first, finds version 6 and opens the file normally. If the lock is held for longer than the 5 s busy timeout the start fails with the lock error, the daemon exits non-zero and `Restart=on-failure` tries again 3 s later, as for any refused migration.
- **Only v5→v6 is hardened.** The earlier steps (v0→v1 through v4→v5), the fresh-schema creation and the migration audit still open a deferred transaction and do not re-read the version. They are reached only by a file older than v5 and are recorded for the backlog; this decision does not change them.
- **`durationMs` in the `schemaMigrated` marker is counted from the moment the write lock is held**, so it does not include time spent waiting for another process.
- **Migration cost, all measurements** on the fixture of 460,000 rows in each process table (920,000 rows; the live file had 461,808 and 337,388 when the review read it), 8 rows per capture, the debug test profile, 28 logical CPUs:
  - ADR 0036, 2026-10-09: 864, 871 and 955 ms inside the migration.
  - The deep review, 2026-10-09: 1,302 ms inside the migration, 1,560 ms for connect, migrate and close.
  - This decision, 2026-10-09, with `BEGIN IMMEDIATE` and the capture-size check, six runs at load average 3–6: **937, 952, 1,006, 1,010, 1,018 and 1,081 ms** inside the migration, 1,121–1,277 ms for connect, migrate and close. With the capture-size check removed, in the same session: 910, 933 and 1,211 ms. The check — one `GROUP BY captured_at_ms` per table — costs less than the spread between runs.
  - The range to quote is therefore **0.86–1.30 s** inside the migration, and it supersedes "864, 871 and 955 ms" as the expected figure. No start timeout exists that it could reach (ADR 0036).
- **The read cost of the rule** is one pass over the rows of each capture already in memory, at most 2 × 50 rows. It was not measured separately; the existing 2,400-sample assembly test still runs inside its budget.
