# TinyTop Handoff

Date: 2026-06-25 22:14 Asia/Jerusalem

## Current Repo State

- Repo: `/home/michel/projects/tinytop`
- Branch: `main`
- Remote: `origin` at `git@github.com:michelabboud/tinytop.git`
- Latest shipped feature checkpoint: `d829160dff5f9c894a04a83eda8bae535c3622ba`
- Latest shipped feature tag: `v0.1.14`
- Current handoff checkpoint version: `0.1.15`
- Version files: `VERSION`, `package.json`, and `tinytop` all read `0.1.15`
- Working tree before this handoff checkpoint: clean and aligned with `origin/main`

## Runtime State

- Dashboard URL: `http://127.0.0.1:4274`
- Health endpoint: `http://127.0.0.1:4274/health`
- Health status at handoff time: `ok`
- Active process: `tinytop-agent` PID `178978`
- Command line:

  ```text
  /home/michel/projects/tinytop/agent/target/release/tinytop-agent serve --public-dir /home/michel/projects/tinytop/public
  ```

- Listener:

  ```text
  127.0.0.1:4274 -> tinytop-agent pid 178978
  ```

- Legacy Bun writer port `127.0.0.1:4276` is not running.
- Stop command if needed:

  ```bash
  kill 178978
  ```

## Rust Collector Confirmation

Yes, the running dashboard is using the Rust daemon and Rust Linux/WSL collector path.

Evidence:

- The live process is `agent/target/release/tinytop-agent serve`.
- The Rust daemon owns the public dashboard routes on `127.0.0.1:4274`.
- `ARCHITECTURE.md` states `tinytop-agent serve` returns the latest stored sample or collects a fresh one, and collects telemetry through `tinytop-collectors`.
- The latest `/api/snapshot` response reported:
  - Host: `WizAI`
  - Distro: `Linux (Ubuntu 24.04)`
  - Kernel: `6.18.33.1-microsoft-standard-WSL2`
  - Runtime: `WSL`
  - Confidence: `high`
  - Reason: `kernel release/version contains Microsoft WSL markers`

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
  - Bun tests: `39 pass`, `0 fail`
  - Rust workspace tests: passed
  - Browser bundle: built `public/app.js` successfully
- `git diff --check`: clean
- Production dialog scan over `public` and `src`, excluding `public/vendor`: no forbidden matches
- Playwright rendered QA:
  - confirmation dialog opened from Live History `Clear`
  - cancel preserved `120 samples / 80 shown`
  - confirm cleared to `0 samples`
  - native browser dialog count: `0`
  - page error count: `0`
- Remote check:
  - `origin/main` points to `d829160dff5f9c894a04a83eda8bae535c3622ba`
  - `origin/v0.1.14` points to the same commit

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
- Add a writer/daemon health indicator in the UI if history or snapshot APIs degrade.
- Add native Windows and macOS collectors when the project moves beyond Linux/WSL.

## Notes For Resuming

- The Rust daemon is a foreground process started by this session, not systemd. If the terminal session is gone and the port is down, restart with `./tinytop rust serve`.
- WSL user systemd was previously unavailable in this environment, so foreground Rust daemon mode is the known-working path.
- The dashboard is loopback-only by design.
- `public/vendor/echarts.min.js` is vendored third-party code and should stay excluded from local UI policy scans.
