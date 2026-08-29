# ADR 0018 — The archive move commits the copy *before* deleting from main (two transactions, never one cross-database transaction)

**Status:** Accepted (2026-08-29) — decided under the tinytop-ladder GO while T7 (hexe run 573)
was implementing spec §9's wording; amends ADR 0014 Decision 1's move mechanic and spec §9 `:177`
(the spec sentence is corrected by the T7 fix lane, which implements this ADR). ADR 0014 is not
edited — this supersedes that one clause.

## Context

Spec §9 (`:177`) and ADR 0014 describe the move of expired L4 rows into `history-archive.sqlite`
as **one transaction per batch** over `ATTACH … AS archive`: `INSERT OR IGNORE` into the archive,
verify the count, `DELETE` from main, `DETACH`. The T7 brief added the reason for `OR IGNORE`:
"the main DB is WAL, and SQLite does not guarantee atomicity across attached databases in WAL
mode, so a crash between the two file commits can leave a batch **present in both**".

That sentence has the direction backwards. Verified at source in the SQLite the store links
(`libsqlite3-sys 0.38.2`, bundled `sqlite3.c`):

- `vdbeCommit`: the super-journal (the mechanism that makes a multi-file commit atomic) is used
  only when more than one database **needs** it, and `aMJNeeded[WAL] = 0` — a WAL database
  never counts toward `nTrans`. `main` is WAL (sqlx default, and ADR 0013's concurrent-read
  design), so `nTrans ≤ 1` whatever the archive's journal mode → the "simple case":
  `sqlite3BtreeCommitPhaseOne` is called for each database **in ATTACH index order** —
  `main` (0), `temp` (1), `archive` (2).
- `sqlite3PagerCommitPhaseOne` for a WAL pager calls `pagerWalFrames(…, isCommit = 1)`: the
  commit frame is written to that file's `-wal`. The transaction is durable *for that file* at
  that instant — for readers immediately, for power loss after the fsync (`synchronous = FULL`,
  sqlx's default).

So inside the spec's single transaction, **the `DELETE` on main becomes durable before the
`INSERT` into the archive**. A process kill between those two `write()` calls (SIGKILL, OOM,
a crash in the daemon, a power cut) leaves the batch in **neither** file: deleted from main, never
committed to the archive. The window is two syscalls wide and needs no fsync misordering. The
queryable archive exists precisely so these rows are not lost; "present in both" is the one state
the code cannot reach, and "absent from both" is the one it must never reach.

## Decision

`move_expired_l4` performs the move as **two transactions on one pooled connection**, ordered so
that the copy is durable before anything is deleted:

1. `ensure_archive_schema` (standalone connection, as before). `ATTACH DATABASE ?1 AS archive`
   outside any transaction (ATTACH/DETACH cannot run inside one).
2. **Transaction A — copy.** `BEGIN` (deferred: a read snapshot of main, a write lock on the
   archive only) → `SELECT MIN(bucket_start_ms), MAX(bucket_start_ms), COUNT(*)` over the batch
   (the oldest ≤ `batch` rows with `bucket_start_ms + 3_600_000 <= cutoff_ms`) →
   `INSERT OR REPLACE INTO archive.metric_rollups_1h SELECT … FROM main.metric_rollups_1h
   WHERE bucket_start_ms BETWEEN ?min AND ?max` → `COMMIT`. Only the archive's WAL receives
   frames. `OR REPLACE`, not `OR IGNORE`: a copy left by an earlier crash, or a bucket that
   changed after it was copied, is brought up to date instead of frozen stale.
3. **Verify the committed copy** (outside any transaction, i.e. reading what A made durable):
   `SELECT COUNT(*) FROM archive.metric_rollups_1h WHERE bucket_start_ms BETWEEN ?min AND ?max`
   must equal the batch count (`bucket_start_ms` is the primary key on both sides, so the range
   and the batch are the same row set). Anything else → `Err(StoreError::Archive { step:
   "verify", .. })`, nothing deleted, watermark untouched.
4. **Transaction B — delete.** `BEGIN IMMEDIATE` →
   `DELETE FROM main.metric_rollups_1h WHERE bucket_start_ms BETWEEN ?min AND ?max AND EXISTS
   (SELECT 1 FROM archive.metric_rollups_1h a WHERE a.bucket_start_ms = metric_rollups_1h.bucket_start_ms
   AND a.sample_count = metric_rollups_1h.sample_count
   AND a.newest_captured_at_ms = metric_rollups_1h.newest_captured_at_ms)` → `COMMIT`. Only
   main's WAL receives frames. A row that changed between A and B (a late write landing in an
   expired bucket) is **not** deleted — its archive copy is stale — and the next call's
   `OR REPLACE` re-copies it. The function returns the rows B deleted.
5. `history_state_set("archiveMovedUntilMs", max + 3_600_000)` after B commits. `DETACH` on
   every exit path, including after a failed step, so a connection never returns to the pool
   attached.

Crash matrix: before A commits → nothing changed. Between A and B → the batch is in both files;
the next call `REPLACE`s (same content) and deletes — convergent. After B → done (the watermark
may lag one batch; it is advisory, recomputed from the data by the next batch's `SELECT`, never
trusted for correctness). "Absent from both" is unreachable.

## Alternatives rejected

- **One cross-database transaction** (spec §9's original wording; ADR 0014) — main commits
  first, as shown above; loses the batch on a kill between two writes.
- **Rollback-journal mode on both files** so the super-journal makes the pair atomic — main is
  WAL by design (readers never block the 1.5 s writer, ADR 0013); giving that up for a
  ≤ 1,000-row hourly batch is the wrong trade, and `journal_mode` is a per-file property that
  would also change every other connection's behaviour.
- **A standalone archive connection instead of ATTACH** — the rows would cross the process as
  bound parameters (18 columns × ≤ 1,000 rows per batch) with a second copy of the column list;
  ATTACH keeps `INSERT … SELECT` inside SQLite with the identical-shape table. Rejected for
  duplication, not correctness — it would also be lossless.
- **`INSERT OR IGNORE` + content-matched delete** — a bucket that changed between copy and
  delete keeps a stale archive copy forever and is never deleted from main: a stuck row (not a
  lost one) re-attempted every tick. `OR REPLACE` converges.

## Consequences

- Two commits per batch instead of one — two commit frames and two fsyncs at `synchronous =
  FULL` per ≤ 1,000 rows, at most 10 batches per maintenance tick. Negligible against the 1.5 s
  cadence; the bound is unchanged.
- The verify step now reads a *committed* state, so it means what the spec says it means.
- A crash between the two commits is not reproducible from user space without fault injection.
  The invariant is carried by (a) the source citation above, (b) a test that runs the copy step
  alone, asserts the batch is in both files, then runs the full move and asserts convergence
  (main clean, archive count unchanged, no duplicates), and (c) the existing
  `chmod 0o444` failure injection (a failed A leaves main intact and the pool detached).
- `docs/sqlite-history-architecture.md` `## Archive` and spec §9 `:177` describe the two-commit
  order; ADR 0014 Decision 1's "one transaction per batch" clause is superseded by this ADR.
- The T7 brief's `OR IGNORE`/"present in both" sentence is the defect this ADR corrects; the
  lane that implemented it did so as instructed — the error was in the plan, not the hand.
