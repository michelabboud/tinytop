# TinyTop Handoff

Date: 2026-06-26 08:55 Asia/Jerusalem

## Current Repo State

- Repo: `/home/michel/projects/tinytop`
- Branch: `main`
- Remote: `origin` at `git@github.com:michelabboud/tinytop.git`
- Latest shipped checkpoint before v0.1.20 work: `4c897c3e3c5dc54b0caaff2081035fd1eb934251`
- Latest shipped tag before v0.1.20 work: `v0.1.19`
- Current checkpoint version: `0.1.20`
- Version files: `VERSION`, `package.json`, and `tinytop` all read `0.1.20`
- Working tree before the v0.1.20 setup verification change: clean and aligned with `origin/main`

## Runtime State

- Dashboard URL when running: `http://127.0.0.1:4274`
- Health endpoint when running: `http://127.0.0.1:4274/health`
- Health status at handoff refresh time: running, `ok`
- Dashboard port `127.0.0.1:4274`: in use by `tinytop-agent serve`
- Legacy Bun collector port `127.0.0.1:4276`: free
- Active TinyTop foreground process at handoff refresh time: Rust daemon PID `331250`

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
- Added `tests/webui-dialogs.test.ts`, which scans dashboard UI files and rejects native `alert`, `confirm`, and `prompt` calls.
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

### v0.1.18 - Documentation Sweep

- Refreshed current docs and guides for the embedded Rust collector/dashboard daemon.
- Updated current-path references from the removed root `public/` tree to `agent/assets/dashboard/` and `legacy/dashboard/`.
- Added `docs/reports/2026-06-25-documentation-sweep.md`.
- Updated the ADR index to show ADR 0001 as superseded by the Rust single-daemon runtime decision, without rewriting the historical ADR.

### v0.1.19 - History Retention Documentation

- Clarified that SQLite raw history retention is not implemented yet.
- Documented that raw rows stay in SQLite until manual archive/reset.
- Documented that `/api/history` query windows and the dashboard's 120-sample buffer are read/rendering limits, not database retention.
- Added `docs/reports/2026-06-26-history-retention-docs.md`.

### v0.1.20 - Runtime-Specific Setup Verification

- Added `bun run check:bun` for Bun dashboard/legacy collector checks.
- Added `bun run check:rust` for Rust fmt/workspace tests.
- Kept `bun run check` and `./tinytop check` as full maintainer verification.
- Updated `./tinytop setup` so Rust selections do not run Bun tests and legacy Bun selections do not run Rust tests.
- Rust release-binary systemd setup now installs the binary before running `./tinytop rust collect` as the smoke check.
- Added `docs/reports/2026-06-26-runtime-specific-verification.md`.

### Release Binary Asset Check

- `v0.1.18` release assets were updated with `tinytop-agent-linux-x86_64` and its `.sha256` file.
- A temporary-HOME install test confirmed `./tinytop rust install-binary` downloaded and ran the release binary.

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

## Verification Evidence From v0.1.18 Documentation Sweep

- `./tinytop check`
  - Bun tests: `43 pass`, `0 fail`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - Rust workspace tests: passed
  - Browser bundle: built `legacy/dashboard/app.js` successfully
- `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`: clean
- `git diff --check`: clean

## Verification Evidence From v0.1.19 History Retention Documentation

- `./tinytop check`
  - Bun tests: `43 pass`, `0 fail`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - Rust workspace tests: passed
  - Browser bundle: built `legacy/dashboard/app.js` successfully
- `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`: clean
- `git diff --check`: clean

## Verification Evidence From v0.1.20 Runtime-Specific Setup Verification

- `bun test tests/wizard.test.ts`
  - Wizard tests: `9 pass`, `0 fail`
- `./tinytop check`
  - `bun run check:bun`: `46 pass`, `0 fail`
  - `src/server.ts --check`: `status: ok`
  - `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB
  - `bun run check:rust`: Rust fmt check and workspace tests passed
  - Browser bundle: built `legacy/dashboard/app.js` successfully
- `cargo fmt --manifest-path agent/Cargo.toml --all -- --check`: clean
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

- TinyTop Rust daemon PID `331250` is running at this handoff refresh. Stop it with `kill 331250` if you need the default dashboard port free.
- WSL user systemd was previously unavailable in this environment, so foreground Rust daemon mode is the known-working path.
- The dashboard is loopback-only by design.
- `legacy/dashboard/vendor/echarts.min.js` and `agent/assets/dashboard/vendor/echarts.min.js` are vendored third-party code and should stay excluded from local UI policy scans.
