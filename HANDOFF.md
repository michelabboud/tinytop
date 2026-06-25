# TinyTop Handoff

Date: 2026-06-25 22:50 Asia/Jerusalem

## Current Repo State

- Repo: `/home/michel/projects/tinytop`
- Branch: `main`
- Remote: `origin` at `git@github.com:michelabboud/tinytop.git`
- Latest shipped checkpoint before this work: `c473814eae44ea4566555548a0b4306f5324a450`
- Latest shipped tag before this work: `v0.1.16`
- Current checkpoint version: `0.1.17`
- Version files: `VERSION`, `package.json`, and `tinytop` all read `0.1.17`
- Working tree before the v0.1.17 work: clean and aligned with `origin/main`

## Runtime State

- Dashboard URL when running: `http://127.0.0.1:4274`
- Health endpoint when running: `http://127.0.0.1:4274/health`
- Health status at handoff refresh time: not running
- Dashboard port `127.0.0.1:4274`: free
- Legacy Bun collector port `127.0.0.1:4276`: free
- Active TinyTop foreground process at handoff refresh time: none found on the default ports

## Rust Collector Confirmation

The default persistent dashboard path is the Rust collector/dashboard daemon.

Evidence:

- `ARCHITECTURE.md` states `tinytop-agent serve` serves the dashboard, returns the latest stored sample or collects a fresh one, and collects telemetry through `tinytop-collectors`.
- `tinytop-agent serve` embeds dashboard assets from `agent/assets/dashboard/` by default.
- `./tinytop systemd install` defaults to the single Rust collector/dashboard daemon.
- The legacy Bun collector now lives at `legacy/bun-collector.ts` and is available only through explicit Bun development or `--bun` systemd mode.
- The legacy Bun dashboard assets now live at `legacy/dashboard/`.

## Recently Completed

### v0.1.14 - Web UI Confirmation Dialogs

- Replaced alert-named UI hooks with `status-message`.
- Added a reusable accessible in-app confirmation dialog.
- Added a confirmed `Clear` action for the browser-local Live History session buffer.
- Added `tests/webui-dialogs.test.ts`, which scans public UI files and rejects native `alert`, `confirm`, and `prompt` calls.
- Updated `README.md`, `GUIDE.md`, `ARCHITECTURE.md`, `CHANGELOG.md`, `PROGRESS.md`, and `docs/reports/2026-06-25-webui-confirmation-dialog-verification.md`.
- Committed and pushed `d829160`.
- Tagged and pushed `v0.1.14`.

### v0.1.15 - Handoff Checkpoint

- Added this root `HANDOFF.md` restart point.
- Recorded live daemon state, Rust collector confirmation, recent verification evidence, and next useful work.

### v0.1.16 - Collector Naming And Legacy Bun Placement

- Moved the legacy Bun collector daemon to `legacy/bun-collector.ts`.
- Added `bun run collector` and `bun run collector:check`, keeping writer aliases for compatibility.
- Updated the setup wizard to ask for `rust` or `bun` collector runtime.
- Kept Rust as the default one-daemon collector/dashboard path.
- Renamed newly rendered legacy Bun systemd collector service to `tinytop-collector.service`.
- Kept cleanup/status paths aware of older `tinytop-writer.service` installs.

### v0.1.17 - Embedded Rust Dashboard Assets

- Moved the static dashboard asset tree to `legacy/dashboard/` for the legacy Bun runtime.
- Added a byte-identical Rust dashboard asset tree under `agent/assets/dashboard/`.
- Embedded the dashboard HTML, CSS, browser JavaScript, and ECharts bundle into `tinytop-agent serve`.
- Kept `--public-dir` and `TINYTOP_PUBLIC_DIR` as explicit development overrides.
- Updated `./tinytop rust serve` and systemd rendering to use embedded assets by default.
- Added ADR 0006 for embedded Rust dashboard assets and legacy dashboard asset ownership.

### Daemon Start

- Started the Rust daemon with:

  ```bash
  ./tinytop rust serve
  ```

- Verified health with:

  ```bash
  curl -fsS http://127.0.0.1:4274/health
  ```

## Verification Evidence From Latest Feature Checkpoint

- `./tinytop check`
  - Bun tests: `43 pass`, `0 fail`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - Rust workspace tests: passed
  - Browser bundle: built `legacy/dashboard/app.js` successfully
- `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`: clean
- `./tinytop rust build`: built `agent/target/release/tinytop-agent`
- Embedded dashboard smoke test with `./tinytop rust serve --sqlite sqlite::memory: --poll-ms 100000`
  - `/health`: `ok`
  - `/`: contained `<title>TinyTop</title>`
  - `/app.js`: contained `requestConfirmation`
  - `/vendor/echarts.min.js`: contained `echarts`
  - Process stopped after the smoke test; default ports are free
- `git diff --check`: clean

## Useful Commands

```bash
cd /home/michel/projects/tinytop
./tinytop help
./tinytop rust serve
curl -fsS http://127.0.0.1:4274/health
curl -fsS http://127.0.0.1:4274/api/snapshot
curl -fsS 'http://127.0.0.1:4274/api/history?limit=5'
./tinytop check
```

## Next Useful Work

- Add SQLite raw-history retention with a configurable 24 to 72 hour default.
- Add one-minute rollups for longer history ranges.
- Add a dashboard setting for visible history duration and persisted sample count.
- Add a collector/daemon health indicator in the UI if history or snapshot APIs degrade.
- Add native Windows and macOS collectors when the project moves beyond Linux/WSL.

## Notes For Resuming

- No TinyTop foreground daemon is running at this handoff refresh. Start one with `./tinytop rust serve` or use `./tinytop systemd start` after installing the service.
- WSL user systemd was previously unavailable in this environment, so foreground Rust daemon mode is the known-working path.
- The dashboard is loopback-only by design.
- `legacy/dashboard/vendor/echarts.min.js` and `agent/assets/dashboard/vendor/echarts.min.js` are vendored third-party code and should stay excluded from local UI policy scans.
