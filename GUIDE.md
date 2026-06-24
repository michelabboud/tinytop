# User Guide

This guide explains how to use TinyTop after it is running.

Open the dashboard:

```text
http://127.0.0.1:4274
```

## Layout

The dashboard is organized for quick scanning:

1. Left rail: runtime summary, navigation, live status.
2. Top identity strip: host, kernel, distro, uptime.
3. Display controls: theme selection.
4. Overview gauges: CPU, RAM, and swap.
5. Live History: graph-type nav, ECharts chart, timeline scrubber.
6. Metric band: load, thread count, root filesystem, runtime.
7. Filesystem and pressure panels.
8. Process table.

## Live Status

The rail status shows the polling state:

- `Live` - polling and rendering current samples.
- `Paused` - polling paused by the user.
- `Error` - latest fetch failed; the alert text explains the failure.

## Refresh And Pause

- `Refresh` requests a fresh snapshot immediately.
- `Pause` stops browser polling. The writer process can still continue collecting samples in the background.
- `Resume` restarts browser polling and returns the UI to live updates.

## Themes

Theme choices are stored in browser `localStorage`:

- Midnight
- Matrix
- Aurora
- Solar
- Ember

Themes affect the browser only. They do not change collection, SQLite, or system state.

## Live History

Live History renders CPU, RAM, swap, and load-derived percent values from the rolling sample buffer.

The browser hydrates recent samples from SQLite on page load, so refreshing the page should not reset the chart to a single sample.

The sample count badge shows:

- `N samples` when all samples are visible.
- `N samples / M shown` when the graph has more samples than the current visible window.

## Graph Modes

### Line

Shows each metric as an independent line over time. Use it to compare trends without stacking values.

### Area

Shows stacked filled areas. This emphasizes total pressure across CPU, RAM, swap, and load. Because it is stacked, vertical height is cumulative rather than each line having an independent baseline.

### Bar

Shows stacked bars per timestamp. Bar mode calculates visible capacity from chart width and enforces a minimum bar width. When the visible window is full, new bars enter from the right and older visible bars leave on the left.

### Heatmap

Shows metric/time cells. Stronger color means a higher sampled value. Use it to spot bursts or quiet periods across metrics.

### Treemap

Shows the selected or latest sample as proportional blocks. Use it for a compact current-sample composition view rather than a time series.

## Timeline

The timeline row sits below the chart.

- Drag the slider to inspect an older sample.
- The main gauges and detail panels update to the selected sample.
- The position label shows the selected local datetime.
- Click `Live` to return to the newest sample.

Keyboard controls on the chart:

- `ArrowLeft` - previous sample
- `ArrowRight` - next sample
- `Home` - oldest available sample
- `End` - return to live

## What Is Persisted

Persisted in SQLite:

- recent host snapshots
- timestamp
- graph metric columns
- full snapshot JSON for UI hydration

Persisted in browser `localStorage`:

- theme
- graph mode

Not persisted:

- selected timeline position after page reload
- pause state
- scroll position

## Reading The Numbers

- CPU is calculated from `/proc/stat` deltas.
- RAM and swap come from `/proc/meminfo`.
- Load percent is derived from 1-minute load divided by CPU core count, capped to 100 for chart display.
- Pressure values come from `/proc/pressure/*` when available.
- Filesystem capacity comes from `df`.
- Process rows come from `ps`, sorted by CPU.

## Refresh Behavior

On page load:

1. The browser requests recent history from `/api/history`.
2. It fills the chart and timeline from SQLite-backed samples.
3. It requests the latest snapshot from `/api/snapshot`.
4. It starts polling every 1500 ms.

If history is unavailable, the dashboard still works from live polling, but the chart starts with newly collected samples.

## Privacy And Safety

The dashboard is local by default. It binds to `127.0.0.1`, reads local system telemetry, and writes local SQLite history. It does not send telemetry to an external service.
