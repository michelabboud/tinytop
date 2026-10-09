# Mechanical review — 0.13.0, commit e70160c (task 1: collector and types)

- **Commit reviewed:** e70160c (`checkpoint/0.13.0`), parent 0642588
- **Plan:** `docs/plans/2026-10-09-memory-ranked-processes-plan.md`, task 1
- **Kind / reviewer:** mechanical review, Claude Sonnet 5.5, a separate in-process subagent, read-only, committed content only (`git show` / `git diff`); no builds or tests run by the reviewer
- **Date:** 2026-10-09 · **Dev mode:** mvp
- **Result:** no blocker. One should-fix, six nits.
- **Coordinator's rulings** are under each finding (foxymai session, Opus 5.5). Findings were re-read against e70160c before ruling.

## Should-fix

**1. The swap-read budget is neither enforced nor surfaced, and the plan still promised a fallback.**
`agent/crates/tinytop-collectors/src/linux.rs:146` (`SWAP_READ_BUDGET_PER_TICK`), `:160` (`within_budget`), `:252` (`last_swap_scan`). Plan task 1 said "fall back to the minute tier if it exceeds it"; the code deliberately does not, and says so in the doc comment and the CHANGELOG, but the plan file was not amended. `within_budget()` and `last_swap_scan()` have no caller outside tests, so an overrun on a host with about 2,000 or more processes is invisible to the operator. The reviewer checked the arithmetic in the doc comment and it holds (30/1500 = 2 %, 30/250 = 12 %, 15–24 µs per process, 1,250–2,000 processes to reach 30 ms).
- **Ruling: accepted, both halves.** The plan is amended in the same commit as this record (the fallback was rejected by the coordinator on 2026-10-09: a minute-tier swap read leaves the per-tick memory list ranking by RSS alone, which misses the swapped-out process the plan exists to catch). A rate-limited warning on overrun, following `gpu/linux.rs:153`, is a fix for the batch this finding belongs to, done before task 3 builds on the collector.

## Nits

**2. The tie-break test cannot tell "CPU descending" from "pid ascending" in the memory list.** `process_rank.rs:353-382`: pids 7 and 9 tie on footprint and 7 is both the busier and the lower pid; the comment at `:382` asserts nothing. — **Accepted:** give the busier process the higher pid. Same fix commit as finding 1.

**3. pid reuse between the process-table refresh and the `/proc` read is not guarded.** `linux.rs:1078`. One wrong swap figure for one tick, in a window of milliseconds. — **Accepted as a known limit**, recorded in `BACKLOG.md`; guarding it needs the start time compared across both reads and is not worth a tick's cost at this mode.

**4. The live test assumes a `VmSwap:` line on every Linux host.** `tests/linux_collector.rs:849`; a sandboxed kernel may omit it (unverified by the reviewer). — **Accepted:** the test should accept "unknown for every process" as a valid host. Same fix commit.

**5. `PROGRESS.md` carries only a new version number;** its date and status text still describe 0.11.0 (pre-existing). — **Accepted:** brought current at the plan's close-out.

**6. `CLAUDE.md` says the two runtimes "must stay behaviorally identical"** and the Bun collector now diverges (no swap, no ranks, a fixed 10 rows). — **Accepted:** `BACKLOG.md` entry now; `CLAUDE.md` gets the stated exception in task 3, which owns the docs.

**7. An old 7-column process-text line is dropped without a trace** (`linux.rs:876`). The only production producer writes 10 columns and the previous parser also dropped malformed lines. — **No change;** noted.

## Checked and found clean

Ranking at N=1, N above the process count and N=0; once-only emission and both ranks for a process in both lists; a total order under `sort_by` (NaN last, `-0.0` folded); saturating footprint and rank conversions; no panic path, no buffer leak between processes, no descriptor leak and strict `VmSwap` parsing in the `/proc` read; tabs inside a command safe in the 10-column format.

## Claims in the brief

1 (types) confirmed · 2 (ranking) confirmed, with nit 2 · 3 (`/proc` read, format, budget) confirmed, with finding 1 · 4 (`common.rs`) confirmed · 5 (agent files are test code only) confirmed: all hunks sit inside `mod tests` · 6 confirmed on content; the brief said 13 struct literals in `tinytop-store`, the diff shows 11 (33 added lines: 3 in `src/lib.rs`, 30 in tests). The brief's count was wrong, not the commit.

## Not reviewed

The whole of `tests/linux_collector.rs` beyond the process-text, swap fixture, live and ignored tests; the seven store test files beyond their diffs; `Cargo.lock` and script version bumps (skimmed); reproducibility of the cost measurement; runtime behaviour on macOS and Windows; concurrency and soundness of the collector.
