# 0026 - Schema v5: thermal sensor storage, and the opt-in thermal collector

## Status

**Accepted (2026-08-30; T17/T17b)** — Task 17 of plan `docs/plans/2026-08-16-sensors-recording-plan.md` (its **first slice**; see that plan's 2026-08-30 amendment). Decisions 1, 2 and 6 are the three questions the T17 fact sheet §D left open; **Michel ruled 05:3xZ — "go by your recommendation"** — so all three stand as written below. Extends ADR 0021 (cadence classes), ADR 0024/0025 (typed history, the interning pattern) and ADR 0015 (the opt-in settings block shape).

## Context

Michel's order, 2026-08-30 04:1xZ, verbatim: **"make it opt in option"** — TinyTop grows core thermals, shipped **disabled by default** and switched on per host. Task 17 is the **thermal slice** of the 2026-08-16 sensors plan (approved for planning, never built), not a fork: fans, PWM, power and disk temperatures stay in that plan's later slices.

Measured read-only across the fleet on 2026-08-30 04:0xZ (`docs/fleet/tinytop/2026-08-30-t17-fact-sheet.md` in the Fabulous repository — §A is the fixture truth):

- **sheep** (Intel N97, kernel 6.8): `hwmon1` = `coretemp` with `temp1` **Package id 0** 54000 m°C and `temp2..temp5` **Core 0..3**, each `_max` 105000 / `_crit` 105000. `hwmon0` = `nvme`.
- **trashcan** (Xeon E5-1620 v2, kernel 7.0): `hwmon0` = `coretemp`, same label shape, `_max` 91000 / `_crit` 105000. **`hwmon1` has an empty `name`.** `hwmon2`/`hwmon3` = `amdgpu` edge, **no `_max`**.
- **The `hwmonN` index is not stable across hosts and is not guaranteed across boots** — `coretemp` is `hwmon1` on sheep and `hwmon0` on trashcan. Any identity derived from the directory index forks a sensor's history at a reboot.
- **Thresholds lie.** sheep's `nvme temp2_max` reads **65261850** m°C (65,261 °C — the driver's "no limit" sentinel); `amdgpu` edge has no `_max` at all; sheep's `nvme temp4` has neither `_max` nor `_crit`.
- **This box (WSL2) has no `/sys/class/hwmon` at all** and no `thermal_zone*` — thermal data is honestly absent, not zero.
- **The GPU's own hwmon temperature already ships** as `gpus[].temperatureC` (0.5.4): `gpu/linux.rs:532-533` reads `<drm device>/hwmon/*/temp1_input`. A second, global hwmon walk would report the same silicon twice.

The store's current shape (read at `80694ae`): `SCHEMA_VERSION = 4` (`migration.rs:15`); `CREATE_SCHEMA_V4_SQL` is seven statement groups applied in one transaction (`migration.rs:586-592`, `apply_schema_groups:914`); `gpu_adapters` / `gpu_samples` are the newest storage pattern (`migration.rs:548-566`); `DashboardSettings.otel` is the opt-in block template (`lib.rs:74`, absent-key rule `:170-193`, `changed_keys` `:220-221`, `validate` `:333`); `HistoryCoverage.otel: Option<HistoryOtelCoverage>` (`lib.rs:357`, `:363-371`) is the coverage template; `prune_gpu_history` sits beside `prune_raw_history` at the L1 horizon (`maintenance.rs:193`, `:201`).

## Decision

1. **Thermal history is stored as an interned dimension plus a raw sample table — `sensor_dim` + `sensor_samples` — not as a column on `metric_samples`.** Schema v5 is **purely additive**: two `CREATE TABLE` statements and `PRAGMA user_version = 5`, appended to the v4 groups as `CREATE_SENSOR_TABLES_V5_SQL`. No table is rebuilt, so v4→v5 needs no row-count guard and no pre-image (ADR 0023's guard exists for rebuilds); a fresh file is created directly in v5 shape.

   ```sql
   CREATE TABLE IF NOT EXISTS sensor_dim (
     sensor_id INTEGER PRIMARY KEY,
     stable_id TEXT NOT NULL UNIQUE,
     chip TEXT NOT NULL,
     kind TEXT NOT NULL,
     label TEXT NOT NULL,
     max_c REAL,
     crit_c REAL,
     first_seen_ms INTEGER NOT NULL,
     last_seen_ms INTEGER NOT NULL
   );

   CREATE TABLE IF NOT EXISTS sensor_samples (
     captured_at_ms INTEGER NOT NULL,
     sensor_id INTEGER NOT NULL REFERENCES sensor_dim(sensor_id),
     value REAL NOT NULL,
     PRIMARY KEY (captured_at_ms, sensor_id)
   ) WITHOUT ROWID;
   ```

   `stable_id` = `hwmon-<chip>-<k>-temp<N>`, where `<chip>` is the `name` file's content verbatim, `<k>` is the 0-based count of earlier chips with the same `name` in sorted-path order (the ADR 0025 duplicate-tuple rule, so a dual-socket box keeps two `coretemp` chips apart), and `temp<N>` is the sysfs attribute index. **The `hwmonN` directory index never appears in `stable_id`** — it is not stable (Context). The store caches `stable_id → sensor_id` primed at `connect`, exactly as `gpu_adapters` does, so a tick costs no lookup; `last_seen_ms` is written **at most once a minute per sensor** (ADR 0025 decision 2's rule, same reason). `max_c` / `crit_c` live in the dimension, not on every row, and are updated in the same once-a-minute write when the kernel's value changes.

   *Rejected:* **one nullable package column on `metric_samples`** (the cheap 08-30 proposal) — it cannot express `Package id 0` **plus** `Core 0..3`, which is the feature Michel asked for; it hardcodes a sensor count into the schema on hosts whose sensor set is discovered at runtime; and when the parent plan's fans/power/disk slices land, the column becomes a legacy path needing a migration off itself, leaving two storage idioms in one store. *Rejected:* wide per-core columns (`core0_c … core15_c`) — same rigidity, worse. *Rejected:* a `sensor_rollups_1m` tier in this slice — ADR 0025 decision 3's ruling stands verbatim for a first sensor slice: **chart it first**, rollups are backlog.

2. **One flag, not per-source: `thermal.enabled`, default `false`, a top-level settings block in the `otel` mould.** New `tinytop-store/src/thermal_settings.rs` beside `otel_settings.rs`: `ThermalSettings { enabled: bool, extra_chips: Vec<String> }`, `Default` → `{ enabled: false, extra_chips: [] }`. `DashboardSettings` gains `pub thermal: ThermalSettings`; **an absent `thermal` key on import keeps the persisted block** (`lib.rs:170-193`'s rule, `has_thermal`); `changed_keys` gains `"thermal"` (`lib.rs:220-221`'s shape); `validate` calls `self.thermal.validate()?`. **Disabled means the collector does not touch hwmon at all** — no `read_dir`, no `sensors` array in the snapshot, no rows, no dimension entries (the ADR 0022 detection-gate discipline; the cost of the disabled path is zero by construction, and the lane proves it with a test asserting a fixture tree is never opened).

   *Rejected:* per-source flags (`thermal.cpu.enabled`) — T17 has exactly **one** source, so per-source granularity is configuration for a distinction that does not yet exist; the parent plan's later slices get their own top-level flags (`fans.enabled`, …) beside this one. *Rejected:* nesting as `sensors.thermal.enabled` — the `otel` template is top-level, and a nesting level bought for hypothetical siblings is churn now for tidiness later.

3. **The wire contract is the parent plan's, verbatim, narrowed to temperatures.** `SystemSnapshot` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub sensors: Vec<SensorReading>` (`tinytop-types/src/lib.rs:209-223`, the `gpus` field at `:221-222` is the shape), where `SensorReading { chip: String, kind: SensorKind, label: String, value: f64, max: Option<f64>, crit: Option<f64> }` and `SensorKind` serializes as `"temp" | "fan" | "pwm" | "power"` — **T17 emits only `Temp`**, and the other variants exist because the enum is the parent plan's contract, each already reachable from that plan's later slices. Adopting the parent contract now means fans and power are pure additions with no rename and no migration.

4. **Thresholds are reported only when present AND sane; a sentinel is absent, never a number.** A `_max` / `_crit` is accepted only when it parses and `0.0 < value_c <= 200.0`; otherwise the field is `None`. sheep's `65261850` m°C and `amdgpu`'s missing `_max` are both **absent**, and the dashboard must render an absent threshold as no threshold — never as a gauge maximum, never as `0`. A chip whose `name` file is empty or unreadable (trashcan `hwmon1`) is **skipped silently and counted**, never labelled `""` and never an error. A `temp<N>_input` without a `temp<N>_label` falls back to the literal `temp<N>` as its label.

5. **T17 reads CPU chips only, by an allow-list, so the GPU is never double-reported.** The scan accepts a chip whose `name` is `coretemp` or `k10temp` — the two drivers that expose `Package id N` / `Core N` semantics — plus any chip named in `thermal.extra_chips` (each entry validated against `^[a-z0-9_]{1,32}$`, so an ARM or Apple host can opt its chip in without a rebuild, per rule 3: no hardcoded fleet assumptions). `amdgpu` (already `gpus[].temperatureC`), `nvme` (disk temps — the parent plan's slice) and ACPI zones are therefore **out of T17 by construction**, not by a subtraction the reader has to verify. The chip name is stored and displayed **verbatim**; the code never assumes Intel.

6. **The dashboard surface is its own `section.panel`, hidden when absent — not a fifth overview gauge and not a drawer.** ADR 0025 decision 6 measured the constraint: the overview grid is `repeat(4, …)` and a fifth gauge wraps onto its own row, which is why the GPU panel became a `section.panel` after the grid. Thermals follow it: a panel after the GPU panel, one row per sensor, the value bar coloured against `max`/`crit` when those are present and uncoloured when they are not (decision 4), hidden entirely when `snapshot.sensors` is empty or absent — which is the Bun runtime always, and WSL2 always. Plus a settings group in the T11 `otel` group's shape. No collapsible drawer.

7. **`sensor_samples` are raw-tier rows pruned at the L1 horizon**, with `gpu_samples`: a `prune_sensor_history` step beside `maintenance.rs:201`'s `prune_gpu_history`, `MaintenanceReport.sensor_rows`, and `wouldDelete.sensorSampleRows` in the import dry-run (server-computed, the `gpuSampleRows` rule) so an L1 shrink never deletes thermal history silently; the dashboard shows the line only when non-zero and treats an absent field as 0 (a 0.5.x daemon behind a newer page). `HistoryCoverage` gains `thermal: Option<HistoryThermalCoverage> { enabled: bool, sensor_count: i64, oldest_captured_at_ms: Option<i64>, newest_captured_at_ms: Option<i64> }` (the `otel` field's `Option` + `skip_serializing_if` shape at `lib.rs:357`), and `db stats` gains `sensorCount` / `sensorSampleCount` — **presence and counts only, never a sensor's value**.

8. **WSL2 and every other sensorless host degrade to honest absence, with a test.** No hwmon tree → `sensors: []` → the field is skipped on the wire → no dimension rows, no samples, panel hidden. This is asserted against a fixture tree, never against the live host; no test in this lane reads `/sys` or `~/.local/share/tinytop/`.

## Alternatives rejected (summary)

- Shelling out to `sensors` / `lm-sensors`, or any subprocess — ADR 0012, and the parent plan says hwmon is world-readable so no root and no child process is needed.
- `/sys/class/thermal/thermal_zone*` as the source — it exists on sheep and trashcan (`x86_pkg_temp`) but carries no per-core detail, no labels and no thresholds; hwmon is the superset. (WSL2 has neither.)
- A rollup tier, an OTel thermal instrument, a separate thermal retention setting, alerting on `crit` — backlog; T17 charts it first.
- Enabling by default on hosts where sensors exist — Michel's order is explicit and the privacy/cost default is OFF.

## Consequences

- **Storage, measured from the shape, not asserted:** ≈ 24 B/row payload in a `WITHOUT ROWID` PK b-tree × 5 sensors on a 4-core Intel box × the fast tick (1.5 s ≈ 57.6 k ticks/day) ≈ **7 MB/day**, flat at the L1 horizon, and **exactly zero** while `thermal.enabled` is false. The lane measures the real per-row cost with `dbstat` at its gate (the ADR 0024 discipline) rather than trusting this estimate.
- v4→v5 is two `CREATE TABLE`s: milliseconds on any file, no rebuild, no data at risk. A v5 file opened by a 0.5.4 daemon fails the version check as designed (`migration.rs:900`).
- `sensors[]` is additive and absent when empty, so the Bun runtime, the shared dashboard and `docs/guides/API.md` stay valid; the Bun runtime gains **no** collector in this slice and hides the panel.
- The parent sensors plan's remaining slices (fans, PWM, power, disk temps via `drivetemp`) become pure additions: a new `SensorKind` value already in the enum, a new chip allow-list entry, a new opt-in flag — **no schema change and no migration.**
- `docs/guides/API.md`, `README.md`, `ARCHITECTURE.md` and `INSTALL.md` gain the thermal surface; the parent plan's step 7 ("ADR: dynamic sensor schema") is satisfied by this ADR.
