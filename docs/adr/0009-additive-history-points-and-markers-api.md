# 0009 Additive History Points And Markers API

## Context

The dashboard needs longer timeline ranges than raw snapshot hydration should carry. Raw `/api/history` returns full `SystemSnapshot` JSON so selected raw samples can update gauges, filesystem, pressure, and process panels. One-minute rollups already exist in SQLite, but exposing them by changing `/api/history` would risk breaking existing raw-history callers and selected-sample behavior.

The dashboard also needs trustworthy timeline annotations for daemon restarts, settings changes, stale periods, and coverage gaps.

## Decision

Keep `/api/history` as the raw snapshot API and add Rust-daemon-only endpoints:

- `GET /api/history/points` for chart-ready raw or rollup points
- `GET /api/history/markers` for persisted daemon events and computed coverage gaps

Persist daemon-start and settings-change markers in `app_events`. Infer coverage-gap markers from raw sample spacing. Keep coverage metadata in `/api/history/coverage`, expanded with DB budget fields.

## Alternatives Rejected

- Change `/api/history` to return rollups for long windows. Rejected because it would overload the raw snapshot contract and complicate selected-sample detail rendering.
- Store only browser-side markers. Rejected because daemon restarts and settings changes need durable provenance across browser reloads.
- Add multiple rollup tables now. Rejected because one-minute rollups are enough for the first 6h/24h/7d/30d browsing slice.

## Consequences

The API surface is additive and backward compatible. The dashboard can choose raw or rollup data by range without faking full snapshots for aggregate points. Future rollup tiers can be added behind the same points endpoint.

## Status

Accepted.
