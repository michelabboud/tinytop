# Dependency Vetting: OpenTelemetry OTLP Metrics

Date: 2026-08-29

## Decision

TinyTop will use the official OpenTelemetry Rust crates on the current 0.32
minor line, pinned exactly because the crates remain pre-1.0:

- `opentelemetry = "=0.32.0"`, with default features disabled and `metrics`
  enabled;
- `opentelemetry_sdk = "=0.32.1"`, with default features disabled and
  `metrics` plus `experimental_metrics_custom_reader` enabled;
- `opentelemetry-otlp = "=0.32.0"`, with default features disabled and
  `metrics`, `http-proto`, `reqwest-client`, and `reqwest-rustls` enabled;
- `opentelemetry-proto = "=0.32.0"` is a test-only dependency with
  `gen-tonic-messages` and `metrics`; and
- `prost = "=0.14.3"` is a test-only dependency at the version selected by
  the exporter subtree.

The patch-level difference for `opentelemetry_sdk` is intentional: 0.32.1 is
the latest stable SDK patch while the API, OTLP exporter, and protocol crates
remain at 0.32.0. All stay on the compatible 0.32 line.

The normal live `cargo search <crate> --limit 1` checks were attempted first
and failed because this lane cannot resolve `crates.io`. The versions above
were then verified with `cargo info <crate>@<version> --offline` against the
locally cached crates and cross-checked against the official upstream release
and crate manifests. This verification included the test-only
`cargo info prost@0.14.3 --offline` dependency check. This is a containment
limitation, not a claim that a live registry query succeeded.

## MSRV and feature verification

The downloaded crate manifests were unpacked into the lane's temporary Cargo
home and read directly. They report:

| Crate | Version | `rust-version` | TinyTop 1.95 compatible |
| --- | ---: | ---: | --- |
| `opentelemetry` | 0.32.0 | 1.75.0 | yes |
| `opentelemetry_sdk` | 0.32.1 | 1.75.0 | yes |
| `opentelemetry-otlp` | 0.32.0 | 1.75.0 | yes |
| `opentelemetry-proto` | 0.32.0 | 1.75.0 | yes |
| `prost` | 0.14.3 | 1.82 | yes |

`opentelemetry-otlp` 0.32 declares `reqwest-client` for the asynchronous
Reqwest client and `reqwest-rustls` for its Rustls TLS path. Its default uses
`reqwest-blocking-client`; disabling defaults is therefore required to keep
exports inside TinyTop's Tokio task. `http-proto` selects protobuf messages and
enables both the `trace` and `metrics` features (`Cargo.toml:88-95`); the trace
code is compiled by that feature closure but TinyTop never invokes it. The
`logs` feature remains off. The test-only protocol crate is needed because the
exporter does not publicly re-export the generated request type.

In SDK 0.32, `ManualReader`, the `MetricReader` trait, and the metrics
`Pipeline` needed by TinyTop's caller-driven export loop are exposed only by
the `experimental_metrics_custom_reader` feature. That gate is deliberately
accepted: `PeriodicReader` owns a standard thread and uses
`futures_executor::block_on`, which cannot host the asynchronous Reqwest
exporter inside TinyTop's Tokio daemon and does not expose each export result
to TinyTop's failure counter. The exact SDK pin contains this pre-1.0,
experimental surface. Every dependency upgrade must deliberately re-verify
the feature gate, custom reader wrapper, temporality, collection, and exporter
error path before changing the pin.

## Security advisories

After resolving the vetted pins, the available local RustSec database was
scanned with:

```text
cargo audit --no-fetch --stale --no-yanked -d /home/michel/.cargo/advisory-db
```

```text
      Loaded 1226 security advisories (from /home/michel/.cargo/advisory-db)
    Scanning Cargo.lock for vulnerabilities (296 crate dependencies)
Crate:     event-listener
Version:   5.4.1
Warning:   unsound
Title:     `event-listener` allows `!Send` tags to cross thread boundaries via `StackSlot`
Date:      2026-07-13
ID:        RUSTSEC-2026-0221
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0221

warning: 1 allowed warning found
```

The command exited successfully with no vulnerabilities and one allowed,
pre-existing warning (`event-listener` 5.4.1, RUSTSEC-2026-0221). No advisory
touches the new OpenTelemetry subtree. `--no-fetch --stale --no-yanked` is
required because the lane cannot update or lock the read-only Cargo
advisory/index state.

## Upstream health and stability

The official `open-telemetry/opentelemetry-rust` repository is active, with
0.30.0 released in May 2025, 0.31.0 in September 2025, and 0.32.0 in May 2026.
It has five listed maintainers from multiple organizations, roughly 2,600
GitHub stars, and regular issue and release activity.

The project describes the metrics API and SDK as stable while the metrics OTLP
exporter remains release-candidate quality. The repository remains pre-1.0 and
has an open OTLP-stability milestone with ten outstanding issues. That status
supports exact pins and deliberate upgrades rather than a loose minor range.

The issue review found active work touching metrics and OTLP HTTP. In
particular, issue 3520 documented a Reqwest 0.13 incompatibility in the separate
`reqwest-rustls-webpki-roots` feature and was closed with a fix. TinyTop selects
`reqwest-rustls`, not the affected WebPKI-roots feature. A 0.31 HTTP/protobuf
version-parsing report was also closed before 0.32. The exporter is therefore
healthy enough for an opt-in, failure-isolated daemon task, while its RC status
remains an explicit upgrade risk.

## Alternatives considered

- Prometheus pull was rejected by ADR 0015: it introduces a scrape/listener
  surface and a second metrics vocabulary instead of the requested push path.
- OTLP over gRPC was rejected because `tonic` and the full gRPC transport add
  build and binary weight without a functional benefit for the local HTTP
  collector endpoint.
- Hand-rolled OTLP over `prost` plus Reqwest was rejected because it would
  reimplement the SDK's aggregation, resource, instrument, and data-point
  semantics. The official SDK already supplies those contracts.

The official crates win because they implement OTLP/HTTP protobuf with the
OpenTelemetry metrics data model, resource handling, and an asynchronous
Rustls transport. The selected `http-proto` feature compiles trace alongside
metrics, but TinyTop never invokes trace; logs stays off, and the blocking
client remains disabled.

## Measured dependency and binary cost

Before the dependency edit:

- `agent/Cargo.lock`: 203 `[[package]]` entries.
- `agent/target/release/tinytop-agent`: 10,614,800 bytes after a cleanly
  completed `cargo build --manifest-path agent/Cargo.toml --release --offline`.

After dependency resolution:

- `agent/Cargo.lock`: 296 `[[package]]` entries, an increase of 93 packages.
- `cargo build --manifest-path agent/Cargo.toml --release --offline` completed
  successfully in 10.28 seconds on the final incremental rebuild.
- `agent/target/release/tinytop-agent`: 17,785,432 bytes.
- Release binary size delta: +7,170,632 bytes, approximately +67.55% from the
  10,614,800-byte baseline.

These are measured release-build values; no size was inferred from a debug
build or dependency metadata.

### Build prerequisites and deferred TLS option

`aws-lc-sys` builds native code, so local Rust daemon builds require a C
compiler on Linux, WSL, macOS, and Windows. Its builder tries `cc` first when
pregenerated bindings are available for the target; CMake is used only when
explicitly selected (`AWS_LC_SYS_CMAKE_BUILDER=1`), for FIPS/no-assembly/sanitizer
builds, for targets without pregenerated bindings, or after the `cc` builder
fails. CMake is harmless to install. On Debian/Ubuntu use `build-essential`
and optionally `cmake`; macOS's Xcode Command Line Tools provide the compiler
(Homebrew's `cmake` is optional); Windows needs Visual Studio Build Tools with
the C++ workload, while CMake is optional for normal builds and required only
on the fallback paths above. On Linux with `cc` absent, the real first `cc`
1.x failure line is `error occurred in cc-rs: failed to find tool "cc": No such file or directory (os error 2)`. The lockfile also records
target-conditional or otherwise unreferenced packages (`quinn`, `jni`,
`wasm-bindgen`, `schannel`, and `security-framework`); they are not compiled
in the Linux build. The Linux-target normal dependency tree contained none of
those packages, and `cargo tree -i quinn --offline` reported `warning: nothing
to print`, confirming that `quinn` has no Linux reverse dependency in this
workspace.

A ring-only Rustls provider is not reachable through the
`opentelemetry-otlp` 0.32 feature set. Reaching that configuration would
require a direct `reqwest`/`rustls` client passed through `with_http_client`,
which is deferred; the orchestrator will list it in the `PROGRESS.md` Backlog.

## Sources

- <https://github.com/open-telemetry/opentelemetry-rust/releases>
- <https://github.com/open-telemetry/opentelemetry-rust>
- <https://github.com/open-telemetry/opentelemetry-rust/blob/main/docs/release_0.32.md>
- <https://github.com/open-telemetry/opentelemetry-rust/issues>
- <https://github.com/open-telemetry/opentelemetry-rust/milestones>
- <https://github.com/open-telemetry/opentelemetry-rust/issues/3520>
- <https://github.com/open-telemetry/opentelemetry-rust/issues/3354>
- <https://docs.rs/opentelemetry/0.32.0>
- <https://docs.rs/opentelemetry_sdk/0.32.1>
- <https://docs.rs/opentelemetry-otlp/0.32.0>
- <https://docs.rs/opentelemetry-proto/0.32.0>
- <https://docs.rs/prost/0.14.3>
