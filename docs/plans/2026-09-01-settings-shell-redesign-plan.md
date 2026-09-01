# Plan — settings shell redesign (U1–U5)

**Status: DONE — U0 shipped in 0.8.2; U1/U2/U3/U5 shipped in 0.9.0 as ADR 0033.**
**Author:** fable@fabulous, 2026-09-01. Every file:line verified at main `85e7086` (v0.8.1, deployed).
**Source:** Michel's five instructions after using the deployed 0.8.1 dialog, with screenshots.

---

## ⚠️ What this plan got wrong, recorded rather than rewritten

**The gate itself was the biggest error.** His five instructions *were* the authorization; turning
them into a proposal with three open questions was an over-gate, and it left items 1, 2, 3 and 5
unbuilt after 0.8.2 shipped. Of the three questions, one had already been answered ("you decide"),
one had a stated recommendation, and one (General's carving) was an ordinary judgement call. Only U0
was built at the time; the rest waited for a nod that was never needed.

Four substantive corrections found while building:

1. **Advanced needed a tab row too.** This plan gave it none, on the grounds that ADR 0029 had
   already split it into two columns. Measured at a 720 px window it still overflowed by 20 px, and
   it has two groups that already exist — so it gained Export · Document. With the OTel group then
   owning the full width, ADR 0029's one-field-per-row rule overflowed by 43 px and was superseded.
2. **Regrouping History was not enough to fix "looks bad".** With `auto-fit`, "Enable L4" and "L4
   days" still landed on different rows, because the grid wraps by rendered width. Tiers now uses two
   fixed columns so each tier's switch pairs with its own field by construction. Caught by looking at
   a screenshot after the geometry check said the panel fit.
3. **The tier switch labels were too long** ("Enable L3 five-minute history" wrapped to three lines
   and staggered the rows). Shortened to "Enable L3"; the resolutions are in the group's help text.
4. **`moveSettingsTab` could not be reused.** It falls back to the literal `"general"` — a *primary*
   tab name, meaningless inside a secondary row, and reachable whenever a remembered sub-tab no
   longer exists. Secondary rows got their own `moveWithinTabRow` / `resolveTabInRow`.

The three "open questions" were resolved as: switch geometry — already answered and shipped in 0.8.2;
help density — one paragraph per group, as recommended; General's carving — Browser · Daemon ·
Thresholds · Display, decided rather than asked, and open to correction in review.

---

## His five, restated so I can be held to them

1. **Redesign History** — it looks bad.
2. **Add a secondary row of tabs** dividing content into groups *that already exist*, so nothing scrolls.
3. **Same for Metrics** — secondary tabs by group, no scroll.
4. **The OpenTelemetry toggle is faulty.**
5. **Add a `(?)` help affordance** in sections/groups.

## What I found before proposing anything

### U4 is a real defect, it affects all 13 switches, and it is my own regression

`styles.css` contains **two competing switch implementations**, and they do not override each other
because **they draw the knob with different pseudo-elements**:

| | ADR 0028's | the legacy one |
|---|---|---|
| selector | `.settings-dialog input[type="checkbox"]` (`:1429`) | `.settings-group .toggle-row input[type="checkbox"]` (`:1528`) |
| specificity | (0,1,1) | **(0,2,1) — wins** |
| knob | **`::after`** (`:1445`), `0.8rem`, `translate: calc(0.99rem - 2px)` | **`::before`** (`:1544`), `16px`, `transform: translateX(18px)` |
| track | `2.15rem × 1.2rem`, on-state `--cyan` | `42px × 24px`, on-state `--amber` |
| origin | `276c88d` (0.7.0, the ADR 0028 pass) | `554d429` (**v0.1.31**, legacy) |

Both rules match every toggle in the dialog, so each switch renders **two knobs** — a 16 px `::before`
at `left: 3px` and a ~12.8 px `::after` at `0.18rem`, travelling **18 px and ~13.8 px** respectively
when checked. The track is sized and coloured by the legacy rule; the ADR 0028 knob rides on top,
smaller and out of step. That is the smeared, mid-travel toggle in Michel's screenshot — obvious when
zoomed, subtle at normal size, and **present on all 13 switches**, not only OpenTelemetry.

**This is ADR 0028's own failure mode, and the irony should be recorded rather than smoothed over.**
That ADR's stated reason for restyling the native input in place was *"one rule set then covers both
the thirteen static fields and the metric picker built at runtime, so the two cannot drift."* It then
**added a second rule set and left the first in place.** Third occurrence in this file of a competing
rule silently winning (after `.metric-family` and `.advanced-document-settings-group`).

**Not a state desync.** The live daemon reports `otel.enabled: false`, and the screenshot's orange
track is the legacy `:checked` background — so the toggle Michel photographed was mid-interaction or
freshly clicked. The defect is the doubled knob, not a lying checkbox. Worth stating because "the
toggle is faulty" could reasonably have meant the other thing, and I checked.

### The scrolling is ADR 0029's documented fallback, not a regression

ADR 0029 fixed the dialog at `height: min(820px, calc(100dvh - 2rem))` and I reported every tab
fitting. That was measured against an **820 px** dialog. Michel's window is shorter, so the `100dvh`
arm wins (~687 px) and `.settings-dialog-body`'s `overflow: auto` engages — the deliberate
small-viewport fallback ADR 0029 records. **My "every tab fits" claim was true at the size I tested
and false at his.** Secondary tabs fix this structurally rather than by measurement, because they cut
the content per view instead of assuming a viewport.

### Which groups actually exist

| panel | fieldsets today | notes |
|---|---|---|
| general | 2 — *This Browser*, *This Daemon* | Daemon holds defaults + 10 thresholds + 6 display toggles; it is the one that overflows |
| history | **1** — *History ladder* | 13 controls of three different kinds in one flat grid; this is why it looks bad |
| metrics | **0** | built at runtime from `METRIC_REGISTRY`, already grouped by **family** |
| thermals | 1 — *CPU thermals* | short, fits |
| advanced | 2 — *OpenTelemetry export*, *Raw settings document* | ADR 0029 already split it into two columns |

`METRIC_REGISTRY` families, verified in `otel.rs`: **cpu 4, memory 3, filesystem 2, pressure 2,
load 1, swap 1** = 13. So Metrics' secondary tabs need **no new grouping** — his "groups that already
exist" is literally true there. History's do not exist yet, which is exactly why U1 and U2 are one
piece of work for that tab.

---

## The design

### U0 — one switch implementation (the U4 fix, and the foundation for everything else)

Delete the **legacy** block (`:1528`–`:1570`) and keep ADR 0028's, which is the accepted decision and
the only one scoped to cover the runtime-built metric rows as well as the static fields.

**But keep the legacy geometry.** The legacy track is `42 × 24` with a `16 px` knob; ADR 0028's is
`2.15rem × 1.2rem` (~34 × 19) with a `12.8 px` knob. The larger target is the better one — it is what
Michel has been looking at, and 24 px is a friendlier hit area. So the surviving rule adopts the
legacy's dimensions and travel, and ADR 0028's `--cyan`/`--surface` colour pair and `::after` knob.
This is a **supersession of ADR 0028's geometry**, recorded in a new ADR, not a silent edit.

**Acceptance is a rendered check, not a test.** A unit test cannot see two knobs. Every switch must be
photographed in both themes, on and off, at the real dialog size — the "one rendered-page check per
dashboard release" law, which is the only instrument that would have caught this.

### U1 + U2 + U3 — a secondary tab row

A second, lower-prominence tablist inside a panel, shown only for panels that declare sub-groups.

| panel | secondary tabs | source of the grouping |
|---|---|---|
| **General** | Browser · Defaults · Thresholds · Display | Browser exists; the Daemon fieldset splits into three by meaning |
| **History** | Tiers · Archive · Disk | **new grouping — this is U1's redesign** |
| **Metrics** | CPU · Memory · Swap · Filesystem · Load · Pressure | `METRIC_REGISTRY.family`, already there |
| **Thermals** | *(none — it fits)* | one short group |
| **Advanced** | *(none — ADR 0029 already gave it two columns)* | |

**History's regrouping (U1), which is the substance of "looks bad":** today thirteen controls of three
different kinds sit in one flat grid, so a toggle for L3 lands between the *L2 days* and *L3 days*
number fields and the eye cannot find a rhythm. Regrouped:

- **Tiers** — L1 raw days, L2 one-minute days, *enable L3* + L3 days, *enable L4* + L4 days, keep L4
  forever. Each tier's toggle sits **with its own number field**, not beside a neighbour's.
- **Archive** — queryable archive, cold `.csv.gz` archive, cold after months, archive directory.
- **Disk** — disk check minutes, minimum free GiB, filesystem check seconds, per-tick process history.

**Accessibility is the part that needs care, and it is why this earns an ADR.** ADR 0027 established
the primary tablist with roving `tabindex`, wrapping arrows, Home/End and `localStorage` memory. A
nested tablist means two arrow-key scopes on one screen. The rules I intend: the secondary row is its
own `role="tablist"` with its own roving `tabindex`; arrows move within the row that has focus and
never jump between rows; the secondary selection is remembered **per parent tab**; and panels keep
being **permanently mounted and toggled with `[hidden]` only**, because `collectDaemonSettingsFromForm`
reads hidden-tab values and a save must not lose a field the user never visited. That last rule is
ADR 0027's and it is load-bearing — a secondary tab that unmounts its panel would silently drop
settings on save.

### U5 — a `(?)` help affordance per group

A real `<button type="button">` per group legend, not a `title` attribute (invisible to touch and to
screen readers) and not a bare icon (no accessible name). It toggles a short description with
`aria-expanded` + `aria-controls`, so the text is in the DOM and reachable rather than a tooltip.

Copy is not filler: each group's help says **what the setting changes and what it costs** — the
non-obvious part. The wording is mine to draft and Michel's to correct; several of these (the ladder
tiers, the cold archive, minimum free GiB) already have a paragraph of reasoning in ADRs 0013–0021
that has never reached the UI.

---

## Lanes

| # | Lane | Scope | Model | Depends on |
|---|---|---|---|---|
| U0 | `settings-u0-one-switch` | delete the legacy block, adopt its geometry into ADR 0028's rule | `ari-sol` (CSS) | nothing |
| U1 | `settings-u1-history-groups` | regroup History into Tiers/Archive/Disk | `ari-sol` (JS/HTML/CSS) | U0 merged |
| U2 | `settings-u2-secondary-tabs` | the nested tablist + per-parent memory + a11y | `ari-sol-deep` (a11y/state) | U1 merged |
| U3 | `settings-u3-metrics-families` | Metrics secondary tabs from `METRIC_REGISTRY` | `ari-sol` (JS) | U2 merged |
| U5 | `settings-u5-help` | `(?)` buttons + copy | `ari-sol` (JS/HTML/CSS) | U2 merged |

Sequential rather than parallel: U0–U3 all edit `styles.css` and `app.js`, and this set has already
taught us what concurrent edits to the same dashboard files cost. U5 could run beside U3.

## Gate

Bun `bun test` only — **not** `bun run check:bun`, which binds a socket (the limit that made T18b and
S3 escalate). In-containment baseline **260 pass / 1 fail across 22 files**; outside, **264 / 0**.
Rust is untouched by U1/U3/U5; U0 and U2 touch no Rust either.

## Acceptance (mine, outside the sandbox, on a scratch daemon — never `:4274`)

- **Every switch, both themes, on and off, photographed** — the only check that sees a doubled knob.
- **Every tab and sub-tab at Michel's viewport height (~687 px), not 820** — the measurement that
  would have caught the scrolling I reported as fixed.
- Keyboard: arrows within each row, no jumping between rows, Home/End, and a save after visiting only
  one sub-tab that **keeps every field from the sub-tabs never opened**.

## Open questions for Michel

1. **The switch geometry.** I intend to keep the size you have been looking at (42 × 24) and drop the
   smaller ADR 0028 one. Say if you would rather have the smaller switch.
2. **General's four sub-tabs** are my grouping of the Daemon fieldset, not an existing one — Browser ·
   Defaults · Thresholds · Display. Metrics and History map to real groups; this one is a judgement
   call and is the one most likely to be wrong.
3. **How much help text.** A sentence per group, or per field? Per group is my recommendation — per
   field turns the dialog into a manual.
