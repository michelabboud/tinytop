# 0005 - Rust Single-Daemon Systemd Runtime

## Status

Accepted

## Context

TinyTop started as a Bun dashboard process plus a Bun writer process. That split
proved the data model and kept SQLite ownership clean, but it still required Bun
for normal persistent operation.

The Rust agent now has enough of the production path to collect Linux/WSL
metrics, own SQLite through SQLx, expose the existing writer-compatible API, and
serve the static dashboard assets. A new user should be able to run TinyTop from
a prebuilt Rust binary or a local Cargo build without needing Bun at runtime.

## Decision

Use `tinytop-agent serve` as the default persistent runtime for systemd. It runs
one loopback daemon on `127.0.0.1:4274` that:

- serves `/`, `/index.html`, `/styles.css`, `/app.js`, and
  `/vendor/echarts.min.js`
- exposes public dashboard APIs under `/api/snapshot` and `/api/history`
- exposes writer-compatible APIs under `/snapshot/latest`, `/snapshot/collect`,
  and `/history`
- collects on an interval and stores history in SQLite

The root `./tinytop` script defaults `systemd render` and `systemd install` to
this Rust service. `./tinytop systemd render --bun` and
`./tinytop systemd install --bun` keep the older Bun split-service path
available.

The setup wizard asks whether the Rust agent should come from a GitHub release
binary or a local Cargo compile before installing systemd services.

## Alternatives Rejected

### Keep Bun As The Persistent Dashboard Service

Rejected because it would leave Bun as a runtime dependency even after the Rust
collector and store were available. That does not meet the no-Bun install goal.

### Use Rust Writer Plus Bun Dashboard As The Default

Rejected as the default because it still requires Bun in systemd mode. It remains
a useful development shape through the existing Bun commands.

### Replace The Bun Source Implementation Entirely

Rejected because the existing Bun dashboard and tests are still useful for
frontend development and fallback. Removing them would add risk without helping
the Rust daemon path.

### Download Browser Assets At Runtime

Rejected because normal startup should not depend on internet access. TinyTop now
redistributes the Apache ECharts browser bundle under `public/vendor/` with
upstream license and notice files.

## Consequences

- Normal systemd installs can run without Bun once a Rust agent binary exists.
- The release process should attach a platform-specific `tinytop-agent` binary
  so `./tinytop rust install-binary` can bootstrap users who do not want to
  compile locally.
- Bun remains required for TypeScript development, the Bun setup wizard, and the
  legacy split runtime.
- `127.0.0.1:4276` is still reserved for the legacy writer API and
  `serve-writer`, but the default Rust daemon serves browser traffic and API
  traffic on `127.0.0.1:4274`.
