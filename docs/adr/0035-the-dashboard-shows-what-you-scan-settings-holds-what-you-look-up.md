# 0035 - The dashboard shows what you scan; Settings > Info holds what you look up

## Status

**Accepted (2026-09-02; 0.11.0)** — Michel, pointing at the block under the history chart:
*"the idea of dashboard is to see important information in a glance — why is this important as in
first page dashboard? I would say, its place is in settings page, just add Info Tab in settings"*.
Anchors verified at main `83aafcf` (v0.10.1, deployed).

## Context

Eight blocks had accumulated under the history chart on the dashboard's front page:

| block | what it is |
|---|---|
| `#history-coverage` | oldest / newest / DB size / budget / budget used / rollups |
| `#history-disk-pressure` | "History disk check: 184 GiB free; minimum 5.0 GiB." |
| `#history-ladder-coverage` | four cards, L1–L4 span and row counts |
| `#history-archive-status` | "Archive — Queryable: off; Cold: off" |
| `#history-otel-status` | "OTel — off" |
| `#thermal-status` | "Thermals — 0 sensors" |
| `#history-marker-list` | the last six timeline events |
| `#history-sample-values` | CPU / RAM / SWAP / LOAD at the scrubbed point |

None of the first seven answers a question you have *at a glance*. They answer questions you go
**looking for**: how much history do I actually have, is the archive on, why is there a step in this
chart. They were occupying the most valuable space on the page — directly under the primary chart —
to report that a service is off and that there is plenty of disk.

**One of them is not like the others.** `describeDiskCoverage` produces two different sentences from
the same payload: `History disk check: 184 GiB free; minimum 5.0 GiB.` when healthy, and
`Disk pressure: 1 GiB free is below 5 GiB. Shrink history or free disk before extending retention.`
with `data-status="critical"` when not. The first is reference material. **The second is an alert**,
and moving it into a settings dialog would bury a warning behind two clicks and a tab.

`#history-sample-values` is also different: it is the chart's own readout, showing the values at
whatever point the timeline is scrubbed to. It has no meaning away from the chart.

## Decision

**1. A new read-only `Info` primary tab in Settings**, with four sub-groups in the ADR 0033 pattern:

| sub-tab | contents |
|---|---|
| **Coverage** | the six coverage stats, plus the disk check reading in its healthy form |
| **Tiers** | the L1–L4 span cards |
| **Services** | archive, OpenTelemetry and thermal status |
| **Events** | the timeline event log |

**2. `Info` is always available.** Unlike `metrics` and `thermals` it has no capability to gate on —
it reports whatever the runtime does report — so each group carries its own empty state
(`info-tiers-unavailable`, `info-services-unavailable`, `info-events-empty`) rather than the tab
disappearing. A tab that vanishes is worse than a tab that says "nothing to report".

**3. The disk reading is split by severity, not by location.** `renderDiskCoverage` now feeds two
elements: the dashboard banner renders **only under pressure**, and the Info line renders always.
One reading, two audiences.

**4. The chart keeps `#history-sample-values`.** It is the chart's output, not reference material.

**5. The blocks keep their existing classes and ids**, so not one renderer changed — only the
container did. `renderHistoryCoverage`, `renderTierCoverage`, `renderArchiveCoverage`,
`renderOtelCoverage`, `renderThermalCoverage` and `renderHistoryMarkers` all address their targets by
id and are indifferent to where those targets live. The one addition is
`syncInfoServicesEmptyState`, which asks the **DOM** whether any of the three service lines is
visible rather than re-deriving it from the coverage payload, so the empty state cannot disagree with
what is on screen.

**6. Info values are shown in full, never truncated.** On the dashboard an ellipsis was acceptable
decoration; this tab exists to give exact numbers. The coverage columns are widened to fit a whole
timestamp (at the inherited width, `Sep 01, 11:44:23 PM` clipped to `Sep 01, 11:44:2…`) and a tier's
two-timestamp range wraps to a second line instead of being cut.

## Alternatives rejected

1. **Leave it on the dashboard.** The status quo, and the thing being objected to. A front page that
   reports "Archive — Queryable: off; Cold: off" forever is spending its best space on a constant.
2. **Move the whole block, disk banner included.** The obvious reading of the instruction, and it
   would silently downgrade a disk-pressure warning into something you only see if you happen to
   open Settings. Rejected on that alone.
3. **A collapsible "details" disclosure under the chart.** Keeps everything one click away without a
   new tab — and leaves the clutter on the dashboard for anyone who expands it once, since the state
   would be remembered. It also duplicates the tab pattern the dialog already has.
4. **A separate top-level Info *section* in the left rail**, beside Overview/History/Processes.
   Defensible, but the rail is for views of the machine; this is metadata about the daemon, which is
   what Settings already is.
5. **Gate the Info tab on a capability**, as `metrics` and `thermals` are. There is no single
   capability that covers coverage + services + events, and a tab appearing and disappearing between
   runtimes is worse than four honest empty states.

## Consequences

- **The dashboard's history section is now the chart, the timeline, the series toggles, the scrubber
  and the scrubbed readout.** Nothing else. That is the whole intent.
- **A disk-pressure warning still appears on the front page**, and now *only* appears there when it
  means something — previously the same row was always present, so a reader had to actually read it
  to notice it had turned critical. Rarity is the signal.
- One more primary tab, so `availableSettingsTabs` returns six entries in the full case. The tab-set
  tests are updated to assert `info` in **every** capability combination.
- Info is read-only, so **nothing in it appears in the ADR 0034 save manifest** — the integrity guard
  covers form controls, and this panel has none.
- The settings dialog now carries both the *controls* for retention, archive, OTel and thermals and
  the *observed result* of each. That adjacency is useful, and it is the argument for Settings over
  a rail section: you set L3 to 90 days under History and check what it actually reaches under Info.

## Addendum (2026-09-02, 0.12.0) — append-only; nothing above is rewritten

Michel, on the new tab: *"in services add the PID of the rust daemon? right?"* and *"also the open
port"*. Both belong here, and the Services group splits into **Daemon process** and **Optional
services** — the daemon is not an optional subsystem, it is the thing answering the page.

`/api/version` already carried the bound host and port, the executable, the working directory and the
database path; **only the pid was missing**, and it is `std::process::id()` — one field on
`DaemonMetadata`, no dependency. Shown alongside them because "which daemon am I actually looking
at?" is a question asked while something is already confusing, and a tooltip is the wrong place for
an answer you may want to copy.

Two details worth keeping:

- **A falsy number is still a number.** `pid ?? "not reported"` would have printed `0` correctly but
  `port ?? …` would not have survived port `0` (which the test fixture uses). Both are compared
  against `null`/`undefined` explicitly, and a test pins pid `0` and port `0` as *values*.
- **Paths get their own full-width rows and wrap on any character.** Truncating the one value someone
  came here to copy would defeat the tab, the same reasoning as decision 6.

This adds a field to a public response shape. It is additive, and an older daemon that does not send
it renders "not reported" rather than a dash, so the two are never confused.
