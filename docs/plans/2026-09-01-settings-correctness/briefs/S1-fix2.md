You are hexe lane **S1-fix2** for tinytop. Your base is `main` at `bcaa405` (= `v0.8.0`). This is a **post-release fix** from the blind review of lane S1: the D1 regression test can pass on a host where it cannot possibly detect the defect.

**The shipped behaviour is correct and is not in question.** Claims 1–10 of the review were satisfied at source: the SQLite branch connects once, reads settings, configures before collecting, propagates failures, stamps `captured_at_ms` after collection, and preserves insert-before-close ordering; the no-`--sqlite` branch never resolves the default database. **Do not change `collect_to`'s logic.** You are fixing a test and one changelog line.

Read first: `CLAUDE.md`; `briefs/S1.md` and `briefs/S1-fix1.md` (what this test is for); `docs/plans/2026-09-01-settings-correctness-plan.md` §D1.

Scope rules: edits ONLY in `agent/crates/tinytop-agent/src/main.rs` (the one test) and `CHANGELOG.md`. **Untouchable:** everything else — `writer.rs`, `agent/assets/dashboard/**`, every other crate, `README.md`, `docs/adr/**`, `PROGRESS.md`, `Cargo.toml`, `Cargo.lock`, `VERSION`.

## F1 — `collect_with_sqlite_honours_persisted_top_process_count` is false-green on a small host

`main.rs` (search for the test name; it sits near `:1399-1423` at this base — verify before editing).

Today it persists `topProcessCount = 3`, collects, asserts `inserted_process_count <= 3`, then runs a *second* bare collect and compares:

```rust
if default_process_count <= 3 {
    eprintln!("host exposes only {default_process_count} processes, so the ... comparison is vacuous");
} else {
    assert!(default_process_count > inserted_process_count, ...);
}
```

**The problem.** The discriminating power depends on how many processes the *host* happens to expose. On this workstation a default collect returns 8, so the contrast fires and the test is meaningful. **Inside a hexe containment lane the process table is tiny — the blind reviewer's run reported exactly three** — and there the `else` branch never runs, the remaining assertion is `3 <= 3`, and **the unfixed code would pass too**. A test that cannot fail where CI runs is worse than no test; that is the same class of defect S1-fix1 already repaired in the sibling hermeticity test, so it must not survive a second time.

**The fix: stop depending on the host's process count.** Do not widen the warning, do not raise the threshold, do not mark the test `#[ignore]`.

Collect **twice into two separate fixture databases**, with `topProcessCount` persisted as **1** in one and **2** in the other, then assert:

- the first inserted snapshot has **exactly 1** process,
- the second has **exactly 2**,
- and therefore the first is **strictly fewer** than the second.

Against the pre-fix code both collections use the default of 8 and return `min(8, host_processes)` — the **same** number both times — so the strict-ordering assertion fails on any host exposing at least two processes. That lowers the requirement from "more than 3 processes" to "at least 2", which a host running the test binary always satisfies.

**No silent escape hatch.** If a bare default collect reports fewer than 2 processes, the environment cannot support the test's premise: **fail with a clear message saying so**, do not `eprintln!` and pass. A test that reports success when it could not check anything is exactly the defect being fixed.

Keep the `// Break caught:` comment convention, and update it to describe what the new shape catches.

**RED is required and must be produced honestly.** Show the test failing against the pre-fix `collect_to` (you can reproduce the old behaviour by temporarily building the collector without the settings read — revert that experiment before finishing, and say in your report exactly how you produced the RED). If you cannot produce a genuine RED on this host, **say so plainly** rather than fabricating a transcript.

## F2 — one changelog line still tells a 0.7.2 reader the wrong thing

`CHANGELOG.md`, in the **0.6.0** entry's "Known limitations recorded rather than papered over" bullet (around `:49` — verify): *"`tinytop-agent collect --json` builds a default collector and does **not** read persisted settings, so it never reports sensors."*

That statement was **true of 0.6.0** and a changelog records what shipped, so **do not rewrite or delete it** — rewriting released history is dishonest and is not what is being asked. **Annotate it** instead: append a short parenthetical noting it was fixed in 0.7.2 and pointing at the rule (`collect --sqlite <db>` now loads that database's settings; bare `collect --json` stays hermetic by design). One sentence, inside the existing bullet.

## Rules (non-negotiable)

- git **READ-ONLY** — no add/commit/push/checkout/stash/branch. Leave the tree dirty; report `git diff --stat`. The orchestrator commits.
- Never bare `cargo fmt`. Use `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`; `rustfmt --edition 2024 <file>` for a file you changed.
- Never open `~/.local/share/tinytop/` or `~/.config/systemd/user/`. Never bind a socket, start a daemon, or reach the network. Fixtures in `std::env::temp_dir()` only.
- Do not bump `VERSION` or any crate version — the orchestrator cuts the release.

## Gate

`cargo fmt --manifest-path agent/Cargo.toml --all -- --check`, then `cargo clippy --manifest-path agent/Cargo.toml --workspace --all-targets -- -D warnings`, then `cargo test --manifest-path agent/Cargo.toml --workspace`. Paste all three in full.

**Baseline at `bcaa405`, measured by the orchestrator: 28 suites / 417 passed / 0 failed / 2 ignored.** The test count does not change unless you split the test; say which you did. **Count the `test result:` lines and report the number** — a truncated run and a failing run share an exit code.

Known sandbox limit — state it verbatim and treat it as passed-for-you if it is the ONLY failure: the 10 `serve_contract` tests bind a port and fail with `PermissionDenied` inside containment.

**Report the process counts your environment produced** (bare default collect, and each of the two configured collects). That number is the whole point of this lane, and the orchestrator needs to see it.

## Report (final message, in order)

Files changed with line ranges · exactly how you produced the RED, and the RED/GREEN transcripts · the three process counts · full gate output with your suite count · `git diff --stat` · anything this brief was silent on — do **not** improvise around it.

You are on the lowest model expected to handle this. If the task exceeds you — stuck after a real attempt, looping, or about to guess — stop and reply `ESCALATE: <what is beyond you, and what you tried>`. Escalating early is cheaper than a wrong answer.
