# 0028 - Settings booleans are restyled native checkboxes, and a switch means an on/off setting

## Status

**Accepted (2026-09-01; 0.7.0)** — Michel's instruction, verbatim: *"fix the setting UIUX use toggle instead of checkboxes, make elements align, use modern UIUX/CSS"*. Extends ADR 0027 (the tabbed settings dialog whose controls this governs).

## Context

ADR 0027 landed the tabbed settings dialog and the per-metric picker. It settled *what* the controls are and left *how a boolean looks* to whatever the browser draws. Three things followed from that:

1. **Every boolean was a raw native checkbox.** A checkbox is the browser's answer to "is this member of a set selected?", but every boolean in this dialog is an on/off *setting* — enable OTel, enable thermals, keep L4 forever, export this metric. The affordance disagreed with the meaning.
2. **The controls did not line up.** `.metric-family-toggle` was positioned with `float: right`, which takes the button out of flow, so the family's bulk control never aligned with its legend text. The metric row nudged its checkbox with a magic `margin-top: 0.2rem` to fake alignment against the first text line. And `.metric-setting-row > span` was a grid, so a metric's *unit* was laid out as its own row — `system.cpu.utilization`, then `1` on the next line, then the description.
3. **Two populations of control.** Thirteen booleans are static markup in `index.html`; the metric picker builds its checkboxes at runtime in `renderMetricRegistry`. Any presentation decision has to cover both, forever.

## Decision

**A boolean in the settings dialog is presented as a switch, implemented by restyling the native `input[type="checkbox"]` in place** — `appearance: none` plus a `::after` knob, scoped to `.settings-dialog` — **and marked `role="switch"`.** No wrapper markup, no custom element, no library.

Three consequences of that shape are deliberate:

- **One rule set covers both populations.** The static fields and the runtime-built metric rows are the same element, so they cannot drift apart, and a boolean added later is styled correctly the day it is written.
- **The switch is themed for free.** The on-state is `--cyan` for the track and `--surface` for the knob. Each theme remaps the *named* tokens to its own palette (`ember` defines `--cyan: #fb923c`), so the switch adopts every theme's accent with no per-theme rule. The pairing also inverts correctly: `--cyan` and `--surface` move in opposite lightness directions, so the knob stays legible on the dark themes *and* on light `solar`. Measured: `ember` `#fb923c` track / `#1c1110` knob; `solar` `#0369a1` track / `#ffffff` knob.
- **A switch means an on/off setting; a checkbox still means set membership.** The checkboxes *outside* the dialog — the history series selectors and the filesystem filter — are left as checkboxes on purpose. They pick members of a set, which is exactly what a checkbox means, and what `role="switch"` would misannounce.

Alignment is fixed at the same time: the legend becomes a flex row (the float is gone), the metric row's magic offset is replaced by a name line whose height equals the switch's, and the unit becomes a small tag beside the name instead of a line of its own.

## Alternatives rejected

1. **A wrapper-markup switch component** (`<label class="switch"><input><span class="track">`). The popular pattern, and rejected: it would have to be applied to thirteen static sites *and* the runtime builder, which is exactly the drift this decision exists to prevent, and it rewrites markup that the dashboard tests assert against as source text for no behavioural gain.
2. **A scripted custom control** (`<div role="switch">` driven by JS). Throws away native keyboard handling, focus, form participation, and `:checked` — every one of which the current settings code already relies on — in exchange for styling freedom the native element does not actually withhold once `appearance: none` is set.
3. **A component library or CSS framework for one control.** A dependency for a thirty-line rule set, against the standing rule that a new dependency must earn its place.
4. **Leaving the checkboxes and only fixing alignment.** Addresses the second complaint and ignores the first: the affordance would still say "member of a set" where the model says "on or off".

## Consequences

- Every boolean in the settings dialog reads as on/off at a glance, and screen readers announce it as a switch rather than a checkbox.
- The dashboard has one documented rule for a recurring question — switch for a setting, checkbox for a set — so the next contributor does not have to guess.
- The styling is scoped to `.settings-dialog`. A boolean added to a *different* dialog will render as a plain checkbox until that dialog opts in; that is intentional, because the set-vs-setting question has to be answered per control rather than globally.
- The switch inherits any theme added later with no extra work, provided the theme keeps defining `--cyan` and `--surface`. A theme that dropped either token would fall back to an unstyled-looking control.
- No dependency, no new markup contract, and no change to how settings are collected or saved.
