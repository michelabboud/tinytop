# 0029 - The settings dialog is a fixed box, and a backdrop dismiss needs a backdrop press

## Status

**Accepted (2026-09-01; 0.7.1)** — Michel's instruction after hitting the bug in 0.7.0, verbatim: *"in settings, when I click on history Tab (if i am at different tab) settings closes"*, then *"so please try to make the setting popup window a fixed size"* / *"fixed size for all tabs"* / *"make sure all fits / if not redesign the specific Tab"*. Extends ADR 0027 (the tabbed dialog) and ADR 0028 (its controls).

## Context

In 0.7.0 the dialog was sized by its content: `width: min(980px, …)` but only a `max-height`. Selecting a shorter tab therefore shrank it. Michel found the consequence immediately, and diagnosed it himself: *"it has something to do with the fact that the window change sizes and suddenly under the mouse is NOT setting windows so it closes itself."* That is exactly right, and the mechanism is a three-part race:

1. Each tab carries a **`focus` listener** (`app.js`) that calls `selectSettingsTab`. Focus lands on **mousedown**, so the panel swaps and the dialog resizes *before mouseup*.
2. A `<dialog>` is centred, so shrinking moves its **top edge down**, out from under the pointer.
3. The dismiss handler was `if (event.target === settingsDialog) …`. A `click` is dispatched to the **nearest common ancestor of the mousedown and mouseup targets**; with mousedown on the tab (inside) and mouseup on the backdrop (outside), that ancestor is the dialog itself. So the dialog dismissed itself mid-click.

The same handler had a second, quieter failure: press inside the Advanced document editor, drag to select, release past the edge — the dialog closed and the edit was gone.

Measured at `58b60b9` in a real browser (980px-wide dialog, 650px of body): general 672, history 372, metrics 884, thermals 185, advanced 783. Three of five tabs overflowed a fixed 820px box, so "fixed size" could not simply be imposed without redesigning them.

## Decision

**1. One fixed box for every tab.** `height: min(820px, calc(100dvh - 2rem))` on `.settings-dialog` and `height: 100%` on `.settings-card`, whose middle grid row is `minmax(0, 1fr)`. The dialog's outer size is now independent of which tab is selected, so no tab switch can move the box under the pointer. It also stops the tab row jumping as you move between panels.

**2. A backdrop dismiss requires the gesture to have STARTED on the backdrop.** A `pointerdown` listener records whether the press landed on the dialog element itself; the `click` handler dismisses only if that is true *and* the click target is the dialog. Fixing the size removes today's trigger; this removes the whole class, including the drag-to-select-and-release-outside case that discarded edits.

**3. Every tab fits the box without scrolling** (`.settings-dialog-body` keeps `overflow: auto` purely as a small-viewport fallback). Where a tab did not fit, the tab was redesigned rather than the box enlarged:

- **Metrics** — families now sit **side by side** (`repeat(auto-fit, minmax(min(100%, 420px), 1fr))`) with each family listing its metrics in one column. 884 → 621.
- **Advanced** — two columns: the OpenTelemetry fields one-per-row on the left, the raw settings document on the right where a JSON editor wants height, its editor flexing to fill and its actions beneath. 783 → 447.
- **General** — no redesign needed; the shared group rhythm was tightened (`gap` 0.8→0.6rem, padding 0.9→0.8rem, label gap 0.4→0.3rem). 672 → 629.

Measured after: general 629, history 353, metrics 621, thermals 174, advanced 447, against 650 of body — all fit, and the dialog reports a single height (820) across every tab.

**4. Panel-scoped selectors where a bare class would lose.** `.settings-group` appears *after* `.metric-family` and `.advanced-document-settings-group` in the file at equal specificity, so those bare selectors were silently dead — `.metric-family`'s single-column rule never applied, and the document group's `display: flex` lost to `display: grid`, which laid the editor out *beside* its buttons. Both are now scoped (`.metrics-settings-groups .metric-family`, `#settings-panel-advanced .advanced-document-settings-group`) and carry a comment saying why.

## Alternatives rejected

1. **Only fix the size, leave the dismiss handler.** Removes today's trigger and leaves the class: any future height change inside a tab (a long validation message, a taller unknown-metrics list) re-arms it, and the drag-to-select data loss stays.
2. **Only fix the dismiss handler, keep content sizing.** Stops the accidental close but keeps the dialog and tab row jumping on every tab switch, which Michel explicitly asked to end.
3. **Drop the tab `focus` listener** so tabs only change on click. It is the ARIA-correct automatic-activation behaviour for a tablist and serves keyboard users; removing it to dodge a layout race would trade an accessibility affordance for a CSS problem.
4. **Let tall tabs scroll inside the fixed box.** Simplest, and rejected on Michel's explicit instruction to make everything fit — scrolling a settings panel hides controls that the tab exists to expose.

## Consequences

- No tab switch can resize the dialog, so the mid-click dismiss cannot recur; a regression test pins the fixed height and the absence of the old `max-height`.
- Short tabs (Thermals on a sensorless host, History) now show empty space below their content. That is the accepted cost of one stable box.
- Every tab must be *checked against the box* when it gains controls. General has ~21px of headroom and Metrics ~29px, so the next widget added to either needs a measurement, not a guess. Below ~820px of viewport the dialog shrinks with `100dvh` and the body scrolls — the fallback is deliberate, not an invariant.
- A future contributor adding a rule for `.metric-family` or `.advanced-document-settings-group` must scope it past `.settings-group` or it will silently do nothing.
