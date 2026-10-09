# Deep review, data path — schema v6, commit c22b57a (task 2: store and migration)

- **Range reviewed:** `e70160c..c22b57a` (`checkpoint/0.14.0`); `65e541b` in between is docs only
- **Plan:** `docs/plans/2026-10-09-memory-ranked-processes-plan.md`, task 2 · **ADR:** 0036
- **Kind / reviewer:** deep review, single reviewer, Claude Opus 5.5 in a separate in-process subagent (same model family as the lane and the coordinator; not a blind pair). Committed content only; the reviewer built and ran in a private `git archive` copy with its own target directory.
- **Date:** 2026-10-09 · **Dev mode:** mvp
- **Verdict (reviewer's words):** **SAFE TO RUN ON THE LIVE DATABASE**, on one condition: the running 0.12.1 daemon must be stopped before any 0.14.0 binary opens the file.
- **Result:** no blocker. Two should-fix, three nits. Coordinator's rulings (foxymai session, Opus 5.5) follow each finding.

## What the reviewer ran

- `cargo test -j 6 --locked -p tinytop-store`: every binary `test result: ok` (`migration_v6` 9 passed, 1 ignored; `migration_v1`–`v4` 9, 6, 5, 10; `process_history` 10).
- The lane's timing test at 460,000 rows per table: `schema v5 → v6 in 1302 ms`, `connect+migrate+close 1560 ms; main file 84054016 -> 88199168 bytes`.
- Three throwaway probes of its own (`tests/review_probe.rs`, private copy only): `3 passed`.
- Read-only queries on the live file: `user_version` 5, WAL, no triggers or views, both process tables in exactly the expected shape, 461,808 fast rows and 337,388 minute rows, every capture exactly 8 rows (ranks 0–7) or 12 rows (ranks 0–11) with the 12-row captures starting at the 15:26:38 UTC settings change, zero non-contiguous captures, latest `daemonStart` markers 0.12.0 and 0.12.1.

## Should-fix

**1. A pre-0.14 daemon that stays running writes rows that land in neither list.**
`agent/crates/tinytop-store/src/migration.rs:1049-1061` (the version is checked once, at connect); `agent/crates/tinytop-agent/src/writer.rs:302` before `:339` (the store is opened and migrated before the port is bound). If a 0.14 binary migrates the file while a 0.12 daemon is still running (`collect --sqlite …`, or `serve` on any port), the old daemon keeps inserting its ten columns into the v6 table with `cpu_rank` and `memory_rank` both NULL. Reproduced by the reviewer's probe P2, six attempts: 13 to 16 rows in neither list each time, 0 errors. Repairable afterwards, but only if someone knows it happened. It also makes one sentence of ADR 0036 and of `docs/sqlite-history-architecture.md` false ("a capture in which no row has a `memory_rank` was written before v6").
- **Ruling: accepted; fixed in code and in the deploy step.** The read path treats a capture in which every row has both ranks NULL as a CPU list (`cpu_rank = rank` at read time), so such rows are never orphaned and nothing has to know it happened. The alternative, a trigger that aborts an insert with both ranks NULL, was rejected: it makes a rule about writers part of the schema and every later migration's shape check. The decision is ADR 0037, which supersedes that sentence of ADR 0036. The deploy step in the plan stops the service before the new binary is started.

**2. A lock collision is reported with the wrong remedy, because the transaction starts deferred.**
`migration.rs:1144` (`pool.begin()`, a plain `BEGIN`) and `:1175-1180` (the error mapping). The transaction reads first and takes the write lock only at the first `ALTER`; a second process holding the lock makes that fail with "database is locked", and the operator is told to look for a trigger or view. Reproduced with two migrators (probe P3, six of six): one succeeds, the other exits with the misleading message; no data touched, the file ends at v6 with one marker. Related and unverified: a second migrator that read version 5 just before the first committed would hit the shape check and be told to move the file aside.
- **Ruling: accepted.** `BEGIN IMMEDIATE` (as `lib.rs:1496` and `:1521` already do), `user_version` re-read inside the transaction so a file another process just migrated is a no-op and not a refusal, and "database is locked" gets its own remedy.

## Nits

**3. The migration cannot detect rows written by a 0.13.0 binary** (`migration.rs:780`, `:791`, `:782`, `:793`): the verification re-checks what the UPDATE just did. Not the case for the live file (every capture has exactly 8 or 12 rows). — **Accepted:** refuse, inside the same transaction, when any capture holds more rows than `MAX_TOP_PROCESS_COUNT`; before 0.13.0 no capture could exceed the configured count.

**4. The CHANGELOG says "no row is deleted or rewritten";** the backfill UPDATE rewrites every process row. — **Accepted:** use the ADR's wording ("rewrites no existing value").

**5. Measured time above the documented range:** 1,302 ms in the reviewer's run against "0.86–0.96 s" in the CHANGELOG and ADR 0036. — **Accepted:** state the range across both measurements; no timeout exists to hit.

## The seven areas

1. **Atomicity — clean.** WAL, `synchronous = NORMAL`, `busy_timeout = 5000`, a pool of one connection; every statement of the migration runs on the transaction, `PRAGMA user_version = 6` included. Probe P1 confirmed the open transaction saw version 6, then tested a copied crash image and a dropped transaction: both came back at version 5, 10 columns, no marker, `integrity_check` ok, and the crash image then migrated normally. The lane's own rollback test fails before the pragma, so it is P1 that proves the pragma rolls back.
2. **The backfill as a claim about history — true for the live file,** not detectable in general (finding 3). The Bun server only ever inserts into `metric_samples`; `config import` writes settings only; `archive.rs`, `maintenance.rs` and `settings_transfer.rs` never reference either process table. No writer stored `rank` with another meaning before 0.13.0.
3. **Startup concurrency —** findings 1 and 2. No lock file or single-instance guard protects the database. Two migrators cannot both commit. The `db stats|check|vacuum|pre-image|archive` and `config` subcommands open without migrating; only `serve` and `collect --sqlite` migrate.
4. **Shape check —** compares column names in order only. Weak against a hand-built table, no risk to data, and it does not wrongly refuse a legitimate v5 file. The lane's chain test from v0–v5 asserts row keys and 13 columns from outside the implementation.
5. **Read and write paths — clean.** 13 columns named and 13 values bound in the same order in both INSERTs; the minute table's `ON CONFLICT` list carries all three columns; all SELECTs read by column name; no string-built SQL carries data. Dropping a tick's process rows while keeping the metric row is the existing behaviour for `rss_bytes`.
6. **Downgrade —** a 0.12.1 `serve` or `collect` refuses a v6 file (tested). An old binary's inspection subcommands do not check the version and do not write process rows.
7. **Anything else —** no injection path (every value is bound; the daemon listens on 127.0.0.1). WAL after migrating 800,000 rows was 47 MB. No start timeout applies.

## The lane's claims

Confirmed: schema version 6 and fresh creation at v6; one transaction for everything; any error leaves v5; idempotent on restart; fresh and migrated `table_info` identical; write, minute-copy and read paths carry the three columns; swap above `i64::MAX` refused like `rss_bytes`; no startup timeout; no process rows in the archive database or cold CSV (code only; no archive file opened). Roughly confirmed: the timing. Contradicted: "no row is … rewritten" (finding 4) and "a capture with no `memory_rank` predates v6" in one case (finding 1).

## Not reviewed

The `tinytop-agent` crate's tests were not run by the reviewer; the dashboard assets; the collectors (task 1, reviewed separately); the Windows and macOS paths; sqlx's rollback-on-drop internals (exercised, not read).
