# 0027 - Tabbed settings, and per-metric OTel export selection stored as a DISABLED set

## Status

**Accepted (2026-08-30; T18/T18b)** — Michel's go, 2026-08-30 13:0xZ. His words, verbatim: *"why not just make it dynamic, fool proof, we can still have advanced mode… select all the metrics you want to send, click click click, apply, enable otel"* and *"the setting UI should be TABBED by Category so the page will not be very long"*. Extends ADR 0015 (the OTel export), ADR 0016 (the configuration document) and ADR 0026 (the thermals settings block, whose group becomes a tab).

## Context

The settings page has grown one `fieldset` per feature — display, thresholds, retention ladder, archive, OTel, and now thermals (ADR 0026) — on a single scrolling column. Two problems, both Michel's, both correct:

1. **The page is too long**, and every new feature makes it longer. Groups are ordered by the history of the code, not by what a user is looking for.
2. **The OTel group exposes OTLP's vocabulary instead of the user's intent.** Endpoint, protocol, headers env var and resource attributes are all questions the *exporter* asks. The question the *user* has — "which metrics do you send?" — has no control at all, because the answer was hardcoded to "all of them" when ADR 0015 scoped the feature as push-only export.

**Measured at `90113b0`:** the daemon registers **thirteen** instruments in `Instruments::new` (`agent/src/otel.rs:263-318`), each an inline string literal: ten OTel semantic-convention names (`system.cpu.utilization`, `system.cpu.load_average.1m/5m/15m`, `system.memory.utilization/usage/limit`, `system.paging.utilization`, `system.filesystem.utilization/usage`) and three tinytop-specific ones (`tinytop.load.percent`, `tinytop.pressure.some`, `tinytop.pressure.full`). Three resource keys are fixed and un-overridable (`otel.rs:26`, `:377-381`). There is no registry: the names exist only as literals at their construction sites, so nothing else in the process — and certainly not the dashboard — can enumerate them.

On this box the export is thirteen metrics against 27 filesystems, i.e. of the order of forty active series from one host. Hosted metrics backends price per series, so selection is a cost lever, not only tidiness.

## Decision

1. **The settings page becomes tabbed: General · History · Metrics · Thermals · Advanced.** One `role="tablist"` with roving `tabindex` and `aria-controls`/`aria-selected`, arrow-key navigation, and each panel a `role="tabpanel"`. **Not a drawer or an accordion** — Michel dislikes drawers, and a tab keeps the selected content at a fixed position instead of pushing the page around. The existing groups move into tabs unchanged; the ADR 0026 thermals group becomes the Thermals tab. The selected tab is remembered in `localStorage` (one key, wrapped in try/catch, defaulting to General) so that saving does not bounce the user back to the first tab. **Absent thermals support hides the Thermals tab entirely**, on the Bun runtime always — a tab that opens onto nothing is worse than no tab.

2. **Metric selection is stored as the set of DISABLED names — `otel.disabledMetrics: Vec<String>`, default empty — never as the set of enabled ones.** The direction matters and is the whole point of this decision: with an enabled list, a metric added in a later release (thermals, GPU) is **absent from every stored configuration and therefore silently off** on every upgraded host — the shadow-state failure, where something you built never runs and nobody notices. With a disabled list, an absent name means enabled, so new metrics ship on by default and only an explicit opt-out persists. *Rejected:* `metrics: { name: bool }` (same forward-compatibility hole as the enabled list, plus it stores the default for every name); a per-family flag (families are a UI grouping, not a contract — a family's membership changes when a metric is added).

3. **An unknown metric name in `disabledMetrics` is ACCEPTED, PRESERVED and shown as inert — never rejected.** Michel runs one configuration across four boxes that will not all be on the same release; a document written by a newer tinytop must survive a round-trip through an older one without losing the user's choices. An unknown name disables nothing (there is no such instrument) and is displayed in the Metrics tab under "unknown on this version (inert)" so it is visible rather than mysterious. Validation still bounds it: at most 64 entries, each 1–128 characters matching `^[a-z][a-z0-9._]*$`, no duplicates. *Rejected:* refusing the document (breaks fleet portability for a typo class the picker cannot produce); silently dropping unknown names (loses the user's choice on the next export — a config that quietly edits itself).

4. **The metric list becomes one registry in the daemon and is served to the dashboard; the UI never hardcodes it.** `pub const METRIC_REGISTRY: [MetricDescriptor; 13]` in `tinytop-agent`, where `MetricDescriptor { name, unit, family, description, semantic_convention: bool }`, and `Instruments::new` builds every instrument **from the registry** rather than from inline literals — so the exported set and the advertised set cannot drift. New route `GET /api/otel/metrics` returns the registry plus, for each entry, whether it is currently disabled. The Metrics tab renders from that response. *Rejected:* a hardcoded copy of the list in `app.js` (rule 3, and it is exactly the drift that makes a picker lie); deriving the list by reflection over the SDK (no such API, and it would report instruments only after first use).

5. **The Advanced tab carries the raw configuration document with validate-before-apply, reusing the machinery that already exists.** ADR 0016's export/import document is the "otel file"; `plan_import` already validates server-side, returns a list of errors, and refuses rather than applying partially. The tab adds only the UI: a textarea seeded from `GET /api/settings/export`, a **Validate** button that calls the existing import dry-run and renders the error list (or the `changed_keys` it would apply), and an **Apply** that is disabled until a validation of the current text has succeeded. **The client never re-implements validation** — it displays the server's verdict, so the two can never disagree. Endpoint, protocol, headers env var and resource attributes stay on this tab: they are expert controls and belong behind the same door as the raw document.

6. **A disabled metric is not recorded, rather than recorded and filtered.** In `collect_and_export`, an instrument whose name is in the disabled set is skipped at record time; an unrecorded gauge simply does not appear in the OTLP request, with no empty metric and no zero-valued data point. *Rejected:* building the payload and stripping metrics afterwards (pays the collection cost for data thrown away, and an empty `ResourceMetrics` is a shape some backends handle badly).

7. **Two lanes, disjoint files, one tag — the T15/T15b shape.** `T18` (registry, setting, route, exporter, docs) and `T18b` (the tabbed shell, the Metrics tab, the Advanced tab, absorbing the existing groups), both based on the commit that merges T17 and T17b, because T18b rewrites the same three dashboard files T17b already changed.

## Alternatives rejected (summary)

- Leaving the page as one long column and only adding the picker — the length is half of Michel's complaint, and each future feature makes it worse.
- An accordion or drawer per category — drawers hide state, and Michel has ruled against them.
- Selection by wildcard/prefix (`system.filesystem.*`) — a stored pattern silently changes meaning when a matching metric is added later; the disabled-set decision exists precisely to make additions predictable. A family "select all" stays a UI affordance that expands to explicit names at save time.
- Per-metric interval or per-metric attribute filtering — out of scope; interval stays global (ADR 0015).

## Consequences

- `otel.disabledMetrics` is additive to the settings document; an absent key keeps the persisted value (the ADR 0015 absent-key rule), and `changed_keys` reports `otel` as it already does.
- `GET /api/otel/metrics` is a new additive route; the Bun runtime does not serve it, and the Metrics tab is hidden there exactly as the OTel group already is.
- The exported series count becomes user-controlled: turning off `system.filesystem.*` on this box removes of the order of half the series, which is the cost lever the decision exists for.
- Enabling a metric later is instant and requires no migration: the name simply leaves the disabled set.
- The registry gives the OTel documentation a single source — README and `docs/guides/API.md` list the thirteen names from it rather than restating them, and a metric added in a later task appears in the picker with no dashboard change.
