# Operations Guide

This guide covers day-to-day runtime checks, process management, SQLite inspection, backup, reset, and troubleshooting.

## Runtime Processes

Default `bun run dev` starts two processes:

- public dashboard: `src/server.ts` on `127.0.0.1:4274`
- internal writer: `src/collector-daemon.ts` on `127.0.0.1:4276`

Check listeners:

```bash
ss -ltnp '( sport = :4274 or sport = :4276 )'
```

Check health:

```bash
curl -fsS http://127.0.0.1:4274/health
curl -fsS http://127.0.0.1:4276/health
```

## Start And Stop

Start:

```bash
bun run dev
```

Start split:

```bash
bun run writer
TINYTOP_DISABLE_WRITER_SPAWN=1 bun run dev
```

Stop foreground processes with `Ctrl-C`.

Stop known PIDs:

```bash
kill <dashboard-pid> <writer-pid>
```

## Verification Commands

```bash
bun run check
bun build public/app.js --target=browser --outdir=/tmp/tinytop-build-check
git diff --check
```

Expected `bun run check` behavior:

1. Runs all Bun tests.
2. Runs `src/server.ts --check`.
3. Runs `src/collector-daemon.ts --check`, including an in-memory SQLite write/read.

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
TINYTOP_HISTORY_DB=/path/to/history.sqlite bun run dev
```

## Inspect SQLite

Using Bun:

```bash
bun -e 'import { Database } from "bun:sqlite"; const db = new Database(process.env.HOME + "/.local/share/tinytop/history.sqlite"); console.log(db.query("select count(*) as samples from metric_samples").get()); db.close();'
```

Recent rows:

```bash
bun -e 'import { Database } from "bun:sqlite"; const db = new Database(process.env.HOME + "/.local/share/tinytop/history.sqlite"); console.log(db.query("select captured_at_ms, cpu_usage_percent, memory_used_percent from metric_samples order by captured_at_ms desc limit 5").all()); db.close();'
```

## Backup History

Best option: stop the dashboard and writer, then copy the database files.

```bash
mkdir -p ~/.local/share/tinytop/backups
cp ~/.local/share/tinytop/history.sqlite* ~/.local/share/tinytop/backups/
```

When the writer is running, include `history.sqlite-wal` and `history.sqlite-shm` in the backup.

## Reset History

Stop the dashboard and writer first. Move current files aside:

```bash
mkdir -p ~/.local/share/tinytop/archive
mv ~/.local/share/tinytop/history.sqlite* ~/.local/share/tinytop/archive/
```

Start the app again:

```bash
bun run dev
```

The writer will create a fresh database.

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

- Stop the stale dashboard/writer process if it belongs to this project.
- Or run with alternate ports:

```bash
PORT=4284 HISTORY_WRITER_PORT=4286 bun run dev
```

If changing standing ports, update `~/.config/fleet/ports/tinytop.toml` and README documentation.

## Dashboard Starts But History Is Empty

Check writer health:

```bash
curl -fsS http://127.0.0.1:4276/health
```

Check public history proxy:

```bash
curl -fsS 'http://127.0.0.1:4274/api/history?limit=3&window_seconds=300'
```

Check the database:

```bash
bun -e 'import { Database } from "bun:sqlite"; const db = new Database(process.env.HOME + "/.local/share/tinytop/history.sqlite"); console.log(db.query("select count(*) as samples from metric_samples").get()); db.close();'
```

If the writer is healthy but the count is zero, wait a few seconds or call:

```bash
curl -fsS http://127.0.0.1:4276/snapshot/collect
```

## Browser Shows Old UI

The server sends `cache-control: no-store`, but browser state can still preserve settings.

Try:

- hard refresh the page
- switch theme or graph mode to confirm JS is live
- inspect `/app.js` response timestamp through browser dev tools if needed

## Writer Fails To Start

Check:

```bash
bun run writer:check
```

Common causes:

- port `4276` already in use
- SQLite directory not writable
- invalid `TINYTOP_HISTORY_DB` path

## Data Growth

Retention is not implemented yet. Monitor database size:

```bash
du -h ~/.local/share/tinytop/history.sqlite*
```

Archive or reset history when needed until retention lands.
