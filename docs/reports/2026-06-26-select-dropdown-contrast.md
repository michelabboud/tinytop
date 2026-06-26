# Native Dropdown Contrast Fix

Date: 2026-06-26
Version: 0.1.26

## Context

The Settings dialog used themed control text on native `<select>` elements. On the Ember theme, the operating system/browser popup rendered a light option menu while retaining pale themed text, making most option labels hard to read.

The same native select pattern is also used by the process table density control, so the fix covers both settings selects and process controls.

## Fix

- Added explicit option foreground/background pairs for Midnight, Matrix, Aurora, Solar, and Ember.
- Mirrored the stylesheet into both `legacy/dashboard/` and `agent/assets/dashboard/`.
- Added a regression test that checks the option selectors and the high-contrast Solar/Ember color pairs.

## Verification

- `bun test tests/dashboard-settings.test.ts`: failed before the CSS fix because no option styling existed.
- `bun test tests/dashboard-settings.test.ts tests/dashboard-assets.test.ts`: `7 pass`, `0 fail`.
- `./tinytop check`: Bun suite `74 pass`, `0 fail`, Rust workspace tests passed, and the dashboard browser bundle built.
- `./tinytop rust build`: built the `tinytop-agent` release binary with embedded `0.1.26` dashboard assets.
- `curl -fsS http://127.0.0.1:4274/api/version`: returned Rust `0.1.26` with `dashboard: "embedded"`.
- Live `/styles.css`: includes `body[data-theme="ember"] .settings-group select option` with `background: #1c1110` and `color: #fff7ed`.
- Playwright computed style in Ember: settings and process select options both resolved to `rgb(28, 17, 16)` background and `rgb(255, 247, 237)` text.
