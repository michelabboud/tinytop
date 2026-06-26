# Install

This guide covers local installation, configuration, startup, verification, upgrade, and uninstall for TinyTop.

## Recommended Install

Use the root command center first:

```bash
./tinytop help
./tinytop doctor
./tinytop setup
```

For the default no-Bun persistent runtime, install a Rust collector binary and then install the Rust collector/dashboard user service:

```bash
./tinytop rust install-binary
./tinytop systemd install --rust
./tinytop systemd start
```

If a release binary is not available for your platform, compile locally:

```bash
./tinytop install-rust --print-only
./tinytop rust build
./tinytop systemd install --rust
```

`./tinytop setup` is still available for source/development installs after Bun is installed. It launches the Bun setup wizard with `bun run setup`, asks whether to install the Rust collector or the legacy Bun collector, and asks Rust users whether to use a GitHub release binary or a local Cargo compile.

Wizard verification follows the selected collector. Rust release-binary installs run a Rust binary smoke check, Rust compile installs run Rust fmt/tests, and legacy Bun installs run only Bun dashboard/collector checks.

## Requirements

- Linux or WSL2
- A shell with access to `/proc`
- Loopback ports `4274` and `4276` available unless overridden
- Rust collector binary from a TinyTop GitHub release, or Rust `1.95.0` or newer to compile locally
- Optional for development and the Bun wizard: Bun matching the repo package manager line, `bun@1.3.11`

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

For an existing local checkout:

```bash
cd /path/to/tinytop
```

## Install Development Dependencies

```bash
./tinytop deps
```

This is only required for Bun/TypeScript development. Apache ECharts is vendored under both `legacy/dashboard/vendor/` and `agent/assets/dashboard/vendor/`; the Rust no-Bun runtime embeds its dashboard assets into the collector binary.

The Rust daemon has its own Cargo workspace under `agent/`.

## Check Ports

The default local ports are:

- `127.0.0.1:4274` - dashboard UI
- `127.0.0.1:4276` - legacy collector API when using split mode

Check live listeners:

```bash
ss -ltnp '( sport = :4274 or sport = :4276 )'
```

Check the local fleet claim:

```bash
cat ~/.config/fleet/ports/tinytop.toml
```

## Start The Dashboard

The foreground command auto-selects the Rust collector/dashboard daemon when a Rust binary or Cargo is available:

```bash
./tinytop start
```

Force a runtime explicitly when needed:

```bash
TINYTOP_RUNTIME=rust ./tinytop start
TINYTOP_RUNTIME=legacy ./tinytop start
```

The legacy Bun path starts:

- the legacy Bun dashboard process on `127.0.0.1:4274`
- the internal legacy Bun collector process on `127.0.0.1:4276`

Open:

```text
http://127.0.0.1:4274
```

## Start Processes Separately

Use this when you want the collector and dashboard supervised as separate foreground processes from one wrapper:

```bash
./tinytop start:split
```

Or run the underlying commands manually in two terminals:

```bash
bun run collector
```

```bash
TINYTOP_DISABLE_WRITER_SPAWN=1 bun run dev
```

If the collector is on a non-default URL:

```bash
HISTORY_WRITER_URL=http://127.0.0.1:4276 bun run dev
```

## Environment Variables

| Variable | Default | Applies to | Description |
| --- | --- | --- | --- |
| `HOST` | `127.0.0.1` | dashboard | Dashboard bind host |
| `PORT` | `4274` | dashboard | Dashboard port |
| `HISTORY_WRITER_HOST` | `127.0.0.1` | collector and dashboard spawn env | Collector bind host; env name retained for compatibility |
| `HISTORY_WRITER_PORT` | `4276` | collector and dashboard proxy URL | Collector port; env name retained for compatibility |
| `HISTORY_WRITER_URL` | unset | dashboard | Full URL for an existing collector; disables auto-spawn |
| `HISTORY_POLL_MS` | `1500` | Rust daemon and legacy Bun collector | Collection interval in milliseconds |
| `TINYTOP_RUNTIME` | `auto` | command center | Runtime selection for `./tinytop start`: `auto`, `rust`, `legacy`, or `bun` |
| `TINYTOP_HISTORY_DB` | `~/.local/share/tinytop/history.sqlite` | Rust daemon and legacy Bun collector | SQLite database path |
| `TINYTOP_DISABLE_WRITER_SPAWN` | unset | dashboard | Set to `1` to require an already-running legacy Bun collector |
| `TINYTOP_PUBLIC_DIR` | unset | Rust daemon | Optional development override for dashboard assets; unset uses embedded assets |
| `XDG_DATA_HOME` | `~/.local/share` | Legacy Bun collector | Base directory for default SQLite path |

## SQLite Location

Default:

```text
~/.local/share/tinytop/history.sqlite
```

Override:

```bash
TINYTOP_HISTORY_DB=/path/to/history.sqlite ./tinytop rust serve
```

SQLite may create sidecar files:

```text
history.sqlite-wal
history.sqlite-shm
```

History retention:

- Automatic SQLite retention is not implemented yet.
- The collector stores raw samples until you manually archive or reset the database.
- The dashboard's recent history window limits what it reads and renders; it does not prune SQLite.

Dashboard settings:

- Browser-local active theme, graph mode, and history range are stored in `localStorage`.
- Rust daemon defaults are stored in SQLite through `/api/settings`.
- Retention and rollup defaults can be saved now; automatic pruning and rollup enforcement are planned next.

## Verify Installation

Run the automated checks:

```bash
./tinytop check
git diff --check
```

Runtime-specific checks are also available:

```bash
bun run check:bun
bun run check:rust
```

If Rust is installed, direct Rust collector checks are:

```bash
cargo test --manifest-path agent/Cargo.toml --workspace
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- collect --json
cargo run --manifest-path agent/Cargo.toml -p tinytop-agent -- serve
```

Check HTTP endpoints:

```bash
curl -fsS http://127.0.0.1:4274/health
curl -fsS http://127.0.0.1:4274/api/version
curl -fsS 'http://127.0.0.1:4274/api/history?limit=3&window_seconds=300'
```

Expected health output:

```text
ok
```

## Upgrade

1. Stop running daemon, dashboard, and collector processes.
2. Pull or apply the new code.
3. Run `./tinytop deps`.
4. Run `./tinytop check`.
5. Start `./tinytop systemd start` or `./tinytop start`.
6. Open the dashboard and confirm the sidebar version line plus History hydration after a browser refresh.

The current schema is created with `CREATE TABLE IF NOT EXISTS` and indexes are created if missing. There is no explicit migration table or automatic retention job yet.

## Reset Local History

Stop the dashboard and collector first, then move the database files aside:

```bash
./tinytop db backup
./tinytop db reset --yes
```

Start TinyTop again. The collector will create a fresh database.

## systemd User Services

Install persistent user services:

```bash
./tinytop systemd install --rust
./tinytop systemd start
```

`./tinytop systemd install` defaults to the Rust daemon. Use
`./tinytop systemd install --bun` only when you explicitly want the legacy Bun
dashboard/collector split.

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
