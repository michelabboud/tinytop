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

The command center can install or build the Rust collector, bootstrap Bun for
development, run the setup wizard, manage user-space systemd services, and
perform SQLite backup/check/reset operations.

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

## Start And Stop

Start the Rust foreground daemon:

```bash
./tinytop rust serve
```

Start Bun development mode:

```bash
./tinytop start
```

Start Bun split mode:

```bash
./tinytop start:split
```

Stop foreground processes with `Ctrl-C`.

Stop systemd services when installed:

```bash
./tinytop systemd stop
```

## Verification Commands

```bash
./tinytop check
git diff --check
```

Expected `bun run check` behavior:

1. Runs all Bun tests.
2. Runs `src/server.ts --check`.
3. Runs `legacy/bun-collector.ts --check`, including an in-memory SQLite write/read.
4. Runs the Rust workspace tests through Cargo.

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
```

Check public history proxy:

```bash
curl -fsS 'http://127.0.0.1:4274/api/history?limit=3&window_seconds=300'
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

The server sends `cache-control: no-store`, but browser state can still preserve settings.

Try:

- hard refresh the page
- switch theme or graph mode to confirm JS is live
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

Retention is not implemented yet. Monitor database size:

```bash
./tinytop db stats
```

Archive or reset history when needed until retention lands.
