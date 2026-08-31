# Architecture

TinyTop's default persistent runtime is a single local Rust daemon. It serves the browser dashboard, collects Linux/WSL telemetry by default, owns SQLite, and exposes dashboard/history APIs over loopback.

The original Bun dashboard and legacy collector remain in the repo for TypeScript development and fallback.

OpenTelemetry export is Rust-daemon-only. The daemon pushes the latest snapshot over OTLP/HTTP; the Bun runtime neither exports to nor reads from OTel.
`tinytop-agent`'s `METRIC_REGISTRY` is the single source for exported metric names, units, families, descriptions, semantic-convention status, and the selection API.

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
  | GET /api/history/gpus
  | GET /api/version
  | GET/PUT /api/settings
  | GET /api/settings/export, POST /api/settings/import[?dryRun=true]
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

The supported Linux/WSL operator entrypoint is the root `./tinytop` Bash command center. It works before Bun is installed for help and bootstrap, auto-selects the Rust collector/dashboard daemon for `./tinytop start` when a release binary or Cargo is available, and supports `TINYTOP_RUNTIME=legacy` for the Bun fallback. Windows uses `tinytop.ps1`, which manages Rust release-binary install, local Windows builds, foreground lifecycle, status/logs, and Windows Service Control Manager commands.

## Data Flow

1. The browser loads the shared dashboard assets: `index.html`, `styles.css`, `/vendor/echarts.min.js`, `app.js`, and `ladder-rules.js`.
2. `app.js` requests `/api/settings` for SQLite-backed daemon defaults.
3. `app.js` reads browser-local theme, graph-mode, history-range, visible-series, process-table, filesystem-toggle, and last-section overrides from `localStorage`.
4. The frontend requests `/api/history` with explicit `since_ms` and `until_ms` bounds for the raw Live, 15m, and 1h ranges.
5. For 6h, 24h, 7d, 30d, 90d, 1y, and All, the frontend makes one `/api/history/points?source=auto&limit=10000` request and renders the tier reported by `source` and `resolutionMs`.
6. The Rust read surface also exposes `/api/history/filesystems`, `/api/history/gpus`, and `/api/history/processes` for typed historical detail.
7. The frontend requests `/api/history/markers` for daemon starts, settings changes, migration events, and computed coverage gaps.
8. The frontend requests `/api/history/coverage` when the Rust daemon is serving the page; dashboard polling coalesces concurrent requests and throttles routine refreshes.
9. The frontend requests `/api/version` once to display the serving runtime and product version.
10. The frontend polls `/api/snapshot` on the configured browser refresh interval.
11. `tinytop-agent serve` answers `/api/snapshot` from the latest snapshot published by the collection task (`503` before the first collection).
12. The Rust daemon collects telemetry on a timer and stores samples through `tinytop-store`.
13. `tinytop-store` writes raw samples, all enabled rollup tiers, typed detail rows, daemon timeline events, and daemon defaults into SQLite through SQLx.
14. `collect_and_store` publishes the latest snapshot through a Tokio `watch` channel; the daemon's independent OTel exporter task reads that channel and pushes gauges on its configured interval without recollecting.
15. The frontend pages raw ranges, reads tier-selected points for long ranges, deduplicates samples by timestamp, down-samples only for browser rendering, updates CPU/RAM/swap/load gauges, computes threshold states, and redraws ECharts views.

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
| `tinytop.ps1` | Windows PowerShell command center for Rust binary install/build, lifecycle, logs, status, and Windows service commands |
| `agent/assets/dashboard/` | The single dashboard asset tree: embedded by the Rust agent at compile time (`include_bytes!`), served from disk by the Bun server |
| `tests/` | Bun tests for parsers, snapshot building, server routes, and history storage |
| `agent/crates/tinytop-types` | Rust snapshot structs serialized to the existing dashboard JSON contract |
| `agent/crates/tinytop-collectors` | Rust platform collector crate; Linux/WSL default plus feature-gated macOS/Windows native collector modules and a detection-gated GPU backend (Linux implemented; Windows/macOS explicit Task 16 returns) |
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

The Rust Linux/WSL collector keeps the same `SystemSnapshot` contract as the Bun collector while using Rust crates for host access. It owns three cadence classes behind the one writer tick: fast uptime, CPU, memory, swap, load, pressure, and processes refresh every `pollIntervalMs`; slow filesystems refresh every `retentionLadder.detailIntervalSec` and are served from cache with their `filesystemsCapturedAtMs`; static hostname, kernel, and distro identity is re-read on the slow tick. It uses `procfs` for Linux kernel metrics and `sysinfo` for process and identity data. Per-filesystem inode counts, which `sysinfo` does not expose, are read directly with the `statvfs(2)` syscall via `rustix` (ADR 0012), so `statvfs` runs once per mount per `detailIntervalSec` instead of once per mount per tick. It does not shell out to `df`, `ps`, or `uname`. The live collector keeps a reusable `sysinfo::System` across samples so process and CPU refreshes have previous state. The Rust store uses SQLx with SQLite today, with SQL isolated in `tinytop-store` so future PostgreSQL/MySQL support does not leak into collector code.

The Linux GPU backend is detected at collector start and re-detected on the
same slow tick for hotplug. When present, it samples DRM sysfs, readable
`/proc/<pid>/fdinfo`, and the GPU node's hwmon on every fast tick; when absent,
the tick has no GPU work and the snapshot omits `gpus`.

The opt-in Linux thermal collector follows the same two-phase cadence: it
discovers allowed hwmon sensors on the slow tick and reads their values on every
fast tick. Stable identities are interned in `sensor_dim`, while raw values are
stored in `sensor_samples` and pruned at the L1 horizon.

The daemon task set includes collection/history maintenance, disk checking, cold archive export, and the OTel exporter. Collection publishes each freshly collected snapshot on a Tokio `watch` channel before attempting SQLite persistence; the exporter task samples the most recent value at export time, applies configured resource attributes and environment-provided headers, and never delays collection when persistence or an export fails. After persistence and maintenance, `collect_and_store` re-configures the collector only when `topProcessCount`, `detailIntervalSec`, or either thermal setting changed, so a saved setting applies on the next collection tick.

The collector crate exposes `NativeCollector` behind target and Cargo feature gates:

- default Linux builds use `linux-collector`
- macOS builds can use `--no-default-features --features macos-collector`
- Windows builds can use `--no-default-features --features windows-collector`

The macOS and Windows modules currently provide the first native slice through `sysinfo`: identity, CPU, memory/swap, load equivalent, disks, and top processes including parent PID/start time when available. Linux remains the reference implementation because pressure, exact `/proc` load thread counts, and live-host parity have not yet been validated on macOS/Windows.

Windows builds use:

```powershell
cargo build --release --manifest-path agent/Cargo.toml -p tinytop-agent --no-default-features --features windows-collector
```

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
- `GET /api/history/filesystems?sinceMs=&untilMs=&mount=&limit=` in the Rust daemon
- `GET /api/history/gpus?sinceMs=&untilMs=&adapter=&limit=` in the Rust daemon
- `GET /api/history/processes?sinceMs=&untilMs=&limit=` in the Rust daemon
- `GET /api/history/markers?limit=&window_seconds=&since_ms=&until_ms=&expected_gap_ms=`
- `GET /vendor/echarts.min.js`
- static frontend assets: `/`, `/index.html`, `/styles.css`, `/app.js`, `/ladder-rules.js`

See [docs/guides/API.md](docs/guides/API.md) for request and response details.

## Legacy Collector API

The Rust daemon exposes these routes on `127.0.0.1:4274`. The legacy split Bun collector exposes the same routes on `127.0.0.1:4276`:

- `GET /health`
- `GET /version`
- `GET /snapshot/latest`
- `GET /snapshot/collect`
- `GET /history?limit=&window_seconds=&since_ms=&until_ms=`

The legacy collector API is internal. It binds to loopback by default and should not be exposed publicly.

## Tiered History Ladder

The Rust store retains history as four tiers: L1 raw samples, L2 one-minute
buckets, L3 five-minute buckets, and L4 hourly buckets. One weighted fold shape
serves every rung; completed buckets freeze after their grace window, and each
enabled coarser tier is promoted before its finer source may be pruned. The
retention horizons and L3/L4 toggles come from `retentionLadder` settings.

Existing populated v0 databases migrate to SQLite `user_version = 1` only after
`VACUUM INTO` has created the complete, non-overwriting
`<database>.pre-v0.sqlite` pre-image. Migration then changes the schema in one
transaction; the pre-image remains until an operator explicitly removes it with
the guarded CLI.

The additive read surface keeps `/api/history` for typed, assembled raw snapshots,
uses `/api/history/points?source=auto` to choose the finest enabled tier that
retains and fits the requested range, reports the ladder through
`/api/history/coverage`, and exposes typed filesystem/process detail endpoints.
The full schema and maintenance/read algorithms live in
[SQLite History Architecture](docs/sqlite-history-architecture.md); the decision
and approved design are [ADR 0013](docs/adr/0013-tiered-history-ladder.md) and the
[tiered-history design](docs/superpowers/specs/2026-08-28-tiered-history-ladder-design.md).

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

Core schema-v4 table excerpt (see [Tiered History Ladder](#tiered-history-ladder) and the SQLite architecture document for the complete tier/detail tables):

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
  runnable_threads INTEGER,
  total_threads INTEGER,
  root_used_percent REAL,
  identity_id INTEGER REFERENCES host_identity(identity_id),
  uptime_seconds INTEGER,
  memory_available_bytes INTEGER,
  swap_free_bytes INTEGER,
  last_pid INTEGER,
  filesystems_captured_at_ms INTEGER,
  CHECK (identity_id IS NULL OR (
    uptime_seconds IS NOT NULL
    AND memory_available_bytes IS NOT NULL
    AND swap_free_bytes IS NOT NULL
  ))
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
  avg_root_used_percent REAL,
  min_cpu_usage_percent REAL,
  min_memory_used_percent REAL,
  min_swap_used_percent REAL,
  min_load_percent REAL,
  min_root_used_percent REAL,
  max_root_used_percent REAL
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

The current v5 implementation stores typed graph/query columns, per-row assembly scalars, interned host identity, and on-change filesystem/presence rows for every raw history sample. `host_identity` is unique over the eight stable identity strings; `fs_mount_events` preserves mount appearance and disappearance independently of value changes. Schema v4's interned GPU identity and process-time layout remain unchanged; v5 adds `sensor_dim` and `sensor_samples` without rebuilding an existing table. The store also keeps daemon defaults in `app_settings`, L2/L3/L4 aggregate buckets, typed process detail rows, maintenance/migration state, and timeline events; `/api/history/coverage` reports the resulting ladder and disk/archive state.

Rust retention is the four-tier ladder described in [Tiered History Ladder](#tiered-history-ladder): each sample refreshes L2, completed buckets promote to enabled coarser tiers before finer rows are pruned, and `retentionHours` / `rollupRetentionDays` are derived compatibility mirrors of L1/L2. The legacy Bun split path still keeps raw rows until manual archive/reset.

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

The browser loads raw Live, 15m, and 1h ranges with explicit time bounds. Every preset from 6h through All uses one `/api/history/points?source=auto&limit=10000` request, so the Rust daemon selects the finest enabled tier that retains and fits the range; Bun disables those Rust-only presets. Raw ranges are paged and browser rendering may downsample, but neither transport nor rendering limits change SQLite retention. The timeline uses timestamp selection and overlays daemon-start, settings-change, migration, and coverage-gap markers. Bar mode derives visible bars from chart width so bars keep their minimum width.

Web UI interaction policy:

- Public browser code must not call native `alert`, `confirm`, or `prompt`.
- Inline errors render through the `status-message` surface.
- Browser-local destructive actions use the reusable `<dialog>` confirmation flow in the dashboard `app.js`.
- Confirmed actions must describe their scope before continuing; for example, clearing History affects only the current tab's loaded samples and does not delete SQLite history.
- Settings checkboxes keep native checkbox semantics in the DOM but are presented as responsive toggle controls so enabled-section and redaction settings remain dense, touch-friendly, and keyboard-focusable.

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

Windows service integration uses `tinytop.ps1 service install|uninstall|start|stop|restart|status`. Install and uninstall require an elevated PowerShell session because they write to Windows Service Control Manager. The service runs `tinytop-agent.exe serve` with explicit host, port, and SQLite path arguments.

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
- [0009 - Additive History Points And Markers API](docs/adr/0009-additive-history-points-and-markers-api.md)
- [0010 - Feature-Gated Native Platform Collectors](docs/adr/0010-feature-gated-native-platform-collectors.md)
- [0011 - PowerShell-First Windows Command Center](docs/adr/0011-powershell-first-windows-command-center.md)
