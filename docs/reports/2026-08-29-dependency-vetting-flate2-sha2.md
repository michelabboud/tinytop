# Dependency Vetting: `flate2` and `sha2`

Date: 2026-08-29

## Decision

TinyTop pins `flate2 = "=1.1.10"` and `sha2 = "=0.11.0"` in workspace
dependencies and consumes both from `tinytop-store`. `flate2` 1.1.10 was
released on 2026-08-28. It fixes an infinite loop while writing a gzip
header/footer ([rust-lang/flate2-rs#547](https://github.com/rust-lang/flate2-rs/pull/547)),
which is on TinyTop's `GzEncoder` writer path, and rejects incomplete deflate
streams at EOF ([#556](https://github.com/rust-lang/flate2-rs/pull/556)), which
strengthens TinyTop's `GzDecoder` verification path.

`cargo tree -p tinytop-store -e features --offline` confirms that `flate2`'s
default `rust_backend` selects `miniz_oxide` 0.9.1 with its `simd` feature and
does not select `zlib`, `zlib-ng`, or another C backend. `sha2` 0.11 uses the
current RustCrypto `digest` API needed for incremental, streamed SHA-256. Its
finalized byte array is encoded explicitly as lowercase hexadecimal for the
`sha256sum` sidecar.

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

### `flate2` 1.1.10

- Upstream: the `rust-lang/flate2-rs` repository; maintainers shown by the
  ecosystem index include Josh Triplett, Alex Crichton, Sebastian Thiel, and
  the Rust project owner group.
- Cadence: 1.1.0 shipped in February 2025 and 1.1.10 on 2026-08-28, with
  regular patch releases between them. The project has published releases
  across the 1.x line since 2017.
- Adoption: the retrieved Lib.rs snapshot reports about 10.7 million downloads
  per month and use in more than 13,000 crates. Download counters include CI
  and automated traffic, so they are evidence of broad distribution rather
  than a unique-user count.
- Backend: the published 1.1.10 manifest defines the default `rust_backend`
  through `miniz_oxide` 0.9.1 (`simd`); all C-backed implementations are
  optional.

### Lock-only entry: `zlib-rs` 0.6.7

`zlib-rs` is an optional `flate2` dependency behind its `zlib-rs` backend
feature, which TinyTop does not enable. `flate2` 1.1.10's new default
`runtime_detection = ["zlib-rs?/std", "crc32fast?/std"]` uses Cargo's weak
`?/` feature syntax: it records `zlib-rs` in the lock without activating the
dependency or its `std` feature. The all-target inverse tree proves it is never
compiled on any target:

```text
$ cargo tree --manifest-path agent/Cargo.toml --target all -p tinytop-store -e features -i zlib-rs --offline
warning: nothing to print.

To find dependencies that require specific target platforms, try to use option `--target all` first, and then narrow your search scope accordingly.
```

Vetted regardless so this record remains useful if a future backend feature
activates it: `zlib-rs` is a pure-Rust zlib port maintained by the Trifecta Tech
Foundation, licensed Zlib, with MSRV 1.75 and no non-optional dependencies.
Crates.io 0.6.7 was published 2026-08-03 and reports about 119.7 million
downloads. The `trifectatechfoundation/zlib-rs` repository was pushed
2026-08-23 and is not archived.

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

## Outside-sandbox verification (Fable, 2026-08-29)

Fable verified crates.io `max_stable_version` as `flate2` 1.1.10 (published
2026-08-28) and `sha2` 0.11.0 (published 2026-03-25); the `sha2` 0.11.0-rc.x
line ended on 2026-02-02. A live `cargo audit` on the final lock reported zero
vulnerabilities and three pre-existing allowed warnings: `event-listener`
5.4.1 (`RUSTSEC-2026-0221`, unsound), yanked `chacha20` 0.10.1, and yanked
`spin` 0.9.8. None is on the `flate2` or `sha2` path.

The one transitive addition attributable to TinyTop's direct `sha2` 0.11.0
use is `const-oid` 0.10.2, selected through `digest`'s default `oid` feature.
SQLx already carried `sha2` 0.11.0 without that feature.

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

- <https://docs.rs/crate/flate2/1.1.10>
- <https://github.com/rust-lang/flate2-rs/releases>
- <https://raw.githubusercontent.com/rust-lang/flate2-rs/1.1.10/Cargo.toml>
- <https://crates.io/crates/zlib-rs/0.6.7>
- <https://github.com/trifectatechfoundation/zlib-rs>
- <https://docs.rs/crate/sha2/0.11.0>
- <https://github.com/RustCrypto/hashes/blob/master/sha2/CHANGELOG.md>
- <https://github.com/RustCrypto/hashes/blob/master/sha2/Cargo.toml>
- <https://lib.rs/crates/flate2>
- <https://lib.rs/crates/sha2>
