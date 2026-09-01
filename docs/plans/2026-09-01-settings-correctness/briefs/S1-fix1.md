You are hexe lane **S1-fix1** for tinytop. Your base is the S1 lane branch `tinytop/settings-s1-collect-honours-settings` at `86ad62b`. The implementation of D1 is **done and correct** — do not redesign it. You are finishing three things the first lane never reached, and fixing one test it got wrong.

**Do not revisit the design.** The rule stands: *the collector is configured from the settings stored in the database the rows are going into.* `collect --json` alone stays hermetic; `collect --json --sqlite <db>` loads that database's settings first. If you think the implementation is wrong, report it — do not change it.

Read first: `CLAUDE.md`; `briefs/S1.md` (the contract you are completing); `git log -1` (what the orchestrator preserved, and why it is marked INCOMPLETE); `docs/plans/2026-09-01-settings-correctness-plan.md` (the amendment block at the top).

Scope rules: edits ONLY in `agent/crates/tinytop-agent/src/main.rs` (tests + the one test's helpers), `README.md`, `docs/guides/API.md`, `CHANGELOG.md`. **Untouchable:** `writer.rs` (lane S2 owns it next; the one-word change there is already correct and complete), `agent/assets/dashboard/**` (lane S3), every other crate, `Cargo.toml`, `Cargo.lock`, `PROGRESS.md`, `docs/adr/**`.

## F1 — `collect_without_sqlite_opens_no_database` is RED, and the assertion cannot ever hold

The test does:

```rust
let snapshot: serde_json::Value = serde_json::from_slice(&stdout)…;
let expected_stdout = format!("{}\n", serde_json::to_string_pretty(&snapshot)…);
assert_eq!(stdout, expected_stdout.as_bytes());
```

This is a round-trip tautology **and it is impossible**: `serde_json::Value` stores objects in a `BTreeMap`, so parsing and re-serialising emits the keys **sorted alphabetically**, while the real output comes from `SystemSnapshot`'s field declaration order (the failing left-hand side begins `{\n  "timestamp":`). It also proves nothing about this lane's change even if it passed — it only tests serde.

**Delete that assertion.** Replace it with one that says something true about the contract: stdout parses as JSON and carries the snapshot's expected top-level shape (e.g. a `timestamp` key and a `processes` array). Keep it cheap; the point of this test is the *absence of a database*, not the JSON.

## F2 — the same test's hermeticity assertions are currently unfalsifiable

The test builds `temporary_home` / `temporary_state` and then asserts no `.sqlite` file appears there — **but it never wires those paths to anything.** `collect()` resolves its default through `default_sqlite_url()`, which reads the real environment. So a regression that *did* consult the default path would create the file under the **real** `~/.local/share/tinytop/` — the live database directory — and this test would still pass green.

That is worse than no test: it is a guard that reports success while the thing it guards is broken, and its failure mode writes into the live data directory.

**Preferred fix:** make it falsifiable by pointing the default resolution at the fixture. `main.rs:1136` shows `default_sqlite_url_from_env` honours **`TINYTOP_HISTORY_DB`** before any other path. Set it to a path inside the fixture directory for the duration of the test, then assert that path was never created. A regression that calls `default_sqlite_url()` then lands in the fixture and fails the test loudly.

**The constraint you must handle:** `std::env::set_var` is `unsafe` in edition 2024 and mutates process-global state shared with every other test running in parallel. If you do this, guard the mutation (a `static` mutex the test acquires), restore the previous value on the way out including on panic, and make sure no other test in this binary reads that variable concurrently. Check before assuming.

**If you judge that unsafe env mutation is not worth destabilising this suite, that is an acceptable answer — but then you must DELETE the misleading assertions** and state plainly in a comment (and in your report) that hermeticity is guaranteed structurally by the `if let Some(sqlite_url)` branch and is not covered by a test. **What you may not do is leave an assertion that cannot fail.** Say which of the two you chose and why.

## F3 — the docs still document the defect as shipped behaviour

The first lane stopped before reaching these. They must be **rewritten, not amended**:

- `README.md:300-303` — the bullet beginning "**`tinytop-agent collect --json` does not read persisted settings**". Replace it with the rule, stated for both modes.
- `README.md:357` — "...and `tinytop-agent collect --json` uses the default." Same treatment for `topProcessCount`.

Then **search the whole of `README.md`, `GUIDE.md` and `docs/guides/API.md`** for any other sentence asserting that `collect` ignores settings, and rewrite each. Report the complete list of what you found. If `docs/guides/API.md` has no CLI `collect` section, **say so** — do not invent one.

**Do not touch `README.md:294-299`** (the "disabling thermals permits at most one more scan" note). That is D2, owned by lane S2, and editing it here will conflict.

## F4 — `CHANGELOG.md`

There is no entry yet. Add one bullet under `## Unreleased`, phrased as the rule and naming both modes. **A `## Unreleased` section already exists on `main` from lane S3** — if your base has it, add to it rather than creating a second one; if a conflict appears at merge, the orchestrator keeps both bullets.

## Rules (non-negotiable)

- git **READ-ONLY** — no add/commit/push/checkout/stash/branch. Leave the tree dirty; report `git diff --stat`. The orchestrator commits.
- Never bare `cargo fmt`. Use `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`; `rustfmt --edition 2024 <file>` for a file you changed.
- Never open `~/.local/share/tinytop/` or `~/.config/systemd/user/`. Never bind a socket, start a daemon, or reach the network. Tests use `std::env::temp_dir()` only.
- `Cargo.toml` / `Cargo.lock` unchanged.

## Gate

`cargo fmt --manifest-path agent/Cargo.toml --all -- --check`, then `cargo clippy --manifest-path agent/Cargo.toml --workspace --all-targets -- -D warnings`, then `cargo test --manifest-path agent/Cargo.toml --workspace`. Paste all three in full.

**Baseline: 28 suites / 411 passed / 0 failed / 2 ignored** on `main` before this work, measured by the orchestrator. Your base adds three tests, one of which is currently RED — so a correct run shows **414 passed / 0 failed** across 28 suites.

**Count the `test result:` lines and report the number.** The previous run stopped after a single suite because of the failure, and reported "1 suite" — a truncated run and a failing run share an exit code, so the count is the only honest check.

Known sandbox limit — state it verbatim and treat it as passed-for-you if it is the ONLY failure: the 10 `serve_contract` tests bind a port and fail with `PermissionDenied` inside containment. **Anything else is yours.**

## Report (final message, in order)

Files changed with line ranges · which F2 option you chose and why · the full gate output with your suite count · the complete list of doc sentences you found and rewrote · `git diff --stat` · anything this brief was silent on — do **not** improvise around it, report it.

You are on the lowest model expected to handle this. If the task exceeds you — stuck after a real attempt, looping, or about to guess — stop and reply `ESCALATE: <what is beyond you, and what you tried>`. Escalating early is cheaper than a wrong answer.
