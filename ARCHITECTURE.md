# Architecture

The dashboard is a single-process local Bun app. `Bun.serve()` hosts a static frontend from `public/` and exposes `/api/snapshot` for live system telemetry collected from Linux/WSL sources.

## Data Flow

1. The browser loads `public/index.html`, `public/styles.css`, `/vendor/echarts.min.js`, and `public/app.js`.
2. The frontend polls `/api/snapshot` on a short interval.
3. The Bun server reads `/proc`, `df`, `ps`, `uname`, and OS release files.
4. Pure parser functions normalize raw system text into a JSON snapshot.
5. The frontend renders gauges, stat tiles, Apache ECharts live history charts, filesystem bars, pressure panels, and process rows.

## Boundaries

- `src/parsers.ts` contains pure parsing and normalization logic.
- `src/collector.ts` performs live filesystem and process reads.
- `src/server.ts` owns HTTP routing and static file serving, including the local ECharts browser bundle route.
- `public/` contains code-native UI assets and the app shell that uses Apache ECharts from `node_modules`.

## Frontend State

Theme and history view mode are frontend-only preferences. The browser stores them in `localStorage` under the `wsl-status-dashboard.*` key prefix. The frontend also keeps a rolling in-memory snapshot buffer for the current session so the timeline scrubber and ECharts history chart can render older samples into the main gauges and label the selected sample with local datetime context. These controls affect CSS variables, chart rendering, and which already-collected sample is displayed; they do not affect collection, server routing, or host state.

## Safety

The app is read-only. It does not restart services, kill processes, change sysctl values, modify WSL configuration, or write system data. It binds to loopback by default.

## Runtime Detection

Runtime detection is explicit. The collector checks `/proc/sys/kernel/osrelease` and `/proc/version` for Microsoft/WSL markers. If those are absent, it checks `WSL_DISTRO_NAME` and `WSL_INTEROP`. If no WSL markers exist and Linux kernel metadata is present, the runtime is classified as real Linux.
