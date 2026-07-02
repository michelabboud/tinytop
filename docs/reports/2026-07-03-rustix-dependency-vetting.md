# Dependency vetting — `rustix` (Rust inode collection)

Date: 2026-07-03
Context: review finding M1 — the Rust collector needs `statvfs(2)` to populate
per-filesystem inode fields without shelling out to `df -i` (see ADR 0012).

## Decision

Add `rustix` as an optional dependency of `tinytop-collectors`, gated behind the
existing `linux-collector` feature, and use only `rustix::fs::statvfs`.

- **Version pinned:** `=1.1.4`, `features = ["fs"]`, `optional = true`, in the
  `[target.'cfg(target_os = "linux")'.dependencies]` block. Matches the repo's exact-pin
  convention (`=x.y.z`) in `agent/Cargo.toml`.

## Findings

- **Latest stable / freshness:** `1.1.4` is the current 1.x stable line and was already
  present in `agent/Cargo.lock` as a transitive dependency (via `tempfile`/toolchain
  deps), so adopting it directly adds no new distinct crate to the build — cargo unifies
  it with the existing resolution. No pre-release; no stale pin.
- **Security & health:** `rustix` is one of the most widely depended-upon crates in the
  ecosystem (transitive dependency of `mio`/`tokio`, `tempfile`, and much of the async
  stack), actively maintained by Dan Gohman, with no open RUSTSEC advisories against
  the 1.x line. Very high real-world adoption.
- **Why this over alternatives:**
  - `libc::statvfs` (also already in-tree): would require `unsafe` FFI in a crate that
    is deliberately `unsafe`-free. `rustix` wraps the same syscall behind a safe API.
  - `nix`: heavier, not in-tree, no advantage over `rustix` for a single syscall.
  - Shelling out to `df -i`: rejected in ADR 0012 (breaks the no-subprocess invariant,
    reintroduces stale-mount hangs).
- **Surface used:** exactly one function, `rustix::fs::statvfs(path) -> io::Result<StatVfs>`,
  reading `f_files` and `f_ffree`. The `fs` feature is the minimal feature set required.
- **Licensing:** `rustix` is `Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT`,
  compatible with the workspace's `Apache-2.0`.

## Verification

`cargo fmt --check` and `cargo test --workspace` pass with the new dependency (full
output in the task close-out). The `linux_collector_does_not_shell_out_for_host_metrics`
guard test still passes, confirming no subprocess path was introduced.
