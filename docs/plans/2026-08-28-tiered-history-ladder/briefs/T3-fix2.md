You are hexe lane **T3-fix2** for tinytop: a one-test follow-up to T3-fix1. Your branch starts from `tinytop/ladder-t3-retention-settings-fix1` (F1–F5 already landed there). Do ONLY the edit below.

**Why:** F2 made the ladder authoritative — `put_settings` always rewrites `retention_hours = l1.keep_days * 24` and `rollup_retention_days = l2.keep_days` from the ladder it is given; a typed caller that edits only the legacy field is overwritten (spec §5: derived mirrors; `docs/sqlite-history-architecture.md` :410-414). One pre-T3 test still encodes the old behaviour and now fails: `agent/crates/tinytop-store/tests/sqlite_history_store.rs` `sqlite_store_persists_dashboard_settings` (:174-228) sets `retention_hours: 96` in a `DashboardSettings` literal and asserts `persisted.retention_hours == 96` — it gets 72.

**The edit (that file only):**
1. Run `cargo test --manifest-path agent/Cargo.toml -p tinytop-store --test sqlite_history_store` first and paste the RED (`left: 72, right: 96` at :224).
2. In the struct literal replace `retention_hours: 96,` with `retention_ladder: RetentionLadder { l1: TierKeep { keep_days: 4 }, ..RetentionLadder::default() },` and import `tinytop_store::retention_ladder::{RetentionLadder, TierKeep}` (the module is `pub mod retention_ladder` in `lib.rs:5`; check `lib.rs:15-25` for existing re-exports and use whichever path the file's other imports follow).
3. Keep `assert_eq!(persisted.retention_hours, 96);` (it now proves the derived mirror) and add `assert_eq!(persisted.retention_ladder.l1.keep_days, 4);` right after it.
4. Re-run the same test → paste GREEN. Then `cargo fmt --manifest-path agent/Cargo.toml --all -- --check` and `cargo test --manifest-path agent/Cargo.toml --workspace --no-fail-fast` → paste the tail with every `test result:` line.

Rules (non-negotiable):
- You are in a git worktree on your own branch. **git is READ-ONLY for you**: no `git add`, `commit`, `push`, `checkout`, `stash`. Leave the tree dirty; report `git diff --stat`.
- Never run bare `cargo fmt`. If the check flags this file, run `rustfmt --edition 2024 <that file>` only.
- **Never open, read, copy, or write `~/.local/share/tinytop/` or any file under it.**
- No other file. No new dependencies. Do not "improve" the test beyond the four steps.

**Known sandbox limits — say so verbatim and treat the gate as passed-for-you if these are the ONLY failures:** the 10 `serve_contract` tests in `tinytop-agent` bind a local port your sandbox denies. Fable re-runs the full gate outside before merge. Do not ESCALATE on a sandbox-denied bind.

Report: the RED, the GREEN, the fmt check, the workspace `test result:` lines, `git diff --stat`.

You are on the lowest model expected to handle this. If the task exceeds you — stuck after a real attempt, looping, or about to guess — stop and reply `ESCALATE: <what is beyond you, and what you tried>`.
