# ADR 0019 — Archive move: key-set verify, full-row delete match, fsynced archive commit, watermark inside the delete transaction (supersedes ADR 0018 Decision steps 3–5; records 0018's errata)

**Status:** Accepted (2026-08-29) — decided under the tinytop-ladder GO after the T7 blind review (luna, hexe run 576) of `ae409ab`; implemented by T7-fix2. ADR 0018's context and its two-transaction order (steps 1–2) stand unchanged; its steps 3–5 are replaced by the Decision below. ADR 0018 is not edited.

## Context

ADR 0018 fixed the crash order (copy and commit into the archive before deleting from `main`). The blind review of the implementation found two defects in the clauses that *follow* the order, and its cross-check of the ADR's citations found two factual errors:

1. **Verify counted an interval, not the batch.** Step 3 said "`SELECT COUNT(*) FROM archive … WHERE bucket_start_ms BETWEEN ?min AND ?max` must equal the batch count — `bucket_start_ms` is the primary key on both sides, so the range and the batch are the same row set." False for the archive side: after any partial batch (a row kept in `main` because its payload changed between the phases, or an operator restoring `main` from a backup), the archive legitimately holds keys inside `[min, max]` that `main` no longer has. The next batch's interval count is then larger than its `row_count` **forever** — every call fails at `verify`, maintenance records the error each tick, and archiving silently stops. No data is lost; the feature is dead until someone notices.
2. **The content match compared 2 of 19 columns.** Step 4 deleted a `main` row when its archive twin matched on `sample_count` and `newest_captured_at_ms`. Every production write path that changes an L4 payload also changes those two (fold and refold both merge samples), so the reachable effect is a stale archive copy of a row that changed between the phases — not a lost row — but the predicate is a heuristic where an exact comparison costs nothing.
3. **Errata in 0018's citations.** It cites `libsqlite3-sys 0.38.2` (SQLite 3.53.2) — the reviewer grepped the newest copy in the cargo registry; `agent/Cargo.lock:731-732` pins **0.37.0 = SQLite 3.51.3**. The cited logic is identical there (`sqlite3.c:89783-89790` `aMJNeeded[WAL] = 0`; `:89831-89849` phase one in ATTACH order; `:65419-65433` `pagerWalFrames(…, isCommit = 1)`), so the premise holds; the citation was wrong. It also says `synchronous = FULL` is in force (sqlx's default) — `lib.rs::apply_pragmas` sets **`PRAGMA synchronous = NORMAL`** on the store pool. Under NORMAL, WAL commits are written but not fsynced; a *process kill* still cannot reorder two `write()`s, but a **power cut** can persist `main`'s later DELETE and lose the archive's earlier INSERT that was still in the page cache — the "absent from both" state 0018 declared unreachable becomes reachable across a power loss.
4. **The watermark could be stranded.** Step 5 wrote `archiveMovedUntilMs` after `DETACH` through the pool (the pool has one connection, so it cannot be written while the move holds it); a DETACH failure after a successful delete returned before the write, and nothing repaired it later.

## Decision

On the attached connection, after `ATTACH DATABASE ?1 AS archive` and before phase A:

- **`PRAGMA archive.synchronous = FULL`** (per-database pragma on the attached file). Phase A's `COMMIT` now fsyncs the archive's WAL before it returns; phase B's `main` commit (NORMAL, unchanged — ADR 0013's writer cadence) cannot be written before the copy is durable. Cost: one fsync per batch, ≤ 10 per tick.
- **Verify = key-set existence.** After phase A commits: `SELECT COUNT(*) FROM main.metric_rollups_1h AS m WHERE m.bucket_start_ms BETWEEN ?min AND ?max AND EXISTS (SELECT 1 FROM archive.metric_rollups_1h AS a WHERE a.bucket_start_ms = m.bucket_start_ms)` must equal `row_count`. Extra archive keys inside the interval are irrelevant; a missing one is the failure the step exists to catch.
- **Delete = full-row equality.** Phase B's `DELETE … WHERE bucket_start_ms BETWEEN ?min AND ?max AND EXISTS (SELECT 1 FROM archive.metric_rollups_1h AS a WHERE a.bucket_start_ms = m.bucket_start_ms AND <all 18 remaining columns equal — `=` for the NOT NULL columns, `IS` for `avg/min/max_root_used_percent`>)`. The predicate is written once (`ARCHIVE_ROW_MATCH`). A row that changed between the phases stays and is re-copied (`OR REPLACE`) by the next call.
- **Watermark inside phase B's transaction.** When `rows_affected == row_count`, the same transaction upserts `history_state.archiveMovedUntilMs = max + 3_600_000` with `updated_at_ms = now` through ONE implementation (`history_state_set_on(&mut connection, …)`, which `history_state_set` also calls with a pool connection). The delete and its watermark commit together; a partial batch leaves the watermark alone; there is no post-commit bookkeeping step left to strand.
- **Schema DDL in one transaction** (`BEGIN … COMMIT` around the three `CREATE`s and `PRAGMA user_version = 1`), so a kill mid-schema cannot leave a `user_version 0` file with objects that the foreign-file refusal then rejects forever.
- Remedy text by phase: before the copy commit → "nothing was written to the archive, nothing was deleted from main"; between the commits (`verify`, `begin delete`, `delete`, `commit delete`) → "the archive copy is committed and is refreshed on retry; nothing was deleted from main"; after the delete commit (`detach`) → "the batch is moved; only the detach failed".

## Alternatives rejected

- **Interval count with a tolerance (`>=`)** — hides a genuinely missing key behind extra ones; the check would no longer mean "the copy is there".
- **Deleting by key only (no content match)** — deletes a row whose archive copy is stale when it changed between the phases; the copy would then be silently wrong forever.
- **`synchronous = FULL` on `main`** — an fsync on every 1.5 s insert for the sake of a ≤ 1,000-row hourly batch; ADR 0013's NORMAL stays. Fsyncing only the archive's commit is exactly the ordering the move needs.
- **Editing ADR 0018 in place** — the log never rewrites a decision; this ADR supersedes the affected clauses and records the errata.

## Consequences

- The crash and power-loss matrix in `docs/sqlite-history-architecture.md` `## Archive` is rewritten: before phase A commits → nothing; between the commits → the batch is in both files (fsynced in the archive), the next call converges; after phase B → done, watermark included. "Absent from both" is unreachable for a process kill *and* for a power cut.
- One partial batch no longer stops archiving; a stale copy cannot survive a delete.
- Tests: the exact livelock scenario (archive `{0, 1h}`, `main` `{0, 2h}`) must move and converge; the fix1 crash-order test extends to a payload-only mutation; the idle-connection assertions run on the store's own pool.
- Citation discipline: read the SQLite the lock links (`agent/Cargo.lock`), not the newest copy in the registry.
