# ADR 0017 — `tinytop-store` owns its free-space check (on `sysinfo`), not `tinytop-collectors`

**Status:** Accepted (2026-08-28) — decided under the tinytop-ladder GO; supplements 0013 (Task 1's migration).

## Context

Spec §7 makes the v0→v1 migration **fail closed**: before `VACUUM INTO` the pre-image, refuse
unless the DB's filesystem has free space ≥ 1.2 × the DB's bytes. The plan (Task 1, Step 3 and
§Contingencies) told the lane to reach the collector's `statvfs` path — "expose
`free_bytes_at` in `tinytop-collectors`". The first T1 lane (hexe run 538, `ari-sol-deep`)
**escalated instead of guessing**, and it was right:

- `tinytop-store` depends only on `serde`, `serde_json`, `sqlx`, `tinytop-types`. It has **no**
  path to `tinytop-collectors`, and the brief forbade the `Cargo.toml` edit that would add one.
- The collector's `statvfs` (`rustix::fs::statvfs`, ADR 0012) is Linux-only, feature-gated. The
  agent has real `macos.rs` and `windows.rs` targets; a Linux-only guard would leave the
  migration with *no* fail-closed path elsewhere.

The plan was a claim about the code, and the claim was wrong.

## Decision

- `tinytop-store` gets `sysinfo.workspace = true` — **already pinned** (`=0.39.5`,
  `default-features = false`, `features = ["disk", "system"]`) and compiled for
  `tinytop-collectors`, so no new crate enters the tree.
- New module `agent/crates/tinytop-store/src/disk.rs`:
  `pub fn free_bytes_at(path: &Path) -> io::Result<u64>` — `sysinfo::Disks::new_with_refreshed_list()`,
  choose the disk whose `mount_point()` is the **longest** prefix of the canonicalised path,
  return `available_space()`; no match → `io::ErrorKind::NotFound` naming the path.
  The prefix match and the 1.2× headroom arithmetic (`free >= db + db / 5`, integer) are pure
  `pub(crate)` functions with unit tests on both sides of each rule.
- The migration refuses (`StoreError::Migration`) when free bytes are **undeterminable** or
  below the headroom. Undeterminable is a refusal, not a skip — a silent skip would migrate
  without a verified pre-image.
- Task 9's `FreeBytesProvider` trait wraps this same function for the hourly disk check; the
  spec's "from the collector's filesystem snapshot" wording for T9 stays permissible (same
  numbers, same longest-prefix rule), but the store no longer *needs* the collector to protect
  its own file.

## Alternatives rejected

1. **`tinytop-store` → `tinytop-collectors` dependency.** No cycle (collectors does not depend on
   store), but it inverts the layering: the persistence crate would pull in `sysinfo`, `procfs`,
   `time` and every platform collector to read one number, and the number it wanted is
   Linux-only anyway.
2. **`rustix::fs::statvfs` directly in `tinytop-store`.** Also already pinned, and more precise
   (no prefix matching) — but unix-only. Windows would need `windows-sys`
   (`GetDiskFreeSpaceExW`): a genuinely new dependency, forbidden by the plan, and two code paths
   where one suffices.
3. **Inject a free-bytes provider through `SqliteHistoryStore::connect`.** Cleanest for tests,
   but changes `connect`'s signature, which `tinytop-agent` calls — outside Task 1's files, and
   it widens a lane that is already the migration. T9 adds the trait *around* the real function
   instead; T1's tests cover the arithmetic without a provider.
4. **Skip the check when it cannot be measured.** Violates spec §7's fail-closed rule.

## Consequences

- One pure, cross-platform mechanism for "free bytes at this path", reused by T1 (migration
  guard) and T9 (hourly disk pressure). Longest-prefix matching handles bind/overlay mounts the
  way `df` does; the unit test pins that behaviour.
- `sysinfo::Disks::new_with_refreshed_list()` enumerates every mount (ms on this box, 21
  filesystems). Acceptable for a one-time migration and an hourly task; never call it per sample.
- Lesson recorded for the planner: a brief's "the one allowed edit outside the Files list" must
  be verified against `Cargo.toml` dependency edges before dispatch, not assumed from a function
  name.
