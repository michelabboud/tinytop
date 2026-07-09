# Fable Night Sweep — tinytop (2026-07-09)

Fleet-wide sweep for small, safe fix/improvement plans (Michel's directive).
**This is a plan document only — nothing below has been implemented.**

## Health snapshot

- **Version:** `0.2.4` per `VERSION`, clean working tree. Recent history is a healthy
  string of small, well-scoped releases (dashboard dedup, reverse-proxy sub-path
  fixes, base-relative asset URLs), each with a matching `CHANGELOG.md` entry and
  regression coverage described in the entry itself.
- **Tests pass:** `bun test` → **101 pass, 0 fail, 535 expect() calls** across 17 files
  (run this pass; read-only, `node_modules`/bun already present, no install performed).
  Did not run `cargo test`/`cargo clippy` (no cargo per sweep rules) or the fuller
  `./tinytop check` chain (starts/serves the daemon — out of scope for a static-review
  pass).
- **Doc drift found:** the v0.2.4 commit (`ba453e3`, "single dashboard source — remove
  the legacy/dashboard duplicate") updated `README.md`'s prose but left a stale version
  stamp. Confirmed live: `README.md:9` still reads `- Version: \`0.2.0\`` while
  `VERSION` (and `CHANGELOG.md`'s top entry) say `0.2.4`; `package.json:3` also still
  reads `"version": "0.2.0"`. Neither is wrong information exactly (the legacy-dashboard
  removal itself is done and verified — `legacy/` now contains only `bun-collector.ts`,
  no stray `legacy/dashboard/`), just a stamp that didn't get bumped in step with
  `VERSION`.
- No stray running processes: `HANDOFF.md` references a daemon PID (`1827235`) from an
  earlier session; checked — not running. Nothing left dangling from that handoff.

## Defects (file:line)

1. **`README.md:9`** — `- Version: \`0.2.0\`` is stale; should read `0.2.4` (or better,
   drop the literal number and say "see `VERSION`", matching how `jobseek-ai`'s README
   handles it, to stop this drifting again). **Size: S.**
2. **`package.json:3`** — `"version": "0.2.0"` is stale relative to `VERSION` (`0.2.4`).
   Not consumed anywhere user-facing that was found this pass (it's a `private: true`
   Bun project, not published), but a mismatched version field is a footgun for anyone
   scripting against it later. **Size: S.**

## Ranked improvements

**S (small, safe, mechanical):**
- Bump `README.md:9` and `package.json:3` to `0.2.4` (or repoint the README line to
  "see `VERSION`" so it can't drift again — `jobseek-ai`'s README already uses that
  pattern: *"Current version: see `VERSION`"*). Cheapest fix in this batch.
- Add a lightweight CI/pre-commit check (or just a `tinytop check` step) that fails if
  `package.json`'s `version` doesn't match `VERSION` — prevents this exact drift from
  recurring across the next N releases.

**M (moderate):**
- `PROGRESS.md`'s "Recommended Next Work" already lists concrete, scoped items:
  optional normalized child tables for process/filesystem history (only if the UI
  starts querying those independently — no evidence yet that it does), wider rollup
  tiers for 30-day browsing, and a real Windows `.exe` release asset + Scoop/winget
  manifests. None re-verified against current code this pass; listed here as already
  triaged and ready to pick up.
- Live macOS/Windows CI or host verification — the native collector modules are
  feature-gated first slices per `README.md`'s "Known Limitations"; full parity work is
  tracked but not started.

**L (large):**
- None identified beyond what's already tracked in `PROGRESS.md`'s roadmap notes
  (wider rollup tiers, cross-platform collector parity) — this repo doesn't appear to
  be carrying large undocumented technical debt.

## What was skipped this pass

- `cargo` checks (fmt/clippy/test for the Rust agent) — out of scope per sweep rules
  (no cargo).
- The full `./tinytop check` chain and any live-daemon smoke test — these start
  services; out of scope for a ~15-minute static-review pass.
- Did not re-verify the Windows/macOS collector feature-gate code paths in detail.
