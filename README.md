# WSL Status Dashboard

A standalone Bun-powered web dashboard for live WSL/Linux workstation status.

## Run

```bash
bun run dev
```

Open <http://127.0.0.1:4274>.

## What It Shows

- CPU utilization and load averages
- RAM and swap usage
- Kernel, distro, uptime, and automatic WSL versus real Linux detection
- Filesystem capacity and inode pressure
- CPU, memory, and I/O pressure from `/proc/pressure/*` when available
- Top processes by CPU and memory
- Live gauges, sparklines, a first-screen history chart, status strips, and stat tiles
- Theme controls: Midnight, Matrix, Aurora, Solar, and Ember
- History view controls: line, area, and heatmap views
- Timeline scrubber that inspects older local samples in the main gauges and returns to live mode

## Port

The dashboard claims `127.0.0.1:4274` in `~/.config/fleet/ports/wsl-status-dashboard.toml`.

## Verification

```bash
bun test
bun run src/server.ts --check
```

## Runtime Detection

The backend classifies the host as `WSL`, `Linux`, or `Unknown`. It checks kernel release/version markers first, then WSL-specific environment variables, and falls back to real Linux when no WSL markers are present.

## Display Controls

Theme and history-view selections are browser-local preferences stored in `localStorage`. The timeline scrubber uses only the current browser session's rolling samples. These controls do not change the system data collection path or write to WSL/Linux configuration.
