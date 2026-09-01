# 0033 - A settings panel with too much content declares sub-groups and shows a secondary tab row

## Status

**Accepted (2026-09-01; 0.9.0)** — Michel's instructions after using the deployed 0.8.1 dialog:
*"please redesign history to look better"*, *"in general add a secondary row of Tabs by dividing data
into groups (that already exists) so we do not scroll"*, *"same thing with Metrics"*, and *"add (?)
sign for help in sections/groups"*. Extends **ADR 0027** (the primary tablist) and amends two
narrow clauses of **ADR 0029**. Anchors verified at main `3761eb5` (v0.8.2, deployed).

## Context

Three separate problems, one shape.

**1. History was one flat fieldset holding fourteen controls of three different kinds.** L1 days, L2
days, an L3 switch, L3 days, an L4 switch, L4 days, a keep-forever switch, a filesystem interval, a
per-tick retention, four archive controls and two disk-guard controls all sat in a single
`repeat(auto-fit, minmax(170px, 1fr))` grid. Because `auto-fit` wraps by rendered width, a switch for
one tier routinely landed between two *other* tiers' number fields. There was no rhythm to follow,
which is the substance of "looks bad".

**2. General overflowed.** Its "This Daemon" fieldset carried 24 controls — four defaults, five
daemon numbers, ten thresholds and six display switches — and ADR 0029's fixed
`height: min(820px, calc(100dvh - 2rem))` resolves to about 687 px on Michel's window, not the 820 px
the ADR 0029 work was measured against. So the body scrolled: ADR 0029's documented small-viewport
fallback, working as designed, on a panel with too much in it. **The earlier "every tab fits without
scrolling" claim was true at the size it was measured and false at his.**

**3. Nothing in the dialog explained what a setting costs.** The reasoning for the ladder tiers, the
cold archive and the minimum-free-space guard is written down in ADRs 0013–0021 and had never reached
the UI.

The grouping his instruction asks for **already exists** in two of the three cases: Metrics is grouped
by `METRIC_REGISTRY.family` (cpu 4, memory 3, filesystem 2, pressure 2, load 1, swap 1 = 13) and
Advanced by its two fieldsets. History's groups did not exist yet, which is exactly why its regroup
and its tab row are one piece of work.

**The constraint that makes this dangerous.** ADR 0027 keeps every primary panel permanently mounted
and toggles it with `[hidden]` alone, because `collectDaemonSettingsFromForm` reads its inputs through
the id-based `elements` cache rather than by walking the visible panel. A detached input still answers
`.value` — with whatever it held when it left the document. **A sub-tab that unmounted its panel would
therefore keep saving, silently, and keep saving stale data.** No error, no exception, no failed
request: just a settings document that quietly disagrees with the form.

## Decision

**1. A panel that declares sub-groups renders a secondary `role="tablist"` inside itself.** The rows
are separate keyboard scopes: arrows move within the row holding focus and never jump between rows,
each row has its own roving `tabindex`, and each row carries its own `aria-label`.

| panel | secondary tabs | source of the grouping |
|---|---|---|
| **General** | Browser · Daemon · Thresholds · Display | Browser existed; the Daemon fieldset splits into three by *kind of thing* |
| **History** | Tiers · Archive · Disk | **new grouping — this is the redesign** |
| **Metrics** | CPU · Memory · Swap · Filesystem · Load · Pressure | `METRIC_REGISTRY.family`, built at runtime |
| **Advanced** | Export · Document | its two existing fieldsets |
| **Thermals** | *(none — it fits)* | one short group |

**2. A sub-panel is HIDDEN, never unmounted** — the ADR 0027 rule, restated here because a nested
tablist is exactly where someone would reach for `replaceChildren`. Verified end to end, not merely
asserted: four fields were edited across three different sub-tabs, the dialog was parked on a
sub-tab holding none of them, and the captured `PUT /api/settings` body carried all four values.

**3. The secondary selection is remembered per parent tab**, in one `tinytop.settingsSubTabs` object
rather than a key per row — the Metrics row's members come from the daemon and are not known in
advance. A remembered name that no longer exists falls back to its own row's first member.

**4. Row movement gets its own rule function.** `moveSettingsTab` falls back to the literal
`"general"`, which is a *primary* tab name and meaningless inside a secondary row; `moveWithinTabRow`
and `resolveTabInRow` fall back to the row's first member and return `null` for an empty row (the
Metrics row does not exist until the registry has been fetched).

**5. Help is a real `<button>` per group legend, toggling real text in the DOM** — `aria-expanded` +
`aria-controls`, collapsed with `hidden`. Not a `title` attribute, which is invisible to touch and
inconsistently announced, and not a bare icon, which has no accessible name. **Density is one
paragraph per group, not per field**: per field turns the dialog into a manual. Each paragraph says
what the setting changes *and what it costs* — the part that is not obvious from the label.

**Eight groups get help; four deliberately do not.** OpenTelemetry export, the raw settings document
and CPU thermals already carry permanently-visible notes that are warnings rather than explanations,
and Browser's three fields are self-evident. Adding a `(?)` beside an existing visible note would be
filler.

**6. History's Tiers group uses TWO FIXED COLUMNS, not `auto-fit`.** This is the actual fix for
"looks bad": each tier's enable switch lands in the left cell with its own days field beside it, by
construction rather than by rendered width. The selector is compound
(`.history-ladder-settings-group.history-tier-group`) so it out-ranks the `auto-fit` rule regardless
of source order, and it is restated inside the narrow-screen media query, which would otherwise lose
to it.

**7. ADR 0029's one-column OpenTelemetry group is superseded.** That rule existed because the group
shared the Advanced panel with the raw document editor and had only half the width. With Advanced
split into sub-tabs the group owns the full width, and one field per row overflowed the short viewport
by 43 px (measured). It now lays out in columns, with the wide fields spanning and the cells
bottom-aligned so a two-line label does not push its input below its neighbours'.

## Alternatives rejected

1. **Make the dialog scroll and accept it.** The instruction was explicitly *"so we do not scroll"*,
   and a scrolling settings dialog hides controls behind a gesture with no affordance.
2. **Make the dialog taller or resizable.** It cannot exceed the viewport, which is the binding
   constraint on a 720 px window; ADR 0029 fixed the height for a reason that still holds (a
   content-sized dialog resized between mousedown and mouseup and dismissed itself).
3. **A `<details>`/accordion per group instead of tabs.** Cheaper, and it reintroduces scrolling as
   soon as two groups are open. It also would not match the primary row's interaction model.
4. **Unmount the inactive sub-panel** (the obvious way to build a tab). Rejected on the silent
   data-loss path in Context; this is the one alternative that would have shipped a defect with no
   symptom until someone noticed their settings were wrong.
5. **Reuse `moveSettingsTab` for both rows.** Its `"general"` fallback would select a primary tab
   name inside a secondary row — reachable whenever a remembered sub-tab no longer exists.
6. **`title` tooltips for help.** Invisible on touch, inconsistently announced by screen readers, and
   impossible to style or to keep on screen while reading a long sentence.
7. **Help text per field.** More thorough and worse: it turns a settings dialog into a manual and
   would reintroduce the scrolling this ADR exists to remove.

## Consequences

- **Every settings panel now fits without scrolling at a 720 px window** — measured across all 16
  tab/sub-tab stops, including with a help paragraph expanded: overflow 0 everywhere. Before this
  change, General and Advanced overflowed at that height.
- **Two tablists means two keyboard scopes on one screen.** The arrow handler is scoped by
  `closest("[data-settings-subtab]")` and returns *before* `preventDefault` when focus is not on a
  sub-tab, so the primary row keeps its own handler. Verified: arrows in History's row wrap without
  moving the primary tab; an arrow on the primary row moves primary and leaves History's sub-selection
  untouched; returning to History lands on the sub-tab last used.
- **A setting is now two clicks away instead of one**, for the panels that gained a row. That is the
  cost of not scrolling, and it is deliberate — a control behind a labelled tab is discoverable, a
  control below the fold is not.
- **Metric family names now reach a DOM `id`.** They arrive from the daemon's registry, so they are
  data: `metricFamilyKeys` folds them to `[a-z0-9-]` and suffixes duplicates, because two families
  folding to one id would point two tabs at one panel and leave the other unreachable, silently.
- **A future contributor adding a sub-panel class that sets `display` must restate `[hidden]`.**
  Setting `display` defeats the `hidden` attribute's own `display: none`, and these panels are hidden
  by attribute only. This is the same family of failure as the competing selectors in ADR 0032 and is
  now asserted by test.
- ADRs 0027 and 0029 are **not edited**. ADR 0027's single-tablist clause and ADR 0029's one-column
  OTel rule are superseded here; both ADRs' central decisions stand.
