# ADR 0023 — Schema v2 migration: one transaction, an in-flight guard, no pre-image; `command_id` indexed on both process tables

- **Status:** Accepted (2026-08-29) — written at Task 13 dispatch (plan `docs/plans/2026-08-29-cadence-classes-and-gpu-plan.md`); extends ADR 0021.
- **Deciders:** Fable (planner), Michel (Phase 5 GO 2026-08-29).

## Context

Schema v2 (Task 13) adds `process_commands` (a command-text dictionary) and `process_samples_fast`
(one row per process per collection tick), and moves `process_samples.command` to a `command_id`
foreign key: `ADD COLUMN command_id`, backfill through the dictionary, `DROP COLUMN command`
(SQLite ≥ 3.35; the lock pins libsqlite3-sys 0.37.0 = SQLite 3.51.3, ADR 0019 errata).

The only migration this store has shipped, v0→v1 (ADR 0011/0013 era), takes a `VACUUM INTO`
**pre-image** (`<db>.pre-v0.sqlite`) before it runs, refuses to start while a pre-image exists,
records an audit row in `history_state.schemaMigration`, and finishes with a mandatory `VACUUM`.
The obvious move is to reuse that machinery for v1→v2. Two more questions arrive with the new
tables: whether `prune_orphan_commands` needs indexes, and whether `started_at` (an RFC 3339
string repeated 40 times a minute per process in the fast table) should be normalised now.

## Decision

1. **v1→v2 runs in ONE transaction with an in-flight guard and NO pre-image.** Before any write
   the migration checks `sqlite_version()` ≥ 3.35.0 and refuses with
   `schema migration requires SQLite ≥ 3.35.0 (linked: <version>)` otherwise. Inside the
   transaction: create the two tables and their indexes, add `command_id`, intern the distinct
   commands, backfill, then **assert `COUNT(*) WHERE command_id IS NULL = 0`** — a non-zero count
   returns an error and the transaction rolls back with the file untouched — then drop `command`,
   write an `app_events` `schemaMigrated` marker, set `user_version = 2`, commit. No `VACUUM`
   afterwards (`DROP COLUMN` already rewrites the table; automatic `VACUUM` is out of scope, plan §5).
   No `history_state.schemaMigration` record (that key's shape is the v0→v1 audit, and
   `complete_pending_migration` would misread it).
2. **`command_id` is indexed on both `process_samples` and `process_samples_fast`.**
3. **`started_at` stays a TEXT column in the fast table** in v2; Task 14's identity table
   normalises it. The plan's ≤ 60 B/row acceptance is a target to *measure and report*, not a
   number to reach by moving T14's work forward.
4. **Fresh files are created directly in the v2 shape** (`CREATE_SCHEMA_V2_SQL`, the full DDL
   ending `PRAGMA user_version = 2`); the frozen `CREATE_SCHEMA_V1_SQL` is kept as history for the
   v0 rebuild path and for the v1 test fixture. The v2 arm of `ensure_schema` never re-applies
   the v1 DDL — its last statement would reset `user_version` to 1. The migrated and the fresh
   `process_samples` have an identical `PRAGMA table_info` (the fresh DDL places `command_id`
   last, where `ALTER TABLE ADD COLUMN` puts it); a test pins that.

## Alternatives rejected

- **Reuse the v0→v1 pre-image + audit + VACUUM chain.** The v0→v1 pre-image existed because that
  migration *deliberately lost data* (it NULLed `snapshot_json` outside the keep window) and
  rewrote a multi-GB file; v1→v2 loses nothing — every command string survives in the dictionary
  and the guard proves every row was mapped before the column is dropped. A pre-image would
  double the file's disk footprint at the moment the disk check (ADR 0020) is least happy, and
  `refuse_existing_pre_image` would turn a leftover file into a retry blocker for a migration
  that is atomic anyway. SQLite's transaction *is* the pre-image here.
- **Silent copy-table fallback when `DROP COLUMN` is unavailable.** Plan §4 forbids it: the
  refusal names the linked version; on this fleet it cannot fire.
- **No index on `command_id`; orphan pruning by full scans.** `prune_orphan_commands` asks, for
  every dictionary row, whether either table still references it. Without indexes that is
  O(commands × fast rows) per maintenance pass — with ~300 commands and ~460k fast rows a day
  that is ~10⁸ row visits per pass; maintenance runs on the collection loop. Two small indexes
  make both `NOT EXISTS` probes O(log n). The index on the fast table costs roughly 12–16 B/row;
  the plan's growth estimate (≈ 35 MB / 24 h) already absorbs it. The orphan prune additionally
  runs only after a pass that deleted process rows — a command can only become orphaned by a
  deletion.
- **Intern `started_at` (or a `pid + started_at` identity) in v2.** That is Task 14's schema v3
  identity table; doing half of it here would give T14 a second migration of the same column.
- **LIMIT-bounded fast prune via `rowid`.** `process_samples_fast` is `WITHOUT ROWID` (the plan's
  DDL, chosen for the clustered `(captured_at_ms, rank)` key); the batch delete uses a row-value
  `IN (SELECT captured_at_ms, rank … LIMIT ?)` instead — the same bounded-batch discipline as
  `strip_snapshot_json`, one autocommit statement per batch so the writer interleaves.

## Consequences

- A v1 file migrates on first start of the v2 binary; `db stats --json` exposes `userVersion`
  (new in T13 — it was never in the output) so the migration is observable; the `app_events`
  marker makes it visible on the dashboard's marker rail.
- The v0→v1 path now chains into v1→v2 (`ensure_schema` arm `0` → v1 → v2; arm `1` completes a
  pending v0→v1 `VACUUM` first). `migration_v1.rs` expectations move from `user_version 1` to `2`.
- Any test that fabricates "a newer schema" must use `SCHEMA_VERSION + 1` (= 3), since 2 is now
  supported.
- The refusal path for `sqlite_version()` < 3.35 is unit-tested on the comparator only; it cannot
  be exercised against the bundled 3.51.3 — stated honestly in the gate.
- If the backfill on a real file exceeds 60 s (plan §4), the fix is batching the `UPDATE` by
  `captured_at_ms` ranges inside the same transaction — never skipping the backfill.
