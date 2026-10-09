# BACKLOG.md — deferred or spotted, not done

One dated line each: date · source · item · status. A line leaves only by being done or by Michel's word.

- 2026-10-09 · 0.12.1 lane report · The legacy Bun collector (`src/collector.ts`) keeps a fixed 10 processes and ignores `topProcessCount` · open
- 2026-10-09 · 0.12.1 lane report · The Bun server replaces a non-numeric settings value with the default instead of refusing it, unlike the Rust daemon · open
- 2026-10-09 · 0.12.1 lane report · The "Processes" label and help text in Settings → General → Daemon are terse enough that the setting was not found · open
- 2026-10-09 · mechanical review of 0.13.0 (finding 3) · pid reuse between the process-table refresh and the `/proc/<pid>/status` read can attach one tick's swap figure to the wrong process; a guard needs the start time compared across both reads · open, accepted limit
- 2026-10-09 · mechanical review of 0.13.0 (finding 6) · The Bun collector has no per-process swap and no CPU/memory ranks, so the two runtimes are no longer behaviourally identical for the process list · open
- 2026-10-09 · task 2 lane report · Test names in `tinytop-store/tests/migration_v1..v4.rs` still say "v4" while asserting schema 6 · open
- 2026-10-09 · task 2 lane report · The minute-tier process INSERT's `ON CONFLICT` clause is unreachable because a `DELETE` precedes it · open
- 2026-10-09 · deep review of schema v6 (finding 2) and its fix lane · Every migration before v5→v6 (`migrate_v1_to_v2` … `migrate_v4_to_v5`, `rebuild_v0_schema`, `apply_schema_groups`, `finish_migration_audit`) still opens a deferred transaction and acts on a version read outside any lock; two processes racing from a file older than v5 get a misleading failure, or worse on the rebuild steps · open
- 2026-10-09 · schema v6 fix lane · The capture-size check cannot catch a 0.13.0 capture at N ≤ 25 (at most 50 rows); 0.13.0 was never deployed anywhere · open, accepted limit (ADR 0037)
- 2026-10-09 · schema v6 fix lane · `docs/adr/README.md`'s one-line description of ADR 0036 still says "nothing is deleted or rewritten" and "0.86–0.96 s" · open
- 2026-10-09 · task 3 lane, confirmed by the coordinator on the live 0.12.1 service · **Per-process CPU is 0.0 % for almost every process in almost every sample** (history: zero non-zero values on 10-02 through 10-07; 63 of 10,776 minute rows on 10-08) while `ps` shows processes at 90 % and more. The "by CPU" list is therefore not ranked by CPU. In the collectors; predates this plan · **done in 0.15.1** (the collector rebuilt sysinfo's CPU list every tick, so the divisor was CPU time since boot)
- 2026-10-09 · task 3 lane report · In the process table's fallback state (stored choice "by memory", capture without a memory list) no column header is highlighted · open
- 2026-10-09 · task 3 lane report · Not checked in a browser: the Bun server's own page, a host with a GPU column at 720 px, compact density at 720 px, a screen reader · open
- 2026-10-09 · 0.15.1 lane report · **The daemon spends about 40 % of one core in its SQLite worker thread** on the 923 MB live database (39 of 47 points on `sqlx-sqlite-worker`, no client connected); a fresh database costs about 6 %. The per-tick insert-plus-maintenance cost grows with database size; needs a profiling lane (`perf` on the worker, `EXPLAIN QUERY PLAN` on the maintenance statements) · open
- 2026-10-09 · 0.15.1 lane report · The macOS/Windows collector (`common.rs`, `SysinfoCollector::refresh`, lines 149–168) rebuilds the CPU list every tick the same way and warms up for 120 ms, so per-process CPU is probably wrong there too; not buildable or runnable on this host · open
- 2026-10-09 · 0.15.1 lane report · Per-process CPU stored before 0.15.1 is wrong in both directions (almost always 0, occasionally several times too high) and the dashboard shows it with no marker · open
- 2026-10-09 · 0.15.1 lane report · `spawn_collection_loop` uses tokio's default burst catch-up, so several collections fire back to back after a stall; `MissedTickBehavior::Delay` may be the better choice · open
- 2026-10-09 · 0.15.1 lane report · The Bun collector's `ps pcpu` is a lifetime average, not the last tick (same unit, different window) · open
- 2026-10-09 · deep batch review (finding 7) · The "no by-memory list" sentence shows above the table for every pre-deploy capture in the By CPU view too; whether a screen reader re-announces it on each poll is untested · open
- 2026-10-09 · deep batch review (finding 8) · A collection within 200 ms of the previous one keeps the previous process table: an exited process is listed once more with unknown swap, and at a poll interval near the 250 ms floor on a slow host every other sample can repeat the table · open
- 2026-10-09 · deep batch review (finding 9) · `docs/guides/API.md` states the 200 ms first-sample window without saying it is Linux only · open
- 2026-10-09 · deep batch review (finding 10) · The v6 migration's busy-error remedy covers `BEGIN IMMEDIATE` and the ALTER/UPDATE statements only; with the write lock held nothing else can return busy · open, no reachable failure
- 2026-10-09 · deep batch review (finding 3, follow-up) · Mark captures older than the first 0.15.1 `daemonStart` event in the dashboard and on the wire, so wrong per-process CPU is not read as right · open
- 2026-10-09 · deep batch review (cleanup) · `bun test` leaves fixture directories under `/tmp` (`tinytop-home-*`, `tinytop-runtime-*`, `tinytop-systemd-units-*`); about 56 from this plan's runs · open
