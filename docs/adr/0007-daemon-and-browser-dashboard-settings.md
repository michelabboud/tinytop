# 0007 - Daemon And Browser Dashboard Settings

Date: 2026-06-26

## Status

Accepted

## Context

TinyTop now has two kinds of dashboard settings:

- Preferences that should follow only one browser tab or browser profile.
- Defaults that should apply to every browser connected to the same local daemon.

Mixing those scopes makes the dashboard surprising. A theme chosen in one browser should not immediately override another operator's browser, while daemon defaults such as refresh interval, default history window, retained history horizon, thresholds, and enabled sections should be stored with the daemon.

## Decision

Browser-local preferences stay in `localStorage`:

- active theme override
- active graph mode override
- selected history range

Daemon-wide defaults are stored in SQLite by the Rust collector/dashboard daemon in:

```sql
CREATE TABLE IF NOT EXISTS app_settings (
  setting_key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
```

The first settings row uses `setting_key = 'dashboard'` and stores typed JSON for:

- `defaultTheme`
- `defaultGraphMode`
- `pollIntervalMs`
- `defaultHistoryWindow`
- `retentionHours`
- `rollupRetentionDays`
- `topProcessCount`
- `redactionDefault`
- `thresholds`
- `enabledSections`

The Rust daemon exposes:

- `GET /api/settings`
- `PUT /api/settings`

The dashboard renders a Settings panel with `This Browser` and `This Daemon` groups. Browser controls apply immediately and persist only in localStorage. Daemon controls save through `/api/settings` and become defaults for future dashboard loads when the browser has no local override.

The legacy Bun dashboard fallback exposes the same settings shape in memory so the UI remains usable, but durable SQLite-backed settings are owned by the Rust daemon.

## Consequences

- The settings UI can be implemented without introducing Svelte or another frontend framework.
- Rust remains the source of truth for durable daemon defaults.
- The dashboard can use daemon `pollIntervalMs` as its browser refresh default, but live daemon collection interval changes are still a future follow-up.
- Retention and rollup settings are saved now; automatic pruning and rollup enforcement are planned in the next storage slice.
