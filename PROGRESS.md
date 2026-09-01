# Progress

## Current Version

- Version: `0.11.0`
- Date: 2026-09-02
- Status: Phase 5 IN PROGRESS. **0.11.0** (ADR 0035) moves seven reference blocks off the dashboard
  into a new read-only **Settings → Info** tab (Coverage · Tiers · Services · Events): the coverage
  stats, the L1–L4 span cards, the archive/OTel/thermal service lines and the event log. His
  principle, verbatim: *"the idea of dashboard is to see important information in a glance"* — none
  of those answer a question you have at a glance, and they held the space directly under the primary
  chart to report that a service is off. Two things deliberately stayed: the chart's own scrubbed
  readout, and the **disk-pressure alert** — `describeDiskCoverage` yields reference material when
  healthy and a warning when not, so the reading is split by severity and the banner now renders only
  under pressure, where its rarity is itself the signal. Not one renderer changed; the blocks kept
  their ids. **0.10.1** fixed an unclosed `#settings-panel-advanced` shipped in 0.9.0 — the parser
  silently re-parented, so the **Thermals tab rendered an empty dialog** and the dialog's action
  buttons could land below its bottom edge — plus a missing `min-height: 0` on the scrolling body.
  **0.10.0** (ADR 0034) makes the settings save **refuse a form it
  cannot read**, and turns the retention ladder into a list of tiers. The save reads 48 controls
  through an id-based cache taken at load; a *detached* input still answers `.value` with what it
  held when it left the document, and a missing one takes a fallback default — so a save wrote data
  the user was not looking at, silently. 0.9.0 answered that with a convention plus a source-grep
  test; this replaces both with a precondition the save checks (`isConnected === true`, per control,
  by name) and a manifest that cannot drift, because a test compares its `elements.*` set to the
  collect function's in both directions. Proven end to end: with the Archive sub-panel removed from
  the document the save produced zero `PUT`s and named all four unreadable controls. The ladder is
  now four rows of `badge · resolution · on-off · span` on shared explicit tracks, replacing 0.9.0's
  two-column grid, which fixed the pairing but still read as eight unrelated boxes. **Also fixed: the
  four crate versions and both wrapper scripts were left at 0.8.2 when 0.9.0 shipped**, so
  `env!("CARGO_PKG_VERSION")` — which stamps the OTel `service.version` and every exported settings
  document — misreported the build while `/api/version` was correct; a new test now asserts every
  restatement of the version agrees with the `VERSION` file.
- **0.9.0** was the settings shell redesign (ADR 0033) — Michel's four
  remaining instructions after using 0.8.1. A panel with more content than a short viewport can hold
  now declares sub-groups and shows a secondary tab row: General → Browser · Daemon · Thresholds ·
  Display, History → Tiers · Archive · Disk, Advanced → Export · Document, Metrics → the six
  `METRIC_REGISTRY` families built at runtime (so the tabs cannot drift from the metrics they
  select); Thermals keeps none because it already fits. **All 16 tab/sub-tab stops measure overflow 0
  at a 720 px window, including with a help paragraph open** — General and Advanced both scrolled
  there before. History's tier rows moved from `auto-fit` to two fixed columns, so each tier's enable
  switch pairs with its own days field by construction rather than by rendered width; that pairing was
  the substance of "looks bad" and is verified by geometry. The two tab rows are separate keyboard
  scopes with per-parent memory, and a sub-panel is hidden rather than unmounted — proven end to end
  by editing four fields across three sub-tabs, parking on a fourth, and reading the captured PUT
  body, because `collectDaemonSettingsFromForm` reads through the id-based `elements` cache and a
  detached input still answers `.value`. Eight groups gained a `(?)` expanding real DOM text (one
  paragraph per group, what it changes and what it costs); four deliberately did not, because they
  already carry visible warnings. Supersedes ADR 0027's single-tablist clause and ADR 0029's
  one-column OTel rule.
- **0.8.2** deleted the second, competing switch implementation: all 13 toggles had been painting two
  knobs 4.16 px apart because the two rules drew the knob with different pseudo-elements and so could
  not override each other (ADR 0032; measured 13/13 → 0/13 in a real browser).
- **0.8.0 / 0.7.3 / 0.7.2** closed the three settings-correctness defects (S1/S2/S3): `collect`
  ignored its target database's persisted settings (D1); the client validator disagreed with the
  server about *which rule fires* for a bad thermal chip list, not merely about wording (D3); and a
  settings write did not reach the collector until the next tick, whose obvious fix would have
  introduced a stale write-back race (D2, ADR 0031). **0.8.1** then replaced S1's regression test,
  which was false-green on any host with fewer than four processes.
- Superseded status (0.7.1): Phase 5 (cadence classes + GPU + sensors, plans
  `docs/plans/2026-08-29-cadence-classes-and-gpu-plan.md` and
  `docs/plans/2026-08-28-tiered-history-ladder/`, ADRs 0021–0030) IN PROGRESS. **0.7.1** fixed three
  defects Michel found while using 0.7.0: the settings dialog dismissed itself on a tab switch (a
  content-sized dialog resized on mousedown, so the mouseup landed on the backdrop and `click` — which
  targets the common ancestor — hit the dialog), tabs that did not fit the now-fixed box were
  redesigned rather than left to scroll, and a historical timeline was being pushed sideways and
  silently evicted by live samples (ADR 0029, ADR 0030). T18 + T18b landed as
  0.7.0: the thirteen OTel instruments become one `METRIC_REGISTRY` the daemon builds from and
  `GET /api/otel/metrics` serves (read-only, `no-store`, and deliberately carrying neither `endpoint`
  nor `headersEnvVar`), with selection stored as the DISABLED set so a metric added later ships ON
  rather than silently off, and an unknown well-formed name preserved inert for cross-version fleet
  config. The cost lever ADR 0027 claims is now MEASURED rather than asserted: 98 series across 13
  metrics with everything on, 14 across 11 with `system.filesystem.*` off — 84 removed, 85.7 %, and
  the disabled pair absent from the request entirely rather than zero-valued. T18b made the settings
  dialog five tabs with every panel permanently mounted (so switching a tab can never drop a field
  the user already filled in), capability-driven tab visibility, and a validate-then-apply Advanced
  document whose only authority is the server's dry-run. On Michel's instruction the dialog's
  booleans then became switches (ADR 0028) — the native input restyled in place so the thirteen
  static fields and the runtime-built metric rows cannot drift — with the `float: right` legend
  button, the magic `margin-top` on the metric row and the unit that occupied its own line all
  fixed. Both lanes were git-read-only by design and were committed by the orchestrator after every
  claim was validated at source; T18b's ESCALATE was a containment artifact (a lane cannot bind a
  socket) caused by a brief that demanded a server-starting gate, not a defect. Next = the settings
  correctness plan (`docs/plans/2026-09-01-settings-correctness-plan.md`, S1–S3) AT MICHEL'S GATE
  with three open questions; T16 (Windows PDH + DXGI, macOS IOKit) still plan-only until his go.

- Superseded status (0.6.0): Phase 5 (cadence classes + GPU + sensors, plans
  `docs/plans/2026-08-29-cadence-classes-and-gpu-plan.md` and
  `docs/plans/2026-08-28-tiered-history-ladder/`, ADRs 0021–0027) IN PROGRESS. T17 landed as 0.6.0:
  opt-in CPU thermals (`coretemp`/`k10temp` plus `thermal.extraChips`, which refuses `amdgpu`/`i915`/
  `nvme`) and schema v5 (ADR 0026: `sensor_dim` + `sensor_samples`, purely additive, 0–1 ms), with
  `stable_id` deliberately free of the unstable `hwmonN` index — `coretemp` is `hwmon1` on sheep and
  `hwmon0` on trashcan and both yield `hwmon-coretemp-0-temp1..5`. Thresholds are reported only when
  present AND sane (`0 < t <= 200`), so sheep's 65261850 m°C `nvme` sentinel is absent, and the
  dashboard renders an absent threshold as no bar at all. T17-fix1 fixed the one user-facing defect
  luna's blind review found (`db stats` and import planning died on an un-migrated database) and its
  demanded audit proved the same defect class was already live for the GPU tables on a v3 file; T17b
  added the Thermals panel, settings group and coverage row (reviewed first-hand: 0 HIGH, 1 MED, 3
  LOW). Hardware-proven on sheep (5 `coretemp`, 105/105) and trashcan (5 at 91/105, two `amdgpu`
  chips correctly excluded, unnamed `hwmon1` skipped, `userVersion 5`, 35 sensor rows); `strace`
  recorded zero `/sys/class/hwmon` opens while disabled on both. Rendered-page check passed in both
  themes against sheep's live sensors through an ssh tunnel. Next = T18 + T18b (ADR 0027: tabbed
  settings and per-metric OTel export selection stored as a DISABLED set — both briefs written and
  ready to dispatch); T16 (Windows PDH + DXGI, macOS IOKit) still plan-only until Michel's go.

## Backlog

- **`sensor_dim.stable_id` still forks when two SAME-NAME chips swap sysfs order across a reboot (T17, luna finding 4, validated)** — the `<k>` disambiguator is derived from scan ORDER, so on a dual-socket box a reboot that reorders the two `coretemp` paths makes the two sockets' histories *cross* rather than break, which is worse because nothing looks wrong. Unreachable on every current fleet host (`<k>` is only load-bearing when a chip name repeats; sheep and trashcan have exactly one `coretemp` each). The fix is to derive the disambiguator from the chip's stable device path — the ADR 0025 decision 2 `pci-<PCI_SLOT_NAME>` trick — which rewrites the identity of every stored sensor and therefore needs its own migration ADR. Recorded as a known limitation on ADR 0026.
- ~~**`tinytop-agent collect --json` ignores persisted settings (found in T17 hardware acceptance)**~~ — **FIXED in 0.7.2 (lane S1).** The rule shipped is *the collector is configured from the settings stored in the database the rows are going into*: `collect --json` alone stays hermetic (no settings read, no database opened or created, `default_sqlite_url` never consulted), while `collect --json --sqlite <db>` loads that database's settings before collecting. **One coverage boundary remains deliberate:** the CI proof is `topProcessCount`, not thermals, because `NativeCollector::thermal_root` (`tinytop-collectors/src/linux.rs:122`) is a private field with no setter or env override and `collect()` lives in another crate — making it injectable is the `include!` decision below. Both fields travel through the same single `configure` call, so the mechanism is pinned; the thermal end is covered by hardware acceptance on real `coretemp` sensors.
- **T17-fix1's end-to-end test reaches the collector by `include!` (test-only debt)** — `tests/thermal_end_to_end.rs` textually includes `tinytop-collectors/src/thermal.rs`, which re-runs that file's own 10 unit tests inside the new target, so the workspace count double-counts them (**411 today; removing the `include!` yields 401, not a loss of ten tests**). The count reconciles exactly as of 0.7.0: 403 test annotations + the 10 duplicated executions = 413 = 411 passed + 2 ignored + 0 filtered. Note that the **385 recorded for 0.6.0 does not reconcile against its own tree** (389 + 10 − 2 = 397), so the 385 → 411 step is mostly a stale baseline figure rather than 26 new tests — the source gained exactly 14, all of them T18's. `include!` appears nowhere else in the repo and rust-analyzer cannot resolve it. The clean route is closed today because `tinytop-collectors` declares `pub(crate) mod thermal`, so a path dev-dependency alone would not reach it; resolving this means deciding whether that module becomes public API to serve a test, which earns its own task.
- **GPU `busyPercent` is `null` when no readable DRM client exists (T15 observation (a))** — idle, and again the tick after a load ends: ADR 0025 decision 5's "no evidence" reading rather than 0 %; the dashboard shows `—`. With 242 denied pids on trashcan that is the honest value. Reporting 0 % whenever an interval exists is a design change — Michel's call, then an ADR 0025 amendment.
- **One `null` GPU busy sample per minute per fdinfo adapter (T15 observation (b))** — every slow-tick (60 s) re-detect resets the fdinfo client state, so the first tick after a re-detect reports `null` (ADR 0025 decision 5 says so explicitly). A future amendment could keep the client map while the adapter set is unchanged (a smoother chart). Not a defect.
- **`gpu_samples` secondary index — measure first (luna run 675 ruling (d))** — adapter-filtered reads over a full 24 h window (luna's 345,600-row example) run without a secondary index today; measure `GET /api/history/gpus` on a populated file before adding one.
- **Time v3→v4 on a POPULATED fast table (T15 observation (e))** — the measured 34 ms is on an empty fast table (the live file is 0.3.1); a 24 h fast table (≈ 460k rows) is untimed on real data — expect low seconds (`INSERT … SELECT` + one index). INSTALL's "seconds on a default 24 h window" stands as a projection until measured.
- **`consecutiveFailures` beside the cumulative OTel `failures`** — deep review ruling 18 (d): sufficient today with `lastFailureMs > lastSuccessMs` and `lastError`; a consecutive count would sharpen operator diagnosis.
- **Measure the disabled-path cost the exporter adds** — deep review ruling 18 (e): one snapshot clone into the watch channel per collection and one settings read per 5 s tick while `otel.enabled=false`; by design (the 5 s tick bounds settings latency), unmeasured — measure before optimising.
- **Stale-check refusal (from the T9 blind review, luna run 600)** — ADR 0020 keeps the last known
  disk-pressure state when a measurement is undeterminable, so a persistent measurement failure
  after a real disk fill leaves `active:false` and growth is still permitted; `lastCheckMs` is a
  signal, not a boundary. Candidate rule: refuse horizon growth / tier or archive enables when no
  successful check has happened for more than 2 × `retentionLadder.diskCheck.intervalMinutes`,
  with a message naming the staleness. Additive to §5; needs a spec sentence and an ADR that
  supplements 0020.
- **First-class `--base-path` / `TINYTOP_BASE_PATH` serving** — mount dashboard/assets/APIs
  under `{base}/...` with a bare-mount redirect, removing the trailing-slash requirement for
  subpath deployments. Polish, not needed by any current deployment (v0.2.2's base-relative
  assets cover the standalone-under-subpath and `/embed` cases). Reference implementation:
  closed PR #1 (superseded; VERSION/ADR-number/dashboard-file conflicts made it unmergeable).
- **Ring-only rustls provider for the OTel exporter (from the T11 fix round, 2026-08-29)** — the OTLP
  HTTP client reaches `aws-lc-sys` (rustls's default crypto provider, built from C), which makes
  `cmake` and a C compiler build prerequisites on every host. `opentelemetry-otlp` 0.32 exposes no
  `ring`-only feature path; reaching one means a direct `reqwest`/`rustls` client passed through
  `with_http_client`. Deferred: documented as a prerequisite instead (INSTALL.md); revisit when the
  OTel crates expose the provider choice or when a macOS/Windows build without CMake is required.

## Completed

### 0.7.1 - Settings dialog fixed box, and the historical timeline holds still

- [x] Settings dialog closed itself on a tab switch (ADR 0029): a content-sized dialog + a tab `focus` handler that fires on **mousedown** resized the box before mouseup; a centred dialog shrinking moves its top edge DOWN, the mouseup landed on the backdrop, and `click` (dispatched to the common ancestor of mousedown/mouseup) reported the dialog as its target, so the dismiss handler fired mid-click. Fixed by one fixed box for every tab plus requiring a backdrop dismiss to have STARTED on the backdrop -- which also stops a drag-to-select in the document editor from discarding the edit.
- [x] Every tab fits the fixed box with no scrolling, measured in a real browser against 650px of body: general 672->629, history 372->353, metrics 884->**621** (families side by side), thermals 185->174, advanced 783->**447** (two columns: OTel fields left, raw document right). Two selectors found silently dead against `.settings-group` on source order and now panel-scoped.
- [x] A historical timeline was pushed sideways by live samples (ADR 0030, pre-existing): `renderSnapshot` pushed every poll into the charted series regardless of window, so a selected window piled up live points AND -- being hydrated at the 1200 render cap -- had its oldest point evicted once per tick until the range was all live data. Now only `live` charts polled samples; a historical window still drives the TILES from the live snapshot, and that render must follow `renderSelectedSample` or the gauges freeze.
- Gate: Rust 28 suites / 411 passed / 0 failed / 2 ignored; Bun 261 passed / 0 failed across 22 files (+7 regression tests); fmt + clippy clean. Timeline fix proven with a control: `15m` counter held 300 across 7 s while tiles changed 3x; `live` moved 161->165 over the same interval.

### 0.8.0 - Settings correctness (S1 + S3 + S2), shipped as 0.7.2 / 0.7.3 / 0.8.0

Michel's instruction: *"please proceed with S1-S3"*. Plan `docs/plans/2026-09-01-settings-correctness-plan.md` (read its amendment block first — the body was written at `0000f80` and several of its claims are stale), evidence base `docs/reports/2026-08-31-settings-defect-inventory.md`.

- [x] **S1 / 0.7.2 (D1)** — the only one of the three that wrote wrong data: `collect()` never loaded settings, so with thermals enabled `collect --sqlite <db>` inserted sensor-less rows into a real database. The rule shipped: *the collector is configured from the settings stored in the database the rows are going into*; `collect --json` alone stays hermetic. A store-open or settings-read failure now aborts rather than falling back to defaults, which would be the same defect in a new costume (`ari-sol-deep`).
- [x] **S3 / 0.7.3 (D3 + M1 + M2)** — the inventory recorded D3 as three diverging strings; re-reading both validators found the two sides also **evaluated differently** (backend per element with early return, client per rule across the whole array), so for `["cpu_a","cpu_a","amdgpu"]` the server said *duplicate* and the client said *reserved*. Aligning only the strings would have looked fixed. Also: thresholds now honour all of ADR 0026 decision 4's `0 < t ≤ 200 °C` band, and a chipless reading groups under `unknown` instead of heading a group rendered `undefined` (`ari-sol`).
- [x] **S2 / 0.8.0 (D2, ADR 0031)** — both write paths (`PUT` and import, never a dry run) configure the collector before responding. The plan's own recommended implementation was **rejected**: nudging with the caller's settings introduces a stale write-back race, because the tick reads settings and applies them with a prune in between and the `applied == desired` guard compares the new config against the old one and applies the old one. The configure path now reads the row itself under the `collector_config` guard, making that mutex the single serialization point (`ari-sol-deep`).
- Blind `ari-luna --effort max` review per lane, every finding validated at source: S3 returned one P3 (downgraded — her supporting reasoning was refuted by executing both implementations against the test's own input), S2 returned none.
- Gate on main: Rust **28 suites / 417 passed / 0 failed / 2 ignored** (411 → 417, +6 new tests), Bun **264 passed / 0 failed across 22 files** (261 → 264), fmt + clippy clean. `cargo audit` 0 vulnerabilities across 296 crates with the same 3 allowed warnings carried since 0.4.0; `bun audit` clean.
- **Three defects were in the briefs, not the lanes**, and each lane stopped rather than improvising: a claim that the `Collector` trait was in scope (invalid — `LinuxCollector` has an *inherent* `collect()`, so trait-only `configure` failed), a claim that `bun test` was containment-safe (it binds a socket), and a test whose hermeticity assertions could never fail because its temporary `HOME` was never wired to anything.

### 0.7.0 - Cadence classes and GPU, Phase 5 (T18 + T18b + the settings switch pass)

- [x] T18 / 0.7.0: `METRIC_REGISTRY` as a `[MetricDescriptor; 13]` (arity enforced by the type, `descriptor()` panics on a missing name, validation covering uniqueness/grammar/families/units and the 10/3 semantic split); `otel.disabledMetrics` as a validated SET (≤ 64 entries, `^[a-z][a-z0-9._]*$`, ≤ 128 chars, duplicates refused naming the repeat) with unknown names accepted and round-tripped byte-identically; record-time skipping in `collect_and_export` so a disabled metric is absent from the request rather than zero-valued, and an all-disabled export succeeding without advancing the failure counter; `GET /api/otel/metrics` read-only + `no-store`, carrying no `endpoint` and no `headersEnvVar`. No dependency added, `Cargo.lock` untouched by the lane (`ari-sol-deep`).
- [x] T18b / 0.7.0: the five-tab settings dialog with permanently mounted panels (selection via `[hidden]` only, so `collectDaemonSettingsFromForm` keeps hidden-tab values), a real tablist (roving `tabindex`, wrapping arrows, Home/End), guarded `localStorage` tab memory resolving to General when unavailable, capability-driven Metrics/Thermals tabs, the registry-driven picker with inert unknown names, and the validate-then-apply Advanced document; client gaps closed (`extraChips` reserved-chip refusal, save errors preferring the server's message) (`ari-sol`).
- [x] Settings UI/UX (ADR 0028), Michel's instruction: dialog booleans become switches (`role="switch"`, native input restyled in place so static and runtime-built controls cannot drift; on-state `--cyan`/`--surface` adopts each theme's accent and inverts correctly on light `solar`); checkboxes outside the dialog left as checkboxes because they select set members; the `float: right` legend button, the metric row's magic `margin-top` and the unit's stolen line all fixed.
- Measured: **98 → 14 series** (13 → 11 metrics) when `system.filesystem.*` is disabled — 84 removed, 85.7 % — decoded from a real OTLP export captured off a scratch daemon, which quantifies (and exceeds) ADR 0027's cost-lever claim of "of the order of half" against "of the order of forty" series.
- Gate on main, identical before and after the UI pass: Rust 28 suites / 411 passed / 0 failed / 2 ignored; Bun 254 passed / 0 failed across 22 files; fmt + clippy clean. Rendered-page check in both themes covering all five tabs, the keyboard path and the off-state rows. Gate detail: see `CHANGELOG.md`.

### 0.5.4 - Cadence classes and GPU, Phase 5 lane 4 (T15 + T15b)

- [x] T15 / 0.5.4: the GPU collector on Linux (DRM sysfs + `/proc/<pid>/fdinfo` engine deltas + hwmon; `gpu_busy_percent` preferred; a failed busy verdict cached until re-detect; NVIDIA proprietary identity-only; no subprocess, no vendor library) and schema v4 (ADR 0025) — both process tables rebuilt with `started_at_ms` in ONE guarded transaction, `gpu_adapters` interned, `gpu_samples` per adapter per tick, `GET /api/history/gpus`, `db stats` GPU counts, `wouldDelete.gpuSampleRows` (hexe run 674 after 670 escalated correctly on the `time` `parsing` feature; luna 675 → T15-fix1 run 676: `drm-total-cycles-*` as the cycles form + three doc lines). T15b: the GPU panel, the row-gated GPU column with sort + detail (hexe run 671; luna 673 clean). Measured: real-file v1→v4 3,215 ms (v3→v4 34 ms over 11,987 minute rows, 0 unparsable), `process_samples_fast` 51.9 B/row (was 66.7; target ≤ 60), the fdinfo scan 4.53 ms over 8 readable / 242 denied pids on trashcan. Acceptance: `Fabulous/docs/fleet/tinytop/2026-08-30-t15-acceptance-checklist.md`. Gate on main: see `CHANGELOG.md`.

### 0.5.3 - Cadence classes and GPU, Phase 5 lane 3 (T14 + T14b)

- [x] T14 / 0.5.3: schema v3 (ADR 0024) — `metric_samples` rebuilt without `snapshot_json`, `host_identity` interning, filesystems on change keyed by the enumeration stamp with presence events, `/api/history` assembled from typed tables, every runtime JSON path removed; T14-fix1: the migration normalises + counts legacy Bun negative inode counts (found by the real-file gate); T14-fix2: the regressing-stamp warn, schema-equality + carried-values tests, doc corrections (luna 667). T14b: the dashboard replay repair, `—` for history pressure, threads as a number; T14b-fix1: the blank-panel regression since 0.3.1 (luna 662's P0). Acceptance: `Fabulous/docs/fleet/tinytop/2026-08-30-t14-acceptance-checklist.md`.

### 0.5.2 - Cadence classes and GPU, Phase 5 lane 2 (T13)

- [x] T13 / 0.5.2: schema v2 (ADR 0023) — `process_commands` dictionary (`command_id`, `UNIQUE(command)`), `process_samples_fast` (WITHOUT ROWID, one row per top-N process per poll tick), `command_id` on the minute table with the `command` text column dropped, v1→v2 in ONE transaction behind a `sqlite_version() ≥ 3.35.0` pre-write check and an in-flight guard, no pre-image; `processFastKeepHours` (1–72, default 24) with its dashboard control and the unconditional `wouldDelete.processFastRows`; `/api/history/processes?sinceMs=` served from the fast table inside the keep window and the minute table outside it (`source` in the response); maintenance prunes expired fast rows and drains orphaned commands in 1,000-row batches (`MaintenanceReport.detail_rows_pruned`); `db stats --json` `userVersion` (hexe run 654 after 649/651/653 escalated correctly on brief lines; luna 655; fix 657: `DROP COLUMN` keeps SQLite's own cause + the first real rollback test, three test-strength items). Measured: v1→v2 on a read-only copy of the live 225 MB file 273 ms (test) / 199 ms (daemon); `process_samples_fast` 66.7 B/row + 19.1 B/row index vs the plan's ≤ 60 B target — reported, not tuned (`started_at TEXT` → T14's interning decision). Gate on main: see `CHANGELOG.md`.

### 0.5.1 - Cadence classes and GPU, Phase 5 lane 1 (T12)

- [x] T12 / 0.5.1: cadence classes owned by the collector (ADR 0021) — `CollectorConfig` + `configure()`, Linux fast/slow/static source split with one `statvfs` site on the slow tick and a cached mount list stamped `filesystemsCapturedAtMs`; `cpu.times` optional (`None` on the sysinfo collectors); `/api/snapshot` + `/snapshot/latest` from the published snapshot (503 `no snapshot yet` only before the first collection); the daemon re-configures the collector only when `topProcessCount` / `detailIntervalSec` changed (next-tick semantics); both hard-coded tens gone; dashboard Filesystem panel shows `as of hh:mm:ss` when its rows are older than one poll (hexe run 643; luna 644; fix 648: sysinfo `totalThreads`/`lastPid` from the full process table, `Filesystem check seconds` label). Gate on main: see `CHANGELOG.md`.

### 0.5.0 - Tiered history ladder, Phase 4 close (T11)

- [x] T11 / 0.5.0: OTLP metrics push exporter (ADR 0015; spec §12): `otel` settings block (`enabled=false`, `http://127.0.0.1:4318/v1/metrics`, `http/protobuf`, `intervalSec` 5–3600, `headersEnvVar` name, `serviceName`, ≤ 32 `resourceAttributes`), absent-`otel` imports keep the persisted block; `otel.rs` builds `SdkMeterProvider` + a shared `ManualReader` + `MetricExporter` (Delta temporality, 10 s timeout) and the writer's 5 s-tick loop exports the latest `watch`-published snapshot at `intervalSec` without ever holding the status lock across an await or a sleep; headers parsed OTLP-style from the named variable at pipeline build only, `%`-re-encoded for the SDK, the standard OTLP header variables refused; cumulative `failures`, `lastSuccessMs`/`lastFailureMs`/sanitized `lastError`, one warn per minute, one recovered line; coverage `otel` block, `db stats` presence-only, dashboard group hidden on Bun. Reviews: luna 630 (P0 status lock across the disabled sleep — fixed in 632, measured 4.15 s → 9 ms), deep dual-blind 633/634 (no P0; P1 endpoint credentials — fixed in 637). Binary +7.2 MB at T11; lock 203 → 296; C compiler prerequisite.
- [x] Phase 4 close / 0.5.0: P4-fix1 (run 637) — endpoint credential/host validation and secret-shaped attribute keys, settings merge inside the write transaction (`put_settings_document`), fail-closed standard-variable preflight, hung-receiver test, presence true-branch, docs (GUIDE privacy, C compiler vs CMake, `trace` feature, spec/ADR/plan amendments).

### 0.4.1 - Tiered history ladder, Phase 3 (T10)

- [x] T10 / 0.4.1: versioned, secret-free settings document (ADR 0016) — `GET /api/settings/export` (attachment, `tinytopConfigVersion` 1), `POST /api/settings/import` with `?dryRun=true` returning `{valid, errors[], warnings[], changedKeys[], wouldDelete}` where `wouldDelete` is five server `COUNT(*)`s under the prune predicates; apply goes through `put_settings` (`BEGIN IMMEDIATE`), runs maintenance, records a `settingsChange` marker `{"source":"import","changed":[…]}`; `config export [--out FILE]` (no-clobber `.tmp` → fsync → hard-link publish, rename fallback where links are unsupported) and `config import FILE [--dry-run]` (exit 1 with ONE refusal JSON; never runs maintenance beside the daemon); dashboard Export/Import buttons (hidden on Bun), the shrink confirm uses the dry-run and the "approx." estimates are gone. Shared store module `settings_transfer.rs`; no new dependency; `user_version` stays 1. Fix round after luna run 617 (hexe run 619): save-path prompt regression, `.tmp` cleanup on failure, directory fsync, rename fallback, `"1"`/`1.5` version tests, zero-event invalid-path invariants, single-object CLI refusal test. Review record: Fabulous `docs/fleet/tinytop/2026-08-29-ari-luna-t10-review.md`.

### 0.4.0 - Tiered history ladder, Phase 2 close

- [x] Phase 2 close / 0.4.0: deep dual-blind review (sol + luna, one 21-claim brief over `v0.3.0..v0.3.3`) and its fix round — the cold export now requires main to hold no rows for a month and stops at the first month still being moved (P0); the command-centre test harness runs every case under a per-call temp `HOME`/XDG root with stubbed `systemctl`/`ss`/`curl`/`pgrep` (P0 — the earlier fix had isolated only the unit directory); `put_settings` reads, validates and writes inside one `BEGIN IMMEDIATE` (P1); strict RFC 4180 verifier; schema-checked archive point reads; 12-month cap per export pass; INSTALL.md operations guidance; clippy-clean workspace with `cargo clippy -- -D warnings` in `check:rust`. GitHub release with `cargo audit` + `bun audit` pasted. Review record: Fabulous `docs/fleet/tinytop/2026-08-29-ari-dual-blind-phase2-review.md`.

### 0.3.3 - Tiered history ladder, Phase 2 (T9)

- [x] T9 / 0.3.3: hourly disk check on the database's filesystem (first check at daemon start, measurement on a blocking thread) writing `history_state.diskPressure` / `lastDiskCheckMs` and the `diskPressure` / `diskRecovered` timeline markers as a four-transition state machine inside one `BEGIN IMMEDIATE` transaction; pressure refuses growth only, never deletes; undeterminable measurements keep the last state (ADR 0020); `pressureSinceMs` in coverage and `db stats`; marker colours in the dashboard. Fix round after luna run 600 (atomic read-modify-write, marker read-back test, full-row assertions, interval clamp). Run 596 escalated correctly on a brief that excluded a test file needing the new field.

### 0.3.2 - Tiered history ladder, Phase 2 (T8)

- [x] T8 / 0.3.2: verified monthly cold export of the queryable archive (`tinytop-1h-YYYY-MM.csv.gz` + `.sha256`, RFC 4180, gzip 6, `.tmp` → fsync → hash → re-read verify → rename → sidecar → manifest → watermark; never deletes), exportable only once every hour of the month has expired from L4; hourly scheduler; real cold coverage; `db archive status|export-now`; carry-overs closed: CLI `close()` checkpoints the WAL, inspection never creates a database, `limit=0`/inverted ranges → 400. Fix round after luna run 589: step naming, record-width verification, incomplete-archive reporting, month-listing boundary; `TINYTOP_SYSTEMD_UNIT_DIR` isolates the command-center tests from the real user units (the gate had stopped the live service; the Phase-2 close fix then isolated `HOME`/XDG and stubbed the host commands too). `flate2` 1.1.10 + `sha2` 0.11.0 vetted (`docs/reports/2026-08-29-dependency-vetting-flate2-sha2.md`).

### 0.3.1 - Tiered history ladder, Phase 2 (T7)

- [x] T7 / 0.3.1: expired L4 rows move into a queryable `history-archive.sqlite` (`retentionLadder.archive.queryable`), `source=auto` falls through to it, coverage and `db stats` report real archive counts, reads never create the file. The plan's single cross-file move transaction was ruled unsafe at SQLite source (main commits first under WAL) — the lane escalated on it correctly; ADR 0018 (copy → commit → verify → delete) and ADR 0019 (key-set verify, full-row delete match, fsynced archive commit, watermark inside the delete transaction) after the blind review. Known carry-overs to T8: the `cli_db` v0 fixture flake (deletes the `-wal` instead of checkpointing), `db stats` on a missing path creates a database, `limit=0`.

### 0.3.0 - Tiered history ladder, Phase 1 (T1–T6)

- [x] T1 / 0.2.7: added SQLite schema v1 and a fail-closed, complete, non-overwriting pre-image migration.
- [x] T2 / 0.2.8: added weighted L1→L4 folding, frozen completed buckets, bounded promotion, and promote-before-prune retention.
- [x] T3 / 0.2.9: added validated `retentionLadder` settings, legacy aliases, and disk-pressure-aware growth rules.
- [x] T5 / 0.2.10: added ladder settings/coverage UI, truthful long-range presets, and shrink confirmation.
- [x] T4 / 0.2.11: added four-tier automatic reads, coverage, and typed filesystem/process detail APIs.
- [x] T6 / 0.3.0: added ladder-aware `db stats --json`, guarded pre-image status/removal, operator docs, and Phase 1 close-out material.
- [x] Phase 1 close / 0.3.0: deep dual-blind review (sol 568 + luna 569, 21 claims over `v0.2.6..HEAD`) and its fix round — P1-fix1 (store/CLI, incl. the source-pruned refold merge), P1-fix1b (replay never re-merges), P1-fix2 (dashboard + docs); tagged `v0.3.0`.

### 0.2.1 - Code-Review Hardening (C1, M1-M4, D1-D2)

- [x] C1/M2: Bun `runText` enforces a 10s timeout that kills the child and falls back, with rate-limited per-source failure logging (parsers still receive `""`).
- [x] M3: Bun dashboard writer proxy (`fetchWriterWithRetry`) times out each attempt with a 3s `AbortSignal.timeout`.
- [x] M1: Rust collector populates per-filesystem inode fields via `statvfs(2)` (rustix) instead of leaving them `null`, matching the Bun `df -i` contract without a subprocess (ADR 0012).
- [x] M4: Rust store persists canonical `runtime_kind` (`RuntimeKind::as_str()`) and canonicalizes legacy `Debug` rows via an idempotent migration.
- [x] D1: `frame-ancestors 'self'` CSP on `/` and `/index.html` in both runtimes; `/embed` keeps configurable ancestors.
- [x] D2: `/embed` frame-ancestors fail closed to `'self'` on invalid configuration in both runtimes.

### 0.2.0 - TinyTop Host Dashboard Integration

- [x] Added `/embed` as a chrome-trimmed, iframe-friendly view of the existing dashboard.
- [x] Added dark/light theme query aliases for embed hosts.
- [x] Added configurable `/embed` `frame-ancestors` CSP via `TINYTOP_EMBED_FRAME_ANCESTORS`.
- [x] Added version/health capability advertisement for `snapshot`, `history`, and `embed`.
- [x] Added `docs/INTEGRATION.md` with the stable tutus-remotus data contract.

### 0.1.35 - Windows Native Runtime Identity And Startup Fixes

- [x] Fixed native Windows direct Rust `serve` startup when `HOME` is absent by adding a `%LOCALAPPDATA%\TinyTop\state\history.sqlite` default with a `USERPROFILE\AppData\Local` fallback.
- [x] Moved native Windows dashboard default port to `127.0.0.1:4275` to avoid collisions with WSL/Linux on `127.0.0.1:4274`.
- [x] Added `tinytop.cmd` and process-scoped execution-policy guidance for systems that block direct `.ps1` execution.
- [x] Fixed `tinytop.ps1 service install` strict-mode argument handling.
- [x] Added daemon OS/install/bind/SQLite metadata to `/health` and `/api/version`.
- [x] Added a dashboard runtime-origin notice for native Windows versus WSL/Linux daemon confusion.

### 0.1.34 - On-Demand Cross-Platform Binary Workflow

- [x] Added `.github/workflows/build-binaries.yml` as a manual `workflow_dispatch` release-binary builder.
- [x] Added platform selection for `all`, `linux`, `windows`, and `macos`.
- [x] Added native hosted-runner builds for Linux x86_64, Windows x86_64, macOS x86_64, and macOS aarch64.
- [x] Uploaded binaries and `.sha256` files as workflow artifacts.
- [x] Added optional upload to an existing GitHub release tag.
- [x] Added workflow contract regression coverage and release-build documentation.

### 0.1.33 - Windows Service Elevation Guard

- [x] Added a shared PowerShell guard for mutating Windows service actions.
- [x] Kept `service status` read-only and non-prompting.
- [x] Required explicit confirmation before interactive non-elevated service mutations.
- [x] Failed non-interactive non-elevated service mutations with Administrator guidance.
- [x] Updated Windows install docs and regression coverage for the service guard.

### 0.1.32 - Live Connected README Screenshot

- [x] Replaced the README screenshot with a fresh dashboard capture from the running Rust daemon.
- [x] Captured the dashboard after it hydrated with real host, CPU, RAM, swap, load, health, and history values.
- [x] Confirmed the visible sidebar shows the green `Live` connection indicator.
- [x] Bumped product, command-center, PowerShell, and Rust crate versions to 0.1.32.

### 0.1.31 - Settings Readout And Rust Agent Rebuild

- [x] Fixed the Settings dialog effective-settings readout so browser/daemon defaults render as compact chips instead of stretched ovals.
- [x] Changed daemon redaction and enabled-section checkboxes into compact responsive toggle controls without changing settings IDs or storage.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical for the CSS fix.
- [x] Added a fresh rendered dashboard screenshot to the README.
- [x] Bumped product, command-center, PowerShell, and Rust crate versions to 0.1.31.
- [x] Rebuilt the release `tinytop-agent` binary with the embedded dashboard CSS fix.

### 0.1.0 - Initial Dashboard

- [x] Created standalone project folder outside `the-operator`.
- [x] Selected Bun as runtime and HTTP server.
- [x] Implemented read-only collectors for `/proc`, `df`, `ps`, `uname`, and OS release data.
- [x] Implemented WSL versus real Linux runtime detection.
- [x] Built the first dashboard UI with gauges, charts, stat tiles, filesystem bars, pressure panels, and process rows.
- [x] Claimed `127.0.0.1:4274`.
- [x] Added initial Bun tests and rendered browser QA.

### 0.1.1 - Themes And Graph Modes

- [x] Added Midnight, Matrix, Aurora, Solar, and Ember themes.
- [x] Added selectable history graph modes.
- [x] Persisted theme and graph preferences in browser-local storage.

### 0.1.2 - Timeline Scrubber

- [x] Moved Live History directly under the main gauges.
- [x] Added history scrubbing for gauge values.
- [x] Added a return-to-live control.
- [x] Kept selected sample datetime context visible.

### 0.1.3 - Graph Nav And Context

- [x] Restored Bar graph mode in Live History.
- [x] Moved graph type controls into the Live History top nav.
- [x] Relocated the timeline below the chart.
- [x] Added numeric context to graph axes, timeline values, and heatmap lanes.

### 0.1.4 - ECharts Migration

- [x] Replaced custom Live History chart rendering with Apache ECharts.
- [x] Added line, stacked area, stacked bar, heatmap, and treemap modes.
- [x] Served the ECharts browser bundle from a local dependency route.
- [x] Verified chart selection, desktop layout, and mobile layout.

### 0.1.5 - Responsive Bar Planning

- [x] Added responsive stacked bar visible-window sizing.
- [x] Documented the SQLite history architecture plan and ADR.
- [x] Kept display settings scoped to browser-local storage.

### 0.1.6 - SQLite Recent History

- [x] Implemented the Bun collector/writer process on `127.0.0.1:4276`.
- [x] Added SQLite-backed recent history storage.
- [x] Added `/api/history`.
- [x] Hydrated Live History from persisted samples on dashboard refresh.
- [x] Prevented duplicate bars when polling returns the same latest sample.
- [x] Added storage and history API tests.

### 0.1.7 - Documentation Pass

- [x] Renamed project identity to TinyTop.
- [x] Renamed package, app title, data path, browser storage keys, and fleet port claim.
- [x] Rewrote `README.md`.
- [x] Added `INSTALL.md`.
- [x] Added `GUIDE.md`.
- [x] Rewrote `ARCHITECTURE.md`.
- [x] Rewrote `CHANGELOG.md`.
- [x] Rewrote `PROGRESS.md`.
- [x] Added `docs/guides/API.md`.
- [x] Added `docs/guides/OPERATIONS.md`.
- [x] Updated `docs/sqlite-history-architecture.md`.

### 0.1.8 - Install Wizard Design

- [x] Reviewed the Telecode install wizard pattern.
- [x] Approved TinyTop's two-layer installer direction.
- [x] Documented the zero-dependency `./tinytop` Bash command center.
- [x] Documented the Bash-to-Bun handoff for `./tinytop setup` -> `bun run setup`.
- [x] Documented planned systemd user services for the writer and dashboard.
- [x] Documented planned SQLite stats, check, backup, vacuum, and reset operations.
- [x] Added ADR 0003 for the Bash bootstrap plus Bun wizard decision.

### 0.1.9 - Install Wizard Implementation

- [x] Added root `./tinytop` Bash command center.
- [x] Added Bun install guidance and `./tinytop install-bun`.
- [x] Added `./tinytop setup` handoff to `bun run setup`.
- [x] Added `src/wizard/index.ts` setup wizard with noninteractive automation flags.
- [x] Added user-space systemd service rendering and management.
- [x] Added SQLite stats, integrity check, backup, vacuum, and guarded reset commands.
- [x] Added command-center, wizard, systemd, and SQLite operation tests.

### 0.1.10 - Public README And Privacy Cleanup

- [x] Added README hero image.
- [x] Added inline README install and usage guide for new users.
- [x] Removed hardcoded local home paths from public docs.
- [x] Replaced host-specific examples with generic examples.
- [x] Removed the old generated UI concept image with host-like demo strings.

### 0.1.11 - Apache License And Private Release Prep

- [x] Switched the project license to Apache License 2.0.
- [x] Added Apache-2.0 package metadata.
- [x] Added a NOTICE file.
- [x] Prepared the docs for a private GitHub release review before public conversion.

### 0.1.12 - Rust Linux Collector Preview

- [x] Kept the existing Bun collector and writer intact.
- [x] Added `agent/` as a Rust workspace.
- [x] Added shared Rust snapshot types matching the existing JSON contract.
- [x] Added a Linux/WSL Rust collector with fixture, live-host, and no-shell-command tests.
- [x] Kept Rust host collection crate-backed through `procfs` and `sysinfo`, with a reusable live `sysinfo::System`.
- [x] Added a SQLx-backed SQLite store crate for the Rust collector path.
- [x] Added `tinytop-agent collect --json` and optional `--sqlite` storage mode.
- [x] Documented the SQLx architecture decision and dependency vetting.

### 0.1.13 - Rust Single-Daemon Runtime

- [x] Added `tinytop-agent serve` as a Rust collector/dashboard daemon on `127.0.0.1:4274`.
- [x] Exposed public `/api/snapshot` and `/api/history` routes from the Rust daemon.
- [x] Exposed legacy collector-compatible `/snapshot/latest`, `/snapshot/collect`, and `/history` routes from the Rust daemon.
- [x] Added interval collection and SQLx-backed SQLite writes in the Rust daemon.
- [x] Updated `./tinytop systemd install` to default to a single Rust `tinytop.service`.
- [x] Kept the legacy Bun split services available with `./tinytop systemd install --bun`.
- [x] Added `./tinytop rust install-binary`, `build`, `serve`, `serve-writer`, `collect`, `test`, and `check`.
- [x] Added Rust-backed DB stats, integrity check, and vacuum support for the command center.
- [x] Updated the setup wizard to ask for GitHub release binary vs local Cargo compile.
- [x] Vendored Apache ECharts with upstream license and notice files for no-Bun runtime use.
- [x] Added ADR 0005 and dependency/provenance reports for Axum and vendored ECharts.

### 0.1.14 - Web UI Confirmation Dialogs

- [x] Scanned the public web UI for native browser dialog APIs.
- [x] Replaced the alert-named inline error surface with `status-message` naming.
- [x] Added a reusable accessible confirmation dialog backed by `<dialog>`.
- [x] Added a confirmed `Clear` control for the browser-local Live History session buffer.
- [x] Added regression coverage for the no-native-dialog policy.
- [x] Documented the dialog policy and rendered verification.

### 0.1.15 - Handoff Checkpoint

- [x] Added root `HANDOFF.md`.
- [x] Captured the current repo, tag, remote, runtime, and health state.
- [x] Confirmed the running daemon is the Rust collector path.
- [x] Recorded recent verification evidence and next useful work.

### 0.1.16 - Collector Naming And Legacy Bun Placement

- [x] Moved the legacy Bun collector daemon to `legacy/bun-collector.ts`.
- [x] Added `bun run collector` and `bun run collector:check` scripts while preserving writer aliases for compatibility.
- [x] Updated the setup wizard to choose `rust` or `bun` collector runtime.
- [x] Kept Rust as the default one-daemon collector/dashboard path.
- [x] Updated new legacy Bun systemd units to use `tinytop-collector.service`.
- [x] Kept command-center cleanup/status paths aware of older `tinytop-writer.service` installs.
- [x] Updated current-facing docs from writer-first language to collector-first language.

### 0.1.17 - Embedded Rust Dashboard Assets

- [x] Moved the static dashboard asset tree to `legacy/dashboard/` for the legacy Bun runtime.
- [x] Added a byte-identical Rust dashboard asset tree under `agent/assets/dashboard/`.
- [x] Embedded the dashboard HTML, CSS, browser JavaScript, and ECharts bundle into `tinytop-agent serve`.
- [x] Kept `--public-dir` and `TINYTOP_PUBLIC_DIR` as explicit development overrides.
- [x] Updated `./tinytop rust serve` and systemd rendering to use embedded assets by default.
- [x] Added regression coverage for embedded Rust serving without a dashboard directory and asset equality across legacy/Rust dashboard trees.
- [x] Added ADR 0006 for embedded Rust dashboard assets and legacy dashboard asset ownership.

### 0.1.18 - Documentation Sweep

- [x] Refreshed root docs and guides for the Rust collector/dashboard daemon and legacy Bun fallback wording.
- [x] Updated dependency and verification reports to point at `agent/assets/dashboard/` and `legacy/dashboard/`.
- [x] Marked the original Bun writer ADR as superseded in the ADR index while preserving the historical ADR file.
- [x] Added a documentation sweep report for the embedded dashboard asset transition.

### 0.1.19 - History Retention Documentation

- [x] Clarified that SQLite raw samples are retained indefinitely until manual archive/reset.
- [x] Clarified that `/api/history` query windows and the dashboard's 120-sample UI buffer are read/rendering limits, not database retention.
- [x] Updated README, guide, install, API, operations, architecture, SQLite history architecture, changelog, progress, and handoff docs.
- [x] Added a documentation report for the retention wording sweep.

### 0.1.20 - Runtime-Specific Setup Verification

- [x] Split package checks into `check:bun`, `check:rust`, and full `check`.
- [x] Updated the setup wizard so Rust selections do not run Bun tests.
- [x] Updated the setup wizard so legacy Bun selections do not run Rust tests.
- [x] Verified Rust release-binary systemd setup installs the binary before running the Rust smoke check.
- [x] Added regression coverage for Rust release, Rust compile, and legacy Bun verification command selection.

### 0.1.21 - Timestamp Timeline Planning And Browser Slice

- [x] Saved the dashboard timeline/settings implementation plan under `docs/superpowers/plans/`.
- [x] Added History range presets for Live, 15m, 1h, 6h, and 24h.
- [x] Replaced index-based timeline selection with timestamp-based selection.
- [x] Changed dashboard history hydration to use explicit `since_ms` and `until_ms` windows.
- [x] Added client-side pagination for large `/api/history` ranges.
- [x] Persisted the selected history range as a browser-local preference.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.
- [x] Added dashboard timeline regression coverage and embedded Rust smoke evidence.

### 0.1.22 - Runtime Auto-Detect And Version Identity

- [x] Added `/api/version` for the Rust collector/dashboard daemon and legacy Bun dashboard.
- [x] Added `/version` on collector-compatible APIs for the Rust daemon and legacy Bun collector.
- [x] Added a sidebar version line showing the serving collector/dashboard runtime and product version.
- [x] Added the SQLite `app_settings` table for daemon dashboard defaults.
- [x] Added `GET /api/settings` and `PUT /api/settings` to the Rust collector/dashboard daemon.
- [x] Added a Settings panel with `This Browser` local preferences and `This Daemon` SQLite-backed defaults.
- [x] Added legacy Bun fallback settings handling so the shared dashboard remains usable in legacy mode.
- [x] Changed `./tinytop start` to auto-select Rust when available and honor `TINYTOP_RUNTIME=legacy|bun` for the legacy fallback.
- [x] Updated `./tinytop status` to read `/api/version` and report the running daemon runtime, component, version, and dashboard asset mode.
- [x] Added foreground `./tinytop stop` and `./tinytop restart` handling for detected Rust and legacy Bun processes when systemd units are absent.
- [x] Aligned Rust crate package versions with the product checkpoint version.

### 0.1.23 - Settings Dialog Presentation

- [x] Moved Settings out of the inline dashboard flow into an accessible modal dialog.
- [x] Changed the rail Settings control from an anchor to a button that opens the dialog.
- [x] Kept `This Browser` and `This Daemon` settings groups intact.
- [x] Kept browser-local and SQLite-backed settings storage unchanged.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.
- [x] Added regression coverage preventing the inline settings section from returning.

### 0.1.24 - Load Overview Gauge

- [x] Added Load as the fourth overview gauge next to CPU, RAM, and swap.
- [x] Normalized Load from 1-minute load divided by CPU core count, capped to 100.
- [x] Added a Load sparkline using the existing normalized load history series.
- [x] Kept the raw 1m/5m/15m load stat tile for detailed context.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.
- [x] Added regression coverage for the Load gauge markup and renderer wiring.

### 0.1.25 - Dashboard Operator Console And Retention

- [x] Saved and executed the operator-console implementation plan under `docs/superpowers/plans/`.
- [x] Added a top operator status strip with Healthy, Warning, Critical, and Stale states from saved thresholds.
- [x] Replaced the native history scrubber with a canvas timeline rail, selected timestamp marker, visible-window shading, and history coverage row.
- [x] Added `/api/history/coverage` in the Rust daemon.
- [x] Added Rust raw-history pruning by `retentionHours`.
- [x] Added Rust one-minute rollup buckets and rollup pruning by `rollupRetentionDays`.
- [x] Expanded daemon thresholds to CPU/RAM/disk/load/pressure warn and critical values.
- [x] Made enabled dashboard sections hide/show Overview, History, Filesystem, Pressure, and Processes.
- [x] Added process search, sort, density controls, and process detail dialog.
- [x] Added filesystem root card, system-mount toggle, and threshold-colored capacity bars.
- [x] Expanded browser-local preferences for visible series, process table state, filesystem toggle, and last section.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.
- [x] Added focused dashboard, server, Rust store, and Rust daemon regression coverage.

### 0.1.26 - Native Dropdown Contrast

- [x] Fixed Settings and process-density native select option colors across Midnight, Matrix, Aurora, Solar, and Ember themes.
- [x] Added regression coverage for readable native dropdown options.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.

### 0.1.27 - Dashboard Operator V2 And Platform Collector Roadmap

- [x] Saved and executed the dashboard operator V2 and platform roadmap plan under `docs/superpowers/plans/`.
- [x] Added an operator detail drawer with metric value, threshold, age, trend, and recent-change explanations.
- [x] Added additive Rust `/api/history/points` and `/api/history/markers` endpoints.
- [x] Added rollup-backed History presets for 6h, 24h, 7d, and 30d.
- [x] Added daemon-start, settings-change, and computed coverage-gap timeline markers.
- [x] Added `targetDatabaseBytes`, DB budget percentage, and rollup oldest/newest coverage fields.
- [x] Polished Settings with validation, dirty-close warning, reset/defaults actions, threshold presets, and effective settings readout.
- [x] Upgraded process details with redacted copy-safe command text, optional parent PID/start time, RSS, and per-PID CPU/RAM trend.
- [x] Added optional process metadata fields to the Rust snapshot contract.
- [x] Started feature-gated native macOS and Windows Rust collector modules using `sysinfo`.
- [x] Kept Linux/WSL as the default reference collector path.
- [x] Added ADR 0009 and ADR 0010.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.
- [x] Cleaned the stale handoff PID note.

### 0.1.28 - SVG Favicon

- [x] Added `favicon.svg` to both `legacy/dashboard/` and `agent/assets/dashboard/`.
- [x] Changed the dashboard `<head>` to reference `/favicon.svg` as an SVG favicon.
- [x] Served `/favicon.svg` from the Rust embedded dashboard path with `image/svg+xml`.
- [x] Expanded asset parity and Rust embedded serving regression coverage for the favicon.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.

### 0.1.29 - Windows Command Center And Critical Status

- [x] Saved and executed the Windows command-center and Critical status plan under `docs/superpowers/plans/`.
- [x] Added `tinytop.ps1` for Windows-native Rust binary install, Rust build, start, stop, restart, status, logs, and service commands.
- [x] Added Windows service install/uninstall/start/stop/restart/status commands through PowerShell and Windows Service Control Manager.
- [x] Made Windows builds select `--no-default-features --features windows-collector`.
- [x] Made the Bash command center print target-specific Rust build commands and use `.exe` binary names on Windows-like shells.
- [x] Strengthened operator strip styling so Critical, Warning, and Stale states are visually obvious at a glance.
- [x] Cleaned the sidebar runtime identity so long WSL detection reasons no longer dominate the brand block.
- [x] Added Windows guide, verification report, and ADR 0011.
- [x] Kept Rust embedded and legacy Bun dashboard assets byte-identical.

## Known Limitations

- Legacy Bun split mode does not enforce durable retention or rollups; use the Rust daemon for automatic pruning and coverage.
- Typed filesystem/process history is implemented; normalized pressure-history child rows remain future work.
- The app is designed for loopback/local use, not remote multi-user deployment.
- Native Windows and macOS collectors are feature-gated first slices; full parity, package-manager distribution, Windows release asset publication, and live-host verification are still future work.

## Recommended Next Work

- [ ] Build and upload a real Windows `.exe` release asset, then add Scoop and winget manifests.
- [ ] Add live macOS and Windows CI/host verification plus release packaging.
