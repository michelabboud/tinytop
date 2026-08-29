# Dependency Vetting: `flate2` and `sha2`

Date: 2026-08-29

## Decision

TinyTop pins `flate2 = "=1.1.9"` and `sha2 = "=0.11.0"` in workspace
dependencies and consumes both from `tinytop-store`. These are the current
stable releases verified from their published documentation and upstream
release history. The requested `cargo search flate2 --limit 1` and
`cargo search sha2 --limit 1` checks were attempted first, but this lane's
network could not resolve `crates.io`; no cached-version hint was treated as
authority.

`cargo tree -p tinytop-store -e features --offline` confirms that `flate2`'s
default `rust_backend` selects `miniz_oxide` and does not select `zlib`,
`zlib-ng`, or another C backend. `sha2` 0.11 uses the current RustCrypto
`digest` API needed for incremental, streamed SHA-256. Its finalized byte array
is encoded explicitly as lowercase hexadecimal for the `sha256sum` sidecar.

## Security advisories

The normal `cargo audit` invocation could not create its advisory-database lock
file below the read-only sandboxed Cargo home. The available advisory database
was therefore scanned with
`cargo audit --no-fetch --stale --no-yanked -d /home/michel/.cargo/advisory-db`;
`--no-yanked` avoids the separate crates.io-index lock that the containment
profile also makes read-only:

```text
      Loaded 1226 security advisories (from /home/michel/.cargo/advisory-db)
    Scanning Cargo.lock for vulnerabilities (202 crate dependencies)
Crate:     event-listener
Version:   5.4.1
Warning:   unsound
Title:     `event-listener` allows `!Send` tags to cross thread boundaries via `StackSlot`
Date:      2026-07-13
ID:        RUSTSEC-2026-0221
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0221

warning: 1 allowed warning found
```

The scan exits successfully with one allowed, pre-existing transitive
`event-listener` warning. It reports no advisory for `flate2`, `miniz_oxide`,
`sha2`, or their newly selected dependency path. Because the advisory database
could not be refreshed in this containment profile, the result is explicitly a
stale-database scan and must not be represented as a live RustSec refresh.

## Maintenance and adoption

### `flate2` 1.1.9

- Upstream: the `rust-lang/flate2-rs` repository; maintainers shown by the
  ecosystem index include Josh Triplett, Alex Crichton, Sebastian Thiel, and
  the Rust project owner group.
- Cadence: 1.1.0 shipped in February 2025 and 1.1.9 in February 2026, with
  regular patch releases between them. The project has published releases
  across the 1.x line since 2017.
- Adoption: the retrieved Lib.rs snapshot reports about 10.7 million downloads
  per month and use in more than 13,000 crates. Download counters include CI
  and automated traffic, so they are evidence of broad distribution rather
  than a unique-user count.
- Backend: the published 1.1.9 manifest defines `default = ["rust_backend"]`
  and routes that feature to `miniz_oxide`; all C-backed implementations are
  optional.

### `sha2` 0.11.0

- Upstream: the RustCrypto `hashes` repository; owners shown by the ecosystem
  index are Tony Arcieri, Artyom Pavlov, and RustCrypto.
- Cadence: the crate has stable releases back to 2016. Version 0.10.9 shipped
  in April 2025, the 0.11 line had public prereleases through 2025 and early
  2026, and 0.11.0 shipped in March 2026.
- Adoption: the retrieved Lib.rs snapshot reports about 13.4 million downloads
  per month and use in more than 24,000 crates, again understood as registry
  traffic rather than unique users.
- API: 0.11.0 is the latest stable RustCrypto line and works with the required
  incremental `Digest`/`Sha256` usage. Keeping 0.10.9 only because SQLx already
  uses it transitively would pin TinyTop's direct API to the older digest line
  without reducing the current lockfile to one SHA implementation.

## Alternatives considered

- `zstd`: ADR 0014 records only about a 20 percent size gain for this workload.
  The common Rust path introduces a C binding, loses universal `gzip` tooling,
  and does not satisfy the specified `.csv.gz` artifact contract.
- `xz`: stronger compression is slower and less universally convenient for
  monthly operator inspection; it also does not satisfy the gzip contract.
- `ring` or OpenSSL for SHA-256: both are substantially heavier dependency and
  build surfaces for one streamed digest; OpenSSL additionally adds a native
  library boundary.
- Hand-rolled SHA-256: rejected because cryptographic primitives should not be
  reimplemented locally when the maintained RustCrypto implementation supplies
  the exact incremental API.

`flate2` wins because it implements the required gzip format and streaming
encoder/decoder with a verified pure-Rust default. `sha2` wins because it is the
focused, widely adopted RustCrypto implementation for streamed SHA-256 with no
native dependency.

## Sources

- <https://docs.rs/crate/flate2/1.1.9>
- <https://github.com/rust-lang/flate2-rs/releases>
- <https://raw.githubusercontent.com/rust-lang/flate2-rs/1.1.9/Cargo.toml>
- <https://docs.rs/crate/sha2/0.11.0>
- <https://github.com/RustCrypto/hashes/blob/master/sha2/CHANGELOG.md>
- <https://github.com/RustCrypto/hashes/blob/master/sha2/Cargo.toml>
- <https://lib.rs/crates/flate2>
- <https://lib.rs/crates/sha2>
