# Changelog

## Unreleased

- **Settings switches no longer carry two competing implementations.** All 13 switches matched both ADR 0028's `::after` knob and a legacy `::before` knob, so the different pseudo-elements rendered two knobs. The legacy block is removed; one rule now combines the legacy geometry with ADR 0028's `--cyan` / `--surface` colour pair, and the on-state hue follows the active theme accent.

## 0.8.1 - 2026-09-01

- **The D1 regression test no longer depends on how many processes the host exposes.** It persisted `topProcessCount = 3` and asserted the inserted count was `<= 3`, comparing against a bare default collect *only* when the host showed more than three processes — so on this workstation (default collect returns 8) the contrast fired and the test was meaningful, while inside a containment lane with a tiny process table the comparison was skipped, the surviving assertion was `3 <= 3`, and the **unfixed code would have passed too**. It now collects twice into separate fixture databases with `topProcessCount` 1 and 2 and asserts exact counts and strict ordering, so it fails on any host with at least two processes; "fewer than two visible" is a hard failure rather than a warning. Found by the blind review of lane S1, which was dispatched *after* 0.8.0 because that lane had been the only one to ship unreviewed.
- The 0.6.0 changelog's known-limitation line still told a 0.7.2 reader that `collect` never reads persisted settings. It was true of 0.6.0 and a changelog records what shipped, so the sentence is preserved verbatim with a fixed-in-0.7.2 pointer appended rather than rewritten.

## 0.8.0 - 2026-09-01

Settings correctness: the three defects from `docs/reports/2026-08-31-settings-defect-inventory.md`, planned as S1/S2/S3 and shipped as 0.7.2, 0.7.3 and this release. Michel's instruction: *"please proceed with S1-S3"*.

- **A settings change made through the dashboard or API now configures the collector before the response returns** (D2, ADR 0031), so a toggle stops reading as a broken switch. It applies to `PUT /api/settings` **and** `POST /api/settings/import` — the plan named only the PUT, but an import is equally a settings write; an import *dry run* stays non-mutating. The change is effective from the next collection, which begins after the save returns; it does not alter a collection already in flight.
- **The configure path now reads the newest persisted settings while holding its configuration guard**, which closes a race the obvious implementation would have introduced. The tick reads settings and applies them with a history prune in between, so a write landing in that window would have been reverted for a tick by the tick's older snapshot — and the `applied == desired` guard cannot detect that, because it would compare the new config against the old one, find them different, and apply the old one. Making the guard the single serialization point means the last writer to the store is always the last writer to the collector. The tick order is otherwise unchanged, and the idempotence guard is now load-bearing rather than an optimisation.

## 0.7.3 - 2026-09-01

- **The dashboard's thermal validator now mirrors the backend's rule *order*, not just its wording** (D3). The backend is the only validator that can refuse a write, so the client must fire the same rule with the same words; otherwise the message an operator sees depends on which validator ran first. The inventory recorded this as three diverging strings, but re-reading both sides found more: the backend loops **per chip in array order**, returning on the first offending *element* (pattern → reserved → duplicate), while the client scanned the whole array **per rule**. For `["cpu_a","cpu_a","amdgpu"]` the server answered *duplicate* and the client answered *reserved* — so aligning only the strings would have looked fixed while a different rule still fired. The client now walks the array once, per element, in the backend's order, and the three diverging messages match `thermal_settings.rs` verbatim; the pattern message already agreed and is untouched. The duplicate message stops being a constant and **names the offending chip**, as the backend's `format!` does, so an operator learns which entry repeats.
- Thermal bars and severity now honour the whole of ADR 0026 decision 4's `0 < t <= 200 °C` band instead of only its lower half. A ceiling outside the band makes the bar absent rather than scaling every reading against a nonsense maximum — which is what sheep's nvme `temp2_max` of `65261850` would otherwise do, rendering a permanently-empty bar with no explanation.
- A thermal reading with a missing, empty, or non-string `chip` no longer heads a group rendered as the literal string `undefined`; such readings group together under `unknown`. They are not dropped — losing a reading is worse than labelling it. Both of these are unreachable from our own backend and are deliberate defence against any other producer of the same shape.

## 0.7.2 - 2026-09-01

- `tinytop-agent collect --json` without `--sqlite` remains hermetic and uses collector defaults without opening a database, while `collect --json --sqlite <db>` configures the collector from that target database's stored settings before inserting the row.

## 0.7.1 - 2026-09-01

Three defects Michel found using 0.7.0, two of them in the dialog 0.7.0 had just shipped and one pre-existing (ADR 0029, ADR 0030).

- **The settings dialog closed itself when you switched tabs** (ADR 0029). Michel diagnosed the mechanism himself — *"the window change sizes and suddenly under the mouse is NOT setting windows so it closes itself"* — and that is exactly it, in three parts: each tab carries a `focus` listener, and focus lands on **mousedown**, so the panel swapped and the content-sized dialog resized *before mouseup*; a `<dialog>` is centred, so shrinking moved its top edge **down**, out from under the pointer; and a `click` is dispatched to the **nearest common ancestor of the mousedown and mouseup targets**, which for a press inside and a release on the backdrop is the dialog itself — so `event.target === settingsDialog` matched and the dialog dismissed itself mid-click. The dialog is now **one fixed box for every tab** (`height: min(820px, calc(100dvh - 2rem))`, `.settings-card` at `height: 100%`), so no tab switch can move it under the pointer, and the tab row no longer jumps. The dismiss handler additionally requires the gesture to have **started** on the backdrop, which removes the whole class — including the quieter case where dragging to select text in the Advanced document editor and releasing past the edge closed the dialog and discarded the edit.
- **Every tab now fits the fixed box without scrolling**, on Michel's instruction to redesign any tab that did not. Measured in a real browser against 650px of body: general 672→**629**, history 372→353, metrics 884→**621**, thermals 185→174, advanced 783→**447**; the dialog reports a single height (820) across all five tabs. Metrics fits because families now sit **side by side**, each listing its metrics in one column. Advanced is now **two columns** — the OpenTelemetry fields one per row on the left, the raw settings document on the right where a JSON editor wants the height, its editor flexing to fill and its Validate/Apply beneath it at normal button size. General needed no redesign, only a tighter shared rhythm. `overflow: auto` stays on the body purely as a small-viewport fallback. Two selectors were also found **silently dead**: `.metric-family` and `.advanced-document-settings-group` tie with `.settings-group` on specificity and lose on source order, so the first never applied its single-column rule and the second lost `display: flex` to `display: grid` — which laid the JSON editor out *beside* its buttons. Both are now panel-scoped, with a comment saying why.
- **A historical timeline was pushed sideways by live samples** (ADR 0030) — pre-existing, not introduced by T18, and reported as *"if i choose anything which is not NOW in timeline, the current metric keep pushing it"*. `renderSnapshot` called `pushHistory` unconditionally, so every poll appended the current sample to the array the chart draws, whatever window was selected. The visible symptom was live points piling up at the right edge of a window that should end where the user asked; the worse one was silent, because `hydrateHistoryPoints` downsamples a selected window to the 1200-sample render cap, so each live push then made `trimHistory` **evict the oldest point of the chosen window** — about one per tick, replacing the whole selected range with live data inside half an hour. Now only the `live` window charts polled samples (`liveSampleEntersHistory`), while a historical window with nothing scrubbed still renders the **tiles** from the live snapshot (`liveSampleDrivesTiles`) so the chart shows the past and the gauges still report now; a scrubbed selection wins over both. The tile render must run **after** `renderSelectedSample`, which otherwise re-renders the tiles from the window's last stored raw sample and froze the gauges — caught in the browser, not by the wiring test, and now pinned by an ordering assertion.
- Verification: Rust **28 suites / 411 passed / 0 failed / 2 ignored**, Bun **261 passed / 0 failed across 22 files** (254 at 0.7.0; +7 regression tests), `rustfmt --check` and clippy `-D warnings` clean. Behavioural proof for the timeline fix used a control rather than a single observation: on `15m` the sample counter held at 300 across 7 s while the tiles changed 3 times, and on `live` over the same interval the counter moved 161 → 165 — the historical window frozen, live still streaming, tiles alive in both. The fixed-box and fit measurements above were taken in a real browser against a scratch daemon on a throwaway database; the live `:4274` was never used as a test target.

## 0.7.0 - 2026-09-01

Phase 5, Task 18: the OTel metric registry with per-metric export selection, the tabbed settings dialog it needed, and a settings UI that finally looks like a control surface (ADR 0027, ADR 0028). The thirteen exporter instruments stop being string literals at their construction sites and become one `METRIC_REGISTRY` the daemon builds from and `GET /api/otel/metrics` serves, so the dashboard never hardcodes a metric list and "advertised" cannot drift from "exported". Selection is stored as the **disabled** set (`otel.disabledMetrics`, default empty), which is the direction that survives an upgrade: a metric added in a later release ships ON rather than silently off. A well-formed name this build does not know is accepted, preserved byte-identically and shown inert, so one fleet configuration round-trips across TinyTop versions instead of quietly losing a newer release's choices.

**The cost lever, measured rather than asserted.** ADR 0027 justified selection as a per-series cost lever and estimated "of the order of forty active series" on this box, of which turning off `system.filesystem.*` would remove "of the order of half". Measured end to end on this host — a scratch daemon on a throwaway database exporting to a capture receiver, the OTLP protobuf decoded and its data points counted — the real numbers are larger and the lever is stronger than the ADR claimed: **98 series across 13 metrics with everything enabled, 14 series across 11 metrics with the two `system.filesystem.*` metrics disabled — 84 series removed, an 85.7 % drop.** The filesystem pair alone is 84 of the 98 (28 mounts × 1 for utilization, 56 for usage). The two disabled metrics are **absent from the request entirely** — 11 metrics present, not 13 carrying zeros — which is ADR 0027 decision 6 (skipped at record time, never recorded-then-stripped) holding on real traffic.

- Agent / store (T18, `ari-sol-deep`): `METRIC_REGISTRY` as a `[MetricDescriptor; 13]` so the count is enforced by the type, with `descriptor()` panicking on a name the registry does not carry and validation covering uniqueness, the name grammar, families, descriptions, units and the exact 10/3 semantic-convention split. `otel.disabledMetrics` is a set, not a list: at most 64 entries, each matching `^[a-z][a-z0-9._]*$` and at most 128 characters, duplicates refused with the repeated value named. `collect_and_export` builds one `HashSet` per export and skips each disabled gauge before `record()`; disabling every metric exports an empty request **without** advancing the failure counter, because an empty export is a configuration, not an error. `GET /api/otel/metrics` is read-only and `no-store`, and deliberately carries no `endpoint` and no `headersEnvVar` — the header value comes from the service environment and must never reach a settings-shaped response. No dependency added; `Cargo.lock` untouched by the lane.
- Dashboard (T18b, `ari-sol`): the settings dialog becomes five tabs — General · History · Metrics · Thermals · Advanced — with every panel **permanently mounted** and selection expressed only through `[hidden]`. That is the load-bearing decision: because no control is ever detached, `collectDaemonSettingsFromForm` still reads History and Thermals values while Metrics is on screen, so switching tabs can never silently drop a field. A real tablist (roving `tabindex`, arrows that wrap, Home/End), the remembered tab restored through guarded `localStorage` and resolving to General when it is no longer available, and capability-driven tabs — Metrics appears only when the route answers, and a non-OK or failed request hides it with no error state, because an absent capability is not a failure the user needs to read about. The picker hardcodes nothing: it renders whatever the route returns, grouped by family in registry order, and unknown names stay visible, read-only and survive a save untouched. Advanced is a validate-then-apply document editor whose only authority is the server's existing dry-run: Apply enables solely for the exact validated text and any keystroke disables it again. Two client gaps closed — `extraChips` accepted `amdgpu`/`i915`/`nvme` in the browser though the server refused them, and a failed settings save discarded the server's message.
- Settings UI/UX (ADR 0028), on Michel's instruction *"use toggle instead of checkboxes, make elements align, use modern UIUX/CSS"*: every boolean in the dialog is an on/off **setting**, so it is now a switch carrying `role="switch"` rather than a checkbox, which is the browser's answer to a different question. The switch is the native input restyled in place (`appearance: none` + an `::after` knob) rather than a wrapper component, because there are two populations of boolean here — thirteen static fields and the metric rows built at runtime — and one rule set covers both so they cannot drift. The on-state pairs `--cyan` with `--surface`; every theme remaps the named tokens to its own palette, so the switch adopts each theme's accent for free and the knob stays legible in both directions (`ember` `#fb923c`/`#1c1110`, light `solar` `#0369a1`/`#ffffff`). Checkboxes **outside** the dialog — history series, filesystem filter — are deliberately left alone: they select members of a set, which is what a checkbox means. Three alignment defects fixed with it: the family's bulk control was `float: right` and never lined up with its legend; the metric row faked alignment with a magic `margin-top: 0.2rem`; and `.metric-setting-row > span` being a grid gave each metric's *unit* a line of its own, so the unit is now a small tag beside the name.
- Verification: merged-main gate re-run and counted by the orchestrator, before and after the UI change with identical results — Rust **28 suites / 411 passed / 0 failed / 2 ignored**, Bun **254 passed / 0 failed across 22 files** (baseline 228 across 21), `rustfmt --check` and clippy `-D warnings` clean. Both lanes were git-read-only by design, so their verified worktrees were committed by the orchestrator after every claim was validated at source: the registry arity, the `descriptor()` panic, the 64/128/duplicate bounds, the handler's read-only projection and the absence of any secret-bearing field in its response. Rendered-page check in a real browser against a scratch daemon on a throwaway database, **both themes**, covering all five tabs, the keyboard path (roving focus, `ArrowLeft` wrapping from the first tab to the last, Home), the metrics picker with its long `filesystem` family expanded, and the off-state rows — the two filesystem switches rendering off exactly as the API had set them.
- **Census correction.** T18b's lane `ESCALATE` was a containment artifact, not a defect: a hexe lane cannot bind a socket, so the pre-existing `tests/server.test.ts` failed `Bun.serve({ port: 0 })` with `EADDRINUSE`, and the brief had wrongly demanded `bun run check:bun` — a script that starts a server — as the lane's gate. Re-run outside the sandbox the same worktree is 254/0. The lane declined to modify the out-of-scope test or work around containment, which was correct. Separately, the Rust count moved 385 → 411 while the source gained exactly **14** test functions (all T18's, all named in its report, none deleted and `#[ignore]` unchanged at 2). The current run reconciles exactly — 403 annotations + the 10 `thermal.rs` tests that `include!` re-executes inside `thermal_end_to_end.rs` = 413 = 411 passed + 2 ignored + 0 filtered — whereas the **385 recorded for 0.6.0 does not reconcile against its own tree** (389 + 10 − 2 = 397). The apparent +26 is therefore a stale baseline figure, not tests appearing or disappearing; 411 is the first count that has been reconciled against the source.

## 0.6.0 - 2026-08-31

Phase 5, Task 17: opt-in CPU thermals and schema v5 (ADR 0026). Michel's order was "make it opt in option", so thermals ship **disabled by default** and are switched on per host from the Settings dialog or a `config import` document. When enabled on Linux the collector reads the CPU package and per-core temperatures from `coretemp` or `k10temp` (plus anything named in `thermal.extraChips`), keeping the kernel chip name verbatim; `amdgpu`, `i915` and `nvme` are refused, because GPU temperature already ships as `gpus[].temperatureC` (ADR 0022/0025) and NVMe belongs to the parent sensors plan's later disk slice. Schema v5 is purely additive — `sensor_dim` + `sensor_samples` and `PRAGMA user_version = 5`, no rebuild and no pre-image, measured at 0–1 ms on a populated file. Sensor identity is `hwmon-<chip>-<k>-temp<N>` and deliberately excludes the `hwmonN` directory index, which is stable neither across hosts nor across boots: `coretemp` is `hwmon1` on sheep and `hwmon0` on trashcan, and both produce `hwmon-coretemp-0-temp1..5`. Thresholds are reported only when present **and** sane (`0 < t <= 200`), so sheep's `nvme temp2_max` sentinel of 65261850 m°C is absent rather than a number, and the dashboard renders an absent threshold as no threshold — never a gauge maximum, never 0. Disabled means the thermal collector touches hwmon not at all; the GPU backend's read of its own DRM device's hwmon is a separate, pre-existing path and is unaffected.

- Store / collector / types (T17, hexe run 689): the v5 DDL appended to the v4 groups as `CREATE_SENSOR_TABLES_V5_SQL`; `sensor_dim` interned by `stable_id` with the cache primed at connect and `last_seen_ms` written at most once a minute (the ADR 0025 rule); `SensorReading { stable_id, chip, kind, label, value, max, crit }` on `SystemSnapshot.sensors`, `skip_serializing_if = "Vec::is_empty"` so a sensorless host omits the key entirely rather than sending `[]`; `ThermalSettings { enabled, extra_chips }` in the `otel` mould, with an absent `thermal` key on import keeping the persisted block; both ticks guarded so a disabled collector performs no `read_dir`. ADR 0026 amendment: `stable_id` is a real field on the wire, because the identity computed by the collector had no other path to the store and a `#[serde(skip)]` carrier would have silently lied about itself after any JSON round trip.
- Fix round (T17-fix1, hexe run 38 of the dispatcher after 37 resource-guard refusals; luna's blind review raised 7, the orchestrator validated each at source and 3 needed code): `StoreStats::stats()` and `settings_transfer.rs`'s `would_delete` ran `(SELECT COUNT(*) FROM sensor_dim)` unconditionally while `connect_for_inspection` is documented as opening **without** migrating, so `db stats` on a v4 file died with `no such table: sensor_dim` — both now probe the optional tables once and substitute a literal `0`, keeping every field present. **The audit the brief demanded found the same defect class already live for the GPU tables on a v3 file**, fixed the same way. `thermal.extraChips` gained `RESERVED_EXTRA_CHIPS`. One end-to-end proof walks collector → wire → store on a missing hwmon root.
- Dashboard (T17b): the per-chip Thermals panel (hidden when no sensor is present, exactly as the GPU panel is), the CPU thermals settings group, and the history coverage row; `sensorBarPercent` returns `null` when neither `crit` nor `max` is usable and the renderer creates no bar element at all in that case. Reviewed first-hand before the merge (0 HIGH, 1 MED, 3 LOW, nothing blocking) — T17 had a blind review and T17b had none.
- Verification: merged-main gate re-run and counted by the orchestrator rather than taken from any report — Rust **28 suites / 385 passed / 0 failed** (baseline 27/367), Bun **228 passed / 0 failed across 21 files**, `rustfmt --check` and clippy `-D warnings` clean; `cargo audit` 0 vulnerabilities over 296 dependencies (the same 3 allowed warnings carried since 0.4.0), `bun audit` clean. Hardware acceptance over ssh: **sheep** (Intel N97, kernel 6.8) 5 `coretemp` readings, `max`/`crit` 105/105, `nvme` present as `hwmon0` and correctly excluded; **trashcan** (Xeon E5-1620 v2, kernel 7.0) 5 readings at 91/105, **two `amdgpu` chips present at `hwmon2`/`hwmon3` and neither reported**, the unnamed `hwmon1` skipped silently, `db stats` `userVersion 5` with 5 sensors and 35 sample rows. `strace` on both hosts recorded **zero opens under `/sys/class/hwmon` while disabled** (the only hwmon-shaped opens are the GPU backend's own `/sys/class/drm/card*/device/hwmon/` reads, ADR 0022 behaviour since 0.5.4). Rendered-page check in a real browser on the merged build, **both themes**, against sheep's live sensors through an ssh tunnel: five rows under a `coretemp` group, `max 105 °C · crit 105 °C`, bar fill 52.381 % for 55 °C against the 105 °C ceiling, complete `aria-label` per row, and zero console errors; the sensorless local host renders the panel hidden with the settings group still available.
- Known limitations recorded rather than papered over: ADR 0026 gains a known-limitation section for the residual `stable_id` fork when two **same-name** chips swap sysfs order across a reboot (unreachable on every current fleet host, since `<k>` is only load-bearing when a chip name repeats; the backlog fix is the ADR 0025 `pci-<PCI_SLOT_NAME>` stable-device-path trick). `tinytop-agent collect --json` builds a default collector and does **not** read persisted settings, so it never reports sensors — the daemon is the path that honours the flag, and the README now says so. (Fixed in 0.7.2: `collect --sqlite <db>` now loads that database's settings; bare `collect --json` stays hermetic by design.) T17-fix1's end-to-end test reaches the collector through `include!`, which re-executes that file's own 10 unit tests inside the new target: of the +18 passes, **8 are new coverage and 10 are duplicates**, so removing the `include!` would yield 375, not a loss of ten tests.

## 0.5.4 - 2026-08-30

Phase 5 lane 4, Task 15: the GPU collector on Linux plus schema v4 (ADR 0025). Adapters are discovered from DRM sysfs (`/sys/class/drm/card<N>`, `card<digits>` only, `drm-driver` mandatory); busy is read from `gpu_busy_percent` where the driver exposes it and otherwise derived from the per-engine `drm-engine-*` / `drm-cycles-*` (or `drm-total-cycles-*`) deltas in `/proc/<pid>/fdinfo` — Δ over the engine capacity × Δt, the adapter reported as its busiest engine — so `busyPercent` is `null` whenever no readable DRM client exists (the "no evidence" reading of ADR 0025 decision 5, not 0 %); a failed busy verdict is cached until the next re-detect; VRAM and temperature come from the adapter's sysfs and hwmon where the driver exposes them; NVIDIA's proprietary driver is identity-only; WSL2 exposes no adapter. Only pids whose `fdinfo` the daemon's user can read contribute (the readable-pid caveat — 8 readable / 242 denied on trashcan); no subprocess and no vendor library is ever used. Schema v4 rebuilds both process tables with `started_at_ms` (the RFC 3339 `started_at` text converted, unparsable values counted before the drop), interns adapters in `gpu_adapters` and stores one `gpu_samples` row per adapter per tick — all in ONE guarded transaction with per-table row-count guards; `GET /api/history/gpus` and the `db stats` GPU counts read them back. Windows (PDH + DXGI) and macOS (IOKit) are Task 16.

- Store / collector / agent (T15, hexe run 674 on the second dispatch — run 670 escalated correctly on the pinned `time` crate lacking its `parsing` feature; the feature was authorised on the existing pinned line and `Cargo.lock` is unchanged by the lanes): `GpuBackend` detection inside the slow tick, `sample()` + `process_busy()` every tick, the collector built GPU-free under test so no non-ignored test touches `/sys` or `/proc`; the v4 DDL, `migrate_v3_to_v4` with its `schemaMigrated` marker (`fromVersion 3, toVersion 4, fastRows, minuteRows, startedAtUnparsed, durationMs`); GPU rows written inside the metric transaction before its commit, the adapter cache primed at connect and `last_seen_ms` refreshed at most once a minute; `started_at_ms` and every GPU metric read as `Option` on both tiers; the GPU prune with its own label and `wouldDelete.gpuSampleRows`. T15-fix1 (hexe run 676, after luna run 675's findings validated by Fable — 1 P1 re-ranked P2, 2 P3, 15/17 claims held): `parse_fdinfo` treats `drm-total-cycles-*` as the cycles form (Xe prints both prefixes; the diagnostic fires on either), the sqlite-architecture current-schema line, the `migration_v3.rs` schema-equality comment, the INSTALL v4 wording (a nullable `gpu_percent` column on the minute tier).
- Dashboard (T15b, hexe run 671; luna run 673: no finding at any severity, nine claims held): the GPU panel (absent-safe, hidden when no adapter is present), the row-gated GPU column in the process table with its sort key (`—` rows sort last through the `-1` sentinel) and the detail-row `GPU` line, `wouldDelete.gpuSampleRows` in the import dry-run, GUIDE coverage; the legacy Bun runtime is unchanged.
- Verification: lane gates outside the sandbox (`gate-t15-lane.log`, `gate-t15-fix1.log`, `gate-t15b-lane.log` in the Temple): Bun 197 → 209 across 20 files, Rust 301 → 344 passed / 0 failed / 2 ignored across 27 suites, `rustfmt --check` + clippy `-D warnings` clean, `tinytop-types` + `tinytop-collectors` `cargo check` for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` clean; real-file migration of a fresh copy of the live v1 database (`gate-t15-realfile.log`): v1→v2 394 ms, v2→v3 2,548 ms, v3→v4 34 ms (11,987 minute rows rebuilt, 0 unparsable start times), 3,215 ms in total, integrity ok; the daemon migrated another fresh copy itself in 264 + 1,014 + 18 ms; hardware acceptance (`2026-08-30-t15-acceptance-checklist.md`): trashcan (2 × amdgpu GCN 1, the fdinfo path) `busyPercent` 96.8 % under a Mesa EGL load, VRAM 6.0 → 19.2 MB, 44 → 57 °C, `gpuPercent` on the load pid only, scan 4.53 ms over 8 readable / 242 denied pids; sheep (i915) 97 % in 2.90 ms; WSL2 `gpus` absent, `db stats` `userVersion 4` with 0 adapters / 0 samples, `process_samples_fast` 51.9 B/row + 19.8 B/row index (plan target ≤ 60; the 0.5.3 baseline was 66.7); merged-main gate (`gate-main-t15-merged.log`) Bun 209/20, Rust 344/0/2; the rendered-page check on the merged release build: WSL2 (panel + column hidden, live and 30 minutes back over a fresh live copy the daemon migrated v1→v4 itself) and trashcan through an SSH tunnel (2 adapters, 94–96 % busy, the GPU column, real sort clicks in both directions with `tinytop.processSort` persisted, the detail dialog at `GPU 94.8 %`).

## 0.5.3 - 2026-08-30

Phase 5 lane 3, Task 14: schema v3 (ADR 0024) plus the dashboard's replay repair — and the fix for a production regression that had blanked the overview, process, filesystem and pressure panels on every release since 0.3.1. `metric_samples` is rebuilt without `snapshot_json`: each row stores its own `uptime_seconds`, `memory_available_bytes`, `swap_free_bytes`, `last_pid` and the filesystem enumeration stamp, `runnable_threads`/`total_threads` become nullable, and the eight identity strings are interned once in `host_identity`. Filesystems are stored on change, keyed by the collector's enumeration stamp, with `fs_mount_events` recording each mount's appearance and disappearance; `/api/history` snapshots are assembled from the typed tables in window-sized batches (no `cpu.times`, no pressure lines, load source fields absent when the collector has none). The v2→v3 migration is one transaction with a guard before the drop: every row that still holds JSON is decoded and must come out assembleable, or the file is left untouched.

- Store (T14, hexe run 661): the v3 DDL with the `CHECK (identity_id IS NULL OR (uptime_seconds, memory_available_bytes, swap_free_bytes all NOT NULL))` invariant; `insert_snapshot` interns the identity, writes the typed scalars, and writes filesystem rows/events only for a NEWER enumeration stamp whose mounts appeared, disappeared or changed; process rows are written after the metric row commits in their own transactions, failures isolated behind a once-a-minute warning; the prune keeps each mount's newest row and event as the carry-forward floor; every runtime JSON path, `snapshotJsonKeepMinutes` and the JSON coverage counters are gone (the key is still accepted and ignored on PUT and import, the import warning reads `snapshotJsonKeepMinutes is no longer used and was ignored`).
- Migration (T14-fix1, hexe run 665; ADR 0024 amendment): a fresh backup of the live v1 database was REFUSED by the guard — 25 rows written by the legacy Bun collector carried `filesystems[].inodeUsed = -999001` (its unclamped `inodeTotal − inodeFree` on a WSL drvfs mount that reports more free inodes than total). The backfill now normalises a legacy payload's negative `inodeUsed`/`inodeTotal` to absent and counts those rows (`legacyInodeRowsNormalised` in the migration audit and the `history migration info` line); the refusal stays for JSON this version does not know, with a remedy that names the manual `UPDATE metric_samples SET snapshot_json = NULL WHERE sample_id = <n>` (INSTALL.md §Upgrade, "Refused schema migration").
- Types/collectors: `LoadSnapshot.runnable`, `total_threads`, `last_pid` are `Option<u64>` (absent when a collector has no source; Linux keeps `Some`); the T12-fix1 `process_totals` stopgap is removed.
- Dashboard (T14b, hexe run 660; T14b-fix1, hexe run 664 after luna run 662's P0): raw-window samples are normalised through one constructor (`normalizeHistorySamples(…, "raw")`) on BOTH the fetch path and the live-poll path — `pushHistory` had stored no `source` since T5-fix1 (`9770dbf`), so `renderSelectedSample` refused every live poll and the overview/process/filesystem/pressure panel was blank on 0.3.1–0.5.2 (reproduced with Playwright on the live 0.3.1 page, proven fixed on a rendered page); history rows render `—` for pressure and the threads line as a number; the JSON control and `snapshotJsonRows` leave `WOULD_DELETE_FIELDS`.
- Fix round (T14-fix2, hexe run 668, after luna run 667 validated by Fable — no P0; her P1 on the regressing enumeration stamp validated as a contract gap, ADR 0024 amendment #2: the on-change rule is monotonic and the discard is now warned once a minute): full ordered `table_info` equality migrated-vs-fresh, the carried filesystem VALUES asserted, three documentation corrections (`fs_samples` DDL excerpt, the non-existent in-progress migration guard, "full/complete raw snapshots"), the u64→i64 overflow remedy.
- Verification: lane gate outside the sandbox (`gate-t14-lane.log`, `gate-t14-fix1.log` in the Temple): Bun 192/192 (19 files), Rust 295 → 300 passed / 0 failed / 1 ignored across 25 suites, `rustfmt --check` + clippy `-D warnings` clean, `tinytop-types` + `tinytop-collectors` `cargo check` for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` clean; real-file migration of the 337 MB live v1 backup (42,893 rows, 2,451 JSON rows): v1→v2 327 ms, v2→v3 2,748 ms, 25 rows normalised, integrity ok; acceptance drivers (`2026-08-30-t14-acceptance-checklist.md`): daemon-driven v1→v2→v3 358 + 2,541 ms with the guard count matching (2,461 = 2,461), on-change filesystems 4.89 rows/mount (vs ~162 per detail tick before), `metric_samples` 217.7 B/row, `/api/history` 1 h window (2,360 rows, 110 MB) 287–312 ms on the release daemon (plan amendment (l): < 500 ms); merged-main gate Bun 197/197, Rust 301/0/1; the rendered-page check on the merged build (Playwright on a temp daemon over a fresh live copy): the live point and a point 30 minutes back both render processes, filesystems and a numeric thread count, pressure `—` for history.

## 0.5.2 - 2026-08-29

Phase 5 lane 2, Task 13: schema v2 (ADR 0023). Process history stops repeating command lines: a `process_commands` dictionary (`command_id INTEGER PRIMARY KEY`, `UNIQUE(command)`) interns each distinct command once, the minute table `process_samples` carries `command_id` instead of the `command` text (the column is dropped with `ALTER TABLE … DROP COLUMN`), and a new `process_samples_fast` table (WITHOUT ROWID) keeps one row per top-N process per poll tick for `processFastKeepHours` (1–72, default 24). A v1 file migrates to v2 in ONE transaction — dictionary, fast table, `command_id` backfill verified complete, both command indexes, the column drop, one `schemaMigrated` marker, `PRAGMA user_version = 2` — behind a `sqlite_version() ≥ 3.35.0` check made before any write and an in-flight guard; there is no pre-image and no `VACUUM` (ADR 0023). Measured on a read-only copy of the live 225 MB v1 file (8,439 process rows, 465 commands): 273 ms in the ignored real-file test, 199 ms daemon-driven with exactly one marker and no unmapped row. The Bun runtime is untouched (the legacy runtime is not a v2 consumer).

- Store: `process_commands`, `process_samples_fast`, `command_id` on `process_samples`; `insert_snapshot` interns commands inside its transaction and writes the fast row every tick and the minute row on the detail interval; `read_history_processes` answers from the fast table when `sinceMs` lies inside the keep window and from the minute table otherwise, joining the dictionary; `migrate_v1_to_v2`; `MaintenanceReport.detail_rows_pruned` carries the prune count (`detail_rows` keeps its meaning: detail rows written since the last pass); expired fast rows are pruned each pass and orphaned commands drained in 1,000-row batches only after a prune removed something.
- Settings: `retentionLadder.processFastKeepHours` (1–72, default 24) on the Rust runtime, validated on both sides; `wouldDelete.processFastRows` is counted unconditionally like every tier; export/import carries the key and older documents without it keep the default.
- API/CLI: `/api/history/processes` takes `sinceMs` (alias `since_ms`) and reports `source: "fast" | "minute"`; `db stats --json` gains `userVersion`.
- Dashboard: the retention-ladder group gains the fast-process keep control (hidden with the group on the Bun runtime); the import preview lists `processFastRows`.
- Docs: README (persistence table, schema v2), GUIDE, `docs/guides/API.md`, `docs/sqlite-history-architecture.md` (schema v2 and the one-transaction migration), plan §4 contingency, ADR 0023.
- Fix round (T13-fix1, hexe run 657, after luna run 655 validated by Fable — no P0; her P1.1 was the review brief's two-dot diff instrument, two P1s re-ranked P3, one P2): a `DROP COLUMN` failure inside the v1→v2 transaction is reported with SQLite's own message instead of the version refusal (the version had already been proven before any write), and the rollback is exercised for the first time — a probe index on `process_samples.command` makes the migration refuse (`error in index idx_probe_command after drop column: no such column: command` on the linked 3.51.3) and leaves the file at `user_version 1` with its rows, columns and zero markers, then succeeds once the index is gone; the growing-horizon dry-run test proves its zero comes from a maintenance pass (a 30 h row counts 1 first, `process_fast_rows 2` after the pass); the interning test compares the per-table `command_id` of one capture/rank with the dictionary's; the source-selection test plants a fast-only `pid 424242` capture so the fast read (2 captures) and the minute/open reads (1, never 424242) can no longer pass on swapped tables.
- Verification: lane gate outside the sandbox (`gate-t13-lane.log`): Bun 192/192 (19 files), Rust 276 passed / 0 failed / 1 ignored across 23 suites, `rustfmt --check` + clippy `-D warnings` clean, live daemon active; fix-round gate (`gate-t13-fix1.log`) 277/0/1 + 192/192; Fable's acceptance 12/12 on a temp daemon and the live-file copy (`2026-08-29-t13-acceptance-checklist.md` in the Fabulous fleet notes): fresh files start at v2 with no marker, the daemon migrates the copy in 199 ms with one marker, the read rule and per-call settings are live, `processFastKeepHours` 0 and 73 → 400 byte-exact, `db stats --json` `userVersion 2`, 0 orphans, ranks 0–7 in both tables. **`process_samples_fast` measures 66.7 B/row (table) + 19.1 B/row (index) against the plan's ≤ 60 B target — reported, not tuned: `started_at TEXT` (~20 B) is T14's interning decision.** Release gate on main: `gate-release-0.5.2.log` in the Fabulous fleet notes.

## 0.5.1 - 2026-08-29

Phase 5 (cadence classes and GPU telemetry, ADR 0021/0022) opens with its first lane, Task 12: the collector now owns three cadence classes — fast (CPU, memory, swap, load, pressure, processes, uptime on every `pollIntervalMs` tick), slow (filesystems every `retentionLadder.detailIntervalSec`, served from a cache between checks and stamped `filesystemsCapturedAtMs`), static (hostname / kernel / distro, re-read on the slow tick) — so `statvfs` runs once per mount per interval instead of once per mount per 1.5 s tick; `/api/snapshot` answers from the daemon's memory; `topProcessCount` is finally effective; `cpu.times` is optional instead of twelve fake zeros on the sysinfo collectors.

- Types: `cpu.times` is `Option<CpuTimes>` (present on the Linux collector, absent on macOS/Windows) and `SystemSnapshot` gains the additive `filesystemsCapturedAtMs` (Unix ms of the last mount enumeration); documents without either key still deserialize.
- Collectors: `CollectorConfig { top_process_count, filesystems_interval }` and `Collector::configure()`; the Linux sources split into `collect_fast_sources` / `collect_slow_sources` with an injectable monotonic clock and hidden counters for tests; the slow cache is never reset by `configure` (a shorter interval can make the next tick due at once); `topProcessCount` (1–50) replaces both hard-coded `take(10)` / `truncate(10)`.
- Agent: `/api/snapshot` and `/snapshot/latest` read the published snapshot from memory (`503 {"error":"no snapshot yet"}` only before the first collection; the daemon collects once before it binds); `collect_and_store` re-configures the collector only when `topProcessCount` or `detailIntervalSec` changed, one tick after the settings write; the tick order stays collect → publish → insert → settings → maintain.
- Dashboard: the Filesystem panel shows `as of hh:mm:ss` (`describeFilesystemFreshness`, strict one-poll boundary) when its rows are older than one poll; hidden on the Bun runtime.
- Docs: README implementation notes and persistence table (`detailIntervalSec` = filesystem check interval), GUIDE refresh behaviour, ARCHITECTURE data flow, API `/api/snapshot` freshness fields and the 503.
- Fix round (T12-fix1, hexe run 648, after luna run 644 validated by Fable): the sysinfo (macOS/Windows) collector derived `load.totalThreads` and `load.lastPid` from the already-truncated top-N list — a constant 10 "threads" since the collector existed, tracking `topProcessCount` after this change; both now come from the full process table via the ungated, unit-tested `process_totals` (`totalThreads` is the PROCESS COUNT on those platforms — sysinfo has no thread totals — documented on the field and in API.md; the honest `Option` lands with schema v3, plan Task 14). The filesystem-interval control is labelled `Filesystem check seconds` (GUIDE had named a label that did not exist).
- Verification: lane gate outside the sandbox on the lane worktree — 21 Rust suites 261/0, Bun 188/188, clippy `-D warnings` and `rustfmt --check` clean, both offline cross-target `cargo check`s (`x86_64-pc-windows-msvc`, `aarch64-apple-darwin`); fix-round gate 264/0 + 188/188; Fable's acceptance on a temp daemon (`:4290`, scratch DB): `filesystemsCapturedAtMs` moved once per 60 s while `timestamp` moved every poll, `topProcessCount 3` applied on the next tick; the per-mount `statvfs` syscall count is NOT measured (no `strace` on the box). Release gate on main: `gate-release-0.5.1.log` in the Fabulous fleet notes.

## 0.5.0 - 2026-08-29

Phase 4 of the tiered history ladder closes the plan with one task and its two review rounds: an OpenTelemetry metrics push exporter for the Rust daemon (ADR 0015, spec §12) — off by default, HTTP/protobuf only, driven by the daemon's own task through the SDK's `ManualReader` (behind `opentelemetry_sdk`'s `experimental_metrics_custom_reader` gate, accepted under exact pins `opentelemetry 0.32.0` / `opentelemetry_sdk 0.32.1` / `opentelemetry-otlp 0.32.0`; vetting report `docs/reports/2026-08-29-dependency-vetting-opentelemetry.md`), pushing the latest collected snapshot as the thirteen §12 gauges with a `service.name` / `service.version` / `host.name` resource, request headers read only from the environment variable the operator names (never stored, never exported, never logged), a cumulative failure counter with a once-a-minute warning, and status surfaced through `/api/history/coverage`, `db stats --json` and the dashboard's OpenTelemetry group (Bun keeps no exporter). The lock delta is 203 → 296 packages and the release binary grew by 7,170,632 B at T11 (10,614,800 → 17,785,432 B; re-measure at the next release build); `aws-lc-sys` makes a C compiler a build prerequisite (CMake only on its fallback paths). Lanes: T11 = hexe runs 625 (a correct escalation on the SDK feature gate) → 627; T10-fix2 = run 626; fast blind review luna 630 → fix round 632 (the status lock held across the disabled-branch sleep — coverage latency 4.15 s → 9 ms measured); the Phase-4 deep dual-blind sol 633 ∥ luna 634 (no P0; one P1) → fix round 637 (P4-fix1). With this release the tiered-history-ladder plan (T1–T11, four phases, ADRs 0013–0020) is complete.

- Dependencies: added exact-pinned OpenTelemetry metric exporter dependencies for Rust HTTP/protobuf push export.
- Store: added the additive, secret-free `otel` settings block and persisted-value compatibility for imports that omit it.
- Exporter: added Rust-daemon-only latest-snapshot OTLP gauges, environment-provided headers, independent task execution, and rate-limited failure handling.
- API/coverage: exposed OTel status, interval, last success/failure, error, and failure count through `/api/history/coverage`.
- CLI: extended `db stats` with OTel exporter status and the headers environment-variable name/presence, never its value.
- Dashboard: added the Rust settings dialog's OpenTelemetry group and coverage status while keeping Bun without an exporter.
- Docs: documented OTLP metrics, units and attributes, systemd environment setup, collector configuration, settings transfer, and two-runtime behavior.
- Dashboard: the import confirmation now names a retention-ladder change that affects no stored history (`retention ladder changes — no stored history is affected`) and an imported document identical to the current settings, instead of listing only the other keys or opening empty; the save path still shows no dialog for a zero-impact change (T10-fix2, hexe run 626; found by the 0.4.1 acceptance pass).
- Exporter fix round (T11-fix1, hexe run 632, after luna run 630 and Fable's acceptance pass): the export loop no longer holds the status lock across its idle sleep while export is disabled — on a default install every `/api/history/coverage` request could wait up to 5 s (measured 4.15 s → 9 ms); settings changes are applied on the next 5-second tick regardless of an in-flight export (≤ 10 s with a receiver hung to the timeout; measured 8.9 s); the loop never builds a pipeline before the first snapshot exists (no `host.name = "unknown"`); `intervalSec` is clamped to 5–3600 when the stored document was edited outside the settings API; `tinytop.load.percent` and `tinytop.pressure.*` carry the `%` unit; the 64-character resource-attribute key cap is named in the validation message on both runtimes; loop-level tests with an injectable tick; `cmake` and a C compiler documented as build prerequisites (`aws-lc-sys`).
- Phase-4 fix round (deep dual-blind sol run 633 ∥ luna run 634, validated by Fable; hexe run 637): `otel.endpoint` refuses URL credentials and host-less URLs (reqwest would have turned `user:password@` into a Basic `Authorization` header while the URL lived in settings, the export document, coverage, `db stats` and stderr — ADR 0016's invariant is now enforced at the validator) and secret-shaped `resourceAttributes` keys (one shared nine-word constant on both sides); settings writes decode and merge the document inside the `BEGIN IMMEDIATE` transaction (`put_settings_document`, used by PUT and import) so a concurrent write can no longer be reverted by a document that omits a block, and the change marker names the pair actually replaced; `OTEL_EXPORTER_OTLP_HEADERS` / `OTEL_EXPORTER_OTLP_METRICS_HEADERS` cannot be selected as `headersEnvVar` and their presence refuses the pipeline unconditionally; a hung-receiver test proves collection completes while an export is pending; `db stats` presence is proven for both branches; the 64-character key cap has its 65-character case; dashboard expectations use the production formatters; the interval clamp's comment states the store's validation; docs: GUIDE's privacy paragraph names the one outbound path, INSTALL/README/the vetting report require a C compiler (CMake only on `aws-lc-sys`'s fallback paths) and note that `http-proto` compiles the trace feature, spec §12 amendment #3 and a dated ADR 0015 amendment record `ManualReader`, the exact pins and the filesystem `type` attribute.

## 0.4.1 - 2026-08-29

Phase 3 of the tiered history ladder closes with one task: a versioned, secret-free settings document that moves between daemons (ADR 0016) — `GET /api/settings/export`, `POST /api/settings/import` (`?dryRun=true` previews with server-computed `wouldDelete`), the `config export` / `config import` CLI verbs, and the dashboard's Export/Import buttons, which also replace the client-side "approx." retention estimates with the same dry-run. The blind review (luna run 617) and its fix round closed a save-path regression the task had introduced (a theme-only save prompted "also changes: defaultTheme"), a stranded `<FILE>.tmp` after a failed export write, a missing directory fsync after the publish, and the hard-link publish on filesystems without `link(2)` (FAT/exFAT — now a re-checked rename fallback with its window documented), and added the missing version-type, invalid-path-invariant and one-object-refusal tests. Audits at the tag: `cargo audit` 0 vulnerabilities (3 pre-existing allowed warnings), `bun audit` clean; `user_version` stays 1 — no migration.

- API: added Rust-only settings export and import routes, including attachment metadata, read-only dry-run validation, exact candidate-horizon deletion counts, warnings, authoritative apply-time validation, maintenance, and source-qualified import markers.
- CLI: added no-create `config export` and `config import` commands with atomic no-overwrite files, structured dry-run/refusal output, round-trip markers, and maintenance deferred to the daemon's next tick.
- Dashboard: added Rust-capability-gated Export/Import controls and replaced retention estimates with server-computed dry-run counts; disabling tiers or archive reads now says their stored tables/files are retained.
- Store: centralized transfer envelope validation, planning, application, changed-key calculation, legacy-mirror normalization, prune-predicate counts, and import marker details in one shared module without adding a dependency or schema change.
- Fix round (luna run 617, hexe run 619): `describeImportPlan` takes `{ includeOtherChanges }` so a theme-only save no longer prompts; `config export --out` removes its `.tmp` on a failed write/sync, fsyncs the directory after the publish, and falls back to a re-checked `rename` where hard links are unsupported; `importSettingsFile` no longer shadows the DOM `document`; tests for `"1"`/`1.5` config versions, zero-event invalid paths, and the CLI's single refusal object.

## 0.4.0 - 2026-08-29

Phase 2 of the tiered history ladder closes. Since 0.3.0 the daemon gained the queryable archive (0.3.1; ADRs 0014, 0018, 0019 — copy, fsynced commit, key-set verify, full-row delete, watermark inside the delete transaction, after the lane escalated on SQLite's file-by-file WAL commit order and the blind review found the interval-count livelock), the verified monthly cold export with its `db archive` commands and the Phase-1 CLI carry-overs (0.3.2; a month is exportable only once every hour of it has expired from L4; the command centre became hermetic under test after a gate stopped the live service), and the disk-pressure check with its two timeline markers (0.3.3; ADRs 0017 and 0020 — first check at start on a blocking thread, one `BEGIN IMMEDIATE` transaction per check, an undeterminable measurement keeps the last state). The Phase-2 deep dual-blind review over `v0.3.0..v0.3.3` (sol + luna, one brief, 21 claims) and its fix round are recorded in the Fabulous archive; its fix round closed two P0s (the cold export could seal a month whose rows were still leaving main; the command-centre harness still wrote a PID file under the real state directory) and a P1 (`put_settings` read the pressure state outside its transaction), tightened the CSV verifier and archive point reads, capped an export pass at twelve months, and made the workspace clippy-clean with clippy in the gate. Audits at the tag: `cargo audit` 0 vulnerabilities (3 pre-existing allowed warnings), `bun audit` clean. `user_version` stays 1; the archive and cold export stay off until enabled.

- Prevented cold export from sealing a month until every row has left the main database; candidate evaluation now stops at the first month still being moved.
- Made command-center test runs hermetic by isolating home/XDG paths and system command stubs per invocation.
- Tightened cold CSV verification to reject quotes in unquoted fields and characters after a closing quote.
- Made archive-point reads apply the same schema inspection and newer-schema refusal as archive coverage and manifests.
- Bound one cold-export pass to twelve oldest eligible months while leaving archive-status manifest listings unbounded.
- Enforced Rust formatting, warning-free Clippy, and workspace tests in `check:rust`; removed the remaining mechanical Clippy findings.
- Serialized settings growth validation with the persisted disk-pressure state in one immediate transaction.

## 0.3.3 - 2026-08-29

Phase 2 Task 9: the disk check (spec §9 hourly block, §5 pressure rule; ADRs 0017 and 0020). The Rust daemon now measures free space on the filesystem holding the database — once immediately at start, then every `retentionLadder.diskCheck.intervalMinutes`, on a blocking thread so a hung mount cannot stall the HTTP runtime — and keeps `history_state.diskPressure` as a four-transition state machine: crossing below `minFreeBytes` activates pressure, records `sinceMs` and writes one `diskPressure` timeline marker; a continuing breach refreshes the numbers and writes nothing; recovery clears it with one `diskRecovered` marker. Pressure never deletes anything; it only refuses extending a horizon or enabling a tier or archive, exactly the rule Phase 1 already enforced from a state nobody was writing. ADR 0020 settles what the spec left open: an undeterminable measurement writes nothing and keeps the last known state (the migration's one-shot guard fails closed; a standing check must not refuse valid settings after a transient enumeration failure), there is no hysteresis, and the previous state is read and the new state, `lastDiskCheckMs` and any marker are written inside one `BEGIN IMMEDIATE` transaction. The first lane (hexe run 596) escalated correctly on a brief that had excluded the test file whose `DiskPressureState` literals needed the new `sinceMs` field; the blind review (luna, run 600) found the read-modify-write outside the transaction and two test gaps, fixed in the fix round (run 603). Coverage and `db stats --json` gain `pressureSinceMs`; the dashboard gains red/green colours for the two markers. Follow-up recorded in the backlog: refuse growth when no successful check has happened for more than two intervals (a stale healthy state is a signal, not a boundary). No on-disk schema change; `Cargo.lock` unchanged.

Phase 2 Task 9 adds the Rust daemon's disk-pressure check (spec §9; ADRs 0017 and 0020). It runs once at daemon start and then at the configured interval, measures the mount containing the database on a blocking thread, and commits the pressure state, last-check time, and any transition marker atomically. An undeterminable measurement keeps the last known state and check time unchanged.

- Added injected and real `sysinfo` free-byte providers plus a four-transition disk-pressure state machine with `diskPressure` and `diskRecovered` timeline markers.
- Refuse only retention growth, tier enables, and archive enables while pressure is active; shrink operations remain available and disk pressure never deletes history.
- Added the immediate, repeating daemon disk task; settings are re-read per tick, so an interval change applies at the next tick without ending collection on errors.
- Exposed `pressureSinceMs` alongside `freeBytes`, `minFreeBytes`, `pressure`, and `lastCheckMs` through history coverage and `db stats --json`.
- Added red/green timeline colors for the two disk markers and kept the existing coverage-driven pressure banner Bun-neutral. Rendering “since …” in the banner is deferred to a later UI task; this release exposes the timestamp through the API and CLI only.
- Added temp-database integration coverage for breach, continuing breach, recovery, healthy refresh, undeterminable measurements, settings refusal/recovery, HTTP coverage, CLI reopen, and real-directory measurement.
- Serialized the disk-pressure read-modify-write under `BEGIN IMMEDIATE` on one connection, preventing concurrent callers from recording duplicate transition markers.
- Clamp hand-edited disk-check intervals to the validated 5–1,440 minute range before sleeping and log an out-of-range stored value once per tick.

## 0.3.2 - 2026-08-29

Phase 2 Task 8: the cold export. When `retentionLadder.archive.cold` is on, the daemon writes each completed UTC month of the queryable archive as `tinytop-1h-YYYY-MM.csv.gz` with a `sha256sum -c`-compatible sidecar, records it in `archive_manifest`, and advances `coldExportedUntilMonth` (spec §9/§14; ADR 0014 Decision 2). The spec's month rule alone would have exported a month on its first archived row and then locked the rest out behind the monotone watermark — rows reach the archive one hour at a time as they expire from L4 — so a month is exportable only once it is `coldAfterMonths` old, every one of its hours has expired from main (`end_of_month + l4.keepDays + 1 day ≤ now`), and it is past the watermark. Every file is verified before it is published: written to `.tmp`, fsynced, hashed, decoded again and checked for header, row count, record width and first/last bucket, then renamed, the directory fsynced, sidecar, manifest row, watermark — a failure at any step names it and leaves the queryable archive untouched; cold export never deletes archive rows. The lane (hexe run 584) also closed three CLI carry-overs from Phase 1: every `db` path now closes its store so the WAL is checkpointed on exit (the root of the `cli_db` fixture flake), inspection of a missing database refuses instead of creating one, and `/api/history/points` rejects `limit=0` and inverted ranges with 400s that name the values. The blind review (luna, run 589) found the `cold fsync` step naming two failure points on both sides of the rename, a record-width gap in verification, an incomplete-archive read that reported an empty manifest, and a month-listing/`time` mismatch for negative timestamps — all fixed in the fix round (runs 593/594). The fix round also made the Bash command-center hermetic under test: `bun run check` on a box with `tinytop.service` installed ran the real `./tinytop stop` and stopped the live daemon, so `TINYTOP_SYSTEMD_UNIT_DIR` now overrides the unit directory and the Bun harness always points it at an empty temp dir. Dependencies (rule 5): `flate2` pinned at 1.1.10 (released 2026-08-28: gzip writer infinite-loop fix, incomplete-stream rejection) and RustCrypto `sha2` 0.11.0; `zlib-rs` appears in the lock through a weak feature and is never compiled. No on-disk schema changes (`user_version` stays 1; `archive_manifest` existed since 0.3.1).

- Export complete UTC archive months only after the configured calendar age, every hour has expired from finite L4 retention, and the prior cold watermark has passed; disabled or forever L4 has no exportable months.
- Added read-only manifest inspection and one-pass, oldest-first cold export through a standalone archive connection, without attaching it to the main pool or deleting queryable archive rows.
- Write DDL-ordered RFC 4180 CSV through pure-Rust gzip level 6, fsync and hash the temporary file, decode it again to verify row count and boundary buckets, then atomically publish the file, checksum sidecar, manifest row, and watermark in order.
- Added step-specific cold-export errors and convergent retry behavior; a failed pass leaves the queryable archive intact and may leave only safe-to-remove temporary or replaceable output files.
- Run cold export hourly after a one-minute startup delay; errors are logged without stopping collection or the scheduler.
- Report manifest-backed cold file counts and bytes in history coverage and `db stats`.
- Added read-only `db archive status` and one-pass `db archive export-now`, with structured refusals for disabled cold/queryable settings and no-create handling for missing databases and archives.
- Added archive eligibility, verified files and sidecars, corruption recovery, exact CSV round-trip, no-delete, incomplete-month, manifest no-create, CLI, and help-contract coverage.
- Close every CLI-opened SQLite store so the last connection checkpoints and removes its WAL, including one-shot collection.
- Refuse `db stats`, `db check`, `db vacuum`, and `db archive` inspection of a missing main database without creating the file, sidecars, or parent directory.
- Reject `/api/history/points?limit=0` and inverted `sinceMs`/`untilMs` ranges with field- and value-specific HTTP 400 errors.
- Pinned `flate2` 1.1.10 with its default pure-Rust `miniz_oxide` backend and RustCrypto `sha2` 0.11.0 for gzip and streamed SHA-256 verification; the inert `zlib-rs` 0.6.7 weak-feature lock entry is never compiled.

## 0.3.1 - 2026-08-29

Phase 2 of the tiered history ladder opens with Task 7, the queryable archive (spec §6/§9/§10; ADRs 0014, 0018 and 0019). Expired hourly (L4) rows now move into `history-archive.sqlite` instead of being deleted when `retentionLadder.archive.queryable` is on, and `source=auto` reads fall through to it for ranges older than L4. The lane that built it (hexe run 573) escalated — correctly — on the move mechanic the plan prescribed: with the main database in WAL mode, SQLite commits attached files one by one, `main` first, so a single cross-file transaction could delete a batch before its copy was durable. ADR 0018 replaced it with copy → commit → verify → delete, and the blind review of that fix (luna, run 576) found the interval-count verify livelocking after a partial batch and the two-column delete match; ADR 0019 settled key-set verification, full-row equality, an fsynced archive commit and a watermark inside the delete transaction. Nothing here touches the on-disk main schema (`user_version` stays 1); the archive file is created only by a move, never by a read or by `db stats`.

- Added the queryable hourly archive at `history-archive.sqlite`, relocated by `retentionLadder.archive.directory` when configured. Per ADRs 0018 and 0019, expired L4 rows move by committing and fsyncing an `INSERT OR REPLACE` archive copy with `archive.synchronous = FULL`, verifying every selected key exists in that committed copy, and only then committing a full-row-equality main deletion with `archiveMovedUntilMs` in the same transaction; maintenance work remains bounded per tick.
- Made archive schema creation transactional across all three objects and `PRAGMA user_version`, preventing a stopped initialization from leaving a partial `user_version = 0` archive that later runs refuse.
- Implemented read-only, no-create archive point and coverage reads. `source=auto` can now return archived hourly points with `available:true`, while explicit archive reads remain empty and unavailable when the queryable archive is disabled.
- Added archive failure/convergence, relocation, auto-read, idle-detach, delete-mode, coverage/no-create, and in-process HTTP regression coverage using temp-directory databases only.
- Restored the seven-column rollup history-point read path so migrated v0 one-minute rows remain readable without decoding migration-added nullable minimum/maximum columns.
- Refused archive schema setup for newer `user_version` files and unrelated `user_version = 0` SQLite databases without writing to or restamping them.

## 0.3.0 - 2026-08-29

Phase 1 of the tiered history ladder (spec `docs/superpowers/specs/2026-08-28-tiered-history-ladder-design.md`; ADRs 0013 and 0017). This release consolidates the per-lane versions **0.2.7** (T1 — schema v1 and the fail-closed, pre-imaged migration), **0.2.8** (T2 — count-weighted fold, frozen buckets, promote-before-prune; the rollup decimation defect is fixed going forward, already-decimated rows are not repaired), **0.2.9** (T3 — `retentionLadder` settings with legacy aliases and disk-pressure rules), **0.2.10** (T5 — dashboard ladder group, coverage card, long-range presets, shrink confirmation) and **0.2.11** (T4 — `source=auto` four-tier reads, coverage, filesystem/process detail APIs), plus the T6 CLI and documentation work listed below. Upgrading migrates the database on the first daemon start: a complete `<db>.pre-v0.sqlite` pre-image is taken before any row is touched (needs free space ≥ 1.2 × the database size; minutes on a large file) and is never deleted automatically — see INSTALL.md. Reviewed by six per-lane blind reviews and one deep dual-blind review over `v0.2.6..v0.3.0` (Fabulous `docs/fleet/tinytop/`).

- Coalesced history-coverage requests and throttled routine dashboard polling to one request per 15 seconds while forcing preset, confirmation-estimate, and post-save refreshes.
- Made retention-ladder capability fail closed until settings prove support, hiding Rust-only controls and stripping `retentionLadder` from unavailable-runtime saves.
- Corrected tier-disable confirmation copy to report retained buckets and read fallback instead of predicting deletion.
- Rendered an unmeasured history-disk check as unknown instead of inventing `0 B` free.
- Made Bash and PowerShell wrappers read the adjacent `VERSION`, with a current `0.2.11` fallback for copied standalone scripts and explicit version commands.
- Updated Phase 1 architecture, API, operator, migration/WAL, progress, guide, spec, and ADR documentation to match the landed four-tier implementation.
- Expanded `tinytop-agent db stats --json` with four-tier ladder coverage, the JSON-bearing raw-sample count, and archive/disk state while preserving the existing `StoreStats` field names.
- Added `tinytop-agent db pre-image status` and guarded `db pre-image remove --yes`. Removal refuses unless the exact canonical pre-image exists, the main database reports `user_version >= 1`, and `PRAGMA integrity_check` returns `ok`; refusal is structured JSON on stdout and never deletes a directory or glob.
- Added black-box temp-database CLI coverage for the stats shape, absent status, all removal refusal paths, successful exact-file removal, and post-removal database integrity, plus pure predicate tests for the non-confirmed, absent, pre-v1, and failed-integrity checks.
- Documented the 0.3.0 migration window and disk requirement, the four-tier architecture/read surface, the new CLI, and the Phase 1 T1–T6 close-out state.
- Fixed pre-image inspection so a missing main database is never created; status reports `databaseExists: false`, and removal refuses because the pre-image may be the only copy.
- Fixed pre-image status and removal through symlinked database paths by sharing the migration's canonical database-path resolution.
- Removed the duplicate raw-sample stats scan from `tinytop-agent db stats` by reusing the stats already carried by history coverage without changing its JSON shape.
- Deferred default database-path resolution for `db` and `serve` until after parsing, so an explicit `--sqlite` never creates the default state directory.
- Fixed same-timestamp detail replacement to remove filesystem and process members omitted by the replacement snapshot.
- Included the SQLite WAL sidecar in migration headroom and `bytesBefore`/`bytesAfter` audit accounting.
- Guarded frozen/partial minute merges against duplicate raw-row replays while documenting that the first post-prune replay remains indistinguishable from a late write.
- Prevented `db stats`, `db check`, and `db vacuum` from migrating existing databases; stats now returns a structured pre-v1 refusal while check and vacuum inspect any schema version in place.
- Added an authentic post-schema-commit migration crash seam and recovery test covering the pending audit, post-commit VACUUM, audit completion, and exactly one migration marker.
- Preserved completed maintenance counts in `MaintenanceError` when a later step fails, and included the partial report in the agent's error log.
- Replaced bare history-detail/points query extraction with field-aware parsing whose JSON rejections name the parameter, observed value, rule, and remedy.
- Removed directory creation from SQLite URL normalization and limited parent creation to commands that may create a database.
- Protected retained L3/L4 buckets from late-write replacement when their finer source tier has passed its retention horizon, merging one new sample instead.
- Replaying an already-counted timestamp older than the L2 horizon no longer merges it into a retained L3/L4 bucket again; only a genuinely new raw row takes the merge path.
- Counted both inclusive range endpoints during `source=auto` tier selection so a `k × resolution` range requires room for `k + 1` points.

## 0.2.11 - 2026-08-28

- Expanded the Rust history-points API to read L1 raw, L2 one-minute, L3 five-minute, and L4 hourly data. `source=auto` now selects the finest enabled tier that retains the requested start and fits the clamped page limit, reports `source` and `resolutionMs`, falls back to the coarsest retaining tier on overflow, and returns a truthful unavailable archive page until queryable archive reads land.
- Expanded history coverage with all four tiers, the snapshot-JSON horizon, detail cadence, disk state, archive configuration/state, and schema-migration state while preserving every existing coverage field.
- Added bounded Rust-only `/api/history/filesystems` and `/api/history/processes` reads over the typed detail tables, including exact mount filtering and complete process groups by capture timestamp. History query parameters accept the specified camelCase names while retaining the existing snake_case aliases.
- Added in-process Axum router tests using temp-directory SQLite stores for the complete 12-row `auto` selection table, coverage shape, filesystem filtering/limit clamping, grouped processes, and JSON-only raw history. Added exact test-only `tower = 0.5.3` and `http-body-util = 0.1.3` pins already present in the lockfile.
- Fixed direct `read_history_points` callers with `limit: None` to use the same 10,000-point effective limit for `auto` source selection and tier reads instead of truncating the selected tier to the legacy 120-row default.
- Added pure clamp and router regression coverage proving detail-history limits clamp to 1–10,000, default to 120, and accept `limit=99999` while returning all matching fixture rows or capture groups.
- Tightened history-coverage contract tests to assert the exact tier, disk, queryable-archive, and cold-archive key sets plus a null migration state on fresh databases.
- Clarified the history read-path documentation so points-store callers and raw-history callers have explicit, distinct omitted-limit behavior.

## 0.2.10 - 2026-08-28

- Added `90d`, `1y`, and `all` dashboard history presets; every preset from `6h` up now selects its tier automatically (`source=auto`, one complete page). Presets disable themselves with a setting-specific tooltip when no enabled tier holds their range (or, on the Bun runtime, beyond `1h`).
- Added the complete History ladder settings group, exact client-side mirrors of the Rust ladder validation messages, L4 forever mode, and read-only `retentionHours`/`rollupRetentionDays` compatibility mirrors derived from L1/L2.
- Added a pre-save shrink confirmation that lists approximate affected rows/buckets from current coverage until the server-computed Task 10 dry-run replaces it.
- Expanded History coverage with per-tier ranges/counts, disk-pressure status, and archive status, while remaining compatible with runtimes that omit newer coverage keys.
- Added the shared `ladder-rules.js` browser module to both the embedded Rust agent and Bun static-asset allow-list.
- Fixed Rust dashboard serving for the shared `ladder-rules.js` module and added unit plus served-asset contract coverage.
- Standardized dashboard disk-pressure handling on the coverage API's `disk.pressure` field, removing the stale `disk.active` compatibility hedge.
- Added non-persistent fallback to the nearest finer available preset when coverage makes the selected window unavailable.
- Expanded Bun's accepted default-history windows to all ten presets; Bun hides the Rust-only ladder/coverage UI, keeps legacy retention inputs editable, and omits `retentionLadder` from saves.
- Switched every preset from 6h up to one `source=auto&limit=10000` request and render from returned source/resolution metadata; previously `30d` silently showed only the newest 6.9 days.
- Corrected the GUIDE timeline walkthrough to list all ten presets and identify the Rust-only long-range boundary.
- Disabled 6h-and-longer presets when the Bun runtime lacks coverage/points routes, with a Rust-daemon tooltip and automatic fallback to a working raw preset.

## 0.2.9 - 2026-08-28

- Added the validated camelCase `retentionLadder` settings block with configurable L1/L2 horizons, L3/L4 toggles and monotonic retention, L4 forever mode, snapshot JSON retention, detail cadence, archive configuration, and disk-check thresholds.
- Single-sourced external settings decoding in `DashboardSettings::from_document`; legacy-only documents merge onto the persisted ladder, while ladder-authoritative saves overwrite the derived `retentionHours` and `rollupRetentionDays` mirrors.
- Single-sourced disk-pressure growth refusal in the ladder validator with the exact free/minimum-byte message, while preserving shrink operations.
- Fixed the one-tick disabled-tier race by saving `l3Enabled`/`l4Enabled` atomically with settings, before a subsequent insert can refold an ancestor. Settings now also drive typed-detail cadence immediately, and settings-change markers report one `retentionLadder` key rather than its derived aliases.

## 0.2.8 - 2026-08-28

- Added the Rust L1 raw → L2 one-minute → L3 five-minute → L4 hourly history ladder with sample-count-weighted folding, minimum/maximum preservation, nullable root utilization, legacy L2 bound fallback, bounded 50-bucket promotion passes, and persistent fold watermarks.
- Fixed the measured rollup decimation defect by freezing completed one-minute buckets: L1 pruning no longer rebuilds the cutoff minute from its surviving tail. The regression test fails against the old path when a 40-sample bucket collapses to 16 under the deterministic 90-second cutoff, then passes unchanged after the prune rebuild is removed.
- Fixed the review finding that the insert path could still rebuild a frozen minute from pruned raw rows. `late_write_into_a_pruned_minute_merges_instead_of_rebuilding` and `late_write_into_the_boundary_minute_merges_instead_of_rebuilding` move RED→GREEN from counts 1/17 to 41 by folding the existing bucket with the late sample whenever the raw minute is provably partial.
- Enforced promote-before-prune across enabled tiers. L2/L3 rows remain until the nearest enabled coarser watermark has passed them, a newly promoted watermark authorizes deletion only on the next maintenance tick, disabled tiers are neither written nor pruned, and L4 `0` retention means forever.
- Added late-write ancestor refolding, ongoing 500-row-bounded snapshot JSON stripping, 60-second typed filesystem/process detail sampling, per-tier coverage metadata, and the oldest JSON-bearing raw timestamp.

## 0.2.7 - 2026-08-28

- Added SQLite schema v1: nullable `metric_samples.snapshot_json`, minimum/root-maximum columns on one-minute rollups, five-minute and hourly rollup tables, migration state, and typed filesystem/process detail tables.
- Added fail-closed populated-v0 migration in the Rust store. Only this migration requires free space of at least 1.2× the database size; it creates a complete non-overwriting `<database>.pre-v0.sqlite` with `VACUUM INTO` before touching rows, rebuilds the schema in one transaction, retains JSON for the latest 60 minutes, runs the one automatic post-migration `VACUUM`, and records `schemaMigration` state plus a `schemaMigrated` marker. Fresh databases are created directly at v1 without the populated-v0 free-space check.
- Added reusable longest-mount-prefix free-space detection and JSON `history_state_get`/`history_state_set` store interfaces, with migration, refusal, headroom-boundary, and mount-selection tests using temp-directory databases only.
- Fixed Rust and Bun raw history reads to exclude rows whose retained `snapshot_json` is `NULL`, so `/api/history` and legacy `/history` expose the JSON keep-window horizon and `latestSnapshot` always returns a complete snapshot.
- Fixed Windows free-space lookup by canonicalizing both the database directory and each `sysinfo` mount point before component-aware longest-prefix matching; mounts that cannot be canonicalized are skipped.
- Expanded migration coverage to use the complete populated v0 schema, preserving a seeded one-minute rollup and app event while asserting all six additive rollup columns and every v1 index.
- Made post-schema migration completion crash-recoverable: the schema transaction records a pending `schemaMigration`, and later v1 connections idempotently finish the VACUUM, audit fields, and single migration marker when `vacuumedAtMs` is still `null`.
- Strengthened fail-closed pre-image refusal coverage to prove the existing pre-image's byte length and modification time remain unchanged and `user_version` stays 0.
- Improved undeterminable-free-space migration errors to name both the database byte count and required pre-image bytes.
- Corrected the architecture and release documentation for JSON-only raw reads, retryable VACUUM completion, the all-writers-stopped migration boundary, and the populated-v0-only 1.2× free-space rule.

## 0.2.6 - 2026-08-28

- Planning only, no runtime change: the **tiered history ladder** is designed and at Michel's gate.
  Spec `docs/superpowers/specs/2026-08-28-tiered-history-ladder-design.md`, plan
  `docs/plans/2026-08-28-tiered-history-ladder-plan.md` (11 hexe lanes across four phases),
  ADRs 0013 (ladder: raw → 1 min → 5 min → 1 h, fold-not-decimate, frozen completed buckets,
  promote-before-prune, `snapshot_json` kept for a recent window only, pre-imaged v0→v1
  migration), 0014 (queryable SQLite archive + cold verified `csv.gz`), 0015 (push-only OTLP
  metrics), 0016 (versioned settings export/import).
- Recorded defect (not yet fixed; fixed by plan Task 2): `prune_raw_history` rebuilds the boundary
  minute's rollup from its surviving tail every tick, so every 1-minute rollup older than
  `retentionHours` ends up with `sample_count` 1–2 — measured 4,274 of 4,289 buckets on the live
  database. Census: Fabulous `docs/reports/2026-08-28-tinytop-history-census.md`.

## 0.2.4 - 2026-07-05

- Deduplicated the dashboard: `agent/assets/dashboard/` is now the **single source** —
  embedded by the Rust agent at compile time (`include_bytes!`) and served from disk by
  the Bun server (`PUBLIC_DIR`). The `legacy/dashboard/` duplicate (previously kept
  byte-identical by a parity test) is removed; the test now asserts the duplicate stays
  gone instead of welding two copies together. No behavior change in either runtime —
  same files, one home. README/ARCHITECTURE/INSTALL/CLAUDE.md updated, including the
  rebuild-after-edit note (the Rust binary embeds the assets, so dashboard edits need a
  rebuild to reach the no-Bun runtime).

## 0.2.3 - 2026-07-05

- Fixed standalone dashboards behind a reverse-proxy sub-path (e.g. nginx `location /mon/`):
  `apiPath()` only derived a mount prefix for URLs ending in `/embed`, so a standalone
  dashboard at `/mon/` loaded its assets (base-relative since 0.2.2) but sent every API
  call to the domain root — shell rendered, all data 404'd. `dashboardBasePath(pathname)`
  now derives the prefix from the document location for **any** mount (`/` and `/embed` →
  ``, `/mon/` and `/mon/embed` → `/mon`, `/proxy/{id}/embed` → `/proxy/{id}`), applied
  identically in both dashboard copies.
- Added shipped-code unit tests: the tests extract `dashboardBasePath` from the actual
  `app.js` both runtimes serve and exercise 9 mount shapes, plus a guard that `apiPath`
  consumes it (no `/embed`-only derivation can silently return).
- Verified end-to-end in a browser behind a prefix-stripping subpath proxy at `/mon/`:
  `settings`/`version`/`snapshot`/`history` all resolve under `/mon/api/...` and return
  200; remaining 404s (favicon, `history/markers`, `history/coverage` on the legacy Bun
  runtime) reproduce identically root-mounted — pre-existing legacy-runtime gaps, not
  sub-path related.
- A standalone sub-path mount must be served **with a trailing slash** (nginx:
  `location /mon/ { ... }` plus a `/mon` → `/mon/` redirect) — same rule the relative
  asset URLs already require. First-class `--base-path` serving (no trailing-slash
  requirement) remains a backlog item (see PROGRESS, closed PR #1).

## 0.2.2 - 2026-07-04

- Made the dashboard's static asset references (`app.js`, `styles.css`, `vendor/echarts.min.js`, `favicon.svg`) **base-relative** instead of root-absolute, so `/embed` loads correctly when served behind a reverse-proxy sub-path (e.g. tutus-remotus embedding it at `/proxy/{id}/embed`). The standalone dashboard is unaffected — relative to `/` (or `/embed`) these resolve to `/app.js`, `/styles.css`, etc. exactly as before. API calls already resolved the sub-path via `apiPath()`; this closes the asset-loading gap so no root-absolute same-origin URLs remain in the embeddable view.
- Applied the change identically to both the legacy dashboard and the Rust-embedded dashboard copy, and added a `dashboard-assets` regression test that fails if any root-absolute same-origin asset ref is reintroduced.
- Documented the base-relative embed contract (and that a same-origin reverse-proxy embed needs no `TINYTOP_EMBED_FRAME_ANCESTORS` change) in `docs/INTEGRATION.md`.

## 0.2.1 - 2026-07-03

- Fixed a hang risk in the Bun collector: `runText` now enforces a 10s timeout and kills the child, so a stuck `df`/`ps`/`uname` (e.g. a stale mount) can no longer wedge a collection cycle (C1).
- Added rate-limited logging to the Bun collector: `readText`/`runText` failures are now logged at most once per 5 minutes per source, making permission errors and missing PSI distinguishable from idle metrics, while parsers still receive the empty-string fallback (M2).
- Fixed the Bun dashboard's writer proxy to time out each attempt with a 3s `AbortSignal.timeout`, so a stalled collector connection fails and retries instead of hanging every dashboard route (M3).
- Fixed a two-runtime contract drift: the Rust collector now populates per-filesystem inode fields via the `statvfs(2)` syscall (rustix) instead of leaving them permanently `null`, matching the Bun `df -i` output without shelling out (M1, ADR 0012).
- Fixed the Rust store to persist canonical `runtime_kind` values (`"WSL"`, `"macOS"`) that match the serde/JSON contract instead of Rust `Debug` spellings (`"Wsl"`, `"MacOs"`), added `RuntimeKind::as_str()` as the single source of truth, and added an idempotent migration that canonicalizes existing rows (M4).
- Added a `frame-ancestors 'self'` Content-Security-Policy to the top-level dashboard HTML routes (`/` and `/index.html`) in both runtimes, so the standalone dashboard cannot be framed by another origin; `/embed` keeps its configurable ancestors (D1).
- Hardened `/embed` frame-ancestors handling to fail closed: an invalid configured value now falls back to `'self'` instead of dropping the CSP header (Rust), rejected identically in both runtimes (D2).
- Added `rustix` (`=1.1.4`, `fs` feature, linux-collector only) as a vetted dependency for `statvfs(2)` inode collection.

## 0.2.0 - 2026-06-30

- Added `/embed`, an iframe-friendly dashboard view for host panels such as tutus-remotus.
- Added `?theme=dark` and `?theme=light` handling for the embedded dashboard view.
- Added `TINYTOP_EMBED_FRAME_ANCESTORS` to configure `/embed` frame permissions while leaving the standalone dashboard unchanged.
- Added `capabilities` to version/health metadata so integrators can detect `snapshot`, `history`, and `embed` support.
- Added `docs/INTEGRATION.md` with the stable TinyTop integration contract for `/api/version`, `/health`, `/api/snapshot`, and `/api/history/points`.
- Bumped product, command-center, PowerShell, and Rust crate versions to 0.2.0.

## 0.1.35 - 2026-06-29

- Fixed native Windows direct `tinytop-agent.exe serve` startup when `HOME` is not set by resolving the default SQLite database to `%LOCALAPPDATA%\TinyTop\state\history.sqlite`, with a `USERPROFILE\AppData\Local` fallback.
- Changed the native Windows dashboard default port to `127.0.0.1:4275` so it can run beside a WSL/Linux TinyTop daemon on `127.0.0.1:4274`.
- Fixed `tinytop.ps1 service install` under `Set-StrictMode` by preserving service subcommands as an array when exactly one rest argument is present.
- Added `tinytop.cmd` as a policy-safe Windows wrapper around `tinytop.ps1`; docs now recommend `Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass` for direct `.ps1` calls when scripts are disabled.
- Added Windows loopback-neighbor detection that warns when another TinyTop daemon is visible on the WSL/Linux default port.
- Added daemon OS, architecture, executable path, working directory, bind host/port, and SQLite URL/path metadata to Rust `/health`, Rust `/api/version`, and legacy Bun metadata surfaces.
- Added a dashboard runtime-origin notice so users can see when the browser is connected to native Windows versus WSL/Linux, including the reported SQLite location.
- Bumped product, command-center, PowerShell, and Rust crate versions to 0.1.35.

## 0.1.34 - 2026-06-27

- Added an on-demand GitHub Actions workflow for building TinyTop release binaries.
- The manual workflow can build Linux x86_64, Windows x86_64, macOS x86_64, macOS aarch64, or all supported release binaries in one run.
- Each build uploads the binary and `.sha256` checksum as workflow artifacts.
- The workflow can optionally attach built assets to an existing GitHub release tag with `gh release upload --clobber`.
- Added regression coverage for the workflow contract and documented the release-build process.

## 0.1.33 - 2026-06-27

- Bumped product, command-center, PowerShell, and Rust crate versions to 0.1.33.
- Added a shared PowerShell elevation/confirmation guard for mutating Windows service commands.
- `.\tinytop.ps1 service install|start|stop|restart|uninstall` now checks for elevated PowerShell before touching Windows Service Control Manager.
- Interactive non-elevated service mutations now warn and require explicit confirmation; non-interactive non-elevated service mutations fail with Administrator guidance.
- Refreshed Windows installation docs for the service elevation behavior.

## 0.1.32 - 2026-06-27

- Replaced the README dashboard screenshot with a fresh live capture from the connected Rust collector/dashboard daemon.
- The new screenshot shows real host, CPU, RAM, swap, load, history, health, and `Live` connection values instead of an empty or pre-hydration view.
- Bumped product and Rust crate versions to 0.1.32 for the screenshot documentation checkpoint.

## 0.1.31 - 2026-06-27

- Bumped product and Rust crate versions to 0.1.31.
- Fixed the Settings dialog effective-settings readout so compact chips no longer stretch into oversized ovals beside the taller daemon settings column.
- Changed daemon boolean settings from tall single-column checkboxes to compact responsive toggle controls while keeping the underlying checkbox semantics and IDs intact.
- Added a fresh rendered dashboard screenshot to the README.
- Rebuilt the embedded Rust collector/dashboard agent so the packaged dashboard includes the Settings layout fixes.
- Added a release verification report for the Settings layout, screenshot, and v0.1.31 closeout.

## 0.1.30 - 2026-06-26

- Bumped product version to 0.1.30.
- Re-verified embedded Rust dashboard runtime behavior and ensured current release files and crate metadata are aligned with the new patch version.

## 0.1.29 - 2026-06-26

- Added `tinytop.ps1` as a native Windows PowerShell command center for the Rust collector/dashboard daemon.
- Added Windows release-binary install, local Rust build, start, stop, restart, status, logs, and Windows service commands to the PowerShell path.
- Made Windows builds select `--no-default-features --features windows-collector`, and made the Bash command center print target-specific Rust build commands.
- Strengthened the dashboard operator strip so Critical, Warning, and Stale states are visually obvious through full-strip styling and a state pill, not only a subtle border.
- Cleaned the sidebar runtime identity so long WSL detection explanations are shown as compact runtime context instead of oversized brand text.
- Added Windows guide, verification report, and ADR 0011 for the PowerShell-first Windows packaging decision.

## 0.1.28 - 2026-06-26

- Added a TinyTop SVG favicon to both the legacy Bun dashboard asset tree and the Rust embedded dashboard asset tree.
- Replaced the blank favicon link with `/favicon.svg` and served it from the Rust collector/dashboard daemon with `image/svg+xml`.
- Expanded dashboard asset parity and Rust embedded serving regression coverage for the favicon.

## 0.1.27 - 2026-06-26

- Added an operator alert detail drawer explaining current state by metric, value, threshold, age, trend, and recent change.
- Added rollup-backed History ranges for 6h, 24h, 7d, and 30d through additive `/api/history/points`, while keeping `/api/history` raw-snapshot compatible.
- Added timeline markers through `/api/history/markers` for daemon starts, settings changes, and computed coverage gaps.
- Added SQLite-backed DB budget settings and coverage fields: `targetDatabaseBytes`, budget percentage, and rollup coverage timestamps.
- Polished Settings with validation, dirty-close warning, reset/defaults buttons, threshold presets, and an effective-settings readout.
- Upgraded process details with redacted copy-safe command text, parent PID/start time when available, RSS, and per-PID CPU/RAM trend.
- Started feature-gated native Rust collector modules for macOS and Windows while keeping Linux as the default reference collector.
- Added ADRs for the additive history points/markers API and feature-gated native platform collectors.
- Cleaned the stale handoff PID note.

## 0.1.26 - 2026-06-26

- Fixed native select dropdown contrast in the Settings dialog and process density control by assigning explicit readable option foreground/background colors for every dashboard theme.
- Added regression coverage for themed native dropdown option colors.
- Kept Rust embedded dashboard assets and legacy Bun dashboard assets byte-identical.

## 0.1.25 - 2026-06-26

- Added an operator status strip with Healthy, Warning, Critical, and Stale states computed from saved daemon thresholds.
- Replaced the native History range input with a canvas timeline rail, selected timestamp marker, visible-window shading, visible-series preferences, and history coverage display.
- Added Rust `/api/history/coverage`, raw-history pruning by `retentionHours`, and one-minute rollups pruned by `rollupRetentionDays`.
- Expanded settings thresholds to CPU/RAM/disk/load/pressure warning and critical values, and applied enabled-section settings to the dashboard layout.
- Added process search/sort/density controls, a process detail dialog, a root filesystem card, a system-mount toggle, and threshold-colored filesystem/pressure states.
- Kept Rust embedded dashboard assets and legacy Bun dashboard assets byte-identical.

## 0.1.24 - 2026-06-26

- Added a Load overview gauge next to CPU, RAM, and swap.
- Normalized the Load gauge from 1-minute load divided by CPU core count, matching the existing History chart load percentage.
- Added a Load sparkline to the overview row while keeping the raw 1m/5m/15m load tile for detail context.
- Kept Rust embedded dashboard assets and legacy Bun dashboard assets byte-identical.

## 0.1.23 - 2026-06-26

- Moved dashboard Settings out of the main metrics flow into an accessible modal dialog opened from the rail.
- Changed the rail Settings item from an anchor to a button so it opens the dialog instead of scrolling the dashboard.
- Kept the existing `This Browser` and `This Daemon` settings split, backed by localStorage and `/api/settings`.
- Kept Rust embedded dashboard assets and legacy Bun dashboard assets byte-identical.

## 0.1.22 - 2026-06-26

- Added `/api/version` to the Rust collector/dashboard daemon and legacy Bun dashboard, plus `/version` to collector-compatible APIs.
- Added SQLite-backed daemon dashboard defaults with `GET /api/settings` and `PUT /api/settings`.
- Added a Settings panel with separate `This Browser` local preferences and `This Daemon` daemon defaults.
- Added typed settings validation for theme, graph mode, history window, refresh interval, retention defaults, thresholds, and enabled sections.
- Added a dashboard sidebar version line so users can see whether Rust or legacy Bun is serving the page.
- Changed `./tinytop start` to auto-select the Rust collector/dashboard daemon when available, with `TINYTOP_RUNTIME=legacy` or `TINYTOP_RUNTIME=bun` as explicit legacy overrides.
- Updated `./tinytop status` to report the running daemon runtime, component, product version, and dashboard asset mode from `/api/version`.
- Added foreground `./tinytop stop`/`restart` awareness for Rust and legacy Bun processes when systemd units are not installed.
- Aligned Rust crate package versions with the product checkpoint version.

## 0.1.21 - 2026-06-26

- Saved the dashboard timeline/settings implementation plan under `docs/superpowers/plans/`.
- Added History range presets for Live, 15m, 1h, 6h, and 24h.
- Replaced index-based timeline state with timestamp-based selection.
- Changed dashboard history hydration to use explicit `since_ms` and `until_ms` windows, with client-side pagination for larger ranges.
- Persisted the selected history range in browser-local storage as `tinytop.historyWindow`.
- Added dashboard timeline regression coverage and refreshed docs for the new timeline behavior and settings roadmap.

## 0.1.20 - 2026-06-26

- Split verification scripts into runtime-specific `check:bun` and `check:rust` commands while keeping `bun run check` as the full maintainer suite.
- Updated the setup wizard to run only the selected collector's verification path: Rust choices avoid Bun tests, and legacy Bun choices avoid Rust tests.
- Made Rust release-binary systemd setup install the release binary before running the Rust smoke check.
- Added regression coverage for Rust release, Rust compile, and legacy Bun setup verification command selection.
- Updated docs and handoff notes for runtime-specific setup verification.

## 0.1.19 - 2026-06-26

- Clarified current history retention behavior across the README, user guide, install guide, API guide, operations guide, architecture docs, progress notes, and handoff.
- Documented that SQLite raw samples are retained until manual archive/reset because automatic retention is not implemented yet.
- Documented that `/api/history` windows and the dashboard's 120-sample rolling buffer are read/rendering limits, not database retention limits.
- Added a documentation report for the history-retention wording sweep.

## 0.1.18 - 2026-06-25

- Refreshed the current documentation and guides after the embedded Rust collector/dashboard asset move.
- Updated user-facing port, process, API, and operations wording to describe the Rust collector/dashboard daemon and the legacy Bun dashboard/collector fallback.
- Updated dependency and UI verification reports so current commands reference `agent/assets/dashboard/` and `legacy/dashboard/` instead of the removed root `public/` tree.
- Marked ADR 0001 as superseded in the ADR index while preserving the historical ADR file unchanged.

## 0.1.17 - 2026-06-25

- Moved the static dashboard assets from root `public/` into `legacy/dashboard/` for the legacy Bun runtime.
- Added a byte-identical Rust dashboard asset tree under `agent/assets/dashboard/`.
- Embedded the dashboard HTML, CSS, browser JavaScript, and ECharts bundle into `tinytop-agent serve`.
- Kept `--public-dir` and `TINYTOP_PUBLIC_DIR` as explicit development overrides while making embedded assets the default Rust path.
- Updated the Bun development server, command center, tests, docs, and handoff for embedded Rust dashboard ownership.
- Added regression coverage for embedded Rust serving without a dashboard directory and for legacy/Rust dashboard asset equality.
- Added ADR 0006 for embedded Rust dashboard assets and legacy dashboard asset ownership.

## 0.1.16 - 2026-06-25

- Moved the legacy Bun collector daemon from `src/collector-daemon.ts` to `legacy/bun-collector.ts`.
- Added `bun run collector` and `bun run collector:check`, keeping writer script aliases for compatibility.
- Updated the setup wizard to ask for `rust` or `bun` collector runtime; Rust means the single collector/dashboard daemon, while Bun means the legacy split collector/dashboard path.
- Renamed new legacy Bun systemd rendering/install output to `tinytop-collector.service`, while keeping cleanup and service actions aware of the older `tinytop-writer.service` name.
- Updated command-center, wizard, architecture, install, API, operations, and README wording from writer-first language to collector-first language.
- Added regression tests for the legacy collector path, setup wizard collector selection, and systemd unit rendering.

## 0.1.15 - 2026-06-25

- Added `HANDOFF.md` as the current TinyTop restart point.
- Recorded the live Rust daemon state, Rust collector confirmation, recent verification evidence, and next useful work.
- Bumped the docs-only checkpoint version so the handoff can be committed, tagged, and pulled cleanly.

## 0.1.14 - 2026-06-25

- Replaced the alert-named inline fetch-error surface with `status-message` naming.
- Added a reusable accessible in-app confirmation dialog for browser UI actions.
- Added a confirmed `Clear` action for the browser-local Live History session buffer without deleting SQLite history or changing system data.
- Added regression coverage that scans the public web UI for browser-native `alert`, `confirm`, and `prompt` calls.
- Documented the no-native-dialog web UI policy and verification evidence.

## 0.1.13 - 2026-06-25

- Added `tinytop-agent serve`, a Rust daemon that serves the dashboard, owns SQLite, collects on an interval, and exposes both public `/api/*` and legacy collector-compatible routes.
- Updated systemd defaults to install a single Rust `tinytop.service`; kept the legacy Bun split services behind `./tinytop systemd install --bun`.
- Added `./tinytop rust` commands for release-binary install, local build, collect, serve, serve-writer, test, and check.
- Updated the setup wizard to ask whether the Rust collector binary should come from a GitHub release binary or a local Cargo compile.
- Added Rust-backed DB `stats`, `check`, and `vacuum` paths so the command center can manage SQLite without Bun when a Rust binary or Cargo is available.
- Vendored the Apache ECharts browser bundle with upstream license and notice files so the Rust daemon can run without `node_modules`.
- Added Axum-based daemon tests, Rust history JSON contract tests, SQLite file-creation regression coverage, and Bash command-center tests for the Rust systemd path.
- Documented the Rust single-daemon runtime, Axum dependency decision, vendored asset provenance, and no-Bun install path.

## 0.1.12 - 2026-06-24

- Added an additive Rust workspace under `agent/` without removing or replacing the existing Bun collector.
- Added shared Rust snapshot types that serialize to the current dashboard JSON contract.
- Added a Rust Linux/WSL collector with parser, fixture, live-host, and no-shell-command tests.
- Added a SQLx-backed SQLite history store proof point for the Rust collector path.
- Added `tinytop-agent collect --json` and optional `--sqlite` collect-and-store mode.
- Documented the Rust collector preview, SQLx decision, dependency vetting, crate-backed host collection, and Rust `1.95.0` requirement.

## 0.1.11 - 2026-06-24

- Changed the project license from MIT to Apache License 2.0.
- Added package license metadata and a NOTICE file for Apache-2.0 attribution.
- Prepared the repository for a private GitHub release before public conversion.

## 0.1.10 - 2026-06-24

- Added a README hero image and inline new-user install guide.
- Removed public-doc references to local home paths, host names, and personalized implementation notes.
- Removed the old generated UI concept image that contained host-like demo strings.

## 0.1.9 - 2026-06-24

- Implemented the root `./tinytop` Bash command center with help, Bun install guidance, doctor/status, dependency install, verification, foreground start, split start, logs, monitor, and restart/stop wrappers.
- Added `bun run setup` as a real Bun setup wizard launched by `./tinytop setup`, with noninteractive automation flags and systemd mode.
- Added user-space systemd rendering and management for `tinytop-writer.service` and `tinytop-dashboard.service`.
- Added SQLite operations for stats, integrity check, backup, vacuum, and guarded reset.
- Added tests for the Bash command center, setup wizard, systemd unit rendering, and SQLite operations.

## 0.1.8 - 2026-06-24

- Recorded the approved Telecode-style install wizard design for TinyTop.
- Chose a two-layer installer: a zero-dependency `./tinytop` Bash command center that can bootstrap Bun, then a richer `bun run setup` wizard once Bun exists.
- Added ADR 0003 for the Bash bootstrap plus Bun wizard architecture.
- Documented the planned command surface for setup, start, restart, stop, status, logs, monitor, stats, SQLite maintenance, backups, and systemd user services.

## 0.1.7 - 2026-06-24

- Renamed the project to TinyTop, including package name, app title, default SQLite data directory, browser storage keys, documentation, and local port claim.
- Rewrote the root `README.md`, `INSTALL.md`, `GUIDE.md`, `ARCHITECTURE.md`, `PROGRESS.md`, and `CHANGELOG.md` documentation set.
- Added operations and API guides under `docs/guides/`.
- Documented ports, environment variables, SQLite location, runtime modes, verification commands, troubleshooting, and current persistence limitations.

## 0.1.6 - 2026-06-24

- Implemented SQLite-backed recent history through a dedicated Bun collector/writer process on `127.0.0.1:4276`.
- Added `/api/history` hydration so refreshing the dashboard refills the Live History chart instead of starting from scratch.
- Made frontend history insertion timestamp-aware so repeated latest samples update in place rather than duplicating bars.
- Added tests for persistent history storage and the dashboard history API.

## 0.1.5 - 2026-06-24

- Made stacked bar history use a viewport-derived visible sample count so bars keep a minimum width and the live window rolls left.
- Added a SQLite history architecture plan and ADR for a dedicated collector/writer process and dashboard read path.
- Kept dashboard display settings as browser-local preferences.

## 0.1.4 - 2026-06-24

- Replaced the hand-rolled Live History canvas chart with Apache ECharts served from the local dependency tree.
- Added ECharts-backed stacked area, stacked bar, heatmap, and treemap graph modes.
- Added a local `/vendor/echarts.min.js` route and coverage for serving that bundle.
- Kept visible-window sample counts, chart sample selection, and compact selected-sample metric chips.

## 0.1.3 - 2026-06-24

- Restored the Live History bar graph mode.
- Moved graph-type controls into the Live History top nav.
- Moved the timeline into its own row under the chart with selected datetime context.
- Added selected-sample metric values and percent-axis labels so bar, line, and area modes have numeric context.
- Added latest-value labels to heatmap lanes so the view has numeric context.
- Kept area mode as a filled-under-line chart for the independent CPU, RAM, swap, and load series.

## 0.1.2 - 2026-06-24

- Moved Live History directly below the CPU, RAM, and swap gauges.
- Removed the duplicate bar history mode.
- Added a timeline scrubber that lets the main gauges inspect older local samples.
- Added a Live control that returns the gauges to the newest sample.

## 0.1.1 - 2026-06-24

- Added five selectable dashboard themes: Midnight, Matrix, Aurora, Solar, and Ember.
- Added four live history graph modes: line, area, bars, and heatmap.
- Persisted theme and graph preferences in browser-local storage.
- Updated chart rendering so theme changes recolor canvas graphs immediately.

## 0.1.0 - 2026-06-24

- Added the initial standalone Bun dashboard project.
- Claimed local port `127.0.0.1:4274`.
- Added read-only live collectors for `/proc`, `df`, `ps`, `uname`, and OS release data.
- Added automatic WSL versus real Linux runtime detection.
- Added dark operations dashboard UI with gauges, stat tiles, charts, filesystem bars, pressure meters, and process rows.
- Added Bun unit tests and rendered Playwright QA coverage.
