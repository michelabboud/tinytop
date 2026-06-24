# History Scrubber Verification

## Scope

Verifies the `0.1.2` correction for history placement and timeline scrubbing.

## Expected Checks

- Live History appears directly after the CPU, RAM, and swap gauge row.
- The duplicate Bars history option is absent.
- Line, Area, and Heatmap remain available as history canvas views.
- The timeline scrubber can select an older local sample.
- Scrubbing updates the main CPU, RAM, and swap gauges to the selected sample.
- The Live button returns the gauges to the latest sample.

## Results

```bash
bunx playwright test overview-graph-mode.spec.js --reporter=line
```

Result: 1 passed, 0 failed. The test used deterministic mocked `/api/snapshot` responses to prove the gauge values change from the newest sample to an older sample and back to live.

```bash
bun visual-live-check.mjs
```

Result: the real live page rendered with `#history` immediately after `#overview`, no Bars option, no console warnings, and a visible historical timestamp after scrubber interaction.

```bash
bun -e '<mobile overflow check>'
```

Result: mobile viewport `390x900` reported `scrollWidth: 390`, `clientWidth: 390`, no Bars option, and `#history` immediately after `#overview`.

Additional full-project verification is recorded in the close-out report for this task.
