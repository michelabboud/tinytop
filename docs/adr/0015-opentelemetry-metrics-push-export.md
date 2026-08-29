# ADR 0015 — Push-only OpenTelemetry metrics export from the Rust daemon

**Status:** Accepted (2026-08-29; T11, 0.5.0) — Michel's go for the ladder A→Z (2026-08-28) covers Phase 4; dispatched as lane T11 from `v0.4.1`.

## Context

Michel: *"we should also support emitting metrics to OpenTelemetry, but we do not read from OpenTelemetry, so previous configuration is still valid and the same."* tinytop already holds every value an OTLP consumer would want, sampled every 1.5 s.

## Decision

- The Rust daemon can **push** OTLP metrics over **HTTP/protobuf** to a configured endpoint at a configured interval (`otel` settings block; **off by default**; endpoint default `http://127.0.0.1:4318/v1/metrics`; interval default 60 s). Values pushed are the latest collected snapshot at export time — no extra collection, no re-sampling.
- Metric names follow OpenTelemetry semantic conventions where one exists (`system.cpu.utilization`, `system.memory.utilization`, `system.memory.usage`, `system.paging.utilization`, `system.cpu.load_average.{1m,5m,15m}`, `system.filesystem.{utilization,usage}`); product-specific values use the `tinytop.` prefix (`tinytop.load.percent`, `tinytop.pressure.{some,full}`). Resource: `service.name`, `service.version`, `host.name`, plus user attributes.
- **Secrets never enter settings.** Request headers (auth) come from the environment variable named by `otel.headersEnvVar` (OTLP `k=v,k=v` syntax), so the exportable settings document (ADR 0016) never carries a token.
- Crates: `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` at one identical version (0.32.x at planning time; the implementing lane confirms the latest stable on crates.io, checks advisories with `cargo audit`, and pins exact versions with a report under `docs/reports/`). HTTP/protobuf only — no gRPC/`tonic` dependency weight.
- Export runs in its own task; a failure increments a counter surfaced in `/api/history/coverage` and logs at `warn` (rate-limited), and can never delay or block collection or persistence.
- **No read path.** tinytop never scrapes, receives, or reads OTLP; nothing in the existing configuration changes meaning.

## Alternatives rejected

- **Prometheus `/metrics` scrape endpoint.** A pull model; would add an unauthenticated listener surface and a second metrics vocabulary. May be added later behind its own ADR.
- **gRPC OTLP.** Pulls in `tonic`/`prost`/`hyper` at full weight for no functional gain on a local dashboard daemon.
- **Bun-runtime parity for OTel.** Like retention and archiving, this is daemon-only; documented in the two-runtime section.

## Consequences

- One optional feature with a real dependency footprint; the crates are pre-1.0 and must be pinned exactly and upgraded deliberately.
- Operators get the machine's metrics into any OTLP collector without a sidecar; tinytop's own storage and UI are unaffected when it is off.
