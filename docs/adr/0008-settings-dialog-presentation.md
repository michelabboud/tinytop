# 0008 - Present Dashboard Settings As A Dialog

Date: 2026-06-26

## Status

Accepted

## Context

ADR 0007 split dashboard settings into browser-local preferences and SQLite-backed daemon defaults. The first implementation placed those controls inline in the main dashboard flow. That made a dense operational dashboard feel heavier and pushed primary metric content down the page.

Settings are an occasional control surface, not live telemetry. They should be easy to reach without competing with the Overview, History, Filesystem, Pressure, and Process views.

## Decision

Settings are presented in an accessible modal `<dialog>` opened from a Settings button in the left rail.

The dialog keeps the same settings split:

- `This Browser` for localStorage-backed active theme, graph mode, and history window.
- `This Daemon` for SQLite-backed daemon defaults saved through `/api/settings`.

The rail Settings item is a button rather than an anchor because it opens an overlay instead of navigating to an in-page section. The dialog supports explicit close/cancel controls, Escape close, backdrop close, and focus return to the Settings button.

## Consequences

- The main dashboard flow returns to metrics-first scanning.
- No frontend router or separate HTML page is needed, preserving the static embedded dashboard model.
- Settings remain available from every viewport without adding another dashboard section.
- Tests now enforce that settings are dialog-backed and that the old inline settings section does not return.
