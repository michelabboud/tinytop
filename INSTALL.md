# Install

This guide covers local installation, configuration, startup, verification, upgrade, and uninstall for TinyTop.

## Recommended Install

Use the root command center first:

```bash
./tinytop help
./tinytop doctor
./tinytop setup
```

`./tinytop setup` checks for Bun, runs `bun install` when dependencies are missing, and then launches the Bun setup wizard with `bun run setup`.

## Requirements

- Linux or WSL2
- Bun matching the repo package manager line: `bun@1.3.11`
- A shell with access to `/proc`
- Loopback ports `4274` and `4276` available unless overridden

If Bun is missing, TinyTop can print or run the official installer:

```bash
./tinytop install-bun --print-only
./tinytop install-bun --yes
```

Official Bun install docs:

```text
https://bun.sh/docs/installation
```

## Clone The Repo

Replace `<repo-url>` with the published repository URL:

```bash
git clone <repo-url> tinytop
cd tinytop
```

For a local checkout that already exists on this machine:

```bash
cd /home/michel/projects/tinytop
```

## Install Dependencies

```bash
./tinytop deps
```

The only runtime dependency is Apache ECharts. The server serves the browser bundle from local `node_modules` through `/vendor/echarts.min.js`.

## Check Ports

The default local ports are:

- `127.0.0.1:4274` - public dashboard UI
- `127.0.0.1:4276` - internal collector/writer API

Check live listeners:

```bash
ss -ltnp '( sport = :4274 or sport = :4276 )'
```

Check the local fleet claim:

```bash
cat ~/.config/fleet/ports/tinytop.toml
```

## Start The Dashboard

The standard command is:

```bash
./tinytop start
```

This starts:

- the public dashboard process on `127.0.0.1:4274`
- the internal writer process on `127.0.0.1:4276`

Open:

```text
http://127.0.0.1:4274
```

## Start Processes Separately

Use this when you want the writer and dashboard supervised as separate foreground processes from one wrapper:

```bash
./tinytop start:split
```

Or run the underlying commands manually in two terminals:

```bash
bun run writer
```

```bash
TINYTOP_DISABLE_WRITER_SPAWN=1 bun run dev
```

If the writer is on a non-default URL:

```bash
HISTORY_WRITER_URL=http://127.0.0.1:4276 bun run dev
```

## Environment Variables

| Variable | Default | Applies to | Description |
| --- | --- | --- | --- |
| `HOST` | `127.0.0.1` | dashboard | Public dashboard bind host |
| `PORT` | `4274` | dashboard | Public dashboard port |
| `HISTORY_WRITER_HOST` | `127.0.0.1` | writer and dashboard spawn env | Writer bind host |
| `HISTORY_WRITER_PORT` | `4276` | writer and dashboard proxy URL | Writer port |
| `HISTORY_WRITER_URL` | unset | dashboard | Full URL for an existing writer; disables auto-spawn |
| `HISTORY_POLL_MS` | `1500` | writer | Collection interval in milliseconds |
| `TINYTOP_HISTORY_DB` | `~/.local/share/tinytop/history.sqlite` | writer | SQLite database path |
| `TINYTOP_DISABLE_WRITER_SPAWN` | unset | dashboard | Set to `1` to require an already-running writer |
| `XDG_DATA_HOME` | `~/.local/share` | writer | Base directory for default SQLite path |

## SQLite Location

Default:

```text
/home/michel/.local/share/tinytop/history.sqlite
```

Override:

```bash
TINYTOP_HISTORY_DB=/path/to/history.sqlite bun run dev
```

SQLite may create sidecar files:

```text
history.sqlite-wal
history.sqlite-shm
```

## Verify Installation

Run the automated checks:

```bash
./tinytop check
git diff --check
```

Check HTTP endpoints:

```bash
curl -fsS http://127.0.0.1:4274/health
curl -fsS http://127.0.0.1:4276/health
curl -fsS 'http://127.0.0.1:4274/api/history?limit=3&window_seconds=300'
```

Expected health output:

```text
ok
```

## Upgrade

1. Stop running dashboard and writer processes.
2. Pull or apply the new code.
3. Run `./tinytop deps`.
4. Run `./tinytop check`.
5. Start `./tinytop start`.
6. Open the dashboard and confirm Live History hydrates after a browser refresh.

The current schema is created with `CREATE TABLE IF NOT EXISTS` and indexes are created if missing. There is no explicit migration table yet.

## Reset Local History

Stop the dashboard and writer first, then move the database files aside:

```bash
./tinytop db backup
./tinytop db reset --yes
```

Start TinyTop again. The writer will create a fresh database.

## systemd User Services

Install persistent user services:

```bash
./tinytop systemd install
./tinytop systemd start
```

Check or follow them:

```bash
./tinytop systemd status
./tinytop systemd logs
```

Remove them:

```bash
./tinytop systemd uninstall
```

## Uninstall

1. Stop running processes.
2. Remove user services if installed: `./tinytop systemd uninstall`.
3. Remove or archive the project directory.
4. Archive the SQLite database if you no longer need history.
5. Remove the port claim only when this project is no longer using those ports:

```bash
mv ~/.config/fleet/ports/tinytop.toml ~/.config/fleet/ports/tinytop.toml.archived
```

## Troubleshooting

See [docs/guides/OPERATIONS.md](docs/guides/OPERATIONS.md).
