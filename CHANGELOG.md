# Changelog

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
