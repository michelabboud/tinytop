# 0034 - The save path refuses a form it cannot read, and the retention ladder is a list

## Status

**Accepted (2026-09-02; 0.10.0)** — Michel, on the ADR 0033 dialog: *"FIX this ---> a sub-tab that
unmounts silently drops settings on save"* and *"make nicer designs here"*. Hardens **ADR 0033**
decision 2 from a convention into a checked precondition, and replaces its decision 6 layout.
Anchors verified at main `a3c9636` (v0.9.0, deployed).

## Context

### The silent drop

`collectDaemonSettingsFromForm` reads its 48 controls through the id-based `elements` cache, taken
once at load. Two things then fail without a sound:

- A **detached** input still answers `.value` — with whatever it held when it left the document.
  Demonstrated in a browser: after `panel.remove()`, `input.isConnected === false` and
  `input.value === "7"`.
- A **missing** input reads as `undefined`, and `numberControlValue(control, fallback)` substitutes
  its fallback.

Either way the save serialises a value the user is not looking at and `PUT`s it. There is no
exception, no failed request, no console warning — the first symptom is someone noticing later that
their settings are wrong, with nothing to trace it to.

ADR 0033 addressed this by ruling that a sub-panel is hidden and never unmounted, and asserting it
with a test that greps `app.js`. **That is a convention plus a source-shape check, and neither is a
guarantee.** Unmounting an inactive panel is the *obvious* thing to do to a tab component — a future
change made for perfectly good reasons ("don't render hidden panels") reintroduces the defect, and
the grep test only catches the exact spelling it knows about. The failure mode is silent data
corruption, which is the category that least deserves to rest on a convention.

### The ladder still did not look like a ladder

ADR 0033 moved History's Tiers group from `auto-fit` to two fixed columns so each tier's switch would
pair with its own days field. That fixed the pairing and left the real problem: eight boxes in a
grid, with nothing to say that L1–L4 are one ladder getting coarser and longer, and no visual
difference between "raw samples, most expensive" and "one per hour, cheapest". Michel, looking at the
result: *"make nicer designs here"*.

## Decision

### 1. The save path checks that every control it depends on is readable, and refuses if not

A manifest names every control `collectDaemonSettingsFromForm` reads, with the label the user sees,
gated by the capability that makes it exist (`retentionLadderAvailable`, `otelAvailable`,
`metricsAvailable`, `thermalAvailable`) so an absent runtime feature is never reported as a broken
form. `brokenSettingsControls` classifies each as `missing` or `detached`;
`settingsIntegrityErrors` renders one message naming them.

**A node counts as readable only when `isConnected === true`** — not merely when it is non-null,
because "still referenced, no longer in the page" is the entire failure being caught.

The check runs **first in `validateDaemonSettings`, and returns before any value is judged.** A range
check on a detached input *passes* — it is checking the stale value — so continuing would produce a
clean validation of data that did not come from the form.

The message names the unreadable settings, says nothing was saved, and says to reload. A bare "save
failed" would leave someone retrying into the same silence.

### 2. The manifest cannot drift, because a test compares it to the function it guards

A hand-written manifest that falls behind the code is a hole in the guard, in exactly the silent
style the guard exists to remove. `tests/dashboard-settings-integrity.test.ts` extracts the
`elements.*` set from `collectDaemonSettingsFromForm` **and** from `daemonSettingsControlManifest`
and fails on any difference in either direction. Drift is a red test, never an invisible gap.

Both mutation classes were verified to fail the comparison before this shipped: adding a read without
a manifest entry, and deleting a manifest entry.

### 3. The retention ladder is a list of tiers

Four rows, each `badge · name and resolution · on-off state · span`, all sharing **explicit** track
sizes — not `auto`, which sizes per row and would stagger the columns. A tier's controls are on its
own row by construction, the columns can be read straight down, and the ladder reads as a ladder. A
switched-off tier's badge and name recede; its switch keeps full contrast so it can be turned back
on, and the authority on what is editable remains the `disabled` attribute `syncLadderControlStates`
already sets.

Row padding and gap are **tuned to measurement, not taste**: at `0.4rem`/`0.3rem` the panel with its
help paragraph expanded overflowed a 720 px window by 9 px.

## Alternatives rejected

1. **Leave it as a convention plus the ADR 0033 grep test.** The status quo, and what Michel
   objected to. The test asserts the shape of today's code, not the property.
2. **Make `collectDaemonSettingsFromForm` re-query the DOM instead of using the cache.** Does not
   fix it: a genuinely removed control then reads `null` and takes the fallback default, which is
   the *same* silent substitution wearing different clothes.
3. **Throw from `collectDaemonSettingsFromForm` when a control is unreadable.** That function also
   feeds the effective-settings readout and the dirty check, which run on every keystroke; throwing
   there turns a save-path problem into a dead dashboard. The refusal belongs at the save boundary.
4. **Instrument every read so the manifest is generated rather than written.** Drift-proof by
   construction and the theoretically cleaner answer, but it means threading a name through ~48 call
   sites in a function that is already the highest-traffic code in the file. The manifest plus a
   comparison test buys the same guarantee — drift is impossible to ship — for a fraction of the
   churn. Revisit if that function is ever rewritten.
5. **Warn but save anyway.** The entire defect is that a save proceeds with data the form is not
   showing. A warning next to a completed corruption is not a fix.
6. **Keep ADR 0033's two-column tier grid.** It fixed the pairing and not the reading; it was the
   layout Michel was looking at when he asked for a nicer design.

## Consequences

- **A save can now be refused, which it never could before.** The trigger is a form that cannot be
  read — a state that should never occur in shipped code, and whose only correct handling is to stop.
  Proven end to end in a browser: with the Archive sub-panel removed from the document, the save
  produced **zero** `PUT`s and named all four unreadable controls; the control case saved normally.
- **Adding a setting now costs one manifest line.** Forgetting it is a red test naming the control,
  which is the point.
- Metric selection is guarded through its container, because `querySelectorAll` works perfectly well
  on a **detached subtree** and would return a plausible, wrong disabled-metrics set.
- The guard is a client-side precondition, not a security control. The daemon remains the authority
  on what a valid settings document is.
- ADR 0033 is **not edited**. Its decision 2 stands and is now enforced rather than trusted; its
  decision 6 layout is superseded here.

## Addendum (2026-09-02, 0.10.1) — append-only; nothing above is rewritten

Michel then asked for the raw document editor to use the space it had: *"in advanced you can give the
document more space, since the window is larger"*, *"just lets it expand close to the bottom
border"*, *"click on validate and see that the notification change the size"*, *"add scroll"*.
Chasing that exposed **two defects, one of them mine from ADR 0033**.

**1. `#settings-panel-advanced` was never closed.** Wrapping its two fieldsets in sub-panels (ADR
0033) added an opening `<div>` without its matching close, so the browser's error recovery quietly
re-parented everything that followed: `#settings-panel-thermals` became a child of the Advanced
panel, `#settings-validation-summary` moved inside `.settings-grid`, and
`.settings-dialog-actions` moved inside the scrolling `.settings-dialog-body`.

Two user-visible consequences, both shipped in 0.9.0 and 0.10.0:

- **The Thermals tab rendered an empty dialog.** Its panel inherited `[hidden]` from the Advanced
  panel, so selecting Thermals showed nothing whenever Advanced was not also selected.
- **`.settings-card`'s three-row grid lost its third row.** With the actions inside the body rather
  than beside it, the card's rows measured `102px / 716px / 0px` and Save, Cancel, Reset and
  Defaults were laid out **below the dialog's bottom edge**, unclickable, whenever a panel was tall.

A parser would have repaired this silently — which is how it survived a release. The new
`tests/dashboard-dialog-structure.test.ts` reads `<div>` **depth in the source** instead: all five
tab panels must share one depth, the actions must be a sibling of the body, and the tags must
balance. Verified against the shipped markup, where it reports Thermals at depth 4 against every
other panel's 3, and the actions at depth 2 against the body's 1.

**2. `.settings-dialog-body` had `min-width: 0` but not `min-height: 0`.** It is the `minmax(0, 1fr)`
row of the card, and a grid item's default `min-height: auto` refuses to shrink below its content —
so even correctly nested, a tall panel would squeeze the actions row. This is also why showing the
validation summary appeared to resize the dialog. With it set, the body is the one scrolling region,
the dialog height and the action row's position are unchanged by the summary (measured: card 818 px
and actions top 950 px, identical before and after Validate), and the body simply scrolls 66 px.

**3. The editor's height is derived from the dialog, not the viewport.** ADR 0029 caps the dialog at
`min(820px, 100dvh - 2rem)`, so on a tall window `100dvh` says nothing about the room available — a
first attempt sized against it produced an 880 px editor inside a 716 px box. Sized against the
dialog's own expression minus measured chrome, the editor renders 396 px where a binary search over
the real layout puts the no-scroll maximum at 397 px.
