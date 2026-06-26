# Runtime-Specific Setup Verification

Date: 2026-06-26

## Purpose

Make setup verification match the collector runtime selected by the user.

## Behavior

- Rust release-binary setup runs `./tinytop rust collect` after installing the release asset.
- Rust local-compile setup runs `bun run check:rust`.
- Legacy Bun setup runs `bun run check:bun`.
- `bun run check` remains the full maintainer suite and runs both runtime checks.
- `./tinytop check` remains the full command-center verification path and also builds the browser dashboard bundle.

## Rationale

Users installing the Rust collector/dashboard daemon do not need Bun dashboard tests during setup. Users choosing the legacy Bun collector path do not need Rust workspace tests during setup. Full cross-runtime verification remains available for maintainers and releases.

## Verification

- `bun test tests/wizard.test.ts`
  - `9 pass`, `0 fail`
  - Confirms Rust release, Rust compile, and legacy Bun setup paths select the expected verification commands.
- `./tinytop check`
  - `bun run check:bun`: `46 pass`, `0 fail`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - `bun run check:rust`: Rust fmt check and workspace tests passed
  - Browser bundle built `legacy/dashboard/app.js`
- `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`: clean
- `git diff --check`: clean
