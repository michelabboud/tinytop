# Architecture

The dashboard is a two-process local Bun app. The public dashboard process serves static frontend assets from `public/` on `127.0.0.1:4274`. A private collector/writer process runs on `127.0.0.1:4276`, collects Linux/WSL telemetry, owns the SQLite database, and exposes the local read API used by the dashboard process.

## Data Flow

1. The browser loads `public/index.html`, `public/styles.css`, `/vendor/echarts.min.js`, and `public/app.js`.
2. The frontend first requests `/api/history` to hydrate the visible rolling window from SQLite.
3. The frontend polls `/api/snapshot` on a short interval.
4. The dashboard process proxies `/api/history` and `/api/snapshot` to the writer process.
5. The writer process reads `/proc`, `df`, `ps`, `uname`, and OS release files.
6. Pure parser functions normalize raw system text into a JSON snapshot.
7. The writer process stores samples in SQLite and serves current/history reads back to the dashboard.
8. The frontend renders gauges, stat tiles, Apache ECharts live history charts, filesystem bars, pressure panels, and process rows.

## Boundaries

- `src/parsers.ts` contains pure parsing and normalization logic.
- `src/collector.ts` performs live filesystem and process reads.
- `src/history-store.ts` owns SQLite schema setup, prepared statements, inserts, and timestamp-range reads.
- `src/collector-daemon.ts` owns the writer HTTP API and scheduled collection loop.
- `src/server.ts` owns public HTTP routing, static file serving, local ECharts browser bundle routing, and proxying dashboard reads to the writer process.
- `public/` contains code-native UI assets and the app shell that uses Apache ECharts from `node_modules`.

## Frontend State

Theme and history view mode are frontend-only preferences. The browser stores them in `localStorage` under the `wsl-status-dashboard.*` key prefix. The frontend keeps a rolling in-memory snapshot buffer hydrated from `/api/history` and updated by `/api/snapshot`. The timeline scrubber and ECharts history chart render older samples into the main gauges and label the selected sample with local datetime context. These controls affect CSS variables, chart rendering, and which already-collected sample is displayed; they do not affect collection, server routing, or host state.

## Persistence

The persistence architecture is documented in `docs/sqlite-history-architecture.md`. It uses a dedicated Bun collector/writer process as the only SQLite owner, with the dashboard process reading history through that writer process rather than opening the database directly.

The architecture decisions are recorded in `docs/adr/0001-sqlite-writer-process.md` and `docs/adr/0002-initial-snapshot-json-history.md`.

## Safety

The app is read-only. It does not restart services, kill processes, change sysctl values, modify WSL configuration, or write system data. It binds to loopback by default.

## Runtime Detection

Runtime detection is explicit. The collector checks `/proc/sys/kernel/osrelease` and `/proc/version` for Microsoft/WSL markers. If those are absent, it checks `WSL_DISTRO_NAME` and `WSL_INTEROP`. If no WSL markers exist and Linux kernel metadata is present, the runtime is classified as real Linux.
