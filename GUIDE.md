# User Guide

This guide explains how to use TinyTop after it is running.

Open the dashboard:

```text
http://127.0.0.1:4274
```

Start TinyTop:

```bash
./tinytop start
```

`./tinytop start` auto-selects the Rust collector/dashboard daemon when a Rust binary or Cargo is available. Use `TINYTOP_RUNTIME=legacy ./tinytop start` only when you explicitly want the legacy Bun dashboard/collector path.

Use persistent user services:

```bash
./tinytop systemd install --rust
./tinytop systemd start
```

## Layout

The dashboard is organized for quick scanning:

1. Left rail: runtime summary, collector/dashboard version, navigation, Settings button, live status.
2. Top identity strip: host, kernel, distro, uptime.
3. Display controls: theme selection.
4. Operator strip: current Healthy, Warning, Critical, or Stale state, worst offender, last-sample age, and a detail drawer.
5. Overview gauges: CPU, RAM, swap, and load.
6. History: graph-type nav, range presets, ECharts chart, timeline rail, coverage, and selected sample values.
7. Metric band: load, thread count, root filesystem, runtime.
8. Filesystem and pressure panels.
9. Process table.

## Live Status

The rail status shows the polling state:

- `Live` - polling and rendering current samples.
- `Paused` - polling paused by the user.
- `Error` - latest fetch failed; the inline status message explains the failure.

The operator strip shows:

- `Healthy` - all tracked metrics are below warning thresholds.
- `Warning` - at least one metric crossed its warning threshold.
- `Critical` - at least one metric crossed its critical threshold or the current snapshot fetch failed.
- `Stale` - the latest collector sample is older than the expected polling window.

Click the operator strip or its `Details` button to open the alert detail drawer. It lists the current metric values, warning/critical thresholds, sample age, recent trend, and what changed recently for the worst offender.

The sidebar version line shows the serving runtime and product version, for example `Rust collector/dashboard v0.1.27`. The same identity is available from:

```bash
curl -fsS http://127.0.0.1:4274/api/version
```

## Refresh And Pause

- `Refresh` requests a fresh snapshot immediately.
- `Pause` stops browser polling. The Rust daemon or legacy Bun collector can still continue collecting samples in the background.
- `Resume` restarts browser polling and returns the UI to live updates.

## Confirmation Windows

TinyTop uses in-app confirmation windows for browser UI actions that discard local state. It does not use native browser `alert`, `confirm`, or `prompt` dialogs.

The History `Clear` button asks for confirmation before clearing the samples currently loaded in the browser tab. This does not delete SQLite history, stop the daemon, or change system data.

## Themes

Theme choices are stored in browser `localStorage`:

- Midnight
- Matrix
- Aurora
- Solar
- Ember

Themes affect the browser only. They do not change collection, SQLite, or system state.

## Settings

The Settings dialog opens from the left rail and is split by scope:

- `This Browser` controls the active theme, graph mode, and history window for the current browser profile. Additional browser-local state includes visible chart series, process table filter/sort/density, filesystem system-mount toggle, and last-used section.
- `This Daemon` controls defaults stored by the Rust daemon in SQLite. These include default theme, default graph mode, browser refresh interval, default history window, retention and rollup defaults, target DB budget, top process count, redaction default, warning/critical thresholds, and enabled dashboard sections.

The dialog validates ranges before saving, warns about unsaved daemon changes before closing, offers threshold presets, can reset the form back to the loaded daemon values, can stage factory defaults, and shows an effective settings readout. Saving daemon defaults uses `PUT /api/settings`. A browser-local setting wins for that browser; daemon defaults are used when no local override exists.

## History

History renders CPU, RAM, swap, and load-derived percent values from SQLite-backed collector samples.

The browser hydrates recent samples from SQLite on page load, so refreshing the page should not reset the chart to a single sample.

The default page-load request uses the `Live` range preset. You can switch the browser's loaded range to `15m`, `1h`, `6h`, `24h`, `7d`, or `30d`. Live, 15m, and 1h use raw snapshots. The 6h and longer ranges use one-minute rollup points through `/api/history/points`, then downsample only when the browser needs fewer points to render smoothly. These ranges are read windows, not the database retention period.

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

- Choose `Live`, `15m`, `1h`, `6h`, `24h`, `7d`, or `30d` to load that timestamp range.
- Drag the timeline rail to inspect the nearest loaded sample by timestamp.
- The main gauges and detail panels update to the selected raw sample. Rollup points update the History readout without replacing live filesystem/process detail with aggregate placeholders.
- The position label shows the selected local datetime.
- The coverage row shows oldest sample, newest sample, database size, target DB budget, budget usage, and rollup bucket count when the Rust daemon serves `/api/history/coverage`.
- Timeline markers show daemon starts, settings changes, and coverage gaps from `/api/history/markers`.
- Click `Now` beside the rail to return to the newest sample in the loaded range.
- Click `Clear` to empty the current tab's session buffer after confirming.

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
- daemon dashboard defaults
- one-minute metric rollups in the Rust daemon
- daemon timeline events for starts and settings changes

SQLite retention:

- The Rust daemon prunes raw samples by `retentionHours`.
- The Rust daemon prunes one-minute rollups by `rollupRetentionDays`.
- The Rust daemon reports target DB budget usage from `targetDatabaseBytes`.
- Legacy Bun split mode keeps raw samples until you archive or reset local history.
- `/api/history` query windows limit what is returned to the browser; retention settings control pruning.

Persisted in browser `localStorage`:

- theme
- graph mode
- selected history range
- visible history series
- process table filter, sort, and density
- filesystem system-mount toggle
- last section

Not persisted:

- selected timeline position after page reload
- pause state
- scroll position

## Reading The Numbers

- CPU is calculated from `/proc/stat` deltas.
- RAM and swap come from `/proc/meminfo`.
- Load percent is derived from 1-minute load divided by CPU core count, capped to 100 for overview gauge and chart display.
- Pressure values come from `/proc/pressure/*` when available.
- In the Rust daemon, filesystem and process data come from Rust crates instead of shelling out.
- Process detail rows include parent PID and start time when the active collector can provide them. The copy command uses a redacted command string to avoid copying obvious token/password values.
- In legacy Bun mode, filesystem capacity comes from `df` and process rows come from `ps`.

## Refresh Behavior

On page load:

1. The browser requests runtime identity from `/api/version`.
2. The browser requests recent raw history from `/api/history` or rollup points from `/api/history/points`, depending on the selected range.
3. It requests timeline markers from `/api/history/markers`.
4. It fills the chart and timeline from SQLite-backed samples or rollup points in the selected timestamp range.
5. It requests the latest snapshot from `/api/snapshot`.
6. It starts polling every 1500 ms.

If history is unavailable, the dashboard still works from live polling, but the chart starts with newly collected samples.

## Privacy And Safety

The dashboard is local by default. It binds to `127.0.0.1`, reads local system telemetry, and writes local SQLite history. It does not send telemetry to an external service.
