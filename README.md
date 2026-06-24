# TinyTop

![TinyTop dashboard hero](docs/assets/tinytop-hero.png)

A standalone Bun-powered dashboard for live WSL/Linux workstation status. It runs locally, reads host telemetry from Linux/WSL sources, stores recent history in SQLite, and renders a dense browser dashboard with Apache ECharts.

## Current Status

- Version: `0.1.12`
- Runtime: Bun
- Public UI: `http://127.0.0.1:4274`
- Internal writer API: `http://127.0.0.1:4276`
- Default SQLite database: `~/.local/share/tinytop/history.sqlite`
- Network exposure: loopback only by default

## Install And Run

```bash
git clone <repo-url> tinytop
cd tinytop
./tinytop setup
./tinytop start
```

Open <http://127.0.0.1:4274>.

`./tinytop setup` is the Telecode-style installer. The Bash command center works before Bun is installed, can print or run the official Bun installer, installs dependencies when needed, and then launches the Bun setup wizard with `bun run setup`.

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

3. Install Bun if the doctor says it is missing:

   ```bash
   ./tinytop install-bun --print-only
   ./tinytop install-bun --yes
   ```

4. Run the setup wizard:

   ```bash
   ./tinytop setup
   ```

5. Start TinyTop in the foreground:

   ```bash
   ./tinytop start
   ```

6. Open the dashboard:

   ```text
   http://127.0.0.1:4274
   ```

7. For persistent background services:

   ```bash
   ./tinytop systemd install
   ./tinytop systemd start
   ```

8. Useful maintenance commands:

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
./tinytop install-bun --print-only
./tinytop setup
./tinytop systemd install
./tinytop db stats
./tinytop db backup
```

For persistent background collection, install user-space systemd services:

```bash
./tinytop systemd install
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
- Browser-local display preferences for theme and graph mode
- Experimental Rust Linux/WSL collector workspace under `agent/`, with shared snapshot types and a SQLx-backed SQLite store path

## Common Commands

```bash
./tinytop setup
./tinytop start
./tinytop start:split
./tinytop systemd render
./tinytop db stats
bun run dev
bun run writer
bun test
bun run check
bun run rust:test
bun run rust:collect
bun build public/app.js --target=browser --outdir=/tmp/tinytop-build-check
```

## Rust Collector Preview

The existing Bun collector remains the default production collector. The additive Rust workspace lives under `agent/` and currently provides a Linux/WSL collector plus a SQLx SQLite history store proof point:

```bash
cargo test --manifest-path agent/Cargo.toml --workspace
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- collect --json
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- collect --json --sqlite sqlite::memory:
```

The Rust agent is not wired into `./tinytop start` yet.

Implementation notes:

- The Rust Linux collector uses `procfs` and `sysinfo`; it does not shell out to `df`, `ps`, or `uname`.
- The live collector keeps a reusable `sysinfo::System` so repeated samples avoid rebuilding all collector state from scratch.
- The Rust preview requires Rust `1.95.0` or newer because the pinned `sysinfo` release uses that MSRV.

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
| [docs/superpowers/specs/2026-06-24-tinytop-install-wizard-design.md](docs/superpowers/specs/2026-06-24-tinytop-install-wizard-design.md) | Install wizard and systemd command-center design record |
| [docs/adr/README.md](docs/adr/README.md) | Architecture decision records |

## License

TinyTop is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

## Configuration Summary

| Variable | Default | Meaning |
| --- | --- | --- |
| `HOST` | `127.0.0.1` | Public dashboard bind host |
| `PORT` | `4274` | Public dashboard port |
| `HISTORY_WRITER_HOST` | `127.0.0.1` | Internal writer bind host |
| `HISTORY_WRITER_PORT` | `4276` | Internal writer port |
| `HISTORY_WRITER_URL` | unset | Existing writer URL; when set, dashboard does not spawn a writer |
| `HISTORY_POLL_MS` | `1500` | Writer collection interval |
| `TINYTOP_HISTORY_DB` | `~/.local/share/tinytop/history.sqlite` | SQLite database path |
| `TINYTOP_DISABLE_WRITER_SPAWN` | unset | Set to `1` when starting the writer separately |

## Ports

The project claims these loopback ports in `~/.config/fleet/ports/tinytop.toml`:

- `127.0.0.1:4274` - public dashboard UI
- `127.0.0.1:4276` - internal collector/writer API

## Persistence

Recent history is stored in SQLite by the writer process. The dashboard process never opens SQLite directly; it reads `/api/snapshot` and `/api/history` through the writer process. The browser hydrates up to 120 recent samples on startup, then continues polling live samples.

The current SQLite implementation stores indexed metric columns plus the complete snapshot JSON. Retention and rollup tables are planned but not implemented yet, so the database grows until manually archived or reset.

## Verification

```bash
./tinytop check
./tinytop help
./tinytop doctor
git diff --check
```

## Safety

The dashboard is read-only with respect to the operating system. It reads `/proc`, `df`, `ps`, `uname`, and OS release files, but it does not restart services, kill processes, modify WSL configuration, or change system state. SQLite writes are limited to the configured dashboard history database.
