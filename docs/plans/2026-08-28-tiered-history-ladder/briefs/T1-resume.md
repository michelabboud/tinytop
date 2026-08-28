# Lane T1 — resume after escalation (2026-08-28)

Your escalation was **correct**: the plan claimed a call path that does not exist —
`tinytop-store` has no dependency on `tinytop-collectors`, and the collector's `statvfs` is
Linux-only. Thank you for stopping instead of guessing. The plan is amended on this branch
(commit `PLAN_FIX_SHA`); nothing else changed.

Re-read, in this order, before touching code:

1. `docs/adr/0017-store-owns-its-free-space-check.md` — the decision and why.
2. Plan Task 1 → **Files** (now includes `src/disk.rs` and the one-line `Cargo.toml` change:
   `sysinfo.workspace = true`, already pinned in the workspace) and **Step 3** (the free-space
   check on `sysinfo::Disks`, longest-mount-prefix, fail closed when undeterminable).
3. Plan §Contingencies — the `statvfs` bullet is resolved; do **not** touch `tinytop-collectors`.
4. `briefs/T1.md` — your dependency rule is updated; every other rule stands unchanged.

Then execute Task 1 from **Step 1** exactly as briefed: RED tests first (paste the failing
output), then Step 3, then GREEN, then the full gate (`bun run check:rust`). Unit-test
`disk.rs`'s pure functions on both sides of each rule as Step 3 now says.

Report in the order `briefs/T1.md` specifies. If the amended plan is still silent on something
you need, STOP and report — do not improvise.
