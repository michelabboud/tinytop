# Deep batch review — 0.13.0 through 0.15.1, range 0642588..0c0c4dc

- **Range:** `0642588..0c0c4dc` on `main`, eight commits (e70160c, 65e541b, c22b57a, e2e9e72, c210740, 874a5e6, 668d9f8, 0c0c4dc)
- **Plan:** `docs/plans/2026-10-09-memory-ranked-processes-plan.md` · **ADRs:** 0036, 0037
- **Kind / reviewer:** deep batch review before deploy, single reviewer, Claude Opus 5.5 in a separate in-process subagent. Same model family as the lanes and the coordinator: a fresh session, not a blind pair. Committed content only; built and ran in a private `git archive 0c0c4dc` copy.
- **Date:** 2026-10-09 · **Dev mode:** mvp
- **Verdict (reviewer's words):** **DEPLOY** — the code has no blocker, and the per-process CPU fix is correct and sufficient on Linux. Change the deploy order first (finding 1).
- **Result:** no blocker. Four should-fix (two in the deploy procedure, two in docs), seven nits. Coordinator's rulings (foxymai session, Opus 5.5) follow each finding.

## What the reviewer ran

- `cargo test -j 6 --locked --workspace --no-fail-fast`: exit 0, every binary `test result: ok` (`tinytop_agent` 87 passed; `process_cpu` 3 passed; `migration_v6` 16 passed, 1 ignored; `process_history` 13 passed; `linux_collector` 23 passed, 2 ignored).
- `cargo clippy -j 6 --locked --workspace --all-targets -- -D warnings`: `Finished`, exit 0.
- `bun test`: `377 pass`, `0 fail`, 29 files; both `--check` self-tests `"status": "ok"`.
- `process_cpu` repeated 12 times in sequence and 6 at once: 18 of 18 passed, no child left behind.
- Mutation check on `tests/dashboard-process-views.test.ts`: 4 of 5 mutations turned tests red (finding 5 is the survivor).
- Two private probes on the real collector: a first collection against a one-core busy loop read 89.0 to 94.7 % across ten fresh collectors; a new CPU hog is absent from the CPU list on the first tick that sees it and reads `89.5 %, cpuRank 4` on the next.
- Live database, read-only: `user_version` 5; largest capture 12 rows in both process tables; no triggers or views; both tables in the expected v5 column order.

## Should-fix

**1. Deploy procedure — a crash or reboot after the build migrates the live database before the backup exists.** Plan lines 55–62 at 0c0c4dc; unit template `tinytop:739-741`. The installed unit runs `agent/target/release/tinytop-agent`, the file `cargo build --release` writes, with `Restart=on-failure`, `RestartSec=3`, enabled. If the old daemon dies or WSL restarts between the build and the planned stop, systemd starts the new binary and it migrates with no backup taken. The migration is safe for this file, so what is lost is the way back, not data.
- **Ruling: accepted.** The plan's deploy section is rewritten: build into a separate target directory, keep the old binary, stop, back up, install, start.

**2. Deploy procedure — the 0.12.1 binary is overwritten by the build, so the rollback has nothing to run.** A 0.12.1 `serve` refuses a v6 file (`migration.rs:1124` onward), so the rollback is "restore the backup and run 0.12.1".
- **Ruling: accepted.** Step 2 of the rewritten deploy section copies the running binary aside.

**3. Docs (0c0c4dc) — per-process CPU stored before 0.15.1 is shown as if it were right, and the guides say nothing.** `docs/guides/API.md:231`; `GUIDE.md`; the table at `app.js:2515` and the per-PID trend at `app.js:2253-2266`. Only the CHANGELOG and a BACKLOG line said so.
- **Ruling: accepted for the docs now** (0.15.2); a marker on old captures is a BACKLOG entry.

**4. Docs (668d9f8, not corrected in 0c0c4dc) — `PROGRESS.md` describes a state that no longer exists** (task 3 "unreleased", no mention of the CPU fix) under a header of 0.15.1.
- **Ruling: accepted**, rewritten in 0.15.2.

## Nits

**5. (668d9f8) No test guards the one XSS-relevant line.** `app.js:2513-2514` is safe today; changing `command.textContent = process.command` to `innerHTML` left all 377 Bun tests green. — **Accepted:** a structural guard in 0.15.2 allows `innerHTML` only on the pause button's two constant templates.

**6. (0c0c4dc) The regression test can leave a busy loop running forever** if the test process itself is killed; `BusyChild` kills on drop only. `tests/process_cpu.rs:39-64`. — **Accepted:** the loop is bounded in 0.15.2.

**7. (668d9f8) The "no by-memory list" sentence shows in the By CPU view too,** for every capture recorded before the deploy; screen-reader re-announcement untested. — **BACKLOG.**

**8. (0c0c4dc) The kept process table has two small visible effects the docs do not state:** an exited process listed once more with unknown swap (reproduced), and repeated tables at a poll interval near the 250 ms floor on a slow host (by arithmetic). — **BACKLOG.**

**9. (0c0c4dc) `API.md:231` states the 200 ms first-sample window without saying it is Linux only.** — **BACKLOG.**

**10. (874a5e6) The busy-error remedy covers `BEGIN IMMEDIATE` and the ALTER/UPDATE statements only.** With the write lock held in WAL mode nothing else can return busy. — **No change;** BACKLOG note.

**11. (668d9f8) `API.md` says `processes.length` is between N and 2N;** a host with fewer than N processes returns fewer. — **Accepted,** corrected in 0.15.2.

**Security:** no finding. The one attack story in the batch (a local user starts a process whose command line contains markup; the owner opens the dashboard) ends with the markup shown as literal text: the table cell uses `textContent` and `title`, the Swap cell and the detail dialog use `textContent`, the notice is a constant, and the only `innerHTML` in `app.js` is two constant SVG templates.

## A. Earlier rulings

Mechanical review of 0.13.0: 1 implemented (`linux.rs:185-216`, called at `:391-393`); 2 implemented (`process_rank.rs:353-390`); 3 implemented as a BACKLOG line; 4 not verified by this reviewer (the lane's report shows it red-then-green); 5 partly (stale again, finding 4 here); 6 implemented (`CLAUDE.md:16`).

Deep review of schema v6: 1 implemented correctly (`lib.rs:4302-4312`; fires only when no row of the capture has either rank; applied on all three read paths at `lib.rs:2580`, `:2613`, `:3116`; never writes); 2 implemented (`migration.rs:1238-1252`, all three branches tested); 3 implemented (`HAVING COUNT(*) > ?1`, exactly 50 migrates and 51 is refused, both tested); 4 implemented; 5 implemented in the CHANGELOG and ADR 0037. The busy mapping masks sqlx's extended result code to the low byte and compares with 5: every `SQLITE_BUSY_*` variant and nothing else.

## B. The CPU fix

Root cause as the commit message says, read from sysinfo 0.39.5: `refresh_cpu_list` replaced the CPU state with one whose previous total is 0; the process refresh that followed did not re-read `/proc/stat` inside 200 ms; each process's tick was divided by the machine's CPU time since boot. The fix is sufficient on Linux. The kept table is at most 200 ms old; `memoryPercent` is unaffected; nothing blocks 200 ms on a hot path (the sleep runs only on a collector's first collection); a brand-new hog ranking last by CPU for one tick is sysinfo's documented contract. The regression test asserts behaviour and passed 18 of 18.

## C. The dashboard

The three client cases in `docs/guides/API.md` match the pure rules, and each rule went red when broken. A stored preference outside the two valid values falls back to `cpu`. The counter describes the shown list. Both controls are real buttons in a labelled group; "By memory" uses `aria-disabled` and stays focusable. New classes that set `display` restate `[hidden]`. The layout rules are scoped to `.process-panel`, and `index.html` has one table. Not rendered in a browser by this reviewer (the lane did; see its report in the 0.15.0 changelog entry).

## D. Integration

All three fields are carried collector → store → routes → dashboard and omitted when absent; ranks are zero-based everywhere and never displayed. The legacy Bun path gives the CPU list as received, no memory view, the sentence, and dashes. Every version restatement is at 0.15.1. The reviewer measured the live 0.12.1 daemon at 52 % of one core; the batch did not cause it, but a sample now stores up to twice the process rows (BACKLOG, profiling lane).

## E. The deploy

Beyond findings 1 and 2: `systemctl stop` ends the daemon without closing the database, so the `-wal` file stays and a backup must be SQLite's `.backup`; `Type=simple` means the port answering is the real start signal; a refused migration restarts every 3 s without end, each attempt harmless; nothing checks for another program holding the database. The reviewer's recommended order and its additions to the verification list are now the plan's deploy section.

## Not reviewed

macOS and Windows paths; rendering in a browser or screen reader; a release build; `tests/linux_collector.rs` beyond the 0c0c4dc diff; the bodies of `tests/migration_v6.rs` and `tests/process_history.rs`; ADR 0036 and `docs/sqlite-history-architecture.md` in full; `otel.rs`, `tinytop.ps1` beyond the version line, the wizard; the cost-measurement claims in the CHANGELOG.
