# 0006 - Embed Dashboard Assets In The Rust Collector

## Status

Accepted

## Context

TinyTop's default runtime is now the Rust collector/dashboard daemon. Before this
decision, `tinytop-agent serve` still depended on a dashboard asset directory on
disk unless callers passed an explicit `--public-dir`. That made the Rust daemon
less self-contained than the project goal: one local binary should be enough to
collect telemetry, own SQLite, serve APIs, and serve the dashboard.

The dashboard itself is still plain HTML, CSS, and browser JavaScript with
Apache ECharts. A future dashboard redesign may move to Svelte or another
frontend compiler, but this phase is about runtime ownership, not changing UI
behavior.

## Decision

Move the legacy Bun dashboard assets to `legacy/dashboard/` and add a
byte-identical Rust dashboard asset tree under `agent/assets/dashboard/`.

Embed the Rust asset tree into `tinytop-agent serve` with compile-time
`include_bytes!` assets. The Rust daemon serves embedded assets by default.
Keep `--public-dir` and `TINYTOP_PUBLIC_DIR` as explicit development overrides
for testing custom asset directories.

Keep the legacy Bun server pointed at `legacy/dashboard/`.

## Alternatives Rejected

### Keep Using A Runtime `public/` Directory

Rejected because it keeps normal Rust operation dependent on a separate asset
directory and weakens the one-daemon install story.

### Embed Assets Directly From `legacy/dashboard/`

Rejected because it would make the Rust product path depend on the legacy tree.
The duplicate `agent/assets/dashboard/` tree makes Rust ownership explicit, and
tests assert it stays byte-identical to the legacy dashboard until a deliberate
UI migration occurs.

### Migrate To Svelte Or SvelteKit Now

Rejected for this phase because it would combine runtime ownership work with a
frontend architecture migration. Svelte remains a good future candidate if the
dashboard needs component boundaries and richer state structure. SvelteKit is
not needed while Rust owns routing, APIs, SQLite, and serving.

## Consequences

- `tinytop-agent serve` can serve the dashboard without Bun and without a
  dashboard directory on disk.
- The Rust binary grows by the size of the embedded dashboard assets, including
  the ECharts bundle.
- Dashboard asset changes must update both `legacy/dashboard/` and
  `agent/assets/dashboard/`; `tests/dashboard-assets.test.ts` enforces byte
  equality.
- `--public-dir` remains useful for local development and contract tests.
