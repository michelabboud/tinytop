# Dependency Vetting: OpenTelemetry OTLP Metrics

Date: 2026-08-29

## Decision

TinyTop will use the official OpenTelemetry Rust crates on the current 0.32
minor line, pinned exactly because the crates remain pre-1.0:

- `opentelemetry = "=0.32.0"`, with default features disabled and `metrics`
  enabled;
- `opentelemetry_sdk = "=0.32.1"`, with default features disabled and
  `metrics` enabled;
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
and crate manifests. This is a containment limitation, not a claim that a live
registry query succeeded.

## MSRV and feature verification

The downloaded crate manifests were unpacked into the lane's temporary Cargo
home and read directly. They report:

| Crate | Version | `rust-version` | TinyTop 1.95 compatible |
| --- | ---: | ---: | --- |
| `opentelemetry` | 0.32.0 | 1.75.0 | yes |
| `opentelemetry_sdk` | 0.32.1 | 1.75.0 | yes |
| `opentelemetry-otlp` | 0.32.0 | 1.75.0 | yes |
| `opentelemetry-proto` | 0.32.0 | 1.75.0 | yes |

`opentelemetry-otlp` 0.32 declares `reqwest-client` for the asynchronous
Reqwest client and `reqwest-rustls` for its Rustls TLS path. Its default uses
`reqwest-blocking-client`; disabling defaults is therefore required to keep
exports inside TinyTop's Tokio task. `http-proto` selects protobuf messages and
the metrics signal. The test-only protocol crate is needed because the
exporter does not publicly re-export the generated request type.

## Security advisories

Before the dependency edit, the available local RustSec database was scanned:

```text
      Loaded 1226 security advisories (from /home/michel/.cargo/advisory-db)
    Scanning agent/Cargo.lock for vulnerabilities (203 crate dependencies)
Crate:     event-listener
Version:   5.4.1
Warning:   unsound
Title:     `event-listener` allows `!Send` tags to cross thread boundaries via `StackSlot`
Date:      2026-07-13
ID:        RUSTSEC-2026-0221
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0221

warning: 1 allowed warning found
```

The final resolved lock is scanned again below. `--no-fetch --stale
--no-yanked` is required because the lane cannot update or lock the read-only
Cargo advisory/index state. Any vulnerability on the new OTel subtree is a
stop condition.

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
Rustls transport while allowing all unrelated default signals and blocking
clients to remain disabled.

## Measured dependency and binary cost

Before the dependency edit:

- `agent/Cargo.lock`: 203 `[[package]]` entries.
- `agent/target/release/tinytop-agent`: 10,614,800 bytes after a cleanly
  completed `cargo build --manifest-path agent/Cargo.toml --release --offline`.

The final lock count, package delta, release binary size, size delta, and final
audit transcript are recorded after Cargo resolves the vetted pins. They cannot
exist before the manifest edit that this report gates.

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
