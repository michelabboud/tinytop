# ADR 0016 — A versioned, secret-free configuration document for export and import

**Status:** Proposed (2026-08-28) — awaiting Michel's go.

## Context

Michel: *"we should be able to export + import configurations."* Settings live in `app_settings` as one JSON document (`DashboardSettings`), edited through the dashboard and `PUT /api/settings`. With the ladder, archive, disk-check and OTel blocks, the document is large enough that recreating it by hand on a second machine is error-prone.

## Decision

- Export = `GET /api/settings/export` (and `tinytop-agent config export`): `{"tinytopConfigVersion": 1, "exportedAtMs", "agentVersion", "settings": <DashboardSettings>}`. The envelope is versioned independently of the agent.
- Import = `POST /api/settings/import` (and `config import FILE`), with a **dry-run mode** that returns validation errors, changed keys, and the rows/buckets that a horizon shrink would delete — computed by the server — so the dashboard's confirmation shows real numbers. A real import applies through the same `put_settings` + `maintain_history` path as the settings dialog and records a `settingsChange` marker with `source: "import"`.
- Validation on import is exactly the settings validation (ranges, minimums, monotonic ladder, disk-pressure growth refusal); unknown top-level keys and unsupported `tinytopConfigVersion` are refused with a message naming the maximum supported version.
- **The document never contains secrets** — enforced by construction: OTel headers are referenced by environment-variable name (ADR 0015), and no setting may hold a credential.

## Alternatives rejected

- **Exporting the raw `app_settings` row.** No version envelope, no agent version, no dry-run; a future schema change would silently import stale shapes.
- **A TOML/YAML config file as the source of truth.** Would create a second authority beside the database; the dialog and API already own the settings.

## Consequences

- Machine-to-machine settings transfer becomes a download and an upload; migrations of the document are explicit (`tinytopConfigVersion`).
- The dashboard's shrink confirmation and the import dry-run share one server-side computation.
