# Load Gauge Verification

Date: 2026-06-26

## Scope

TinyTop `0.1.24` adds Load as a fourth overview gauge beside CPU, RAM, and swap.

## Implemented

- Added a Load overview card with:
  - normalized load percentage
  - raw `1m load / CPU cores` context
  - load sparkline
- Reused the existing load normalization policy: 1-minute load divided by CPU core count, capped to 100.
- Kept the raw load metric-band tile so the dashboard still shows 1m, 5m, and 15m load averages.
- Kept `agent/assets/dashboard/` and `legacy/dashboard/` byte-identical.
- Added `tests/dashboard-overview.test.ts` regression coverage.

## Verification

Focused red/green:

- Red: `bun test tests/dashboard-overview.test.ts` failed before implementation because `load-gauge` and renderer bindings did not exist.
- Green: `bun test tests/dashboard-overview.test.ts tests/dashboard-assets.test.ts` passed after implementation.

Full release checks:

- `bun test tests/dashboard-overview.test.ts tests/dashboard-assets.test.ts tests/dashboard-timeline.test.ts`: `9 pass`, `0 fail`, `44 expect() calls`.
- `./tinytop check`: `65 pass`, `0 fail`, Rust fmt/workspace tests passed, browser bundle built.
- `./tinytop rust build`: rebuilt `agent/target/release/tinytop-agent` with embedded `0.1.24` dashboard assets.
- `diff -qr agent/assets/dashboard legacy/dashboard`: no differences.
- `git diff --check`: clean.
- `bun audit`: no vulnerabilities found.
- `cargo audit --file agent/Cargo.lock`: scanned 196 crate dependencies with no vulnerabilities reported.

Live embedded dashboard smoke:

- `./tinytop status`: `rust collector-dashboard-daemon v0.1.24 (embedded dashboard)`.
- `/api/version`: Rust `0.1.24` embedded dashboard identity.
- `/`: contains `id="load-gauge"`, `id="load-value"`, `id="load-capacity"`, and `id="load-spark"`.
- `/app.js`: contains `loadPercent`, `setGauge(elements.loadGauge, loadPressure)`, and load sparkline drawing.
- `/api/snapshot`: returned live load averages and CPU core count.

Rendered browser smoke:

```bash
bun ./node_modules/.bin/playwright test tinytop-load-gauge-smoke.spec.js --reporter=line
```

Result:

- Desktop and mobile Load gauge smoke tests: `2 passed`.
- The smoke asserts four overview cards, bounded Load gauge percentage, no page errors, and no mobile horizontal overflow.
- Screenshots were saved outside the repo at `/tmp/tinytop-load-gauge-desktop.png` and `/tmp/tinytop-load-gauge-mobile.png`.
