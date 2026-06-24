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
- Live History top-nav controls: line, stacked area, stacked bar, heatmap, and treemap views
- Apache ECharts-powered Live History chart with tooltips, axes, visible-window sample count, and selectable samples
- Timeline scrubber under the history chart with selected datetime context, compact selected-sample values, and a return-to-live control
- Heatmap view shows discrete metric/time cells where stronger color means a higher sampled value

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

Theme and history-view selections are browser-local preferences stored in `localStorage`. The timeline scrubber and selectable ECharts chart use only the current browser session's rolling samples and label the selected sample with its local datetime and metric values. Heatmap mode renders CPU, RAM, swap, and load as discrete metric/time cells for spotting spikes and quiet stretches. These controls do not change the system data collection path or write to WSL/Linux configuration.
