# Documentation Sweep

Date: 2026-06-25
Version: 0.1.18

## Scope

This pass refreshed TinyTop's current-facing docs and guides after the Rust
collector/dashboard daemon started embedding dashboard assets by default.

The current dashboard asset layout is:

- `agent/assets/dashboard/` - embedded by `tinytop-agent serve`
- `legacy/dashboard/` - served by the legacy Bun dashboard

The root `public/` dashboard tree was removed in `0.1.17`.

## Updated Areas

- Root docs: `README.md`, `INSTALL.md`, `CHANGELOG.md`, `PROGRESS.md`, and
  `HANDOFF.md`
- Guides: `docs/guides/API.md` and `docs/guides/OPERATIONS.md`
- Reports: dependency and Web UI verification reports that previously pointed
  at the old root `public/` tree
- ADR index: `docs/adr/README.md`

Historical ADR files were not rewritten. ADR 0001 now appears as superseded in
the ADR index, while ADR 0005 and ADR 0006 describe the current Rust
collector/dashboard ownership model.

## Verification

Verification evidence is recorded in the root `HANDOFF.md` and the closeout for
the `0.1.18` docs checkpoint.
