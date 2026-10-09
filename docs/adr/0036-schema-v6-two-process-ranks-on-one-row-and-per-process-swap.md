# 0036 - Schema v6: two process ranks on one row, and per-process swap

## Status

**Accepted (2026-10-09)** — Task 2 of plan `docs/plans/2026-10-09-memory-ranked-processes-plan.md` (approved by Michel 2026-10-09: "1. go"). Extends ADR 0021 (processes are a fast-cadence class with a command dictionary), ADR 0023 (one-transaction migrations with no pre-image when nothing is lost) and ADR 0025 (the v4 shape of both process tables). It supersedes none of them: no earlier ADR fixes the meaning of `rank`, and the v4 column order is kept and only appended to.

## Context

On 2026-10-09 the host ran out of memory twice (00:19 and 18:30 local). The process history could only partly say who held the memory, for two reasons measured that day:

1. Each sample stored the top N processes **by CPU**. Only 7 of the top 12 by memory were also in the top 12 by CPU.
2. A swapped-out process shows almost no resident memory. At 18:34 the live `mai-core` had 0.1 GB resident and 2.6 GB in swap, so ranking by resident memory alone would still have missed it.

Task 1 (0.13.0) changed the collector: a sample is now the top N by CPU, in CPU order, followed by the members of the top N by memory (resident plus swap) that are not already there, in memory order. Each process appears once and carries `swapBytes`, `cpuRank` and `memoryRank`. A sample therefore holds between N and 2N processes. Until this decision the store dropped the three new fields: `process_samples_fast` (per tick) and `process_samples` (per minute) had one `rank` column, which was the process's position in the only list there was.

The store has to keep both rankings, keep per-process swap, and do it on a live database of about 820 MiB whose two process tables held 458,320 and 337,148 rows on 2026-10-09 (read-only count).

## Decision

1. **Two rank columns on the one row a process already has.** Both tables gain three nullable columns, appended in this order: `swap_bytes INTEGER`, `cpu_rank INTEGER`, `memory_rank INTEGER`. `cpu_rank` and `memory_rank` are zero-based positions in their lists; `NULL` means the process is not in that list. `swap_bytes` is `NULL` when swap is unknown (a platform that does not expose it, a kernel thread, a process that could not be read) — never zero.

2. **`rank` stays the primary-key ordinal and stops meaning "CPU position".** The key stays `(captured_at_ms, rank)` and `rank` is the row's position in the sample as the collector emitted it, `0..len`. Because the collector emits the CPU list first, `rank = cpu_rank` on the first N rows of a sample and history read in `rank` order still shows the CPU ranking first. Nothing else about `rank` is promised.

3. **Schema v6 is `ALTER TABLE … ADD COLUMN`, not a rebuild.** SQLite appends an added column to the stored schema and rewrites no row, and both tables (one of them `WITHOUT ROWID`) accept a nullable column with no default. The fresh-database DDL lists the three columns last so a fresh file and a migrated file have the same `table_info`; a test compares them, with their index names, for a file migrated from v5 and for one migrated from v1.

4. **Existing rows are backfilled with `cpu_rank = rank` and nothing else.** Every row written before v6 was a member of the top N by CPU, stored in CPU order, so its `rank` is its CPU position — that is a fact, not a guess. Its swap and its memory position were never measured; they stay `NULL`.

5. **One transaction, all or nothing, with a shape check before the first write and a verification before the commit.** Inside one transaction the migration: reads the column names of both tables and refuses unless each is exactly the schema-v5 list; adds the three columns and runs the backfill, one table after the other; requires for each table that the row count is unchanged and that no row is left with `cpu_rank` different from `rank` or with a non-`NULL` `swap_bytes` or `memory_rank`; writes one `schemaMigrated` marker with both row counts and the duration; sets `user_version = 6`; commits. Any error returns before the commit, the transaction is rolled back, and the file is a v5 file exactly as before. A failed statement is reported with the statement and SQLite's own message.

6. **No pre-image.** The v0→v1 pre-image exists because that migration deliberately discards JSON (ADR 0013, ADR 0023). v5→v6 deletes nothing and rewrites no existing value, so a pre-image would spend a full copy of the database to protect against nothing the transaction does not already protect against.

7. **The minute tier is still a copy of one tick.** `process_samples` is not folded from `process_samples_fast`: once per detail interval the store writes the due tick's processes to the minute table as they are. There is therefore no rule to invent for "a process whose rank differed between the ticks of a minute" — the minute row carries the ranks and the swap of the one tick it was copied from, as it already did for `cpu_percent` and `rss_bytes`.

8. **Overflow is refused, not clamped, exactly as for `rss_bytes`.** `swap_bytes` goes through the same checked `u64` → `i64` conversion. A value above `i64::MAX` fails the tick's process transaction: no process row of that sample is written to either table, the metric row is kept, and the writer logs its rate-limited warning. Ranks are `u32` and convert without loss; a stored rank or swap that does not fit its type on the way back is a read error, not a wrapped number.

9. **No index on the new columns.** A sample holds at most 2 × 50 rows. Ordering one capture by `memory_rank` is a sort of at most 100 rows that the reader already holds; no query selects across captures by rank.

10. **The archive tiers are not touched.** `history-archive.sqlite` and the cold `tinytop-1h-YYYY-MM.csv.gz` files hold hourly metric buckets only (ADR 0014). Process rows never reach them, so neither format changes.

## Alternatives rejected

- **A second table for the memory list** (`process_memory_samples`, keyed like the first). It keeps `rank` meaning "CPU position" and needs no backfill. Rejected: at N = 12 the two lists shared 7 of 12 members on the day this was measured, so most processes would be stored twice — command id, CPU, memory and start time duplicated — and the two copies of one process in one sample could disagree after a partial write. Every read would join or merge two tables to answer "what was this process doing", the prune, the orphan-command sweep and the import dry-run count would each gain a table, and the question the feature exists for ("who held the memory, and what was their CPU?") would be the join.
- **Replacing CPU ranking with memory ranking.** One list, no new columns. Rejected: it trades one blind spot for the other. The CPU list is what every existing chart, the live table and weeks of stored history mean, and a CPU incident is as real as a memory one.
- **A single combined rank** (a weighted score, or one list interleaved from both). Rejected: it cannot be turned back into either ranking, so neither "top by CPU" nor "top by memory" could be shown truthfully; the weights would be a magic value nobody could defend; and old rows could not be backfilled into it honestly.
- **A table rebuild to place the columns "properly"** (as v4 did). Rejected: nothing about these columns needs a position, a rebuild copies every row and needs the row-count guards a rebuild needs, and `ADD COLUMN` plus a fresh DDL that lists them last yields the identical shape.
- **Backfilling `swap_bytes = 0` or a derived `memory_rank` for old rows.** Rejected: an old sample is the top N by CPU, not by memory, so a memory rank computed inside it would be a rank within the wrong population, and zero swap would be an invented measurement.
- **Dropping `rank` in favour of a key on `(captured_at_ms, pid)`.** Rejected: a pid can recur within one sample only by error, but `rank` is what makes the emitted order — the CPU list first — reproducible from the table, and changing the primary key is the rebuild rejected above.

## Consequences

- **Rows per sample are between N and 2N.** Measured 2026-10-09 at N = 12: the union was 17 rows, about 40 % more process-history rows than the 12 a sample held before. The two process tables and their four indexes were 64.0 MiB at 8 rows per sample (CHANGELOG 0.12.1), so the row count, and with it most of that figure, grows in proportion to the union, up to 2× in the worst case.
- **`rank` is an ordinal.** A consumer that read `rank` as the CPU position is still right for the first N rows of a sample and wrong for the rows after them, which are in the memory list only. Consumers should read `cpuRank` and `memoryRank`.
- **The backfill is only true for rows written by a binary older than 0.13.0.** The 0.13.0 collector already emits the union, and its store wrote every emitted row with `rank` as the ordinal, so a v5 file that a 0.13.0 daemon wrote to would hold memory-only rows that the backfill would label with a CPU rank. 0.13.0 was a checkpoint and was not deployed: the live database's highest `rank` was 11 at N = 12 when read on 2026-10-09. A 0.13.0 binary must not be run against a database that will later be migrated.
- **`NULL` in `memory_rank` has two causes.** In a sample written at v6 it means "not in the memory list"; in a row written before v6 it means "not recorded". They can be told apart per capture: every v6 sample that has processes has a memory list, so a capture in which no row has a `memory_rank` predates v6 and has no memory ranking to show.
- **A snapshot that carries no ranks at all** (one not produced by the Rust collectors) is stored with both ranks `NULL`. The store records what it is given and does not infer a CPU rank at write time; only the one-off backfill does, because there the inference is a known fact about every row.
- **Migration cost, measured 2026-10-09** on a fixture of 460,000 rows in each process table (920,000 rows; the live file had 458,320 and 337,148), 8 rows per capture, the debug test profile, load average 9 on 28 logical CPUs, ext4: **864, 871 and 955 ms** inside the migration, about 1.1–1.2 s for connect, migrate and close. The main file grew from 84.05 MB to 88.20 MB, about 4.5 bytes per backfilled row. The cost of a new row with a populated `swap_bytes` was not measured; from SQLite's record format it is the three header bytes plus up to eight bytes of swap and one or two per rank.
- **The daemon's start has no timeout the migration can hit.** The unit is `Type=simple` with no watchdog and the daemon awaits the store before it serves; the dashboard is unavailable for the second the migration takes. If a migration is refused the daemon exits non-zero and `Restart=on-failure` retries every 3 s, each attempt rolled back — the same behaviour as every earlier migration.
- **There is no downgrade.** A 0.13.0 or older binary refuses a v6 file (`unsupported SQLite schema version 6 … upgrade tinytop-agent`). Going back means restoring the backup the deploy step takes first.
- **A hand-altered process table is refused, not adapted to.** A v5 file whose process tables have any other column list — a column of the same name added by hand, a dropped column, a missing table — stops the daemon with the expected and the found column lists and the remedy (restore a backup or move the file aside). The cost is that such a file needs an operator; the alternative was to alter a table of unknown meaning.
- **The history route's rows gained three optional fields** (`swapBytes`, `cpuRank`, `memoryRank` on `/api/history/processes`, and on the processes of `/api/history` samples). They are omitted when `NULL`, so a client that does not know them sees the rows it saw before, plus the memory-only rows after the first N.
