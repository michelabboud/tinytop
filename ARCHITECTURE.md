# Architecture

TinyTop's default persistent runtime is a single local Rust daemon. It serves the browser dashboard, collects Linux/WSL telemetry by default, owns SQLite, and exposes dashboard/history APIs over loopback.

The original Bun dashboard and legacy collector remain in the repo for TypeScript development and fallback.

## Runtime Topology

```text
Browser
  |
  | GET /, /app.js, /styles.css, /vendor/echarts.min.js
  | GET /api/snapshot
  | GET /api/history
  | GET /api/history/coverage
  | GET /api/history/points
  | GET /api/history/markers
  | GET /api/version
  | GET/PUT /api/settings
  v
Rust daemon: tinytop-agent serve
  127.0.0.1:4274
  |
  | reads Linux/WSL metrics through procfs and sysinfo
  | optional feature-gated macOS/Windows collectors use sysinfo
  | writes and reads SQLite
  v
SQLite: ~/.local/share/tinytop/history.sqlite
```

`./tinytop systemd install` defaults to this Rust collector/dashboard service. The daemon also keeps the legacy collector-compatible routes (`/snapshot/latest`, `/snapshot/collect`, `/history`, and `/version`) on the same port for API continuity.

For development, `bun run dev` starts `src/server.ts`, and that process spawns `legacy/bun-collector.ts`. For split supervision, start the legacy Bun collector separately with `bun run collector`, then start the dashboard with `TINYTOP_DISABLE_WRITER_SPAWN=1`.

The supported operator entrypoint is the root `./tinytop` Bash command center. It works before Bun is installed for help and bootstrap, auto-selects the Rust collector/dashboard daemon for `./tinytop start` when a release binary or Cargo is available, and supports `TINYTOP_RUNTIME=legacy` for the Bun fallback.

## Data Flow

1. The browser loads embedded Rust dashboard assets: `index.html`, `styles.css`, `/vendor/echarts.min.js`, and `app.js`.
2. `app.js` requests `/api/settings` for SQLite-backed daemon defaults.
3. `app.js` reads browser-local theme, graph-mode, history-range, visible-series, process-table, filesystem-toggle, and last-section overrides from `localStorage`.
4. The frontend requests `/api/history` with explicit `since_ms` and `until_ms` bounds for raw Live, 15m, and 1h ranges.
5. The frontend requests `/api/history/points` for 6h, 24h, 7d, and 30d chart ranges backed by one-minute rollups.
6. The frontend requests `/api/history/markers` for daemon starts, settings changes, and computed coverage gaps.
7. The frontend requests `/api/history/coverage` when the Rust daemon is serving the page.
8. The frontend requests `/api/version` once to display the serving runtime and product version.
9. The frontend polls `/api/snapshot` on the configured browser refresh interval.
10. `tinytop-agent serve` returns the latest stored sample or collects a fresh one.
11. The Rust daemon collects telemetry on a timer and stores samples through `tinytop-store`.
12. `tinytop-store` writes samples, one-minute rollups, daemon timeline events, and daemon defaults into SQLite through SQLx.
13. The frontend pages raw ranges, reads rollup points for long ranges, deduplicates samples by timestamp, down-samples only for browser rendering, updates CPU/RAM/swap/load gauges, computes threshold states, and redraws ECharts views.

## Modules

| Path | Responsibility |
| --- | --- |
| `src/parsers.ts` | Pure parsing and normalization for `/proc`, pressure, load, filesystems, and runtime detection |
| `src/collector.ts` | Live host reads from Linux/WSL sources and `SystemSnapshot` construction |
| `src/history-store.ts` | SQLite setup, pragmas, indexes, prepared inserts, latest reads, range reads |
| `src/settings.ts` | Legacy Bun settings shape and validation for the fallback dashboard server |
| `src/version.ts` | Shared legacy Bun runtime/version metadata used by the dashboard and collector APIs |
| `legacy/bun-collector.ts` | Legacy Bun collector HTTP API, scheduled collection loop, SQLite ownership |
| `src/server.ts` | Legacy Bun HTTP server, static assets, ECharts route, collector proxy |
| `src/ops.ts` | SQLite maintenance helpers for stats, integrity checks, and vacuum |
| `src/wizard/index.ts` | Bun setup wizard launched by `./tinytop setup`, including runtime-specific Rust versus Bun verification |
| `tinytop` | Bash command center for setup, Bun bootstrap, systemd services, logs, status, and DB operations |
| `legacy/dashboard/` | Legacy Bun dashboard asset tree |
| `agent/assets/dashboard/` | Rust-embedded dashboard asset tree; kept byte-identical to `legacy/dashboard/` |
| `tests/` | Bun tests for parsers, snapshot building, server routes, and history storage |
| `agent/crates/tinytop-types` | Rust snapshot structs serialized to the existing dashboard JSON contract |
| `agent/crates/tinytop-collectors` | Rust platform collector crate; Linux/WSL default plus feature-gated macOS/Windows native collector modules |
| `agent/crates/tinytop-store` | SQLx-backed Rust history store using the current SQLite schema |
| `agent/crates/tinytop-agent` | Rust CLI and daemon for collection, SQLite history, dashboard serving, and legacy collector-compatible APIs |

## Rust Daemon

The Rust workspace is intentionally additive. The existing Bun metric collector remains intact in `src/collector.ts`, while the legacy Bun collector daemon lives under `legacy/`. Systemd defaults to the Rust collector/dashboard daemon.

Current Rust commands:

```bash
cargo test --manifest-path agent/Cargo.toml --workspace
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- collect --json
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- serve
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- serve-writer
```

The Rust Linux/WSL collector keeps the same `SystemSnapshot` contract as the Bun collector while using Rust crates for host access. It uses `procfs` for Linux kernel metrics such as CPU ticks, memory, load, uptime, and pressure stall information, and `sysinfo` for disk, process, hostname, OS, and kernel metadata. It does not shell out to `df`, `ps`, or `uname`. The live collector keeps a reusable `sysinfo::System` across samples so process and CPU refreshes have previous state and avoid rebuilding all collector state on every interval. The Rust store uses SQLx with SQLite today, with SQL isolated in `tinytop-store` so future PostgreSQL/MySQL support does not leak into collector code.

The collector crate exposes `NativeCollector` behind target and Cargo feature gates:

- default Linux builds use `linux-collector`
- macOS builds can use `--no-default-features --features macos-collector`
- Windows builds can use `--no-default-features --features windows-collector`

The macOS and Windows modules currently provide the first native slice through `sysinfo`: identity, CPU, memory/swap, load equivalent, disks, and top processes including parent PID/start time when available. Linux remains the reference implementation because pressure, exact `/proc` load thread counts, and live-host parity have not yet been validated on macOS/Windows.

## Public Dashboard API

The Rust daemon and legacy Bun dashboard expose:

- `GET /health`
- `GET /api/version`
- `GET /api/settings`
- `PUT /api/settings`
- `GET /api/snapshot`
- `GET /api/history?limit=&window_seconds=&since_ms=&until_ms=`
- `GET /api/history/coverage` in the Rust daemon
- `GET /api/history/points?limit=&window_seconds=&since_ms=&until_ms=&source=`
- `GET /api/history/markers?limit=&window_seconds=&since_ms=&until_ms=&expected_gap_ms=`
- `GET /vendor/echarts.min.js`
- static frontend assets: `/`, `/index.html`, `/styles.css`, `/app.js`

See [docs/guides/API.md](docs/guides/API.md) for request and response details.

## Legacy Collector API

The Rust daemon exposes these routes on `127.0.0.1:4274`. The legacy split Bun collector exposes the same routes on `127.0.0.1:4276`:

- `GET /health`
- `GET /version`
- `GET /snapshot/latest`
- `GET /snapshot/collect`
- `GET /history?limit=&window_seconds=&since_ms=&until_ms=`

The legacy collector API is internal. It binds to loopback by default and should not be exposed publicly.

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

CREATE TABLE IF NOT EXISTS app_settings (
  setting_key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS metric_rollups_1m (
  bucket_start_ms INTEGER PRIMARY KEY,
  first_captured_at_ms INTEGER NOT NULL,
  newest_captured_at_ms INTEGER NOT NULL,
  sample_count INTEGER NOT NULL,
  avg_cpu_usage_percent REAL NOT NULL,
  max_cpu_usage_percent REAL NOT NULL,
  avg_memory_used_percent REAL NOT NULL,
  max_memory_used_percent REAL NOT NULL,
  avg_swap_used_percent REAL NOT NULL,
  max_swap_used_percent REAL NOT NULL,
  avg_load_percent REAL NOT NULL,
  max_load_percent REAL NOT NULL,
  avg_root_used_percent REAL
);

CREATE INDEX IF NOT EXISTS idx_metric_rollups_1m_newest
  ON metric_rollups_1m (newest_captured_at_ms DESC);

CREATE TABLE IF NOT EXISTS app_events (
  event_id INTEGER PRIMARY KEY,
  occurred_at_ms INTEGER NOT NULL,
  marker_type TEXT NOT NULL,
  label TEXT NOT NULL,
  details_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_app_events_occurred_type
  ON app_events (occurred_at_ms DESC, marker_type);
```

The current implementation stores indexed graph/query columns plus full `SystemSnapshot` JSON. It also stores dashboard daemon defaults as typed JSON in `app_settings` under `setting_key = 'dashboard'`, maintains one-minute aggregate metric buckets in `metric_rollups_1m`, records daemon timeline events in `app_events`, and exposes coverage metadata through `/api/history/coverage`. This supports refresh-safe chart hydration, rollup-backed long ranges, selected raw-sample detail rendering, shared daemon defaults for future dashboard loads, retention enforcement, and history coverage display.

In the Rust daemon, every stored sample also refreshes its one-minute rollup bucket. Raw samples are pruned by `retentionHours`; rollup buckets are pruned by `rollupRetentionDays`. The legacy Bun split path still keeps raw rows until manual archive/reset. Normalized filesystem/process/pressure child tables remain future work.

## Frontend State

Browser-local settings:

- `tinytop.theme`
- `tinytop.graphMode`
- `tinytop.historyWindow`
- `tinytop.visibleSeries`
- `tinytop.processFilter`
- `tinytop.processSort`
- `tinytop.processDensity`
- `tinytop.filesystemShowSystem`
- `tinytop.lastSection`

SQLite-backed daemon defaults:

- `defaultTheme`
- `defaultGraphMode`
- `pollIntervalMs`
- `defaultHistoryWindow`
- `retentionHours`
- `rollupRetentionDays`
- `targetDatabaseBytes`
- `topProcessCount`
- `redactionDefault`
- `thresholds`
- `enabledSections`

In-memory session state:

- hydrated snapshots
- selected timeline timestamp and timeline markers
- ECharts instance
- pause/loading flags
- active confirmation dialog resolver and return-focus target
- active settings dialog focus-return target
- active process-detail dialog

The browser loads raw Live, 15m, and 1h timestamp ranges with `since_ms` and `until_ms` query parameters. The 6h, 24h, 7d, and 30d presets use `/api/history/points` with one-minute rollups. Large raw ranges are paged through the existing API limit, deduplicated by captured timestamp, and downsampled to a browser rendering cap when needed. This browser cap is a UI memory/rendering policy, not the SQLite retention policy. The timeline rail draws an overview trace from loaded samples or rollup points, uses timestamp selection rather than sample-index state, and overlays daemon-start, settings-change, and coverage-gap markers. Bar mode calculates the number of visible bars from the chart width so bars never shrink below the configured minimum width.

Web UI interaction policy:

- Public browser code must not call native `alert`, `confirm`, or `prompt`.
- Inline errors render through the `status-message` surface.
- Browser-local destructive actions use the reusable `<dialog>` confirmation flow in the dashboard `app.js`.
- Confirmed actions must describe their scope before continuing; for example, clearing History affects only the current tab's loaded samples and does not delete SQLite history.

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
Bun split path remains available through `tinytop-collector.service` and
`tinytop-dashboard.service` when explicitly installed with `--bun`.

## Decisions

Architecture decision records live in [docs/adr/README.md](docs/adr/README.md).

- [0001 - SQLite Writer Process](docs/adr/0001-sqlite-writer-process.md)
- [0002 - Initial Snapshot JSON History](docs/adr/0002-initial-snapshot-json-history.md)
- [0003 - Bash Bootstrap Plus Bun Install Wizard](docs/adr/0003-bash-bootstrap-bun-install-wizard.md)
- [0004 - Additive Rust Agent With SQLx Store](docs/adr/0004-rust-agent-sqlx-store.md)
- [0005 - Rust Single-Daemon Systemd Runtime](docs/adr/0005-rust-single-daemon-systemd-runtime.md)
- [0006 - Embed Dashboard Assets In The Rust Collector](docs/adr/0006-embedded-dashboard-assets.md)
- [0007 - Daemon And Browser Dashboard Settings](docs/adr/0007-daemon-and-browser-dashboard-settings.md)
- [0008 - Present Dashboard Settings As A Dialog](docs/adr/0008-settings-dialog-presentation.md)
