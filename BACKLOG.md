# BACKLOG.md — deferred or spotted, not done

One dated line each: date · source · item · status. A line leaves only by being done or by Michel's word.

- 2026-10-09 · 0.12.1 lane report · The legacy Bun collector (`src/collector.ts`) keeps a fixed 10 processes and ignores `topProcessCount` · open
- 2026-10-09 · 0.12.1 lane report · The Bun server replaces a non-numeric settings value with the default instead of refusing it, unlike the Rust daemon · open
- 2026-10-09 · 0.12.1 lane report · The "Processes" label and help text in Settings → General → Daemon are terse enough that the setting was not found · open
- 2026-10-09 · mechanical review of 0.13.0 (finding 3) · pid reuse between the process-table refresh and the `/proc/<pid>/status` read can attach one tick's swap figure to the wrong process; a guard needs the start time compared across both reads · open, accepted limit
- 2026-10-09 · mechanical review of 0.13.0 (finding 6) · The Bun collector has no per-process swap and no CPU/memory ranks, so the two runtimes are no longer behaviourally identical for the process list · open
