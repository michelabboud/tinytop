# Plan: Sensors Recording (thermals, fans, GPU, power) — v1

**Status:** APPROVED for planning 2026-08-16 (Michel: "do the plans, we will wait
for Ari's tanks to fill"). **Execution parked** until token refill (Ari reset
Wed 2026-08-20 ~06:32; Fable reset Fri 2026-08-21 04:59).
**Author:** Fable (design). **Intended executors:** sonnet-tier lanes for
implementation, codex (ari-sol xhigh) for review, Fable gatekeeps merges.
This document is self-contained: an executor without the design conversation
must be able to run it end to end.

## Goal

TinyTop records and charts **all hardware sensors**: CPU temps, disk temps,
motherboard/ACPI zones, **GPU temps/PWM/power**, and **fan speeds** — with the
same SQLite history + rollups + retention the existing metrics get.
(Michel's ask, verbatim, idea 0045.)

## Source of truth: `/sys/class/hwmon` (read directly, no subprocess)

One uniform kernel tree. Per `hwmon*/`: `name` (chip), `temp<N>_input/_label/
_max/_crit`, `fan<N>_input/_label`, `pwm<N>` (0–255 → percent), `power<N>_average`
(µW → W). Values in millidegrees / RPM / µW — normalize in the collector.

- Covers: coretemp (CPU), amdgpu (GPU temp+pwm+power), nvme, acpitz/platform
  chips (motherboard), applesmc (Mac fans+temps), drivetemp (SATA disk temps).
- **drivetemp is a kernel module** and may not be loaded: document
  `modprobe drivetemp` + `/etc/modules-load.d/drivetemp.conf` in INSTALL.md;
  `./tinytop doctor` gains a check ("SATA disks present but no drivetemp hwmon").
- **No root required** — hwmon is world-readable. Never shell out to `sensors`.
- **v1 scope: temp, fan, pwm, power. Voltages/currents deferred** (decided:
  low value, high noise; revisit on request).

## Hard repo invariants the executor MUST respect (from CLAUDE.md)

1. **Two runtimes, one contract**: every snapshot field / API route lands in
   BOTH Rust (`agent/`) and Bun (`src/`, `legacy/`) with tests in both.
2. **Single dashboard tree** `agent/assets/dashboard/` — edit once, then
   REBUILD the Rust agent (embedded assets; stale-binary trap).
3. Gates before any "done": `bun run check` (= check:bun + check:rust).

## Contract changes (additive only — no breaking changes)

- `SystemSnapshot` += `sensors: SensorReading[]`:
  `{ chip: string, kind: "temp"|"fan"|"pwm"|"power", label: string,
     value: number, max?: number, crit?: number }`
  (dynamic per host; empty array on WSL — see below).
- SQLite (via SQLx migration, mirroring existing tables):
  - `sensor_dim (id INTEGER PK, chip TEXT, kind TEXT, label TEXT, UNIQUE(chip,kind,label))`
  - `sensor_samples (ts_ms INTEGER, sensor_id INTEGER, value REAL)`
  - `sensor_rollups_1m (minute_ms, sensor_id, min, avg, max, n)`
  - pruned by the SAME saved retention/rollup windows as existing history.
- API (new routes; legacy routes untouched):
  - `GET /api/sensors` — current readings + discovery (chips, kinds, thresholds)
  - `GET /api/sensors/points?since_ms&until_ms` — rollup-backed series
    (raw samples for the Live/15m/1h ranges, rollups beyond — same split as
    `/api/history` vs `/api/history/points`).
- Dashboard: new "Sensors" section — temperature chart (per-sensor toggleable
  series), fan RPM chart, live grid colored against `max`/`crit`. Follow the
  existing section/series-toggle patterns in `app.js`; respect theme + embed mode.

## WSL degradation (tinytop's home box has NO sensors)

WSL2 exposes an empty/absent hwmon tree. Required behavior, with tests:
collector returns `sensors: []`; UI hides the Sensors section when the array is
empty AND `/api/sensors` reports no chips; no errors, no placeholder noise.

## Execution order (each step = commit; run gates at each ✋)

1. **Types** — `agent/crates/tinytop-types` `SensorReading` + snapshot field;
   TS types in `src/` to match. Serde JSON must match the TS contract exactly.
2. **Rust collector** — hwmon walker in `tinytop-agent` collect path (unit tests
   against a fixture tree under `tests/fixtures/hwmon/`; do NOT read the live
   host in tests). Normalize units; skip unreadable files silently but count
   them (log at debug once, not per-poll). ✋
3. **Bun collector parity** — same walker in `legacy/bun-collector.ts` +
   `src/collector.ts`, same fixture tests in `tests/`. ✋
4. **Store** — migration + write path + 1m rollups + prune, in `tinytop-store`
   and the Bun `src/history-store.ts`. Test: retention prunes sensor tables. ✋
5. **API** — both runtimes, both routes. Contract test: Rust and Bun responses
   byte-compatible for the same fixture input. ✋
6. **Dashboard** — one tree, then `./tinytop rust build` (stale-binary trap).
   Manual check in browser (`bun run dev` first, then Rust serve). ✋
7. **Docs** — README (feature + drivetemp note), ARCHITECTURE (data flow),
   INSTALL (drivetemp), CHANGELOG, PROGRESS; **ADR: dynamic sensor schema**
   (dim/samples split vs wide columns; chosen for per-host variability).
8. **Gate + ship** — `bun run check` green, bump VERSION, commit, tag, push.

## GitHub release builds (Michel's addition, 2026-08-16)

Add a GitHub Actions release workflow (`.github/workflows/release.yml`),
triggered on tag push, building **two release artifacts** of `tinytop-agent`:

- `tinytop-agent-x86_64-unknown-linux-gnu.tar.gz` (ubuntu runner) — serves the
  whole x86_64 room (workstation/WSL, trashcan, goat, sheep — all measured x86_64).
- `tinytop-agent-x86_64-pc-windows-msvc.zip` (windows runner) — TinyTop's native
  Windows runtime (`tinytop.ps1` install path).

*Assumption to confirm with Michel:* "two releases" = Linux + Windows (the
product's two supported platforms). If he meant Linux x86_64 + **aarch64**
instead, swap the second target for `aarch64-unknown-linux-gnu` (cross or ARM
runner) — the workflow shape is identical, and a third target is one matrix
line either way. Release notes via `gh release create` from CHANGELOG; binaries
checksummed (`sha256sum` asset). `./tinytop rust install-binary` and
`tinytop.ps1` then install from the release instead of a local build — which is
exactly how the no-toolchain trashcan stays honest.

## Deployment phase (separate task, after the above ships)

- Build release binary on the workstation; **ship binary to trashcan**
  (`./tinytop rust install-binary` path — no Rust toolchain on trashcan, his
  ruling). `./tinytop systemd install --rust`, loopback :4274, port claim note.
- trashcan extras: `modprobe drivetemp` for the SATA-attached Apple SSD;
  applesmc fan + temps appear automatically; amdgpu ×2 appear automatically.
- goat/sheep: optional, same recipe, AFTER trashcan proves clean.
- Verify per machine: sensors section shows CPU+disk+fan (+GPU on trashcan),
  values sane vs `sensors` output, history accumulates across a service restart.

## Escalation clause (put in every lane brief)

"You are on the lowest model expected to handle this. If the task exceeds you —
stuck after a real attempt, looping, or you'd be guessing — reply
`ESCALATE: <what is beyond you, what you tried>` instead of continuing."

## Out of scope (do not build)

Voltages/currents · alerting/notifications · cross-machine aggregation ·
public binding · netdata-style per-second granularity (existing poll cadence).
