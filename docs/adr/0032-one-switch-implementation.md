# 0032 - There is exactly one switch implementation, and it keeps the legacy geometry

## Status

**Accepted (2026-09-01; 0.8.2)** — Michel, using the deployed 0.8.1 dialog: *"the toggle of open
telemetry is faulty"*. Amends **ADR 0028**'s geometry; ADR 0028's core decision (restyle the native
`input[type="checkbox"]` in place rather than build a wrapper component) **stands unchanged**.
Anchors verified at main `85e7086`.

## Context

The reported defect is real, is not confined to OpenTelemetry, and was introduced by ADR 0028's own
implementation. `styles.css` carries **two switch implementations that do not override each other**,
because they draw the knob with **different pseudo-elements**:

| | ADR 0028's | legacy |
|---|---|---|
| selector | `.settings-dialog input[type="checkbox"]` (`:1429`) | `.settings-group .toggle-row input[type="checkbox"]` (`:1528`) |
| specificity | (0,1,1) | **(0,2,1) — wins** |
| knob | `::after`, `0.8rem`, `translate: calc(0.99rem - 2px)` | `::before`, `16px`, `transform: translateX(18px)` |
| track | `2.15rem × 1.2rem`, on-state `--cyan` | `42px × 24px`, on-state `--amber` |
| introduced | `276c88d` (0.7.0) | `554d429` (**v0.1.31**) |

Both selectors match every toggle in the dialog. `::before` and `::after` are distinct boxes and
nothing suppresses either, so **each of the 13 switches renders two knobs** — one 16 px travelling
18 px, one ~12.8 px travelling ~13.8 px — over a track sized and coloured by the legacy rule. The
result reads as a smeared or mid-travel knob: obvious when magnified, easy to miss at normal size,
and present on every switch rather than only the one reported.

**ADR 0028 described exactly this failure and then caused it.** Its stated reason for restyling the
native input in place was that *"one rule set then covers both the thirteen static fields and the
metric picker built at runtime, so the two cannot drift."* It added a second rule set and left the
first in place. This is the third time a competing rule has silently won in this stylesheet, after
`.metric-family` and `.advanced-document-settings-group` (ADR 0029 decision 4).

The other reading was checked and rejected: the live daemon reports `otel.enabled: false`, and the
orange track is the legacy `:checked` background, so the photographed switch was mid-interaction
rather than misreporting its state. The defect is the doubled knob, not a desynced checkbox.

## Decision

**1. One implementation survives: ADR 0028's.** The legacy block (`:1528`–`:1572`) is deleted. ADR
0028's rule is the one scoped to the whole dialog, so it covers the runtime-built metric picker rows
as well as the static fields — which is the property ADR 0028 wanted and did not get.

**2. It adopts the legacy geometry.** The surviving rule takes the legacy track (`42 × 24`), knob
(`16 px` at `3 px`) and travel (`18 px`), replacing ADR 0028's smaller `2.15rem × 1.2rem` / `0.8rem`.
Two reasons: it is the size Michel has actually been looking at since v0.1.31, so nothing shifts under
him; and a 24 px control is a better pointer target than a 19 px one. **This is the part that amends
ADR 0028.**

**3. It keeps ADR 0028's colour pair.** On-state stays `--cyan` over `--surface`, not the legacy
`--amber`. ADR 0028's reasoning holds and is worth preserving: `--cyan` is remapped by every theme to
that theme's own accent, so the switch inherits `ember`'s `#fb923c` and light `solar`'s `#0369a1`
for free and inverts correctly, whereas a literal `--amber` pins one hue across all themes.

**4. The `.toggle-row` layout rules are not switches and stay.** `:1491` (the row grid) and `:1502`
(the history-ladder row box) style the *row*, not the control, and are untouched.

## Alternatives rejected

1. **Keep the legacy block, delete ADR 0028's.** Simplest edit, and wrong: the legacy selector
   requires a `.toggle-row` ancestor, so it would not reach the metric picker rows built at runtime —
   reintroducing the exact drift ADR 0028 exists to prevent, and pinning `--amber` across all themes.
2. **Scope one of them tighter so both survive.** Preserves both hues at the cost of two rule sets
   that must be kept in step by hand. That is the situation being fixed.
3. **Suppress the losing pseudo-element** (`::after { content: none }`). A one-line fix that leaves
   two implementations in the file and makes the next reader wonder which is live.
4. **Keep ADR 0028's smaller geometry.** Defensible — it is the accepted decision — but it would
   shrink every switch under a user who has never seen the small one, to no benefit.

## Consequences

- **Every switch in the dialog changes appearance**, from a doubled knob to a single one. This is
  visible, deliberate, and cannot be verified by a unit test — a test cannot see two knobs. Acceptance
  is a **rendered check of every switch, both themes, on and off**, at the real dialog size. That is
  the "one rendered-page check per dashboard release" law, and it is the only instrument that would
  have caught the original defect.
- The on-state hue changes from literal `--amber` to the theme's accent. On `ember` and `solar` this
  is a near-identical orange; on `midnight` it becomes that theme's accent, which is the intent.
- A future contributor adding switch styling must add it to the single `.settings-dialog
  input[type="checkbox"]` rule. A new, more specific rule elsewhere will silently win again, and this
  file has now demonstrated that failure three times.
- ADR 0028 is **not edited**. Its geometry is superseded here; its central decision stands.

## Errata (2026-09-01, before this ADR shipped)

**Decision 3's consequence above is wrong for `solar`, and the sentence is left standing so the
error is visible rather than quietly rewritten.**

It claims that keeping `--cyan` leaves "a near-identical orange" on both `ember` and `solar`.
Measured in a real browser against the fixed stylesheet:

| theme | today, legacy `--amber` | after this ADR, `--cyan` | verdict |
|---|---|---|---|
| `ember` | `#f59e0b` | `#fb923c` | near-identical orange — **claim holds** |
| `solar` | `#b45309` (orange) | `#0369a1` (**blue**) | **claim is false** |
| `matrix` | `#fbbf24` | `#67e8f9` | orange → cyan |
| `aurora` | `#fbbf24` | `#38bdf8` | orange → blue |
| default | `#f59e0b` | `#38bdf8` | orange → blue |

So on four of five themes — including `solar`, which is the one in daily use — the on-state switch
changes from orange to the theme's cool accent. That is a visible change to every switch in the
dialog and it was not stated when the decision was written.

**Decision 3 stands after being put to Michel, who returned it ("you decide"), on this evidence:**
`.history-series-toggles input` and `.inline-toggle input` already carry `accent-color: var(--cyan)`
(`styles.css:1831-1834`), so on `solar` every other accented control in the dashboard is *already*
this blue. The amber switch was the outlier, not the convention it appeared to be. Keeping `--amber`
would preserve familiarity at the cost of pinning one literal hue across all five themes — the exact
coupling ADR 0028 existed to remove.

**Method note, recorded because it nearly produced a false finding.** An earlier probe reported the
painted track colour as frozen across all themes, which looked like a fifth competing rule. It was an
artifact: setting `data-theme` and reading `getComputedStyle` in the *same* task returns stale
custom-property-dependent paint, because `void offsetHeight` forces layout but not that recomputation.
Read in a separate task, `--cyan` resolves correctly per theme. **Any future theme measurement in this
dialog must set the attribute and read in separate tasks.**
