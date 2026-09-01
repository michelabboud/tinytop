# 0030 - A live sample never enters a historical window's charted series

## Status

**Accepted (2026-09-01; 0.7.1)** — Michel's report, verbatim: *"notice the behaviour if i choose anything which is not NOW in timeline, the current metric keep pushing it"*, confirmed still present after the 0.7.0 deploy (*"same with version 7"*). Pre-existing; not introduced by T18.

## Context

The dashboard polls `/api/snapshot` every `pollIntervalMs` (default 1500 ms). Each poll called `renderSnapshot`, which called `pushHistory(snapshot)` **unconditionally**: the live sample was appended to `state.snapshots` — the same array the history chart draws — and the array re-sorted and re-derived. `state.historyWindowKey` was never consulted anywhere near that push.

Selecting a preset (15m … 90d, 1y, All) hydrates `state.snapshots` from the daemon via `hydrateHistoryPoints`, which **downsamples the window to `MAX_HISTORY_RENDER_SAMPLES` (1200)**. So the chosen window arrives at or near the render cap, and then:

- every poll appended one live raw sample to it, visibly piling up at the right-hand edge of a window that was supposed to end where the user asked; and
- `trimHistory()` slices to the last 1200, so **once at the cap each live push evicted the oldest point of the chosen window** — roughly one per tick. Over ~30 minutes at 1.5 s the entire selected range had been replaced by live data.

That is what "the current metric keep pushing it" describes, and the second effect is data loss in the view rather than a cosmetic wobble: the window silently erodes from the left while the user is reading it.

## Decision

**Only the `live` window charts live samples.** `renderSnapshot` routes the polled snapshot through two pure rules in `ladder-rules.js`:

- `liveSampleEntersHistory(historyWindowKey)` — true only for `"live"`. Gates `pushHistory`.
- `liveSampleDrivesTiles(historyWindowKey, selectedAtMs)` — true when a historical window is charted **and** nothing is scrubbed. Gates a direct `renderSnapshotDetails(snapshot)`.

Silencing the push must not freeze the gauges: on a historical window the chart shows the past while the tiles keep reporting the machine now. If the user has scrubbed to a specific sample, that selection wins — it is what they asked to look at.

**Ordering is load-bearing.** The live tile render must run **after** `renderSelectedSample()`. That function re-renders the tiles from the charted window's last stored sample whenever the sample's source is `raw`, which on a raw-tier window (15m, 1h) is a real snapshot — so calling the live render first left it overwritten and the gauges frozen. This was caught in the browser, not by the wiring test, and the ordering is now pinned by an assertion.

A historical window still refreshes wholesale when it is re-selected; it is a fixed span, not a frozen one.

## Alternatives rejected

1. **Re-fetch the whole historical window on every poll** so it slides as a true rolling range. Correct-looking, and rejected: it puts a full window query (up to 1200 points, and for long presets an archive read) on a 1.5 s timer for a view the user is deliberately holding still.
2. **Keep pushing, but drop the oldest only when the sample is newer than the window's end.** Keeps the eviction machinery and the visible pile-up; treats the symptom.
3. **Push into a separate live overlay series drawn on top.** More code and a second scale/downsample path, to show data the user did not ask for on a window they chose precisely to exclude it.
4. **Freeze the tiles too while a historical window is charted.** Simplest branch, and wrong: the tiles are the "is my machine OK right now" answer, and the header still says Live.

## Consequences

- A chosen window holds still and keeps its full span for as long as it is displayed. Verified behaviourally with a control: on `15m` the sample counter held at 300 across 7 s while the tiles changed 3 times; on `live` the counter moved 161 → 165 over the same interval.
- Two more pure, unit-tested rules in `ladder-rules.js` rather than an inline condition, matching how the rest of the dashboard's decisions are expressed and tested.
- The live/historical split is now explicit, so a future "auto-refresh the historical window" feature has one obvious place to live (alternative 1) instead of being re-introduced accidentally by restoring the unconditional push.
- `MAX_HISTORY_RENDER_SAMPLES` still caps the live window; that behaviour is unchanged.
