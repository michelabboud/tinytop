# 0003 - Bash Bootstrap Plus Bun Install Wizard

## Status

Accepted

## Context

TinyTop needs an installer that helps a new user before the project runtime is
available. A Bun-only CLI cannot help if Bun is missing. A Bash-only installer
can bootstrap a fresh shell, but it becomes awkward for interactive validation,
typed helpers, and tests once the project dependencies are available.

The approved installer also needs to manage two local processes: the collector
writer and the dashboard. The writer owns SQLite, while the dashboard reads
through the writer API. Persistent operation should use user-space systemd
services so TinyTop remains a local, non-root workstation tool.

## Decision

TinyTop will use a two-layer installer:

1. A root `tinytop` Bash command center for bootstrap, help, Bun installation
   guidance, process control, DB operations, and systemd user service commands.
2. A Bun setup wizard, exposed as `bun run setup`, for richer interactive setup
   after Bun is available.

The Bash `./tinytop setup` command will detect Bun first. If Bun is missing, it
prints or optionally runs the official Bun installer. If Bun exists, it can run
`bun install` and then launch `bun run setup`.

Systemd user services will run the writer and dashboard separately. The
dashboard service will set `TINYTOP_DISABLE_WRITER_SPAWN=1` and depend on the
writer service.

## Alternatives Rejected

### Bun-only CLI

Rejected because it cannot run before Bun is installed, which is exactly when a
new user needs the most help.

### Bash-only Wizard

Rejected because large interactive flows, validation helpers, endpoint smoke
checks, and unit tests are easier to keep maintainable in Bun/TypeScript once
the runtime exists.

### PM2 As The Primary Supervisor

Rejected for TinyTop because systemd user services are already present on the
target Linux/WSL-style environment, avoid an extra Node package, and keep the
service model close to the OS. PM2 can be reconsidered later as an optional
adapter if users ask for it.

### Documentation-only Install

Rejected because the project is intended to behave like a real repo a new user
can install and operate. Static docs do not check ports, dependencies, services,
or DB health.

## Consequences

- `./tinytop help` works on a fresh machine before Bun is installed.
- The richer `bun run setup` wizard can still reuse TinyTop modules and tests.
- The installer has a small Bash surface that must be tested separately.
- The systemd integration keeps the writer and dashboard lifecycle explicit.
- Destructive DB management must be carefully guarded in the Bash layer.
