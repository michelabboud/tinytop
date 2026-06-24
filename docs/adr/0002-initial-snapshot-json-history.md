# 0002 - Initial Snapshot JSON History

## Status

Accepted

## Context

The dashboard needs refresh-safe recent history immediately. The first persisted workflow is the Live History chart and timeline, which need the same full `SystemSnapshot` objects the frontend already renders. Future process/filesystem analytics may need normalized child tables, but the current user-facing need is restoring recent samples without duplicating collection work or coupling SQLite to the dashboard process.

## Decision

The initial SQLite implementation uses one `metric_samples` table with:

- indexed `captured_at_ms` for range reads,
- typed graph/query columns for CPU, RAM, swap, load, runtime, and root filesystem pressure,
- `snapshot_json` containing the complete `SystemSnapshot` for gauge/detail restoration.

The writer process is still the only SQLite owner. The dashboard reads current and historical samples through the writer API.

## Alternatives Rejected

### Normalize Every Snapshot Detail Immediately

Filesystem, pressure, and process child tables remain useful later, but implementing them before a query uses them increases schema and write complexity without improving the refresh behavior. The current chart and timeline need complete snapshots, not independent process aggregation.

### Store Only Snapshot JSON

That would be simplest, but it would leave common graph/range fields hidden inside JSON. Keeping typed metric columns preserves efficient timestamp-window reads and a clear path to rollups.

## Consequences

- Refresh can restore the chart, scrubber, gauges, filesystem cards, pressure panels, and process rows from persisted recent samples.
- Timestamp-range queries use B-tree indexes on `captured_at_ms`.
- Future analytics can add child tables or rollups without changing the browser hydration contract.
