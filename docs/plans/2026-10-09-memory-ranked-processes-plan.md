# Memory-ranked processes and per-process swap — plan

- **Status:** approved 2026-10-09 (Michel: "1. go"), running
- **Written / approved / last updated:** 2026-10-09 / 2026-10-09 / 2026-10-09
- **Dev mode:** mvp (no `Dev mode:` line in `CLAUDE.md`; the rulebook's default for a project without one)
- **Coordinator:** the foxymai Claude Code session (Opus 5.5). **Tasks run back to back:** when a task's close-out chain finishes, the next one starts in the same turn.
- **Execution and communication mode:** in-process subagents through the Agent tool, one at a time (the three tasks share files, so they are sequential). A lane returns its report to the coordinator; the coordinator reads the diff, runs the close-out chain (version, commit, `checkpoint/<VERSION>` tag, push) and dispatches the review. Lanes have read-only git. If the Agent tool is unavailable the coordinator does the task inline.

## Why

On 2026-10-09 the host ran out of memory twice (00:19 and 18:30 local). tinytop's history could only partly say who held the memory, for two reasons measured that day:

1. The process list is the top N **by CPU**. Only 7 of the top 12 by memory were also in the top 12 by CPU.
2. A process that has been swapped out shows almost no resident memory. At 18:34 the live `mai-core` had 0.1 GB resident and 2.6 GB in swap. Ranking by RSS alone would still miss it.

## What is built

1. **Swap per process**, recorded next to RSS (`VmSwap` from `/proc/<pid>/status` on Linux; absent on platforms that do not expose it).
2. **Two lists per sample:** the top N by CPU and the top N by memory, where memory is RSS plus swap. A process in both lists is stored once.
3. **One setting.** The existing `topProcessCount` (1–50, default 12) applies to both lists. A sample therefore holds between N and 2N rows.
4. **Dashboard and API:** the process table gets a "by CPU / by memory" switch and a Swap column, live and in history; process objects gain `swapBytes`, `cpuRank`, `memoryRank`.

## Storage

Schema version +1. Both `process_samples` and `process_samples_fast` gain three nullable columns: `swap_bytes INTEGER`, `cpu_rank INTEGER`, `memory_rank INTEGER`. `rank` stays the primary-key ordinal of the row within its sample and stops meaning "CPU position" for new rows; `cpu_rank` and `memory_rank` carry the positions (zero-based, `NULL` when the process is not in that list). The migration is `ALTER TABLE … ADD COLUMN` (no table rewrite) and backfills `cpu_rank = rank` for existing rows, in one transaction, following the pattern of the existing migrations and their tests. Rollups from the fast tier to the minute tier carry the three columns.

An ADR records this (two ranks on one row against a second table, and against replacing CPU ranking).

## Threat sketch

- Mode: mvp. The daemon binds `127.0.0.1` only.
- Worth breaking: the history database (weeks of host metrics); nothing worth stealing beyond process command lines, which are already stored.
- Entry points: the local HTTP API (`/api/settings`, the history routes), the settings import document, `/proc`.
- Trust boundary: localhost; anyone on the machine is already trusted with `/proc`.
- Attacker: a local user or a page in the owner's browser reaching `127.0.0.1:4274`.
- Floor items touched: **no data-loss path** (the migration must be transactional, tested from every earlier schema version, and refuse rather than half-apply); **no injection** (new query parameters for sort/rank go through bound parameters and a closed enum, never string-built SQL). No new dependency, no new exposure, no secret.

## Tasks

| # | Task | Owner | Depends on | File boundary | Evidence | Next |
|---|---|---|---|---|---|---|
| 1 | Collector and types: read per-process swap; build the CPU list and the memory list; emit one deduplicated set with `swapBytes`, `cpuRank`, `memoryRank`; measure the cost of the swap read per tick against a named budget and fall back to the minute tier if it exceeds it | Rust lane, Opus 5.5 | — | `agent/crates/tinytop-types`, `agent/crates/tinytop-collectors` | unit tests on fixtures (in both lists, CPU only, memory only, swapped-out process ranks by memory, platform without swap); the measured per-tick cost | 2 |
| 2 | Store and migration: schema +1, three columns on both tables, backfill, write path, rollup, read path | Rust lane, Opus 5.5 | 1 | `agent/crates/tinytop-store`, the writer in `agent/crates/tinytop-agent` | migration tests from every earlier version, including a populated database and an interrupted migration; write-and-read-back tests; row counts between N and 2N | 3 |
| 3 | API and dashboard: fields on the wire, sort switch, Swap column, history view, docs | Lane, Opus 5.5 (Rust API) with the dashboard in the same lane | 2 | `agent/crates/tinytop-agent` routes, `agent/assets/dashboard`, `src/`, `tests/`, `README.md`, `GUIDE.md`, `docs/guides/API.md`, `ARCHITECTURE.md` | Rust and Bun suites; the dialog and manifest guard tests; a rendered check of the table at 720 px in both sort modes | deploy |

Every lane: `-j 6`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_INCREMENTAL=0`, a free-memory check before each build, never bare `cargo fmt`, no touch of the live service or its database.

## Reviews

- Mechanical review per task (Standard tier), on the commit, pipelined.
- Task 2 is on a data path: it gets a **deep** review (Strong tier, a fresh session) before task 3 builds on it.
- One deep review of the whole batch after task 3, before the deploy.
- No Fable seat (Michel's rule of 2026-10-09). No `v*` release tag in this plan; each task ends on `checkpoint/<VERSION>`.

## Deploy

Rebuild and restart `tinytop.service` (Michel, 2026-10-09: "you can freely rebuild and restart the service, no approval needed"). The schema migration runs at that start. Before it: copy the live database to a dated backup with SQLite's backup command and verify it opens. After it: version, schema version, and three consecutive samples showing both ranks and swap.

## Out of scope

The legacy Bun collector's fixed list of ten processes; alerting on swap; anything outside tinytop.
