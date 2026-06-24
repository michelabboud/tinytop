# TinyTop

A standalone Bun-powered dashboard for live WSL/Linux workstation status. It runs locally, reads host telemetry from Linux/WSL sources, stores recent history in SQLite, and renders a dense browser dashboard with Apache ECharts.

## Current Status

- Version: `0.1.7`
- Runtime: Bun
- Public UI: `http://127.0.0.1:4274`
- Internal writer API: `http://127.0.0.1:4276`
- Default SQLite database: `~/.local/share/tinytop/history.sqlite`
- Network exposure: loopback only by default

## Quick Start

```bash
bun install
bun run dev
```

Open <http://127.0.0.1:4274>.

`bun run dev` starts the public dashboard process and an internal collector/writer process. The dashboard serves the browser UI on `127.0.0.1:4274`; the writer owns SQLite and exposes its local read API on `127.0.0.1:4276`.

For full setup and configuration, see [INSTALL.md](INSTALL.md). For day-to-day usage, see [GUIDE.md](GUIDE.md).

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

## Common Commands

```bash
bun run dev
bun run writer
bun test
bun run check
bun build public/app.js --target=browser --outdir=/tmp/tinytop-build-check
```

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
| [docs/adr/README.md](docs/adr/README.md) | Architecture decision records |

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
bun run check
bun build public/app.js --target=browser --outdir=/tmp/tinytop-build-check
git diff --check
```

## Safety

The dashboard is read-only with respect to the operating system. It reads `/proc`, `df`, `ps`, `uname`, and OS release files, but it does not restart services, kill processes, modify WSL configuration, or change system state. SQLite writes are limited to the configured dashboard history database.
