# TinyTop

![TinyTop dashboard hero](docs/assets/tinytop-hero.png)

A standalone local dashboard for live WSL/Linux workstation status. The default persistent runtime is a single Rust daemon that serves the dashboard, collects host telemetry, stores recent history in SQLite, and renders a dense browser UI with Apache ECharts.

## Current Status

- Version: `0.1.14`
- Runtime: Rust daemon for persistent installs; Bun remains available for development and fallback
- Public UI: `http://127.0.0.1:4274`
- Legacy writer API: `http://127.0.0.1:4276`
- Default SQLite database: `~/.local/share/tinytop/history.sqlite`
- Network exposure: loopback only by default

## Install And Run

```bash
git clone <repo-url> tinytop
cd tinytop
./tinytop rust install-binary
./tinytop systemd install --rust
./tinytop systemd start
```

Open <http://127.0.0.1:4274>.

For persistent installs without Bun, use the Rust agent:

```bash
./tinytop rust install-binary
./tinytop systemd install --rust
./tinytop systemd start
```

If a release binary is not available for your platform, compile locally:

```bash
./tinytop install-rust --print-only
./tinytop rust build
./tinytop systemd install --rust
```

`./tinytop setup` is the Telecode-style Bun wizard for source/development installs. It asks whether systemd should use a GitHub release binary or a local Cargo compile.

For full setup and configuration, see [INSTALL.md](INSTALL.md). For day-to-day usage, see [GUIDE.md](GUIDE.md).

## New User Guide

1. Clone the repo and enter it:

   ```bash
   git clone <repo-url> tinytop
   cd tinytop
   ```

2. Inspect the command center:

   ```bash
   ./tinytop help
   ./tinytop doctor
   ```

3. Install the Rust agent. Prefer a release binary:

   ```bash
   ./tinytop rust install-binary
   ```

   Or compile locally:

   ```bash
   ./tinytop install-rust --print-only
   ./tinytop rust build
   ```

4. Install persistent user-space systemd service:

   ```bash
   ./tinytop systemd install --rust
   ./tinytop systemd start
   ```

5. Open the dashboard:

   ```text
   http://127.0.0.1:4274
   ```

6. Install Bun only if you want the Bun setup wizard or TypeScript development:

   ```bash
   ./tinytop install-bun --print-only
   ./tinytop install-bun --yes
   ```

7. Optional source setup wizard:

   ```bash
   ./tinytop setup
   ```

8. Optional foreground Bun development runtime:

   ```bash
   ./tinytop start
   ```

9. Useful maintenance commands:

   ```bash
   ./tinytop status
   ./tinytop logs
   ./tinytop db stats
   ./tinytop db backup
   ./tinytop db check
   ```

## Command Center

The root `./tinytop` command is the supported operator entrypoint:

```bash
./tinytop help
./tinytop doctor
./tinytop rust install-binary
./tinytop rust build
./tinytop install-bun --print-only
./tinytop setup
./tinytop systemd install --rust
./tinytop db stats
./tinytop db backup
```

For persistent background collection, install user-space systemd services:

```bash
./tinytop systemd install --rust
./tinytop systemd start
```

## What It Shows

- CPU utilization, CPU core count, and load averages
- RAM and swap usage
- Kernel, distro, uptime, and automatic WSL versus real Linux detection
- Filesystem capacity and inode pressure
- CPU, memory, and I/O pressure from `/proc/pressure/*` when available
- Top processes by CPU and memory
- Live gauges, sparklines, status strips, and stat tiles
- Apache ECharts Live History views: line, stacked area, stacked bar, heatmap, and treemap
- Responsive Bar mode that keeps a minimum bar width and rolls the visible window left as new samples arrive
- SQLite-backed recent history so browser refreshes refill Live History instead of starting empty
- Timeline scrubber with selected datetime context, compact metric values, and a return-to-live control
- In-app confirmation dialogs for browser-local destructive actions, including clearing the session history buffer
- Browser-local display preferences for theme and graph mode
- Rust Linux/WSL daemon under `agent/` with shared snapshot types, crate-backed collection, SQLx SQLite history, and a no-Bun systemd path

## Common Commands

```bash
./tinytop setup
./tinytop rust install-binary
./tinytop rust build
./tinytop rust serve
./tinytop systemd render
./tinytop start
./tinytop start:split
./tinytop db stats
bun run dev
bun run writer
bun test
bun run check
bun run rust:test
bun run rust:serve
bun build public/app.js --target=browser --outdir=/tmp/tinytop-build-check
```

## Rust Daemon

The Rust workspace lives under `agent/` and provides the default persistent runtime:

```bash
cargo test --manifest-path agent/Cargo.toml --workspace
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- collect --json
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- serve --public-dir public
```

The Rust daemon serves the dashboard and APIs on `127.0.0.1:4274`. The older Bun dashboard/writer split is still available with `./tinytop start`, `./tinytop start:split`, and `./tinytop systemd install --bun`.

Implementation notes:

- The Rust Linux collector uses `procfs` and `sysinfo`; it does not shell out to `df`, `ps`, or `uname`.
- The live collector keeps a reusable `sysinfo::System` so repeated samples avoid rebuilding all collector state from scratch.
- Local Rust builds require Rust `1.95.0` or newer because the pinned `sysinfo` release uses that MSRV.

## Documentation Map

| File | Purpose |
| --- | --- |
| [INSTALL.md](INSTALL.md) | Prerequisites, setup, environment variables, running, upgrade, uninstall |
| [GUIDE.md](GUIDE.md) | User guide for the dashboard UI, graph modes, timeline, refresh behavior |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Process model, data flow, modules, SQLite schema, safety boundaries |
| [CHANGELOG.md](CHANGELOG.md) | Versioned release notes |
| [PROGRESS.md](PROGRESS.md) | Completed milestones and next work |
| [docs/guides/API.md](docs/guides/API.md) | Public dashboard API and internal writer API |
| [docs/guides/OPERATIONS.md](docs/guides/OPERATIONS.md) | Runtime checks, SQLite inspection, backup/reset, troubleshooting |
| [docs/sqlite-history-architecture.md](docs/sqlite-history-architecture.md) | Persistence design and current SQLite implementation |
| [docs/reports/2026-06-24-rust-agent-dependency-vetting.md](docs/reports/2026-06-24-rust-agent-dependency-vetting.md) | Rust agent dependency and SQLx vetting |
| [docs/reports/2026-06-25-rust-daemon-dependency-vetting.md](docs/reports/2026-06-25-rust-daemon-dependency-vetting.md) | Rust daemon and vendored dashboard asset dependency vetting |
| [docs/reports/2026-06-25-webui-confirmation-dialog-verification.md](docs/reports/2026-06-25-webui-confirmation-dialog-verification.md) | Web UI confirmation-dialog policy and rendered verification |
| [docs/superpowers/specs/2026-06-24-tinytop-install-wizard-design.md](docs/superpowers/specs/2026-06-24-tinytop-install-wizard-design.md) | Install wizard and systemd command-center design record |
| [docs/adr/README.md](docs/adr/README.md) | Architecture decision records |

## License

TinyTop is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

## Configuration Summary

| Variable | Default | Meaning |
| --- | --- | --- |
| `HOST` | `127.0.0.1` | Public dashboard bind host |
| `PORT` | `4274` | Public dashboard port |
| `HISTORY_WRITER_HOST` | `127.0.0.1` | Legacy writer bind host |
| `HISTORY_WRITER_PORT` | `4276` | Legacy writer port |
| `HISTORY_WRITER_URL` | unset | Existing writer URL; when set, dashboard does not spawn a writer |
| `HISTORY_POLL_MS` | `1500` | Writer collection interval |
| `TINYTOP_HISTORY_DB` | `~/.local/share/tinytop/history.sqlite` | SQLite database path |
| `TINYTOP_DISABLE_WRITER_SPAWN` | unset | Set to `1` when starting the writer separately |
| `TINYTOP_PUBLIC_DIR` | `./public` | Static dashboard asset directory for the Rust daemon |

## Ports

The project claims these loopback ports in `~/.config/fleet/ports/tinytop.toml`:

- `127.0.0.1:4274` - public dashboard UI
- `127.0.0.1:4276` - legacy/internal collector-writer API for split mode

## Persistence

Recent history is stored in SQLite by the Rust daemon in the default runtime. In legacy Bun split mode, the writer process owns SQLite and the dashboard process reads through the writer API. The browser hydrates up to 120 recent samples on startup, then continues polling live samples.

The current SQLite implementation stores indexed metric columns plus the complete snapshot JSON. Retention and rollup tables are planned but not implemented yet, so the database grows until manually archived or reset.

## Verification

```bash
./tinytop check
./tinytop help
./tinytop doctor
git diff --check
```

## Safety

The dashboard is read-only with respect to the operating system. The Rust collector uses `procfs` and `sysinfo` instead of shelling out to `df`, `ps`, or `uname`. SQLite writes are limited to the configured dashboard history database.
