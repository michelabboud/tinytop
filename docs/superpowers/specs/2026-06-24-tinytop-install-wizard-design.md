# TinyTop Install Wizard - Design Spec

Date: 2026-06-24
Status: approved for implementation
Recorded in: TinyTop 0.1.8
Target implementation: TinyTop 0.1.9

Current status note, 2026-06-25: this spec records the original Bun
writer/dashboard design. TinyTop now defaults to one Rust
collector/dashboard user service, with the legacy Bun split path available only
when explicitly selected.

## Goal

Make TinyTop feel installable by a new user, not just runnable by the author.
The installer must cover the first-run path, day-to-day process control, logs,
SQLite history management, backups, systemd user services, and Bun
installation help.

The command surface should be a single operator-facing front door:

```bash
./tinytop help
./tinytop setup
./tinytop start
./tinytop status
./tinytop logs
./tinytop db stats
```

The front door must work before Bun is installed. Once Bun is available, it can
launch the richer Bun wizard with `bun run setup`.

## Telecode Pattern To Reuse

Telecode's setup flow is useful because it is explicit, idempotent, and
operator-friendly:

- It has a dedicated setup entrypoint, `bun run setup`.
- It checks prerequisites first and prints concrete install guidance.
- It announces long commands before running them.
- It writes configuration idempotently and backs up existing config before
  changes.
- It offers a supervisor choice and gives recovery commands if a supervisor is
  missing.
- It ends with smoke checks and clear next steps.

TinyTop should keep that experience but adapt it to TinyTop's simpler local
dashboard shape. TinyTop does not need Telecode's Telegram, AI runner, or auth
setup complexity.

## Approved Architecture

TinyTop will use a two-layer installer:

1. `./tinytop`: production-grade Bash command center.
2. `bun run setup`: interactive Bun wizard launched by the Bash command center
   when Bun exists.

The Bash layer owns bootstrap and operations. The Bun layer owns typed,
interactive configuration and runtime validation.

```text
fresh shell
  |
  v
./tinytop
  |
  +-- Bun missing -> explain / print / optionally run official Bun installer
  |
  +-- Bun present -> bun install if needed
  |
  +-- setup -> bun run setup
  |
  +-- start/status/logs/db/systemd -> validated shell operations
```

## Bash Command Center

The root `tinytop` script will be portable Bash with no Node or Bun dependency.
It should use ANSI colors by default and disable them when `NO_COLOR` is set or
`--plain` is passed.

### Required Commands

| Command | Purpose |
| --- | --- |
| `help` | Full command reference and examples |
| `setup` | Run bootstrap checks, then launch `bun run setup` |
| `doctor` | Check Bun, dependencies, ports, systemd, SQLite path, and services |
| `install-bun` | Explain, print, or run the official Bun installer |
| `deps` | Run `bun install` |
| `check` | Run `bun run check` and browser build smoke command |
| `start` | Start dashboard in foreground with auto-spawned writer |
| `start:split` | Start writer and dashboard as separate foreground processes |
| `restart` | Restart systemd services if installed, otherwise print foreground commands |
| `stop` | Stop systemd services if installed, otherwise identify matching PIDs |
| `status` | Show ports, services, DB path, process status, and health checks |
| `logs` | Tail systemd logs when installed, otherwise point to foreground mode |
| `monitor` | Periodically print health, sample count, DB size, and current URLs |
| `stats` | Show current snapshot and SQLite sample counts |
| `db stats` | Print SQLite size, sample count, earliest/latest sample |
| `db backup` | Copy SQLite files to a timestamped backup directory |
| `db vacuum` | Run `VACUUM` through Bun/SQLite after services are stopped |
| `db check` | Run `PRAGMA integrity_check` |
| `db reset` | Destructive reset gated by `--yes` |
| `systemd install` | Install user services for writer and dashboard |
| `systemd uninstall` | Disable and remove user services |
| `systemd start` | Start both user services |
| `systemd stop` | Stop both user services |
| `systemd restart` | Restart both user services |
| `systemd status` | Show both user service statuses |
| `systemd logs` | Follow both journals |

### Bash Responsibilities

- Resolve the repo root from the script location.
- Detect Bun with `command -v bun`.
- Detect `systemctl --user` support and whether the user manager is reachable.
- Detect whether dependencies are installed by checking `node_modules`.
- Detect port conflicts on `127.0.0.1:4274` and `127.0.0.1:4276`.
- Detect whether TinyTop systemd user units are installed.
- Resolve the default SQLite path as
  `${XDG_DATA_HOME:-$HOME/.local/share}/tinytop/history.sqlite`, unless
  `TINYTOP_HISTORY_DB` is set.
- Refuse destructive DB reset unless `--yes` is present.
- Never require root for normal setup. Boot persistence should use user-space
  systemd services.

## Bun Setup Wizard

The Bun wizard will live at `src/wizard/index.ts` and be exposed as:

```json
{
  "scripts": {
    "setup": "bun run src/wizard/index.ts"
  }
}
```

The Bash `./tinytop setup` command launches it after checking Bun and running
`bun install` when needed.

### Wizard Steps

| Step | Action |
| --- | --- |
| 1 | Welcome banner, scope, estimated time, Ctrl-C behavior |
| 2 | Prerequisite summary: Bun, Git, systemd user manager, ports, SQLite path |
| 3 | Runtime mode: simple foreground, split foreground, or systemd user services |
| 4 | Port and bind review with defaults `127.0.0.1:4274` and `127.0.0.1:4276` |
| 5 | History database path review and backup directory review |
| 6 | Collection interval review, defaulting to `HISTORY_POLL_MS=1500` |
| 7 | Show generated environment summary; secrets are not relevant today |
| 8 | Run `bun install` if dependencies are missing |
| 9 | Run `bun run check` |
| 10 | Optionally install systemd user services |
| 11 | Optionally start services |
| 12 | Smoke check health endpoints and print dashboard URL |
| 13 | Done banner with docs and next commands |

### Wizard Implementation Notes

- Prefer a small in-repo prompt helper over a dependency unless a dependency
  clearly earns its place.
- If a prompt dependency is added later, vet and document it first.
- Keep each step isolated enough to test with injected input/output and command
  runners, following the Telecode orchestrator pattern.
- The wizard should be idempotent: rerunning it should detect existing services,
  existing dependencies, existing DB path, and current ports.

## Systemd User Services

TinyTop should ship generated or templated user units for two processes:

- `tinytop-writer.service`
- `tinytop-dashboard.service`

The writer owns SQLite and the dashboard reads through the writer API.

Dashboard service requirements:

- `After=tinytop-writer.service`
- `Requires=tinytop-writer.service`
- `Environment=TINYTOP_DISABLE_WRITER_SPAWN=1`
- `ExecStart=<bun> run src/server.ts`

Writer service requirements:

- `ExecStart=<bun> run src/collector-daemon.ts`
- `Restart=on-failure`
- `RestartSec=3`

Both units should:

- Use `WorkingDirectory=<repo root>`.
- Use the actual Bun path found by the installer.
- Avoid `User=` because these are user services.
- Bind to loopback by default.
- Be installed under `~/.config/systemd/user/`.

The installer may offer `loginctl enable-linger "$USER"` as an explanation for
boot persistence, but it should not run privileged commands without explicit
operator action.

## SQLite Operations

The command center must manage the DB without bypassing the writer ownership
model during normal runtime.

Read-only operations can inspect the DB directly when the writer is running:

- `db stats`
- `db check`
- `stats`

Write or rewrite operations should require stopped services:

- `db vacuum`
- `db reset`

Backups must include WAL sidecar files when present:

- `history.sqlite`
- `history.sqlite-wal`
- `history.sqlite-shm`

Backup destination:

```text
~/.local/share/tinytop/backups/YYYYMMDD-HHMMSS/
```

## Error Handling

- Missing Bun: print official install command, docs URLs, and `./tinytop
  install-bun --yes` option.
- Missing curl during Bun install: fail with a package-manager hint.
- Port conflict: show the owning process if available and the env override.
- systemd unavailable: explain foreground and split foreground alternatives.
- Service start failure: print `systemctl --user status ...` and journal
  command.
- DB missing: explain that it is created by the writer on first sample.
- DB integrity failure: fail closed and point at backup/reset commands.

## Testing Strategy

### Bash Tests

Add shell tests that run the command center in a temp home/repo fixture:

- `help` exits 0 and includes setup, systemd, and DB commands.
- `doctor` reports missing Bun without crashing.
- `install-bun --print-only` prints the official command but does not run it.
- `db reset` refuses without `--yes`.
- systemd rendering includes split writer/dashboard units.

### Bun Tests

Add wizard tests around pure helpers and injected command runners:

- prereq detection maps command availability correctly.
- setup launches the wizard from Bash only after Bun exists.
- generated systemd units use the discovered Bun path.
- DB path resolution honors `TINYTOP_HISTORY_DB` and `XDG_DATA_HOME`.
- smoke-check logic handles healthy and unhealthy endpoints.

### Verification Commands

The implementation closeout must run:

```bash
bun test
bun run check
bun build public/app.js --target=browser --outdir=/tmp/tinytop-build-check
./tinytop help
./tinytop doctor
./tinytop systemd render
git diff --check
```

If services are installed during verification, the closeout must report whether
they were left running or stopped.

## Implementation Slices

1. Add the Bash command center with help, Bun detection, doctor, and install-bun.
2. Add systemd unit rendering and systemd install/start/stop/status/logs.
3. Add DB stats/check/backup/vacuum/reset operations.
4. Add the Bun setup wizard entrypoint and connect `./tinytop setup` to it.
5. Add tests for Bash behavior and wizard helpers.
6. Update README, INSTALL, GUIDE, OPERATIONS, ARCHITECTURE, CHANGELOG, and
   PROGRESS.

## Acceptance Criteria

- A fresh user can run `./tinytop help` before installing Bun.
- A fresh user can run `./tinytop setup`, install or locate Bun, install
  dependencies, and reach the dashboard URL.
- The collector can be made persistent with user-space systemd services.
- The dashboard can be restarted without losing the writer-owned SQLite history.
- Refreshing the dashboard after services restart hydrates recent history from
  SQLite.
- DB backup and reset flows are documented and guarded.
- The implementation has tests and fresh verification evidence.
