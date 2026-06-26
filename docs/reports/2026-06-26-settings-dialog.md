# Settings Dialog Verification

Date: 2026-06-26

## Scope

TinyTop `0.1.23` moves Settings out of the main dashboard metrics flow and into an accessible modal dialog.

## Implemented

- Replaced the rail Settings anchor with a button.
- Removed the inline dashboard Settings section.
- Added `<dialog id="settings-dialog">` with `This Browser` and `This Daemon` groups.
- Added explicit Close and Cancel controls, Escape close, backdrop close, and focus return to the Settings button.
- Tuned the dialog settings grid so labels and controls fit on desktop and mobile without horizontal fieldset overflow.
- Kept settings persistence unchanged:
  - browser-local active theme, graph mode, and history window remain in `localStorage`
  - daemon defaults remain in SQLite through `GET /api/settings` and `PUT /api/settings`
- Kept `agent/assets/dashboard/` and `legacy/dashboard/` byte-identical.

## Verification

Focused UI checks:

```bash
bun test tests/dashboard-settings.test.ts tests/dashboard-assets.test.ts tests/webui-dialogs.test.ts
```

Result:

- Settings dialog, asset parity, and web UI dialog policy tests: `8 pass`, `0 fail`.

Full release checks:

- `./tinytop check`: `63 pass`, `0 fail`, Rust fmt/workspace tests passed, browser bundle built.
- `./tinytop rust build`: rebuilt `agent/target/release/tinytop-agent` with embedded `0.1.23` dashboard assets.
- `diff -qr agent/assets/dashboard legacy/dashboard`: no differences.
- `git diff --check`: clean.
- `bun audit`: no vulnerabilities found.
- `cargo audit --file agent/Cargo.lock`: scanned 196 crate dependencies with no vulnerabilities reported.

Live embedded dashboard smoke:

- `./tinytop status`: `rust collector-dashboard-daemon v0.1.23 (embedded dashboard)`.
- `/api/version`: Rust `0.1.23` embedded dashboard identity.
- `/`: contains `id="settings-dialog"`, `id="settings-open-button"`, `This Browser`, and `This Daemon`.
- `/`: old inline settings section and `href="#settings"` anchor are absent.
- `/app.js`: contains `openSettingsDialog` and `closeSettingsDialog`.
- `/api/settings`: returns SQLite-backed daemon defaults.

Rendered browser smoke:

```bash
bun ./node_modules/.bin/playwright test tinytop-settings-dialog-smoke.spec.js --reporter=line
```

Result:

- Desktop and mobile settings-dialog smoke tests: `2 passed`.
- The smoke asserts no page errors and no settings-fieldset horizontal overflow.
- Screenshots were saved outside the repo at `/tmp/tinytop-settings-dialog-desktop.png` and `/tmp/tinytop-settings-dialog-mobile.png`.
