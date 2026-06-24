# WSL Status Dashboard Design

## Goal

Build a standalone, visually polished Bun web dashboard that shows live WSL/Linux status: CPU, RAM, swap, kernel, load, filesystem, pressure, and process data.

## Visual Direction

The app uses a dark OLED operations cockpit style: deep black background, slate panels, crisp Inter/system typography, small-radius panels, and restrained green, cyan, violet, amber, and red accents. It should feel like a premium local developer workstation monitor, not a marketing page.

## Layout

- Left rail: product title, live state, compact navigation anchors, and runtime identity.
- Header strip: host, distro, kernel, WSL status, uptime, refresh state, pause/resume, and manual refresh.
- Primary gauges: CPU, RAM, and swap as circular gauges with large numeric values.
- Secondary metrics: load averages, process count, root filesystem, pressure, and update latency.
- Filesystem section: capacity bars and inode bars for mounted filesystems.
- History section: streaming CPU, RAM, swap, and load charts.
- Process table: top processes by CPU and memory.

## Data Sources

- `/proc/meminfo`
- `/proc/stat`
- `/proc/loadavg`
- `/proc/uptime`
- `/proc/pressure/{cpu,memory,io}`
- `/etc/os-release`
- `uname -r`
- `df -PB1 -T`
- `df -Pi -T`
- `ps`

## Interactions

- Auto-refresh every 1.5 seconds.
- Pause/resume live polling.
- Manual refresh.
- Visible stale/error state if a snapshot fails.
- Reduced-motion support freezes decorative transitions while keeping values readable.

## Acceptance Criteria

- Runs with `bun run dev`.
- Serves on `127.0.0.1:4274`.
- Automatically detects whether the app is running in WSL, real Linux, or an unknown Linux-like environment.
- `/api/snapshot` returns real local data, not fixtures.
- Browser UI contains gauges, stats, charts, filesystem bars, and a process table.
- Unit tests cover parsers and normalization logic.
- Browser QA verifies desktop and mobile layout.
