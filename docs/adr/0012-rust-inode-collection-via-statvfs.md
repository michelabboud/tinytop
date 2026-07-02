# 0012 - Rust inode collection via statvfs, not a `df -i` subprocess

## Status

Accepted.

## Context

The Bun collector populates per-filesystem inode fields (`inodeTotal`, `inodeUsed`,
`inodeUsedPercent`) by shelling out to `df -Pi -T` and parsing the output. The Rust
collector hard-coded `df_inodes_text: String::new()`, so those three fields were
**permanently `null`** in the Rust runtime while the Bun runtime filled them. That
is a two-runtime `SystemSnapshot` contract drift (review finding M1): the same
product served different JSON depending on which runtime was active.

The obvious fix suggested in review was to mirror Bun and shell out to `df -Pi -T`
from Rust (with a timeout). But the Rust collector has a standing invariant — stated
in `ARCHITECTURE.md`, in `CLAUDE.md`, and enforced by the
`linux_collector_does_not_shell_out_for_host_metrics` guard test — that it reads
host metrics through crates (`procfs`, `sysinfo`), never external commands. ADR 0005
made the single Rust daemon the default runtime precisely to avoid the fragility of
the Bun split/subprocess model. Shelling out to `df` would also reintroduce the
exact stale-mount hang that review finding C1 is fixing on the Bun side, forcing us
to add subprocess-timeout machinery to a crate that is otherwise subprocess-free.

`sysinfo` exposes disk block usage but not inode counts, which is why the field was
left empty in the first place.

## Decision

Collect inode data in Rust with the `statvfs(2)` syscall via the `rustix::fs::statvfs`
safe wrapper, iterating the **same** `Disks` list already used for block usage so the
mount strings match exactly. `f_files` is the total inode count and `f_ffree` the free
count; used and used-percent are derived. The results are formatted into
`df -Pi -T`-compatible text and fed through the existing `parse_inodes` / `merge_filesystems`
path unchanged, so only the data *source* changes, not the parse/merge pipeline.

A mount that cannot be statted (permission denied, disappeared, unresponsive network
filesystem) is omitted, leaving its inode fields `null` — identical to the previous
empty-string fallback.

`rustix` is added as an optional dependency gated behind the existing
`linux-collector` feature (pinned `=1.1.4`, `features = ["fs"]`); it was already a
transitive dependency of the workspace. See
`docs/reports/2026-07-03-rustix-dependency-vetting.md`.

## Alternatives rejected

- **Shell out to `df -Pi -T` with a timeout (the review's suggestion).** Rejected: it
  breaks the no-subprocess invariant and its guard test, would need a new
  subprocess-timeout mechanism in a sync, subprocess-free crate, and reintroduces the
  stale-mount hang C1 is eliminating. It matches Bun's *mechanism* where we only need
  to match its *output contract*.
- **Raw `libc::statvfs` FFI.** `libc` is also in the tree, but the collector crate
  deliberately contains no `unsafe`; `rustix` provides the same syscall behind a safe,
  well-audited wrapper.
- **Leave inode fields null in Rust and drop them from the Bun contract.** Rejected:
  it removes a real, useful signal (inode exhaustion is a distinct failure mode from
  block exhaustion) rather than achieving parity.

## Consequences

- The two runtimes now agree on the inode fields; the `SystemSnapshot` contract no
  longer drifts by runtime.
- The no-subprocess invariant and its guard test remain intact — `statvfs` is a
  syscall, not a spawned process.
- `statvfs`, like `sysinfo`'s existing per-disk `total_space`/`available_space` calls,
  can in principle block on an unresponsive network mount. This adds **no new** hang
  surface: `sysinfo` already stats every listed mount for block usage on the same
  code path. If that ever proves a problem, it must be solved once for both callers
  (e.g. mount-level filtering), not by special-casing inodes.
- Inode `used-percent` is computed by the collector (rounded to 0.1) rather than
  parsed from `df`'s integer percent, so Rust and Bun may differ by a rounding step —
  acceptable, since the runtimes already report independently-sampled live values.
