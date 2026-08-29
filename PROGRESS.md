# Progress

## Current Version

- Version: `0.5.2`
- Date: 2026-08-29
- Status: Phase 5 (cadence classes + GPU, plan `docs/plans/2026-08-29-cadence-classes-and-gpu-plan.md`,
  ADRs 0021–0023) IN PROGRESS. T13 landed as 0.5.2: schema v2 — the `process_commands`
  dictionary, the per-tick `process_samples_fast` table, `command_id` on the minute table,
  `processFastKeepHours`, `/api/history/processes` `source` fast|minute, `db stats --json`
  `userVersion`; v1→v2 in one transaction (ADR 0023; 199 ms on a copy of the live file);
  review rounds luna 655 → fix 657. Next = T14 (schema v3: filesystems stored on change, the
  identity table, `/api/history` assembled from typed tables, `snapshot_json` dropped; brief
  `docs/plans/2026-08-28-tiered-history-ladder/briefs/T14.md`), then T15–T17 → 0.6.0. The live
  daemon still runs 0.3.1 — redeploy is an explicitly ordered step.

## Backlog

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
