# TinyTop Install Wizard Implementation Plan

> Current status note, 2026-06-25: this is a historical `0.1.9` implementation
> plan. Persistent installs now default to the Rust `tinytop.service`
> collector/dashboard daemon; the Bun split dashboard/collector path remains
> available only when explicitly requested with `--bun`.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the real Telecode-style TinyTop install and operations flow: a zero-dependency `./tinytop` Bash command center that can bootstrap Bun and then launch a Bun setup wizard.

**Architecture:** `./tinytop` is the front door for help, bootstrap, process control, systemd user services, logs, DB maintenance, and handoff to Bun. `src/wizard/index.ts` implements the interactive setup wizard with injectable dependencies. `src/ops.ts` owns SQLite maintenance helpers used by the Bash command center.

**Tech Stack:** Bash, Bun, TypeScript, Bun test, SQLite through `bun:sqlite`, systemd user services.

## Global Constraints

- `./tinytop help` must work before Bun is installed.
- `./tinytop setup` must detect missing Bun, explain installation, and launch `bun run setup` once Bun exists.
- Persistent mode uses systemd user services: `tinytop-writer.service` and `tinytop-dashboard.service`.
- Dashboard service must set `TINYTOP_DISABLE_WRITER_SPAWN=1` and depend on the writer service.
- SQLite writes remain owned by the writer during normal runtime; destructive DB maintenance is guarded.
- No new dependency unless it is vetted and documented.
- Version target is `0.1.9`.

---

### Task 1: Bash Command Center Core

**Files:**
- Create: `tinytop`
- Test: `tests/tinytop-script.test.ts`

**Interfaces:**
- Produces: `./tinytop help`, `./tinytop install-bun --print-only`, `./tinytop doctor`, `./tinytop setup`.
- Consumes: repo scripts in `package.json`.

- [ ] **Step 1: Write failing tests for help, Bun guidance, doctor, and guarded reset.**
- [ ] **Step 2: Run `bun test tests/tinytop-script.test.ts` and verify the command is missing.**
- [ ] **Step 3: Implement `tinytop` with banner, ANSI controls, command dispatch, Bun detection, setup handoff, and guarded DB reset shell path.**
- [ ] **Step 4: Run `bun test tests/tinytop-script.test.ts` and verify the command behavior.**

### Task 2: SQLite Operations Helper

**Files:**
- Create: `src/ops.ts`
- Test: `tests/ops.test.ts`

**Interfaces:**
- Produces: `resolveHistoryDbPath`, `readHistoryDbStats`, `checkHistoryDb`, `vacuumHistoryDb`, and an `ops.ts` CLI.
- Consumes: `bun:sqlite` and the existing `metric_samples` table.

- [ ] **Step 1: Write failing tests for DB path resolution, stats, integrity check, and missing DB handling.**
- [ ] **Step 2: Run `bun test tests/ops.test.ts` and verify the helper is missing.**
- [ ] **Step 3: Implement the SQLite helper and CLI commands used by `./tinytop db ...` and `./tinytop stats`.**
- [ ] **Step 4: Run `bun test tests/ops.test.ts` and verify DB behavior.**

### Task 3: Systemd User Services

**Files:**
- Modify: `tinytop`
- Test: `tests/tinytop-script.test.ts`

**Interfaces:**
- Produces: `./tinytop systemd render|install|uninstall|start|stop|restart|status|logs`.
- Consumes: `command -v bun`, repo root, `systemctl --user`, `journalctl --user`.

- [ ] **Step 1: Add failing tests for `systemd render` output.**
- [ ] **Step 2: Run `bun test tests/tinytop-script.test.ts` and verify render is absent.**
- [ ] **Step 3: Implement split writer/dashboard units and user-service command wrappers.**
- [ ] **Step 4: Run the script tests and `./tinytop systemd render`.**

### Task 4: Bun Setup Wizard

**Files:**
- Create: `src/wizard/index.ts`
- Test: `tests/wizard.test.ts`
- Modify: `package.json`
- Modify: `src/runtime.d.ts`

**Interfaces:**
- Produces: `runWizard`, `buildSetupSummary`, `main`.
- Consumes: `./tinytop` command center through shell command execution.

- [ ] **Step 1: Write failing tests for noninteractive wizard runs with injected answers and command runner.**
- [ ] **Step 2: Run `bun test tests/wizard.test.ts` and verify the wizard is missing.**
- [ ] **Step 3: Implement prompt helpers, idempotent command flow, and `bun run setup` entrypoint.**
- [ ] **Step 4: Run wizard tests and `bun run setup -- --non-interactive --skip-checks`.**

### Task 5: Integration, Docs, Version

**Files:**
- Modify: `README.md`, `INSTALL.md`, `GUIDE.md`, `ARCHITECTURE.md`, `CHANGELOG.md`, `PROGRESS.md`, `docs/guides/OPERATIONS.md`
- Modify: `VERSION`, `package.json`

**Interfaces:**
- Produces: final user documentation for the implemented command center and wizard.

- [ ] **Step 1: Update docs to replace design-status language with real usage.**
- [ ] **Step 2: Bump version to `0.1.9`.**
- [ ] **Step 3: Run `bun test`, `bun run check`, browser build, `./tinytop help`, `./tinytop doctor`, `./tinytop systemd render`, and `git diff --check`.**
- [ ] **Step 4: Commit and tag `v0.1.9`.**

## Self-Review

- Spec coverage: the plan covers Bash bootstrap, Bun wizard handoff, systemd services, DB maintenance, tests, docs, and versioning.
- Placeholder scan: no open placeholders; each task has concrete files and verification commands.
- Type consistency: `resolveHistoryDbPath`, `readHistoryDbStats`, `checkHistoryDb`, `vacuumHistoryDb`, `runWizard`, and `buildSetupSummary` are the stable implementation names used across tasks.
