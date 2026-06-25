# Architecture

TinyTop's default persistent runtime is a single local Rust daemon. It serves the browser dashboard, collects Linux/WSL telemetry, owns SQLite, and exposes dashboard/history APIs over loopback.

The original Bun dashboard and writer remain in the repo for TypeScript development and fallback.

## Runtime Topology

```text
Browser
  |
  | GET /, /app.js, /styles.css, /vendor/echarts.min.js
  | GET /api/snapshot
  | GET /api/history
  v
Rust daemon: tinytop-agent serve
  127.0.0.1:4274
  |
  | reads Linux/WSL metrics through procfs and sysinfo
  | writes and reads SQLite
  v
SQLite: ~/.local/share/tinytop/history.sqlite
```

`./tinytop systemd install` defaults to this Rust service. The daemon also keeps writer-compatible routes (`/snapshot/latest`, `/snapshot/collect`, and `/history`) on the same port for API continuity.

For development, `bun run dev` starts `src/server.ts`, and that process spawns `src/collector-daemon.ts`. For split supervision, start the writer separately with `bun run writer`, then start the dashboard with `TINYTOP_DISABLE_WRITER_SPAWN=1`.

The supported operator entrypoint is the root `./tinytop` Bash command center.
It works before Bun is installed for help and bootstrap, then hands off to
`bun run setup` for the richer setup wizard.

## Data Flow

1. The browser loads `public/index.html`, `public/styles.css`, `/vendor/echarts.min.js`, and `public/app.js`.
2. `public/app.js` reads browser-local theme and graph-mode settings from `localStorage`.
3. The frontend requests `/api/history?limit=120&window_seconds=180` to hydrate Live History from SQLite.
4. The frontend polls `/api/snapshot` every 1500 ms.
5. `tinytop-agent serve` returns the latest stored sample or collects a fresh one.
6. The Rust daemon collects telemetry on a timer and stores samples through `tinytop-store`.
7. `tinytop-store` writes one row per sample into SQLite through SQLx.
8. The frontend deduplicates samples by timestamp, updates gauges, and redraws ECharts views.

## Modules

| Path | Responsibility |
| --- | --- |
| `src/parsers.ts` | Pure parsing and normalization for `/proc`, pressure, load, filesystems, and runtime detection |
| `src/collector.ts` | Live host reads from Linux/WSL sources and `SystemSnapshot` construction |
| `src/history-store.ts` | SQLite setup, pragmas, indexes, prepared inserts, latest reads, range reads |
| `src/collector-daemon.ts` | Legacy Bun writer HTTP API, scheduled collection loop, SQLite ownership |
| `src/server.ts` | Legacy Bun public HTTP server, static assets, ECharts route, writer proxy |
| `src/ops.ts` | SQLite maintenance helpers for stats, integrity checks, and vacuum |
| `src/wizard/index.ts` | Bun setup wizard launched by `./tinytop setup` |
| `tinytop` | Bash command center for setup, Bun bootstrap, systemd services, logs, status, and DB operations |
| `public/index.html` | App shell and semantic dashboard structure |
| `public/styles.css` | Themes, layout, responsive behavior, dashboard styling |
| `public/app.js` | Browser state, polling, history hydration, ECharts rendering, interactions |
| `tests/` | Bun tests for parsers, snapshot building, server routes, and history storage |
| `agent/crates/tinytop-types` | Rust snapshot structs serialized to the existing dashboard JSON contract |
| `agent/crates/tinytop-collectors` | Rust platform collector crate; currently Linux/WSL only |
| `agent/crates/tinytop-store` | SQLx-backed Rust history store using the current SQLite schema |
| `agent/crates/tinytop-agent` | Rust CLI and daemon for collection, SQLite history, dashboard serving, and writer-compatible APIs |

## Rust Daemon

The Rust workspace is intentionally additive. The existing Bun collector remains intact in `src/collector.ts`, but systemd defaults to the Rust daemon.

Current Rust commands:

```bash
cargo test --manifest-path agent/Cargo.toml --workspace
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- collect --json
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- serve --public-dir public
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- serve-writer
```

The Rust Linux/WSL collector keeps the same `SystemSnapshot` contract as the Bun collector while using Rust crates for host access. It uses `procfs` for Linux kernel metrics such as CPU ticks, memory, load, uptime, and pressure stall information, and `sysinfo` for disk, process, hostname, OS, and kernel metadata. It does not shell out to `df`, `ps`, or `uname`. The live collector keeps a reusable `sysinfo::System` across samples so process and CPU refreshes have previous state and avoid rebuilding all collector state on every interval. The Rust store uses SQLx with SQLite today, with SQL isolated in `tinytop-store` so future PostgreSQL/MySQL support does not leak into collector code.

## Public Dashboard API

The Rust daemon and legacy public dashboard expose:

- `GET /health`
- `GET /api/snapshot`
- `GET /api/history?limit=&window_seconds=&since_ms=&until_ms=`
- `GET /vendor/echarts.min.js`
- static frontend assets: `/`, `/index.html`, `/styles.css`, `/app.js`

See [docs/guides/API.md](docs/guides/API.md) for request and response details.

## Writer-Compatible API

The Rust daemon exposes these routes on `127.0.0.1:4274`. The legacy split writer exposes the same routes on `127.0.0.1:4276`:

- `GET /health`
- `GET /snapshot/latest`
- `GET /snapshot/collect`
- `GET /history?limit=&window_seconds=&since_ms=&until_ms=`

The writer-compatible API is internal. It binds to loopback by default and should not be exposed publicly.

## SQLite

Default database path:

```text
~/.local/share/tinytop/history.sqlite
```

Override:

```bash
TINYTOP_HISTORY_DB=/path/to/history.sqlite ./tinytop rust serve
```

SQLite pragmas:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;
```

Current table:

```sql
CREATE TABLE IF NOT EXISTS metric_samples (
  sample_id INTEGER PRIMARY KEY,
  captured_at_ms INTEGER NOT NULL UNIQUE,
  snapshot_timestamp TEXT NOT NULL,
  hostname TEXT NOT NULL,
  runtime_kind TEXT NOT NULL,
  cpu_usage_percent REAL NOT NULL,
  cpu_cores INTEGER NOT NULL,
  memory_used_percent REAL NOT NULL,
  memory_used_bytes INTEGER NOT NULL,
  memory_total_bytes INTEGER NOT NULL,
  swap_used_percent REAL NOT NULL,
  swap_used_bytes INTEGER NOT NULL,
  swap_total_bytes INTEGER NOT NULL,
  load_one REAL NOT NULL,
  load_five REAL NOT NULL,
  load_fifteen REAL NOT NULL,
  load_percent REAL NOT NULL,
  runnable_threads INTEGER NOT NULL,
  total_threads INTEGER NOT NULL,
  root_used_percent REAL,
  snapshot_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_metric_samples_captured_at
  ON metric_samples (captured_at_ms DESC);

CREATE INDEX IF NOT EXISTS idx_metric_samples_runtime_captured_at
  ON metric_samples (runtime_kind, captured_at_ms DESC);
```

The current implementation stores indexed graph/query columns plus full `SystemSnapshot` JSON. This supports refresh-safe chart hydration and selected-sample detail rendering. Normalized filesystem/process/pressure child tables and rollups are planned for future longer-range analytics.

## Frontend State

Browser-local settings:

- `tinytop.theme`
- `tinytop.graphMode`

In-memory session state:

- hydrated snapshots
- selected timeline index
- ECharts instance
- pause/loading flags
- active confirmation dialog resolver and return-focus target

The browser keeps a rolling 120-sample window. Bar mode calculates the number of visible bars from the chart width so bars never shrink below the configured minimum width. When the visible capacity is reached, the window rolls left: new samples appear on the right and older visible samples disappear on the left.

Web UI interaction policy:

- Public browser code must not call native `alert`, `confirm`, or `prompt`.
- Inline errors render through the `status-message` surface.
- Browser-local destructive actions use the reusable `<dialog>` confirmation flow in `public/app.js`.
- Confirmed actions must describe their scope before continuing; for example, clearing Live History affects only the current tab's session buffer and does not delete SQLite history.

## Runtime Detection

Runtime detection is explicit and conservative:

1. Check kernel release/version text for Microsoft/WSL markers.
2. Check `WSL_DISTRO_NAME` and `WSL_INTEROP`.
3. If no WSL markers exist and Linux metadata is present, classify as real Linux.
4. Otherwise classify as unknown.

## Safety Boundaries

The app is read-only with respect to the operating system:

- The Rust daemon reads Linux/WSL metrics through crates such as `procfs` and `sysinfo`.
- The legacy Bun collector reads `/proc`, OS release files, `df`, `ps`, and `uname`.
- It writes only to the configured SQLite history database.
- It does not restart services, kill processes, change sysctl values, edit WSL config, or modify host state.
- It binds to loopback by default.

Systemd integration uses user services under `~/.config/systemd/user/`.
The default unit is `tinytop.service`, running `tinytop-agent serve`. The legacy
Bun split path remains available through `tinytop-writer.service` and
`tinytop-dashboard.service` when explicitly installed with `--bun`.

## Decisions

Architecture decision records live in [docs/adr/README.md](docs/adr/README.md).

- [0001 - SQLite Writer Process](docs/adr/0001-sqlite-writer-process.md)
- [0002 - Initial Snapshot JSON History](docs/adr/0002-initial-snapshot-json-history.md)
- [0003 - Bash Bootstrap Plus Bun Install Wizard](docs/adr/0003-bash-bootstrap-bun-install-wizard.md)
- [0004 - Additive Rust Agent With SQLx Store](docs/adr/0004-rust-agent-sqlx-store.md)
- [0005 - Rust Single-Daemon Systemd Runtime](docs/adr/0005-rust-single-daemon-systemd-runtime.md)
