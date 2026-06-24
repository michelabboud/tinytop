# 0001 - SQLite Writer Process

## Status

Accepted

## Context

The dashboard needs persistent history so a refresh can render the recent window immediately instead of starting from an empty in-memory buffer. The same history must support live charts, timeline browsing, and future longer-range queries without introducing write contention or making the browser-facing server responsible for database lifecycle details.

The storage owner also needs to handle migrations, SQLite pragmas, retention, rollups, and query shape in one place.

## Decision

Use two local Bun processes:

- `collector-writer` owns host data collection, SQLite writes, SQLite reads, migrations, retention, and rollups.
- `dashboard` serves the UI and talks to the writer process over a local read API. It does not open the SQLite database directly.

Use one main raw `metric_samples` table keyed by `captured_at_ms`, with child tables for filesystem/process/pressure detail and rollup tables for longer ranges. Do not start with daily tables.

The detailed schema and migration path are documented in [../sqlite-history-architecture.md](../sqlite-history-architecture.md).

## Alternatives Rejected

### Dashboard Opens SQLite Directly

This keeps the process count smaller, but splits collection, reads, writes, migrations, and WAL/checkpoint behavior across the UI-serving process. It also makes future background collection harder because browser traffic and storage ownership are coupled.

### Both Processes Open SQLite

SQLite can handle multiple readers, but the application still needs one owner for migrations, writes, retention, and operational policy. Allowing both processes to open the database invites accidental write paths and harder-to-debug lock behavior.

### Daily Tables

Daily tables make dropping old data easy, but they complicate every query that crosses midnight and add schema-management overhead. The dashboard's primary access pattern is timestamp-range reads over recent samples, which a single indexed table handles well. Retention plus rollups are the simpler scaling path.

## Consequences

- The writer process becomes the source of truth for current and historical data.
- The dashboard can refresh by requesting the last configured time window from the writer.
- Browser display preferences remain in `localStorage`; persisted host samples remain in SQLite.
- The project needs supervision for two Bun processes instead of one when persistence is implemented.
