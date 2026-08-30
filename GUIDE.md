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

The GPU panel appears only when the daemon detects an adapter. It shows each adapter's name, busy percentage, VRAM used/total when the driver reports it, and temperature when the GPU node has a sensor. Busy is the busiest engine, matching Task Manager's rule; it is `—` on the first sample, on NVIDIA proprietary adapters, and wherever the kernel exposes no source. On Linux, fdinfo-derived busy is computed over only the processes the daemon's user can see. The Bun runtime and WSL2 never show the panel.

The sidebar version line shows the serving runtime and product version, for example `Rust collector/dashboard v0.1.34`. The same identity is available from:

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
- `This Daemon` controls defaults stored by the Rust daemon in SQLite. These include default theme, default graph mode, browser refresh interval, default history window, target DB budget, top process count, redaction default, warning/critical thresholds, and enabled dashboard sections.
- On the Rust daemon, `History ladder` controls L1 raw and L2 one-minute retention, optional L3 five-minute and L4 hourly tiers, the filesystem check interval (`Filesystem check seconds`, also the typed detail cadence), archive options, and disk-check thresholds. `processFastKeepHours` keeps per-tick process rows for 1–72 hours before older windows fall back to once-a-minute rows. L4 can be kept forever, and `History hours` / `Rollup days` are read-only compatibility mirrors derived from L1/L2. Bun has no ladder: the group is replaced by `History ladder — Rust daemon only`, the coverage card is hidden, the legacy retention inputs remain editable, and saves omit `retentionLadder`.

The dialog validates ranges before saving, including the ladder's monotonic tier rules, with the same field-specific messages as the Rust server. It warns about unsaved daemon changes before closing, offers threshold presets, can reset the form back to the loaded daemon values, can stage factory defaults, and shows an effective settings readout. Boolean daemon options, including redaction and enabled dashboard sections, render as compact responsive toggle controls so several options can fit per row on desktop while remaining touch-friendly on narrow screens. Saving daemon defaults uses `PUT /api/settings`. If a save shrinks a horizon or disables a tier/archive, the dialog asks the Rust server for a dry-run first and shows the rows or buckets older than the candidate horizons; an L1 shrink may list `GPU rows deleted`. Disabled tier tables and archive files are reported as retained, not deleted. A browser-local setting wins for that browser; daemon defaults are used when no local override exists.

On the Rust daemon, `Export JSON` downloads a versioned settings document and `Import JSON…` uploads one for a server dry-run before confirmation. The file contains the daemon settings and transfer metadata, never a secret; credentials remain outside settings. Invalid envelopes and settings show the server's validation messages, and retention growth may be refused while disk pressure is active. Shell operators can use `tinytop-agent config export [--out FILE]` and `tinytop-agent config import FILE --dry-run` before applying with `config import FILE`; CLI application records the import but leaves pruning to the daemon's next tick so a second process does not run maintenance beside it.

The Rust-only `OpenTelemetry` settings group controls push export over HTTP/protobuf: enabled, endpoint, interval, service name, resource attributes, and the `headersEnvVar` name. The referenced environment variable contains OTLP request headers such as `authorization=Bearer <token>` and is read by the daemon; header values never enter settings exports. Header values are read when the exporter pipeline is built, so apply a rotated value by toggling export off and on, changing the `otel` settings block, or restarting the daemon. Settings changes are picked up on the exporter's next 5-second tick; an export already in flight (bounded by its 10-second timeout) can delay that tick, so a change is applied within 10 seconds at worst and within 5 seconds when the receiver answers promptly. Changing only the environment does not rebuild an already-running pipeline. An absent `otel` block in an imported version-1 document keeps the daemon's persisted OTel settings. Bun has no exporter.

Browser validation of archive and database paths is advisory; the server validates the authoritative host-native path.

For an operator-managed cold copy, enable both `retentionLadder.archive.queryable` and `retentionLadder.archive.cold`. Completed eligible UTC months land in the configured archive directory (beside the main database when `directory` is empty) as `tinytop-1h-YYYY-MM.csv.gz` plus a `.sha256` sidecar, and a month is exported only once every one of its rows has left the main database. Run `sha256sum -c tinytop-1h-YYYY-MM.csv.gz.sha256` from that directory before copying or restoring a file. The CSV is the cold record; restore tooling should read it as an RFC 4180 document in archive DDL column order. Cold export never removes the queryable SQLite rows, and rows that arrive after their month was exported stay queryable rather than changing the sealed CSV.

## History

History renders CPU, RAM, swap, and load-derived percent values from SQLite-backed collector samples.

The browser hydrates recent samples from SQLite on page load, so refreshing the page should not reset the chart to a single sample.

On the Rust daemon, a red disk-pressure banner means the database filesystem has less free space than `retentionLadder.diskCheck.minFreeBytes`. TinyTop does not delete anything because of the banner: it continues collecting and still permits retention shrinkage, but refuses extending a horizon or enabling a tier/archive until pressure clears. Free disk space or shrink history, then wait for the next configured disk check; restarting the daemon also checks immediately. The timeline records one `diskPressure` marker when a breach begins and one `diskRecovered` marker when it clears.

The default page-load request uses the `Live` range preset. You can switch the browser's loaded range to `15m`, `1h`, `6h`, `24h`, `7d`, `30d`, `90d`, `1y`, or `All`. Live, 15m, and 1h use paged raw snapshots. From 6h up, one `source=auto&limit=10000` request lets the Rust daemon select the finest tier that both holds the range start and fits the response: at defaults, 6h → 1 minute (360 points), 24h → 1 minute (1,440), 7d → 5 minutes (2,016), 30d → 5 minutes (8,640), 90d → 1 hour (2,160), and 1y → 1 hour (8,760). All uses the coarsest tier holding the oldest data; its newest 10,000 hourly buckets span about 416 days, with the archive holding the rest. A long preset is disabled only when no enabled tier holds its start and the archive is not queryable; the tooltip names the controlling ladder setting. If the active preset becomes unavailable, the dashboard refetches the nearest finer preset without changing the saved choice. The browser down-samples only when it needs fewer points to render smoothly. These ranges are read windows, not the database retention period.

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

- Choose `Live`, `15m`, `1h`, `6h`, `24h`, `7d`, `30d`, `90d`, `1y`, or `All` to load that timestamp range; presets from `6h` up need the Rust daemon.
- Drag the timeline rail to inspect the nearest loaded sample by timestamp.
- The main gauges and detail panels update to the selected raw sample. Rollup points update the History readout without replacing live filesystem/process detail with aggregate placeholders.
- The position label shows the selected local datetime.
- The coverage card shows oldest/newest samples, database size and budget, each available ladder tier's horizon/count/range, disk pressure, and archive status when the Rust daemon serves those `/api/history/coverage` fields. Older runtimes omit the newer portions without breaking the card.
- Timeline markers show daemon starts, settings changes, disk-pressure/recovery transitions, and coverage gaps from `/api/history/markers`.
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
- typed history tables used to assemble detail views since schema v3; when a value was not recorded, Pressure and Threads/runnable show `—`, and selecting a point in `live`, `15m`, or `1h` renders its processes and filesystems
- daemon dashboard defaults
- one-minute metric rollups in the Rust daemon
- daemon timeline events for starts, settings changes, disk-pressure breaches (`diskPressure`), and recoveries (`diskRecovered`)

SQLite retention:

- The Rust daemon prunes and promotes history according to `retentionLadder`; L1/L2 also derive the saved `retentionHours` and `rollupRetentionDays` compatibility mirrors.
- The Rust daemon reports target DB budget usage from `targetDatabaseBytes`.
- Legacy Bun split mode keeps raw samples until you archive or reset local history.
- `/api/history` query windows limit what is returned to the browser; retention settings control pruning.

Persisted in browser `localStorage`:

- theme
- graph mode
- selected history range
- visible history series
- process table filter, PID/CPU/RAM/RSS/GPU sort, and density
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
- GPU busy is the busiest engine's percentage over the sampling interval, capped at 100%.
- Per-process GPU percentage is shown only when at least one process row has a value.
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

The daemon's own collection tick is `pollIntervalMs`; filesystems are re-checked every `detailIntervalSec` (default 60 seconds), and the Filesystem panel shows `as of hh:mm:ss` when its rows are older than one poll. A saved `Top processes` count applies on the daemon's next collection tick.

If history is unavailable, the dashboard still works from live polling, but the chart starts with newly collected samples.

## Privacy And Safety

The dashboard is local by default. It binds to `127.0.0.1`, reads local system telemetry, and writes local SQLite history. Nothing leaves the box by default. The only outbound path is the OpenTelemetry export, which is off by default; after the operator enables it in the Settings dialog or with `config import`, it sends the §12 system metrics (never process names, settings, or header values) to the endpoint the operator configures.
