# Operations Guide

This guide covers day-to-day runtime checks, process management, SQLite inspection, backup, reset, and troubleshooting.

## Command Center

Use `./tinytop` for day-to-day operations:

```bash
./tinytop help
./tinytop doctor
./tinytop status
./tinytop logs
./tinytop monitor
./tinytop stats
```

The Bash command center can install or build the Rust collector, bootstrap Bun for
development, run the setup wizard, manage user-space systemd services, and
perform SQLite backup/check/reset operations. On Windows, use `.\tinytop.ps1`
for Rust install/build/start/stop/status/logs and Windows service commands.

## Runtime Processes

Default persistent mode runs one Rust daemon:

- Rust collector/dashboard daemon: `tinytop-agent serve` on `127.0.0.1:4274`

Legacy Bun development mode starts two processes:

- legacy Bun dashboard: `src/server.ts` on `127.0.0.1:4274`
- legacy Bun collector: `legacy/bun-collector.ts` on `127.0.0.1:4276`

Check listeners:

```bash
ss -ltnp '( sport = :4274 or sport = :4276 )'
```

Check health:

```bash
curl -fsS http://127.0.0.1:4274/health
```

Check the exact running runtime and product version:

```bash
./tinytop status
curl -fsS http://127.0.0.1:4274/api/version
```

## Start And Stop

Start the foreground runtime. The command center auto-selects the Rust collector/dashboard daemon when a Rust binary or Cargo is available:

```bash
./tinytop start
```

Force Rust or legacy Bun explicitly:

```bash
TINYTOP_RUNTIME=rust ./tinytop start
TINYTOP_RUNTIME=legacy ./tinytop start
```

Start Bun split mode:

```bash
./tinytop start:split
```

Stop foreground processes:

```bash
./tinytop stop
```

`Ctrl-C` in the foreground terminal still works for interactive runs.

Stop systemd services when installed:

```bash
./tinytop systemd stop
```

On Windows:

```powershell
.\tinytop.ps1 start
.\tinytop.ps1 status
.\tinytop.ps1 stop
```

Windows service commands:

```powershell
.\tinytop.ps1 service install
.\tinytop.ps1 service start
.\tinytop.ps1 service status
.\tinytop.ps1 service stop
```

`service install` and `service uninstall` require PowerShell running as Administrator.

## Verification Commands

```bash
./tinytop check
git diff --check
```

Expected `./tinytop check` behavior:

1. Runs `bun run check`, which runs `bun run check:bun` and `bun run check:rust`.
2. Builds the browser dashboard bundle.

Runtime-specific setup verification:

- `bun run check:bun` runs Bun tests plus `src/server.ts --check` and `legacy/bun-collector.ts --check`.
- `bun run check:rust` runs Rust formatting and workspace tests through Cargo.
- Rust release-binary setup uses `./tinytop rust collect` as a binary smoke check after installing the release asset.

## SQLite Path

Default:

```text
~/.local/share/tinytop/history.sqlite
```

Sidecar files while WAL is active:

```text
history.sqlite-wal
history.sqlite-shm
```

Override:

```bash
TINYTOP_HISTORY_DB=/path/to/history.sqlite ./tinytop rust serve
```

## Inspect SQLite

Using the command center:

```bash
./tinytop db stats
```

Integrity check:

```bash
./tinytop db check
```

`./tinytop db stats|check|vacuum` uses the Rust collector binary first, falls back to Cargo
when a release binary is unavailable, and uses Bun only as the final fallback.

## Backup History

Best option: stop the daemon, dashboard, or collector, then use:

```bash
./tinytop db backup
```

When the collector is running, include `history.sqlite-wal` and `history.sqlite-shm` in the backup.

## Reset History

Stop the daemon, dashboard, or collector first. Move current files aside:

```bash
./tinytop db backup
./tinytop db reset --yes
```

Start the app again:

```bash
./tinytop rust serve
```

The Rust daemon or legacy Bun collector will create a fresh database.

## Port Conflicts

Symptom:

```text
Failed to start server
```

Check:

```bash
ss -ltnp '( sport = :4274 or sport = :4276 )'
```

Fix:

- Stop the stale daemon, dashboard, or collector process if it belongs to this project.
- Or run with alternate ports:

```bash
PORT=4284 ./tinytop rust serve
```

If changing standing ports, update `~/.config/fleet/ports/tinytop.toml` and README documentation.

## Dashboard Starts But History Is Empty

Check daemon health:

```bash
curl -fsS http://127.0.0.1:4274/health
curl -fsS http://127.0.0.1:4274/api/version
```

Check public history proxy:

```bash
curl -fsS 'http://127.0.0.1:4274/api/history?limit=3&window_seconds=300'
curl -fsS http://127.0.0.1:4274/api/history/coverage
curl -fsS 'http://127.0.0.1:4274/api/history/points?source=rollup&limit=5'
curl -fsS 'http://127.0.0.1:4274/api/history/markers?limit=5'
```

Check the database:

```bash
./tinytop db stats
```

If the daemon is healthy but the count is zero, wait a few seconds or call:

```bash
curl -fsS http://127.0.0.1:4274/snapshot/collect
```

## Browser Shows Old UI

The server sends `cache-control: no-store`, but browser state can still preserve settings. The Rust daemon embeds dashboard assets into the `tinytop-agent` binary, so CSS or JavaScript changes require rebuilding the Rust agent and restarting the daemon before the browser can receive those embedded assets.

Try:

- hard refresh the page
- switch theme or graph mode to confirm JS is live
- check the sidebar version line or `curl -fsS http://127.0.0.1:4274/api/version`
- check the settings API with `curl -fsS http://127.0.0.1:4274/api/settings`
- inspect `/app.js` response timestamp through browser dev tools if needed

## Daemon Fails To Start

Check:

```bash
./tinytop check
```

Common causes:

- port `4274` already in use
- legacy split mode: port `4276` already in use
- SQLite directory not writable
- invalid `TINYTOP_HISTORY_DB` path

## Data Growth

The Rust daemon prunes raw SQLite rows according to `retentionHours` from `/api/settings` after successful collection or settings updates. It also maintains one-minute rollups in `metric_rollups_1m`, prunes them according to `rollupRetentionDays`, records daemon timeline events in `app_events`, and reports target DB budget usage from `targetDatabaseBytes`.

The dashboard's selected timestamp range and browser rendering cap control what is loaded and drawn; those read windows are separate from database pruning.

Legacy Bun split mode keeps raw SQLite rows until you manually archive or reset the database.

Monitor database size:

```bash
./tinytop db stats
curl -fsS http://127.0.0.1:4274/api/history/coverage
```

Archive or reset history manually when you need to force cleanup outside the configured Rust retention window. SQLite file size may not shrink immediately after pruning until a vacuum runs, so compare `databaseBytes`, `databaseBudgetPercent`, and the configured budget rather than assuming deleted rows instantly return disk space.
