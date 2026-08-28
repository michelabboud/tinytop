# Tiered History Ladder — Implementation Plan

> **For agentic workers:** this plan is executed **one task per hexe lane** (Ari), dispatched and reviewed by Fable (planner/reviewer — Fable writes no code). Each lane receives a brief (§Lane protocol) naming exactly one task below. Steps use checkbox (`- [ ]`) syntax. Read the spec first: `docs/superpowers/specs/2026-08-28-tiered-history-ladder-design.md` — the plan argues from it and never repeats its rationale.

**Goal:** Replace the 72-hour/1-minute retention with a four-tier ladder (raw → 1 min → 5 min → 1 h) that folds instead of decimates, never deletes before promoting, keeps `snapshot_json` only for a recent window, archives expired hourly rows (queryable SQLite + cold gzip CSV), checks disk space, exports/imports its configuration, and can push metrics to OpenTelemetry.

**Architecture:** All maintenance stays in the Rust daemon (`tinytop-store` + `tinytop-agent`), as today. New store modules (`ladder.rs`, `maintenance.rs`, `migration.rs`, `archive.rs`, `retention_ladder.rs`) beside the existing `lib.rs`; additive API endpoints in `writer.rs` per the ADR 0009 pattern; single-source dashboard assets. One `fold()` function serves every tier and the archive; fold watermarks and disk state live in a new `history_state` table.

**Tech Stack:** Rust 2024 (`rust-version 1.95`), sqlx `=0.9.0` (sqlite, runtime-tokio), tokio `=1.52.3`, axum `=0.8.9`, serde/serde_json (pinned in `agent/Cargo.toml`), Bun tests for the dashboard. New crates only where a task names them: `flate2 1.1.x` (T8), `opentelemetry`/`opentelemetry_sdk`/`opentelemetry-otlp` 0.32.x (T11) — each vetted by its lane per global rule 5 with a report in `docs/reports/`.

**Spec:** `docs/superpowers/specs/2026-08-28-tiered-history-ladder-design.md` · **ADRs:** 0013, 0014, 0015, 0016 (Proposed → Accepted on Michel's go).

## Global Constraints

- Workspace: `agent/Cargo.toml` — `edition = "2024"`, `rust-version = "1.95"`, all dependency versions pinned `=x.y.z`. New crates are added to `[workspace.dependencies]` pinned exactly, then referenced with `.workspace = true`.
- Gates (from `CLAUDE.md`): `bun run check:rust` (= `cargo fmt --check` + `cargo test --workspace` under `agent/`); `bun run check:bun` whenever `agent/assets/dashboard/**` or `src/**` change; `bun run check` = both. A task is not done until its gate output is pasted in the lane report.
- **Two-runtime invariant:** `agent/assets/dashboard/` is the single dashboard source; the Bun runtime (`src/`) must keep working with **no change** except where a task names a file under `src/`. Legacy settings keys `retentionHours` / `rollupRetentionDays` must remain present in every saved settings document (spec §5).
- **The live database `~/.local/share/tinytop/history.sqlite` and `~/.local/share/tinytop/` are never opened, read, or written by any test or lane.** Tests use `std::env::temp_dir()` fixtures exactly like `agent/crates/tinytop-store/tests/sqlite_history_store.rs:12-30`.
- Formatting: never run bare `cargo fmt` (it reformats the workspace and pollutes the diff). Run `cargo fmt --check --manifest-path agent/Cargo.toml`; if it flags a file you changed, run `rustfmt --edition 2024 <that file>` only.
- Git in a lane: **read-only.** No commits, no pushes, no branch changes, no `git add`. Leave the worktree dirty; report `git diff --stat`. Fable commits after review.
- Errors: no swallowed errors, no `unwrap()` on I/O or SQL in non-test code; every refusal names field, rule, observed value, remedy (spec §15). Logging via the existing `eprintln!`/`tracing` pattern in `writer.rs` (match whichever the touched function uses).
- Docs: each task lists the docs it must update. `CHANGELOG.md` gets an entry per task under the next version heading; `VERSION` is bumped by Fable at merge, not by the lane.
- Naming: settings keys camelCase in JSON, snake_case in Rust; tables/columns snake_case; tiers are `l1..l4` in JSON and `Tier::L1..L4` in Rust.

---

## Context & Rationale (read before any step)

**Why the ladder exists** — measured facts, not opinions: the live DB is 4.09 GB as a 72-hour window because 96 % of each 1.5 s row is a 20 KB `snapshot_json`; the typed columns cost 139 B/row. `prune_raw_history` (`agent/crates/tinytop-store/src/lib.rs:676-690`) calls `rebuild_rollup_bucket(bucket_start_ms(cutoff_ms))` after deleting, which recomputes the boundary minute from its surviving tail every 1.5 s, so every 1-minute rollup older than 72 h holds 1–2 samples (4,274 of 4,289 measured). The fix is a rule, not a patch: **prune never rebuilds; only inserts rebuild; completed buckets are frozen.**

**Decisions already made — do not reopen at execution time:**
- Resolutions (1 min / 5 min / 1 h) are structural; only horizons and toggles are settings.
- "id + one JSON row" was measured 8.8× larger than typed columns and rejected. Do not "simplify" the schema back into JSON.
- Compressed CSV beat compressed SQLite and JSONL in measurement; the cold format is CSV.gz + `.sha256`. gzip via `flate2` (pure-Rust backend), not zstd.
- `snapshot_json` stays for the recent window only (`snapshotJsonKeepMinutes`, floor 60). The `/api/history` raw endpoint therefore has that horizon; the `1h` preset must keep working.
- Legacy decimated 1-minute rows are **not** repaired or deleted; `sample_count` tells the truth.
- OTel is push-only, HTTP/protobuf, off by default, headers from an env var. No read path, no Prometheus endpoint.
- The migration takes a `VACUUM INTO` pre-image and **fails closed**; the pre-image is never auto-deleted.

**Codebase invariants that look wrong but are intentional:**
- `metric_samples.captured_at_ms` is `UNIQUE` and the Bun runtime upserts on it (`src/history-store.ts:104-150`) — keep that column shape exactly.
- `history_points` resolves `Auto` before dispatch (`lib.rs:731-745`); `HistoryPointMode::Auto` reaching `read_*` is `unreachable!` by design — extend the resolver, not the readers' match.
- `HistoryMarkerType` (`lib.rs:307-320`) stores camelCase strings (`daemonStart`, `settingsChange`, `coverageGap`); new markers follow that exact storage form.
- `DashboardSettings` uses `#[serde(default = "…")]` per new field so old `app_settings` documents still deserialize (`lib.rs:38-39` is the precedent). Every new field gets a default fn.
- The dashboard's settings dialog validates client-side with `validateRange` (`agent/assets/dashboard/app.js:2326-2347`) mirroring server ranges; keep the mirror exact, message for message.
- `rebuild_rollup_bucket` currently deletes a bucket when it finds zero samples (`lib.rs:918-928`). After T2 that branch only runs on the insert path (which always has ≥ 1 sample), so it becomes unreachable in practice — keep it, it is harmless and defensive.

**User constraints:** Michel pays for Ari tokens; a lane that stalls escalates (§Lane protocol) rather than grinding. No dependency beyond those named. No refactors outside the named files.

---

## Lane protocol (Fable runs this; every lane's brief embeds the rules)

**Dispatch** (one task per lane; Rust/store/API/OTel on `ari-sol-deep`; dashboard JS on `ari-sol`; docs-only on `ari-spark`):
```bash
cd ~/projects/tinytop
mkdir -p docs/plans/2026-08-28-tiered-history-ladder/briefs
# Fable writes briefs/T<n>.md from the template below, then:
nohup setsid hexe run --plan docs/plans/2026-08-28-tiered-history-ladder/briefs/T<n>.md \
  --profile <ari-sol-deep|ari-sol|ari-spark> --key tinytop-ladder-t<n> \
  --project tinytop --task "T<n> <slug>" \
  --worktree --worktree-branch tinytop/ladder-t<n>-<slug> --worktree-base main --keep-worktree \
  > ~/projects/Fabulous/docs/fleet/tinytop/lane-t<n>.log 2>&1 &
```
Task order and parallelism: T1 → T2 → T3 → (T4 ∥ T5) → T6 → **Phase 1 close** → T7 → T8 → T9 → **Phase 2 close** → T10 → **Phase 3 close** → T11 → **Phase 4 close**. A task's `--worktree-base` is `main` *after* its predecessors merged.

**Brief template** (`briefs/T<n>.md`):
```
You are hexe lane T<n> for tinytop. Execute ONLY "Task <n>: <title>" from
docs/plans/2026-08-28-tiered-history-ladder-plan.md. Read, in this order:
docs/superpowers/specs/2026-08-28-tiered-history-ladder-design.md (whole),
the plan's "Global Constraints", "Context & Rationale", then Task <n>.
Rules: worktree only; git is READ-ONLY (no commit/add/push); never bare `cargo fmt`;
never open ~/.local/share/tinytop/; no new dependencies except those the task names;
no edits outside the task's Files list. Run the task's gate and paste its full output.
Report: files changed with line ranges, tests added (names), gate output, `git diff --stat`,
anything the plan was silent on (do NOT improvise around it).
You are on the lowest model expected to handle this. If the task exceeds you — stuck after a
real attempt, looping, or about to guess — stop and reply `ESCALATE: <what is beyond you, what
you tried>`. Escalating early is cheaper than a wrong answer.
```

**Review & merge (Fable):** per lane, `/ari-dual-review` fast (luna) over the worktree diff; Fable validates every finding against the code; fixes go back to the same lane (`hexe run --session resume --key tinytop-ladder-t<n>`) or a fix-lane; then `git merge --no-ff` into `main`, `CHANGELOG.md` entry confirmed, `VERSION` patch bump, commit, tag, push. Per phase: one deep dual-blind (`ari-sol-deep` + `ari-luna`) over the phase range; then merge → docs → tag → `gh release` → `cargo audit` (under `agent/`) + `bun audit`.

---

## File structure (locked in)

| file | responsibility |
|---|---|
| `agent/crates/tinytop-store/src/lib.rs` | existing store; gains `mod` declarations, `StoreError` variants, `history_state` get/set, calls into the new modules; **shrinks** by moving nothing else (no refactor) |
| `agent/crates/tinytop-store/src/retention_ladder.rs` (new, T3) | `RetentionLadder`, `TierSetting`, `ArchiveSettings`, `DiskCheckSettings`, defaults, `validate()`, legacy alias derivation |
| `agent/crates/tinytop-store/src/ladder.rs` (new, T2) | `Tier` enum, `TierBucket`, `Stat`, `fold`, `raw_to_bucket`, bucket math (`bucket_start_ms_for(tier, ts)`, `is_complete`) |
| `agent/crates/tinytop-store/src/maintenance.rs` (new, T2) | `maintain(&store, &settings, now_ms)`: promote, strip JSON, prune with watermarks; detail-row writes |
| `agent/crates/tinytop-store/src/migration.rs` (new, T1) | `user_version` handling, v0→v1 with pre-image, schema creation for v1 tables |
| `agent/crates/tinytop-store/src/archive.rs` (new, T7/T8) | archive DB open/attach, move batches, manifest, cold CSV.gz export + verify |
| `agent/crates/tinytop-store/src/disk.rs` (new, T9) | free-bytes provider trait + disk pressure state machine |
| `agent/crates/tinytop-agent/src/writer.rs` | routes, settings update, collection loop, `history_points_query`, disk/cold/OTel scheduling |
| `agent/crates/tinytop-agent/src/otel.rs` (new, T11) | OTLP exporter task |
| `agent/crates/tinytop-agent/src/main.rs` | CLI: `db stats`, `db pre-image`, `db archive`, `config export/import` |
| `agent/assets/dashboard/app.js`, `index.html`, `styles.css` | presets, ladder settings group, confirm dialog, coverage card, export/import |
| tests: `agent/crates/tinytop-store/tests/ladder_fold.rs`, `tests/ladder_maintenance.rs`, `tests/migration_v1.rs`, `tests/archive.rs`, `tests/disk_check.rs`, `tests/retention_settings.rs`; `agent/crates/tinytop-agent/tests/history_api.rs`, `tests/settings_transfer.rs`, `tests/otel_export.rs` |
| docs: `docs/sqlite-history-architecture.md`, `README.md`, `ARCHITECTURE.md`, `GUIDE.md`, `CHANGELOG.md`, `PROGRESS.md`, `docs/reports/…` (dependency vetting) |

---

# Phase 1 — the ladder (→ 0.3.0)

### Task 1: Schema v1 + pre-imaged migration

**Files:** Create `agent/crates/tinytop-store/src/migration.rs`, `agent/crates/tinytop-store/src/disk.rs` (`pub fn free_bytes_at(path: &Path) -> io::Result<u64>` via `sysinfo::Disks`, longest-mount-prefix match — **ADR 0017**); Modify `agent/crates/tinytop-store/src/lib.rs` (`connect` at :348, schema block :1007-1130, `StoreError` enum, add `pub mod migration;` and `pub mod disk;`), `agent/crates/tinytop-store/Cargo.toml` (**exactly one added line**: `sysinfo.workspace = true` — already pinned and compiled in the workspace, so not a new dependency); Test `agent/crates/tinytop-store/tests/migration_v1.rs` (+ unit tests inside `disk.rs`); Docs `docs/sqlite-history-architecture.md` (§Current Schema, §Retention), `CHANGELOG.md`.

**Interfaces — Produces:**
```rust
// migration.rs
pub const SCHEMA_VERSION: i64 = 1;
pub struct MigrationReport { pub from: i64, pub to: i64, pub pre_image_path: Option<PathBuf>, pub samples_kept: i64, pub json_rows_kept: i64, pub duration_ms: i64, pub bytes_before: i64, pub bytes_after: i64 }
pub(crate) async fn ensure_schema(pool: &SqlitePool, db_path: &Path, now_ms: i64, snapshot_json_keep_ms: i64) -> Result<Option<MigrationReport>, StoreError>;
// lib.rs
pub enum StoreError { /* existing… */ Migration { reason: String, remedy: String }, Validation(String) }
pub async fn history_state_get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StoreError>;
pub async fn history_state_set<T: Serialize>(&self, key: &str, value: &T, now_ms: i64) -> Result<(), StoreError>;
```

- [ ] **Step 1: failing tests** — in `tests/migration_v1.rs`, using the temp-dir fixture pattern from `tests/sqlite_history_store.rs:12-30`:
  - `fresh_database_is_created_at_schema_version_1`: connect to a new path; `PRAGMA user_version` == 1; tables `metric_rollups_5m`, `metric_rollups_1h`, `history_state`, `fs_samples`, `process_samples` exist; `metric_rollups_1m` has column `min_cpu_usage_percent`.
  - `v0_database_is_migrated_with_pre_image_and_json_window`: build a v0 DB by hand with raw `sqlx` (copy the v0 `CREATE TABLE metric_samples` DDL from `lib.rs:1022-1060` verbatim, `user_version` 0), insert 10 rows spaced 10 min apart ending at `now`, each with a 1 KB `snapshot_json`; connect via `SqliteHistoryStore::connect`; assert `<db>.pre-v0.sqlite` exists and opens with 10 rows; main DB `user_version` == 1; rows with `captured_at_ms >= now − 60 min` still have JSON (7), older ones have `NULL` (3); `history_state.schemaMigration` present; file bytes after < bytes before.
  - `migration_refuses_when_pre_image_exists`: pre-create `<db>.pre-v0.sqlite` (empty file); `connect` returns `Err(StoreError::Migration{..})` whose `reason` contains the path and whose `remedy` mentions moving it; `user_version` still 0.
  - `bun_created_database_migrates_the_same_way`: v0 DDL with `snapshot_json TEXT NOT NULL` (as `src/history-store.ts` creates it) → same outcome as the previous positive test.
- [ ] **Step 2: run** `cargo test --manifest-path agent/Cargo.toml -p tinytop-store --test migration_v1` → all FAIL (module missing).
- [ ] **Step 3: implement** `migration.rs`: `ensure_schema` reads `PRAGMA user_version`; `0` with a non-empty `metric_samples` → §7 of the spec exactly: free-space check = `std::fs::metadata(db_path)?.len()` for DB bytes + `disk::free_bytes_at(db_dir)` (ADR 0017: `sysinfo::Disks::new_with_refreshed_list()`, pick the disk whose `mount_point()` is the **longest** prefix of the canonicalised directory, return its `available_space()`; no disk matches → `io::Error` of kind `NotFound` naming the path). Refuse with `StoreError::Migration` when the free bytes cannot be determined **or** `free < db_bytes + db_bytes / 5` (spec §7's 1.2× rule in integer math); `reason` names the path and both numbers, `remedy` says how many bytes to free. Keep the prefix match and the headroom arithmetic as pure `pub(crate)` functions in `disk.rs` with unit tests on BOTH sides of each rule — the refusal test keeps the failure in place (free = 1.19× db must refuse; 1.2× must pass; an empty mount list must be `NotFound`); `VACUUM INTO` the pre-image (refuse if exists); the transaction in §7 step 2; `VACUUM`; write `history_state.schemaMigration`. `0` with empty/no `metric_samples` → create everything at v1 directly. `1` → create-if-not-exists only (idempotent). Move the existing `CREATE TABLE` statements from `lib.rs:1022-1130` into `migration.rs` unchanged except `snapshot_json TEXT` (nullable) and the additive columns/tables from spec §6. `connect` calls `ensure_schema` after the PRAGMAs and before `migrate_runtime_kind_to_canonical` (`lib.rs:992`).
- [ ] **Step 4: run** the test file → PASS; then `cargo test --manifest-path agent/Cargo.toml -p tinytop-store` → all existing tests still PASS (the `snapshot` fixture inserts JSON; nothing else changed for them).
- [ ] **Step 5: docs** — `docs/sqlite-history-architecture.md` §Current Schema: full v1 DDL; new §Schema versions and migration (pre-image path, fail-closed rules, the one automatic `VACUUM`). `CHANGELOG.md` entry.
- [ ] **Acceptance:** `bun run check:rust` green (paste); `migration_v1` 4/4; report the measured migration duration on the 10-row fixture (it will be ms; the live 4 GB run is Fable's to time at deploy).
- [x] **Landed** as run 541 (`ari-sol-deep`), branch commit `8321584`, 4/4 + 4 disk unit tests, 40–45 ms on the fixture; gate green outside the sandbox (the sandbox denies the port `serve_contract` binds — Fable runs the full gate). **Fix-lane T1-fix1** (after luna review run 542, validated by Fable): F1 raw reads must filter `snapshot_json IS NOT NULL` on BOTH runtimes (spec §10/§13 — `src/history-store.ts` + one Bun test are allowed for this) · F2 Windows verbatim-path prefix in `disk.rs` (canonicalise mounts too) · F3 the v0 fixture must be the COMPLETE v0 DDL so the `ALTER`/index path is exercised · F4 `schemaMigration` written inside the schema transaction, idempotent VACUUM completion on the next connect · F5 refusal leaves the pre-image untouched (test) · F6 error text · F7 CHANGELOG under 0.2.7. Brief: `briefs/T1-fix1.md`.

### Task 2: `fold`, tiers, frozen buckets, promote-before-prune

**Files:** Create `agent/crates/tinytop-store/src/ladder.rs`, `agent/crates/tinytop-store/src/maintenance.rs`; Modify `lib.rs` (post-T1 lines: `insert_snapshot` :457-545, `rebuild_rollup_bucket` :946-1035, `prune_raw_history` :731-746, `prune_rollups` :748-759, `HistoryCoverage` :226-238, `history_coverage` :690-729); Modify `agent/crates/tinytop-agent/src/writer.rs` (`maintain_history` :456-470); Tests `tests/ladder_fold.rs`, `tests/ladder_maintenance.rs`; Docs `docs/sqlite-history-architecture.md` (§Rollups And Coverage, §Retention), `CHANGELOG.md`.

**Interfaces — Consumes:** T1's tables and `history_state_get/set`. **Produces:**
```rust
// ladder.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Tier { L1, L2, L3, L4 }
impl Tier { pub fn resolution_ms(self) -> i64 /* L1: poll interval passed at call sites; L2 60_000; L3 300_000; L4 3_600_000 */; pub fn table(self) -> &'static str; pub fn finer(self) -> Option<Tier>; pub fn coarser(self) -> Option<Tier>; }
#[derive(Clone, Copy, Debug, PartialEq)] pub struct Stat { pub avg: f64, pub min: f64, pub max: f64 }
#[derive(Clone, Debug, PartialEq)] pub struct TierBucket { pub bucket_start_ms: i64, pub first_captured_at_ms: i64, pub newest_captured_at_ms: i64, pub sample_count: i64, pub cpu: Stat, pub memory: Stat, pub swap: Stat, pub load: Stat, pub root_used: Option<Stat> }
pub fn fold(bucket_start_ms: i64, finer: &[TierBucket]) -> Option<TierBucket>;
pub fn raw_to_bucket(sample: &RawSampleRow) -> TierBucket;   // RawSampleRow = the sqlx row struct read_tier_buckets(Tier::L1, ..) maps from metric_samples; define it in ladder.rs
pub fn bucket_start_for(resolution_ms: i64, ts_ms: i64) -> i64;
pub fn is_complete(bucket_start_ms: i64, resolution_ms: i64, grace_ms: i64, now_ms: i64) -> bool;
pub fn grace_ms(poll_interval_ms: i64) -> i64;   // max(3000, 2 * poll)
// maintenance.rs
pub struct MaintenanceReport { pub promoted_l3: i64, pub promoted_l4: i64, pub json_stripped: i64, pub pruned: [i64; 4], pub detail_rows: i64, pub expired_l4: i64 }
pub(crate) async fn maintain(store: &SqliteHistoryStore, settings: &DashboardSettings, now_ms: i64) -> Result<MaintenanceReport, StoreError>;
// lib.rs (SqliteHistoryStore)
pub async fn read_tier_buckets(&self, tier: Tier, since_ms: i64, until_ms: i64) -> Result<Vec<TierBucket>, StoreError>;
pub async fn upsert_tier_bucket(&self, tier: Tier, bucket: &TierBucket) -> Result<(), StoreError>;
```
Until T3 lands, `maintain` reads horizons from a `LadderConfig { l1_keep_ms, l2_keep_ms, l3: Option<i64>, l4: Option<i64> /* 0 = forever */, snapshot_json_keep_ms, detail_interval_ms, poll_interval_ms }` built in `maintenance.rs` from the legacy fields (`retention_hours`, `rollup_retention_days`) with L3 = 90 d and L4 = 730 d enabled, JSON 60 min, detail 60 s. T3 replaces that constructor with the real settings block; keep the struct.

- [ ] **Step 1: failing tests** — `tests/ladder_fold.rs` (pure functions, no DB):
```rust
use tinytop_store::ladder::{fold, Stat, TierBucket};
fn b(start: i64, count: i64, avg: f64, min: f64, max: f64) -> TierBucket {
    let s = Stat { avg, min, max };
    TierBucket { bucket_start_ms: start, first_captured_at_ms: start, newest_captured_at_ms: start + 59_000,
        sample_count: count, cpu: s.clone(), memory: s.clone(), swap: s.clone(), load: s, root_used: None }
}
#[test] fn fold_weights_by_sample_count_not_average_of_averages() {
    let out = fold(0, &[b(0, 40, 10.0, 5.0, 20.0), b(60_000, 3, 100.0, 90.0, 100.0)]).unwrap();
    assert_eq!(out.sample_count, 43);
    assert!((out.cpu.avg - (10.0 * 40.0 + 100.0 * 3.0) / 43.0).abs() < 1e-9);
    assert_eq!(out.cpu.min, 5.0); assert_eq!(out.cpu.max, 100.0);
    assert_eq!(out.first_captured_at_ms, 0); assert_eq!(out.newest_captured_at_ms, 119_000);
}
#[test] fn fold_of_empty_is_none() { assert!(fold(0, &[]).is_none()); }
#[test] fn fold_root_used_ignores_buckets_without_a_value() { /* two buckets, one root_used Some(Stat{avg:50,min:50,max:50}) with count 10, one None with count 30 → out.root_used == Some avg 50, min 50, max 50 */ }
```
  and `tests/ladder_maintenance.rs` (temp-dir store; helper `snapshot(ts, cpu)` copied from `tests/sqlite_history_store.rs`):
  - `decimation_regression_completed_minute_keeps_its_sample_count`: insert 40 samples per minute for minutes 0,1,2 (1.5 s apart); call `maintain` with `now = minute 3 + 5 s` and `l1_keep_ms = 90 s` so the L1 cutoff lands *inside* minute 1; assert `metric_rollups_1m` bucket 0 and bucket 1 both still have `sample_count == 40` and their `avg_cpu_usage_percent` unchanged from before the call. **This test must fail on the current code** (bucket 1 would drop to a 1–2 sample tail) — run it once against the unmodified `prune_raw_history` to prove RED, paste that failure in the report.
  - `insert_rebuilds_its_minute_with_min_and_max`: 3 samples cpu 10/50/30 in one minute → bucket has min 10, max 50, avg 30, count 3.
  - `l2_rows_survive_their_horizon_until_l3_has_folded_them`: fill 20 minutes of L2 directly via `upsert_tier_bucket(Tier::L2, …)`; horizon `l2_keep_ms` = 5 min; `history_state.l3FoldedUntilMs` absent → `maintain` prunes nothing from L2 and promotes the complete 5-minute buckets (assert `metric_rollups_5m` count == 3 for 15 complete minutes with grace, watermark advanced); second `maintain` call now prunes the L2 rows behind the watermark and older than the horizon.
  - `promotion_is_bounded_per_call`: 2 days of L2 rows (2,880), `now` after; one `maintain` call folds at most 50 L3 buckets; repeated calls converge; `MaintenanceReport.promoted_l3` sums to 576.
  - `late_write_refolds_ancestors`: after promotion, insert a raw sample whose minute is behind `l3FoldedUntilMs` → the containing 5m and 1h buckets are recomputed (count increases by 1).
  - `json_is_stripped_outside_the_keep_window`: 200 samples over 100 min, keep 60 min → after `maintain`, `COUNT(*) WHERE snapshot_json IS NOT NULL` == samples within 60 min; bounded 500 per call.
  - `l4_forever_never_prunes_l4`: `l4 = Some(0)`; ancient L4 rows survive `maintain`.
  - `disabled_tier_is_neither_written_nor_pruned`: `l3 = None` → no 5m rows written; pre-existing 5m rows (inserted directly) untouched; L4 folds from L2.
  - `detail_rows_written_at_detail_interval`: two inserts 1.5 s apart → one set of `fs_samples`/`process_samples` rows (the snapshot fixture has 1 filesystem and 1 process); a third insert 61 s later → a second set.
- [ ] **Step 2: run** `cargo test -p tinytop-store --test ladder_fold --test ladder_maintenance` → FAIL (modules missing), **except** run the decimation test alone against current code first to capture the RED evidence.
- [ ] **Step 3: implement** `ladder.rs` per the interface; `maintenance.rs` per spec §9 in that exact order; in `lib.rs`: `insert_snapshot` writes detail rows when due (`history_state.lastDetailMs`), calls the new 1m rebuild (extend the existing SQL at :948-966 with `MIN(...)` columns and `MAX(root_used_percent)`), then `refold_ancestors_if_behind_watermarks`; **delete the `rebuild_rollup_bucket` call from `prune_raw_history`** (:742); `prune_rollups` becomes the generic per-tier prune with the watermark guard; `read_tier_buckets` / `upsert_tier_bucket` share one SQL builder keyed by `Tier::table()` (legacy 1m rows: `COALESCE(min_*, avg_*)`); `history_coverage` gains per-tier counts and `snapshot_json_oldest_ms`. In `writer.rs`, `maintain_history` calls `maintenance::maintain` and logs the report at `debug` (non-zero deletions at `info`); a step error is logged at `error` and does not abort the tick.
- [ ] **Step 4: run** both test files → PASS; whole crate → PASS; `cargo test --workspace` under `agent/` → PASS.
- [ ] **Step 5: docs** — §Rollups And Coverage rewritten around `fold` and the freeze rule; §Retention rewritten around promote-before-prune and the watermarks; `CHANGELOG.md` entry that names the decimation bug and the RED→GREEN test.
- [ ] **Acceptance:** `bun run check:rust` green; the decimation test's RED output on old code and GREEN on new code both pasted.
- [x] **Landed** as run 544 (`ari-sol-deep`), branch commit `80ff897`: ladder_fold 4/4, ladder_maintenance 11/11, decimation RED (40 → 16 with the old prune call in place) then GREEN; full gate green outside the sandbox. Plan corrections it surfaced: `maintain` must be `pub` (the agent crate calls it); the prescribed fixture leaves 16 survivors, not 1–2; `maintain_with_config` (doc-hidden) is the test entry; `Tier::resolution_ms()` returns 0 for L1 (call sites pass the poll interval). **Fix-lane T2-fix1** (after luna review run 545): F1 the insert path must fold, not rebuild, a minute whose raw rows are partial (late write into a pruned/boundary minute turned a frozen 40-count bucket into 1/17). Brief `briefs/T2-fix1.md`. The P2 (tier-enabled flags stale for one tick after a save) is assigned to T3.

### Task 3: `retentionLadder` settings, validation, legacy aliases

**Files:** Create `agent/crates/tinytop-store/src/retention_ladder.rs`; Modify `lib.rs` (post-T2 lines: `DashboardSettings` :38-51 and `Default` :85-101, `validate` :133-225, `put_settings` :452-473, `get_settings` :392-411), `maintenance.rs` (replace the `LadderConfig` legacy constructor at :13-24); `writer.rs` (`changed_setting_keys` :554-593, `update_settings` :244-263); Tests `tests/retention_settings.rs`; Docs `README.md` (settings table), `docs/sqlite-history-architecture.md` (§Retention), `CHANGELOG.md`.

**Interfaces — Produces:**
```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct RetentionLadder { pub l1: TierKeep, pub l2: TierKeep, pub l3: ToggledTierKeep, pub l4: ToggledTierKeep, pub snapshot_json_keep_minutes: i64, pub detail_interval_sec: i64, pub archive: ArchiveSettings, pub disk_check: DiskCheckSettings }
pub struct TierKeep { pub keep_days: i64 }
pub struct ToggledTierKeep { pub enabled: bool, pub keep_days: i64 }
pub struct ArchiveSettings { pub queryable: bool, pub cold: bool, pub cold_after_months: i64, pub directory: String }
pub struct DiskCheckSettings { pub interval_minutes: i64, pub min_free_bytes: i64 }
impl RetentionLadder { pub fn validate(&self, disk_pressure: Option<&DiskPressureState>, previous: Option<&RetentionLadder>) -> Result<(), StoreError>; /* corrected 2026-08-28 (T3-fix1): the plan's `bool` could not carry the spec message's `free X`; `DiskPressureState {active, freeBytes, minFreeBytes}` is the history_state.diskPressure document. `put_settings` delegates the pressure rule to this one function. */ pub fn from_legacy(retention_hours: i64, rollup_retention_days: i64) -> Self; pub fn to_ladder_config(&self, poll_interval_ms: i64) -> LadderConfig; }
// DashboardSettings gains: #[serde(default = "RetentionLadder::default_for_serde")] pub retention_ladder: RetentionLadder,
```
Defaults and every rule: spec §5, verbatim. `get_settings`: if the stored document lacks `retentionLadder`, populate it via `from_legacy` (do not persist until the next save). **Added 2026-08-28 (T3-fix1, luna P2):** `DashboardSettings::from_document(document, persisted_ladder: Option<&RetentionLadder>)` is the ONE decoder for any document that may lack `retentionLadder` (store read → `None` → `from_legacy`; HTTP/import payload → `Some(persisted)` → persisted ladder + `apply_legacy_aliases`); `put_settings` never infers absence by value — the ladder it is given is authoritative and the legacy fields are always rewritten from it. **Task 10's import endpoint MUST decode through `from_document`.** `put_settings`: overwrite `retention_hours = l1.keep_days * 24`, `rollup_retention_days = l2.keep_days` before writing, then validate. The disk-pressure rule reads `history_state.diskPressure` (T9 writes it; until then it is absent = inactive).

- [ ] **Step 1: failing tests** in `tests/retention_settings.rs`: table-driven over every rule in spec §5 (one case per rule, asserting the exact error text contains the field name and the observed value), `from_legacy(72, 30)` → `l1 3, l2 30`; `from_legacy(24, 2)` → `l1 3, l2 7`; round-trip save/load keeps `retentionHours == 72` and `rollupRetentionDays == 30` in the stored JSON (read `app_settings.value_json` directly and assert both keys present); disk-pressure rule: with `diskPressure.active=true` extending `l2` from 30 → 31 fails, shrinking to 20 succeeds, enabling `l3` fails; `serde` default: a stored document with **no** `retentionLadder` key deserializes and reports the legacy-derived ladder.
- [ ] **Step 2: run** → FAIL. **Step 3: implement.** **Step 4: run** → PASS; `cargo test --workspace` PASS. **Step 5: docs** as listed; `changed_setting_keys` reports `retentionLadder` (one key) when any sub-field changes.
- [ ] **Acceptance:** `bun run check:rust` green; `bun run check:bun` green (Bun's `src/settings.ts` untouched and its tests still pass because the legacy keys remain).

### Task 4: Read API — four-tier `auto`, coverage, detail endpoints

**Files:** Modify `lib.rs` (`HistoryPointMode` :261-276, `resolve_history_point_source` :731-745, `history_points` :540-545 + a generic `read_tier_history_points(tier, query)`, `history_coverage` :636-670), `writer.rs` (router :193-216, `history_points_query` :507-535, add `history_filesystems`, `history_processes`); Tests — **corrected 2026-08-28 (plan defect, Fable's):** `tinytop-agent` is bin-only (`main.rs` + `writer.rs`; `router`/`AppState` private), so an integration test file cannot reach the router. Tests live in `writer.rs`'s existing `#[cfg(test)] mod tests` (axum `Router` + `tower::ServiceExt::oneshot` against a temp store, no sockets); dev-deps `tower = { version = "=0.5.3", features = ["util"] }` + `http-body-util = "=0.1.3"` (both already transitive in `Cargo.lock`) — ~~`tests/history_api.rs`~~; Docs `docs/sqlite-history-architecture.md` (§Read Path), `README.md` (API table), `CHANGELOG.md`.

- [ ] **Step 1: failing tests:** `auto_picks_finest_tier_that_still_holds_the_range_start` — table (one test, every row asserts `source` AND `resolutionMs`; unless a row says otherwise: `limit=10_000`, `pollIntervalMs=1_500`, keepDays 3/30/90/730, L3+L4 enabled, `archive.queryable=false`, `untilMs=now`): (now−1 h → L1, `resolutionMs` 1500), (now−2 d → L2: L1 holds 3 d but 115,200 raw points > limit), (now−6 d → L2: L1 fails (b)), (now−30 d → L3: L2 holds it but 43,200 > limit; 8,640 fits), (now−60 d → L4: L3 holds it but 17,280 > limit; 1,440 fits), (now−300 d → L4 by (b): L3 keeps 90 d), (now−30 d with L3 disabled → L4: L2 overflows, L3 skipped by (a)), (now−30 d with `limit=100` → L4: no tier fits (c), coarsest tier holding the start), (now−800 d → L4: nothing satisfies (b), archive not queryable → coarsest enabled), (now−800 d with `archive.queryable=true` → `archive`, empty page, `available:false` until T7), (now−1 h with `untilMs` ABSENT → L1: `range_ms` uses `now`, never 0), (L4 `keepDays=0`, now−3000 d → L4: 0 = forever satisfies (b)). **Corrected 2026-08-28 after run 554 escalated:** the original six rows were written without the arithmetic (`limit 100 over 30 d → L3` is impossible: 30 d = 43,200 / 8,640 / 720 points at 1 m / 5 m / 1 h) — spec §10 now carries the amended rule with the same worked table; response JSON has `source` and `resolutionMs`. `coverage_reports_every_tier_and_json_horizon` — shape per spec §10 (assert keys, not values). `filesystems_endpoint_filters_by_mount_and_clamps_limit`; `processes_endpoint_groups_by_capture_time`. `raw_history_omits_rows_without_json`.
- [ ] **Step 2: run** → FAIL. **Step 3: implement** per spec §10 — the resolver takes `(&RetentionLadder, now_ms, query)`; `"5m"`, `"1h"`, `"archive"` parse; `Archive` returns an empty page until T7 (documented in the response as `"source":"archive"` with `"available":false`). **Step 4: run** → PASS; workspace PASS. **Step 5: docs.**
- [ ] **Acceptance:** `bun run check:rust` green; `curl` transcript of the three endpoints against `cargo run -p tinytop-agent -- serve` on a temp DB (`--sqlite sqlite:///tmp/…`) pasted.

### Task 5: Dashboard — presets, ladder settings, shrink confirmation, coverage card

**Files:** Modify `agent/assets/dashboard/app.js` (`HISTORY_WINDOWS` :41-49, `validateDaemonSettings` :2326-2347, `renderEffectiveSettings` :2348-2380, `collectDaemonSettingsFromForm` :2487-2520, the settings save handler that calls `PUT /api/settings`, the coverage renderer), `agent/assets/dashboard/index.html` (settings dialog `#settings-dialog` :367-560: new "History ladder" fieldset, legacy inputs made `readonly` with hint), `agent/assets/dashboard/styles.css` (fieldset + banner styles, match existing tokens); Tests: `tests/dashboard-ladder.test.ts` (Bun) if `app.js` exposes testable pure functions — add `validateRetentionLadder(ladder, previous, diskPressure)` and `historyWindowFor(key, coverage)` as pure functions and export them behind the existing module pattern (check how `tests/*.test.ts` import from `agent/assets/dashboard/app.js` or `src/`; if the dashboard is not importable, put the pure functions in `agent/assets/dashboard/ladder-rules.js` loaded by `index.html` before `app.js` and import that file in the Bun test); Docs `GUIDE.md` (settings walkthrough), `CHANGELOG.md`.

Server-computed confirmation: the save handler first calls `POST /api/settings/import?dryRun=true` with `{"tinytopConfigVersion":1,"settings":candidate}` — **T10 implements that endpoint; until T10 lands, the dashboard computes `wouldDelete` from `/api/history/coverage` tier counts (rows older than the new horizon ≈ `bucketCount × (1 − newKeep/oldKeep)` clamped ≥ 0) and labels it "approx."**; T10 switches it to the server number and removes the approximation (T10's task lists this edit).

- [ ] **Step 1: failing Bun tests:** `HISTORY_WINDOWS` has keys `live,15m,1h,6h,24h,7d,30d,90d,1y,all` with sources `raw,raw,raw,rollup,rollup,rollup,rollup,5m,1h,auto`; `validateRetentionLadder` mirrors each server message (copy the exact strings from `retention_ladder.rs`); `historyWindowFor("90d", coverageWithL3Disabled)` returns `{disabled:true, reason:"retentionLadder.l3.enabled"}`.
- [ ] **Step 2: run** `bun test tests/dashboard-ladder.test.ts` → FAIL. **Step 3: implement** per spec §11. **Step 4:** `bun run check:bun` PASS; `cargo test -p tinytop-agent` still PASS (the Rust binary embeds the assets — `bun run check:rust` must also pass, and the report must state that the Rust agent was rebuilt after the asset edit, per `CLAUDE.md`).
- [ ] **Acceptance:** both gates green; a screenshot or `./tinytop start` + manual checklist: open settings → ladder group renders → set L2 to 6 → inline error names "l2.keepDays ≥ 7" → set L4 forever → save → coverage card shows four tiers.

### Task 6: CLI + docs + Phase 1 close-out material

**Files:** Modify `agent/crates/tinytop-agent/src/main.rs` (`db stats` output, new `db pre-image status|remove`, usage text :352), `README.md`, `ARCHITECTURE.md`, `INSTALL.md` (migration note: first start after upgrade takes a pre-image and may take minutes on a large DB; free space ≥ 1.2×), `PROGRESS.md`, `CHANGELOG.md` (0.3.0 heading consolidating T1–T6); Tests: `agent/crates/tinytop-agent/tests/cli_db.rs` (spawn the binary with `--sqlite` on a temp DB; `db stats --json` contains `tiers`; `db pre-image remove` refuses when `user_version` < 1 and when `integrity_check` != `ok`).

- [ ] Steps: failing test → implement → pass → docs. **Acceptance:** `bun run check` green; `tinytop-agent db stats --json` sample pasted.

**Phase 1 close (Fable):** deep dual-blind over `v0.2.5..HEAD`; findings validated and fixed; `VERSION` 0.3.0; tag; `gh release` with notes; `cargo audit` + `bun audit` output in the notes. **Deploy on Michel's nod only** — the first start migrates the 4 GB live DB (pre-image ≈ 4 GB, needs ≥ 5 GB free; expect minutes).

# Phase 2 — archive and disk (→ 0.4.0)

### Task 7: Queryable archive (move expired L4 rows into `history-archive.sqlite`)

**Files:** Create `agent/crates/tinytop-store/src/archive.rs`; Modify `lib.rs` (`StoreError::Archive`, `read_tier_history_points` for `Archive`), `maintenance.rs` (the "expire L4" step), `writer.rs` (coverage `archive.queryable` block); Tests `tests/archive.rs`; Docs `docs/sqlite-history-architecture.md` (new §Archive), `README.md`, `CHANGELOG.md`.

**Interfaces — Produces:**
```rust
pub struct ArchivePaths { pub db: PathBuf, pub directory: PathBuf }
pub fn archive_paths(main_db: &Path, settings: &ArchiveSettings) -> ArchivePaths;
pub(crate) async fn ensure_archive_schema(paths: &ArchivePaths) -> Result<(), StoreError>;   // user_version 1, metric_rollups_1h, archive_manifest
pub(crate) async fn move_expired_l4(store: &SqliteHistoryStore, paths: &ArchivePaths, cutoff_ms: i64, batch: usize) -> Result<i64, StoreError>; // returns rows moved; ATTACH…INSERT OR IGNORE…verify count…DELETE…DETACH per batch
pub(crate) async fn read_archive_points(paths: &ArchivePaths, since_ms: i64, until_ms: i64, limit: i64) -> Result<Vec<TierBucket>, StoreError>;
```
- [ ] **Tests:** `expired_l4_rows_move_and_main_rows_vanish_only_after_verified_insert` (inject a failure after INSERT by pointing the archive path at a read-only directory on the second batch → main rows remain, error surfaced, watermark not advanced); `archive_directory_setting_relocates_the_file`; `auto_falls_through_to_archive_for_ranges_older_than_l4`; `archive_is_never_attached_while_idle` (after a move, `PRAGMA database_list` on the pool shows only `main`).
- [ ] Steps: RED → implement (spec §9 "expire L4" + §6 archive DDL) → GREEN → docs. **Acceptance:** `bun run check:rust` green; a transcript showing `db stats --json` with `archive.queryable.bucketCount` after a forced move (`db archive status`).

### Task 8: Cold export — verified monthly `csv.gz` + `.sha256`

**Files:** Modify `archive.rs`, `agent/Cargo.toml` (`flate2 = "=1.1.<latest>"`, default features — pure-Rust `miniz_oxide`), `tinytop-store/Cargo.toml`, `writer.rs` (hourly scheduler), `main.rs` (`db archive export-now`); Tests `tests/archive.rs` (append); Docs `docs/reports/2026-MM-DD-dependency-vetting-flate2.md` (rule 5: version pinned, advisories via `cargo audit`, maintenance/adoption, alternatives weighed — zstd, xz — and why gzip won: measurement in ADR 0014), `docs/sqlite-history-architecture.md` §Archive, `GUIDE.md`, `CHANGELOG.md`.

- [ ] **Tests:** `cold_export_writes_verified_month_files` (build an archive with 3 months; `coldAfterMonths=1`; run export → two files + two sidecars; `sha256sum -c` via `std::process::Command` succeeds; manifest has 2 rows; watermark `coldExportedUntilMonth` set); `corrupted_tmp_does_not_advance_month` (inject a writer that truncates; assert `.tmp` remains, no `.csv.gz`, watermark unchanged, error names the step); `csv_round_trip_is_row_exact` (decompress, parse with a minimal RFC 4180 reader in the test, compare every field with the archive rows); `cold_requires_queryable` (validation).
- [ ] Steps: RED → implement (spec §9 "cold export") → GREEN → vetting report → docs. **Acceptance:** `bun run check:rust` green; `cargo audit` output pasted; a real `zcat file | head -3` transcript from the test artifact.

### Task 9: Disk check + pressure state + growth refusal + UI banner

**Files:** Create `agent/crates/tinytop-store/src/disk.rs` (`trait FreeBytesProvider { fn free_bytes(&self, path: &Path) -> io::Result<u64> }`, real impl = `disk::free_bytes_at` landed by T1 (sysinfo, ADR 0017), `DiskPressure` state + `check(...)`); Modify `writer.rs` (hourly task; coverage `disk` block), `retention_ladder.rs` (`validate` consumes `diskPressure`), `app.js`/`index.html` (banner from coverage), `main.rs` (`db stats` shows disk); Tests `tests/disk_check.rs` (injected provider: breach → `diskPressure` marker once, state active; recovery → `diskRecovered`, state inactive; growth refused / shrink allowed via `RetentionLadder::validate`); Docs `GUIDE.md`, `README.md`, `CHANGELOG.md`.
- [ ] Steps: RED → implement (spec §9 hourly block) → GREEN → docs. **Acceptance:** `bun run check` green; transcript of `db stats --json` showing `disk.pressure=true` after running with `--min-free-bytes` set above the box's free space in a temp config (add that flag only to the test harness, not to settings).

**Phase 2 close (Fable):** deep dual-blind; `VERSION` 0.4.0; tag; release; audits.

# Phase 3 — configuration transfer (→ 0.4.x)

### Task 10: Settings export/import — API, CLI, dashboard buttons

**Files:** Modify `writer.rs` (routes `GET /api/settings/export`, `POST /api/settings/import` with `dryRun`; `wouldDelete` computed from tier counts older than the candidate horizons via one `SELECT COUNT(*)` per tier), `lib.rs` (`count_rows_older_than(tier, cutoff_ms)`), `main.rs` (`config export [--out FILE]`, `config import FILE [--dry-run]`), `app.js`/`index.html` (Export/Import buttons; **replace T5's approximate `wouldDelete` with the dry-run response and delete the "approx." label**); Tests `agent/crates/tinytop-agent/tests/settings_transfer.rs` (export shape; dry-run diff lists `changedKeys` and `wouldDelete`; import applies and records `settingsChange{source:"import"}`; unknown top-level key refused; `tinytopConfigVersion: 2` refused naming max 1; import under disk pressure refuses growth); `tests/cli_config.rs`; Docs `README.md`, `GUIDE.md`, `CHANGELOG.md`.
- [ ] Steps: RED → implement (spec §10 config, ADR 0016) → GREEN → docs. **Acceptance:** `bun run check` green; `config export | config import --dry-run` round-trip transcript pasted.

**Phase 3 close (Fable):** fast review; `VERSION` 0.4.1; tag; release.

# Phase 4 — OpenTelemetry (→ 0.5.0)

### Task 11: OTLP metrics push exporter

**Files:** Create `agent/crates/tinytop-agent/src/otel.rs`; Modify `agent/Cargo.toml` (+ `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` at one exact version, features: `metrics`, `http-proto` + the reqwest client feature the crate documents for HTTP/protobuf, `default-features = false`), `tinytop-agent/Cargo.toml`, `lib.rs`/`retention_ladder.rs` sibling `otel_settings.rs` (`OtelSettings` block, spec §12, defaults + validation: endpoint parses as `http`/`https` URL, interval 5–3600, `headersEnvVar` matches `^[A-Z][A-Z0-9_]*$`), `writer.rs` (spawn/stop the exporter on settings change; coverage `otel` block), `app.js`/`index.html` (OTel settings group), `main.rs` (`db stats` shows otel); Tests `agent/crates/tinytop-agent/tests/otel_export.rs` (a tokio `TcpListener` HTTP server in the test that accepts `POST /v1/metrics`, decodes the protobuf `ExportMetricsServiceRequest` with the `opentelemetry-proto` types the exporter crate re-exports, asserts metric names from spec §12 and resource `service.name=tinytop`; a second test points at a closed port and asserts `failures` increments while `insert_snapshot` continues on schedule); Docs `docs/reports/2026-MM-DD-dependency-vetting-opentelemetry.md` (rule 5; **the lane confirms the latest stable on crates.io at execution time — 0.32.0 was current at planning — checks `cargo audit`, notes pre-1.0 status and the exact-pin policy**), `README.md` (OTel section with a collector example), `GUIDE.md`, `ARCHITECTURE.md` (two-runtime section: daemon-only), `CHANGELOG.md`.
- [ ] Steps: vetting report first (if the crates fail vetting — unmaintained, advisory, MSRV > 1.95 — **STOP and report; do not substitute a crate**) → RED → implement → GREEN → docs. **Acceptance:** `bun run check` green; `cargo audit` clean pasted; the test server's decoded metric list pasted.

**Phase 4 close (Fable):** deep dual-blind; `VERSION` 0.5.0; tag; release; audits.

---

## Contingencies

- **`VACUUM INTO` is refused by sqlx's pool (e.g. "cannot VACUUM from within a transaction")** → run it on a dedicated `SqliteConnection` opened with `SqliteConnectOptions::from_str(url)` outside any transaction; never on the pool.
- **`ALTER TABLE … RENAME` breaks the `UNIQUE(captured_at_ms)` constraint name or the indexes** → recreate both indexes explicitly after the rename (`idx_metric_samples_captured_at`, `idx_metric_samples_runtime_captured_at`) and assert them in `fresh_database_is_created_at_schema_version_1`.
- **The decimation regression test is GREEN on the unmodified code** → the fixture is wrong (the cutoff is not inside a completed minute). Re-read spec §9; the L1 cutoff must fall strictly inside minute 1 with `now` past minute 2's grace. Do not weaken the assertion.
- ~~**`statvfs` is not exposed by `tinytop-collectors`** → add one `pub fn free_bytes_at(path: &Path) -> io::Result<u64>` there (feature-gated exactly like ADR 0012's inode collection), nothing else.~~ **RESOLVED 2026-08-28 — T1 escalated on this, correctly:** `tinytop-store` has no dependency on `tinytop-collectors` (and must not gain one), and the collector's `statvfs` is Linux-only while `macos.rs`/`windows.rs` are real targets. The free-space check lives in `tinytop-store/src/disk.rs` on `sysinfo` (already pinned in the workspace) — **ADR 0017**. No lane touches `tinytop-collectors` for this.
- **The dashboard `app.js` cannot be imported by Bun tests** → create `agent/assets/dashboard/ladder-rules.js` for the pure functions, load it from `index.html` before `app.js`, import it in the test; the Rust embed list in `writer.rs:static_relative_path` and `router` must gain the new asset path (that is an allowed edit for T5).
- **`opentelemetry-otlp` 0.32's HTTP client feature pulls a TLS stack that fails to build on the box** → use the `reqwest-client` feature with `default-features = false` and `rustls-tls` if the crate offers it; if neither builds, STOP and report with the exact error.
- ~~**The `auto` test row `limit 100 over 30 d → L3`**~~ **RESOLVED 2026-08-28 — T4 escalated on this, correctly (run 554):** the row was arithmetically impossible (no tier fits 100 points over 30 d). Spec §10's `auto` rule is amended (L1 resolution = `pollIntervalMs`; `range_ms` uses `now` when `until` is absent; no-tier-fits-(c) → coarsest tier holding the start) and Task 4 Step 1's table is rewritten with explicit `limit`/`pollIntervalMs`/keepDays per row. Re-dispatched as key `tinytop-ladder-t4b` on the same branch.
- **Observed 2026-08-28 (not a contingency — a dashboard defect to fix once T4 lands; T5-fix or T6, decided at the T5 review):** `app.js` fetches rollup presets as ONE page of ≤ `MAX_HISTORY_PAGE_SIZE` (10,000) buckets — `30d` at 1 m holds 43,200 buckets, so the chart silently shows only the newest 6.9 d; `7d` loses its oldest 80 minutes; §11's `90d` at 5 m (25,920) would show 34.7 d. The fix is the ladder itself: presets `7d`/`30d`/`90d`/`1y` request `source=auto&limit=10000` and render by the returned `source`/`resolutionMs` (auto gives 7d → L3, 30d → L3, 90d → L4, 1y → L4 — every range complete in one page).
- **Default rule:** anything not covered here — STOP and report in the lane's final message. Do not improvise around an undocumented obstacle.

## Out of scope (lanes must not do these even if tempting)

- Repairing or deleting legacy decimated 1-minute rows. Deleting the pre-image automatically. Any `VACUUM` outside the v1 migration and the explicit CLI verb.
- Configurable resolutions, percentiles, sketches, Parquet, zstd, Prometheus scrape, gRPC, reading OTel.
- Moving `DashboardSettings` or other existing code out of `lib.rs` (new code goes in new modules; existing code stays put).
- Touching `src/` (Bun runtime) or `legacy/` except where T5's contingency names a test import.
- Dependency bumps of existing pinned crates. Formatting sweeps. Renaming existing settings keys.
- UI charts for `fs_samples` / `process_samples` (API only in this plan).

## Verification (Fable, at each phase close and once at the end)

1. `bun run check` green on `main`; `cargo audit` (under `agent/`) and `bun audit` clean.
2. On a **copy** of the live DB (`cp ~/.local/share/tinytop/history.sqlite /tmp/tt/`), `tinytop-agent serve --sqlite sqlite:///tmp/tt/history.sqlite`: pre-image appears, `user_version` 1, size drops to tens of MB, `db stats --json` shows four tiers, `/api/history/points?source=auto` over 60 d returns 5-minute buckets with `sample_count` > 1 for *new* buckets.
3. Decimation proof: after 3 days of runtime (or the test's simulated clock), every 1-minute bucket older than L1's horizon still reports its full `sample_count`.
4. Shrink L2 in the dialog → confirmation shows server counts → rows disappear on the next tick; extend under injected disk pressure → refused with the rule named.
5. Archive: force `l4.keepDays = 1` on the copy → rows move to `history-archive.sqlite`; `db archive export-now` → `sha256sum -c *.sha256` OK; `zcat` shows the header.
6. `config export > a.json`; edit; `config import a.json --dry-run` shows the diff; import; marker recorded.
7. OTel: point at a local collector (`otelcol` or the test receiver) → metrics arrive with spec §12 names; stop the collector → `failures` climbs, collection continues.
8. Bun runtime: `TINYTOP_RUNTIME=bun ./tinytop start` still serves the dashboard and writes samples into the v1 schema.
