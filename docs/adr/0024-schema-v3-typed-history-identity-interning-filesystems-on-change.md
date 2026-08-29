# ADR 0024 — Schema v3: `metric_samples` rebuilt without `snapshot_json`, per-row uptime and the three scalars, `host_identity` interning, filesystems on change keyed by the enumeration stamp with presence events, `/api/history` assembled from typed tables

- **Status:** Accepted (2026-08-30) — written at Task 14 dispatch (plan `docs/plans/2026-08-29-cadence-classes-and-gpu-plan.md`, Task 14 + its amendments); extends ADR 0021 (decision 2 and 4) and ADR 0023 (the migration shape).
- **Deciders:** Fable (planner), Michel (Phase 5 GO 2026-08-29).

## Context

ADR 0021 decided that history becomes typed and the per-sample JSON blob is dropped (decision 2), and
that filesystems are checked on the slow tick and stored on change (decision 4). Task 14 is the lane
that does it. Before writing its brief the orchestrator had a fact sheet built from the code at
0.5.1 (Fabulous `docs/fleet/tinytop/2026-08-29-t14-fact-sheet.md`, `## PLAN vs CODE`) and re-anchored
every claim at 0.5.2 (`b13f89c`). Five of its findings change the design the plan text described:

1. **Store-on-change conflicts with the plan's carry-forward rule.** Task 14 carried a mount forward
   only when a row for it was seen within `detailIntervalSec × 2`, but an unchanged mount writes no
   row, so after two intervals it would vanish from every replayed snapshot although it never left the
   box. No heartbeat was written anywhere. The same gap made `filesystemsCapturedAtMs` (read by the
   dashboard's freshness label, `ladder-rules.js` `describeFilesystemFreshness`) unreconstructible.
2. **Pressure is read.** The dashboard's pressure card and the pressure gauge read
   `pressure.{cpu,memory,io}.some.avg10` (`app.js` `pressureValue`); ADR 0021 and the plan said the
   dashboard read nothing from `pressure`. `pressureValue` returns `0` for an absent value, so a
   history snapshot without pressure would render fabricated zeros.
3. **Uptime by boot-time derivation is not exact.** The plan derived `identity.uptimeSeconds` from a
   `boot_time_ms` stored on the identity row (`captured − boot`). A reboot inside the retained window
   makes that wrong for every row before it, and the "rounded to the second so jitter does not spawn
   rows" rule was a heuristic on top of it.
4. **The three scalars are needed for assembly, not because consumers read them.**
   `memory.availableBytes`, `swap.freeBytes` and `load.lastPid` are read by no consumer, but they are
   non-optional fields of `SystemSnapshot`; an assembled snapshot must carry the observed value or
   nothing can be assembled without inventing one.
5. **Dashboard replay is dead today.** `normalizedHistorySamples` drops `source` and
   `renderSelectedSample` requires `"raw"`, so `/api/history` snapshots are never rendered in detail.
   Task 14's acceptance (a point 30 minutes back renders processes and filesystems) cannot pass
   without repairing it; that repair is a dashboard change and belongs to the dashboard lane (T14b).

Two more facts shape the migration: the two thread columns are `NOT NULL` (the T12 luna finding
that makes them `Option` needs a table rebuild — SQLite has no `ALTER COLUMN`), and the store already
implements the safe rebuild pattern (`rebuild_v0_schema`: create new, copy, drop old, rename).

## Decision

1. **`metric_samples` is rebuilt in ONE transaction, no pre-image (ADR 0023's shape), into the v3
   shape:** every v1 column except `snapshot_json` (dropped), `runnable_threads` and `total_threads`
   made nullable, plus `identity_id INTEGER REFERENCES host_identity(identity_id)`,
   `uptime_seconds INTEGER`, `memory_available_bytes INTEGER`, `swap_free_bytes INTEGER`,
   `last_pid INTEGER`, `filesystems_captured_at_ms INTEGER`, and the table constraint
   `CHECK (identity_id IS NULL OR (uptime_seconds IS NOT NULL AND memory_available_bytes IS NOT NULL AND swap_free_bytes IS NOT NULL))`.
   `sample_id` values are preserved by the copy; both indexes are recreated. A row is **assembleable
   iff `identity_id IS NOT NULL`**; the constraint makes the database enforce that an assembleable row
   carries every non-optional scalar. (`last_pid` is outside the constraint because it is optional on
   the type after this ADR — see 5.)
2. **Backfill from each row's own JSON, never from a neighbour.** During the rebuild every v2 row that
   still holds `snapshot_json` (the 60-minute window at migration time) is decoded and its identity is
   interned, its `uptime_seconds`, `memory_available_bytes`, `swap_free_bytes`, `last_pid` and
   `filesystemsCapturedAtMs` are copied into the new columns. Rows whose JSON was already stripped get
   NULLs and stay non-assembleable — exactly the rows `read_history` refuses today
   (`WHERE snapshot_json IS NOT NULL`), so the migration loses no replayable sample and invents none.
3. **`host_identity` interns the eight identity strings** (`hostname`, `platform`, `arch`, `distro`,
   `kernel`, `runtime_kind`, `runtime_confidence`, `runtime_reason`; `UNIQUE` over all eight,
   `first_seen_ms`); uptime is NOT part of identity and is stored per row (8 B/row, ≈ 1.4 MB per 72 h
   at 1.5 s), so `identity.uptimeSeconds` round-trips exactly through any number of reboots with no
   jitter rule. The writer keeps the current identity in memory, primed at connect from the newest
   sample's row, and interns a new row only when one of the eight strings differs.
4. **Filesystems: on-change rows keyed by the enumeration stamp, plus presence events.** The key of the
   filesystem domain is `fs_key_ms = snapshot.filesystemsCapturedAtMs` (the collector's enumeration
   stamp) or, when a collector does not stamp, the sample's `captured_at_ms`; `captured_at_ms` is taken
   by the writer after `collect()` returns, so the stamp never exceeds it. A `fs_samples` row is written
   at `fs_key_ms` only when the enumeration is new (stamp differs from the last one written) AND the
   mount is new or differs from its last stored row in `filesystem`, `fs_type`, `size_bytes`,
   `used_bytes`, `available_bytes`, `inode_used` or `inode_total`. A new table
   `fs_mount_events (captured_at_ms, mount, present ∈ {0,1}, PRIMARY KEY (mount, captured_at_ms)) WITHOUT ROWID`
   receives one row when a mount appears and one when it disappears. Each `metric_samples` row stores
   its stamp (`filesystems_captured_at_ms`). Assembly at a row with key `E`: `filesystemsCapturedAtMs`
   is the stored stamp; a mount is present iff its newest event at or before `E` has `present = 1`; its
   values are its newest `fs_samples` row at or before `E`. No time-window heuristic remains. The
   migration backfills the events from the existing per-interval rows (`present = 1` at each mount's
   first row; `present = 0` one millisecond after the last row of every mount absent from the newest
   detail tick) so pre-v3 rows assemble by the same rule. Pruning keeps, per mount, the newest row and
   the newest event even when older than the L2 horizon (the carry-forward floor), and drops both once
   a mount's newest event is `present = 0` and older than the horizon.
5. **Type changes (the only ones):** `LoadSnapshot.runnable`, `total_threads` and `last_pid` become
   `Option<u64>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), `None` where no source
   exists (the sysinfo collectors; Linux keeps `/proc/loadavg`); T12-fix1's `process_totals` stopgap
   goes. `IdentitySnapshot.uptime_seconds` stays `u64` — every collector has it and every assembleable
   row stores it. `cpu.times` and every `pressure.*.{some,full}` are `None` in history (ADR 0021
   decision 2, unchanged); the dashboard must render `—` for them, never `0` (T14b).
6. **Pressure is not stored.** Storing what the card reads exactly would mean the four `PressureLine`
   fields for three resources (twelve REAL columns, ≈ 100 B/row — more than half the typed row) to
   replay one 10-second average per resource; the honest alternative is a documented omission with an
   honest `—` in replay. ADR 0021's sentence "the dashboard reads nothing from `pressure`" is amended,
   not the decision.
7. **`started_at` stays TEXT in both process tables until schema v4 (Task 15).** The 66.7 B/row measured
   at T13 against the ≤ 60 B target is carried as a baseline; converting to `started_at_ms INTEGER`
   requires rebuilding the WITHOUT ROWID fast table and belongs with the next table-shape migration.
8. **`/api/history` answers from the assembler; `latest_snapshot()` is deleted; every JSON path goes**
   (`strip_snapshot_json`, `count_snapshot_json_older_than`, the coverage counters, `db stats`'s
   `snapshotJsonSampleCount`, `wouldDelete.snapshotJsonRows`, `LadderConfig.snapshot_json_keep_ms`,
   `MaintenanceReport.json_stripped`, `RetentionLadder.snapshot_json_keep_minutes`). Older settings
   documents and a dashboard cached at 0.5.2 still send `snapshotJsonKeepMinutes`: both write paths
   accept and ignore it, the import path warning reads exactly
   `snapshotJsonKeepMinutes is no longer used and was ignored`.
9. **Fresh files are created directly in v3 shape; `user_version` 3; one `schemaMigrated` marker per
   migration; the chain v0 → v1 → v2 → v3 is proven on a read-only copy of the live file at gate time.**

## Alternatives rejected

- **A per-mount `last_seen_ms` heartbeat (one UPDATE per mount per slow tick).** Exact for the present,
  wrong for a mount that disappears and comes back (a single stamp cannot say the mount was absent in
  between), and 27 × 1,440 row writes a day for information the events table carries in two rows.
- **An enumeration log table (one row per slow tick) to reconstruct `filesystemsCapturedAtMs`.** Not
  needed once the stamp is stored per sample (8 B/row); the stamp is what the live snapshot said.
- **Deriving uptime from a stored boot time.** See Context 3.
- **Making `memory.availableBytes`/`swap.freeBytes` optional on the type to tolerate rows without them.**
  The rows without them are exactly the rows that were never replayable; refusing to assemble them keeps
  the type honest and the API unchanged.
- **Storing pressure typed.** See Decision 6.
- **`ALTER TABLE ... DROP COLUMN snapshot_json` plus tolerated `NOT NULL` thread columns.** The
  columns must become nullable for the honest `Option`; SQLite cannot relax a constraint in place; one
  rebuild does both.
- **Assembling from the newest JSON row's identity for every older row (the plan's wording).** Rows
  without JSON are not assembled at all (Decision 2), so nothing is borrowed from a neighbour.
- **Converting `started_at` to integer milliseconds in this lane.** See Decision 7.

## Consequences

- `metric_samples` ≈ 174 + 8 (uptime) + 8 + 8 + 8 (scalars) + 8 (stamp) + 8 (identity) ≈ 222 B/row
  typed, still ≈ 38 MB per 72 h at 1.5 s and flat; `fs_samples` near zero between real changes;
  `fs_mount_events` two rows per mount lifetime; `host_identity` one row per distinct identity.
- `/api/history` and `/api/history/filesystems` document the on-change rule; `/api/history` snapshots
  omit `cpu.times` and every `pressure` line, and `load.runnable`/`totalThreads`/`lastPid` may be absent.
- The dashboard needs T14b before the acceptance can pass (replay repair, `—` rendering, the JSON
  control removed, `WOULD_DELETE_FIELDS` without `snapshotJsonRows`).
- A v1 file's slack is reclaimed only by the operator's `db vacuum` (ADR 0023's pre-image/VACUUM law).
- Every nullable column is read as `Option<T>` (sqlx-sqlite decodes NULL into `i64` as 0 silently).
- The migration decodes up to one hour of JSON rows (≈ 2,400 × 28 KB on the live box) inside the
  transaction; plan §4's 60-second budget applies and is measured on the live-file copy at gate time.

---

**Amendment 2026-08-30 (T14-fix1, after the orchestrator's real-file gate of lane T14 — hexe run #661,
commit `e29468d`).** Decision 2 says every row that still holds `snapshot_json` "is decoded"; the T14
brief made an undecodable row REFUSE the migration and leave the file untouched (guard before the drop).
The first real-file run (a fresh `sqlite3.backup()` of the live v1 database: 42,893 rows, 2,451 JSON
rows) refused the whole file: 25 rows written by the legacy Bun collector during a 36-second window
carry `filesystems[].inodeUsed = -999001` — that writer computes `inodeTotal − inodeFree` unclamped,
and WSL's drvfs mount `/usr/lib/wsl/drivers` reports more free inodes than total (`f_files 999`,
`f_ffree 1,000,000`; the Rust collector clamps and writes `0`). `FilesystemSnapshot.inode_used` is
`Option<u64>`, so serde refuses the negative integer and the daemon exits 1 on every start until an
operator edits rows by hand. **Ruling:** the refusal stays for JSON this version does not know, but a
KNOWN quirk of our own legacy writer is normalised, not refused — during the v2→v3 backfill a negative
`inodeUsed`/`inodeTotal` becomes absent (the backfill stores nothing from those two fields, so no value
is lost or invented), the number of such rows is counted in the migration audit
(`legacyInodeRowsNormalised`) and in the `history migration info` line, and the remaining refusal's
remedy names the manual SQL (`INSTALL.md` §Upgrade) instead of `db check` (which is
`PRAGMA integrity_check` and shows no row). **Rejected:** a lenient deserializer on the type (the type
stays honest — a negative count is not a count — and the migration is the only place the daemon decodes
legacy JSON); downgrading ANY undecodable row to non-assembleable (a count would hide a systematic type
mismatch across the whole file); patching the Bun collector (Michel's ruling 2026-08-29: `legacy/` is
not updated). Measured on the same copy with the 25 rows' payload cleared: v1→v2 363 ms, v2→v3 2,641 ms
(2,426 JSON rows decoded, 1 identity, 27 filesystem events), total 3,266 ms — plan §4's 60 s budget holds.

**Amendment 2026-08-30 #2 (T14-fix2 — luna run #667's P1 on `tinytop-store/src/lib.rs:1588`, validated by the
orchestrator as a CONTRACT gap, not a code defect).** Decision 4 says a `fs_samples` row is written "when the
enumeration is new (stamp differs from the last one written)". The writer requires the stamp to be NEWER than the
last processed stamp, and that is the correct rule: a sample whose `filesystemsCapturedAtMs` is OLDER than the last
processed stamp cannot be replayed correctly — in `1000 (/) → 3000 (/) → late 2000 (/ + /data)` processing the late
stamp would write `appear(/data)@2000` while nothing recorded `/data` as absent at 3000 (absent mounts leave no
row), so every assembly after 3000 would show `/data` present. The successor enumeration's mount set is not
recoverable, so the late stamp's filesystems are discarded: the metric row is stored with its stamp, no
`fs_samples` row and no `fs_mount_events` row is written, the in-memory state keeps the newest stamp, and the
assembled row at that stamp shows the filesystem state as of the newest stamp at or before it. **T14-fix2 adds
the missing diagnostic:** the discard is warned once a minute (`history writer warning: …`, the same
rate-limited mechanism as the process-row warning) instead of being silent, and the architecture document states
the rule. A stamp EQUAL to the last processed one is the normal steady state (no new enumeration), never a warning.
**Rejected:** processing a late stamp (the corruption above; pinned by
`regressing_filesystem_key_cannot_reintroduce_a_mount_into_future_history`); refusing the whole sample (loses a
metric row for a filesystem-domain problem); storing the full mount set per enumeration to make late stamps
replayable (27 × 1,440 rows a day for a case that needs a backwards clock step or a second writer).
